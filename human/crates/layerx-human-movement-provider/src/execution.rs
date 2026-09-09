use layerx_human_service::custody::{
    EvmAcknowledgement, EvmAction, EvmExternalSignature, EvmTransaction, KeyClass, KmsProvider,
    PrincipalKeyBinding, ProviderKeyReference, RemoteKmsProvider,
};
use layerx_human_service::journeys::MovementExecutionIdentity;
use layerx_human_service::server::movement_provider::PlanningRequest;
use layerx_paxeer_client::{raw_call, Json, TrackerConfig, TransactionHash};
use layerx_types::intent::EvmAddress;
use sha3::{Digest, Keccak256};

use crate::config::{hex, hex_string};
use crate::Error;

pub(crate) struct Execution<'a> {
    pub identity: &'a MovementExecutionIdentity,
    pub action_key: [u8; 32],
    pub target: EvmAddress,
    pub calldata: &'a [u8],
    pub signed_transaction: Option<&'a [u8]>,
}

pub(crate) fn pending_nonce(tracker: &TrackerConfig, wallet: EvmAddress) -> Result<u64, Error> {
    let params = [
        Json::Text(format!("0x{}", hex_string(&wallet.bytes()))),
        Json::Text("pending".to_owned()),
    ];
    let mut values = Vec::new();
    for endpoint in &tracker.endpoints {
        if let Ok(Json::Text(value)) = raw_call(endpoint, "eth_getTransactionCount", &params) {
            if let Some(value) = value
                .strip_prefix("0x")
                .and_then(|v| u64::from_str_radix(v, 16).ok())
            {
                values.push(value);
            }
        }
    }
    values
        .iter()
        .find(|value| {
            values.iter().filter(|other| other == value).count()
                >= tracker.minimum_endpoint_agreement
        })
        .copied()
        .ok_or(Error::Integrity)
}

pub(crate) fn prepare(
    plan: &PlanningRequest,
    execution: &Execution<'_>,
    nonce: u64,
) -> Result<EvmTransaction, Error> {
    validate_identity(plan, execution)?;
    let context = &plan.context;
    if execution.calldata.len() < 4
        || execution.calldata.len() > 524_288
        || execution.target.bytes() == [0; 20]
        || context.evm_gas_limit < 21_000
        || context.evm_max_fee_per_gas == 0
        || context.evm_max_priority_fee_per_gas > context.evm_max_fee_per_gas
    {
        return Err(Error::Integrity);
    }
    Ok(EvmTransaction {
        chain_id: context.paxeer_chain_id,
        nonce,
        max_priority_fee_per_gas: context.evm_max_priority_fee_per_gas,
        max_fee_per_gas: context.evm_max_fee_per_gas,
        gas_limit: context.evm_gas_limit,
        to: execution.target.bytes(),
        value: [0; 32],
        calldata: execution.calldata.to_vec(),
    })
}

fn validate_identity(plan: &PlanningRequest, execution: &Execution<'_>) -> Result<(), Error> {
    let identity = execution.identity;
    if identity.plan_id != plan.idempotency_key
        || identity.principal != plan.principal
        || identity.tenant != plan.tenant
        || identity.wallet != plan.context.wallet
        || identity.account
            != layerx_paxeer_client::account_address_for_protocol(
                &plan.context.account,
                plan.context.protocol_version,
            )
            .map_err(|_| Error::Integrity)?
        || execution.action_key == [0; 32]
    {
        return Err(Error::Integrity);
    }
    Ok(())
}

fn kms_call<T: serde::Serialize>(
    kms: &RemoteKmsProvider,
    plan: &PlanningRequest,
    opcode: u8,
    value: &T,
) -> Result<EvmAction, Error> {
    let context = &plan.context;
    let binding = PrincipalKeyBinding::from_digest(
        context.custody_binding_digest,
        context.network.value(),
        KeyClass::HumanPrimary,
    )
    .map_err(|_| Error::Integrity)?;
    let reference = ProviderKeyReference::new(context.custody_provider_reference.clone())
        .map_err(|_| Error::Integrity)?;
    let payload = serde_json::to_vec(value).map_err(|_| Error::Integrity)?;
    let bytes = kms
        .evm_operation(opcode, &binding, &reference, &payload)
        .map_err(|_| Error::Integrity)?;
    serde_json::from_slice(&bytes).map_err(|_| Error::Integrity)
}

pub(crate) fn recover(
    kms: &RemoteKmsProvider,
    plan: &PlanningRequest,
    execution: &Execution<'_>,
) -> Result<EvmAction, Error> {
    validate_identity(plan, execution)?;
    let action = kms_call(kms, plan, 10, &execution.action_key)?;
    validate_action(plan, execution, &action)?;
    Ok(action)
}

fn validate_action(
    plan: &PlanningRequest,
    execution: &Execution<'_>,
    action: &EvmAction,
) -> Result<(), Error> {
    let auth = &action.authorization;
    let expected = prepare(plan, execution, auth.transaction.nonce)?;
    if auth.plan_id != execution.identity.plan_id
        || auth.action_key != execution.action_key
        || auth.principal != plan.principal.as_str()
        || auth.tenant != plan.tenant.as_str()
        || auth.wallet != plan.context.wallet.bytes()
        || auth.binding_digest != plan.context.custody_binding_digest
        || auth.not_before != plan.context.not_before
        || auth.not_after != plan.context.not_after
        || auth.transaction != expected
    {
        return Err(Error::Integrity);
    }
    if let Some(raw) = execution.signed_transaction {
        if raw != action.raw_transaction {
            return Err(Error::Conflict);
        }
    }
    if let Some(hash) = action.transaction_hash {
        if action.raw_transaction.is_empty()
            || hash != <[u8; 32]>::from(Keccak256::digest(&action.raw_transaction))
        {
            return Err(Error::Integrity);
        }
    } else if !action.raw_transaction.is_empty() || action.acknowledged {
        return Err(Error::Integrity);
    }
    Ok(())
}

pub(crate) fn submit(
    kms: &RemoteKmsProvider,
    tracker: &TrackerConfig,
    plan: &PlanningRequest,
    execution: &Execution<'_>,
) -> Result<Option<TransactionHash>, Error> {
    let mut action = recover(kms, plan, execution)?;
    if action.raw_transaction.is_empty() {
        action = kms_call(kms, plan, 8, &execution.action_key)?;
        validate_action(plan, execution, &action)?;
    }
    let hash = action.transaction_hash.ok_or(Error::Integrity)?;
    if !action.acknowledged && !observed(tracker, hash) {
        let mut acknowledged = false;
        for endpoint in &tracker.endpoints {
            if let Ok(Json::Text(value)) = raw_call(
                endpoint,
                "eth_sendRawTransaction",
                &[Json::Text(format!(
                    "0x{}",
                    hex_string(&action.raw_transaction)
                ))],
            ) {
                if hex::<32>(&value)? != hash {
                    return Err(Error::Integrity);
                }
                acknowledged = true;
                break;
            }
        }
        if !acknowledged {
            return Ok(None);
        }
    }
    let acknowledgement = EvmAcknowledgement {
        action_key: execution.action_key,
        transaction_hash: hash,
    };
    let recorded = kms_call(kms, plan, 9, &acknowledgement)?;
    validate_action(plan, execution, &recorded)?;
    if !recorded.acknowledged {
        return Err(Error::Integrity);
    }
    Ok(Some(TransactionHash::new(hash)))
}

pub(crate) fn lookup(
    kms: &RemoteKmsProvider,
    tracker: &TrackerConfig,
    plan: &PlanningRequest,
    execution: &Execution<'_>,
) -> Result<Option<TransactionHash>, Error> {
    let action = recover(kms, plan, execution)?;
    let Some(hash) = action.transaction_hash else {
        return Ok(None);
    };
    if !action.acknowledged && !observed(tracker, hash) {
        return Ok(None);
    }
    let recorded = kms_call(
        kms,
        plan,
        9,
        &EvmAcknowledgement {
            action_key: execution.action_key,
            transaction_hash: hash,
        },
    )?;
    validate_action(plan, execution, &recorded)?;
    if !recorded.acknowledged {
        return Err(Error::Integrity);
    }
    Ok(Some(TransactionHash::new(hash)))
}

fn observed(tracker: &TrackerConfig, hash: [u8; 32]) -> bool {
    let mut agreement = 0;
    for endpoint in &tracker.endpoints {
        if let Ok(value) = raw_call(
            endpoint,
            "eth_getTransactionByHash",
            &[Json::Text(format!("0x{}", hex_string(&hash)))],
        ) {
            if value
                .member("hash")
                .and_then(Json::as_text)
                .and_then(|v| hex::<32>(v).ok())
                == Some(hash)
            {
                agreement += 1;
            }
        }
    }
    agreement >= tracker.minimum_endpoint_agreement
}

pub(crate) fn external_signature(
    kms: &RemoteKmsProvider,
    plan: &PlanningRequest,
    execution: &Execution<'_>,
    signature: &[u8],
) -> Result<Vec<u8>, Error> {
    if signature.len() != 65 {
        return Err(Error::Integrity);
    }
    recover(kms, plan, execution)?;
    let request = EvmExternalSignature {
        action_key: execution.action_key,
        signature: signature.to_vec(),
    };
    let signed = kms_call(kms, plan, 12, &request)?;
    validate_action(plan, execution, &signed)?;
    if signed.raw_transaction.is_empty() || signed.transaction_hash.is_none() {
        return Err(Error::Integrity);
    }
    Ok(signed.raw_transaction)
}

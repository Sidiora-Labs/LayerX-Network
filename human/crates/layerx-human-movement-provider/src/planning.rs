use layerx_human_service::custody::Operation;
use layerx_human_service::journeys::{MoveLegExecution, MovePlan, RouteResolver};
use layerx_human_service::notify::JourneyId;
use layerx_human_service::server::movement_provider::{AuthorizedMovePlan, PlanningRequest};
use sha2::{Digest, Sha256};

use crate::config::hex_string;
use crate::Error;
use layerx_human_service::journeys::{
    DepositAgentPlan, DepositPlan, SettlementConfig, WithdrawalAgentPlan, WithdrawalPlan,
};
use layerx_paxeer_client::DepositProofConfig;
use layerx_types::intent::{EvmAddress, WithdrawalId};

pub(crate) fn identity(request: &PlanningRequest, purpose: &[u8]) -> Result<[u8; 32], Error> {
    let mut digest = Sha256::new();
    digest.update(b"layerx-human-movement-provider/action/v2\0");
    for value in [
        request.principal.as_str().as_bytes(),
        request.tenant.as_str().as_bytes(),
        purpose,
    ] {
        digest.update(
            u64::try_from(value.len())
                .map_err(|_| Error::Capacity)?
                .to_be_bytes(),
        );
        digest.update(value);
    }
    digest.update(request.idempotency_key);
    Ok(digest.finalize().into())
}

pub(crate) fn deposit_plan(
    request: &PlanningRequest,
    vault: EvmAddress,
) -> Result<DepositPlan, Error> {
    if request.operation != "deposit.start" {
        return Err(Error::Integrity);
    }
    let context = &request.context;
    Ok(DepositPlan {
        journey_id: JourneyId::new(format!(
            "deposit-{}",
            hex_string(&identity(request, b"deposit")?)
        ))
        .map_err(|_| Error::Integrity)?,
        idempotency_key: request.idempotency_key,
        wallet: context.wallet,
        network: context.network,
        paxeer_chain_id: context.paxeer_chain_id,
        layerx_network: context.network,
        layerx_protocol_version: context.protocol_version,
        vault,
        asset: context.asset,
        amount: context.amount,
        recipient: context.account.clone(),
        reserve: context.reserve.clone(),
        currency: context.currency.clone(),
        agent: DepositAgentPlan {
            actor: context.actor.clone(),
            authority: context.authority.clone(),
            account_sequence: context.account_sequence,
            not_before: context.not_before,
            not_after: context.not_after,
            fee_limit: context.fee_limit,
            custody_key: context.custody_key.clone(),
        },
    })
}

pub(crate) fn withdrawal_plan(
    request: &PlanningRequest,
    settlement: SettlementConfig,
    reminder: u64,
) -> Result<WithdrawalPlan, Error> {
    if request.operation != "withdraw.start" {
        return Err(Error::Integrity);
    }
    let context = &request.context;
    let id = identity(request, b"withdrawal")?;
    Ok(WithdrawalPlan {
        journey_id: JourneyId::new(format!("withdraw-{}", hex_string(&id)))
            .map_err(|_| Error::Integrity)?,
        idempotency_key: request.idempotency_key,
        network: context.network,
        layerx_protocol_version: context.protocol_version,
        withdrawal_id: WithdrawalId::new(id),
        owner: context.account.clone(),
        withdrawals_account: context.withdrawals_account.clone(),
        payout_address: context.wallet,
        asset: context.asset,
        amount: context.amount,
        currency: context.currency.clone(),
        settlement,
        reminder_interval_seconds: reminder,
        agent: WithdrawalAgentPlan {
            actor: context.actor.clone(),
            authority: context.authority.clone(),
            account_sequence: context.account_sequence,
            not_before: context.not_before,
            not_after: context.not_after,
            fee_limit: context.fee_limit,
            custody_key: context.custody_key.clone(),
        },
    })
}

pub(crate) fn validate(
    request: &PlanningRequest,
    config: &DepositProofConfig,
) -> Result<(), Error> {
    let context = &request.context;
    if context.network.value() != config.layerx_network_id
        || context.protocol_version != config.layerx_protocol_version
        || context.paxeer_chain_id != config.paxeer_chain_id
        || context.not_before > request.now
        || context.not_after <= request.now
        || context.binding_receipt_digest == [0; 32]
        || context.identity_authority_evidence.is_empty()
        || context.balance_evidence.is_empty()
        || context.wallet.bytes() == [0; 20]
    {
        return Err(Error::Integrity);
    }
    Ok(())
}

pub(crate) fn move_plan(request: &PlanningRequest) -> Result<AuthorizedMovePlan, Error> {
    if request.operation != "move.quote" {
        return Err(Error::Integrity);
    }
    let context = &request.context;
    let route_request = context.route.as_ref().ok_or(Error::Integrity)?;
    if route_request.asset != context.asset || route_request.amount != context.amount {
        return Err(Error::Integrity);
    }
    let route = RouteResolver::resolve(route_request).map_err(|_| Error::Integrity)?;
    let plan_id = identity(request, b"move")?;
    let mut executions = Vec::with_capacity(route.legs().len());
    for index in 0..route.legs().len() {
        let index = u64::try_from(index).map_err(|_| Error::Capacity)?;
        let mut digest = Sha256::new();
        digest.update(b"layerx-human-movement-provider/leg/v2\0");
        digest.update(plan_id);
        digest.update(index.to_be_bytes());
        executions.push(
            MoveLegExecution::new(
                digest.finalize().into(),
                context.actor.clone(),
                context.authority.clone(),
                context
                    .account_sequence
                    .checked_add(index)
                    .ok_or(Error::Capacity)?,
                context.not_before,
                context.not_after,
                context.fee_limit,
            )
            .map_err(|_| Error::Integrity)?,
        );
    }
    let fee_estimate = context
        .fee_limit
        .checked_mul(u128::try_from(executions.len()).map_err(|_| Error::Capacity)?)
        .ok_or(Error::Capacity)?;
    let plan = MovePlan::new(
        JourneyId::new(format!("move-{}", hex_string(&plan_id))).map_err(|_| Error::Integrity)?,
        request.idempotency_key,
        context.custody_key.clone(),
        Operation::ProtocolMutation,
        route_request.clone(),
        executions,
        fee_estimate,
        context.currency.clone(),
        "After each step receives a verified receipt",
    )
    .map_err(|_| Error::Integrity)?;
    AuthorizedMovePlan::from_wire_parts(
        format!("quote-{}", hex_string(&plan_id)),
        context.not_after,
        context.not_after,
        plan,
    )
    .map_err(|_| Error::Integrity)
}

//! Native withdrawal lifecycle receipts and their original signed requests.

use super::{
    encode_unsigned, receipt_digest, verify_sequencer_signature, AuthorizedBatch, Evidence,
    ReceiptCheck, VerificationFailure, VerifiedReceipt,
};
use layerx_types::account::AccountId;
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry, Payload};
use layerx_wire::activity::{decode_signed, encode_signed, encode_unsigned as unsigned_activity};
use layerx_wire::hash::{
    account_id_for_protocol, activity_id, payload_hash, payload_hash_for, Domain,
};
use layerx_wire::receipt::ProtocolReceipt;
use sha2::{Digest as _, Sha256};

fn refused() -> VerificationFailure {
    VerificationFailure::at(ReceiptCheck::ReceiptShape)
}

/// Returns the exact declared native withdrawal activity registry.
///
/// # Errors
/// Refuses an unavailable canonical module declaration.
pub fn registry() -> Result<ModuleRegistry, VerificationFailure> {
    let kind = ActivityType::new(ModuleId::Asset, 9).map_err(|_| refused())?;
    let module = ModuleRegistration::new(ModuleId::Asset, &[kind]).map_err(|_| refused())?;
    ModuleRegistry::new(&[module]).map_err(|_| refused())
}

fn shape(receipt: &ProtocolReceipt) -> Result<(), VerificationFailure> {
    if receipt.protocol_version() != 3
        || receipt.module_id() != 1
        || receipt.module_version() != 1
        || receipt.operation() != 9
        || receipt.result_code() != 0
        || receipt.activity_id() == [0; 32]
        || receipt.parameter_version() == 0
        || receipt.amount() == 0
        || receipt.asset() == [0; 32]
        || receipt.from() == [0; 32]
        || receipt.to() == [0; 32]
        || receipt.from() == receipt.to()
        || receipt.debit_balance_before().checked_sub(receipt.amount())
            != Some(receipt.debit_balance_after())
        || receipt
            .credit_balance_before()
            .checked_add(receipt.amount())
            != Some(receipt.credit_balance_after())
        || receipt.authorization_hash() == [0; 32]
        || receipt.context_hash() == [0; 32]
        || receipt.transfer_set_root() == [0; 32]
        || receipt.program_outcome().is_some()
    {
        return Err(refused());
    }
    Ok(())
}

fn authenticate(
    bytes: &[u8],
    authorized: &AuthorizedBatch,
) -> Result<VerifiedReceipt, VerificationFailure> {
    let receipt = verify_sequencer_signature(bytes, authorized.sequencer_public_key())?;
    let protocol = receipt.protocol().ok_or_else(refused)?;
    shape(protocol)?;
    if protocol.batch_id() != authorized.batch_id()
        || authorized.asset() != protocol.asset()
        || protocol.previous_state_root() != authorized.previous_state_root()
        || protocol.resulting_state_root() != authorized.resulting_state_root()
    {
        return Err(refused());
    }
    let unsigned = encode_unsigned(&receipt).map_err(|_| refused())?;
    let digest = receipt_digest(&unsigned).map_err(|_| refused())?;
    Ok(VerifiedReceipt {
        receipt,
        canonical_bytes: bytes.to_vec(),
        evidence: Evidence::sequencer(digest),
    })
}

fn payload(body: &[u8]) -> Result<[u8; 108], VerificationFailure> {
    if body.len() != 254 || body[..2] != [0, 2] || body[118..130] != [0; 12] {
        return Err(refused());
    }
    let mut payload = [0; 108];
    payload[..32].copy_from_slice(&body[70..102]);
    payload[32..48].copy_from_slice(&body[102..118]);
    payload[48..68].copy_from_slice(&body[130..150]);
    payload[68..100].copy_from_slice(&body[150..182]);
    payload[100..].copy_from_slice(&body[246..]);
    if [2..6, 6..38, 38..70, 70..102, 102..118, 130..150, 150..182]
        .into_iter()
        .any(|range| body[range].iter().all(|byte| *byte == 0))
    {
        return Err(refused());
    }
    let kind = ActivityType::new(ModuleId::Asset, 9).map_err(|_| refused())?;
    let value = Payload::new(&registry()?, kind, &payload).map_err(|_| refused())?;
    if payload_hash_for(&value).map_err(|_| refused())? != body[182..214] {
        return Err(refused());
    }
    Ok(payload)
}

fn effects(receipt: &ProtocolReceipt) -> Result<&[u8], VerificationFailure> {
    let [transfer, event] = receipt.effects() else {
        return Err(refused());
    };
    if transfer.module_id() != 1
        || transfer.ordinal() != 0
        || transfer.kind() != 2
        || !transfer.monetary()
        || transfer.event_type() != 0
        || !transfer.body().is_empty()
        || event.module_id() != 1
        || event.ordinal() != 1
        || event.kind() != 3
        || event.monetary()
        || event.event_type() != 9
        || event.transfer_set_root() != [0; 32]
    {
        return Err(refused());
    }
    let body = event.body();
    let payload = payload(body)?;
    if body[6..38] != receipt.activity_id()
        || receipt.fee_charged()
            > u128::from(u64::from_be_bytes(
                payload[100..].try_into().map_err(|_| refused())?,
            ))
    {
        return Err(refused());
    }
    let destination = AccountId::parse("system:paxeer-withdrawals").map_err(|_| refused())?;
    let destination = account_id_for_protocol(&destination, 3).map_err(|_| refused())?;
    let mut leg = [0; 115];
    leg[1..33].copy_from_slice(&body[38..70]);
    leg[33..65].copy_from_slice(&destination);
    leg[65..97].copy_from_slice(&payload[..32]);
    leg[97..113].copy_from_slice(&payload[32..48]);
    leg[113..].copy_from_slice(&3_u16.to_be_bytes());
    let mut nullifier = Sha256::new();
    nullifier.update(b"LX:WITHDRAWAL:v1");
    nullifier.update(&body[2..118]);
    nullifier.update(&body[150..182]);
    let nullifier: [u8; 32] = nullifier.finalize().into();
    if receipt.asset() != body[70..102]
        || receipt.from() != body[38..70]
        || receipt.to() != destination
        || receipt.amount()
            != u128::from_be_bytes(payload[32..48].try_into().map_err(|_| refused())?)
        || receipt.context_hash() != nullifier
        || receipt.transfer_set_root() != transfer.transfer_set_root()
        || crate::merkle::leaf_hash(&leg).map_err(|_| refused())? != transfer.transfer_set_root()
    {
        return Err(refused());
    }
    Ok(body)
}

pub(super) fn verify_effects(
    bytes: &[u8],
    authorized: &AuthorizedBatch,
) -> Result<VerifiedReceipt, VerificationFailure> {
    let verified = authenticate(bytes, authorized)?;
    let receipt = verified.receipt().protocol().ok_or_else(refused)?;
    if receipt.result_code() != 0 {
        return Err(refused());
    }
    effects(receipt)?;
    Ok(verified)
}

/// Verifies a withdrawal outcome against the caller's original signed activity.
///
/// # Errors
/// Refuses wrong network, owner signature, activity, payload, fee, effects or batch roots.
pub fn verify(
    bytes: &[u8],
    authorized: &AuthorizedBatch,
    signed_activity: &[u8],
    network_id: u32,
) -> Result<VerifiedReceipt, VerificationFailure> {
    let activity = decode_signed(signed_activity, &registry()?).map_err(|_| refused())?;
    if activity.protocol_version() != 3
        || activity.network_id() != network_id
        || network_id == 0
        || encode_signed(&activity).map_err(|_| refused())? != signed_activity
        || activity.authority().len() != 32
        || activity.payload().len() != 108
        || payload_hash(&activity).map_err(|_| refused())? != activity.payload_hash()
    {
        return Err(refused());
    }
    let unsigned = unsigned_activity(&activity).map_err(|_| refused())?;
    let message =
        layerx_crypto::SignatureMessage::new(Domain::SignaturePreimage, 3, network_id, &unsigned)
            .map_err(|_| refused())?;
    layerx_crypto::ed25519::verify(
        &activity.authority().try_into().map_err(|_| refused())?,
        &activity
            .signature()
            .ok_or_else(refused)?
            .try_into()
            .map_err(|_| refused())?,
        message,
    )
    .map_err(|_| refused())?;
    let signing_digest = message.digest();
    let verified = authenticate(bytes, authorized)?;
    let receipt = verified.receipt().protocol().ok_or_else(refused)?;
    if activity_id(&activity).map_err(|_| refused())? != receipt.activity_id()
        || receipt.authorization_hash() != signing_digest
        || receipt.timestamp() < activity.timestamp_bound().not_before
        || receipt.timestamp() > activity.timestamp_bound().not_after
        || receipt.fee_charged() > activity.fee_limit()
        || activity.fee_limit()
            != u128::from(u64::from_be_bytes(
                activity.payload()[100..]
                    .try_into()
                    .map_err(|_| refused())?,
            ))
    {
        return Err(refused());
    }
    if receipt.result_code() == 0 {
        let body = effects(receipt)?;
        let actor = std::str::from_utf8(activity.actor_did()).map_err(|_| refused())?;
        let source = AccountId::parse(&format!("agent:{actor}:main")).map_err(|_| refused())?;
        if body[2..6] != network_id.to_be_bytes()
            || body[38..70] != account_id_for_protocol(&source, 3).map_err(|_| refused())?
            || payload(body)?.as_slice() != activity.payload()
            || body[214..246] != activity.idempotency_key()
        {
            return Err(refused());
        }
    } else if !receipt.effects().is_empty() {
        return Err(refused());
    }
    Ok(verified)
}

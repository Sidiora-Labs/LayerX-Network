use layerx_types::payload::{ActivityType, ModuleRegistration, ModuleRegistry};
use layerx_wire::activity::{decode_signed, encode_signed, encode_unsigned, Activity};
use layerx_wire::hash::{receipt_digest, Domain};
use layerx_wire::receipt::ProtocolReceipt;
use sha2::{Digest as _, Sha256};

use super::{
    verify_sequencer_signature, AuthorizedBatch, ReceiptCheck, VerificationFailure, VerifiedReceipt,
};
use crate::evidence::Evidence;

/// Independently retained signing request for a native owner module outcome.
#[derive(Clone, Copy, Debug)]
pub struct NativeOwnerOutcomeContext<'a> {
    pub canonical_activity: &'a [u8],
    pub actor: &'a [u8],
    pub action_key: [u8; 32],
    pub activity_type: ActivityType,
    pub owner_public_key: [u8; 32],
    pub network_id: u32,
}

/// Exact refusal without changing transfer-receipt verification semantics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeOwnerOutcomeFailure {
    ActivityDecode,
    ActivityBinding,
    ActivitySignature,
    PayloadHash,
    FeeLimit,
    Receipt(VerificationFailure),
}

impl From<VerificationFailure> for NativeOwnerOutcomeFailure {
    fn from(value: VerificationFailure) -> Self {
        Self::Receipt(value)
    }
}

fn digest(domain: Domain, bytes: &[u8]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(domain.tag());
    hash.update(bytes);
    hash.finalize().into()
}

fn activity(
    expected: &NativeOwnerOutcomeContext<'_>,
) -> Result<Activity, NativeOwnerOutcomeFailure> {
    use NativeOwnerOutcomeFailure::{
        ActivityBinding, ActivityDecode, ActivitySignature, PayloadHash,
    };
    if expected.network_id == 0
        || expected.actor.is_empty()
        || expected.action_key == [0; 32]
        || !(2..=7).contains(&(expected.activity_type.module() as u16))
    {
        return Err(ActivityBinding);
    }
    let registration =
        ModuleRegistration::new(expected.activity_type.module(), &[expected.activity_type])
            .map_err(|_| ActivityBinding)?;
    let registry = ModuleRegistry::new(&[registration]).map_err(|_| ActivityBinding)?;
    let value =
        decode_signed(expected.canonical_activity, &registry).map_err(|_| ActivityDecode)?;
    if encode_signed(&value).map_err(|_| ActivityDecode)? != expected.canonical_activity
        || value.protocol_version() != 3
        || value.network_id() != expected.network_id
        || value.actor_did() != expected.actor
        || value.activity_type() != expected.activity_type
        || value.idempotency_key() != expected.action_key
        || value.authority() != expected.owner_public_key
    {
        return Err(ActivityBinding);
    }
    if value.payload_hash() != digest(Domain::PayloadHash, value.payload()) {
        return Err(PayloadHash);
    }
    let signature: [u8; 64] = value
        .signature()
        .ok_or(ActivitySignature)?
        .try_into()
        .map_err(|_| ActivitySignature)?;
    let unsigned = encode_unsigned(&value).map_err(|_| ActivityDecode)?;
    layerx_crypto::ed25519::verify_digest(
        &expected.owner_public_key,
        &signature,
        &digest(Domain::SignaturePreimage, &unsigned),
    )
    .map_err(|_| ActivitySignature)?;
    Ok(value)
}

fn projection(protocol: &ProtocolReceipt) -> Result<(), VerificationFailure> {
    if protocol.operation() != 0
        || protocol.asset() != [0; 32]
        || protocol.amount() != 0
        || protocol.from() != [0; 32]
        || protocol.to() != [0; 32]
        || protocol.debit_sequence() != 0
        || protocol.debit_balance_before() != 0
        || protocol.debit_balance_after() != 0
        || protocol.credit_balance_before() != 0
        || protocol.credit_balance_after() != 0
        || protocol.transfer_set_root() != [0; 32]
        || protocol.authorization_hash() != [0; 32]
        || protocol.context_hash() != [0; 32]
        || protocol.program_outcome().is_some()
        || protocol.total_units().is_some()
        || protocol
            .effects()
            .iter()
            .any(layerx_wire::receipt::Effect::monetary)
    {
        return Err(VerificationFailure::at(ReceiptCheck::ReceiptShape));
    }
    Ok(())
}

/// Verifies an owner-signed native non-transfer module activity and its exact
/// sequencer-signed success or refusal. The operation is authenticated from the
/// original activity because this receipt shape has no ledger operation tag.
///
/// The returned evidence is sequencer-signed. Batch inclusion and checkpoint
/// finality still require their independent verification paths.
///
/// # Errors
/// Rejects altered activities, signing authority, operation or action bindings,
/// incompatible receipt projections, excess fees, and batch/root/signature mismatches.
pub fn verify_native_owner_outcome(
    receipt_bytes: &[u8],
    authorised: &AuthorizedBatch,
    expected: &NativeOwnerOutcomeContext<'_>,
) -> Result<VerifiedReceipt, NativeOwnerOutcomeFailure> {
    let activity = activity(expected)?;
    let receipt = verify_sequencer_signature(receipt_bytes, authorised.sequencer_public_key())?;
    let protocol = receipt
        .protocol()
        .ok_or_else(|| VerificationFailure::at(ReceiptCheck::ReceiptShape))?;
    if protocol.protocol_version() != activity.protocol_version() {
        return Err(VerificationFailure::at(ReceiptCheck::ProtocolVersion).into());
    }
    if protocol.module_id() != expected.activity_type.module() as u16
        || protocol.module_version() != 1
    {
        return Err(VerificationFailure::at(ReceiptCheck::Module).into());
    }
    if protocol.activity_id() != digest(Domain::ActivityId, expected.canonical_activity) {
        return Err(VerificationFailure::at(ReceiptCheck::ActivityId).into());
    }
    if protocol.global_sequence() == 0 || protocol.timestamp() == 0 {
        return Err(VerificationFailure::at(ReceiptCheck::ReceiptShape).into());
    }
    projection(protocol)?;
    if protocol.batch_id() != authorised.batch_id() {
        return Err(VerificationFailure::at(ReceiptCheck::BatchId).into());
    }
    if authorised.asset() != protocol.asset() {
        return Err(VerificationFailure::at(ReceiptCheck::Asset).into());
    }
    if protocol.previous_state_root() != authorised.previous_state_root() {
        return Err(VerificationFailure::at(ReceiptCheck::PreviousStateRoot).into());
    }
    if protocol.resulting_state_root() != authorised.resulting_state_root() {
        return Err(VerificationFailure::at(ReceiptCheck::ResultingStateRoot).into());
    }
    if protocol.fee_charged() > activity.fee_limit() {
        return Err(NativeOwnerOutcomeFailure::FeeLimit);
    }
    let window = activity.timestamp_bound();
    if protocol.result_code() == 0
        && !(window.not_before..=window.not_after).contains(&protocol.timestamp())
    {
        return Err(NativeOwnerOutcomeFailure::ActivityBinding);
    }
    let unsigned = layerx_wire::receipt::encode_unsigned(&receipt)
        .map_err(|_| VerificationFailure::at(ReceiptCheck::CanonicalEncoding))?;
    let digest = receipt_digest(&unsigned)
        .map_err(|_| VerificationFailure::at(ReceiptCheck::CanonicalEncoding))?;
    Ok(VerifiedReceipt {
        receipt,
        canonical_bytes: receipt_bytes.to_vec(),
        evidence: Evidence::sequencer(digest),
    })
}

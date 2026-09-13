//! Offline verification of sequencer-signed canonical receipts.

use layerx_crypto::ed25519;
use layerx_wire::hash::receipt_digest;
use layerx_wire::limits::protocol_version_uses_occupancy;
use layerx_wire::receipt::{decode, encode, encode_unsigned, Receipt};

use crate::evidence::Evidence;
use crate::level::achieved;

mod native_credit;

const PROGRAMS_MODULE_ID: u32 = 9;
const PROGRAMS_STATE_OPERATION: u16 = 0;
const PROGRAMS_CALL_OPERATION: u16 = 3;

const fn supported_protocol_version(version: u16) -> bool {
    protocol_version_uses_occupancy(version)
}

const fn supported_programs_module_version(version: u32) -> bool {
    matches!(version, 1..=3)
}

const fn supports_program_account_state(version: u32) -> bool {
    matches!(version, 2 | 3)
}

const fn programs_version_for_protocol(protocol: u16, module: u32, account_state: bool) -> bool {
    if protocol == layerx_wire::limits::STATE_COMMITMENT_PROTOCOL_VERSION {
        module == 4
    } else if account_state {
        supports_program_account_state(module)
    } else {
        supported_programs_module_version(module)
    }
}

const fn supported_program_guest_abi(version: u16) -> bool {
    matches!(version, 1 | 2)
}

/// Core-published batch facts needed to establish sequencer authority and the
/// state-root chain for one receipt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthorizedBatch {
    batch_id: [u8; 32],
    asset: [u8; 32],
    previous_state_root: [u8; 32],
    resulting_state_root: [u8; 32],
    sequencer_public_key: [u8; 32],
}

impl AuthorizedBatch {
    /// Creates the independently supplied batch evidence for one receipt.
    #[must_use]
    pub const fn new(
        batch_id: [u8; 32],
        asset: [u8; 32],
        previous_state_root: [u8; 32],
        resulting_state_root: [u8; 32],
        sequencer_public_key: [u8; 32],
    ) -> Self {
        Self {
            batch_id,
            asset,
            previous_state_root,
            resulting_state_root,
            sequencer_public_key,
        }
    }

    #[must_use]
    pub const fn batch_id(&self) -> [u8; 32] {
        self.batch_id
    }

    #[must_use]
    pub const fn asset(&self) -> [u8; 32] {
        self.asset
    }

    #[must_use]
    pub const fn previous_state_root(&self) -> [u8; 32] {
        self.previous_state_root
    }

    #[must_use]
    pub const fn resulting_state_root(&self) -> [u8; 32] {
        self.resulting_state_root
    }

    #[must_use]
    pub const fn sequencer_public_key(&self) -> [u8; 32] {
        self.sequencer_public_key
    }
}

/// The exact verification stage that rejected a receipt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReceiptCheck {
    /// Canonical decoding failed.
    Decode,
    /// Re-encoding did not reproduce the supplied bytes.
    CanonicalEncoding,
    /// The byte string was not a full signed protocol receipt.
    ReceiptShape,
    /// A required signature was absent.
    MissingSignature,
    /// The protocol version was not the supported version.
    ProtocolVersion,
    /// The receipt was not emitted by the required protocol module.
    Module,
    /// The core result was not successful.
    ResultCode,
    /// The operation tag was absent.
    Operation,
    /// The activity identifier was all zero.
    ActivityId,
    /// The batch identifier did not match the authorised batch.
    BatchId,
    /// The receipt asset did not match the independently supplied batch fact.
    Asset,
    /// The previous state root did not match the supplied chain predecessor.
    PreviousStateRoot,
    /// The resulting state root did not match the supplied chain successor.
    ResultingStateRoot,
    /// The debit balance did not decrease by exactly the amount.
    DebitBalance,
    /// The credit balance did not increase by exactly the amount.
    CreditBalance,
    /// The signature did not verify under the authorised sequencer key.
    SequencerSignature,
}

/// A typed receipt failure that never masquerades as a lower verified level.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VerificationFailure {
    /// Exact failed verification stage.
    pub check: ReceiptCheck,
}

impl VerificationFailure {
    const fn at(check: ReceiptCheck) -> Self {
        Self { check }
    }
}

/// A receipt whose canonical encoding, invariants, root chain, and authorised
/// sequencer signature all passed locally.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedReceipt {
    receipt: Receipt,
    canonical_bytes: Vec<u8>,
    evidence: Evidence,
}

/// Exact economic facts decoded from one canonical protocol receipt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProtocolReceiptFacts {
    result_code: i32,
    asset: [u8; 32],
    amount: u128,
    fee_charged: u128,
}

impl ProtocolReceiptFacts {
    /// Returns the exact protocol result.
    #[must_use]
    pub const fn result_code(self) -> i32 {
        self.result_code
    }

    /// Returns the receipt asset identifier.
    #[must_use]
    pub const fn asset(self) -> [u8; 32] {
        self.asset
    }

    /// Returns the exact executed amount.
    #[must_use]
    pub const fn amount(self) -> u128 {
        self.amount
    }

    /// Returns the exact fee charged by core.
    #[must_use]
    pub const fn fee_charged(self) -> u128 {
        self.fee_charged
    }
}

impl VerifiedReceipt {
    /// Borrows the decoded core receipt.
    #[must_use]
    pub const fn receipt(&self) -> &Receipt {
        &self.receipt
    }

    /// Borrows the exact core-produced bytes that were verified.
    #[must_use]
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical_bytes
    }

    /// Returns the exact level established by this verification routine.
    #[must_use]
    pub const fn level(&self) -> layerx_types::verify::VerificationLevel {
        achieved(&self.evidence)
    }

    /// Returns the receipt digest and achieved level as one immutable record.
    #[must_use]
    pub const fn evidence(&self) -> &Evidence {
        &self.evidence
    }
}

/// Verifies one full receipt without network, clock, database, or ambient
/// process state.
///
/// # Errors
///
/// Returns a failure naming the exact canonical, invariant, root-chain, or
/// signature check that failed. No partial verified value is returned.
pub fn verify(
    receipt_bytes: &[u8],
    authorised: &AuthorizedBatch,
) -> Result<VerifiedReceipt, VerificationFailure> {
    let verified = verify_outcome(receipt_bytes, authorised)?;
    if verified
        .receipt
        .protocol()
        .is_some_and(|receipt| receipt.result_code() != 0)
    {
        return Err(VerificationFailure::at(ReceiptCheck::ResultCode));
    }
    Ok(verified)
}

/// Verifies a full sequencer-signed receipt while preserving either its
/// successful or rejected protocol result exactly.
///
/// # Errors
///
/// Returns the exact canonical, invariant, root-chain, or signature check that
/// failed. No partial verified value is returned.
pub fn verify_outcome(
    receipt_bytes: &[u8],
    authorised: &AuthorizedBatch,
) -> Result<VerifiedReceipt, VerificationFailure> {
    let receipt =
        decode(receipt_bytes).map_err(|_| VerificationFailure::at(ReceiptCheck::Decode))?;
    let reproduced =
        encode(&receipt).map_err(|_| VerificationFailure::at(ReceiptCheck::CanonicalEncoding))?;
    if reproduced != receipt_bytes {
        return Err(VerificationFailure::at(ReceiptCheck::CanonicalEncoding));
    }
    let protocol = receipt
        .protocol()
        .ok_or_else(|| VerificationFailure::at(ReceiptCheck::ReceiptShape))?;
    if !supported_protocol_version(protocol.protocol_version()) {
        return Err(VerificationFailure::at(ReceiptCheck::ProtocolVersion));
    }
    if protocol.module_id() == 8 && protocol.operation() == 0 {
        return native_credit::verify(receipt_bytes, authorised);
    }
    if u32::from(protocol.module_id()) == PROGRAMS_MODULE_ID && protocol.operation() == 0 {
        return verify_program_state_outcome(receipt_bytes, authorised);
    }
    if u32::from(protocol.module_id()) == PROGRAMS_MODULE_ID
        && u16::from(protocol.operation()) == PROGRAMS_CALL_OPERATION
    {
        return verify_program_outcome(receipt_bytes, authorised);
    }
    if protocol.operation() == 0 {
        return Err(VerificationFailure::at(ReceiptCheck::Operation));
    }
    if protocol.activity_id() == [0; 32] {
        return Err(VerificationFailure::at(ReceiptCheck::ActivityId));
    }
    if protocol.batch_id() != authorised.batch_id {
        return Err(VerificationFailure::at(ReceiptCheck::BatchId));
    }
    if protocol.asset() != authorised.asset || protocol.asset() == [0; 32] {
        return Err(VerificationFailure::at(ReceiptCheck::Asset));
    }
    if protocol.previous_state_root() != authorised.previous_state_root {
        return Err(VerificationFailure::at(ReceiptCheck::PreviousStateRoot));
    }
    if protocol.resulting_state_root() != authorised.resulting_state_root {
        return Err(VerificationFailure::at(ReceiptCheck::ResultingStateRoot));
    }
    if protocol.result_code() == 0 {
        if protocol
            .debit_balance_before()
            .checked_sub(protocol.amount())
            != Some(protocol.debit_balance_after())
        {
            return Err(VerificationFailure::at(ReceiptCheck::DebitBalance));
        }
        if protocol
            .credit_balance_before()
            .checked_add(protocol.amount())
            != Some(protocol.credit_balance_after())
        {
            return Err(VerificationFailure::at(ReceiptCheck::CreditBalance));
        }
    }
    let signature = protocol
        .sequencer_signature()
        .ok_or_else(|| VerificationFailure::at(ReceiptCheck::MissingSignature))?;
    let unsigned = encode_unsigned(&receipt)
        .map_err(|_| VerificationFailure::at(ReceiptCheck::CanonicalEncoding))?;
    let digest = receipt_digest(&unsigned)
        .map_err(|_| VerificationFailure::at(ReceiptCheck::CanonicalEncoding))?;
    ed25519::verify_digest(&authorised.sequencer_public_key, &signature, &digest)
        .map_err(|_| VerificationFailure::at(ReceiptCheck::SequencerSignature))?;
    Ok(VerifiedReceipt {
        receipt,
        canonical_bytes: reproduced,
        evidence: Evidence::sequencer(digest),
    })
}

#[derive(Clone, Copy, Debug)]
pub struct MaintainedOutcomeEvidence<'a> {
    pub header: &'a [u8],
    pub header_signature: &'a [u8; 64],
    pub activity_proof: &'a crate::merkle::Proof,
    pub maintenance: &'a [u8],
    pub maintenance_proof: &'a crate::merkle::Proof,
    pub authorization: &'a crate::inclusion::SequencerAuthorization,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MaintainedOutcomeFailure {
    Inclusion(crate::inclusion::InclusionError),
    MaintenanceEncoding,
    SequenceRange,
    Receipt(ReceiptCheck),
}

/// Verifies a selected maintained outcome through the signed maintenance transition.
///
/// # Errors
/// Refuses invalid signatures, inclusion, identity, positions or transition roots.
pub fn verify_outcome_maintained(
    receipt_bytes: &[u8],
    authorised: &AuthorizedBatch,
    evidence: &MaintainedOutcomeEvidence<'_>,
) -> Result<VerifiedReceipt, MaintainedOutcomeFailure> {
    let activity_batch = maintained_activity_batch(receipt_bytes, authorised, evidence)?;
    verify_outcome(receipt_bytes, &activity_batch)
        .map_err(|failure| MaintainedOutcomeFailure::Receipt(failure.check))
}

/// Verifies a selected maintained Programs state outcome.
///
/// # Errors
/// Refuses invalid maintained evidence or any Programs state receipt invariant.
pub fn verify_program_state_maintained(
    receipt_bytes: &[u8],
    authorised: &AuthorizedBatch,
    evidence: &MaintainedOutcomeEvidence<'_>,
) -> Result<VerifiedReceipt, MaintainedOutcomeFailure> {
    let activity_batch = maintained_activity_batch(receipt_bytes, authorised, evidence)?;
    verify_program_state(receipt_bytes, &activity_batch)
        .map_err(|failure| MaintainedOutcomeFailure::Receipt(failure.check))
}

/// Authenticates maintained batch evidence and returns its activity transition facts.
///
/// # Errors
/// Refuses mismatched authorization, inclusion, identity, sequence or sealed roots.
pub fn authorized_maintained_activity_batch(
    receipt_bytes: &[u8],
    authorised: &AuthorizedBatch,
    evidence: &MaintainedOutcomeEvidence<'_>,
) -> Result<AuthorizedBatch, MaintainedOutcomeFailure> {
    maintained_activity_batch(receipt_bytes, authorised, evidence)
}

fn maintained_activity_batch(
    receipt_bytes: &[u8],
    authorised: &AuthorizedBatch,
    evidence: &MaintainedOutcomeEvidence<'_>,
) -> Result<AuthorizedBatch, MaintainedOutcomeFailure> {
    use crate::inclusion::verify_receipt;
    use MaintainedOutcomeFailure::{Receipt as Failure, SequenceRange};
    let included = verify_receipt(
        receipt_bytes,
        evidence.activity_proof,
        evidence.header,
        evidence.header_signature,
        evidence.authorization,
    )
    .map_err(MaintainedOutcomeFailure::Inclusion)?;
    verify_receipt(
        evidence.maintenance,
        evidence.maintenance_proof,
        evidence.header,
        evidence.header_signature,
        evidence.authorization,
    )
    .map_err(MaintainedOutcomeFailure::Inclusion)?;
    let header = included.header().header();
    if authorised.sequencer_public_key != evidence.authorization.public_key() {
        return Err(Failure(ReceiptCheck::SequencerSignature));
    }
    if authorised.previous_state_root != header.previous_state_root() {
        return Err(Failure(ReceiptCheck::PreviousStateRoot));
    }
    if authorised.resulting_state_root != header.resulting_state_root() {
        return Err(Failure(ReceiptCheck::ResultingStateRoot));
    }
    let maintenance = layerx_wire::maintenance::decode_occupancy_maintenance(evidence.maintenance)
        .map_err(|_| MaintainedOutcomeFailure::MaintenanceEncoding)?;
    if maintenance.resulting_state_root != authorised.resulting_state_root {
        return Err(Failure(ReceiptCheck::ResultingStateRoot));
    }
    let receipt = decode(receipt_bytes).map_err(|_| Failure(ReceiptCheck::Decode))?;
    let protocol = receipt
        .protocol()
        .ok_or(Failure(ReceiptCheck::ReceiptShape))?;
    let count = header
        .last_sequence()
        .checked_sub(header.first_sequence())
        .and_then(|count| u32::try_from(count).ok())
        .ok_or(SequenceRange)?;
    if count == 0
        || count.checked_add(1) != Some(evidence.maintenance_proof.leaf_count())
        || evidence.maintenance_proof.leaf_index() != count
        || evidence.activity_proof.leaf_count() != evidence.maintenance_proof.leaf_count()
        || evidence.activity_proof.leaf_index() >= count
        || header
            .first_sequence()
            .checked_add(u64::from(evidence.activity_proof.leaf_index()))
            != Some(protocol.global_sequence())
    {
        return Err(SequenceRange);
    }
    let expected = layerx_wire::hash::receipt_execution_batch_id_maintenance(
        protocol,
        header,
        &maintenance,
        count,
    )
    .map_err(|_| Failure(ReceiptCheck::BatchId))?;
    if expected != authorised.batch_id {
        return Err(Failure(ReceiptCheck::BatchId));
    }
    Ok(AuthorizedBatch::new(
        expected,
        authorised.asset,
        authorised.previous_state_root,
        maintenance.previous_state_root,
        authorised.sequencer_public_key,
    ))
}

/// Verifies canonical receipt bytes and their internal sequencer signature
/// without accepting transport-supplied batch, asset, or state facts.
///
/// This is the receipt-authenticity primitive used after the same bytes have
/// independently passed a signed-header Merkle inclusion check.
///
/// # Errors
///
/// Refuses non-canonical or unsupported receipt shapes, zero activity
/// identities, missing signatures, and signatures invalid under the pinned
/// sequencer key.
pub fn verify_sequencer_signature(
    receipt_bytes: &[u8],
    sequencer_public_key: [u8; 32],
) -> Result<Receipt, VerificationFailure> {
    let receipt =
        decode(receipt_bytes).map_err(|_| VerificationFailure::at(ReceiptCheck::Decode))?;
    let reproduced =
        encode(&receipt).map_err(|_| VerificationFailure::at(ReceiptCheck::CanonicalEncoding))?;
    if reproduced != receipt_bytes {
        return Err(VerificationFailure::at(ReceiptCheck::CanonicalEncoding));
    }
    let protocol = receipt
        .protocol()
        .ok_or_else(|| VerificationFailure::at(ReceiptCheck::ReceiptShape))?;
    if !supported_protocol_version(protocol.protocol_version()) {
        return Err(VerificationFailure::at(ReceiptCheck::ProtocolVersion));
    }
    if protocol.activity_id() == [0; 32] {
        return Err(VerificationFailure::at(ReceiptCheck::ActivityId));
    }
    let signature = protocol
        .sequencer_signature()
        .ok_or_else(|| VerificationFailure::at(ReceiptCheck::MissingSignature))?;
    let unsigned = encode_unsigned(&receipt)
        .map_err(|_| VerificationFailure::at(ReceiptCheck::CanonicalEncoding))?;
    let digest = receipt_digest(&unsigned)
        .map_err(|_| VerificationFailure::at(ReceiptCheck::CanonicalEncoding))?;
    ed25519::verify_digest(&sequencer_public_key, &signature, &digest)
        .map_err(|_| VerificationFailure::at(ReceiptCheck::SequencerSignature))?;
    Ok(receipt)
}

/// Verifies one successful ABI-two Programs state receipt without imposing
/// ledger-transfer fields on an ACCOUNT or `WIND_DOWN` transition.
///
/// # Errors
///
/// Returns the exact canonical, module, root-chain, result or sequencer
/// signature check which refused the state receipt.
pub fn verify_program_state(
    receipt_bytes: &[u8],
    authorised: &AuthorizedBatch,
) -> Result<VerifiedReceipt, VerificationFailure> {
    let verified = verify_program_state_outcome(receipt_bytes, authorised)?;
    if verified
        .receipt()
        .protocol()
        .is_none_or(|receipt| receipt.result_code() != 0)
    {
        return Err(VerificationFailure::at(ReceiptCheck::ResultCode));
    }
    Ok(verified)
}

fn verify_program_state_outcome(
    receipt_bytes: &[u8],
    authorised: &AuthorizedBatch,
) -> Result<VerifiedReceipt, VerificationFailure> {
    let receipt =
        decode(receipt_bytes).map_err(|_| VerificationFailure::at(ReceiptCheck::Decode))?;
    let reproduced =
        encode(&receipt).map_err(|_| VerificationFailure::at(ReceiptCheck::CanonicalEncoding))?;
    if reproduced != receipt_bytes {
        return Err(VerificationFailure::at(ReceiptCheck::CanonicalEncoding));
    }
    let protocol = receipt
        .protocol()
        .ok_or_else(|| VerificationFailure::at(ReceiptCheck::ReceiptShape))?;
    if !supported_protocol_version(protocol.protocol_version()) {
        return Err(VerificationFailure::at(ReceiptCheck::ProtocolVersion));
    }
    if u32::from(protocol.module_id()) != PROGRAMS_MODULE_ID
        || !programs_version_for_protocol(
            protocol.protocol_version(),
            protocol.module_version(),
            true,
        )
        || u16::from(protocol.operation()) != PROGRAMS_STATE_OPERATION
    {
        return Err(VerificationFailure::at(ReceiptCheck::Module));
    }
    if protocol.result_code() != 0 && !protocol.effects().is_empty() {
        return Err(VerificationFailure::at(ReceiptCheck::ReceiptShape));
    }
    if protocol.activity_id() == [0; 32] {
        return Err(VerificationFailure::at(ReceiptCheck::ActivityId));
    }
    if protocol.batch_id() != authorised.batch_id {
        return Err(VerificationFailure::at(ReceiptCheck::BatchId));
    }
    if protocol.previous_state_root() != authorised.previous_state_root {
        return Err(VerificationFailure::at(ReceiptCheck::PreviousStateRoot));
    }
    if protocol.resulting_state_root() != authorised.resulting_state_root {
        return Err(VerificationFailure::at(ReceiptCheck::ResultingStateRoot));
    }
    let signature = protocol
        .sequencer_signature()
        .ok_or_else(|| VerificationFailure::at(ReceiptCheck::MissingSignature))?;
    let unsigned = encode_unsigned(&receipt)
        .map_err(|_| VerificationFailure::at(ReceiptCheck::CanonicalEncoding))?;
    let digest = receipt_digest(&unsigned)
        .map_err(|_| VerificationFailure::at(ReceiptCheck::CanonicalEncoding))?;
    ed25519::verify_digest(&authorised.sequencer_public_key, &signature, &digest)
        .map_err(|_| VerificationFailure::at(ReceiptCheck::SequencerSignature))?;
    Ok(VerifiedReceipt {
        receipt,
        canonical_bytes: reproduced,
        evidence: Evidence::sequencer(digest),
    })
}

/// Verifies a sequencer-signed Programs execution receipt, preserving both
/// successful and refused outcomes without imposing 402LXP transfer fields.
///
/// # Errors
///
/// Returns the exact decode, canonical-encoding, receipt-shape, protocol
/// version, module, guest ABI, activity, batch, root-chain or sequencer
/// signature check which refused the receipt.
pub fn verify_program_outcome(
    receipt_bytes: &[u8],
    authorised: &AuthorizedBatch,
) -> Result<VerifiedReceipt, VerificationFailure> {
    verify_program_outcome_selected(receipt_bytes, authorised, false)
}

/// Verifies a stored protocol-1 Programs receipt with historical module binding.
///
/// # Errors
/// Refuses any nonhistorical version, module mismatch or failed receipt proof.
pub fn verify_historical_program_outcome_v1(
    receipt_bytes: &[u8],
    authorised: &AuthorizedBatch,
) -> Result<VerifiedReceipt, VerificationFailure> {
    verify_program_outcome_selected(receipt_bytes, authorised, true)
}

fn verify_program_outcome_selected(
    receipt_bytes: &[u8],
    authorised: &AuthorizedBatch,
    historical_v1: bool,
) -> Result<VerifiedReceipt, VerificationFailure> {
    let receipt =
        decode(receipt_bytes).map_err(|_| VerificationFailure::at(ReceiptCheck::Decode))?;
    let reproduced =
        encode(&receipt).map_err(|_| VerificationFailure::at(ReceiptCheck::CanonicalEncoding))?;
    if reproduced != receipt_bytes {
        return Err(VerificationFailure::at(ReceiptCheck::CanonicalEncoding));
    }
    let protocol = receipt
        .protocol()
        .ok_or_else(|| VerificationFailure::at(ReceiptCheck::ReceiptShape))?;
    if if historical_v1 {
        protocol.protocol_version() != 1
    } else {
        !supported_protocol_version(protocol.protocol_version())
    } {
        return Err(VerificationFailure::at(ReceiptCheck::ProtocolVersion));
    }
    if historical_v1 && protocol.module_version() != 1 {
        return Err(VerificationFailure::at(ReceiptCheck::Module));
    }
    if u32::from(protocol.module_id()) != PROGRAMS_MODULE_ID
        || !programs_version_for_protocol(
            protocol.protocol_version(),
            protocol.module_version(),
            false,
        )
        || u16::from(protocol.operation()) != PROGRAMS_CALL_OPERATION
    {
        return Err(VerificationFailure::at(ReceiptCheck::Module));
    }
    let outcome = protocol
        .program_outcome()
        .ok_or_else(|| VerificationFailure::at(ReceiptCheck::ReceiptShape))?;
    if !supported_program_guest_abi(outcome.abi_version()) || outcome.runtime_version() != 1 {
        return Err(VerificationFailure::at(ReceiptCheck::ProtocolVersion));
    }
    if protocol.activity_id() == [0; 32] {
        return Err(VerificationFailure::at(ReceiptCheck::ActivityId));
    }
    if protocol.batch_id() != authorised.batch_id {
        return Err(VerificationFailure::at(ReceiptCheck::BatchId));
    }
    if protocol.previous_state_root() != authorised.previous_state_root {
        return Err(VerificationFailure::at(ReceiptCheck::PreviousStateRoot));
    }
    if protocol.resulting_state_root() != authorised.resulting_state_root {
        return Err(VerificationFailure::at(ReceiptCheck::ResultingStateRoot));
    }
    let signature = protocol
        .sequencer_signature()
        .ok_or_else(|| VerificationFailure::at(ReceiptCheck::MissingSignature))?;
    let unsigned = encode_unsigned(&receipt)
        .map_err(|_| VerificationFailure::at(ReceiptCheck::CanonicalEncoding))?;
    let digest = receipt_digest(&unsigned)
        .map_err(|_| VerificationFailure::at(ReceiptCheck::CanonicalEncoding))?;
    ed25519::verify_digest(&authorised.sequencer_public_key, &signature, &digest)
        .map_err(|_| VerificationFailure::at(ReceiptCheck::SequencerSignature))?;
    Ok(VerifiedReceipt {
        receipt,
        canonical_bytes: reproduced,
        evidence: Evidence::sequencer(digest),
    })
}

/// Verifies a Programs call against a separately pinned sequencer and prior
/// state root. Batch identifiers and the resulting root remain signed receipt
/// facts; they are not accepted from a sibling transport document.
///
/// # Errors
///
/// Returns the previous-state-root check when the receipt does not chain from
/// the pinned root, otherwise the exact [`verify_program_outcome`] failure.
pub fn verify_program_outcome_at_root(
    receipt_bytes: &[u8],
    sequencer_public_key: [u8; 32],
    expected_previous_state_root: [u8; 32],
) -> Result<VerifiedReceipt, VerificationFailure> {
    let receipt =
        decode(receipt_bytes).map_err(|_| VerificationFailure::at(ReceiptCheck::Decode))?;
    let protocol = receipt
        .protocol()
        .ok_or_else(|| VerificationFailure::at(ReceiptCheck::ReceiptShape))?;
    if protocol.previous_state_root() != expected_previous_state_root {
        return Err(VerificationFailure::at(ReceiptCheck::PreviousStateRoot));
    }
    let authorised = AuthorizedBatch::new(
        protocol.batch_id(),
        protocol.asset(),
        expected_previous_state_root,
        protocol.resulting_state_root(),
        sequencer_public_key,
    );
    verify_program_outcome(receipt_bytes, &authorised)
}

/// Decodes exact economic facts only when the supplied receipt is canonical.
/// Authority and state-root verification remain the responsibility of
/// [`verify_outcome`]; callers use this only with receipt bytes retained from a
/// successfully verified value.
///
/// # Errors
///
/// Returns the exact decode, canonical-encoding or receipt-shape failure.
pub fn canonical_protocol_facts(
    receipt_bytes: &[u8],
) -> Result<ProtocolReceiptFacts, VerificationFailure> {
    let receipt =
        decode(receipt_bytes).map_err(|_| VerificationFailure::at(ReceiptCheck::Decode))?;
    let reproduced =
        encode(&receipt).map_err(|_| VerificationFailure::at(ReceiptCheck::CanonicalEncoding))?;
    if reproduced != receipt_bytes {
        return Err(VerificationFailure::at(ReceiptCheck::CanonicalEncoding));
    }
    let protocol = receipt
        .protocol()
        .ok_or_else(|| VerificationFailure::at(ReceiptCheck::ReceiptShape))?;
    Ok(ProtocolReceiptFacts {
        result_code: protocol.result_code(),
        asset: protocol.asset(),
        amount: protocol.amount(),
        fee_charged: protocol.fee_charged(),
    })
}

#[cfg(test)]
mod programs_version_contract {
    use super::{
        programs_version_for_protocol, supported_program_guest_abi,
        supported_programs_module_version, supported_protocol_version,
        supports_program_account_state,
    };

    #[test]
    fn module_and_guest_versions_are_independent() {
        assert!(!supported_protocol_version(0));
        assert!(!supported_protocol_version(1));
        assert!(supported_protocol_version(2));
        assert!(supported_protocol_version(3));
        assert!(!supported_protocol_version(4));
        assert!(supported_programs_module_version(3));
        assert!(supports_program_account_state(3));
        assert!(supported_program_guest_abi(2));
        assert!(!supported_program_guest_abi(3));
        assert!(!supported_programs_module_version(4));
        assert!(!supports_program_account_state(1));
    }

    #[test]
    fn state_commitment_protocol_selects_exactly_programs_module_four() {
        assert!(programs_version_for_protocol(3, 4, false));
        assert!(programs_version_for_protocol(3, 4, true));
        assert!(!programs_version_for_protocol(3, 3, false));
        assert!(!programs_version_for_protocol(3, 3, true));
        assert!(!programs_version_for_protocol(3, 5, false));
        assert!(!programs_version_for_protocol(2, 4, false));
        assert!(!programs_version_for_protocol(2, 4, true));
        assert!(programs_version_for_protocol(2, 3, false));
        assert!(programs_version_for_protocol(2, 1, false));
        assert!(!programs_version_for_protocol(2, 1, true));
        assert!(programs_version_for_protocol(2, 2, true));
    }
}

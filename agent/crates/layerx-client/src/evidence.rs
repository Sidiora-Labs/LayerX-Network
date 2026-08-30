//! Production finality-evidence reads and authenticated registration.

use std::collections::BTreeSet;

use layerx_proof::checkpoint::{
    verify_certificate, Attestation, Certificate, Checkpoint, CheckpointError, GuarantorKey,
    SettlementDomain, ThresholdReport,
};
use layerx_proof::inclusion::{
    verify_activity, verify_receipt, InclusionError, SequencerAuthorization,
};
use layerx_proof::merkle::{MerkleError, Proof, MAX_DEPTH};
use layerx_proof::receipt::verify_sequencer_signature;
use layerx_proof::state::{
    verify_nested_account, AccountProofError, NestedAccountProof, VerifiedAccountState,
};
use layerx_types::payload::ModuleRegistry;
use layerx_wire::activity::{decode_signed, encode_signed};
use layerx_wire::hash::activity_id;
use layerx_wire::receipt::{decode_batch_header, Receipt};

use crate::lni::refusal::decode_core_refusal;
use crate::lni::schema::{decode_envelope, encode_envelope, Envelope, SchemaError, Version};
use crate::lni::transport::{FrameTransport, TransportError};

const CHECKPOINT_REQUEST_TAG: u16 = 14;
const CHECKPOINT_RESPONSE_TAG: u16 = 15;
const PROOF_BUNDLE_REQUEST_TAG: u16 = 16;
const PROOF_BUNDLE_RESPONSE_TAG: u16 = 17;
const ERROR_RESPONSE_TAG: u16 = 25;
const REGISTER_REQUEST_TAG: u16 = 28;
const REGISTER_RESPONSE_TAG: u16 = 29;
const WIRE_VERSION: u16 = 1;
const MAX_RECEIPT_BYTES: usize = 4_096;
const MAX_VALIDITY_PROOF_BYTES: usize = 1_048_576;
const MAX_SETTLEMENT_REFERENCE_BYTES: usize = 1_024;
const SETTLEMENT_REFERENCE_BYTES: usize = 110;
const MAX_GUARANTORS: usize = 32;
const ATTESTATION_BYTES: usize = 274;
const MAX_HEADER_BYTES: usize = 4096;

/// Minimum client frame bound required by the core's bounded CP1+CX1 frame.
pub const MINIMUM_FINALITY_FRAME_BYTES: usize = MAX_VALIDITY_PROOF_BYTES + 96 * 1024 + 22;

/// Exact root selection admitted by account and proof reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RootSelector {
    Latest,
    Batch(u64),
    Checkpoint([u8; 32]),
}

impl RootSelector {
    pub(crate) fn encode(self, bytes: &mut Vec<u8>) {
        match self {
            Self::Latest => bytes.push(1),
            Self::Batch(batch) => {
                bytes.push(2);
                bytes.extend_from_slice(&batch.to_be_bytes());
            }
            Self::Checkpoint(identifier) => {
                bytes.push(3);
                bytes.extend_from_slice(&identifier);
            }
        }
    }

    fn decode(reader: &mut Reader<'_>) -> Result<Self, EvidenceError> {
        match reader.u8()? {
            1 => Ok(Self::Latest),
            2 => {
                let batch = reader.u64()?;
                if batch == 0 {
                    return Err(EvidenceError::Malformed);
                }
                Ok(Self::Batch(batch))
            }
            3 => {
                let identifier = reader.array()?;
                if identifier == [0; 32] {
                    return Err(EvidenceError::Malformed);
                }
                Ok(Self::Checkpoint(identifier))
            }
            _ => Err(EvidenceError::Malformed),
        }
    }
}

/// One immutable checkpoint selector.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CheckpointSelector {
    Identifier([u8; 32]),
    Batch(u64),
}

/// Exact proof target admitted by tags 16/17.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProofBundleSelector {
    Activity([u8; 32]),
    AccountState {
        activity_id: [u8; 32],
        account_id: [u8; 32],
    },
    Receipt([u8; 32]),
}

/// Trusted boundary coordinates for one finality read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EvidenceContext {
    pub interface_version: Version,
    pub correlation_id: u64,
    pub expected_protocol_version: u16,
    pub expected_network_id: u32,
    pub handshake_sequencer_key: [u8; 32],
}

/// Checkpoint certificate, bonded-set snapshot and settlement registration
/// returned by an authenticated node only after its daemon-owned finality
/// authority accepted the bundle, then independently rechecked by this client.
#[derive(Clone, Debug)]
pub struct VerifiedCheckpoint {
    checkpoint_bytes: Vec<u8>,
    context_bytes: Vec<u8>,
    canonical_header: Vec<u8>,
    report: ThresholdReport,
    set_version: u64,
    resulting_registration_count: u64,
}

impl VerifiedCheckpoint {
    #[must_use]
    pub fn checkpoint_bytes(&self) -> &[u8] {
        &self.checkpoint_bytes
    }

    #[must_use]
    pub fn context_bytes(&self) -> &[u8] {
        &self.context_bytes
    }

    #[must_use]
    pub fn canonical_header(&self) -> &[u8] {
        &self.canonical_header
    }

    #[must_use]
    pub const fn report(&self) -> &ThresholdReport {
        &self.report
    }

    #[must_use]
    pub const fn set_version(&self) -> u64 {
        self.set_version
    }

    /// Returns the core registry count after this checkpoint's durable
    /// registration. CX1 transports the preceding expected count.
    #[must_use]
    pub const fn registration_count(&self) -> u64 {
        self.resulting_registration_count
    }
}

/// Exact finality bytes that passed local structural and cryptographic checks,
/// but have not yet been accepted by the node's trusted finality authority.
#[derive(Clone, Debug)]
pub struct FinalityEvidenceCandidate {
    checkpoint_bytes: Vec<u8>,
    context_bytes: Vec<u8>,
    checkpoint_id: [u8; 32],
    batch_number: u64,
}

impl FinalityEvidenceCandidate {
    /// Checks exact core/Paxeer evidence bytes before they can be submitted to
    /// the node's independently configured finality-authority callback.
    ///
    /// # Errors
    ///
    /// Returns the same structural, bonded-set, registration, certificate and
    /// settlement failures as a checkpoint read.
    pub fn from_exact_bytes(
        checkpoint_bytes: Vec<u8>,
        context_bytes: Vec<u8>,
        expected_protocol_version: u16,
        expected_network_id: u32,
    ) -> Result<Self, EvidenceError> {
        let checked = check_checkpoint_bytes(
            &checkpoint_bytes,
            &context_bytes,
            expected_protocol_version,
            expected_network_id,
        )?;
        Ok(Self {
            checkpoint_bytes,
            context_bytes,
            checkpoint_id: checked.report.evidence().checkpoint_id().ok_or(
                EvidenceError::Checkpoint(CheckpointError::CheckpointIdentifier),
            )?,
            batch_number: checked.report.batch_number(),
        })
    }
}

/// Durable acknowledgement returned only after verified append and fsync.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegistrationAck {
    pub checkpoint_id: [u8; 32],
    pub batch_number: u64,
    pub durable_record_digest: [u8; 32],
}

/// A verified activity, receipt, or account-state proof response.
#[derive(Clone, Debug)]
pub enum VerifiedProofBundle {
    Activity {
        canonical_bytes: Vec<u8>,
        activity_id: [u8; 32],
        proof: Proof,
        signed_header: SignedHeader,
    },
    Receipt {
        canonical_bytes: Vec<u8>,
        activity_id: [u8; 32],
        proof: Proof,
        signed_header: SignedHeader,
    },
    Account {
        canonical_bytes: Vec<u8>,
        activity_id: [u8; 32],
        verified: VerifiedAccountState,
        signed_header: SignedHeader,
    },
}

impl VerifiedProofBundle {
    #[must_use]
    pub const fn signed_header(&self) -> &SignedHeader {
        match self {
            Self::Activity { signed_header, .. }
            | Self::Receipt { signed_header, .. }
            | Self::Account { signed_header, .. } => signed_header,
        }
    }

    #[must_use]
    pub fn canonical_bytes(&self) -> &[u8] {
        match self {
            Self::Activity {
                canonical_bytes, ..
            }
            | Self::Receipt {
                canonical_bytes, ..
            }
            | Self::Account {
                canonical_bytes, ..
            } => canonical_bytes,
        }
    }
}

/// Exact signed-header authority block carried by proof responses.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignedHeader {
    pub sequencer_id: [u8; 32],
    pub public_key: [u8; 32],
    pub first_batch_number: u64,
    pub last_batch_number: u64,
    pub canonical_bytes: Vec<u8>,
    pub signature: [u8; 64],
}

impl SignedHeader {
    pub(crate) fn response_authorization(&self) -> SequencerAuthorization {
        SequencerAuthorization::new(
            self.sequencer_id,
            self.public_key,
            self.first_batch_number,
            self.last_batch_number,
        )
    }

    fn pinned_key_authorization(
        &self,
        handshake_key: [u8; 32],
        expected_protocol_version: u16,
        expected_network_id: u32,
    ) -> Result<SequencerAuthorization, EvidenceError> {
        require_handshake_key(self, handshake_key)?;
        let header =
            decode_batch_header(&self.canonical_bytes).map_err(|_| EvidenceError::Malformed)?;
        let batch = header.batch_number();
        if expected_protocol_version == 0
            || expected_network_id == 0
            || header.protocol_version() != expected_protocol_version
            || header.network_id() != expected_network_id
            || header.sequencer_id() != self.sequencer_id
            || batch < self.first_batch_number
            || batch > self.last_batch_number
        {
            return Err(EvidenceError::SequencerMismatch);
        }
        // NodeInfo authenticates the sequencer key, not an authorization
        // range. Limit the locally constructed authority to this exact signed
        // header; the response's range can never grant broader trust.
        Ok(SequencerAuthorization::new(
            self.sequencer_id,
            handshake_key,
            batch,
            batch,
        ))
    }

    #[must_use]
    pub fn same_evidence(&self, other: &Self) -> bool {
        self == other
    }

    /// Returns the batch number from the already verified canonical header.
    pub fn batch_number(&self) -> Result<u64, EvidenceError> {
        decode_batch_header(&self.canonical_bytes)
            .map(|header| header.batch_number())
            .map_err(|_| EvidenceError::Malformed)
    }
}

/// Exact transport or verification refusal for production evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EvidenceError {
    Transport(TransportError),
    Envelope(SchemaError),
    UnexpectedResponse,
    CoreRefusal {
        class: u8,
        result: layerx_types::result::ResultCode,
    },
    Unavailable,
    Malformed,
    SelectorMismatch,
    NetworkMismatch,
    SequencerMismatch,
    Activity,
    Receipt,
    Merkle(MerkleError),
    Inclusion(InclusionError),
    Account(AccountProofError),
    BondedSet,
    Requirements,
    Registration,
    Settlement,
    Checkpoint(CheckpointError),
}

impl From<TransportError> for EvidenceError {
    fn from(value: TransportError) -> Self {
        Self::Transport(value)
    }
}

impl From<SchemaError> for EvidenceError {
    fn from(value: SchemaError) -> Self {
        Self::Envelope(value)
    }
}

/// Retrieves and independently verifies one durable finalized checkpoint.
pub fn checkpoint(
    transport: &mut dyn FrameTransport,
    selector: CheckpointSelector,
    context: EvidenceContext,
) -> Result<VerifiedCheckpoint, EvidenceError> {
    if context.correlation_id == 0 || context.expected_network_id == 0 {
        return Err(EvidenceError::Malformed);
    }
    let mut request = Vec::with_capacity(35);
    request.extend_from_slice(&WIRE_VERSION.to_be_bytes());
    match selector {
        CheckpointSelector::Identifier(identifier) if identifier != [0; 32] => {
            request.push(1);
            request.extend_from_slice(&identifier);
        }
        CheckpointSelector::Batch(batch) if batch != 0 => {
            request.push(2);
            request.extend_from_slice(&batch.to_be_bytes());
        }
        CheckpointSelector::Identifier(_) | CheckpointSelector::Batch(_) => {
            return Err(EvidenceError::Malformed);
        }
    }
    let response = exchange(
        transport,
        context.interface_version,
        context.correlation_id,
        CHECKPOINT_REQUEST_TAG,
        CHECKPOINT_RESPONSE_TAG,
        &request,
        &[],
    )?;
    if response.payload.is_empty() || response.proof.is_empty() {
        return Err(EvidenceError::Unavailable);
    }
    let verified = checked_checkpoint(
        response.payload,
        response.proof,
        context.expected_protocol_version,
        context.expected_network_id,
    )?;
    match selector {
        CheckpointSelector::Identifier(identifier)
            if verified.report.evidence().checkpoint_id() != Some(identifier) =>
        {
            Err(EvidenceError::SelectorMismatch)
        }
        CheckpointSelector::Batch(batch) if verified.report.batch_number() != batch => {
            Err(EvidenceError::SelectorMismatch)
        }
        CheckpointSelector::Identifier(_) | CheckpointSelector::Batch(_) => Ok(verified),
    }
}

/// Registers one locally verified evidence bundle through additive LNI tags 28/29.
pub fn register_finality_evidence(
    transport: &mut dyn FrameTransport,
    evidence: &FinalityEvidenceCandidate,
    interface_version: Version,
    correlation_id: u64,
) -> Result<RegistrationAck, EvidenceError> {
    if correlation_id == 0 {
        return Err(EvidenceError::Malformed);
    }
    let response = exchange(
        transport,
        interface_version,
        correlation_id,
        REGISTER_REQUEST_TAG,
        REGISTER_RESPONSE_TAG,
        &evidence.checkpoint_bytes,
        &evidence.context_bytes,
    )?;
    if !response.proof.is_empty() || response.payload.len() != 74 {
        return Err(EvidenceError::Malformed);
    }
    let mut reader = Reader::new(&response.payload);
    if reader.u16()? != WIRE_VERSION {
        return Err(EvidenceError::Malformed);
    }
    let acknowledgement = RegistrationAck {
        checkpoint_id: reader.array()?,
        batch_number: reader.u64()?,
        durable_record_digest: reader.array()?,
    };
    reader.finish()?;
    if acknowledgement.checkpoint_id != evidence.checkpoint_id
        || acknowledgement.batch_number != evidence.batch_number
        || acknowledgement.durable_record_digest == [0; 32]
    {
        return Err(EvidenceError::Registration);
    }
    Ok(acknowledgement)
}

/// Retrieves and verifies an activity, receipt, or nested account proof.
#[allow(clippy::too_many_arguments)]
pub fn proof_bundle(
    transport: &mut dyn FrameTransport,
    selector: ProofBundleSelector,
    context: EvidenceContext,
    registry: &ModuleRegistry,
) -> Result<VerifiedProofBundle, EvidenceError> {
    let (kind, target_activity_id, account_id) = match selector {
        ProofBundleSelector::Activity(identifier) => (1, identifier, None),
        ProofBundleSelector::AccountState {
            activity_id,
            account_id,
        } => (2, activity_id, Some(account_id)),
        ProofBundleSelector::Receipt(identifier) => (3, identifier, None),
    };
    if target_activity_id == [0; 32]
        || account_id.is_some_and(|value| value == [0; 32])
        || context.correlation_id == 0
    {
        return Err(EvidenceError::Malformed);
    }
    let mut request = Vec::with_capacity(67);
    request.extend_from_slice(&WIRE_VERSION.to_be_bytes());
    request.push(kind);
    request.extend_from_slice(&target_activity_id);
    if let Some(account) = account_id {
        request.extend_from_slice(&account);
    }
    let response = exchange(
        transport,
        context.interface_version,
        context.correlation_id,
        PROOF_BUNDLE_REQUEST_TAG,
        PROOF_BUNDLE_RESPONSE_TAG,
        &request,
        &[],
    )?;
    if response.payload.is_empty() || response.proof.is_empty() {
        return Err(EvidenceError::Unavailable);
    }
    if kind == 2 {
        let account = account_id.ok_or(EvidenceError::Malformed)?;
        let decoded = decode_nested_evidence(
            &response.proof,
            context.expected_protocol_version,
            context.expected_network_id,
        )?;
        if decoded.selector != RootSelector::Latest || decoded.proof.account_id != account {
            return Err(EvidenceError::SelectorMismatch);
        }
        let authorization = decoded.signed_header.pinned_key_authorization(
            context.handshake_sequencer_key,
            context.expected_protocol_version,
            context.expected_network_id,
        )?;
        let verified = verify_nested_account(
            &response.payload,
            account,
            None,
            &decoded.proof,
            &authorization,
        )
        .map_err(EvidenceError::Account)?;
        if verified.receipt_activity_id() != target_activity_id {
            return Err(EvidenceError::SelectorMismatch);
        }
        return Ok(VerifiedProofBundle::Account {
            canonical_bytes: response.payload,
            activity_id: target_activity_id,
            verified,
            signed_header: decoded.signed_header,
        });
    }
    let mut reader = Reader::new(&response.proof);
    if reader.u16()? != WIRE_VERSION || reader.u8()? != kind {
        return Err(EvidenceError::Malformed);
    }
    let asserted_activity_id = reader.array()?;
    if asserted_activity_id != target_activity_id {
        return Err(EvidenceError::SelectorMismatch);
    }
    let proof = decode_proof(&mut reader)?;
    let signed_header = decode_signed_header(&mut reader)?;
    reader.finish()?;
    let authorization = signed_header.pinned_key_authorization(
        context.handshake_sequencer_key,
        context.expected_protocol_version,
        context.expected_network_id,
    )?;
    match kind {
        1 => {
            let activity =
                decode_signed(&response.payload, registry).map_err(|_| EvidenceError::Activity)?;
            if encode_signed(&activity).map_err(|_| EvidenceError::Activity)? != response.payload
                || activity_id(&activity).map_err(|_| EvidenceError::Activity)?
                    != target_activity_id
            {
                return Err(EvidenceError::Activity);
            }
            verify_activity(
                &response.payload,
                &proof,
                &signed_header.canonical_bytes,
                &signed_header.signature,
                &authorization,
            )
            .map_err(EvidenceError::Inclusion)?;
            Ok(VerifiedProofBundle::Activity {
                canonical_bytes: response.payload,
                activity_id: target_activity_id,
                proof,
                signed_header,
            })
        }
        3 => {
            let receipt = verify_sequencer_signature(&response.payload, authorization.public_key())
                .map_err(|_| EvidenceError::Receipt)?;
            let Receipt::Protocol(protocol) = receipt else {
                return Err(EvidenceError::Receipt);
            };
            if protocol.activity_id() != target_activity_id {
                return Err(EvidenceError::SelectorMismatch);
            }
            verify_receipt(
                &response.payload,
                &proof,
                &signed_header.canonical_bytes,
                &signed_header.signature,
                &authorization,
            )
            .map_err(EvidenceError::Inclusion)?;
            Ok(VerifiedProofBundle::Receipt {
                canonical_bytes: response.payload,
                activity_id: target_activity_id,
                proof,
                signed_header,
            })
        }
        _ => Err(EvidenceError::Malformed),
    }
}

pub(crate) struct DecodedNestedEvidence {
    pub selector: RootSelector,
    pub proof: NestedAccountProof,
    pub signed_header: SignedHeader,
    pub checkpoint: Option<VerifiedCheckpoint>,
}

pub(crate) fn decode_nested_evidence(
    bytes: &[u8],
    expected_protocol_version: u16,
    expected_network_id: u32,
) -> Result<DecodedNestedEvidence, EvidenceError> {
    let mut reader = Reader::new(bytes);
    if reader.u16()? != WIRE_VERSION || reader.u8()? != 2 {
        return Err(EvidenceError::Malformed);
    }
    let selector = RootSelector::decode(&mut reader)?;
    let account_id = reader.array()?;
    let account_root = reader.array()?;
    let universal_root = reader.array()?;
    let resulting_state_root = reader.array()?;
    if account_id == [0; 32]
        || account_root == [0; 32]
        || universal_root == [0; 32]
        || resulting_state_root == [0; 32]
    {
        return Err(EvidenceError::Malformed);
    }
    let account_proof = decode_proof(&mut reader)?;
    let account_tree_proof = decode_proof(&mut reader)?;
    let universal_root_proof = decode_proof(&mut reader)?;
    let receipt_bytes = decode_receipt_bytes(&mut reader)?;
    let receipt_proof = decode_proof(&mut reader)?;
    let signed_header = decode_signed_header(&mut reader)?;
    let checkpoint = match reader.u8()? {
        0 => None,
        1 => {
            let checkpoint_bytes = reader
                .length_prefixed(
                    MAX_VALIDITY_PROOF_BYTES
                        + MAX_HEADER_BYTES
                        + 16
                        + MAX_GUARANTORS * ATTESTATION_BYTES,
                )?
                .to_vec();
            let context_bytes = reader.length_prefixed(128 * 1024)?.to_vec();
            Some(checked_checkpoint(
                checkpoint_bytes,
                context_bytes,
                expected_protocol_version,
                expected_network_id,
            )?)
        }
        _ => return Err(EvidenceError::Malformed),
    };
    reader.finish()?;
    let proof = NestedAccountProof {
        account_id,
        account_root,
        universal_root,
        resulting_state_root,
        account_proof,
        account_tree_proof,
        universal_root_proof,
        receipt_bytes,
        receipt_proof,
        header_bytes: signed_header.canonical_bytes.clone(),
        header_signature: signed_header.signature,
    };
    bind_selector(selector, &signed_header, checkpoint.as_ref())?;
    Ok(DecodedNestedEvidence {
        selector,
        proof,
        signed_header,
        checkpoint,
    })
}

fn decode_receipt_bytes(reader: &mut Reader<'_>) -> Result<Vec<u8>, EvidenceError> {
    Ok(reader.length_prefixed(MAX_RECEIPT_BYTES)?.to_vec())
}

fn bind_selector(
    selector: RootSelector,
    signed: &SignedHeader,
    checkpoint: Option<&VerifiedCheckpoint>,
) -> Result<(), EvidenceError> {
    let header =
        decode_batch_header(&signed.canonical_bytes).map_err(|_| EvidenceError::Malformed)?;
    match selector {
        RootSelector::Latest => match checkpoint {
            Some(checkpoint) if checkpoint.canonical_header() == signed.canonical_bytes => Ok(()),
            Some(_) => Err(EvidenceError::SelectorMismatch),
            None => Ok(()),
        },
        RootSelector::Batch(batch) if header.batch_number() == batch => match checkpoint {
            Some(checkpoint) if checkpoint.canonical_header() == signed.canonical_bytes => Ok(()),
            Some(_) => Err(EvidenceError::SelectorMismatch),
            None => Ok(()),
        },
        RootSelector::Checkpoint(identifier) => {
            let checkpoint = checkpoint.ok_or(EvidenceError::Unavailable)?;
            if checkpoint.report.evidence().checkpoint_id() == Some(identifier)
                && checkpoint.canonical_header() == signed.canonical_bytes
            {
                Ok(())
            } else {
                Err(EvidenceError::SelectorMismatch)
            }
        }
        RootSelector::Batch(_) => Err(EvidenceError::SelectorMismatch),
    }
}

fn require_handshake_key(
    signed: &SignedHeader,
    handshake_key: [u8; 32],
) -> Result<(), EvidenceError> {
    if handshake_key == [0; 32] || signed.public_key != handshake_key {
        Err(EvidenceError::SequencerMismatch)
    } else {
        Ok(())
    }
}

struct CheckedCheckpoint {
    report: ThresholdReport,
    canonical_header: Vec<u8>,
    set_version: u64,
    resulting_registration_count: u64,
}

fn checked_checkpoint(
    checkpoint_bytes: Vec<u8>,
    context_bytes: Vec<u8>,
    expected_protocol_version: u16,
    expected_network_id: u32,
) -> Result<VerifiedCheckpoint, EvidenceError> {
    let checked = check_checkpoint_bytes(
        &checkpoint_bytes,
        &context_bytes,
        expected_protocol_version,
        expected_network_id,
    )?;
    Ok(VerifiedCheckpoint {
        checkpoint_bytes,
        context_bytes,
        canonical_header: checked.canonical_header,
        report: checked.report,
        set_version: checked.set_version,
        resulting_registration_count: checked.resulting_registration_count,
    })
}

fn resulting_registration_count(expected: u64) -> Result<u64, EvidenceError> {
    expected.checked_add(1).ok_or(EvidenceError::Registration)
}

fn requirements_satisfied(
    requirements: &Requirements,
    header_epoch: u64,
    certificate_threshold: usize,
) -> bool {
    requirements.checkpoint_epoch == header_epoch
        && requirements.threshold == certificate_threshold
        && requirements.threshold != 0
        && requirements.availability_answered
        && !requirements.equivocation_detected
        && requirements.now_ms >= requirements.challenge_window_end_ms
}

fn check_checkpoint_bytes(
    checkpoint_bytes: &[u8],
    context_bytes: &[u8],
    expected_protocol_version: u16,
    expected_network_id: u32,
) -> Result<CheckedCheckpoint, EvidenceError> {
    let checkpoint = decode_checkpoint_material(&checkpoint_bytes)?;
    let context = decode_checkpoint_context(&context_bytes)?;
    let header = decode_batch_header(checkpoint.certificate.checkpoint().header_bytes())
        .map_err(|_| EvidenceError::Malformed)?;
    if expected_protocol_version == 0
        || expected_network_id == 0
        || header.protocol_version() != expected_protocol_version
        || header.network_id() != expected_network_id
    {
        return Err(EvidenceError::NetworkMismatch);
    }
    if !requirements_satisfied(
        &context.requirements,
        header.epoch(),
        checkpoint.certificate.threshold(),
    ) {
        return Err(EvidenceError::Requirements);
    }
    if context.registration.checkpoint_id == [0; 32]
        || context.registration.checkpoint_id
            != layerx_proof::checkpoint::checkpoint_id(checkpoint.certificate.checkpoint())
                .map_err(EvidenceError::Checkpoint)?
        || context.registration.resulting_state_root != header.resulting_state_root()
        || context.registration.batch_number != header.batch_number()
        || context.registration.chain_id == 0
        || context.registration.contract == [0; 20]
        || context.registration.reference != checkpoint.settlement_reference
    {
        return Err(EvidenceError::Registration);
    }
    let settlement = decode_settlement_reference(&checkpoint.settlement_reference)?;
    if settlement.chain_id != context.registration.chain_id
        || settlement.contract != context.registration.contract
        || settlement.checkpoint_id != context.registration.checkpoint_id
    {
        return Err(EvidenceError::Settlement);
    }
    let (keys, timely_eligible) = context.bonded_keys(&checkpoint.certificate)?;
    if timely_eligible < context.requirements.threshold {
        return Err(EvidenceError::Requirements);
    }
    let report = verify_certificate(
        &checkpoint.certificate,
        &keys,
        &context.registration.checkpoint_id,
        SettlementDomain::new(context.registration.chain_id, context.registration.contract),
        Some(&context.registration.reference),
    )
    .map_err(EvidenceError::Checkpoint)?;
    if report.batch_number() != context.registration.batch_number
        || report.resulting_state_root() != context.registration.resulting_state_root
    {
        return Err(EvidenceError::Registration);
    }
    Ok(CheckedCheckpoint {
        report,
        canonical_header: checkpoint.certificate.checkpoint().header_bytes().to_vec(),
        set_version: context.set_version,
        resulting_registration_count: resulting_registration_count(
            context.expected_registration_count,
        )?,
    })
}

struct CheckpointMaterial {
    certificate: Certificate,
    settlement_reference: Vec<u8>,
}

fn decode_checkpoint_material(bytes: &[u8]) -> Result<CheckpointMaterial, EvidenceError> {
    let mut reader = Reader::new(bytes);
    if reader.u16()? != WIRE_VERSION {
        return Err(EvidenceError::Malformed);
    }
    let header = reader.length_prefixed(MAX_HEADER_BYTES)?.to_vec();
    let validity = reader.length_prefixed(MAX_VALIDITY_PROOF_BYTES)?.to_vec();
    let count = usize::from(reader.u8()?);
    if count == 0 || count > MAX_GUARANTORS {
        return Err(EvidenceError::Malformed);
    }
    let mut attestations = Vec::with_capacity(count);
    let mut prior = None;
    for _ in 0..count {
        let attestation = decode_attestation(&mut reader)?;
        if prior.is_some_and(|identifier| identifier >= attestation.guarantor_id()) {
            return Err(EvidenceError::Malformed);
        }
        prior = Some(attestation.guarantor_id());
        attestations.push(attestation);
    }
    let threshold = usize::from(reader.u8()?);
    if threshold == 0 || threshold > count {
        return Err(EvidenceError::Malformed);
    }
    let settlement_reference = reader
        .length_prefixed_u16(MAX_SETTLEMENT_REFERENCE_BYTES)?
        .to_vec();
    reader.finish()?;
    if settlement_reference.len() != SETTLEMENT_REFERENCE_BYTES {
        return Err(EvidenceError::Settlement);
    }
    Ok(CheckpointMaterial {
        certificate: Certificate::new(
            Checkpoint::new(header, validity),
            attestations,
            threshold,
            Some(settlement_reference.clone()),
        ),
        settlement_reference,
    })
}

fn decode_attestation(reader: &mut Reader<'_>) -> Result<Attestation, EvidenceError> {
    let protocol_version = reader.u16()?;
    let network_id = reader.u32()?;
    let paxeer_chain_id = reader.u64()?;
    let paxeer_settlement_contract = reader.array()?;
    let epoch = reader.u64()?;
    let checkpoint_id = reader.array()?;
    let checkpoint_hash = reader.array()?;
    let guarantor_id = reader.array()?;
    let batch_number = reader.u64()?;
    let data_availability_root = reader.array()?;
    let replayed = reader.boolean()?;
    let data_possessed = reader.boolean()?;
    let availability_class_mask = reader.u8()?;
    let attested_at_ms = reader.u64()?;
    let signer = reader.array()?;
    let signature = reader.array()?;
    let signature_v = reader.u8()?;
    Ok(Attestation::new(
        protocol_version,
        network_id,
        paxeer_chain_id,
        paxeer_settlement_contract,
        epoch,
        checkpoint_id,
        checkpoint_hash,
        guarantor_id,
        batch_number,
        data_availability_root,
        replayed,
        data_possessed,
        availability_class_mask,
        attested_at_ms,
        signer,
        signature,
        signature_v,
    ))
}

#[derive(Clone)]
struct SignerAuthorization {
    public_key: [u8; 33],
    active_from_epoch: u64,
    active_until_epoch: u64,
    set_version: u64,
}

#[derive(Clone)]
struct Bond {
    guarantor_id: [u8; 32],
    public_key: [u8; 33],
    bond: u128,
    joined_epoch: u64,
    removed_epoch: u64,
    ejected_at_version: u64,
    signers: Vec<SignerAuthorization>,
    jailed: bool,
    unresolved_slashing: bool,
    active: bool,
}

struct Requirements {
    checkpoint_epoch: u64,
    challenge_window_end_ms: u64,
    checkpoint_deadline_ms: u64,
    now_ms: u64,
    threshold: usize,
    minimum_bond: u128,
    availability_answered: bool,
    equivocation_detected: bool,
}

struct Registration {
    checkpoint_id: [u8; 32],
    resulting_state_root: [u8; 32],
    batch_number: u64,
    chain_id: u64,
    contract: [u8; 20],
    reference: Vec<u8>,
}

struct CheckpointContextMaterial {
    expected_registration_count: u64,
    set_version: u64,
    bonds: Vec<Bond>,
    requirements: Requirements,
    registration: Registration,
}

impl CheckpointContextMaterial {
    fn bonded_keys(
        &self,
        certificate: &Certificate,
    ) -> Result<(Vec<GuarantorKey>, usize), EvidenceError> {
        let epoch = self.requirements.checkpoint_epoch;
        let mut keys = Vec::with_capacity(self.bonds.len());
        let mut timely_eligible = 0;
        for bond in &self.bonds {
            let signer = bond
                .signers
                .iter()
                .find(|authorization| {
                    authorization.active_from_epoch <= epoch
                        && (authorization.active_until_epoch == 0
                            || epoch < authorization.active_until_epoch)
                })
                .map(|authorization| authorization.public_key);
            let eligible = bond.active
                && !bond.jailed
                && !bond.unresolved_slashing
                && bond.ejected_at_version == 0
                && bond.joined_epoch <= epoch
                && (bond.removed_epoch == 0 || bond.removed_epoch > epoch)
                && bond.bond >= self.requirements.minimum_bond;
            keys.push(GuarantorKey::new(
                bond.guarantor_id,
                signer.unwrap_or(bond.public_key),
                eligible && signer.is_some(),
            ));
        }
        for attestation in certificate.attestations() {
            let eligible = self.bonds.iter().any(|bond| {
                bond.guarantor_id == attestation.guarantor_id()
                    && bond.active
                    && !bond.jailed
                    && !bond.unresolved_slashing
                    && bond.ejected_at_version == 0
                    && bond.joined_epoch <= epoch
                    && (bond.removed_epoch == 0 || bond.removed_epoch > epoch)
                    && bond.bond >= self.requirements.minimum_bond
                    && bond.signers.iter().any(|authorization| {
                        authorization.active_from_epoch <= epoch
                            && (authorization.active_until_epoch == 0
                                || epoch < authorization.active_until_epoch)
                    })
            });
            if attestation.attested_at_ms() <= self.requirements.checkpoint_deadline_ms && eligible
            {
                timely_eligible += 1;
            }
        }
        Ok((keys, timely_eligible))
    }
}

fn decode_checkpoint_context(bytes: &[u8]) -> Result<CheckpointContextMaterial, EvidenceError> {
    let mut reader = Reader::new(bytes);
    if reader.u16()? != WIRE_VERSION {
        return Err(EvidenceError::Malformed);
    }
    let expected_registration_count = reader.u64()?;
    let set_version = reader.u64()?;
    let last_governance_sequence = reader.u64()?;
    let count = usize::from(reader.u8()?);
    if count == 0
        || count > MAX_GUARANTORS
        || set_version == 0
        || last_governance_sequence > set_version
    {
        return Err(EvidenceError::BondedSet);
    }
    let mut bonds = Vec::with_capacity(count);
    let mut prior_id = None;
    let mut prior_bond_signer_keys = BTreeSet::new();
    for _ in 0..count {
        let guarantor_id = reader.array()?;
        let public_key = reader.array()?;
        let bond = reader.u128()?;
        let joined_epoch = reader.u64()?;
        let removed_epoch = reader.u64()?;
        let ejected_at_version = reader.u64()?;
        let signer_count = usize::from(reader.u8()?);
        if guarantor_id == [0; 32]
            || prior_id.is_some_and(|prior| prior >= guarantor_id)
            || joined_epoch == 0
            || ejected_at_version > set_version
            || signer_count == 0
            || signer_count > MAX_GUARANTORS
        {
            return Err(EvidenceError::BondedSet);
        }
        let mut signers = Vec::with_capacity(signer_count);
        let mut bond_signer_keys = BTreeSet::new();
        for index in 0..signer_count {
            let authorization = SignerAuthorization {
                public_key: reader.array()?,
                active_from_epoch: reader.u64()?,
                active_until_epoch: reader.u64()?,
                set_version: reader.u64()?,
            };
            if authorization.active_from_epoch == 0
                || authorization.set_version == 0
                || authorization.set_version > set_version
                || authorization.active_until_epoch != 0
                    && authorization.active_until_epoch <= authorization.active_from_epoch
                || index == 0 && authorization.active_from_epoch != joined_epoch
                || signers.last().is_some_and(|prior: &SignerAuthorization| {
                    prior.active_until_epoch != authorization.active_from_epoch
                        || prior.set_version >= authorization.set_version
                })
                || layerx_crypto::secp256k1::evm_address(&authorization.public_key).is_err()
                || prior_bond_signer_keys.contains(&authorization.public_key)
            {
                return Err(EvidenceError::BondedSet);
            }
            bond_signer_keys.insert(authorization.public_key);
            signers.push(authorization);
        }
        let flags = reader.u8()?;
        if flags & !0x07 != 0
            || signers.last().map(|value| value.public_key) != Some(public_key)
            || (removed_epoch == 0
                && signers
                    .last()
                    .is_none_or(|value| value.active_until_epoch != 0))
            || (removed_epoch != 0
                && signers
                    .last()
                    .is_none_or(|value| value.active_until_epoch != removed_epoch))
        {
            return Err(EvidenceError::BondedSet);
        }
        let jailed = flags & 1 != 0;
        let unresolved_slashing = flags & 2 != 0;
        let active = flags & 4 != 0;
        if (ejected_at_version != 0 && (active || !jailed)) || (removed_epoch != 0 && active) {
            return Err(EvidenceError::BondedSet);
        }
        bonds.push(Bond {
            guarantor_id,
            public_key,
            bond,
            joined_epoch,
            removed_epoch,
            ejected_at_version,
            signers,
            jailed,
            unresolved_slashing,
            active,
        });
        prior_bond_signer_keys.extend(bond_signer_keys);
        prior_id = Some(guarantor_id);
    }
    let checkpoint_epoch = reader.u64()?;
    let challenge_window_end_ms = reader.u64()?;
    let checkpoint_deadline_ms = reader.u64()?;
    let now_ms = reader.u64()?;
    let threshold = usize::from(reader.u8()?);
    let minimum_bond = reader.u128()?;
    let requirement_flags = reader.u8()?;
    if checkpoint_epoch == 0
        || threshold == 0
        || threshold > MAX_GUARANTORS
        || requirement_flags & !0x03 != 0
    {
        return Err(EvidenceError::Requirements);
    }
    let checkpoint_id = reader.array()?;
    let resulting_state_root = reader.array()?;
    let batch_number = reader.u64()?;
    let chain_id = reader.u64()?;
    let contract = reader.array()?;
    let reference = reader
        .length_prefixed_u16(MAX_SETTLEMENT_REFERENCE_BYTES)?
        .to_vec();
    reader.finish()?;
    if reference.len() != SETTLEMENT_REFERENCE_BYTES {
        return Err(EvidenceError::Settlement);
    }
    Ok(CheckpointContextMaterial {
        expected_registration_count,
        set_version,
        bonds,
        requirements: Requirements {
            checkpoint_epoch,
            challenge_window_end_ms,
            checkpoint_deadline_ms,
            now_ms,
            threshold,
            minimum_bond,
            availability_answered: requirement_flags & 1 != 0,
            equivocation_detected: requirement_flags & 2 != 0,
        },
        registration: Registration {
            checkpoint_id,
            resulting_state_root,
            batch_number,
            chain_id,
            contract,
            reference,
        },
    })
}

struct SettlementReference {
    chain_id: u64,
    contract: [u8; 20],
    checkpoint_id: [u8; 32],
}

fn decode_settlement_reference(bytes: &[u8]) -> Result<SettlementReference, EvidenceError> {
    if bytes.len() != SETTLEMENT_REFERENCE_BYTES {
        return Err(EvidenceError::Settlement);
    }
    let mut reader = Reader::new(bytes);
    if reader.u16()? != WIRE_VERSION {
        return Err(EvidenceError::Settlement);
    }
    let reference = SettlementReference {
        chain_id: reader.u64()?,
        contract: reader.array()?,
        checkpoint_id: reader.array()?,
    };
    let tx_id: [u8; 32] = reader.array()?;
    let settled_block = reader.u64()?;
    let settled_at_ms = reader.u64()?;
    reader.finish()?;
    if reference.chain_id == 0
        || reference.contract == [0; 20]
        || reference.checkpoint_id == [0; 32]
        || tx_id == [0; 32]
        || settled_block == 0
        || settled_at_ms == 0
    {
        return Err(EvidenceError::Settlement);
    }
    Ok(reference)
}

fn decode_signed_header(reader: &mut Reader<'_>) -> Result<SignedHeader, EvidenceError> {
    if reader.u16()? != WIRE_VERSION {
        return Err(EvidenceError::Malformed);
    }
    let sequencer_id = reader.array()?;
    let public_key = reader.array()?;
    let first_batch_number = reader.u64()?;
    let last_batch_number = reader.u64()?;
    let canonical_bytes = reader.length_prefixed(MAX_HEADER_BYTES)?.to_vec();
    let signature = reader.array()?;
    if sequencer_id == [0; 32]
        || public_key == [0; 32]
        || first_batch_number == 0
        || last_batch_number < first_batch_number
    {
        return Err(EvidenceError::Malformed);
    }
    Ok(SignedHeader {
        sequencer_id,
        public_key,
        first_batch_number,
        last_batch_number,
        canonical_bytes,
        signature,
    })
}

fn decode_proof(reader: &mut Reader<'_>) -> Result<Proof, EvidenceError> {
    let leaf_index = reader.u32()?;
    let leaf_count = reader.u32()?;
    let depth = usize::from(reader.u8()?);
    if depth > MAX_DEPTH {
        return Err(EvidenceError::Malformed);
    }
    let mut siblings = Vec::with_capacity(depth);
    for _ in 0..depth {
        siblings.push(reader.array()?);
    }
    Proof::new(leaf_index, leaf_count, siblings).map_err(EvidenceError::Merkle)
}

struct Response {
    payload: Vec<u8>,
    proof: Vec<u8>,
}

fn exchange(
    transport: &mut dyn FrameTransport,
    version: Version,
    correlation_id: u64,
    request_tag: u16,
    response_tag: u16,
    payload: &[u8],
    proof: &[u8],
) -> Result<Response, EvidenceError> {
    transport.send(&encode_envelope(Envelope {
        version,
        message_tag: request_tag,
        correlation_id,
        canonical_payload: payload,
        proof_material: proof,
    })?)?;
    let response_bytes = transport.receive()?;
    let response = decode_envelope(&response_bytes)?;
    if response.version.major == version.major
        && response.message_tag == ERROR_RESPONSE_TAG
        && response.correlation_id == correlation_id
        && response.proof_material.is_empty()
    {
        let refusal = decode_core_refusal(response.canonical_payload)
            .ok_or(EvidenceError::UnexpectedResponse)?;
        return Err(EvidenceError::CoreRefusal {
            class: refusal.class,
            result: refusal.result,
        });
    }
    if response.version.major != version.major
        || response.message_tag != response_tag
        || response.correlation_id != correlation_id
    {
        return Err(EvidenceError::UnexpectedResponse);
    }
    Ok(Response {
        payload: response.canonical_payload.to_vec(),
        proof: response.proof_material.to_vec(),
    })
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn bytes(&mut self, length: usize) -> Result<&'a [u8], EvidenceError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(EvidenceError::Malformed)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(EvidenceError::Malformed)?;
        self.offset = end;
        Ok(value)
    }

    fn length_prefixed(&mut self, maximum: usize) -> Result<&'a [u8], EvidenceError> {
        let length = usize::try_from(self.u32()?).map_err(|_| EvidenceError::Malformed)?;
        if length > maximum {
            return Err(EvidenceError::Malformed);
        }
        self.bytes(length)
    }

    fn length_prefixed_u16(&mut self, maximum: usize) -> Result<&'a [u8], EvidenceError> {
        let length = usize::from(self.u16()?);
        if length > maximum {
            return Err(EvidenceError::Malformed);
        }
        self.bytes(length)
    }

    fn array<const LENGTH: usize>(&mut self) -> Result<[u8; LENGTH], EvidenceError> {
        self.bytes(LENGTH)?
            .try_into()
            .map_err(|_| EvidenceError::Malformed)
    }

    fn u8(&mut self) -> Result<u8, EvidenceError> {
        self.array().map(u8::from_be_bytes)
    }

    fn u16(&mut self) -> Result<u16, EvidenceError> {
        self.array().map(u16::from_be_bytes)
    }

    fn u32(&mut self) -> Result<u32, EvidenceError> {
        self.array().map(u32::from_be_bytes)
    }

    fn u64(&mut self) -> Result<u64, EvidenceError> {
        self.array().map(u64::from_be_bytes)
    }

    fn u128(&mut self) -> Result<u128, EvidenceError> {
        self.array().map(u128::from_be_bytes)
    }

    fn boolean(&mut self) -> Result<bool, EvidenceError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(EvidenceError::Malformed),
        }
    }

    fn finish(self) -> Result<(), EvidenceError> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(EvidenceError::Malformed)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GENERATOR_KEY: [u8; 33] = [
        0x02, 0x79, 0xbe, 0x66, 0x7e, 0xf9, 0xdc, 0xbb, 0xac, 0x55, 0xa0, 0x62, 0x95, 0xce, 0x87,
        0x0b, 0x07, 0x02, 0x9b, 0xfc, 0xdb, 0x2d, 0xce, 0x28, 0xd9, 0x59, 0xf2, 0x81, 0x5b, 0x16,
        0xf8, 0x17, 0x98,
    ];

    fn checkpoint_context(
        signer_authorizations: &[([u8; 33], u64, u64, u64)],
        expected_registration_count: u64,
    ) -> Vec<u8> {
        let set_version = signer_authorizations
            .last()
            .map(|authorization| authorization.3)
            .unwrap_or(1);
        let current_key = signer_authorizations
            .last()
            .map(|authorization| authorization.0)
            .unwrap_or(GENERATOR_KEY);
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&WIRE_VERSION.to_be_bytes());
        bytes.extend_from_slice(&expected_registration_count.to_be_bytes());
        bytes.extend_from_slice(&set_version.to_be_bytes());
        bytes.extend_from_slice(&set_version.to_be_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&[1; 32]);
        bytes.extend_from_slice(&current_key);
        bytes.extend_from_slice(&100_u128.to_be_bytes());
        bytes.extend_from_slice(&1_u64.to_be_bytes());
        bytes.extend_from_slice(&0_u64.to_be_bytes());
        bytes.extend_from_slice(&0_u64.to_be_bytes());
        bytes.push(
            u8::try_from(signer_authorizations.len())
                .unwrap_or_else(|error| panic!("signer count overflow: {error}")),
        );
        for (key, active_from, active_until, authorization_version) in signer_authorizations {
            bytes.extend_from_slice(key);
            bytes.extend_from_slice(&active_from.to_be_bytes());
            bytes.extend_from_slice(&active_until.to_be_bytes());
            bytes.extend_from_slice(&authorization_version.to_be_bytes());
        }
        bytes.push(4);
        bytes.extend_from_slice(&2_u64.to_be_bytes());
        bytes.extend_from_slice(&100_u64.to_be_bytes());
        bytes.extend_from_slice(&90_u64.to_be_bytes());
        bytes.extend_from_slice(&100_u64.to_be_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&1_u128.to_be_bytes());
        bytes.push(1);
        let checkpoint_id = [3; 32];
        let contract = [5; 20];
        bytes.extend_from_slice(&checkpoint_id);
        bytes.extend_from_slice(&[4; 32]);
        bytes.extend_from_slice(&1_u64.to_be_bytes());
        bytes.extend_from_slice(&7_u64.to_be_bytes());
        bytes.extend_from_slice(&contract);
        let mut reference = Vec::with_capacity(SETTLEMENT_REFERENCE_BYTES);
        reference.extend_from_slice(&WIRE_VERSION.to_be_bytes());
        reference.extend_from_slice(&7_u64.to_be_bytes());
        reference.extend_from_slice(&contract);
        reference.extend_from_slice(&checkpoint_id);
        reference.extend_from_slice(&[6; 32]);
        reference.extend_from_slice(&9_u64.to_be_bytes());
        reference.extend_from_slice(&10_u64.to_be_bytes());
        bytes.extend_from_slice(
            &u16::try_from(reference.len())
                .unwrap_or_else(|error| panic!("reference length overflow: {error}"))
                .to_be_bytes(),
        );
        bytes.extend_from_slice(&reference);
        bytes
    }

    #[test]
    fn nested_receipt_bound_matches_core_state_limit() {
        let mut accepted = Vec::with_capacity(MAX_RECEIPT_BYTES + 4);
        accepted.extend_from_slice(
            &u32::try_from(MAX_RECEIPT_BYTES)
                .unwrap_or_else(|error| panic!("receipt bound overflow: {error}"))
                .to_be_bytes(),
        );
        accepted.resize(MAX_RECEIPT_BYTES + 4, 7);
        let mut reader = Reader::new(&accepted);
        assert_eq!(
            decode_receipt_bytes(&mut reader),
            Ok(vec![7; MAX_RECEIPT_BYTES])
        );
        assert_eq!(reader.finish(), Ok(()));

        let rejected = u32::try_from(MAX_RECEIPT_BYTES + 1)
            .unwrap_or_else(|error| panic!("receipt rejection bound overflow: {error}"))
            .to_be_bytes();
        assert_eq!(
            decode_receipt_bytes(&mut Reader::new(&rejected)),
            Err(EvidenceError::Malformed)
        );
    }

    #[test]
    fn bonded_snapshot_validates_all_keys_and_allows_intra_bond_reuse() {
        let reused = checkpoint_context(&[(GENERATOR_KEY, 1, 2, 1), (GENERATOR_KEY, 2, 0, 2)], 0);
        assert!(decode_checkpoint_context(&reused).is_ok());

        let invalid = checkpoint_context(&[([0; 33], 1, 0, 1)], 0);
        assert!(matches!(
            decode_checkpoint_context(&invalid),
            Err(EvidenceError::BondedSet)
        ));
    }

    #[test]
    fn finality_window_may_close_after_attestation_deadline() {
        let requirements = Requirements {
            checkpoint_epoch: 2,
            challenge_window_end_ms: 100,
            checkpoint_deadline_ms: 90,
            now_ms: 100,
            threshold: 1,
            minimum_bond: 1,
            availability_answered: true,
            equivocation_detected: false,
        };
        assert!(requirements_satisfied(&requirements, 2, 1));
    }

    #[test]
    fn registration_count_is_resulting_and_checked() {
        assert_eq!(resulting_registration_count(0), Ok(1));
        assert_eq!(resulting_registration_count(41), Ok(42));
        assert_eq!(
            resulting_registration_count(u64::MAX),
            Err(EvidenceError::Registration)
        );
    }
}

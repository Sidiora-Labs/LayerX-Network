//! Cryptographic deployment and current-lifecycle evidence for Programs.

use core::fmt::{self, Display};
use std::fs::{self, File};
use std::io::Read as _;
use std::path::Path;

use layerx_programs_runtime::{
    ProgramId, UpgradePolicy, ABI_V1_VERSION, ABI_V2_VERSION,
};
use layerx_proof::inclusion::{
    verify_activity, verify_receipt as verify_receipt_inclusion, SequencerAuthorization,
};
use layerx_proof::merkle::{decode_proof, encode_proof, Proof};
use layerx_proof::receipt::{verify_program_state, AuthorizedBatch};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_wire::activity::{decode_signed, encode_signed};
use layerx_wire::hash::{activity_id, execution_batch_id, payload_hash, receipt_digest};
use layerx_wire::receipt::{
    decode as decode_receipt, decode_batch_header, encode_unsigned,
};

use crate::account_state::verify_state_membership;
use crate::{
    DeploymentRecord, ProgramLifecycle, ReadFreshness, StateProof,
};

const PROGRAMS_MODULE_ID: u16 = 9;
const DEPLOY_ORDINAL: u16 = 1;
const UPGRADE_ORDINAL: u16 = 2;
const PROGRAM_RECORD_BYTES: usize = 71;
const PROGRAM_KEY_PREFIX: &[u8] = b"program\0";
const STATUS_KEY_PREFIX: &[u8] = b"wind-down\0s";
const STATUS_RECORD_BYTES: usize = 54;
const WASM_HEADER: &[u8; 8] = b"\0asm\x01\0\0\0";
const MAX_MODULE_BYTES: usize = 32 * 1024 * 1024;
const MAX_EVIDENCE_BYTES: usize = 40 * 1024 * 1024;
const EVIDENCE_DOMAIN: &[u8] = b"LayerX/programs/deployment-proof/v1\0";
const TRUST_HISTORY_DOMAIN: &[u8] = b"LayerX/sequencer-trust-history/v1\0";
const TRUST_ANCHOR_BYTES: usize = 103;
const MAX_TRUST_ANCHORS: usize = 64;
const MAX_TRUST_HISTORY_BYTES: usize =
    TRUST_HISTORY_DOMAIN.len() + 4 + MAX_TRUST_ANCHORS * TRUST_ANCHOR_BYTES;

/// One exact state-tree leaf and its index-aware membership proof.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StateLeafWitness {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
    pub proof: StateProof,
}

/// Canonical proof of the program lifecycle at one Programs root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProgramLifecycleProof {
    /// The wind-down status key is absent between the supplied adjacent leaves.
    Active {
        lower: Option<StateLeafWitness>,
        upper: Option<StateLeafWitness>,
    },
    /// The wind-down status key is present with its exact protocol record.
    Status(StateLeafWitness),
}

/// Receipt, signed-batch and state-tree material for one Programs state head.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramStateProof {
    pub receipt: Vec<u8>,
    pub receipt_proof: Proof,
    pub header: Vec<u8>,
    pub header_signature: [u8; 64],
    pub programs_root: [u8; 32],
    pub programs_root_proof: StateProof,
    pub program_record: StateLeafWitness,
    pub lifecycle: ProgramLifecycleProof,
}

/// Canonical deployment/upgrade activity plus its receipt and resulting state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeploymentProof {
    pub activity: Vec<u8>,
    pub activity_proof: Proof,
    pub state: ProgramStateProof,
}

/// Exact cryptographic refusal returned before deployment state is trusted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolEvidenceError {
    InvalidTrustAnchor,
    TrustHistoryUnavailable,
    TrustAnchorUnavailable,
    TrustAnchorAmbiguous,
    HistoricalTrustAnchor,
    ProtocolDomain,
    SequencerRevoked,
    CanonicalActivity,
    UnsupportedActivity,
    PayloadHash,
    ActivityInclusion,
    Receipt,
    ReceiptInclusion,
    BatchMismatch,
    BatchIdentifier,
    ActivityReceiptMismatch,
    StateRoot,
    StateProof,
    ProgramRecord,
    LifecycleProof,
    DeploymentMismatch,
    Stale,
    Encoding,
}

impl Display for ProtocolEvidenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidTrustAnchor => "deployment verifier trust anchor is invalid",
            Self::TrustHistoryUnavailable => {
                "canonical protected sequencer trust history is unavailable"
            }
            Self::TrustAnchorUnavailable => {
                "signed batch has no configured sequencer trust anchor"
            }
            Self::TrustAnchorAmbiguous => {
                "signed batch matches more than one sequencer trust anchor"
            }
            Self::HistoricalTrustAnchor => {
                "historical sequencer trust cannot authorize a current state head"
            }
            Self::ProtocolDomain => "evidence belongs to a different network, protocol, or epoch",
            Self::SequencerRevoked => "evidence was signed at or after sequencer-key revocation",
            Self::CanonicalActivity => "Programs activity is not canonical",
            Self::UnsupportedActivity => "activity is not a Programs deploy or upgrade",
            Self::PayloadHash => "Programs activity payload hash does not match",
            Self::ActivityInclusion => "Programs activity is not included by the signed batch",
            Self::Receipt => "Programs receipt is not a successful sequencer-signed receipt",
            Self::ReceiptInclusion => "Programs receipt is not included by the signed batch",
            Self::BatchMismatch => "activity, receipt, and state do not share one signed batch",
            Self::BatchIdentifier => "receipt batch identifier does not match the core execution context",
            Self::ActivityReceiptMismatch => "Programs receipt names a different activity",
            Self::StateRoot => "Programs root is not committed by the receipt state root",
            Self::StateProof => "Programs state membership proof is invalid",
            Self::ProgramRecord => "Programs registry record is non-canonical",
            Self::LifecycleProof => "Programs lifecycle proof is invalid",
            Self::DeploymentMismatch => "activity and resulting Programs record disagree",
            Self::Stale => "Programs evidence is outside the freshness bound",
            Self::Encoding => "deployment evidence encoding is non-canonical",
        })
    }
}

impl std::error::Error for ProtocolEvidenceError {}

/// Opaque deployment material admitted only after activity, receipt, batch,
/// state-root, registry-record and lifecycle verification all succeeded.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedDeploymentEvidence {
    record: DeploymentRecord,
    receipt_digest: [u8; 32],
    activity_id: [u8; 32],
    batch_header_digest: [u8; 32],
    state_root: [u8; 32],
    programs_root: [u8; 32],
    freshness: ReadFreshness,
    proof: DeploymentProof,
}

impl VerifiedDeploymentEvidence {
    #[must_use]
    pub const fn program(&self) -> ProgramId {
        self.record.program
    }

    #[must_use]
    pub const fn version(&self) -> u32 {
        self.record.version
    }

    #[must_use]
    pub const fn code_hash(&self) -> [u8; 32] {
        self.record.new_code_hash
    }

    #[must_use]
    pub const fn abi_version(&self) -> u16 {
        self.record.abi_version
    }

    #[must_use]
    pub const fn receipt_digest(&self) -> [u8; 32] {
        self.receipt_digest
    }

    #[must_use]
    pub const fn activity_id(&self) -> [u8; 32] {
        self.activity_id
    }

    #[must_use]
    pub const fn batch_header_digest(&self) -> [u8; 32] {
        self.batch_header_digest
    }

    #[must_use]
    pub const fn state_root(&self) -> [u8; 32] {
        self.state_root
    }

    #[must_use]
    pub const fn programs_root(&self) -> [u8; 32] {
        self.programs_root
    }

    #[must_use]
    pub const fn freshness(&self) -> ReadFreshness {
        self.freshness
    }

    #[must_use]
    pub const fn lifecycle(&self) -> ProgramLifecycle {
        ProgramLifecycle::Active
    }

    #[must_use]
    pub fn module(&self) -> &[u8] {
        &self.record.module
    }

    #[must_use]
    pub const fn record(&self) -> &DeploymentRecord {
        &self.record
    }

    #[must_use]
    pub const fn proof(&self) -> &DeploymentProof {
        &self.proof
    }
}

/// Opaque current state for one program. A catalog must consume one of these
/// for every admitted module before it can authorize an activity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedProgramHead {
    program: ProgramId,
    version: u32,
    code_hash: [u8; 32],
    abi_version: u16,
    policy: UpgradePolicy,
    lifecycle: ProgramLifecycle,
    receipt_digest: [u8; 32],
    state_root: [u8; 32],
    programs_root: [u8; 32],
    freshness: ReadFreshness,
    valid_until_ms: u64,
}

impl VerifiedProgramHead {
    #[must_use]
    pub const fn program(&self) -> ProgramId {
        self.program
    }

    #[must_use]
    pub const fn version(&self) -> u32 {
        self.version
    }

    #[must_use]
    pub const fn code_hash(&self) -> [u8; 32] {
        self.code_hash
    }

    #[must_use]
    pub const fn abi_version(&self) -> u16 {
        self.abi_version
    }

    #[must_use]
    pub const fn policy(&self) -> UpgradePolicy {
        self.policy
    }

    #[must_use]
    pub const fn lifecycle(&self) -> ProgramLifecycle {
        self.lifecycle
    }

    #[must_use]
    pub const fn receipt_digest(&self) -> [u8; 32] {
        self.receipt_digest
    }

    #[must_use]
    pub const fn state_root(&self) -> [u8; 32] {
        self.state_root
    }

    #[must_use]
    pub const fn programs_root(&self) -> [u8; 32] {
        self.programs_root
    }

    #[must_use]
    pub const fn freshness(&self) -> ReadFreshness {
        self.freshness
    }

    pub const fn valid_until_ms(&self) -> u64 {
        self.valid_until_ms
    }
}

/// Opaque current receipt head established under the one configured active
/// sequencer trust anchor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VerifiedProtocolHead {
    activity_id: [u8; 32],
    receipt_digest: [u8; 32],
    batch_header_digest: [u8; 32],
    state_root: [u8; 32],
    freshness: ReadFreshness,
    sequencer_public_key: [u8; 32],
}

impl VerifiedProtocolHead {
    #[must_use]
    pub const fn activity_id(&self) -> [u8; 32] {
        self.activity_id
    }

    #[must_use]
    pub const fn receipt_digest(&self) -> [u8; 32] {
        self.receipt_digest
    }

    #[must_use]
    pub const fn batch_header_digest(&self) -> [u8; 32] {
        self.batch_header_digest
    }

    #[must_use]
    pub const fn state_root(&self) -> [u8; 32] {
        self.state_root
    }

    #[must_use]
    pub const fn freshness(&self) -> ReadFreshness {
        self.freshness
    }

    #[must_use]
    pub const fn sequencer_public_key(&self) -> [u8; 32] {
        self.sequencer_public_key
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SequencerTrustAnchor {
    protocol_version: u16,
    network_id: u32,
    epoch: u64,
    sequencer_id: [u8; 32],
    sequencer_public_key: [u8; 32],
    first_batch: u64,
    last_batch: u64,
    revoked_from_batch: Option<u64>,
}

impl SequencerTrustAnchor {
    const fn effective_last_batch(self) -> u64 {
        match self.revoked_from_batch {
            Some(revoked) => {
                let last_valid = revoked - 1;
                if last_valid < self.last_batch {
                    last_valid
                } else {
                    self.last_batch
                }
            }
            None => self.last_batch,
        }
    }

    const fn authorization(self) -> SequencerAuthorization {
        SequencerAuthorization::new(
            self.sequencer_id,
            self.sequencer_public_key,
            self.first_batch,
            self.effective_last_batch(),
        )
    }
}

/// Verifier bound to a bounded, protected history of core-published
/// sequencer authorizations and one explicit current anchor.
#[derive(Clone, Debug)]
pub struct ProtocolDeploymentVerifier {
    anchors: Vec<SequencerTrustAnchor>,
    current_anchor: usize,
    staleness_limit_ms: u64,
}

impl ProtocolDeploymentVerifier {
    /// Loads one canonical bounded trust history from a private regular file.
    /// The proof request cannot add or replace any anchor selected here.
    ///
    /// # Errors
    ///
    /// Refuses missing, symlinked, non-private, oversized, non-canonical,
    /// overlapping or current-anchor-ambiguous histories.
    pub fn from_protected_history(
        path: &Path,
        staleness_limit_ms: u64,
    ) -> Result<Self, ProtocolEvidenceError> {
        if staleness_limit_ms == 0 {
            return Err(ProtocolEvidenceError::InvalidTrustAnchor);
        }
        let link_metadata = fs::symlink_metadata(path)
            .map_err(|_| ProtocolEvidenceError::TrustHistoryUnavailable)?;
        if !link_metadata.file_type().is_file() || link_metadata.file_type().is_symlink() {
            return Err(ProtocolEvidenceError::TrustHistoryUnavailable);
        }
        require_private_history(&link_metadata)?;
        let mut file = File::open(path)
            .map_err(|_| ProtocolEvidenceError::TrustHistoryUnavailable)?;
        let metadata = file
            .metadata()
            .map_err(|_| ProtocolEvidenceError::TrustHistoryUnavailable)?;
        require_private_history(&metadata)?;
        require_same_history_file(&link_metadata, &metadata)?;
        let length = usize::try_from(metadata.len())
            .map_err(|_| ProtocolEvidenceError::TrustHistoryUnavailable)?;
        if length == 0 || length > MAX_TRUST_HISTORY_BYTES {
            return Err(ProtocolEvidenceError::TrustHistoryUnavailable);
        }
        let mut bytes = Vec::with_capacity(length);
        file.read_to_end(&mut bytes)
            .map_err(|_| ProtocolEvidenceError::TrustHistoryUnavailable)?;
        if bytes.len() != length {
            return Err(ProtocolEvidenceError::TrustHistoryUnavailable);
        }
        let (anchors, current_anchor) = decode_trust_history(&bytes)?;
        Ok(Self {
            anchors,
            current_anchor,
            staleness_limit_ms,
        })
    }

    #[must_use]
    pub const fn staleness_limit_ms(&self) -> u64 {
        self.staleness_limit_ms
    }

    /// Verifies a canonical deploy or upgrade through its exact successful
    /// receipt and resulting Programs state before returning opaque evidence.
    pub fn verify_deployment(
        &self,
        proof: &DeploymentProof,
        now_ms: u64,
    ) -> Result<VerifiedDeploymentEvidence, ProtocolEvidenceError> {
        let activity = canonical_programs_activity(&proof.activity)?;
        let parsed = parse_lifecycle_activity(activity.activity_type().ordinal(), activity.payload())?;
        if payload_hash(&activity).map_err(|_| ProtocolEvidenceError::PayloadHash)?
            != activity.payload_hash()
        {
            return Err(ProtocolEvidenceError::PayloadHash);
        }
        let activity_identifier =
            activity_id(&activity).map_err(|_| ProtocolEvidenceError::CanonicalActivity)?;
        let head = self.verify_program_head(&proof.state, EvidenceMoment::Current(now_ms))?;
        let anchor = self.anchors[head.anchor_index];
        verify_activity_domain(&activity, anchor)?;
        let included = verify_activity(
            &proof.activity,
            &proof.activity_proof,
            &proof.state.header,
            &proof.state.header_signature,
            &anchor.authorization(),
        )
        .map_err(|_| ProtocolEvidenceError::ActivityInclusion)?;
        if included.header().digest() != head.batch_header_digest
            || head.activity_id != activity_identifier
        {
            return Err(if head.activity_id != activity_identifier {
                ProtocolEvidenceError::ActivityReceiptMismatch
            } else {
                ProtocolEvidenceError::BatchMismatch
            });
        }
        let record = bind_deployment(parsed, &head)?;
        Ok(VerifiedDeploymentEvidence {
            record,
            receipt_digest: head.receipt_digest,
            activity_id: activity_identifier,
            batch_header_digest: head.batch_header_digest,
            state_root: head.state_root,
            programs_root: head.programs_root,
            freshness: head.freshness,
            proof: proof.clone(),
        })
    }

    /// Re-verifies a durably stored historical deployment without pretending
    /// its old receipt is a current execution head. All cryptographic and state
    /// bindings remain mandatory; only the admission-time wall clock check is
    /// omitted.
    pub fn verify_historical_deployment(
        &self,
        proof: &DeploymentProof,
    ) -> Result<VerifiedDeploymentEvidence, ProtocolEvidenceError> {
        let activity = canonical_programs_activity(&proof.activity)?;
        let parsed = parse_lifecycle_activity(activity.activity_type().ordinal(), activity.payload())?;
        if payload_hash(&activity).map_err(|_| ProtocolEvidenceError::PayloadHash)?
            != activity.payload_hash()
        {
            return Err(ProtocolEvidenceError::PayloadHash);
        }
        let activity_identifier =
            activity_id(&activity).map_err(|_| ProtocolEvidenceError::CanonicalActivity)?;
        let head = self.verify_program_head(&proof.state, EvidenceMoment::Historical)?;
        let anchor = self.anchors[head.anchor_index];
        verify_activity_domain(&activity, anchor)?;
        let included = verify_activity(
            &proof.activity,
            &proof.activity_proof,
            &proof.state.header,
            &proof.state.header_signature,
            &anchor.authorization(),
        )
        .map_err(|_| ProtocolEvidenceError::ActivityInclusion)?;
        if included.header().digest() != head.batch_header_digest
            || head.activity_id != activity_identifier
        {
            return Err(if head.activity_id != activity_identifier {
                ProtocolEvidenceError::ActivityReceiptMismatch
            } else {
                ProtocolEvidenceError::BatchMismatch
            });
        }
        let record = bind_deployment(parsed, &head)?;
        Ok(VerifiedDeploymentEvidence {
            record,
            receipt_digest: head.receipt_digest,
            activity_id: activity_identifier,
            batch_header_digest: head.batch_header_digest,
            state_root: head.state_root,
            programs_root: head.programs_root,
            freshness: head.freshness,
            proof: proof.clone(),
        })
    }

    /// Verifies one fresh Programs registry/lifecycle view under a successful,
    /// batch-included, sequencer-signed state receipt.
    pub fn verify_current_program(
        &self,
        proof: &ProgramStateProof,
        expected_program: ProgramId,
        now_ms: u64,
    ) -> Result<VerifiedProgramHead, ProtocolEvidenceError> {
        let head = self.verify_program_head(proof, EvidenceMoment::Current(now_ms))?;
        if head.record.program != expected_program {
            return Err(ProtocolEvidenceError::ProgramRecord);
        }
        let valid_until_ms = head
            .freshness
            .observed_at
            .checked_add(self.staleness_limit_ms)
            .ok_or(ProtocolEvidenceError::Stale)?;
        Ok(VerifiedProgramHead {
            program: head.record.program,
            version: head.record.version,
            code_hash: head.record.code_hash,
            abi_version: head.record.abi_version,
            policy: head.record.policy,
            lifecycle: head.lifecycle,
            receipt_digest: head.receipt_digest,
            state_root: head.state_root,
            programs_root: head.programs_root,
            freshness: head.freshness,
            valid_until_ms,
        })
    }

    /// Verifies one fresh protocol receipt under the explicitly configured
    /// current trust anchor.
    pub fn verify_current_protocol_head(
        &self,
        receipt: &[u8],
        receipt_proof: &Proof,
        header: &[u8],
        header_signature: &[u8; 64],
        now_ms: u64,
    ) -> Result<VerifiedProtocolHead, ProtocolEvidenceError> {
        self.verify_protocol_head(
            receipt,
            receipt_proof,
            header,
            header_signature,
            EvidenceMoment::Current(now_ms),
        )
    }

    /// Verifies a historical protocol receipt under the exact trust anchor
    /// selected from its signed header, without applying current-head age.
    pub fn verify_historical_protocol_head(
        &self,
        receipt: &[u8],
        receipt_proof: &Proof,
        header: &[u8],
        header_signature: &[u8; 64],
    ) -> Result<VerifiedProtocolHead, ProtocolEvidenceError> {
        self.verify_protocol_head(
            receipt,
            receipt_proof,
            header,
            header_signature,
            EvidenceMoment::Historical,
        )
    }

    fn verify_protocol_head(
        &self,
        receipt: &[u8],
        receipt_proof: &Proof,
        header: &[u8],
        header_signature: &[u8; 64],
        moment: EvidenceMoment,
    ) -> Result<VerifiedProtocolHead, ProtocolEvidenceError> {
        let claims = self.verify_receipt(
            receipt,
            receipt_proof,
            header,
            header_signature,
            moment,
        )?;
        Ok(VerifiedProtocolHead {
            activity_id: claims.activity_id,
            receipt_digest: claims.receipt_digest,
            batch_header_digest: claims.batch_header_digest,
            state_root: claims.state_root,
            freshness: claims.freshness,
            sequencer_public_key: self.anchors[claims.anchor_index].sequencer_public_key,
        })
    }

    fn verify_program_head(
        &self,
        proof: &ProgramStateProof,
        moment: EvidenceMoment,
    ) -> Result<VerifiedHeadClaims, ProtocolEvidenceError> {
        let receipt = self.verify_receipt(
            &proof.receipt,
            &proof.receipt_proof,
            &proof.header,
            &proof.header_signature,
            moment,
        )?;
        if proof.programs_root == [0; 32] {
            return Err(ProtocolEvidenceError::StateRoot);
        }
        verify_state_membership(
            &PROGRAMS_MODULE_ID.to_be_bytes(),
            &proof.programs_root,
            &proof.programs_root_proof,
            receipt.state_root,
        )
        .map_err(|_| ProtocolEvidenceError::StateRoot)?;
        verify_witness(&proof.program_record, proof.programs_root)?;
        let record = decode_program_record(&proof.program_record)?;
        let lifecycle = verify_lifecycle(record.program, &proof.lifecycle, proof.programs_root)?;
        Ok(VerifiedHeadClaims {
            record,
            lifecycle,
            activity_id: receipt.activity_id,
            receipt_digest: receipt.receipt_digest,
            batch_header_digest: receipt.batch_header_digest,
            state_root: receipt.state_root,
            programs_root: proof.programs_root,
            freshness: receipt.freshness,
            anchor_index: receipt.anchor_index,
        })
    }

    fn verify_receipt(
        &self,
        receipt: &[u8],
        receipt_proof: &Proof,
        header_bytes: &[u8],
        header_signature: &[u8; 64],
        moment: EvidenceMoment,
    ) -> Result<VerifiedReceiptClaims, ProtocolEvidenceError> {
        let selected = self.select_anchor(header_bytes, moment)?;
        let anchor = self.anchors[selected];
        let decoded = decode_receipt(receipt).map_err(|_| ProtocolEvidenceError::Receipt)?;
        let protocol = decoded.protocol().ok_or(ProtocolEvidenceError::Receipt)?;
        let inclusion = verify_receipt_inclusion(
            receipt,
            receipt_proof,
            header_bytes,
            header_signature,
            &anchor.authorization(),
        )
        .map_err(|_| ProtocolEvidenceError::ReceiptInclusion)?;
        let header = inclusion.header().header();
        if protocol.global_sequence() < header.first_sequence()
            || protocol.global_sequence() > header.last_sequence()
        {
            return Err(ProtocolEvidenceError::BatchMismatch);
        }
        let expected_batch_id = execution_batch_id(
            header.previous_state_root(),
            protocol.activity_id(),
            protocol.global_sequence(),
            header.batch_number(),
        )
        .map_err(|_| ProtocolEvidenceError::BatchIdentifier)?;
        if protocol.batch_id() != expected_batch_id {
            return Err(ProtocolEvidenceError::BatchIdentifier);
        }
        let authorized = AuthorizedBatch::new(
            protocol.batch_id(),
            protocol.asset(),
            header.previous_state_root(),
            header.resulting_state_root(),
            anchor.sequencer_public_key,
        );
        let verified = verify_program_state(receipt, &authorized)
            .map_err(|_| ProtocolEvidenceError::Receipt)?;
        let verified_protocol = verified.receipt().protocol().ok_or(ProtocolEvidenceError::Receipt)?;
        if verified_protocol.resulting_state_root() != header.resulting_state_root()
            || verified_protocol.protocol_version() != header.protocol_version()
            || verified_protocol.timestamp() != header.timestamp_ms()
            || verified_protocol.activity_root() != header.activity_merkle_root()
        {
            return Err(ProtocolEvidenceError::StateRoot);
        }
        if verified_protocol.timestamp() == 0 {
            return Err(ProtocolEvidenceError::Stale);
        }
        if let EvidenceMoment::Current(now_ms) = moment {
            if now_ms == 0
                || now_ms < verified_protocol.timestamp()
                || now_ms.saturating_sub(verified_protocol.timestamp()) > self.staleness_limit_ms
            {
                return Err(ProtocolEvidenceError::Stale);
            }
        }
        let unsigned = encode_unsigned(verified.receipt()).map_err(|_| ProtocolEvidenceError::Receipt)?;
        let digest = receipt_digest(&unsigned).map_err(|_| ProtocolEvidenceError::Receipt)?;
        Ok(VerifiedReceiptClaims {
            activity_id: verified_protocol.activity_id(),
            receipt_digest: digest,
            batch_header_digest: inclusion.header().digest(),
            state_root: verified_protocol.resulting_state_root(),
            freshness: ReadFreshness {
                observed_sequence: verified_protocol.global_sequence(),
                observed_at: verified_protocol.timestamp(),
            },
            anchor_index: selected,
        })
    }

    fn select_anchor(
        &self,
        header_bytes: &[u8],
        moment: EvidenceMoment,
    ) -> Result<usize, ProtocolEvidenceError> {
        let header = decode_batch_header(header_bytes)
            .map_err(|_| ProtocolEvidenceError::ReceiptInclusion)?;
        let mut selected = None;
        let mut revoked = false;
        for (index, anchor) in self.anchors.iter().copied().enumerate() {
            if header.protocol_version() == anchor.protocol_version
                && header.network_id() == anchor.network_id
                && header.epoch() == anchor.epoch
                && header.sequencer_id() == anchor.sequencer_id
                && header.batch_number() >= anchor.first_batch
                && header.batch_number() <= anchor.last_batch
            {
                if anchor
                    .revoked_from_batch
                    .is_some_and(|batch| header.batch_number() >= batch)
                {
                    revoked = true;
                    continue;
                }
                if selected.replace(index).is_some() {
                    return Err(ProtocolEvidenceError::TrustAnchorAmbiguous);
                }
            }
        }
        let selected = match selected {
            Some(index) => index,
            None if revoked => return Err(ProtocolEvidenceError::SequencerRevoked),
            None => return Err(ProtocolEvidenceError::TrustAnchorUnavailable),
        };
        if matches!(moment, EvidenceMoment::Current(_)) && selected != self.current_anchor {
            return Err(ProtocolEvidenceError::HistoricalTrustAnchor);
        }
        Ok(selected)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ProgramRecord {
    program: ProgramId,
    version: u32,
    code_hash: [u8; 32],
    abi_version: u16,
    policy: UpgradePolicy,
}

struct VerifiedHeadClaims {
    record: ProgramRecord,
    lifecycle: ProgramLifecycle,
    activity_id: [u8; 32],
    receipt_digest: [u8; 32],
    batch_header_digest: [u8; 32],
    state_root: [u8; 32],
    programs_root: [u8; 32],
    freshness: ReadFreshness,
    anchor_index: usize,
}

#[derive(Clone, Copy)]
enum EvidenceMoment {
    Current(u64),
    Historical,
}

struct VerifiedReceiptClaims {
    activity_id: [u8; 32],
    receipt_digest: [u8; 32],
    batch_header_digest: [u8; 32],
    state_root: [u8; 32],
    freshness: ReadFreshness,
    anchor_index: usize,
}

fn verify_activity_domain(
    activity: &layerx_wire::activity::Activity,
    anchor: SequencerTrustAnchor,
) -> Result<(), ProtocolEvidenceError> {
    if activity.protocol_version() != anchor.protocol_version
        || activity.network_id() != anchor.network_id
    {
        return Err(ProtocolEvidenceError::ProtocolDomain);
    }
    Ok(())
}

fn decode_trust_history(
    bytes: &[u8],
) -> Result<(Vec<SequencerTrustAnchor>, usize), ProtocolEvidenceError> {
    if bytes.len() > MAX_TRUST_HISTORY_BYTES
        || bytes.get(..TRUST_HISTORY_DOMAIN.len()) != Some(TRUST_HISTORY_DOMAIN)
    {
        return Err(ProtocolEvidenceError::InvalidTrustAnchor);
    }
    let mut cursor = TRUST_HISTORY_DOMAIN.len();
    let count = usize::from(u16::from_be_bytes(history_array::<2>(bytes, &mut cursor)?));
    let current_anchor = usize::from(u16::from_be_bytes(history_array::<2>(bytes, &mut cursor)?));
    if count == 0
        || count > MAX_TRUST_ANCHORS
        || current_anchor >= count
        || bytes.len().checked_sub(cursor) != count.checked_mul(TRUST_ANCHOR_BYTES)
    {
        return Err(ProtocolEvidenceError::InvalidTrustAnchor);
    }
    let mut anchors = Vec::with_capacity(count);
    let mut previous_entry: Option<&[u8]> = None;
    for _ in 0..count {
        let entry_start = cursor;
        let protocol_version = u16::from_be_bytes(history_array::<2>(bytes, &mut cursor)?);
        let network_id = u32::from_be_bytes(history_array::<4>(bytes, &mut cursor)?);
        let epoch = u64::from_be_bytes(history_array::<8>(bytes, &mut cursor)?);
        let sequencer_id = history_array::<32>(bytes, &mut cursor)?;
        let sequencer_public_key = history_array::<32>(bytes, &mut cursor)?;
        let first_batch = u64::from_be_bytes(history_array::<8>(bytes, &mut cursor)?);
        let last_batch = u64::from_be_bytes(history_array::<8>(bytes, &mut cursor)?);
        let revoked_marker = history_array::<1>(bytes, &mut cursor)?[0];
        let revoked_value = u64::from_be_bytes(history_array::<8>(bytes, &mut cursor)?);
        let revoked_from_batch = match (revoked_marker, revoked_value) {
            (0, 0) => None,
            (1, value) if value > first_batch => Some(value),
            _ => return Err(ProtocolEvidenceError::InvalidTrustAnchor),
        };
        let entry = bytes
            .get(entry_start..cursor)
            .ok_or(ProtocolEvidenceError::InvalidTrustAnchor)?;
        if previous_entry.is_some_and(|previous| previous >= entry)
            || !matches!(protocol_version, 1 | 2)
            || network_id == 0
            || epoch == 0
            || sequencer_id == [0; 32]
            || sequencer_public_key == [0; 32]
            || first_batch == 0
            || last_batch < first_batch
        {
            return Err(ProtocolEvidenceError::InvalidTrustAnchor);
        }
        previous_entry = Some(entry);
        anchors.push(SequencerTrustAnchor {
            protocol_version,
            network_id,
            epoch,
            sequencer_id,
            sequencer_public_key,
            first_batch,
            last_batch,
            revoked_from_batch,
        });
    }
    if cursor != bytes.len() {
        return Err(ProtocolEvidenceError::InvalidTrustAnchor);
    }
    let current = anchors[current_anchor];
    if anchors.iter().any(|anchor| anchor.network_id != current.network_id) {
        return Err(ProtocolEvidenceError::InvalidTrustAnchor);
    }
    let current_position = (
        current.epoch,
        current.first_batch,
        current.protocol_version,
    );
    if anchors.iter().any(|anchor| {
        (anchor.epoch, anchor.first_batch, anchor.protocol_version) > current_position
    }) {
        return Err(ProtocolEvidenceError::InvalidTrustAnchor);
    }
    for (index, left) in anchors.iter().copied().enumerate() {
        for right in anchors.iter().copied().skip(index + 1) {
            if left.protocol_version == right.protocol_version
                && left.network_id == right.network_id
                && left.epoch == right.epoch
                && left.sequencer_id == right.sequencer_id
                && left.first_batch <= right.effective_last_batch()
                && right.first_batch <= left.effective_last_batch()
            {
                return Err(ProtocolEvidenceError::TrustAnchorAmbiguous);
            }
        }
    }
    Ok((anchors, current_anchor))
}

fn history_array<const N: usize>(
    bytes: &[u8],
    cursor: &mut usize,
) -> Result<[u8; N], ProtocolEvidenceError> {
    let end = cursor
        .checked_add(N)
        .ok_or(ProtocolEvidenceError::InvalidTrustAnchor)?;
    let value = bytes
        .get(*cursor..end)
        .and_then(|slice| slice.try_into().ok())
        .ok_or(ProtocolEvidenceError::InvalidTrustAnchor)?;
    *cursor = end;
    Ok(value)
}

#[cfg(unix)]
fn require_private_history(metadata: &fs::Metadata) -> Result<(), ProtocolEvidenceError> {
    use std::os::unix::fs::PermissionsExt as _;

    if metadata.permissions().mode() & 0o077 != 0 {
        return Err(ProtocolEvidenceError::TrustHistoryUnavailable);
    }
    Ok(())
}

#[cfg(unix)]
fn require_same_history_file(
    path_metadata: &fs::Metadata,
    file_metadata: &fs::Metadata,
) -> Result<(), ProtocolEvidenceError> {
    use std::os::unix::fs::MetadataExt as _;

    if path_metadata.dev() != file_metadata.dev() || path_metadata.ino() != file_metadata.ino() {
        return Err(ProtocolEvidenceError::TrustHistoryUnavailable);
    }
    Ok(())
}

#[cfg(not(unix))]
fn require_private_history(_metadata: &fs::Metadata) -> Result<(), ProtocolEvidenceError> {
    Err(ProtocolEvidenceError::TrustHistoryUnavailable)
}

#[cfg(not(unix))]
fn require_same_history_file(
    _path_metadata: &fs::Metadata,
    _file_metadata: &fs::Metadata,
) -> Result<(), ProtocolEvidenceError> {
    Err(ProtocolEvidenceError::TrustHistoryUnavailable)
}

enum LifecycleActivity {
    Deploy {
        program: ProgramId,
        abi_version: u16,
        policy: UpgradePolicy,
        new_code_hash: [u8; 32],
        module: Vec<u8>,
    },
    Upgrade {
        program: ProgramId,
        abi_version: u16,
        old_code_hash: [u8; 32],
        new_code_hash: [u8; 32],
        module: Vec<u8>,
    },
}

fn canonical_programs_activity(
    bytes: &[u8],
) -> Result<layerx_wire::activity::Activity, ProtocolEvidenceError> {
    let deploy = ActivityType::new(ModuleId::Programs, DEPLOY_ORDINAL)
        .map_err(|_| ProtocolEvidenceError::UnsupportedActivity)?;
    let upgrade = ActivityType::new(ModuleId::Programs, UPGRADE_ORDINAL)
        .map_err(|_| ProtocolEvidenceError::UnsupportedActivity)?;
    let registration = ModuleRegistration::new(ModuleId::Programs, &[deploy, upgrade])
        .map_err(|_| ProtocolEvidenceError::UnsupportedActivity)?;
    let registry = ModuleRegistry::new(&[registration])
        .map_err(|_| ProtocolEvidenceError::UnsupportedActivity)?;
    let activity = decode_signed(bytes, &registry)
        .map_err(|_| ProtocolEvidenceError::CanonicalActivity)?;
    if encode_signed(&activity).map_err(|_| ProtocolEvidenceError::CanonicalActivity)? != bytes {
        return Err(ProtocolEvidenceError::CanonicalActivity);
    }
    Ok(activity)
}

fn parse_lifecycle_activity(
    ordinal: u16,
    payload: &[u8],
) -> Result<LifecycleActivity, ProtocolEvidenceError> {
    let program = ProgramId::new(array::<32>(payload, 0)?)
        .map_err(|_| ProtocolEvidenceError::CanonicalActivity)?;
    let abi_version = u16::from_be_bytes(array::<2>(payload, 32)?);
    if !matches!(abi_version, ABI_V1_VERSION | ABI_V2_VERSION) {
        return Err(ProtocolEvidenceError::CanonicalActivity);
    }
    match ordinal {
        DEPLOY_ORDINAL => {
            if payload.len() < 104 || payload[35] != 0 {
                return Err(ProtocolEvidenceError::CanonicalActivity);
            }
            let authority = array::<32>(payload, 36)?;
            let policy = match payload[34] {
                0 if authority == [0; 32] => UpgradePolicy::Immutable,
                1 if authority != [0; 32] => UpgradePolicy::Authority(authority),
                _ => return Err(ProtocolEvidenceError::CanonicalActivity),
            };
            let new_code_hash = array::<32>(payload, 68)?;
            let wasm_length = usize::try_from(u32::from_be_bytes(array::<4>(payload, 100)?))
                .map_err(|_| ProtocolEvidenceError::CanonicalActivity)?;
            if wasm_length == 0
                || wasm_length > MAX_MODULE_BYTES
                || payload.len().checked_sub(104) != Some(wasm_length)
            {
                return Err(ProtocolEvidenceError::CanonicalActivity);
            }
            let module = payload[104..].to_vec();
            if module.get(..WASM_HEADER.len()) != Some(WASM_HEADER) {
                return Err(ProtocolEvidenceError::CanonicalActivity);
            }
            if crate::hash::sha256(&module) != new_code_hash {
                return Err(ProtocolEvidenceError::DeploymentMismatch);
            }
            Ok(LifecycleActivity::Deploy {
                program,
                abi_version,
                policy,
                new_code_hash,
                module,
            })
        }
        UPGRADE_ORDINAL => {
            if payload.len() < 106 || payload[35] != 0 || payload[34] & 0xfe != 0 {
                return Err(ProtocolEvidenceError::CanonicalActivity);
            }
            let old_code_hash = array::<32>(payload, 36)?;
            let new_code_hash = array::<32>(payload, 68)?;
            let hook_length = usize::from(u16::from_be_bytes(array::<2>(payload, 100)?));
            let wasm_length = usize::try_from(u32::from_be_bytes(array::<4>(payload, 102)?))
                .map_err(|_| ProtocolEvidenceError::CanonicalActivity)?;
            let variable = hook_length
                .checked_add(wasm_length)
                .ok_or(ProtocolEvidenceError::CanonicalActivity)?;
            if wasm_length == 0
                || wasm_length > MAX_MODULE_BYTES
                || payload.len().checked_sub(106) != Some(variable)
                || ((payload[34] & 1) == 0) != (hook_length == 0)
            {
                return Err(ProtocolEvidenceError::CanonicalActivity);
            }
            let module = payload[106 + hook_length..].to_vec();
            if module.get(..WASM_HEADER.len()) != Some(WASM_HEADER) {
                return Err(ProtocolEvidenceError::CanonicalActivity);
            }
            if crate::hash::sha256(&module) != new_code_hash {
                return Err(ProtocolEvidenceError::DeploymentMismatch);
            }
            Ok(LifecycleActivity::Upgrade {
                program,
                abi_version,
                old_code_hash,
                new_code_hash,
                module,
            })
        }
        _ => Err(ProtocolEvidenceError::UnsupportedActivity),
    }
}

fn bind_deployment(
    activity: LifecycleActivity,
    head: &VerifiedHeadClaims,
) -> Result<DeploymentRecord, ProtocolEvidenceError> {
    if head.lifecycle != ProgramLifecycle::Active {
        return Err(ProtocolEvidenceError::LifecycleProof);
    }
    let (program, abi_version, policy, old_code_hash, new_code_hash, module) = match activity {
        LifecycleActivity::Deploy {
            program,
            abi_version,
            policy,
            new_code_hash,
            module,
        } => {
            if head.record.version != 1 || head.record.policy != policy {
                return Err(ProtocolEvidenceError::DeploymentMismatch);
            }
            (program, abi_version, policy, None, new_code_hash, module)
        }
        LifecycleActivity::Upgrade {
            program,
            abi_version,
            old_code_hash,
            new_code_hash,
            module,
        } => {
            if head.record.version <= 1
                || !matches!(head.record.policy, UpgradePolicy::Authority(authority) if authority != [0; 32])
            {
                return Err(ProtocolEvidenceError::DeploymentMismatch);
            }
            (
                program,
                abi_version,
                head.record.policy,
                Some(old_code_hash),
                new_code_hash,
                module,
            )
        }
    };
    if program != head.record.program
        || abi_version != head.record.abi_version
        || new_code_hash != head.record.code_hash
    {
        return Err(ProtocolEvidenceError::DeploymentMismatch);
    }
    Ok(DeploymentRecord {
        program,
        version: head.record.version,
        abi_version,
        upgrade_policy: policy,
        old_code_hash,
        new_code_hash,
        sequence: head.freshness.observed_sequence,
        observed_at: head.freshness.observed_at,
        module,
        migration: None,
    })
}

fn decode_program_record(
    witness: &StateLeafWitness,
) -> Result<ProgramRecord, ProtocolEvidenceError> {
    if witness.key.len() != PROGRAM_KEY_PREFIX.len() + 32
        || witness.key.get(..PROGRAM_KEY_PREFIX.len()) != Some(PROGRAM_KEY_PREFIX)
        || witness.value.len() != PROGRAM_RECORD_BYTES
    {
        return Err(ProtocolEvidenceError::ProgramRecord);
    }
    let program = ProgramId::new(
        witness.key[PROGRAM_KEY_PREFIX.len()..]
            .try_into()
            .map_err(|_| ProtocolEvidenceError::ProgramRecord)?,
    )
    .map_err(|_| ProtocolEvidenceError::ProgramRecord)?;
    let authority: [u8; 32] = witness.value[1..33]
        .try_into()
        .map_err(|_| ProtocolEvidenceError::ProgramRecord)?;
    let policy = match witness.value[0] {
        0 if authority == [0; 32] => UpgradePolicy::Immutable,
        1 if authority != [0; 32] => UpgradePolicy::Authority(authority),
        _ => return Err(ProtocolEvidenceError::ProgramRecord),
    };
    let code_hash = witness.value[33..65]
        .try_into()
        .map_err(|_| ProtocolEvidenceError::ProgramRecord)?;
    let abi_version = u16::from_be_bytes(
        witness.value[65..67]
            .try_into()
            .map_err(|_| ProtocolEvidenceError::ProgramRecord)?,
    );
    let version = u32::from_be_bytes(
        witness.value[67..71]
            .try_into()
            .map_err(|_| ProtocolEvidenceError::ProgramRecord)?,
    );
    if code_hash == [0; 32]
        || !matches!(abi_version, ABI_V1_VERSION | ABI_V2_VERSION)
        || version == 0
    {
        return Err(ProtocolEvidenceError::ProgramRecord);
    }
    Ok(ProgramRecord {
        program,
        version,
        code_hash,
        abi_version,
        policy,
    })
}

fn verify_lifecycle(
    program: ProgramId,
    proof: &ProgramLifecycleProof,
    programs_root: [u8; 32],
) -> Result<ProgramLifecycle, ProtocolEvidenceError> {
    let mut target = Vec::with_capacity(STATUS_KEY_PREFIX.len() + 32);
    target.extend_from_slice(STATUS_KEY_PREFIX);
    target.extend_from_slice(&program.bytes());
    match proof {
        ProgramLifecycleProof::Active { lower, upper } => {
            verify_absence(&target, lower.as_ref(), upper.as_ref(), programs_root)?;
            Ok(ProgramLifecycle::Active)
        }
        ProgramLifecycleProof::Status(witness) => {
            if witness.key != target || witness.value.len() != STATUS_RECORD_BYTES {
                return Err(ProtocolEvidenceError::LifecycleProof);
            }
            verify_witness(witness, programs_root)?;
            if witness.value[0] != 1
                || witness.value[2..34] != program.bytes()
                || u64::from_be_bytes(
                    witness.value[34..42]
                        .try_into()
                        .map_err(|_| ProtocolEvidenceError::LifecycleProof)?,
                ) == 0
                || u64::from_be_bytes(
                    witness.value[42..50]
                        .try_into()
                        .map_err(|_| ProtocolEvidenceError::LifecycleProof)?,
                ) == 0
            {
                return Err(ProtocolEvidenceError::LifecycleProof);
            }
            match witness.value[1] {
                2 => Ok(ProgramLifecycle::Deprecated),
                3 => Ok(ProgramLifecycle::Tombstoned),
                _ => Err(ProtocolEvidenceError::LifecycleProof),
            }
        }
    }
}

fn verify_absence(
    target: &[u8],
    lower: Option<&StateLeafWitness>,
    upper: Option<&StateLeafWitness>,
    root: [u8; 32],
) -> Result<(), ProtocolEvidenceError> {
    if lower.is_none() && upper.is_none() {
        return Err(ProtocolEvidenceError::LifecycleProof);
    }
    if let Some(value) = lower {
        verify_witness(value, root)?;
        if value.key.as_slice() >= target {
            return Err(ProtocolEvidenceError::LifecycleProof);
        }
    }
    if let Some(value) = upper {
        verify_witness(value, root)?;
        if value.key.as_slice() <= target {
            return Err(ProtocolEvidenceError::LifecycleProof);
        }
    }
    match (lower, upper) {
        (Some(left), Some(right)) => {
            if left.proof.leaf_count != right.proof.leaf_count
                || left.proof.leaf_index.checked_add(1) != Some(right.proof.leaf_index)
            {
                return Err(ProtocolEvidenceError::LifecycleProof);
            }
        }
        (Some(left), None) => {
            if left.proof.leaf_index.checked_add(1) != Some(left.proof.leaf_count) {
                return Err(ProtocolEvidenceError::LifecycleProof);
            }
        }
        (None, Some(right)) => {
            if right.proof.leaf_index != 0 {
                return Err(ProtocolEvidenceError::LifecycleProof);
            }
        }
        (None, None) => return Err(ProtocolEvidenceError::LifecycleProof),
    }
    Ok(())
}

fn verify_witness(
    witness: &StateLeafWitness,
    root: [u8; 32],
) -> Result<(), ProtocolEvidenceError> {
    if witness.key.is_empty()
        || u32::try_from(witness.key.len()).is_err()
        || u32::try_from(witness.value.len()).is_err()
    {
        return Err(ProtocolEvidenceError::StateProof);
    }
    verify_state_membership(&witness.key, &witness.value, &witness.proof, root)
        .map_err(|_| ProtocolEvidenceError::StateProof)
}

fn array<const N: usize>(
    bytes: &[u8],
    offset: usize,
) -> Result<[u8; N], ProtocolEvidenceError> {
    bytes
        .get(offset..offset.saturating_add(N))
        .and_then(|value| value.try_into().ok())
        .ok_or(ProtocolEvidenceError::CanonicalActivity)
}

impl DeploymentProof {
    /// Encodes the untrusted proof material for durable replay. The encoding
    /// does not itself confer verification authority.
    #[must_use]
    pub fn canonical_encoding(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(EVIDENCE_DOMAIN);
        put_bytes(&mut bytes, &self.activity);
        put_bytes(&mut bytes, &encode_proof(&self.activity_proof));
        encode_program_state(&mut bytes, &self.state);
        bytes
    }

    /// Decodes untrusted proof material. Callers must pass the result through
    /// [`ProtocolDeploymentVerifier`] before using any claim it contains.
    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolEvidenceError> {
        if bytes.len() > MAX_EVIDENCE_BYTES
            || bytes.get(..EVIDENCE_DOMAIN.len()) != Some(EVIDENCE_DOMAIN)
        {
            return Err(ProtocolEvidenceError::Encoding);
        }
        let mut cursor = EVIDENCE_DOMAIN.len();
        let activity = take_bytes(bytes, &mut cursor, MAX_EVIDENCE_BYTES)?;
        let activity_proof = decode_proof(&take_bytes(bytes, &mut cursor, 1_034)?)
            .map_err(|_| ProtocolEvidenceError::Encoding)?;
        let state = decode_program_state(bytes, &mut cursor)?;
        if cursor != bytes.len() {
            return Err(ProtocolEvidenceError::Encoding);
        }
        let proof = Self {
            activity,
            activity_proof,
            state,
        };
        if proof.canonical_encoding() != bytes {
            return Err(ProtocolEvidenceError::Encoding);
        }
        Ok(proof)
    }

    /// Computes the canonical unsigned receipt digest used only to address
    /// stored proof material. This does not verify the receipt signature.
    pub fn claimed_receipt_digest(&self) -> Result<[u8; 32], ProtocolEvidenceError> {
        let receipt = decode_receipt(&self.state.receipt)
            .map_err(|_| ProtocolEvidenceError::Receipt)?;
        let unsigned = encode_unsigned(&receipt).map_err(|_| ProtocolEvidenceError::Receipt)?;
        receipt_digest(&unsigned).map_err(|_| ProtocolEvidenceError::Receipt)
    }
}

fn encode_program_state(bytes: &mut Vec<u8>, state: &ProgramStateProof) {
    put_bytes(bytes, &state.receipt);
    put_bytes(bytes, &encode_proof(&state.receipt_proof));
    put_bytes(bytes, &state.header);
    bytes.extend_from_slice(&state.header_signature);
    bytes.extend_from_slice(&state.programs_root);
    put_state_proof(bytes, &state.programs_root_proof);
    put_witness(bytes, &state.program_record);
    match &state.lifecycle {
        ProgramLifecycleProof::Active { lower, upper } => {
            bytes.push(0);
            put_optional_witness(bytes, lower.as_ref());
            put_optional_witness(bytes, upper.as_ref());
        }
        ProgramLifecycleProof::Status(witness) => {
            bytes.push(1);
            put_witness(bytes, witness);
        }
    }
}

fn decode_program_state(
    bytes: &[u8],
    cursor: &mut usize,
) -> Result<ProgramStateProof, ProtocolEvidenceError> {
    let receipt = take_bytes(bytes, cursor, MAX_EVIDENCE_BYTES)?;
    let receipt_proof = decode_proof(&take_bytes(bytes, cursor, 1_034)?)
        .map_err(|_| ProtocolEvidenceError::Encoding)?;
    let header = take_bytes(bytes, cursor, 4_096)?;
    let header_signature = take_array::<64>(bytes, cursor)?;
    let programs_root = take_array::<32>(bytes, cursor)?;
    let programs_root_proof = take_state_proof(bytes, cursor)?;
    let program_record = take_witness(bytes, cursor)?;
    let lifecycle = match take_array::<1>(bytes, cursor)?[0] {
        0 => ProgramLifecycleProof::Active {
            lower: take_optional_witness(bytes, cursor)?,
            upper: take_optional_witness(bytes, cursor)?,
        },
        1 => ProgramLifecycleProof::Status(take_witness(bytes, cursor)?),
        _ => return Err(ProtocolEvidenceError::Encoding),
    };
    Ok(ProgramStateProof {
        receipt,
        receipt_proof,
        header,
        header_signature,
        programs_root,
        programs_root_proof,
        program_record,
        lifecycle,
    })
}

fn put_optional_witness(bytes: &mut Vec<u8>, witness: Option<&StateLeafWitness>) {
    bytes.push(u8::from(witness.is_some()));
    if let Some(value) = witness {
        put_witness(bytes, value);
    }
}

fn take_optional_witness(
    bytes: &[u8],
    cursor: &mut usize,
) -> Result<Option<StateLeafWitness>, ProtocolEvidenceError> {
    match take_array::<1>(bytes, cursor)?[0] {
        0 => Ok(None),
        1 => take_witness(bytes, cursor).map(Some),
        _ => Err(ProtocolEvidenceError::Encoding),
    }
}

fn put_witness(bytes: &mut Vec<u8>, witness: &StateLeafWitness) {
    put_bytes(bytes, &witness.key);
    put_bytes(bytes, &witness.value);
    put_state_proof(bytes, &witness.proof);
}

fn take_witness(
    bytes: &[u8],
    cursor: &mut usize,
) -> Result<StateLeafWitness, ProtocolEvidenceError> {
    Ok(StateLeafWitness {
        key: take_bytes(bytes, cursor, 4_096)?,
        value: take_bytes(bytes, cursor, MAX_MODULE_BYTES)?,
        proof: take_state_proof(bytes, cursor)?,
    })
}

fn put_state_proof(bytes: &mut Vec<u8>, proof: &StateProof) {
    bytes.extend_from_slice(&proof.leaf_index.to_be_bytes());
    bytes.extend_from_slice(&proof.leaf_count.to_be_bytes());
    bytes.push(u8::try_from(proof.siblings.len()).unwrap_or(u8::MAX));
    for sibling in &proof.siblings {
        bytes.extend_from_slice(sibling);
    }
}

fn take_state_proof(
    bytes: &[u8],
    cursor: &mut usize,
) -> Result<StateProof, ProtocolEvidenceError> {
    let leaf_index = u32::from_be_bytes(take_array::<4>(bytes, cursor)?);
    let leaf_count = u32::from_be_bytes(take_array::<4>(bytes, cursor)?);
    let count = usize::from(take_array::<1>(bytes, cursor)?[0]);
    if count > 32 {
        return Err(ProtocolEvidenceError::Encoding);
    }
    let mut siblings = Vec::with_capacity(count);
    for _ in 0..count {
        siblings.push(take_array::<32>(bytes, cursor)?);
    }
    Ok(StateProof {
        leaf_index,
        leaf_count,
        siblings,
    })
}

fn put_bytes(bytes: &mut Vec<u8>, value: &[u8]) {
    bytes.extend_from_slice(&u32::try_from(value.len()).unwrap_or(u32::MAX).to_be_bytes());
    bytes.extend_from_slice(value);
}

fn take_bytes(
    bytes: &[u8],
    cursor: &mut usize,
    limit: usize,
) -> Result<Vec<u8>, ProtocolEvidenceError> {
    let length = usize::try_from(u32::from_be_bytes(take_array::<4>(bytes, cursor)?))
        .map_err(|_| ProtocolEvidenceError::Encoding)?;
    if length > limit {
        return Err(ProtocolEvidenceError::Encoding);
    }
    let end = cursor.checked_add(length).ok_or(ProtocolEvidenceError::Encoding)?;
    let value = bytes
        .get(*cursor..end)
        .ok_or(ProtocolEvidenceError::Encoding)?
        .to_vec();
    *cursor = end;
    Ok(value)
}

fn take_array<const N: usize>(
    bytes: &[u8],
    cursor: &mut usize,
) -> Result<[u8; N], ProtocolEvidenceError> {
    let end = cursor.checked_add(N).ok_or(ProtocolEvidenceError::Encoding)?;
    let value = bytes
        .get(*cursor..end)
        .and_then(|slice| slice.try_into().ok())
        .ok_or(ProtocolEvidenceError::Encoding)?;
    *cursor = end;
    Ok(value)
}

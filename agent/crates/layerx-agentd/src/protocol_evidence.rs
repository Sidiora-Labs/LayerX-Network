//! Raw protocol evidence ingress and verifier-owned trusted protocol policy.

use std::collections::BTreeSet;

use layerx_programs::hex;
use layerx_proof::inclusion::{
    verify_receipt as verify_receipt_inclusion, verify_state, InclusionError, InclusionEvidence,
    SequencerAuthorization,
};
use layerx_proof::merkle::Proof;
use layerx_proof::receipt::{
    verify_outcome, verify_sequencer_signature, AuthorizedBatch, ReceiptCheck, VerificationFailure,
    VerifiedReceipt as ProofVerifiedReceipt,
};
use layerx_types::ids::Did;
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_wire::activity::decode_signed;
use layerx_wire::hash::{activity_id, receipt_execution_batch_id};
use layerx_wire::receipt::{decode, decode_batch_header, BatchHeader};
use sha2::{Digest, Sha256};

use crate::config::{read_protected_source, ProtectedSourceError, StartupConfig};

const MAX_AUTHORITY_SOURCE_BYTES: usize = 65_536;
const AUTHORITY_SOURCE_VERSION: &str = "layerx-sequencer-authority-v1";
const MAX_CUMULATIVE_EVIDENCE_ITEMS: usize = 4_096;

/// One sequencer key and validity interval installed by trusted daemon configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TrustedSequencer {
    sequencer_id: [u8; 32],
    public_key: [u8; 32],
    epoch: u64,
    first_batch_number: u64,
    last_batch_number: u64,
    revoked_at_batch: Option<u64>,
}

impl TrustedSequencer {
    /// Creates one configured sequencer key interval.
    ///
    /// `revoked_at_batch` is inclusive: evidence at or after that batch is refused.
    ///
    /// # Errors
    ///
    /// Refuses an empty identity or key, a reversed batch range, or a revocation
    /// point outside the configured range.
    fn new(
        sequencer_id: [u8; 32],
        public_key: [u8; 32],
        epoch: u64,
        first_batch_number: u64,
        last_batch_number: u64,
        revoked_at_batch: Option<u64>,
    ) -> Result<Self, VerifierPolicyError> {
        if sequencer_id == [0; 32] || public_key == [0; 32] {
            return Err(VerifierPolicyError::InvalidAuthorization);
        }
        if first_batch_number > last_batch_number {
            return Err(VerifierPolicyError::InvalidBatchRange);
        }
        if revoked_at_batch
            .is_some_and(|batch| batch < first_batch_number || batch > last_batch_number)
        {
            return Err(VerifierPolicyError::InvalidRevocation);
        }
        Ok(Self {
            sequencer_id,
            public_key,
            epoch,
            first_batch_number,
            last_batch_number,
            revoked_at_batch,
        })
    }

    const fn active_last_batch(self) -> Option<u64> {
        let last_batch = match self.revoked_at_batch {
            Some(0) => return None,
            Some(revoked_at) => {
                let active_last = revoked_at - 1;
                if active_last < self.last_batch_number {
                    active_last
                } else {
                    self.last_batch_number
                }
            }
            None => self.last_batch_number,
        };
        if self.first_batch_number <= last_batch {
            Some(last_batch)
        } else {
            None
        }
    }
}

/// Immutable trust policy owned by the daemon, never supplied by proof ingress.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProtocolEvidenceVerifier {
    expected_protocol_version: u16,
    expected_network_id: u32,
    sequencers: Vec<TrustedSequencer>,
}

impl ProtocolEvidenceVerifier {
    /// Builds one exact-network verifier from trusted daemon configuration.
    ///
    /// # Errors
    ///
    /// Refuses zero identities, an empty authority set, and overlapping active
    /// ranges because canonical batch authority must select one sequencer.
    fn new(
        expected_protocol_version: u16,
        expected_network_id: u32,
        mut sequencers: Vec<TrustedSequencer>,
    ) -> Result<Self, VerifierPolicyError> {
        if expected_protocol_version == 0 || expected_network_id == 0 || sequencers.is_empty() {
            return Err(VerifierPolicyError::EmptyPolicy);
        }
        sequencers.sort_by_key(|entry| {
            (
                entry.first_batch_number,
                entry.last_batch_number,
                entry.epoch,
                entry.sequencer_id,
            )
        });
        let mut active_through = None;
        for entry in &sequencers {
            let Some(last_batch) = entry.active_last_batch() else {
                continue;
            };
            if active_through.is_some_and(|previous| entry.first_batch_number <= previous) {
                return Err(VerifierPolicyError::AmbiguousAuthorization);
            }
            active_through = Some(last_batch);
        }
        Ok(Self {
            expected_protocol_version,
            expected_network_id,
            sequencers,
        })
    }

    pub(crate) fn load(config: &StartupConfig) -> Result<Self, VerifierPolicyError> {
        let bytes = read_protected_source(
            &config.sequencer_authority_source,
            MAX_AUTHORITY_SOURCE_BYTES,
        )
        .map_err(|failure| match failure {
            ProtectedSourceError::Unavailable => VerifierPolicyError::AuthoritySourceUnavailable,
            ProtectedSourceError::TooLarge => VerifierPolicyError::AuthoritySourceMalformed,
            ProtectedSourceError::Unprotected | ProtectedSourceError::Changed => {
                VerifierPolicyError::AuthoritySourceUnprotected
            }
        })?;
        if bytes.is_empty() {
            return Err(VerifierPolicyError::AuthoritySourceMalformed);
        }
        let source = std::str::from_utf8(&bytes)
            .map_err(|_| VerifierPolicyError::AuthoritySourceMalformed)?;
        let mut lines = source.lines();
        if lines.next() != Some(AUTHORITY_SOURCE_VERSION) {
            return Err(VerifierPolicyError::AuthoritySourceMalformed);
        }
        let mut sequencers = Vec::new();
        for line in lines {
            if line.is_empty() || line.trim() != line {
                return Err(VerifierPolicyError::AuthoritySourceMalformed);
            }
            let mut fields = line.split(',');
            let (
                Some(sequencer_id),
                Some(public_key),
                Some(epoch),
                Some(first_batch),
                Some(last_batch),
                Some(revoked_at),
            ) = (
                fields.next(),
                fields.next(),
                fields.next(),
                fields.next(),
                fields.next(),
                fields.next(),
            )
            else {
                return Err(VerifierPolicyError::AuthoritySourceMalformed);
            };
            if fields.next().is_some() {
                return Err(VerifierPolicyError::AuthoritySourceMalformed);
            }
            let sequencer_id = hex::decode_digest(sequencer_id)
                .map_err(|_| VerifierPolicyError::AuthoritySourceMalformed)?;
            let public_key = hex::decode_digest(public_key)
                .map_err(|_| VerifierPolicyError::AuthoritySourceMalformed)?;
            let epoch = epoch
                .parse()
                .map_err(|_| VerifierPolicyError::AuthoritySourceMalformed)?;
            let first_batch = first_batch
                .parse()
                .map_err(|_| VerifierPolicyError::AuthoritySourceMalformed)?;
            let last_batch = last_batch
                .parse()
                .map_err(|_| VerifierPolicyError::AuthoritySourceMalformed)?;
            let revoked_at_batch = if revoked_at == "active" {
                None
            } else {
                Some(
                    revoked_at
                        .parse()
                        .map_err(|_| VerifierPolicyError::AuthoritySourceMalformed)?,
                )
            };
            sequencers.push(TrustedSequencer::new(
                sequencer_id,
                public_key,
                epoch,
                first_batch,
                last_batch,
                revoked_at_batch,
            )?);
        }
        Self::new(
            config.expected_protocol_version,
            config.network_id,
            sequencers,
        )
    }

    pub(crate) fn accepts_handshake_key(&self, batch: u64, public_key: [u8; 32]) -> bool {
        self.sequencers.iter().any(|entry| {
            entry.public_key == public_key
                && batch >= entry.first_batch_number
                && batch <= entry.last_batch_number
                && entry
                    .revoked_at_batch
                    .is_none_or(|revoked_at| batch < revoked_at)
        })
    }

    fn authorization_for(
        &self,
        canonical_header: &[u8],
    ) -> Result<(BatchHeader, SequencerAuthorization), VerifierPolicyError> {
        let header =
            decode_batch_header(canonical_header).map_err(|_| VerifierPolicyError::HeaderDecode)?;
        if header.protocol_version() != self.expected_protocol_version {
            return Err(VerifierPolicyError::ProtocolVersion);
        }
        if header.network_id() != self.expected_network_id {
            return Err(VerifierPolicyError::Network);
        }
        let mut saw_identity = false;
        let mut saw_epoch = false;
        let mut saw_revoked = false;
        let mut selected = None;
        for entry in &self.sequencers {
            if entry.sequencer_id != header.sequencer_id() {
                continue;
            }
            saw_identity = true;
            if entry.epoch != header.epoch() {
                continue;
            }
            saw_epoch = true;
            if header.batch_number() < entry.first_batch_number
                || header.batch_number() > entry.last_batch_number
            {
                continue;
            }
            if entry
                .revoked_at_batch
                .is_some_and(|revoked_at| header.batch_number() >= revoked_at)
            {
                saw_revoked = true;
                continue;
            }
            if selected.replace(entry).is_some() {
                return Err(VerifierPolicyError::AmbiguousAuthorization);
            }
        }
        let Some(entry) = selected else {
            return Err(if !saw_identity {
                VerifierPolicyError::UnknownSequencer
            } else if !saw_epoch {
                VerifierPolicyError::Epoch
            } else if saw_revoked {
                VerifierPolicyError::Revoked
            } else {
                VerifierPolicyError::BatchRange
            });
        };
        let active_last_batch = entry
            .active_last_batch()
            .ok_or(VerifierPolicyError::Revoked)?;
        Ok((
            header,
            SequencerAuthorization::new(
                entry.sequencer_id,
                entry.public_key,
                entry.first_batch_number,
                active_last_batch,
            ),
        ))
    }

    /// Verifies raw receipt ingress and issues an opaque receipt token.
    ///
    /// # Errors
    ///
    /// Refuses policy identity, canonical inclusion, signed batch identity, receipt
    /// signature, root-chain, and sequence failures before issuing evidence.
    fn verify_receipt(
        &self,
        raw: &RawReceiptEvidence,
    ) -> Result<VerifiedReceiptEvidence, ReceiptEvidenceError> {
        let (selected_header, authorization) = self
            .authorization_for(&raw.canonical_header)
            .map_err(ReceiptEvidenceError::Policy)?;
        let inclusion = verify_receipt_inclusion(
            &raw.canonical_receipt,
            &raw.proof,
            &raw.canonical_header,
            &raw.header_signature,
            &authorization,
        )
        .map_err(ReceiptEvidenceError::Inclusion)?;
        let decoded = decode(&raw.canonical_receipt).map_err(|_| {
            ReceiptEvidenceError::Receipt(VerificationFailure {
                check: ReceiptCheck::Decode,
            })
        })?;
        let protocol =
            decoded
                .protocol()
                .ok_or(ReceiptEvidenceError::Receipt(VerificationFailure {
                    check: ReceiptCheck::ReceiptShape,
                }))?;
        if protocol.protocol_version() != selected_header.protocol_version() {
            return Err(ReceiptEvidenceError::ProtocolVersion);
        }
        if protocol.global_sequence() < selected_header.first_sequence()
            || protocol.global_sequence() > selected_header.last_sequence()
        {
            return Err(ReceiptEvidenceError::SequenceRange);
        }
        let expected_execution_id = receipt_execution_batch_id(protocol, &selected_header)
            .map_err(|_| ReceiptEvidenceError::BatchIdentity)?;
        if protocol.batch_id() != expected_execution_id {
            return Err(ReceiptEvidenceError::BatchIdentity);
        }
        let authorised = AuthorizedBatch::new(
            expected_execution_id,
            protocol.asset(),
            selected_header.previous_state_root(),
            selected_header.resulting_state_root(),
            authorization.public_key(),
        );
        let verified = verify_outcome(&raw.canonical_receipt, &authorised)
            .map_err(ReceiptEvidenceError::Receipt)?;
        let protocol = verified
            .receipt()
            .protocol()
            .ok_or(ReceiptEvidenceError::Receipt(VerificationFailure {
                check: ReceiptCheck::ReceiptShape,
            }))?;
        let receipt_ref = Sha256::digest(verified.canonical_bytes()).into();
        Ok(VerifiedReceiptEvidence {
            receipt_ref,
            activity_id: protocol.activity_id(),
            global_sequence: protocol.global_sequence(),
            result_code: protocol.result_code(),
            amount: protocol.amount(),
            module_id: protocol.module_id(),
            operation: protocol.operation(),
            verified,
            inclusion,
        })
    }

    fn verify_signed_receipt_inclusion(
        &self,
        raw: &RawReceiptEvidence,
    ) -> Result<VerifiedCumulativeReceipt, ReceiptEvidenceError> {
        let (header, authorization) = self
            .authorization_for(&raw.canonical_header)
            .map_err(ReceiptEvidenceError::Policy)?;
        verify_receipt_inclusion(
            &raw.canonical_receipt,
            &raw.proof,
            &raw.canonical_header,
            &raw.header_signature,
            &authorization,
        )
        .map_err(ReceiptEvidenceError::Inclusion)?;
        let receipt =
            verify_sequencer_signature(&raw.canonical_receipt, authorization.public_key())
                .map_err(ReceiptEvidenceError::Receipt)?;
        let protocol =
            receipt
                .protocol()
                .ok_or(ReceiptEvidenceError::Receipt(VerificationFailure {
                    check: ReceiptCheck::ReceiptShape,
                }))?;
        if protocol.protocol_version() != header.protocol_version() {
            return Err(ReceiptEvidenceError::ProtocolVersion);
        }
        if protocol.global_sequence() < header.first_sequence()
            || protocol.global_sequence() > header.last_sequence()
        {
            return Err(ReceiptEvidenceError::SequenceRange);
        }
        Ok(VerifiedCumulativeReceipt::from_protocol(
            protocol, header, &raw.proof,
        ))
    }

    /// Verifies a raw state leaf and issues an opaque state token.
    ///
    /// # Errors
    ///
    /// Refuses policy identity, canonical header, signature, state-root, and Merkle
    /// failures before issuing evidence.
    fn verify_state(
        &self,
        raw: &RawStateEvidence,
    ) -> Result<VerifiedStateEvidence, StateEvidenceError> {
        let (_, authorization) = self
            .authorization_for(&raw.canonical_header)
            .map_err(StateEvidenceError::Policy)?;
        let inclusion = verify_state(
            &raw.canonical_state,
            &raw.proof,
            &raw.resulting_state_root,
            &raw.canonical_header,
            &raw.header_signature,
            &authorization,
        )
        .map_err(StateEvidenceError::Inclusion)?;
        let observed_head_sequence = inclusion.header().header().last_sequence();
        Ok(VerifiedStateEvidence {
            canonical_state: raw.canonical_state.clone(),
            inclusion,
            observed_head_sequence,
        })
    }
}

/// Exact trusted-policy refusal class.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerifierPolicyError {
    AuthoritySourceUnavailable,
    AuthoritySourceUnprotected,
    AuthoritySourceMalformed,
    EmptyPolicy,
    InvalidAuthorization,
    InvalidBatchRange,
    InvalidRevocation,
    AmbiguousAuthorization,
    HeaderDecode,
    ProtocolVersion,
    Network,
    UnknownSequencer,
    Epoch,
    BatchRange,
    Revoked,
    HandshakeKey,
}

/// Opaque daemon boot authority retained only after trusted config and handshake agree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceAuthority {
    verifier: ProtocolEvidenceVerifier,
}

impl EvidenceAuthority {
    pub(crate) const fn new(verifier: ProtocolEvidenceVerifier) -> Self {
        Self { verifier }
    }

    pub(crate) const fn verifier(&self) -> &ProtocolEvidenceVerifier {
        &self.verifier
    }

    /// Verifies raw receipt ingress under the daemon's accepted startup authority.
    ///
    /// # Errors
    ///
    /// Refuses every policy, signature, inclusion, sequence, or batch-identity mismatch.
    pub fn verify_receipt(
        &self,
        raw: &RawReceiptEvidence,
    ) -> Result<VerifiedReceiptEvidence, ReceiptEvidenceError> {
        self.verifier.verify_receipt(raw)
    }

    /// Verifies raw state ingress under the daemon's accepted startup authority.
    ///
    /// # Errors
    ///
    /// Refuses every policy, signature, root, or inclusion mismatch.
    pub fn verify_state(
        &self,
        raw: &RawStateEvidence,
    ) -> Result<VerifiedStateEvidence, StateEvidenceError> {
        self.verifier.verify_state(raw)
    }

    fn verify_cumulative_window(
        &self,
        window: CumulativeUseWindow,
        entries: &[RawActivityReceiptEvidence],
    ) -> Result<Vec<VerifiedCumulativeReceipt>, CumulativeUseError> {
        let length = window
            .last
            .checked_sub(window.first)
            .and_then(|distance| distance.checked_add(1))
            .and_then(|length| usize::try_from(length).ok())
            .ok_or(CumulativeUseError::InvalidWindow)?;
        if length == 0 || length > MAX_CUMULATIVE_EVIDENCE_ITEMS {
            return Err(CumulativeUseError::InvalidWindow);
        }
        if entries.is_empty() || entries.len() > length {
            return Err(CumulativeUseError::IncompleteWindow);
        }
        let mut receipt_refs = BTreeSet::new();
        let mut activity_ids = BTreeSet::new();
        let mut previous_header: Option<BatchHeader> = None;
        let mut current_header: Option<BatchHeader> = None;
        let mut expected_leaf = 0_u32;
        let mut activity_count = 0_u32;
        let mut covered_until = window.first;
        let mut verified = Vec::with_capacity(entries.len());
        for entry in entries {
            let receipt = match &entry.receipt {
                CumulativeReceiptEvidence::Outcome(raw) => self
                    .verify_receipt(raw)
                    .map(|value| VerifiedCumulativeReceipt::from_verified(&value, raw.proof()))
                    .map_err(CumulativeUseError::Receipt)?,
                CumulativeReceiptEvidence::SignedInclusion(raw) => self
                    .verifier
                    .verify_signed_receipt_inclusion(raw)
                    .map_err(CumulativeUseError::Receipt)?,
            };
            let receipt_ref: [u8; 32] = Sha256::digest(entry.receipt().canonical_receipt()).into();
            if !receipt_refs.insert(receipt_ref) || !activity_ids.insert(receipt.activity_id()) {
                return Err(CumulativeUseError::Duplicate);
            }
            let header = receipt.header();
            if current_header.as_ref() != Some(header) {
                if expected_leaf != activity_count || header.first_sequence() != covered_until {
                    return Err(CumulativeUseError::IncompleteWindow);
                }
                if previous_header.as_ref().is_some_and(|previous| {
                    previous.batch_number().checked_add(1) != Some(header.batch_number())
                        || header.previous_state_root() != previous.resulting_state_root()
                }) {
                    return Err(CumulativeUseError::BatchChain);
                }
                activity_count = header
                    .last_sequence()
                    .checked_sub(header.first_sequence())
                    .and_then(|value| u32::try_from(value).ok())
                    .filter(|value| *value > 0)
                    .ok_or(CumulativeUseError::Sequence)?;
                expected_leaf = 0;
                current_header = Some(header.clone());
            }
            let expected_leaf_count = activity_count
                .checked_add(1)
                .ok_or(CumulativeUseError::InvalidWindow)?;
            if receipt.leaf_count() != expected_leaf_count
                || receipt.leaf_index() != expected_leaf
                || receipt.global_sequence()
                    != header
                        .first_sequence()
                        .checked_add(u64::from(expected_leaf))
                        .ok_or(CumulativeUseError::Sequence)?
            {
                return Err(CumulativeUseError::IncompleteWindow);
            }
            expected_leaf = expected_leaf
                .checked_add(1)
                .ok_or(CumulativeUseError::InvalidWindow)?;
            if expected_leaf == activity_count {
                covered_until = header
                    .last_sequence()
                    .checked_add(1)
                    .ok_or(CumulativeUseError::InvalidWindow)?;
                previous_header = Some(header.clone());
            }
            verified.push(receipt);
        }
        if expected_leaf != activity_count
            || covered_until
                != window
                    .last
                    .checked_add(1)
                    .ok_or(CumulativeUseError::InvalidWindow)?
        {
            return Err(CumulativeUseError::IncompleteWindow);
        }
        Ok(verified)
    }

    /// Authenticates a complete protocol-sequence window and derives cumulative
    /// successful activity use for one actor from the exact executed activities.
    ///
    /// # Errors
    ///
    /// Refuses empty, oversized, incomplete, duplicated, noncanonical, forked,
    /// or receipt/activity-mismatched windows before issuing cumulative facts.
    pub fn authenticate_cumulative_use(
        &self,
        actor: &Did,
        window: CumulativeUseWindow,
        entries: &[RawActivityReceiptEvidence],
    ) -> Result<AuthenticatedCumulativeUse, CumulativeUseError> {
        let receipts = self.verify_cumulative_window(window, entries)?;
        let mut amount = 0_u128;
        let mut count = 0_u64;
        for (entry, receipt) in entries.iter().zip(receipts) {
            let module = ModuleId::from_u16(receipt.module_id())
                .map_err(|_| CumulativeUseError::Activity)?;
            let kind = ActivityType::new(module, u16::from(receipt.operation()))
                .map_err(|_| CumulativeUseError::Activity)?;
            let registration = ModuleRegistration::new(module, &[kind])
                .map_err(|_| CumulativeUseError::Activity)?;
            let registry =
                ModuleRegistry::new(&[registration]).map_err(|_| CumulativeUseError::Activity)?;
            let activity = decode_signed(&entry.canonical_activity, &registry)
                .map_err(|_| CumulativeUseError::Activity)?;
            if activity_id(&activity).map_err(|_| CumulativeUseError::Activity)?
                != receipt.activity_id()
                || activity.activity_type() != kind
            {
                return Err(CumulativeUseError::ActivityReceiptMismatch);
            }
            if receipt.result_code() == 0 && activity.actor_did() == actor.as_bytes() {
                amount = amount
                    .checked_add(receipt.amount())
                    .ok_or(CumulativeUseError::AmountOverflow)?;
                count = count
                    .checked_add(1)
                    .ok_or(CumulativeUseError::CountOverflow)?;
            }
        }
        Ok(AuthenticatedCumulativeUse {
            actor: actor.clone(),
            window,
            amount,
            count,
        })
    }

    pub(crate) fn receipt_replay_guard() -> ReceiptReplayGuard {
        ReceiptReplayGuard::default()
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ReceiptReplayGuard {
    receipt_refs: BTreeSet<[u8; 32]>,
    activity_ids: BTreeSet<[u8; 32]>,
}

impl ReceiptReplayGuard {
    pub(crate) fn admit(
        &mut self,
        receipt: &VerifiedReceiptEvidence,
    ) -> Result<(), ReceiptReplayError> {
        if self.receipt_refs.contains(&receipt.receipt_ref()) {
            return Err(ReceiptReplayError::DuplicateReceipt);
        }
        if self.activity_ids.contains(&receipt.activity_id()) {
            return Err(ReceiptReplayError::DuplicateActivity);
        }
        self.receipt_refs.insert(receipt.receipt_ref());
        self.activity_ids.insert(receipt.activity_id());
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReceiptReplayError {
    DuplicateReceipt,
    DuplicateActivity,
}

/// Raw receipt bytes, Merkle path, and signature returned by a core boundary.
///
/// Trusted keys, ranges, network identity, and protocol identity are deliberately
/// absent. They come only from `ProtocolEvidenceVerifier` configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawReceiptEvidence {
    canonical_receipt: Vec<u8>,
    proof: Proof,
    canonical_header: Vec<u8>,
    header_signature: [u8; 64],
}

impl RawReceiptEvidence {
    #[must_use]
    pub fn new(
        canonical_receipt: Vec<u8>,
        proof: Proof,
        canonical_header: Vec<u8>,
        header_signature: [u8; 64],
    ) -> Self {
        Self {
            canonical_receipt,
            proof,
            canonical_header,
            header_signature,
        }
    }

    #[must_use]
    pub fn canonical_receipt(&self) -> &[u8] {
        &self.canonical_receipt
    }

    #[must_use]
    pub const fn proof(&self) -> &Proof {
        &self.proof
    }

    #[must_use]
    pub fn canonical_header(&self) -> &[u8] {
        &self.canonical_header
    }

    #[must_use]
    pub const fn header_signature(&self) -> [u8; 64] {
        self.header_signature
    }
}

/// Exact verification stage which refused raw receipt evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReceiptEvidenceError {
    Policy(VerifierPolicyError),
    Inclusion(InclusionError),
    Receipt(VerificationFailure),
    ProtocolVersion,
    SequenceRange,
    BatchIdentity,
}

/// Inclusive global-sequence window covered by cumulative-use evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CumulativeUseWindow {
    pub first: u64,
    pub last: u64,
}

/// One exact signed activity paired with its independently authenticated receipt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawActivityReceiptEvidence {
    canonical_activity: Vec<u8>,
    receipt: CumulativeReceiptEvidence,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum CumulativeReceiptEvidence {
    Outcome(RawReceiptEvidence),
    SignedInclusion(RawReceiptEvidence),
}

impl RawActivityReceiptEvidence {
    #[must_use]
    pub fn new(canonical_activity: Vec<u8>, receipt: RawReceiptEvidence) -> Self {
        Self {
            canonical_activity,
            receipt: CumulativeReceiptEvidence::Outcome(receipt),
        }
    }

    /// Admits the gateway proof shape: exact sequencer-signed receipt bytes
    /// included in an independently authenticated signed batch header.
    #[must_use]
    pub fn from_signed_inclusion(canonical_activity: Vec<u8>, receipt: RawReceiptEvidence) -> Self {
        Self {
            canonical_activity,
            receipt: CumulativeReceiptEvidence::SignedInclusion(receipt),
        }
    }

    #[must_use]
    pub fn canonical_activity(&self) -> &[u8] {
        &self.canonical_activity
    }

    #[must_use]
    pub const fn receipt(&self) -> &RawReceiptEvidence {
        match &self.receipt {
            CumulativeReceiptEvidence::Outcome(receipt)
            | CumulativeReceiptEvidence::SignedInclusion(receipt) => receipt,
        }
    }
}

struct VerifiedCumulativeReceipt {
    activity_id: [u8; 32],
    global_sequence: u64,
    result_code: i32,
    amount: u128,
    module_id: u16,
    operation: u8,
    header: BatchHeader,
    leaf_index: u32,
    leaf_count: u32,
}

impl VerifiedCumulativeReceipt {
    fn from_verified(receipt: &VerifiedReceiptEvidence, proof: &Proof) -> Self {
        let header = receipt.inclusion.header().header().clone();
        Self {
            activity_id: receipt.activity_id(),
            global_sequence: receipt.global_sequence(),
            result_code: receipt.result_code(),
            amount: receipt.amount(),
            module_id: receipt.module_id(),
            operation: receipt.operation(),
            header,
            leaf_index: proof.leaf_index(),
            leaf_count: proof.leaf_count(),
        }
    }

    fn from_protocol(
        protocol: &layerx_wire::receipt::ProtocolReceipt,
        header: BatchHeader,
        proof: &Proof,
    ) -> Self {
        Self {
            activity_id: protocol.activity_id(),
            global_sequence: protocol.global_sequence(),
            result_code: protocol.result_code(),
            amount: protocol.amount(),
            module_id: protocol.module_id(),
            operation: protocol.operation(),
            header,
            leaf_index: proof.leaf_index(),
            leaf_count: proof.leaf_count(),
        }
    }

    const fn activity_id(&self) -> [u8; 32] {
        self.activity_id
    }

    const fn global_sequence(&self) -> u64 {
        self.global_sequence
    }

    const fn result_code(&self) -> i32 {
        self.result_code
    }

    const fn amount(&self) -> u128 {
        self.amount
    }

    const fn module_id(&self) -> u16 {
        self.module_id
    }

    const fn operation(&self) -> u8 {
        self.operation
    }

    const fn header(&self) -> &BatchHeader {
        &self.header
    }

    const fn leaf_index(&self) -> u32 {
        self.leaf_index
    }

    const fn leaf_count(&self) -> u32 {
        self.leaf_count
    }
}

/// Opaque cumulative facts issued only for a complete authenticated window.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthenticatedCumulativeUse {
    actor: Did,
    window: CumulativeUseWindow,
    amount: u128,
    count: u64,
}

impl AuthenticatedCumulativeUse {
    pub(crate) const fn actor(&self) -> &Did {
        &self.actor
    }

    pub(crate) const fn window(&self) -> CumulativeUseWindow {
        self.window
    }

    pub(crate) const fn amount(&self) -> u128 {
        self.amount
    }

    pub(crate) const fn count(&self) -> u64 {
        self.count
    }
}

/// Exact refusal while producing authenticated cumulative-use facts.
#[derive(Debug)]
pub enum CumulativeUseError {
    InvalidWindow,
    IncompleteWindow,
    Receipt(ReceiptEvidenceError),
    Duplicate,
    Sequence,
    Activity,
    ActivityReceiptMismatch,
    BatchChain,
    AmountOverflow,
    CountOverflow,
}

/// Receipt facts available only after configured policy and canonical proof verification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedReceiptEvidence {
    verified: ProofVerifiedReceipt,
    inclusion: InclusionEvidence,
    receipt_ref: [u8; 32],
    activity_id: [u8; 32],
    global_sequence: u64,
    result_code: i32,
    amount: u128,
    module_id: u16,
    operation: u8,
}

impl VerifiedReceiptEvidence {
    pub(crate) fn verify_authorized_maintained(
        raw: &RawReceiptEvidence,
        activity_batch: &AuthorizedBatch,
        evidence: &layerx_proof::receipt::MaintainedOutcomeEvidence<'_>,
        receipts: &[Vec<u8>],
        protocol_version: u16,
        network_id: u32,
    ) -> Result<Self, ReceiptEvidenceError> {
        let header = decode_batch_header(raw.canonical_header())
            .map_err(|_| ReceiptEvidenceError::Policy(VerifierPolicyError::HeaderDecode))?;
        if header.protocol_version() != protocol_version
            || header.network_id() != network_id
            || evidence.header != raw.canonical_header()
            || evidence.header_signature != &raw.header_signature
            || evidence.activity_proof != &raw.proof
            || evidence.authorization.public_key() != activity_batch.sequencer_public_key()
        {
            return Err(ReceiptEvidenceError::BatchIdentity);
        }
        let sealed_batch = AuthorizedBatch::new(
            activity_batch.batch_id(),
            activity_batch.asset(),
            header.previous_state_root(),
            header.resulting_state_root(),
            activity_batch.sequencer_public_key(),
        );
        let authenticated = layerx_proof::receipt::authorized_maintained_activity_batch_chain(
            &raw.canonical_receipt,
            &sealed_batch,
            evidence,
            receipts,
        )
        .map_err(|_| ReceiptEvidenceError::BatchIdentity)?;
        if authenticated != *activity_batch {
            return Err(ReceiptEvidenceError::BatchIdentity);
        }
        let verified = verify_outcome(&raw.canonical_receipt, &authenticated)
            .map_err(ReceiptEvidenceError::Receipt)?;
        let inclusion = verify_receipt_inclusion(
            &raw.canonical_receipt,
            &raw.proof,
            &raw.canonical_header,
            &raw.header_signature,
            evidence.authorization,
        )
        .map_err(ReceiptEvidenceError::Inclusion)?;
        let protocol = verified
            .receipt()
            .protocol()
            .ok_or(ReceiptEvidenceError::BatchIdentity)?;
        Ok(Self {
            receipt_ref: Sha256::digest(verified.canonical_bytes()).into(),
            activity_id: protocol.activity_id(),
            global_sequence: protocol.global_sequence(),
            result_code: protocol.result_code(),
            amount: protocol.amount(),
            module_id: protocol.module_id(),
            operation: protocol.operation(),
            verified,
            inclusion,
        })
    }
    pub(crate) fn verify_authorized(
        raw: &RawReceiptEvidence,
        batch: &AuthorizedBatch,
        protocol_version: u16,
        network_id: u32,
    ) -> Result<Self, ReceiptEvidenceError> {
        let header = decode_batch_header(raw.canonical_header())
            .map_err(|_| ReceiptEvidenceError::Policy(VerifierPolicyError::HeaderDecode))?;
        if header.previous_state_root() != batch.previous_state_root()
            || header.resulting_state_root() != batch.resulting_state_root()
        {
            return Err(ReceiptEvidenceError::BatchIdentity);
        }
        let sequencer = TrustedSequencer::new(
            header.sequencer_id(),
            batch.sequencer_public_key(),
            header.epoch(),
            header.batch_number(),
            header.batch_number(),
            None,
        )
        .map_err(ReceiptEvidenceError::Policy)?;
        let policy = ProtocolEvidenceVerifier::new(protocol_version, network_id, vec![sequencer])
            .map_err(ReceiptEvidenceError::Policy)?;
        let verified = policy.verify_receipt(raw)?;
        let protocol = verified
            .verified
            .receipt()
            .protocol()
            .ok_or(ReceiptEvidenceError::BatchIdentity)?;
        if protocol.batch_id() != batch.batch_id() || protocol.asset() != batch.asset() {
            return Err(ReceiptEvidenceError::BatchIdentity);
        }
        Ok(verified)
    }
    #[must_use]
    pub const fn receipt_ref(&self) -> [u8; 32] {
        self.receipt_ref
    }

    #[must_use]
    pub fn canonical_receipt(&self) -> &[u8] {
        self.verified.canonical_bytes()
    }

    #[must_use]
    pub const fn level(&self) -> layerx_types::verify::VerificationLevel {
        self.inclusion.level()
    }

    #[must_use]
    pub fn activity_id(&self) -> [u8; 32] {
        self.activity_id
    }

    #[must_use]
    pub fn result_code(&self) -> i32 {
        self.result_code
    }

    #[must_use]
    pub const fn global_sequence(&self) -> u64 {
        self.global_sequence
    }

    #[must_use]
    pub fn amount(&self) -> u128 {
        self.amount
    }

    #[must_use]
    pub const fn module_id(&self) -> u16 {
        self.module_id
    }

    #[must_use]
    pub const fn operation(&self) -> u8 {
        self.operation
    }
}

/// Raw state leaf and signed inclusion material returned by a core boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawStateEvidence {
    canonical_state: Vec<u8>,
    proof: Proof,
    resulting_state_root: [u8; 32],
    canonical_header: Vec<u8>,
    header_signature: [u8; 64],
}

impl RawStateEvidence {
    #[must_use]
    pub fn new(
        canonical_state: Vec<u8>,
        proof: Proof,
        resulting_state_root: [u8; 32],
        canonical_header: Vec<u8>,
        header_signature: [u8; 64],
    ) -> Self {
        Self {
            canonical_state,
            proof,
            resulting_state_root,
            canonical_header,
            header_signature,
        }
    }

    #[must_use]
    pub fn canonical_state(&self) -> &[u8] {
        &self.canonical_state
    }

    #[must_use]
    pub const fn proof(&self) -> &Proof {
        &self.proof
    }

    #[must_use]
    pub const fn resulting_state_root(&self) -> [u8; 32] {
        self.resulting_state_root
    }

    #[must_use]
    pub fn canonical_header(&self) -> &[u8] {
        &self.canonical_header
    }

    #[must_use]
    pub const fn header_signature(&self) -> [u8; 64] {
        self.header_signature
    }
}

/// Exact verification stage which refused raw state evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StateEvidenceError {
    Policy(VerifierPolicyError),
    Inclusion(InclusionError),
}

/// State bytes available only after configured policy and canonical proof verification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedStateEvidence {
    canonical_state: Vec<u8>,
    inclusion: InclusionEvidence,
    observed_head_sequence: u64,
}

impl VerifiedStateEvidence {
    #[must_use]
    pub fn canonical_state(&self) -> &[u8] {
        &self.canonical_state
    }

    #[must_use]
    pub const fn level(&self) -> layerx_types::verify::VerificationLevel {
        self.inclusion.level()
    }

    #[must_use]
    pub const fn observed_head_sequence(&self) -> u64 {
        self.observed_head_sequence
    }
}

#[cfg(test)]
#[path = "protocol_evidence_native_tests.rs"]
mod native_terminal_tests;

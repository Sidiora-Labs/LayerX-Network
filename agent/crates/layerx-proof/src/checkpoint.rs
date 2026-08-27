//! Bonded-guarantor threshold and settlement-anchor verification.

use std::collections::BTreeSet;

use layerx_crypto::secp256k1;
use layerx_wire::hash::{checkpoint_attestation_digest, checkpoint_id as hash_checkpoint_id};
use layerx_wire::receipt::{decode_batch_header, encode_batch_header};

use crate::availability::RootCommitments;
use crate::evidence::Evidence;
use crate::level::achieved;

const ATTESTATION_BYTES: usize = 189;
const ALL_AVAILABILITY_CLASSES: u8 = 0x1f;

/// Canonical checkpoint body committed by guarantors.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Checkpoint {
    header_bytes: Vec<u8>,
    validity_proof: Vec<u8>,
}

impl Checkpoint {
    /// Creates a checkpoint body from exact core-produced bytes.
    #[must_use]
    pub fn new(header_bytes: Vec<u8>, validity_proof: Vec<u8>) -> Self {
        Self {
            header_bytes,
            validity_proof,
        }
    }

    /// Borrows the exact core-produced checkpoint header bytes.
    #[must_use]
    pub fn header_bytes(&self) -> &[u8] {
        &self.header_bytes
    }

    /// Borrows the exact core-produced validity proof committed by the
    /// checkpoint identifier.
    #[must_use]
    pub fn validity_proof(&self) -> &[u8] {
        &self.validity_proof
    }
}

/// One exact replay-and-possession attestation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Attestation {
    protocol_version: u16,
    network_id: u32,
    paxeer_chain_id: u64,
    paxeer_settlement_contract: [u8; 20],
    epoch: u64,
    checkpoint_id: [u8; 32],
    checkpoint_hash: [u8; 32],
    guarantor_id: [u8; 32],
    batch_number: u64,
    data_availability_root: [u8; 32],
    replayed: bool,
    data_possessed: bool,
    availability_class_mask: u8,
    attested_at_ms: u64,
    signer: [u8; 20],
    signature: [u8; 64],
    signature_v: u8,
}

impl Attestation {
    /// Creates one attestation decoded from core certificate bytes.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn new(
        protocol_version: u16,
        network_id: u32,
        paxeer_chain_id: u64,
        paxeer_settlement_contract: [u8; 20],
        epoch: u64,
        checkpoint_id: [u8; 32],
        checkpoint_hash: [u8; 32],
        guarantor_id: [u8; 32],
        batch_number: u64,
        data_availability_root: [u8; 32],
        replayed: bool,
        data_possessed: bool,
        availability_class_mask: u8,
        attested_at_ms: u64,
        signer: [u8; 20],
        signature: [u8; 64],
        signature_v: u8,
    ) -> Self {
        Self {
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
        }
    }

    #[must_use]
    pub const fn guarantor_id(&self) -> [u8; 32] {
        self.guarantor_id
    }

    #[must_use]
    pub const fn signature(&self) -> [u8; 64] {
        self.signature
    }

    #[must_use]
    pub const fn signer(&self) -> [u8; 20] {
        self.signer
    }

    #[must_use]
    pub const fn signature_v(&self) -> u8 {
        self.signature_v
    }

    fn message(&self) -> [u8; ATTESTATION_BYTES] {
        let mut message = [0_u8; ATTESTATION_BYTES];
        message[..2].copy_from_slice(&self.protocol_version.to_be_bytes());
        message[2..6].copy_from_slice(&self.network_id.to_be_bytes());
        message[6..14].copy_from_slice(&self.paxeer_chain_id.to_be_bytes());
        message[14..34].copy_from_slice(&self.paxeer_settlement_contract);
        message[34..42].copy_from_slice(&self.epoch.to_be_bytes());
        message[42..74].copy_from_slice(&self.checkpoint_id);
        message[74..106].copy_from_slice(&self.checkpoint_hash);
        message[106..138].copy_from_slice(&self.guarantor_id);
        message[138..146].copy_from_slice(&self.batch_number.to_be_bytes());
        message[146..178].copy_from_slice(&self.data_availability_root);
        message[178] = u8::from(self.replayed);
        message[179] = u8::from(self.data_possessed);
        message[180] = self.availability_class_mask;
        message[181..189].copy_from_slice(&self.attested_at_ms.to_be_bytes());
        message
    }
}

/// One guarantor key record from the bonded set at the checkpoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GuarantorKey {
    guarantor_id: [u8; 32],
    public_key: [u8; 33],
    bonded: bool,
}

/// Settlement domain owned by the certificate verifier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettlementDomain {
    paxeer_chain_id: u64,
    settlement_contract: [u8; 20],
}

impl SettlementDomain {
    /// Creates the exact Paxeer chain and `GuarantorBond` contract expected by
    /// the verifier.
    #[must_use]
    pub const fn new(paxeer_chain_id: u64, settlement_contract: [u8; 20]) -> Self {
        Self {
            paxeer_chain_id,
            settlement_contract,
        }
    }

    #[must_use]
    pub const fn paxeer_chain_id(self) -> u64 {
        self.paxeer_chain_id
    }

    #[must_use]
    pub const fn settlement_contract(self) -> [u8; 20] {
        self.settlement_contract
    }
}

impl GuarantorKey {
    /// Creates one checkpoint-relative guarantor key record.
    #[must_use]
    pub const fn new(guarantor_id: [u8; 32], public_key: [u8; 33], bonded: bool) -> Self {
        Self {
            guarantor_id,
            public_key,
            bonded,
        }
    }
}

/// A checkpoint certificate plus its optional registered settlement reference.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Certificate {
    checkpoint: Checkpoint,
    attestations: Vec<Attestation>,
    threshold: usize,
    settlement_reference: Option<Vec<u8>>,
}

impl Certificate {
    /// Creates certificate material decoded from core-produced bytes.
    #[must_use]
    pub fn new(
        checkpoint: Checkpoint,
        attestations: Vec<Attestation>,
        threshold: usize,
        settlement_reference: Option<Vec<u8>>,
    ) -> Self {
        Self {
            checkpoint,
            attestations,
            threshold,
            settlement_reference,
        }
    }

    #[must_use]
    pub const fn checkpoint(&self) -> &Checkpoint {
        &self.checkpoint
    }

    #[must_use]
    pub fn attestations(&self) -> &[Attestation] {
        &self.attestations
    }

    #[must_use]
    pub const fn threshold(&self) -> usize {
        self.threshold
    }

    #[must_use]
    pub fn settlement_reference(&self) -> Option<&[u8]> {
        self.settlement_reference.as_deref()
    }
}

/// Successful distinct-signature count and the level it established.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThresholdReport {
    /// Number of distinct bonded signatures that verified.
    pub achieved: usize,
    /// Configured certificate threshold.
    pub required: usize,
    evidence: Evidence,
    protocol_version: u16,
    network_id: u32,
    batch_number: u64,
    first_sequence: u64,
    last_sequence: u64,
    data_availability_root: [u8; 32],
    record_roots: RootCommitments,
    resulting_state_root: [u8; 32],
}

impl ThresholdReport {
    /// Returns the exact evidence-gated level established by the certificate.
    #[must_use]
    pub const fn level(&self) -> layerx_types::verify::VerificationLevel {
        achieved(&self.evidence)
    }

    /// Returns the checkpoint and optional settlement identifiers that passed.
    #[must_use]
    pub const fn evidence(&self) -> &Evidence {
        &self.evidence
    }

    /// Returns the protocol version from the verified canonical header.
    #[must_use]
    pub const fn protocol_version(&self) -> u16 {
        self.protocol_version
    }

    /// Returns the network identifier from the verified canonical header.
    #[must_use]
    pub const fn network_id(&self) -> u32 {
        self.network_id
    }

    /// Returns the verified batch number committed by the checkpoint.
    #[must_use]
    pub const fn batch_number(&self) -> u64 {
        self.batch_number
    }

    /// Returns the first global sequence committed by the checkpoint batch.
    #[must_use]
    pub const fn first_sequence(&self) -> u64 {
        self.first_sequence
    }

    /// Returns the last global sequence committed by the checkpoint batch.
    #[must_use]
    pub const fn last_sequence(&self) -> u64 {
        self.last_sequence
    }

    /// Returns the data-availability root verified as part of the checkpoint.
    #[must_use]
    pub const fn data_availability_root(&self) -> [u8; 32] {
        self.data_availability_root
    }

    /// Returns the four record roots verified as part of the checkpoint.
    #[must_use]
    pub const fn record_roots(&self) -> RootCommitments {
        self.record_roots
    }

    /// Returns the resulting state root from the verified canonical header.
    #[must_use]
    pub const fn resulting_state_root(&self) -> [u8; 32] {
        self.resulting_state_root
    }
}

/// Exact certificate verification failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CheckpointError {
    /// Canonical header decoding or re-encoding failed.
    Header,
    /// The certificate refers to a different checkpoint identifier.
    CheckpointIdentifier,
    /// An attestation's committed checkpoint fields do not match its body.
    CheckpointFields,
    /// One guarantor appears more than once.
    DuplicateSigner([u8; 32]),
    /// One signer is absent from, or unbonded in, the supplied set.
    SignerMembership([u8; 32]),
    /// One included signature is invalid.
    Signature([u8; 32]),
    /// The achieved count is below the configured threshold.
    Threshold { achieved: usize, required: usize },
    /// The settlement reference is absent from or differs from registration.
    Settlement,
}

/// Recomputes the checkpoint identifier from its canonical body.
///
/// # Errors
///
/// Returns a header failure when canonical header validation or hashing fails.
pub fn checkpoint_id(checkpoint: &Checkpoint) -> Result<[u8; 32], CheckpointError> {
    let header =
        decode_batch_header(&checkpoint.header_bytes).map_err(|_| CheckpointError::Header)?;
    let encoded = encode_batch_header(&header).map_err(|_| CheckpointError::Header)?;
    if encoded != checkpoint.header_bytes {
        return Err(CheckpointError::Header);
    }
    hash_checkpoint_id(&encoded, &checkpoint.validity_proof).map_err(|_| CheckpointError::Header)
}

/// Verifies all certificate attestations against the bonded checkpoint set.
///
/// # Errors
///
/// Returns the exact identifier, field, distinctness, membership, signature,
/// threshold, or settlement check that failed. No partial level is returned.
pub fn verify_certificate(
    certificate: &Certificate,
    bonded_set: &[GuarantorKey],
    registered_checkpoint_id: &[u8; 32],
    expected_settlement_domain: SettlementDomain,
    registered_settlement_reference: Option<&[u8]>,
) -> Result<ThresholdReport, CheckpointError> {
    let header = decode_batch_header(&certificate.checkpoint.header_bytes)
        .map_err(|_| CheckpointError::Header)?;
    let identifier = checkpoint_id(&certificate.checkpoint)?;
    if &identifier != registered_checkpoint_id {
        return Err(CheckpointError::CheckpointIdentifier);
    }
    let mut seen = BTreeSet::new();
    let mut achieved = 0;
    if expected_settlement_domain.paxeer_chain_id == 0
        || expected_settlement_domain.settlement_contract == [0; 20]
    {
        return Err(CheckpointError::Settlement);
    }
    for attestation in &certificate.attestations {
        if !seen.insert(attestation.guarantor_id) {
            return Err(CheckpointError::DuplicateSigner(attestation.guarantor_id));
        }
        if attestation.protocol_version != header.protocol_version()
            || attestation.network_id != header.network_id()
            || attestation.paxeer_chain_id != expected_settlement_domain.paxeer_chain_id
            || attestation.paxeer_settlement_contract
                != expected_settlement_domain.settlement_contract
            || attestation.epoch != header.epoch()
            || attestation.checkpoint_id != identifier
            || attestation.checkpoint_hash != identifier
            || attestation.batch_number != header.batch_number()
            || attestation.data_availability_root != header.data_availability_root()
            || !attestation.replayed
            || !attestation.data_possessed
            || attestation.availability_class_mask != ALL_AVAILABILITY_CLASSES
            || attestation.attested_at_ms == 0
            || attestation.signer == [0; 20]
            || !matches!(attestation.signature_v, 27 | 28)
        {
            return Err(CheckpointError::CheckpointFields);
        }
        let key = bonded_set
            .iter()
            .find(|key| key.guarantor_id == attestation.guarantor_id && key.bonded)
            .ok_or(CheckpointError::SignerMembership(attestation.guarantor_id))?;
        let digest = checkpoint_attestation_digest(&attestation.message())
            .map_err(|_| CheckpointError::Signature(attestation.guarantor_id))?;
        if secp256k1::evm_address(&key.public_key)
            .map_err(|_| CheckpointError::Signature(attestation.guarantor_id))?
            != attestation.signer
        {
            return Err(CheckpointError::Signature(attestation.guarantor_id));
        }
        secp256k1::verify_recoverable_digest(
            &key.public_key,
            &attestation.signature,
            attestation.signature_v,
            &digest,
        )
            .map_err(|_| CheckpointError::Signature(attestation.guarantor_id))?;
        achieved += 1;
    }
    if certificate.threshold == 0 || achieved < certificate.threshold {
        return Err(CheckpointError::Threshold {
            achieved,
            required: certificate.threshold,
        });
    }
    let settlement_reference = if let Some(reference) = certificate.settlement_reference.as_deref()
    {
        if reference.is_empty() || registered_settlement_reference != Some(reference) {
            return Err(CheckpointError::Settlement);
        }
        Some(reference.to_vec())
    } else {
        None
    };
    Ok(ThresholdReport {
        achieved,
        required: certificate.threshold,
        evidence: Evidence::checkpoint(identifier, settlement_reference),
        protocol_version: header.protocol_version(),
        network_id: header.network_id(),
        batch_number: header.batch_number(),
        first_sequence: header.first_sequence(),
        last_sequence: header.last_sequence(),
        data_availability_root: header.data_availability_root(),
        record_roots: RootCommitments {
            activity: header.activity_merkle_root(),
            receipt: header.receipt_merkle_root(),
            event: header.event_merkle_root(),
            oracle: header.oracle_root(),
        },
        resulting_state_root: header.resulting_state_root(),
    })
}

//! Activity and state inclusion under an authorised signed batch header.

use layerx_crypto::ed25519;
use layerx_wire::hash::batch_header_digest;
use layerx_wire::receipt::{decode_batch_header, encode_batch_header, BatchHeader};

use crate::evidence::Evidence;
use crate::level::achieved;
use crate::merkle::{verify_path, MerkleError, Proof};

/// The sequencer authority record valid for an inclusive batch range.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SequencerAuthorization {
    sequencer_id: [u8; 32],
    public_key: [u8; 32],
    first_batch_number: u64,
    last_batch_number: u64,
}

impl SequencerAuthorization {
    /// Creates one core-published sequencer authorisation range.
    #[must_use]
    pub const fn new(
        sequencer_id: [u8; 32],
        public_key: [u8; 32],
        first_batch_number: u64,
        last_batch_number: u64,
    ) -> Self {
        Self {
            sequencer_id,
            public_key,
            first_batch_number,
            last_batch_number,
        }
    }
}

/// Exact failure class for header and inclusion verification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InclusionError {
    /// The header failed canonical decoding.
    HeaderDecode,
    /// The header did not re-encode to the supplied bytes.
    HeaderCanonical,
    /// The batch number lies outside the authorisation range.
    BatchNumber,
    /// The header sequencer identity is not the authorised identity.
    SequencerIdentity,
    /// The batch signature did not verify under the authorised public key.
    HeaderSignature,
    /// The named state root differs from the signed header commitment.
    StateRoot,
    /// The index-aware Merkle proof failed.
    Merkle(MerkleError),
}

/// A canonical header whose authority range and signature passed locally.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedBatchHeader {
    header: BatchHeader,
    canonical_bytes: Vec<u8>,
    digest: [u8; 32],
}

impl VerifiedBatchHeader {
    /// Borrows the exact decoded header.
    #[must_use]
    pub const fn header(&self) -> &BatchHeader {
        &self.header
    }

    /// Borrows the exact core-produced header bytes.
    #[must_use]
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical_bytes
    }

    /// Returns the exact signed header digest.
    #[must_use]
    pub const fn digest(&self) -> [u8; 32] {
        self.digest
    }
}

/// One locally established inclusion level and its signed batch header.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InclusionEvidence {
    header: VerifiedBatchHeader,
    evidence: Evidence,
}

impl InclusionEvidence {
    /// Borrows the signed batch-header evidence.
    #[must_use]
    pub const fn header(&self) -> &VerifiedBatchHeader {
        &self.header
    }

    /// Returns only the level established by the called verifier.
    #[must_use]
    pub const fn level(&self) -> layerx_types::verify::VerificationLevel {
        achieved(&self.evidence)
    }

    /// Returns the signed header and proof-root identifiers supporting the level.
    #[must_use]
    pub const fn evidence(&self) -> &Evidence {
        &self.evidence
    }
}

fn verify_header(
    header_bytes: &[u8],
    signature: &[u8; 64],
    authorization: &SequencerAuthorization,
) -> Result<VerifiedBatchHeader, InclusionError> {
    let header = decode_batch_header(header_bytes).map_err(|_| InclusionError::HeaderDecode)?;
    let reproduced = encode_batch_header(&header).map_err(|_| InclusionError::HeaderCanonical)?;
    if reproduced != header_bytes {
        return Err(InclusionError::HeaderCanonical);
    }
    if header.batch_number() < authorization.first_batch_number
        || header.batch_number() > authorization.last_batch_number
    {
        return Err(InclusionError::BatchNumber);
    }
    if header.sequencer_id() != authorization.sequencer_id {
        return Err(InclusionError::SequencerIdentity);
    }
    let digest = batch_header_digest(&reproduced).map_err(|_| InclusionError::HeaderCanonical)?;
    ed25519::verify_digest(&authorization.public_key, signature, &digest)
        .map_err(|_| InclusionError::HeaderSignature)?;
    Ok(VerifiedBatchHeader {
        header,
        canonical_bytes: reproduced,
        digest,
    })
}

/// Verifies canonical activity bytes against a signed batch activity root.
///
/// # Errors
///
/// Returns a typed header, authority, signature, or Merkle failure. No
/// batch-included evidence is returned unless every check passes.
pub fn verify_activity(
    activity_bytes: &[u8],
    proof: &Proof,
    header_bytes: &[u8],
    header_signature: &[u8; 64],
    authorization: &SequencerAuthorization,
) -> Result<InclusionEvidence, InclusionError> {
    let header = verify_header(header_bytes, header_signature, authorization)?;
    let activity_root = header.header.activity_merkle_root();
    verify_path(activity_bytes, proof, &activity_root).map_err(InclusionError::Merkle)?;
    let evidence = Evidence::batch(header.digest, activity_root);
    Ok(InclusionEvidence { header, evidence })
}

/// Verifies canonical receipt bytes against a signed batch receipt root.
///
/// # Errors
///
/// Returns a typed header, authority, signature, or Merkle failure. No
/// batch-included evidence is returned unless every check passes.
pub fn verify_receipt(
    receipt_bytes: &[u8],
    proof: &Proof,
    header_bytes: &[u8],
    header_signature: &[u8; 64],
    authorization: &SequencerAuthorization,
) -> Result<InclusionEvidence, InclusionError> {
    let header = verify_header(header_bytes, header_signature, authorization)?;
    let receipt_root = header.header.receipt_merkle_root();
    verify_path(receipt_bytes, proof, &receipt_root).map_err(InclusionError::Merkle)?;
    let evidence = Evidence::batch(header.digest, receipt_root);
    Ok(InclusionEvidence { header, evidence })
}

/// Verifies canonical state-leaf bytes against a named resulting state root
/// and the same root committed by an authorised signed batch header.
///
/// # Errors
///
/// Returns a typed header, root, signature, or Merkle failure. No state-proven
/// evidence is returned unless every check passes.
pub fn verify_state(
    state_leaf_bytes: &[u8],
    proof: &Proof,
    named_resulting_state_root: &[u8; 32],
    header_bytes: &[u8],
    header_signature: &[u8; 64],
    authorization: &SequencerAuthorization,
) -> Result<InclusionEvidence, InclusionError> {
    let header = verify_header(header_bytes, header_signature, authorization)?;
    if header.header.resulting_state_root() != *named_resulting_state_root {
        return Err(InclusionError::StateRoot);
    }
    verify_path(state_leaf_bytes, proof, named_resulting_state_root)
        .map_err(InclusionError::Merkle)?;
    let evidence = Evidence::state(header.digest, *named_resulting_state_root);
    Ok(InclusionEvidence { header, evidence })
}

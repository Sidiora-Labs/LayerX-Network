//! Explorer-facing mirror-only verification with pinned source provenance.

use layerx_mirror::source::{
    MirrorObservation, MirrorReadPolicy, MirrorSourceError, MirrorSources,
};
use layerx_mirror::{MirrorEvidenceLevel, MirrorVerifier, MirrorVerifyError, SignedHeaderTrust};
use layerx_proof::merkle::Proof;
use layerx_types::verify::VerificationLevel;

/// Display-safe result. A mirror fact cannot be returned without naming the
/// signed header and the exact mirrored head from which it was established.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MirrorReceiptReport {
    pub verification_level: VerificationLevel,
    pub receipt_digest: [u8; 32],
    pub batch_number: u64,
    pub signed_header_digest: [u8; 32],
    pub observation: MirrorObservation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MirrorStateReport {
    pub verification_level: VerificationLevel,
    pub batch_number: u64,
    pub signed_header_digest: [u8; 32],
    pub observation: MirrorObservation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExplorerMirrorError {
    Source(MirrorSourceError),
    Verification(MirrorVerifyError),
    MissingProvenance,
    ReceiptDigest,
}

impl From<MirrorSourceError> for ExplorerMirrorError {
    fn from(value: MirrorSourceError) -> Self {
        Self::Source(value)
    }
}

impl From<MirrorVerifyError> for ExplorerMirrorError {
    fn from(value: MirrorVerifyError) -> Self {
        Self::Verification(value)
    }
}

pub fn verify_mirrored_receipt(
    sources: &MirrorSources,
    batch_number: u64,
    policy: &MirrorReadPolicy,
    canonical_receipt: &[u8],
    trust: SignedHeaderTrust,
) -> Result<MirrorReceiptReport, ExplorerMirrorError> {
    let archive = sources.read(batch_number, policy)?;
    let verified = MirrorVerifier::from_source(archive, trust)?.receipt(canonical_receipt)?;
    let receipt_digest = verified
        .value()
        .evidence()
        .receipt_digest()
        .ok_or(ExplorerMirrorError::ReceiptDigest)?;
    Ok(MirrorReceiptReport {
        verification_level: level(verified.level()),
        receipt_digest,
        batch_number: verified.batch_number(),
        signed_header_digest: verified.signed_header_digest(),
        observation: verified
            .observation()
            .cloned()
            .ok_or(ExplorerMirrorError::MissingProvenance)?,
    })
}

pub fn verify_mirrored_state(
    sources: &MirrorSources,
    batch_number: u64,
    policy: &MirrorReadPolicy,
    canonical_state: &[u8],
    proof: &Proof,
    trust: SignedHeaderTrust,
) -> Result<MirrorStateReport, ExplorerMirrorError> {
    let archive = sources.read(batch_number, policy)?;
    let verified = MirrorVerifier::from_source(archive, trust)?.state(canonical_state, proof)?;
    Ok(MirrorStateReport {
        verification_level: level(verified.level()),
        batch_number: verified.batch_number(),
        signed_header_digest: verified.signed_header_digest(),
        observation: verified
            .observation()
            .cloned()
            .ok_or(ExplorerMirrorError::MissingProvenance)?,
    })
}

const fn level(value: MirrorEvidenceLevel) -> VerificationLevel {
    match value {
        MirrorEvidenceLevel::BatchIncluded => VerificationLevel::BATCH_INCLUDED,
        MirrorEvidenceLevel::StateProven => VerificationLevel::STATE_PROVEN,
    }
}

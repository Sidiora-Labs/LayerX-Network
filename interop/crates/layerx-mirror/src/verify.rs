//! Offline verification rooted only in a finalised mirror archive.

use layerx_crypto::ed25519;
use layerx_proof::inclusion::{verify_state, InclusionError, SequencerAuthorization};
use layerx_proof::merkle::{build_proof, verify_path, MerkleError, Proof};
use layerx_proof::receipt::{verify_outcome, AuthorizedBatch, ReceiptCheck, VerifiedReceipt};
use layerx_wire::hash::batch_header_digest;
use layerx_wire::receipt::{decode, decode_batch_header};

use crate::source::{MirrorLag, MirrorObservation, MirrorSourceFreshness, ObservedArchive};
use crate::{
    ArchiveData, ArchiveError, CheckpointCoordinate, CheckpointFreshness, MirrorFreshness,
};

/// Authority configured independently of mirror payloads. The public key and
/// range are never accepted from an archive.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SignedHeaderTrust {
    pub sequencer_id: [u8; 32],
    pub sequencer_public_key: [u8; 32],
    pub first_batch_number: u64,
    pub last_batch_number: u64,
}

/// Mirror coordinates displayed with every mirror-derived result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MirrorVerificationFreshness {
    pub latest_batch_mirrored: Option<u64>,
    pub latest_checkpoint_mirrored: Option<CheckpointCoordinate>,
    pub batch_lag: MirrorLag,
    pub checkpoint: Option<CheckpointFreshness>,
}

impl MirrorVerificationFreshness {
    #[must_use]
    pub const fn offline(
        latest_batch_mirrored: u64,
        latest_checkpoint_mirrored: Option<CheckpointCoordinate>,
    ) -> Self {
        Self {
            latest_batch_mirrored: Some(latest_batch_mirrored),
            latest_checkpoint_mirrored,
            batch_lag: MirrorLag::Unknown,
            checkpoint: None,
        }
    }

    #[must_use]
    pub fn relative(value: MirrorFreshness) -> Self {
        Self {
            latest_batch_mirrored: value.latest_batch_mirrored,
            latest_checkpoint_mirrored: value.latest_checkpoint_mirrored,
            batch_lag: MirrorLag::Known(value.batch_lag),
            checkpoint: Some(value.checkpoint),
        }
    }

    #[must_use]
    pub const fn from_source(value: MirrorSourceFreshness) -> Self {
        Self {
            latest_batch_mirrored: value.latest_batch,
            latest_checkpoint_mirrored: value.latest_checkpoint,
            batch_lag: value.batch_lag,
            checkpoint: None,
        }
    }
}

/// Evidence established from the archive commitment and an independently
/// configured sequencer key, without a `LayerX` RPC or hosted service.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MirrorVerification<T> {
    value: T,
    level: MirrorEvidenceLevel,
    batch_number: u64,
    signed_header_digest: [u8; 32],
    freshness: MirrorVerificationFreshness,
    observation: Option<MirrorObservation>,
}

impl<T> MirrorVerification<T> {
    #[must_use]
    pub const fn value(&self) -> &T {
        &self.value
    }

    #[must_use]
    pub const fn level(&self) -> MirrorEvidenceLevel {
        self.level
    }

    #[must_use]
    pub const fn batch_number(&self) -> u64 {
        self.batch_number
    }

    #[must_use]
    pub const fn signed_header_digest(&self) -> [u8; 32] {
        self.signed_header_digest
    }

    #[must_use]
    pub const fn freshness(&self) -> MirrorVerificationFreshness {
        self.freshness
    }

    #[must_use]
    pub fn observation(&self) -> Option<&MirrorObservation> {
        self.observation.as_ref()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MirrorEvidenceLevel {
    BatchIncluded,
    StateProven,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MirrorVerifyError {
    Archive(ArchiveError),
    Header,
    HeaderAuthority,
    HeaderSignature,
    ReceiptMissing,
    ReceiptDecode,
    ReceiptBatchMismatch,
    Receipt(ReceiptCheck),
    ReceiptInclusion(MerkleError),
    State(InclusionError),
    SourceCommitment,
    SourceBatch,
    SourceReorged,
    CheckpointTrustUnavailable,
}

/// Offline verifier over untrusted archive bytes and separately configured
/// signed-header authority.
pub struct MirrorVerifier {
    archive: ArchiveData,
    trust: SignedHeaderTrust,
    header_digest: [u8; 32],
    freshness: MirrorVerificationFreshness,
    observation: Option<MirrorObservation>,
}

impl MirrorVerifier {
    /// Admits one sealed source observation. Archive, chain identity,
    /// canonical position and freshness remain one indivisible source fact.
    pub fn from_source(
        observed: ObservedArchive,
        trust: SignedHeaderTrust,
    ) -> Result<Self, MirrorVerifyError> {
        let (archive_bytes, observation) = observed.into_parts();
        if crate::archive_commitment(&archive_bytes) != observation.commitment {
            return Err(MirrorVerifyError::SourceCommitment);
        }
        let freshness = MirrorVerificationFreshness::from_source(observation.freshness);
        Self::admit(&archive_bytes, trust, freshness, Some(observation))
    }

    fn admit(
        archive_bytes: &[u8],
        trust: SignedHeaderTrust,
        freshness: MirrorVerificationFreshness,
        observation: Option<MirrorObservation>,
    ) -> Result<Self, MirrorVerifyError> {
        let archive = ArchiveData::decode(archive_bytes).map_err(MirrorVerifyError::Archive)?;
        let header = decode_batch_header(&archive.canonical_batch_header)
            .map_err(|_| MirrorVerifyError::Header)?;
        if header.sequencer_id() != trust.sequencer_id
            || header.batch_number() < trust.first_batch_number
            || header.batch_number() > trust.last_batch_number
            || archive.batch_authorization.sequencer_id != trust.sequencer_id
            || archive.batch_authorization.sequencer_public_key != trust.sequencer_public_key
            || archive.batch_authorization.first_batch_number != trust.first_batch_number
            || archive.batch_authorization.last_batch_number != trust.last_batch_number
        {
            return Err(MirrorVerifyError::HeaderAuthority);
        }
        let header_digest = batch_header_digest(&archive.canonical_batch_header)
            .map_err(|_| MirrorVerifyError::Header)?;
        ed25519::verify_digest(
            &trust.sequencer_public_key,
            &archive.batch_authorization.header_signature,
            &header_digest,
        )
        .map_err(|_| MirrorVerifyError::HeaderSignature)?;
        if freshness
            .latest_batch_mirrored
            .is_some_and(|latest| latest < archive.batch_number)
        {
            return Err(MirrorVerifyError::HeaderAuthority);
        }
        if observation
            .as_ref()
            .is_some_and(|value| value.batch_number != archive.batch_number)
        {
            return Err(MirrorVerifyError::SourceBatch);
        }
        Ok(Self {
            archive,
            trust,
            header_digest,
            freshness,
            observation,
        })
    }

    /// Verifies a receipt retained by the archive against both its sequencer
    /// signature and the receipt root in the independently signed header.
    ///
    /// # Errors
    ///
    /// Refuses absent or non-included receipts and any canonical, invariant,
    /// or sequencer-signature failure.
    pub fn receipt(
        &self,
        canonical_receipt: &[u8],
    ) -> Result<MirrorVerification<VerifiedReceipt>, MirrorVerifyError> {
        let index = self
            .archive
            .records
            .receipts
            .iter()
            .position(|record| record == canonical_receipt)
            .ok_or(MirrorVerifyError::ReceiptMissing)?;
        let leaves = self
            .archive
            .records
            .receipts
            .iter()
            .map(Vec::as_slice)
            .collect::<Vec<_>>();
        let (proof, root) =
            build_proof(&leaves, index).map_err(MirrorVerifyError::ReceiptInclusion)?;
        if root != self.archive.record_roots.receipt {
            return Err(MirrorVerifyError::ReceiptInclusion(
                MerkleError::RootMismatch,
            ));
        }
        verify_path(canonical_receipt, &proof, &root)
            .map_err(MirrorVerifyError::ReceiptInclusion)?;
        let decoded = decode(canonical_receipt).map_err(|_| MirrorVerifyError::ReceiptDecode)?;
        let receipt = decoded.protocol().ok_or(MirrorVerifyError::ReceiptDecode)?;
        let header = decode_batch_header(&self.archive.canonical_batch_header)
            .map_err(|_| MirrorVerifyError::Header)?;
        if receipt.previous_state_root() != header.previous_state_root()
            || receipt.resulting_state_root() != header.resulting_state_root()
            || receipt.global_sequence() < header.first_sequence()
            || receipt.global_sequence() > header.last_sequence()
        {
            return Err(MirrorVerifyError::ReceiptBatchMismatch);
        }
        let authorised = AuthorizedBatch::new(
            receipt.batch_id(),
            receipt.asset(),
            receipt.previous_state_root(),
            receipt.resulting_state_root(),
            self.trust.sequencer_public_key,
        );
        let value = verify_outcome(canonical_receipt, &authorised)
            .map_err(|failure| MirrorVerifyError::Receipt(failure.check))?;
        Ok(self.report(value, MirrorEvidenceLevel::BatchIncluded))
    }

    /// Verifies caller-supplied state inclusion against this archive's signed
    /// header. State bytes and proof remain untrusted inputs.
    ///
    /// # Errors
    ///
    /// Refuses invalid state proofs and any signed-header authority mismatch.
    pub fn state(
        &self,
        canonical_state: &[u8],
        proof: &Proof,
    ) -> Result<MirrorVerification<layerx_proof::inclusion::InclusionEvidence>, MirrorVerifyError>
    {
        let header = decode_batch_header(&self.archive.canonical_batch_header)
            .map_err(|_| MirrorVerifyError::Header)?;
        let authority = SequencerAuthorization::new(
            self.trust.sequencer_id,
            self.trust.sequencer_public_key,
            self.trust.first_batch_number,
            self.trust.last_batch_number,
        );
        let value = verify_state(
            canonical_state,
            proof,
            &header.resulting_state_root(),
            &self.archive.canonical_batch_header,
            &self.archive.batch_authorization.header_signature,
            &authority,
        )
        .map_err(MirrorVerifyError::State)?;
        Ok(self.report(value, MirrorEvidenceLevel::StateProven))
    }

    fn report<T>(&self, value: T, level: MirrorEvidenceLevel) -> MirrorVerification<T> {
        MirrorVerification {
            value,
            level,
            batch_number: self.archive.batch_number,
            signed_header_digest: self.header_digest,
            freshness: self.freshness,
            observation: self.observation.clone(),
        }
    }

    /// The v2 archive carries canonical checkpoint bytes but omits the
    /// attestation replay/possession timestamps needed by the protocol-owned
    /// checkpoint verifier. It must never be promoted to checkpoint level.
    pub const fn checkpoint_level(&self) -> Result<(), MirrorVerifyError> {
        Err(MirrorVerifyError::CheckpointTrustUnavailable)
    }
}

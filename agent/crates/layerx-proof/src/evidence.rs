//! Identifiers for the exact bytes and commitments supporting a level.

use layerx_types::verify::VerificationLevel;

use crate::level::Achieved;

/// Evidence identifiers inseparably paired with an achieved level.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Evidence {
    achieved: Achieved,
    receipt_digest: Option<[u8; 32]>,
    header_digest: Option<[u8; 32]>,
    proof_root: Option<[u8; 32]>,
    checkpoint_id: Option<[u8; 32]>,
    settlement_reference: Option<Vec<u8>>,
}

impl Evidence {
    pub(crate) const fn sequencer(receipt_digest: [u8; 32]) -> Self {
        Self {
            achieved: Achieved::sequencer_signed(),
            receipt_digest: Some(receipt_digest),
            header_digest: None,
            proof_root: None,
            checkpoint_id: None,
            settlement_reference: None,
        }
    }

    pub(crate) const fn batch(header_digest: [u8; 32], proof_root: [u8; 32]) -> Self {
        Self {
            achieved: Achieved::batch_included(),
            receipt_digest: None,
            header_digest: Some(header_digest),
            proof_root: Some(proof_root),
            checkpoint_id: None,
            settlement_reference: None,
        }
    }

    pub(crate) const fn state(header_digest: [u8; 32], proof_root: [u8; 32]) -> Self {
        Self {
            achieved: Achieved::state_proven(),
            receipt_digest: None,
            header_digest: Some(header_digest),
            proof_root: Some(proof_root),
            checkpoint_id: None,
            settlement_reference: None,
        }
    }

    pub(crate) fn checkpoint(
        checkpoint_id: [u8; 32],
        settlement_reference: Option<Vec<u8>>,
    ) -> Self {
        Self {
            // A matching registration reference proves only that the
            // checkpoint was registered. Settlement anchoring requires a
            // separate live Paxeer finality verifier and is never inferred
            // from transported bytes.
            achieved: Achieved::checkpoint_finalised(),
            receipt_digest: None,
            header_digest: None,
            proof_root: None,
            checkpoint_id: Some(checkpoint_id),
            settlement_reference,
        }
    }

    pub(crate) const fn achieved(&self) -> Achieved {
        self.achieved
    }

    /// Returns the exact level established by the producing verifier.
    #[must_use]
    pub const fn level(&self) -> VerificationLevel {
        self.achieved.level()
    }

    /// Returns the signed receipt digest when receipt verification produced it.
    #[must_use]
    pub const fn receipt_digest(&self) -> Option<[u8; 32]> {
        self.receipt_digest
    }

    /// Returns the signed batch-header digest when inclusion produced it.
    #[must_use]
    pub const fn header_digest(&self) -> Option<[u8; 32]> {
        self.header_digest
    }

    /// Returns the root against which the path was verified.
    #[must_use]
    pub const fn proof_root(&self) -> Option<[u8; 32]> {
        self.proof_root
    }

    /// Returns the recomputed registered checkpoint identifier.
    #[must_use]
    pub const fn checkpoint_id(&self) -> Option<[u8; 32]> {
        self.checkpoint_id
    }

    /// Borrows the exact matched settlement-registration reference. Its
    /// presence does not establish Paxeer settlement anchoring.
    #[must_use]
    pub fn settlement_reference(&self) -> Option<&[u8]> {
        self.settlement_reference.as_deref()
    }
}

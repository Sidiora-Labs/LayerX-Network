//! Evidence-gated verification-level production.

use layerx_types::verify::VerificationLevel;

use crate::evidence::Evidence;

/// A level token constructible only by proof routines in this crate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Achieved(VerificationLevel);

impl Achieved {
    pub(crate) const fn sequencer_signed() -> Self {
        Self(VerificationLevel::SEQUENCER_SIGNED)
    }

    pub(crate) const fn batch_included() -> Self {
        Self(VerificationLevel::BATCH_INCLUDED)
    }

    pub(crate) const fn state_proven() -> Self {
        Self(VerificationLevel::STATE_PROVEN)
    }

    pub(crate) const fn checkpoint_finalised() -> Self {
        Self(VerificationLevel::CHECKPOINT_FINALISED)
    }

    pub(crate) const fn level(self) -> VerificationLevel {
        self.0
    }
}

/// Returns the exact level inseparably carried by verified evidence.
///
/// Callers can request and compare public lattice values, but cannot construct
/// an `Evidence` or raise its private achieved token.
#[must_use]
pub const fn achieved(evidence: &Evidence) -> VerificationLevel {
    evidence.achieved().level()
}

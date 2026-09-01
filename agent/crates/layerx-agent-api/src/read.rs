//! Verified read, proof, availability, export, and projection contract types.

use crate::identity::{Asset, ContractError};
use crate::verify::Level;
use crate::write_contract::CanonicalBytes;
use crate::{Amount, Sequence, TimestampSeconds};

macro_rules! required_reference {
    ($name:ident, $field:literal) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(String);

        impl $name {
            /// Constructs a non-empty read reference.
            ///
            /// # Errors
            /// Returns [`ContractError::Empty`] when the reference is empty.
            pub fn new(value: impl Into<String>) -> Result<Self, ContractError> {
                let value = value.into();
                if value.is_empty() {
                    return Err(ContractError::Empty($field));
                }
                Ok(Self(value))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

required_reference!(AccountRef, "account");
required_reference!(ModuleRef, "module");
required_reference!(BatchRef, "batch");
required_reference!(CheckpointRef, "checkpoint");
required_reference!(HistoryCursor, "cursor");
required_reference!(ProviderRef, "provider");
required_reference!(FactRef, "fact");

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RelativeTo {
    Batch(BatchRef),
    Checkpoint(CheckpointRef),
}

/// Full freshness coordinate attached to every authoritative read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Freshness {
    pub chain_head: Sequence,
    pub latest_sealed_batch: BatchRef,
    pub latest_finalised_checkpoint: CheckpointRef,
    pub value_sequence: Sequence,
    pub relative_to: RelativeTo,
}

mod sealed {
    pub trait Sealed {}
}

/// Values whose bytes and meaning originate from the `LayerX` core or its evidence.
pub trait CoreProduced: sealed::Sealed {}

/// An authoritative value that always carries achieved proof level and freshness.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedRead<T: CoreProduced> {
    pub value: T,
    pub achieved_verification_level: Level,
    pub freshness: Freshness,
}

impl<T: CoreProduced> VerifiedRead<T> {
    #[must_use]
    pub const fn new(value: T, achieved_verification_level: Level, freshness: Freshness) -> Self {
        Self {
            value,
            achieved_verification_level,
            freshness,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadRequest<S> {
    pub selector: S,
    pub requested_verification_level: Level,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BalanceSelector {
    pub account: AccountRef,
    pub asset: Asset,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleStateSelector {
    pub module: ModuleRef,
    pub key: CanonicalBytes,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistorySelector {
    pub first: Sequence,
    pub last: Sequence,
    pub cursor: Option<HistoryCursor>,
    pub page_limit: u32,
}

impl HistorySelector {
    /// Validates an ordered, bounded history request.
    ///
    /// # Errors
    /// Returns [`ContractError::Zero`] for an inverted range or zero page limit.
    pub fn validate(self) -> Result<Self, ContractError> {
        if self.last.0 < self.first.0 {
            return Err(ContractError::Zero("history_range"));
        }
        if self.page_limit == 0 {
            return Err(ContractError::Zero("page_limit"));
        }
        Ok(self)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BalanceValue {
    pub account: AccountRef,
    pub asset: Asset,
    pub amount: Amount,
    pub canonical_state: CanonicalBytes,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountValue(pub CanonicalBytes);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleStateValue(pub CanonicalBytes);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryValue {
    pub records: Vec<CanonicalBytes>,
    pub next_cursor: Option<HistoryCursor>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchValue(pub CanonicalBytes);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckpointValue(pub CanonicalBytes);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProofBundle {
    pub target: CanonicalBytes,
    pub proofs: Vec<CanonicalBytes>,
}

macro_rules! core_produced {
    ($($name:ty),+ $(,)?) => {
        $(
            impl sealed::Sealed for $name {}
            impl CoreProduced for $name {}
        )+
    };
}

core_produced!(
    BalanceValue,
    AccountValue,
    ModuleStateValue,
    HistoryValue,
    BatchValue,
    CheckpointValue,
    ProofBundle,
);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AvailabilityClass {
    Activities,
    Receipts,
    Oracle,
    StateDiff,
    Recovery,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClassReport {
    pub class: AvailabilityClass,
    pub complete: bool,
    pub verified_chunks: u32,
    pub verified_bytes: u64,
    pub failure: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderReport {
    pub provider: ProviderRef,
    pub classes: Vec<ClassReport>,
    pub failure: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AvailabilityCompletion {
    Complete { provider: ProviderRef },
    Partial,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AvailabilityReport {
    pub completion: AvailabilityCompletion,
    pub classes: Vec<ClassReport>,
    pub providers: Vec<ProviderReport>,
}

impl sealed::Sealed for AvailabilityReport {}
impl CoreProduced for AvailabilityReport {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AvailabilityRequest {
    pub selector: String,
    pub requested_verification_level: Level,
    pub maximum_bytes: u64,
    pub maximum_chunks: u32,
    pub deadline: TimestampSeconds,
}

impl AvailabilityRequest {
    /// Enforces finite retrieval bounds.
    ///
    /// # Errors
    /// Returns [`ContractError::Zero`] for any zero bound or empty selector.
    pub fn validate(self) -> Result<Self, ContractError> {
        if self.selector.is_empty() {
            return Err(ContractError::Empty("availability_selector"));
        }
        if self.maximum_bytes == 0 || self.maximum_chunks == 0 || self.deadline.0 == 0 {
            return Err(ContractError::Zero("availability_bound"));
        }
        Ok(self)
    }
}

/// Self-contained artifacts needed to verify a stated fact set offline.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OfflineExport {
    pub facts: Vec<FactRef>,
    pub receipts: Vec<CanonicalBytes>,
    pub proofs: Vec<CanonicalBytes>,
    pub certificates: Vec<CanonicalBytes>,
    pub headers: Vec<CanonicalBytes>,
}

impl sealed::Sealed for OfflineExport {}
impl CoreProduced for OfflineExport {}

impl OfflineExport {
    /// Refuses an export that states no facts or carries no verification evidence.
    ///
    /// # Errors
    /// Returns [`ContractError::Empty`] when facts or all evidence are absent.
    pub fn validate(self) -> Result<Self, ContractError> {
        if self.facts.is_empty() {
            return Err(ContractError::Empty("fact_set"));
        }
        if self.receipts.is_empty()
            && self.proofs.is_empty()
            && self.certificates.is_empty()
            && self.headers.is_empty()
        {
            return Err(ContractError::Empty("offline_evidence"));
        }
        Ok(self)
    }
}

/// Explicitly non-authoritative estimate; it cannot satisfy [`CoreProduced`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectionResult<T> {
    pub projected: T,
    pub rationale: String,
    pub observed_freshness: Freshness,
}

impl<T> ProjectionResult<T> {
    /// Creates a projection with a mandatory rationale.
    ///
    /// # Errors
    /// Returns [`ContractError::Empty`] when the rationale is empty.
    pub fn new(
        projected: T,
        rationale: impl Into<String>,
        observed_freshness: Freshness,
    ) -> Result<Self, ContractError> {
        let rationale = rationale.into();
        if rationale.is_empty() {
            return Err(ContractError::Empty("projection_rationale"));
        }
        Ok(Self {
            projected,
            rationale,
            observed_freshness,
        })
    }
}

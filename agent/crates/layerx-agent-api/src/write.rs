//! Prepare, sign, submit, track, and wait contract types.

use crate::identity::{ActivityType, AgentDid, Asset, AuthorityRef, ContractError, ExplicitSet};
use crate::verify::Level;
use crate::{Amount, Sequence, TimestampSeconds};
use layerx_types::result::ResultCode;

macro_rules! required_bytes {
    ($name:ident, $field:literal) => {
        #[derive(Clone, Debug, Eq, PartialEq)]
        pub struct $name(Vec<u8>);

        impl $name {
            /// Constructs a required non-empty byte string.
            ///
            /// # Errors
            /// Returns [`ContractError::Empty`] for an empty value.
            pub fn new(value: Vec<u8>) -> Result<Self, ContractError> {
                if value.is_empty() {
                    return Err(ContractError::Empty($field));
                }
                Ok(Self(value))
            }

            #[must_use]
            pub fn as_bytes(&self) -> &[u8] {
                &self.0
            }
        }
    };
}

macro_rules! required_reference {
    ($name:ident, $field:literal) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(String);

        impl $name {
            /// Constructs a required non-empty reference.
            ///
            /// # Errors
            /// Returns [`ContractError::Empty`] for an empty reference.
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

required_bytes!(CanonicalBytes, "canonical_bytes");
required_bytes!(SigningPreimage, "signing_preimage");
required_bytes!(SignatureBytes, "signature");
required_bytes!(PayloadBytes, "payload");
required_reference!(PreparationRef, "preparation_ref");
required_reference!(SubmissionRef, "submission_ref");
required_reference!(ReceiptRef, "receipt_ref");
required_reference!(IdempotencyRef, "idempotency_key");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimestampBound {
    pub not_before: TimestampSeconds,
    pub not_after: TimestampSeconds,
}

impl TimestampBound {
    /// Validates a non-inverted bound.
    ///
    /// # Errors
    /// Returns [`ContractError::Zero`] when the end precedes the start.
    pub const fn validate(self) -> Result<Self, ContractError> {
        if self.not_after.0 < self.not_before.0 {
            return Err(ContractError::Zero("timestamp_bound"));
        }
        Ok(self)
    }
}

/// Complete request from which the daemon builds canonical unsigned bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrepareRequest {
    /// Full protocol module and ordinal packed into the canonical u32 form.
    pub protocol_activity_type: u32,
    pub actor: AgentDid,
    pub authority: AuthorityRef,
    pub account_sequence: Sequence,
    pub timestamp_bound: TimestampBound,
    pub idempotency_key: IdempotencyRef,
    pub fee_limit: Amount,
    pub payload: PayloadBytes,
    pub payload_hash: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisclosedAmount {
    pub counterparty: AgentDid,
    pub amount: Amount,
}

/// Meaning decoded from, and digest-bound to, the returned canonical bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Disclosure {
    pub canonical_digest: [u8; 32],
    pub activity_type: ActivityType,
    pub actor: AgentDid,
    pub authority: AuthorityRef,
    pub counterparties: ExplicitSet<AgentDid>,
    pub amounts: ExplicitSet<DisclosedAmount>,
    pub asset: Asset,
    pub fee_limit: Amount,
    pub expiry: TimestampSeconds,
    pub idempotency_key: IdempotencyRef,
}

/// Byte-exact preparation and its disclosure from the same decoded bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Prepared {
    pub preparation_ref: PreparationRef,
    pub unsigned_canonical_bytes: CanonicalBytes,
    pub signing_preimage: SigningPreimage,
    pub disclosure: Disclosure,
    pub expiry: TimestampSeconds,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignRequest {
    pub preparation_ref: PreparationRef,
    pub signature: SignatureBytes,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubmitRequest {
    pub preparation_ref: PreparationRef,
    pub signature: SignatureBytes,
    pub approval_release_ref: Option<[u8; 32]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrackRequest {
    pub submission_ref: SubmissionRef,
}

/// Exactly the durable submission state machine. Executed carries a mandatory receipt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SubmissionState {
    Prepared,
    Signed,
    Queued,
    Submitted,
    Acknowledged,
    Unknown,
    Executed { receipt_ref: ReceiptRef },
    Failed { result: ResultCode },
    Expired,
}

impl SubmissionState {
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Signed => "signed",
            Self::Queued => "queued",
            Self::Submitted => "submitted",
            Self::Acknowledged => "acknowledged",
            Self::Unknown => "unknown",
            Self::Executed { .. } => "executed",
            Self::Failed { .. } => "failed",
            Self::Expired => "expired",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Transition {
    pub from: SubmissionState,
    pub to: SubmissionState,
    pub cause: String,
    pub at: TimestampSeconds,
}

impl Transition {
    /// Rejects a transition without a recorded cause.
    ///
    /// # Errors
    /// Returns [`ContractError::Empty`] when cause is empty.
    pub fn validate(self) -> Result<Self, ContractError> {
        if self.cause.is_empty() {
            return Err(ContractError::Empty("transition_cause"));
        }
        Ok(self)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceRef {
    pub kind: String,
    pub digest: [u8; 32],
}

/// Current state and only the level actually justified by attached evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrackedSubmission {
    pub submission_ref: SubmissionRef,
    pub state: SubmissionState,
    pub evidence: Vec<EvidenceRef>,
    pub verification_level: Level,
    pub transitions: Vec<Transition>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WaitRequest {
    pub submission_ref: SubmissionRef,
    pub requested_verification_level: Level,
    pub deadline: TimestampSeconds,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WaitResult {
    pub submission: TrackedSubmission,
    pub actual_verification_level: Level,
    pub deadline_elapsed: bool,
}

//! Total API error, idempotency, and verification-level contract.

use layerx_types::error::ErrorClass as LayerErrorClass;
use layerx_types::result::ResultCode;
use layerx_types::verify::VerificationLevel;

use crate::identity::ContractError;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RequestId(pub u64);

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ReasonCode(String);

impl ReasonCode {
    /// Constructs a stable lowercase machine-readable reason.
    ///
    /// # Errors
    /// Returns [`ContractError::Empty`] for empty or non-machine-readable text.
    pub fn new(value: impl Into<String>) -> Result<Self, ContractError> {
        let value = value.into();
        if value.is_empty()
            || !value.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_.".contains(&byte)
            })
        {
            return Err(ContractError::Empty("reason"));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Stable layer-level classes, including a distinct missing-capability class.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ErrorClass {
    TransportFailure,
    Deadline,
    ProtocolIncompatibility,
    UnavailableCapability,
    CoreRejection,
    VerificationFailure,
    PolicyRefusal,
    CapabilityRefusal,
    BudgetRefusal,
    RateLimit,
    IdempotencyConflict,
    InternalFault,
}

impl ErrorClass {
    pub const ALL: &'static [Self] = &[
        Self::TransportFailure,
        Self::Deadline,
        Self::ProtocolIncompatibility,
        Self::UnavailableCapability,
        Self::CoreRejection,
        Self::VerificationFailure,
        Self::PolicyRefusal,
        Self::CapabilityRefusal,
        Self::BudgetRefusal,
        Self::RateLimit,
        Self::IdempotencyConflict,
        Self::InternalFault,
    ];
}

impl From<LayerErrorClass> for ErrorClass {
    fn from(value: LayerErrorClass) -> Self {
        match value {
            LayerErrorClass::TransportFailure => Self::TransportFailure,
            LayerErrorClass::Deadline => Self::Deadline,
            LayerErrorClass::ProtocolIncompatibility => Self::ProtocolIncompatibility,
            LayerErrorClass::UnavailableCapability => Self::UnavailableCapability,
            LayerErrorClass::CoreRejection => Self::CoreRejection,
            LayerErrorClass::VerificationFailure => Self::VerificationFailure,
            LayerErrorClass::PolicyRefusal => Self::PolicyRefusal,
            LayerErrorClass::CapabilityRefusal => Self::CapabilityRefusal,
            LayerErrorClass::BudgetRefusal => Self::BudgetRefusal,
            LayerErrorClass::RateLimit => Self::RateLimit,
            LayerErrorClass::InternalFault => Self::InternalFault,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Retriability {
    Terminal,
    Retriable,
}

/// One lossless error envelope used by all generated SDKs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApiError {
    pub class: ErrorClass,
    pub protocol_result_code: Option<ResultCode>,
    pub retriability: Retriability,
    pub request_id: RequestId,
    pub reason: ReasonCode,
}

impl ApiError {
    /// Constructs the non-emulated unavailable-capability error.
    ///
    /// # Errors
    /// Returns [`ContractError`] if the capability cannot form a reason code.
    pub fn unavailable_capability(
        request_id: RequestId,
        capability: &str,
    ) -> Result<Self, ContractError> {
        let reason = ReasonCode::new(format!("unavailable_capability.{capability}"))?;
        Ok(Self {
            class: ErrorClass::UnavailableCapability,
            protocol_result_code: None,
            retriability: Retriability::Terminal,
            request_id,
            reason,
        })
    }
}

/// Ordered contract-level verification lattice shared by every SDK.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum Level {
    Unverified = 0,
    SequencerSigned = 1,
    BatchIncluded = 2,
    StateProven = 3,
    CheckpointFinalised = 4,
    SettlementAnchored = 5,
}

impl From<VerificationLevel> for Level {
    fn from(value: VerificationLevel) -> Self {
        match value.wire_rank() {
            0 => Self::Unverified,
            1 => Self::SequencerSigned,
            2 => Self::BatchIncluded,
            3 => Self::StateProven,
            4 => Self::CheckpointFinalised,
            _ => Self::SettlementAnchored,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VerificationStatus {
    Achieved(Level),
    Unverified {
        requested: Level,
        achieved: Level,
        reason: ReasonCode,
    },
}

/// The only successful API envelope; verification status cannot be omitted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApiSuccess<T> {
    pub request_id: RequestId,
    pub value: T,
    pub verification_status: VerificationStatus,
}

/// Caller-supplied idempotency key.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Key([u8; 32]);

impl Key {
    /// Rejects the reserved all-zero key.
    ///
    /// # Errors
    /// Returns [`ContractError::Empty`] for the all-zero key.
    pub fn new(bytes: [u8; 32]) -> Result<Self, ContractError> {
        if bytes == [0; 32] {
            return Err(ContractError::Empty("idempotency_key"));
        }
        Ok(Self(bytes))
    }

    #[must_use]
    pub const fn bytes(self) -> [u8; 32] {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BodyDigest(pub [u8; 32]);

/// Mandatory outer envelope for every mutating operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdempotentMutation<T> {
    pub request_id: RequestId,
    pub key: Key,
    pub body_digest: BodyDigest,
    pub operation: T,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IdempotencyOutcome<T> {
    First(T),
    RepeatedOriginal(T),
    Conflict {
        original_body: BodyDigest,
        repeated_body: BodyDigest,
    },
}

/// Classifies a repeated key without ever replacing the original result.
#[must_use]
pub fn classify_repeat<T>(
    original_body: BodyDigest,
    repeated_body: BodyDigest,
    original_result: T,
) -> IdempotencyOutcome<T> {
    if original_body == repeated_body {
        IdempotencyOutcome::RepeatedOriginal(original_result)
    } else {
        IdempotencyOutcome::Conflict {
            original_body,
            repeated_body,
        }
    }
}

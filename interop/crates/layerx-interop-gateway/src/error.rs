//! Typed gateway errors mirroring the human plane's error discipline: every
//! failure carries a stable machine code, a declared retriability, and the
//! trace it travelled under, and renders through the redaction-gated
//! error-response emission only.

use std::fmt::{Display, Formatter};

use layerx_proof::receipt::VerificationFailure;

use crate::adapter::AdapterError;
use crate::audit::AuditError;
use crate::principal::StoreError;
use crate::redaction::{emit, FieldValue, Label, RedactionError, ERROR_RESPONSE_SCHEMA};
use crate::trace::Traced;

/// Whether retrying the failed translation can succeed without a change.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Retriability {
    Retriable,
    Terminal,
}

impl Retriability {
    /// Returns the emission label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Retriable => "retriable",
            Self::Terminal => "terminal",
        }
    }
}

/// Every typed failure the gateway core returns.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GatewayError {
    Adapter(AdapterError),
    Store(StoreError),
    Audit(AuditError),
    DuplicateAdapter,
    UnknownAdapter,
    InvalidTranslation,
    UnknownTranslation,
    IdempotencyConflict,
    ReceiptRequired,
    NotStateChanging,
    TranslationClosed,
    ReceiptRejected(VerificationFailure),
    Corrupt(&'static str),
}

impl GatewayError {
    /// Returns the stable machine code named on the error response.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Adapter(_) => "adapter-declaration",
            Self::Store(_) => "principal-scope",
            Self::Audit(_) => "audit-append",
            Self::DuplicateAdapter => "adapter-duplicate",
            Self::UnknownAdapter => "adapter-unknown",
            Self::InvalidTranslation => "translation-invalid",
            Self::UnknownTranslation => "translation-unknown",
            Self::IdempotencyConflict => "idempotency-conflict",
            Self::ReceiptRequired => "receipt-required",
            Self::NotStateChanging => "translation-read-only",
            Self::TranslationClosed => "translation-closed",
            Self::ReceiptRejected(_) => "receipt-rejected",
            Self::Corrupt(_) => "record-corrupt",
        }
    }

    /// Returns the declared retriability of this failure.
    #[must_use]
    pub const fn retriability(&self) -> Retriability {
        match self {
            Self::Audit(_) | Self::ReceiptRejected(_) => Retriability::Retriable,
            Self::Adapter(_)
            | Self::Store(_)
            | Self::DuplicateAdapter
            | Self::UnknownAdapter
            | Self::InvalidTranslation
            | Self::UnknownTranslation
            | Self::IdempotencyConflict
            | Self::ReceiptRequired
            | Self::NotStateChanging
            | Self::TranslationClosed
            | Self::Corrupt(_) => Retriability::Terminal,
        }
    }
}

impl Display for GatewayError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Adapter(error) => write!(formatter, "adapter declaration refused: {error}"),
            Self::Store(error) => write!(formatter, "principal scope refused: {error}"),
            Self::Audit(error) => write!(formatter, "audit append failed: {error}"),
            Self::DuplicateAdapter => {
                formatter.write_str("adapter identifier is already registered")
            }
            Self::UnknownAdapter => formatter.write_str("adapter is not registered"),
            Self::InvalidTranslation => {
                formatter.write_str("translation request is outside its declared bounds")
            }
            Self::UnknownTranslation => {
                formatter.write_str("translation is unknown in this principal scope")
            }
            Self::IdempotencyConflict => {
                formatter.write_str("idempotency key was reused with different content")
            }
            Self::ReceiptRequired => formatter.write_str(
                "a state-changing translation terminates only in a receipt-verified operation",
            ),
            Self::NotStateChanging => {
                formatter.write_str("a read-only translation cannot claim a settlement receipt")
            }
            Self::TranslationClosed => {
                formatter.write_str("translation already reached a terminal refusal")
            }
            Self::ReceiptRejected(failure) => {
                write!(
                    formatter,
                    "receipt verification failed: {:?}",
                    failure.check
                )
            }
            Self::Corrupt(reason) => write!(formatter, "corrupt gateway record: {reason}"),
        }
    }
}

impl std::error::Error for GatewayError {}

impl From<AdapterError> for GatewayError {
    fn from(value: AdapterError) -> Self {
        Self::Adapter(value)
    }
}

impl From<StoreError> for GatewayError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<AuditError> for GatewayError {
    fn from(value: AuditError) -> Self {
        Self::Audit(value)
    }
}

/// Renders one traced failure as the redaction-gated error-response emission:
/// stable code, declared retriability, and the trace it travelled under -
/// nothing else can leave the gateway on the error path.
///
/// # Errors
///
/// Returns an emission refusal from the redaction gate.
pub fn error_emission(traced: &Traced<GatewayError>) -> Result<Vec<u8>, RedactionError> {
    emit(
        ERROR_RESPONSE_SCHEMA,
        &[
            FieldValue::Label(Label::new(traced.error().code())?),
            FieldValue::Label(Label::new(traced.error().retriability().label())?),
            FieldValue::Trace(traced.trace().clone()),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::{error_emission, GatewayError, Retriability};
    use crate::adapter::AdapterError;
    use crate::audit::AuditError;
    use crate::principal::StoreError;
    use crate::redaction::{decode, FieldValue};
    use crate::trace::TraceId;

    const EVERY_ERROR: &[GatewayError] = &[
        GatewayError::Adapter(AdapterError::UnpinnedVersion),
        GatewayError::Store(StoreError::InvalidPrincipal),
        GatewayError::Audit(AuditError::Unhashable),
        GatewayError::DuplicateAdapter,
        GatewayError::UnknownAdapter,
        GatewayError::InvalidTranslation,
        GatewayError::UnknownTranslation,
        GatewayError::IdempotencyConflict,
        GatewayError::ReceiptRequired,
        GatewayError::NotStateChanging,
        GatewayError::TranslationClosed,
        GatewayError::Corrupt("test"),
    ];

    #[test]
    fn every_failure_renders_a_redacted_code_retriability_and_trace() {
        let trace = TraceId::mint([9; 16]);
        for error in EVERY_ERROR {
            let traced = trace.wrap(*error);
            let bytes =
                error_emission(&traced).unwrap_or_else(|error| panic!("emission refused: {error}"));
            let emission =
                decode(&bytes).unwrap_or_else(|error| panic!("emission unreadable: {error}"));
            let [FieldValue::Label(code), FieldValue::Label(retriability), FieldValue::Trace(emitted)] =
                emission.values()
            else {
                panic!("error emission shape drifted");
            };
            assert_eq!(code.as_str(), traced.error().code());
            assert_eq!(retriability.as_str(), traced.error().retriability().label());
            assert_eq!(emitted, &trace);
        }
    }

    #[test]
    fn conflicts_and_authority_refusals_are_terminal() {
        assert_eq!(
            GatewayError::IdempotencyConflict.retriability(),
            Retriability::Terminal
        );
        assert_eq!(
            GatewayError::ReceiptRequired.retriability(),
            Retriability::Terminal
        );
        assert_eq!(
            GatewayError::DuplicateAdapter.retriability(),
            Retriability::Terminal
        );
    }
}

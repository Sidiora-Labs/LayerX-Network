//! Uniform output redaction for audit, log, metric, trace, and error surfaces.
//!
//! The model is intentionally conservative: key material, daemon tokens, and secret
//! configuration are never rendered; identifiers and core receipts render only as
//! digests; signed payload bytes never enter an output or audit entry at all, while the
//! owning tenant's retention window controls whether an output may carry their digest.
//! Redaction cannot identify a secret that a caller
//! falsely labels as public, so the workspace source scanner separately rejects direct
//! output surfaces that bypass this API.

use std::collections::BTreeMap;
use std::fmt::{Debug, Display, Formatter};

use sha2::{Digest, Sha256};

use crate::store::TenantId;
use crate::tenant::{Config, RedactionPolicy};

const REDACTED: &str = "[REDACTED]";
const MAX_PUBLIC_BYTES: usize = 4_096;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputSurface {
    Audit,
    Log,
    Metric,
    Trace,
    Error,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DataClass {
    PublicText,
    Identifier,
    SignedPayload { written_sequence: u64 },
    CoreReceipt,
    PrivateKey,
    SessionToken,
    SecretConfiguration,
}

#[derive(Clone, Eq, PartialEq)]
pub struct Redacted(String);

impl Redacted {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn stored(value: String) -> Result<Self, RedactionError> {
        validate_public(&value)?;
        Ok(Self(value))
    }

    /// Renders one operator-facing boot diagnostic before any tenant exists.
    ///
    /// Startup errors name missing configuration and refused protocol
    /// evidence; the text is still held to the public-text bounds, and
    /// anything outside them renders as the redaction marker instead.
    #[must_use]
    pub fn boot_diagnostic(value: &str) -> Self {
        if validate_public(value).is_ok() {
            Self(value.to_owned())
        } else {
            Self(REDACTED.to_owned())
        }
    }
}

impl Debug for Redacted {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Display for Redacted {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Eq, PartialEq)]
pub enum PayloadEvidence {
    Digest([u8; 32]),
    Redacted,
}

impl PayloadEvidence {
    pub(crate) const fn digest(&self) -> Option<[u8; 32]> {
        match self {
            Self::Digest(value) => Some(*value),
            Self::Redacted => None,
        }
    }
}

impl Debug for PayloadEvidence {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Digest(value) => formatter
                .debug_tuple("PayloadDigest")
                .field(&hex(value))
                .finish(),
            Self::Redacted => formatter.write_str("RedactedPayload"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderedOutput {
    pub surface: OutputSurface,
    pub value: Redacted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RedactionError {
    WrongTenant,
    MissingTenantConfiguration,
    InvalidPublicText,
}

impl Display for RedactionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::WrongTenant => "redaction configuration belongs to another tenant",
            Self::MissingTenantConfiguration => "tenant redaction configuration is unavailable",
            Self::InvalidPublicText => "public output text is invalid",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for RedactionError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TenantRedaction {
    policy: RedactionPolicy,
    audit_retention_sequences: u64,
}

#[derive(Debug, Default)]
pub struct RedactionRegistry {
    tenants: BTreeMap<TenantId, TenantRedaction>,
}

impl RedactionRegistry {
    pub fn configure(&mut self, config: &Config) {
        self.tenants.insert(
            config.tenant.clone(),
            TenantRedaction {
                policy: config.redaction,
                audit_retention_sequences: config.retention.audit,
            },
        );
    }

    /// Renders one value for a surface under the owning tenant's configured policy.
    ///
    /// # Errors
    ///
    /// Returns `MissingTenantConfiguration` for an unconfigured tenant, and `InvalidPublicText`
    /// when public text is not UTF-8, is empty, exceeds 4096 bytes, or carries a control
    /// character other than tab or newline.
    pub fn render(
        &self,
        tenant: &TenantId,
        surface: OutputSurface,
        class: DataClass,
        value: &[u8],
        current_sequence: u64,
    ) -> Result<RenderedOutput, RedactionError> {
        let config = self
            .tenants
            .get(tenant)
            .ok_or(RedactionError::MissingTenantConfiguration)?;
        render_with(*config, surface, class, value, current_sequence)
    }
}

/// Renders one value for a surface under an explicitly supplied tenant configuration.
///
/// # Errors
///
/// Returns `WrongTenant` when the configuration belongs to another tenant, and
/// `InvalidPublicText` when public text is not UTF-8, is empty, exceeds 4096 bytes, or carries a
/// control character other than tab or newline.
pub fn redact(
    config: &Config,
    tenant: &TenantId,
    surface: OutputSurface,
    class: DataClass,
    value: &[u8],
    current_sequence: u64,
) -> Result<RenderedOutput, RedactionError> {
    if &config.tenant != tenant {
        return Err(RedactionError::WrongTenant);
    }
    render_with(
        TenantRedaction {
            policy: config.redaction,
            audit_retention_sequences: config.retention.audit,
        },
        surface,
        class,
        value,
        current_sequence,
    )
}

#[must_use]
pub fn protect_payload(
    config: &Config,
    written_sequence: u64,
    current_sequence: u64,
    value: &[u8],
) -> PayloadEvidence {
    let retained = current_sequence <= written_sequence.saturating_add(config.retention.audit);
    match (config.redaction, retained) {
        (RedactionPolicy::ReceiptOnly, _) | (_, false) => PayloadEvidence::Redacted,
        (RedactionPolicy::Strict | RedactionPolicy::Standard, true) => {
            PayloadEvidence::Digest(Sha256::digest(value).into())
        }
    }
}

fn render_with(
    config: TenantRedaction,
    surface: OutputSurface,
    class: DataClass,
    value: &[u8],
    current_sequence: u64,
) -> Result<RenderedOutput, RedactionError> {
    let rendered = match class {
        DataClass::PublicText => {
            let text = std::str::from_utf8(value).map_err(|_| RedactionError::InvalidPublicText)?;
            validate_public(text)?;
            Redacted(text.to_owned())
        }
        DataClass::Identifier | DataClass::CoreReceipt => Redacted(digest(value)),
        DataClass::SignedPayload { written_sequence } => {
            let retained = current_sequence
                <= written_sequence.saturating_add(config.audit_retention_sequences);
            if retained && config.policy == RedactionPolicy::Standard {
                Redacted(digest(value))
            } else {
                Redacted(REDACTED.to_owned())
            }
        }
        DataClass::PrivateKey | DataClass::SessionToken | DataClass::SecretConfiguration => {
            Redacted(REDACTED.to_owned())
        }
    };
    Ok(RenderedOutput {
        surface,
        value: rendered,
    })
}

fn validate_public(value: &str) -> Result<(), RedactionError> {
    if value.is_empty()
        || value.len() > MAX_PUBLIC_BYTES
        || value
            .chars()
            .any(|character| character.is_control() && character != '\n' && character != '\t')
    {
        Err(RedactionError::InvalidPublicText)
    } else {
        Ok(())
    }
}

fn digest(value: &[u8]) -> String {
    let hash: [u8; 32] = Sha256::digest(value).into();
    format!("sha256:{}", hex(&hash))
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

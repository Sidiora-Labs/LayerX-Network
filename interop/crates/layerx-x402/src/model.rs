//! Strict x402 v2 wire models. Monetary values remain decimal strings on the
//! wire and exact `u128` integers in memory; no floating-point path exists.

use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};

use layerx_interop_gateway::error::GatewayError;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

pub const X402_VERSION: u8 = 2;
pub const PAYMENT_REQUIRED_HEADER: &str = "PAYMENT-REQUIRED";
pub const PAYMENT_SIGNATURE_HEADER: &str = "PAYMENT-SIGNATURE";
pub const PAYMENT_RESPONSE_HEADER: &str = "PAYMENT-RESPONSE";

const SHORT_TEXT_LIMIT: usize = 512;
const URL_LIMIT: usize = 2_048;
const TAG_LIMIT: usize = 5;
const TAG_TEXT_LIMIT: usize = 32;
const EXTENSION_LIMIT: usize = 32;

/// Exact atomic amount represented by a decimal JSON string as x402 v2
/// requires. Negative, fractional, exponent, padded and overflowed values are
/// unrepresentable.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct AtomicAmount(u128);

impl AtomicAmount {
    /// Parses one canonical unsigned decimal amount.
    ///
    /// # Errors
    ///
    /// Refuses empty, padded, non-decimal and out-of-range values.
    pub fn parse(value: &str) -> Result<Self, X402Error> {
        if value.is_empty()
            || value.len() > 39
            || value.len() > 1 && value.starts_with('0')
            || !value.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(X402Error::InvalidAmount);
        }
        value
            .parse::<u128>()
            .map(Self)
            .map_err(|_| X402Error::InvalidAmount)
    }

    #[must_use]
    pub const fn from_u128(value: u128) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn value(self) -> u128 {
        self.0
    }
}

impl Serialize for AtomicAmount {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for AtomicAmount {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(D::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ResourceInfo {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
}

impl ResourceInfo {
    pub fn validate(&self) -> Result<(), X402Error> {
        validate_url(&self.url)?;
        if self
            .description
            .as_ref()
            .is_some_and(|value| !bounded_text(value, SHORT_TEXT_LIMIT))
            || self
                .mime_type
                .as_ref()
                .is_some_and(|value| !bounded_text(value, TAG_TEXT_LIMIT))
            || self
                .service_name
                .as_ref()
                .is_some_and(|value| !printable_text(value, TAG_TEXT_LIMIT))
            || self.tags.len() > TAG_LIMIT
            || self
                .tags
                .iter()
                .any(|tag| !printable_text(tag, TAG_TEXT_LIMIT))
        {
            return Err(X402Error::Bounds);
        }
        if let Some(icon) = &self.icon_url {
            validate_url(icon)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Extension {
    pub info: Value,
    pub schema: Value,
}

pub type Extensions = BTreeMap<String, Extension>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PaymentRequirements {
    pub scheme: String,
    pub network: String,
    pub amount: AtomicAmount,
    pub asset: String,
    pub pay_to: String,
    pub max_timeout_seconds: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<Value>,
}

impl PaymentRequirements {
    pub fn validate(&self) -> Result<(), X402Error> {
        if !identifier(&self.scheme, 32)
            || !caip2(&self.network)
            || !bounded_text(&self.asset, 256)
            || !bounded_text(&self.pay_to, 256)
            || self.amount.value() == 0
            || self.max_timeout_seconds == 0
        {
            return Err(X402Error::InvalidRequirements);
        }
        Ok(())
    }

    pub fn layerx_facts(&self) -> Result<([u8; 32], [u8; 32]), X402Error> {
        let (namespace, _) = self
            .network
            .split_once(':')
            .ok_or(X402Error::InvalidRequirements)?;
        if namespace != "layerx" {
            return Err(X402Error::UnsupportedOffer);
        }
        Ok((parse_hex32(&self.asset)?, parse_hex32(&self.pay_to)?))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PaymentRequired {
    pub x402_version: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub resource: ResourceInfo,
    pub accepts: Vec<PaymentRequirements>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extensions: Extensions,
}

impl PaymentRequired {
    pub fn validate(&self) -> Result<(), X402Error> {
        validate_version(self.x402_version)?;
        self.resource.validate()?;
        if self.accepts.is_empty()
            || self.accepts.len() > 32
            || self
                .error
                .as_ref()
                .is_some_and(|value| !bounded_text(value, SHORT_TEXT_LIMIT))
        {
            return Err(X402Error::InvalidRequirements);
        }
        for requirements in &self.accepts {
            requirements.validate()?;
        }
        validate_extensions(&self.extensions)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PaymentPayload {
    pub x402_version: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<ResourceInfo>,
    pub payload: Value,
    pub accepted: PaymentRequirements,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extensions: Extensions,
}

impl PaymentPayload {
    pub fn validate(&self) -> Result<(), X402Error> {
        validate_version(self.x402_version)?;
        self.accepted.validate()?;
        if let Some(resource) = &self.resource {
            resource.validate()?;
        }
        if !self.payload.is_object() {
            return Err(X402Error::InvalidPayload);
        }
        validate_extensions(&self.extensions)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SettlementResponse {
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payer: Option<String>,
    pub transaction: String,
    pub network: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<AtomicAmount>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extensions: BTreeMap<String, Value>,
}

impl SettlementResponse {
    pub fn validate_wire(&self) -> Result<(), X402Error> {
        if !caip2(&self.network)
            || self.transaction.len() > 512
            || self
                .payer
                .as_ref()
                .is_some_and(|payer| !bounded_text(payer, 256))
            || self
                .error_reason
                .as_ref()
                .is_some_and(|reason| !bounded_text(reason, SHORT_TEXT_LIMIT))
        {
            return Err(X402Error::Bounds);
        }
        if self.success {
            if self.transaction.is_empty() || self.error_reason.is_some() {
                return Err(X402Error::EvidenceMissing);
            }
        } else {
            let reason = self
                .error_reason
                .as_deref()
                .ok_or(X402Error::InvalidPayload)?;
            if reason == "settlement_pending" {
                if self.transaction.is_empty() {
                    return Err(X402Error::InvalidPayload);
                }
            } else if !self.transaction.is_empty() {
                return Err(X402Error::InvalidPayload);
            }
        }
        Ok(())
    }
}

/// Stable adapter failures. No variant contains external payload bytes or
/// secrets, so the gateway can safely carry it across redaction boundaries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum X402Error {
    Decode,
    Encode,
    WrongVersion,
    Bounds,
    InvalidAmount,
    InvalidRequirements,
    InvalidPayload,
    UnsupportedOffer,
    RequirementsMismatch,
    ExtensionsMismatch,
    PaymentPending,
    PaymentRefused,
    EvidenceMissing,
    EvidenceMismatch,
    Gateway(GatewayError),
}

impl Display for X402Error {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Decode => formatter.write_str("x402 message decoding failed"),
            Self::Encode => formatter.write_str("x402 message encoding failed"),
            Self::WrongVersion => formatter.write_str("x402 version is not v2"),
            Self::Bounds => formatter.write_str("x402 field exceeds its declared bound"),
            Self::InvalidAmount => formatter.write_str("x402 amount is not a canonical integer"),
            Self::InvalidRequirements => formatter.write_str("x402 requirements are invalid"),
            Self::InvalidPayload => formatter.write_str("x402 payment payload is invalid"),
            Self::UnsupportedOffer => formatter.write_str("no supported x402 offer was found"),
            Self::RequirementsMismatch => {
                formatter.write_str("payment payload does not match the issued requirements")
            }
            Self::ExtensionsMismatch => {
                formatter.write_str("payment payload did not preserve required extensions")
            }
            Self::PaymentPending => formatter.write_str("payment remains pending"),
            Self::PaymentRefused => formatter.write_str("payment was refused"),
            Self::EvidenceMissing => formatter.write_str("settlement has no backing evidence"),
            Self::EvidenceMismatch => formatter.write_str("settlement evidence does not match"),
            Self::Gateway(error) => write!(formatter, "gateway translation failed: {error}"),
        }
    }
}

impl std::error::Error for X402Error {}

fn validate_version(version: u8) -> Result<(), X402Error> {
    if version == X402_VERSION {
        Ok(())
    } else {
        Err(X402Error::WrongVersion)
    }
}

fn validate_extensions(extensions: &Extensions) -> Result<(), X402Error> {
    if extensions.len() > EXTENSION_LIMIT
        || extensions
            .keys()
            .any(|name| !identifier(name, TAG_TEXT_LIMIT))
    {
        return Err(X402Error::Bounds);
    }
    Ok(())
}

fn bounded_text(value: &str, limit: usize) -> bool {
    !value.is_empty() && value.len() <= limit && !value.contains('\0')
}

fn printable_text(value: &str, limit: usize) -> bool {
    bounded_text(value, limit) && value.bytes().all(|byte| matches!(byte, 0x20..=0x7e))
}

fn identifier(value: &str, limit: usize) -> bool {
    bounded_text(value, limit)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn caip2(value: &str) -> bool {
    let Some((namespace, reference)) = value.split_once(':') else {
        return false;
    };
    identifier(namespace, 32) && identifier(reference, 64)
}

fn validate_url(value: &str) -> Result<(), X402Error> {
    if value.len() > URL_LIMIT
        || !(value.starts_with("https://") || value.starts_with("http://"))
        || value.contains(['\r', '\n', '\0'])
    {
        return Err(X402Error::Bounds);
    }
    Ok(())
}

fn parse_hex32(value: &str) -> Result<[u8; 32], X402Error> {
    let digits = value.strip_prefix("0x").unwrap_or(value);
    if digits.len() != 64 || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(X402Error::InvalidRequirements);
    }
    let mut decoded = [0u8; 32];
    for (index, pair) in digits.as_bytes().chunks_exact(2).enumerate() {
        let high = hex_nibble(pair[0]).ok_or(X402Error::InvalidRequirements)?;
        let low = hex_nibble(pair[1]).ok_or(X402Error::InvalidRequirements)?;
        decoded[index] = high << 4 | low;
    }
    Ok(decoded)
}

const fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

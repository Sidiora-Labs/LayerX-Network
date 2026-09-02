//! Declared checkpoint settlement configuration shared by every verifier.

use std::fmt;

use crate::json::{self, JsonError, JsonValue};

/// Schema identifier of the declared checkpoint settlement document.
pub const CHECKPOINT_SETTLEMENT_SCHEMA: &str = "layerx/checkpoint-settlement/1";

/// Repository-relative path of the declared checkpoint settlement document.
pub const CHECKPOINT_SETTLEMENT_PATH: &str = "contracts/config/checkpoint-settlement.json";

/// One bonded guarantor declared for a settlement domain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeclaredGuarantor {
    guarantor_id: [u8; 32],
    signer: [u8; 20],
    public_key: [u8; 33],
}

impl DeclaredGuarantor {
    /// Returns the guarantor identifier registered with the bond contract.
    #[must_use]
    pub const fn guarantor_id(&self) -> [u8; 32] {
        self.guarantor_id
    }

    /// Returns the EVM signer address of the guarantor key.
    #[must_use]
    pub const fn signer(&self) -> [u8; 20] {
        self.signer
    }

    /// Returns the compressed secp256k1 public key.
    #[must_use]
    pub const fn public_key(&self) -> [u8; 33] {
        self.public_key
    }
}

/// One named settlement domain: chain, contract and bonded guarantor set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeclaredSettlementDomain {
    name: String,
    paxeer_chain_id: u64,
    network_id: u32,
    settlement_contract: [u8; 20],
    guarantor_set: Vec<DeclaredGuarantor>,
}

impl DeclaredSettlementDomain {
    /// Returns the declared domain name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the Paxeer chain identifier attestations must commit to.
    #[must_use]
    pub const fn paxeer_chain_id(&self) -> u64 {
        self.paxeer_chain_id
    }

    /// Returns the `LayerX` network identifier headers must commit to.
    #[must_use]
    pub const fn network_id(&self) -> u32 {
        self.network_id
    }

    /// Returns the `GuarantorBond` contract address attestations commit to.
    #[must_use]
    pub const fn settlement_contract(&self) -> [u8; 20] {
        self.settlement_contract
    }

    /// Returns the bonded guarantor set in ascending identifier order.
    #[must_use]
    pub fn guarantor_set(&self) -> &[DeclaredGuarantor] {
        &self.guarantor_set
    }
}

/// Declared finality policy applied by every verifier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeclaredFinalityPolicy {
    maximum_attestation_delay_seconds: u64,
    certificate_threshold: usize,
}

impl DeclaredFinalityPolicy {
    /// Returns the maximum header-relative attestation delay in seconds.
    #[must_use]
    pub const fn maximum_attestation_delay_seconds(&self) -> u64 {
        self.maximum_attestation_delay_seconds
    }

    /// Returns the maximum header-relative attestation delay in milliseconds.
    #[must_use]
    pub const fn maximum_attestation_delay_ms(&self) -> u64 {
        self.maximum_attestation_delay_seconds * 1_000
    }

    /// Returns the minimum number of distinct bonded attestations.
    #[must_use]
    pub const fn certificate_threshold(&self) -> usize {
        self.certificate_threshold
    }
}

/// The complete declared checkpoint settlement configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeclaredCheckpointSettlement {
    protocol_version: u16,
    checkpoint_certificate_domain: Vec<u8>,
    guarantor_attestation_domain: Vec<u8>,
    header_encoding_prefix: Vec<u8>,
    header_length: usize,
    finality_policy: DeclaredFinalityPolicy,
    domains: Vec<DeclaredSettlementDomain>,
}

/// Failure to read or validate the declared settlement configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SettlementError {
    /// The document is not well-formed or lacks a required field.
    Json(JsonError),
    /// The document declares another schema.
    Schema(String),
    /// A declared value is outside its permitted range.
    Value { path: String, detail: &'static str },
    /// No settlement domain carries the requested name.
    UnknownDomain(String),
}

impl fmt::Display for SettlementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(formatter, "checkpoint settlement: {error}"),
            Self::Schema(schema) => {
                write!(
                    formatter,
                    "checkpoint settlement schema {schema} unsupported"
                )
            }
            Self::Value { path, detail } => {
                write!(formatter, "checkpoint settlement {path}: {detail}")
            }
            Self::UnknownDomain(name) => {
                write!(formatter, "checkpoint settlement domain {name} undeclared")
            }
        }
    }
}

impl std::error::Error for SettlementError {}

impl From<JsonError> for SettlementError {
    fn from(error: JsonError) -> Self {
        Self::Json(error)
    }
}

fn value_error(path: &str, detail: &'static str) -> SettlementError {
    SettlementError::Value {
        path: path.to_owned(),
        detail,
    }
}

fn domain_tag(document: &JsonValue, path: &str) -> Result<Vec<u8>, SettlementError> {
    let text = document.str_at(path)?;
    if text.is_empty() || !text.is_ascii() || text.bytes().any(|byte| byte == 0) {
        return Err(value_error(path, "domain tag must be non-empty ASCII"));
    }
    let mut tag = text.as_bytes().to_vec();
    tag.push(0);
    Ok(tag)
}

fn guarantor(document: &JsonValue, path: &str) -> Result<DeclaredGuarantor, SettlementError> {
    let guarantor_id = document.hex_array_at::<32>(&format!("{path}.guarantor_id"))?;
    let signer = document.hex_array_at::<20>(&format!("{path}.signer"))?;
    let public_key = document.hex_array_at::<33>(&format!("{path}.public_key"))?;
    if guarantor_id == [0; 32] {
        return Err(value_error(path, "guarantor identifier is zero"));
    }
    if signer == [0; 20] {
        return Err(value_error(path, "guarantor signer is zero"));
    }
    if !matches!(public_key[0], 2 | 3) {
        return Err(value_error(path, "public key is not compressed"));
    }
    Ok(DeclaredGuarantor {
        guarantor_id,
        signer,
        public_key,
    })
}

fn domain(
    document: &JsonValue,
    name: &str,
    threshold: usize,
) -> Result<DeclaredSettlementDomain, SettlementError> {
    let path = format!("settlement_domains.{name}");
    let paxeer_chain_id = document.u64_at(&format!("{path}.paxeer_chain_id"))?;
    let network_id = document.u64_at(&format!("{path}.network_id"))?;
    let settlement_contract =
        document.hex_array_at::<20>(&format!("{path}.settlement_contract"))?;
    let set_path = format!("{path}.guarantor_set");
    let count = document.array_at(&set_path)?.len();
    if paxeer_chain_id == 0 {
        return Err(value_error(&path, "paxeer chain id is zero"));
    }
    let network_id =
        u32::try_from(network_id).map_err(|_| value_error(&path, "network id exceeds u32"))?;
    if network_id == 0 {
        return Err(value_error(&path, "network id is zero"));
    }
    if settlement_contract == [0; 20] {
        return Err(value_error(&path, "settlement contract is zero"));
    }
    if count < threshold {
        return Err(value_error(
            &set_path,
            "guarantor set is smaller than the certificate threshold",
        ));
    }
    let mut guarantor_set = Vec::with_capacity(count);
    for index in 0..count {
        let entry = guarantor(document, &format!("{set_path}.{index}"))?;
        if let Some(previous) = guarantor_set.last() {
            let previous: &DeclaredGuarantor = previous;
            if previous.guarantor_id >= entry.guarantor_id {
                return Err(value_error(
                    &set_path,
                    "guarantor identifiers are not strictly ascending",
                ));
            }
        }
        if guarantor_set
            .iter()
            .any(|existing: &DeclaredGuarantor| existing.signer == entry.signer)
        {
            return Err(value_error(&set_path, "guarantor signers repeat"));
        }
        guarantor_set.push(entry);
    }
    Ok(DeclaredSettlementDomain {
        name: name.to_owned(),
        paxeer_chain_id,
        network_id,
        settlement_contract,
        guarantor_set,
    })
}

impl DeclaredCheckpointSettlement {
    /// Parses and validates the declared settlement document text.
    ///
    /// # Errors
    ///
    /// Returns the first JSON, schema, or range violation. Every declared
    /// domain is validated; none is skipped.
    pub fn parse(text: &str) -> Result<Self, SettlementError> {
        let document = json::parse(text)?;
        let schema = document.str_at("schema")?;
        if schema != CHECKPOINT_SETTLEMENT_SCHEMA {
            return Err(SettlementError::Schema(schema.to_owned()));
        }
        let protocol_version = u16::try_from(document.u64_at("protocol_version")?)
            .map_err(|_| value_error("protocol_version", "exceeds u16"))?;
        if protocol_version == 0 {
            return Err(value_error("protocol_version", "is zero"));
        }
        let checkpoint_certificate_domain = domain_tag(&document, "checkpoint_certificate_domain")?;
        let guarantor_attestation_domain = domain_tag(&document, "guarantor_attestation_domain")?;
        if checkpoint_certificate_domain == guarantor_attestation_domain {
            return Err(value_error(
                "guarantor_attestation_domain",
                "repeats the checkpoint certificate domain",
            ));
        }
        let header_encoding_prefix = document.hex_at("header_encoding_prefix")?;
        if header_encoding_prefix.is_empty() {
            return Err(value_error("header_encoding_prefix", "is empty"));
        }
        let header_length = usize::try_from(document.u64_at("header_length")?)
            .map_err(|_| value_error("header_length", "exceeds usize"))?;
        if header_length < header_encoding_prefix.len() {
            return Err(value_error(
                "header_length",
                "is shorter than the header encoding prefix",
            ));
        }
        let maximum_attestation_delay_seconds =
            document.u64_at("finality_policy.maximum_attestation_delay_seconds")?;
        if maximum_attestation_delay_seconds == 0
            || maximum_attestation_delay_seconds
                .checked_mul(1_000)
                .is_none()
        {
            return Err(value_error(
                "finality_policy.maximum_attestation_delay_seconds",
                "must be positive and representable in milliseconds",
            ));
        }
        let certificate_threshold = usize::try_from(
            document.u64_at("finality_policy.certificate_threshold")?,
        )
        .map_err(|_| value_error("finality_policy.certificate_threshold", "exceeds usize"))?;
        if certificate_threshold == 0 {
            return Err(value_error(
                "finality_policy.certificate_threshold",
                "is zero",
            ));
        }
        let names = document.keys_at("settlement_domains")?;
        if names.is_empty() {
            return Err(value_error("settlement_domains", "declares no domain"));
        }
        let mut domains = Vec::with_capacity(names.len());
        for name in names {
            if name.is_empty() || name.contains('.') {
                return Err(value_error("settlement_domains", "domain name is invalid"));
            }
            domains.push(domain(&document, name, certificate_threshold)?);
        }
        Ok(Self {
            protocol_version,
            checkpoint_certificate_domain,
            guarantor_attestation_domain,
            header_encoding_prefix,
            header_length,
            finality_policy: DeclaredFinalityPolicy {
                maximum_attestation_delay_seconds,
                certificate_threshold,
            },
            domains,
        })
    }

    /// Returns the declared wire protocol version.
    #[must_use]
    pub const fn protocol_version(&self) -> u16 {
        self.protocol_version
    }

    /// Returns the NUL-terminated checkpoint certificate domain tag.
    #[must_use]
    pub fn checkpoint_certificate_domain(&self) -> &[u8] {
        &self.checkpoint_certificate_domain
    }

    /// Returns the NUL-terminated guarantor attestation domain tag.
    #[must_use]
    pub fn guarantor_attestation_domain(&self) -> &[u8] {
        &self.guarantor_attestation_domain
    }

    /// Returns the exact leading bytes of every canonical checkpoint header.
    #[must_use]
    pub fn header_encoding_prefix(&self) -> &[u8] {
        &self.header_encoding_prefix
    }

    /// Returns the exact canonical checkpoint header length.
    #[must_use]
    pub const fn header_length(&self) -> usize {
        self.header_length
    }

    /// Returns the declared finality policy.
    #[must_use]
    pub const fn finality_policy(&self) -> DeclaredFinalityPolicy {
        self.finality_policy
    }

    /// Returns every declared settlement domain in document order.
    #[must_use]
    pub fn domains(&self) -> &[DeclaredSettlementDomain] {
        &self.domains
    }

    /// Returns the settlement domain declared under a name.
    ///
    /// # Errors
    ///
    /// Returns an unknown-domain error when no domain carries the name.
    pub fn domain(&self, name: &str) -> Result<&DeclaredSettlementDomain, SettlementError> {
        self.domains
            .iter()
            .find(|domain| domain.name == name)
            .ok_or_else(|| SettlementError::UnknownDomain(name.to_owned()))
    }
}

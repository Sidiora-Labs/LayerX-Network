#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fmt::{Display, Formatter, Write as _};

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use ed25519_dalek::{Signature, VerifyingKey};
use layerx_interop_gateway::adapter::{AdapterDescriptor, AdapterId, ConformanceSuite, PinnedSpec};
use layerx_interop_gateway::error::GatewayError;
use layerx_interop_gateway::principal::PrincipalId;
use layerx_interop_gateway::trace::TraceId;
use layerx_proof::receipt::{verify as verify_receipt, AuthorizedBatch};
use openssl::hash::MessageDigest;
use openssl::pkey::PKey;
use openssl::rsa::Padding;
use openssl::sign::{RsaPssSaltlen, Verifier};
use sha2::{Digest as _, Sha256};

const ADAPTER_ID: &str = "visa-tap";
const SIGNATURE_LABEL: &str = "sig2";
const COMPONENTS: &str = "(\"@authority\" \"@path\")";
const BROWSER_TAG: &str = "agent-browser-auth";
const PAYER_TAG: &str = "agent-payer-auth";
const MAX_WINDOW_SECONDS: u64 = 8 * 60;
/// Maximum clock disagreement accepted by TAP credential verification.
pub const MAX_CLOCK_SKEW_SECONDS: u64 = 5 * 60;
const VALUE_LIMIT: usize = 512;
const SIGNATURE_LIMIT: usize = 16 * 1024;
const BINDING_DOMAIN: &[u8] = b"LayerX/interop/visa-tap/binding/v1\0";

/// Exact official Visa Trusted Agent Protocol repository revision used by
/// this adapter. Visa publishes no standalone specification document; the
/// pinned repository `README.md` is the published protocol description.
pub const VISA_TAP_SPEC_COMMIT: &str = "16d59bdf3f8a542bc538d0962edbb80ea30a02af";
/// SHA-256 of `README.md` at [`VISA_TAP_SPEC_COMMIT`].
pub const VISA_TAP_SPEC_SHA256: [u8; 32] = [
    0x5f, 0x5f, 0xba, 0xef, 0x32, 0xd5, 0x75, 0xd1, 0xf8, 0x3a, 0x0a, 0x2c, 0x80, 0x51, 0x33, 0x8c,
    0x37, 0xf7, 0x22, 0x4c, 0xd6, 0xaf, 0xd4, 0x64, 0xd6, 0x38, 0xb8, 0xf2, 0x86, 0x3c, 0xad, 0xa5,
];
const VISA_TAP_SPEC_DOCUMENT: &[u8] = include_bytes!("../../../specs/vendor/visa-tap/README.md");

/// Trusted Agent Protocol interaction asserted by the message signature tag.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentIntent {
    Browse,
    Pay,
}

impl AgentIntent {
    fn parse(value: &str) -> Result<Self, TapError> {
        match value {
            BROWSER_TAG => Ok(Self::Browse),
            PAYER_TAG => Ok(Self::Pay),
            _ => Err(TapError::InvalidTag),
        }
    }
}

/// Algorithms explicitly supported by the official Visa TAP sample and key
/// registry contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TapAlgorithm {
    Ed25519,
    RsaPssSha256,
}

impl TapAlgorithm {
    fn parse(value: &str) -> Result<Self, TapError> {
        match value {
            "Ed25519" | "ed25519" => Ok(Self::Ed25519),
            "PS256" | "rsa-pss-sha256" | "RSA-PSS-SHA256" => Ok(Self::RsaPssSha256),
            _ => Err(TapError::UnsupportedAlgorithm),
        }
    }
}

/// Parsed RFC 9421 signature input retaining its exact structured-field value
/// for signature-base reconstruction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignatureInput {
    parameters: String,
    created: u64,
    expires: u64,
    key_id: String,
    algorithm: TapAlgorithm,
    nonce: String,
    intent: AgentIntent,
}

impl SignatureInput {
    /// Parses the single `sig2` dictionary member required by Visa TAP.
    ///
    /// # Errors
    ///
    /// Refuses duplicate, missing, malformed, unknown, or oversized fields.
    pub fn parse(header: &str) -> Result<Self, TapError> {
        if header.is_empty()
            || header.len() > SIGNATURE_LIMIT
            || header.bytes().any(|byte| matches!(byte, b'\r' | b'\n' | 0))
        {
            return Err(TapError::MalformedSignatureInput);
        }
        let (label, parameters) = header
            .split_once('=')
            .ok_or(TapError::MalformedSignatureInput)?;
        if label.trim() != SIGNATURE_LABEL || parameters.contains(',') {
            return Err(TapError::MalformedSignatureInput);
        }
        let parameters = parameters.trim();
        let tail = parameters
            .strip_prefix(COMPONENTS)
            .ok_or(TapError::MissingCoveredComponent)?;
        let mut created = None;
        let mut expires = None;
        let mut key_id = None;
        let mut algorithm = None;
        let mut nonce = None;
        let mut tag = None;
        let mut canonical = COMPONENTS.to_owned();
        for parameter in tail.split(';').filter(|value| !value.is_empty()) {
            let (name, value) = parameter
                .split_once('=')
                .ok_or(TapError::MalformedSignatureInput)?;
            let name = name.trim();
            match name {
                "created" => {
                    let value = parse_number(value)?;
                    set_once(&mut created, value)?;
                    let _ = write!(canonical, ";created={value}");
                }
                "expires" => {
                    let value = parse_number(value)?;
                    set_once(&mut expires, value)?;
                    let _ = write!(canonical, ";expires={value}");
                }
                "keyid" | "keyId" => {
                    let value = parse_quoted(value)?;
                    set_once(&mut key_id, value.to_owned())?;
                    let _ = write!(canonical, ";{name}=\"{value}\"");
                }
                "alg" => {
                    let value = parse_quoted(value)?;
                    set_once(&mut algorithm, TapAlgorithm::parse(value)?)?;
                    let _ = write!(canonical, ";alg=\"{value}\"");
                }
                "nonce" => {
                    let value = parse_quoted(value)?;
                    set_once(&mut nonce, value.to_owned())?;
                    let _ = write!(canonical, ";nonce=\"{value}\"");
                }
                "tag" => {
                    let value = parse_quoted(value)?;
                    set_once(&mut tag, AgentIntent::parse(value)?)?;
                    let _ = write!(canonical, ";tag=\"{value}\"");
                }
                _ => {
                    validate_extension_parameter(name, value)?;
                    let _ = write!(canonical, ";{name}={}", value.trim());
                }
            }
        }
        let key_id = key_id.ok_or(TapError::MissingSignatureParameter)?;
        let nonce = nonce.ok_or(TapError::MissingSignatureParameter)?;
        if !valid_token(&key_id) || !valid_nonce(&nonce) {
            return Err(TapError::MalformedSignatureInput);
        }
        Ok(Self {
            parameters: canonical,
            created: created.ok_or(TapError::MissingSignatureParameter)?,
            expires: expires.ok_or(TapError::MissingSignatureParameter)?,
            key_id,
            algorithm: algorithm.ok_or(TapError::MissingSignatureParameter)?,
            nonce,
            intent: tag.ok_or(TapError::MissingSignatureParameter)?,
        })
    }

    #[must_use]
    pub const fn created(&self) -> u64 {
        self.created
    }

    #[must_use]
    pub const fn expires(&self) -> u64 {
        self.expires
    }

    #[must_use]
    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    #[must_use]
    pub fn nonce(&self) -> &str {
        &self.nonce
    }

    #[must_use]
    pub const fn intent(&self) -> AgentIntent {
        self.intent
    }
}

/// One incoming merchant request with exact TAP header values.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TapRequest {
    authority: String,
    path: String,
    input: SignatureInput,
    signature: Vec<u8>,
}

impl TapRequest {
    /// Parses and binds one TAP signature to the target authority and path.
    ///
    /// # Errors
    ///
    /// Refuses unsafe target components, malformed structured fields, and
    /// invalid base64 signature dictionaries.
    pub fn parse(
        authority: impl Into<String>,
        path: impl Into<String>,
        signature_input: &str,
        signature: &str,
    ) -> Result<Self, TapError> {
        let authority = authority.into();
        let path = path.into();
        if canonical_tap_authority(&authority)? != authority || canonical_tap_path(&path)? != path {
            return Err(TapError::InvalidTarget);
        }
        let input = SignatureInput::parse(signature_input)?;
        let prefix = format!("{SIGNATURE_LABEL}=:");
        let encoded = signature
            .strip_prefix(&prefix)
            .and_then(|value| value.strip_suffix(':'))
            .ok_or(TapError::MalformedSignature)?;
        if encoded.contains(',') || encoded.len() > SIGNATURE_LIMIT {
            return Err(TapError::MalformedSignature);
        }
        let signature = STANDARD
            .decode(encoded)
            .map_err(|_| TapError::MalformedSignature)?;
        if signature.is_empty() {
            return Err(TapError::MalformedSignature);
        }
        Ok(Self {
            authority,
            path,
            input,
            signature,
        })
    }

    fn signature_base(&self) -> Vec<u8> {
        format!(
            "\"@authority\": {}\n\"@path\": {}\n\"@signature-params\": {}",
            self.authority, self.path, self.input.parameters
        )
        .into_bytes()
    }

    /// Returns the signed nonce that a durable service must consume after
    /// cryptographic verification.
    #[must_use]
    pub fn nonce(&self) -> &str {
        self.input.nonce()
    }

    /// Returns the exact canonical authority covered by the signature.
    #[must_use]
    pub fn authority(&self) -> &str {
        &self.authority
    }

    /// Returns the exact canonical path covered by the signature.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }
}

/// Current registry status for one trusted-agent public key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyStatus {
    Active,
    Revoked,
}

/// Algorithm-specific public material retrieved from the trusted registry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgentPublicKey {
    Ed25519([u8; 32]),
    RsaPssSha256Pem(Vec<u8>),
}

impl AgentPublicKey {
    const fn algorithm(&self) -> TapAlgorithm {
        match self {
            Self::Ed25519(_) => TapAlgorithm::Ed25519,
            Self::RsaPssSha256Pem(_) => TapAlgorithm::RsaPssSha256,
        }
    }
}

/// Registry-authenticated trusted agent and key facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegisteredAgentKey {
    pub key_id: String,
    pub agent_id: String,
    pub agent_domain: String,
    pub layerx_agent: Option<[u8; 32]>,
    pub key: AgentPublicKey,
    pub status: KeyStatus,
    pub expires_at: u64,
}

impl RegisteredAgentKey {
    fn validate(&self, requested_key: &str, now: u64) -> Result<(), TapError> {
        if self.key_id != requested_key
            || !valid_token(&self.key_id)
            || !valid_token(&self.agent_id)
            || !valid_https_origin(&self.agent_domain)
        {
            return Err(TapError::RegistryMismatch);
        }
        if self.layerx_agent.is_some_and(|agent| agent == [0; 32]) {
            return Err(TapError::RegistryMismatch);
        }
        match self.status {
            KeyStatus::Revoked => return Err(TapError::Revoked),
            KeyStatus::Active => {}
        }
        if self.expires_at <= now {
            return Err(TapError::ExpiredKey);
        }
        Ok(())
    }
}

/// Real key-discovery boundary. Implementations retrieve and authenticate the
/// Visa or scheme key registry and never synthesize an active key.
pub trait TrustedAgentRegistry {
    /// Resolves one key identifier at the caller-supplied deterministic time.
    ///
    /// # Errors
    ///
    /// Returns unavailable, unknown, revoked, expired, or malformed status.
    fn resolve(&self, key_id: &str, now: u64) -> Result<RegisteredAgentKey, TapError>;
}

/// One-use nonce store. Consumption occurs only after signature verification.
#[derive(Debug, Default)]
pub struct NonceWindow {
    consumed: BTreeMap<(String, String), u64>,
}

impl NonceWindow {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            consumed: BTreeMap::new(),
        }
    }

    fn consume(
        &mut self,
        key_id: &str,
        nonce: &str,
        expires: u64,
        now: u64,
    ) -> Result<(), TapError> {
        self.consumed.retain(|_, expiry| *expiry > now);
        let identity = (key_id.to_owned(), nonce.to_owned());
        if self.consumed.contains_key(&identity) {
            return Err(TapError::Replay);
        }
        self.consumed.insert(identity, expires);
        Ok(())
    }
}

/// Cryptographically verified trusted-agent facts. This type conveys an edge
/// identity assertion only and holds no `LayerX` protocol capability.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedTrustedAgent {
    pub agent_id: String,
    pub agent_domain: String,
    pub key_id: String,
    pub layerx_agent: Option<[u8; 32]>,
    pub intent: AgentIntent,
    pub expires_at: u64,
    pub signature_digest: [u8; 32],
}

/// Non-authoritative commerce meaning handed from TAP middleware to the sole
/// `LayerX` typed-intent authority. A trusted-agent credential authenticates the
/// caller and its declared browse/pay purpose; it does not authorize an
/// economic effect.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrustedCommerceIntent {
    pub principal: PrincipalId,
    pub layerx_agent: [u8; 32],
    pub trusted_agent_id: String,
    pub intent: AgentIntent,
    pub credential_evidence: [u8; 32],
}

/// The only boundary allowed to turn authenticated TAP meaning into a `LayerX`
/// intent. Implementations must invoke the existing canonical intent compiler;
/// the TAP adapter never constructs protocol payload bytes or signing power.
pub trait LayerXIntentAuthority {
    type Intent;

    /// Compiles authenticated commerce meaning through the canonical intent
    /// authority.
    ///
    /// # Errors
    ///
    /// Returns a typed construction or policy refusal.
    fn compile(
        &mut self,
        intent: &TrustedCommerceIntent,
        trace: &TraceId,
    ) -> Result<Self::Intent, TapError>;
}

/// Honest merchant-visible state after a TAP-originated `LayerX` operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MerchantOperationResult {
    Pending,
    Refused,
    ReceiptVerified { receipt_digest: [u8; 32] },
}

impl MerchantOperationResult {
    /// Constructs a success result only after the canonical `LayerX` verifier
    /// accepts the receipt against an authorised batch.
    ///
    /// # Errors
    ///
    /// Returns [`TapError::ReceiptMismatch`] for malformed, unauthorised, or
    /// cryptographically invalid receipt bytes.
    pub fn from_receipt(
        canonical_receipt: &[u8],
        authorised_batch: &AuthorizedBatch,
    ) -> Result<Self, TapError> {
        let verified = verify_receipt(canonical_receipt, authorised_batch)
            .map_err(|_| TapError::ReceiptMismatch)?;
        Ok(Self::ReceiptVerified {
            receipt_digest: verified
                .evidence()
                .receipt_digest()
                .ok_or(TapError::ReceiptMismatch)?,
        })
    }
}

/// Strict TAP message verifier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TapVerifier;

impl TapVerifier {
    /// Verifies freshness against a server-owned time, key status, target
    /// binding, and cryptography without consuming replay state.
    ///
    /// This split lets hosted services perform replay consumption atomically in
    /// their durable state store after signature verification. The configured
    /// skew is bounded and never comes from the credential request.
    ///
    /// # Errors
    ///
    /// Returns a typed merchant-facing refusal for every failed gate.
    pub fn verify_credential(
        request: &TapRequest,
        registry: &impl TrustedAgentRegistry,
        now: u64,
        clock_skew_seconds: u64,
    ) -> Result<VerifiedTrustedAgent, TapError> {
        verify_window(&request.input, now, clock_skew_seconds)?;
        let registered = registry.resolve(request.input.key_id(), now)?;
        registered.validate(request.input.key_id(), now)?;
        if registered.key.algorithm() != request.input.algorithm {
            return Err(TapError::AlgorithmMismatch);
        }
        let base = request.signature_base();
        verify_signature(&registered.key, &base, &request.signature)?;
        Ok(VerifiedTrustedAgent {
            agent_id: registered.agent_id,
            agent_domain: registered.agent_domain,
            key_id: registered.key_id,
            layerx_agent: registered.layerx_agent,
            intent: request.input.intent,
            expires_at: request.input.expires,
            signature_digest: Sha256::digest(&request.signature).into(),
        })
    }

    /// Verifies freshness, key status, target binding, cryptography, and nonce
    /// uniqueness in that order without an ambient clock read.
    ///
    /// # Errors
    ///
    /// Returns a typed merchant-facing refusal for every failed gate.
    pub fn verify(
        request: &TapRequest,
        registry: &impl TrustedAgentRegistry,
        nonces: &mut NonceWindow,
        now: u64,
    ) -> Result<VerifiedTrustedAgent, TapError> {
        let verified = Self::verify_credential(request, registry, now, 0)?;
        nonces.consume(
            request.input.key_id(),
            request.input.nonce(),
            request.input.expires,
            now,
        )?;
        Ok(verified)
    }
}

fn verify_window(
    input: &SignatureInput,
    now: u64,
    clock_skew_seconds: u64,
) -> Result<(), TapError> {
    if clock_skew_seconds > MAX_CLOCK_SKEW_SECONDS {
        return Err(TapError::ClockSkewTooLarge);
    }
    if input.created > now.saturating_add(clock_skew_seconds) {
        return Err(TapError::NotYetValid);
    }
    if input.expires.saturating_add(clock_skew_seconds) <= now {
        return Err(TapError::Expired);
    }
    if input
        .expires
        .checked_sub(input.created)
        .is_none_or(|window| window > MAX_WINDOW_SECONDS)
    {
        return Err(TapError::WindowTooLong);
    }
    Ok(())
}

fn verify_signature(
    key: &AgentPublicKey,
    message: &[u8],
    signature: &[u8],
) -> Result<(), TapError> {
    match key {
        AgentPublicKey::Ed25519(public_key) => {
            let signature: [u8; 64] = signature
                .try_into()
                .map_err(|_| TapError::MalformedSignature)?;
            let key = VerifyingKey::from_bytes(public_key).map_err(|_| TapError::InvalidKey)?;
            key.verify_strict(message, &Signature::from_bytes(&signature))
                .map_err(|_| TapError::InvalidSignature)
        }
        AgentPublicKey::RsaPssSha256Pem(public_key) => {
            let key = PKey::public_key_from_pem(public_key).map_err(|_| TapError::InvalidKey)?;
            if key.bits() < 2048 {
                return Err(TapError::InvalidKey);
            }
            let mut verifier =
                Verifier::new(MessageDigest::sha256(), &key).map_err(|_| TapError::InvalidKey)?;
            verifier
                .set_rsa_padding(Padding::PKCS1_PSS)
                .and_then(|()| verifier.set_rsa_pss_saltlen(RsaPssSaltlen::DIGEST_LENGTH))
                .and_then(|()| verifier.update(message))
                .and_then(|()| verifier.verify(signature))
                .map_err(|_| TapError::InvalidSignature)
                .and_then(|valid| valid.then_some(()).ok_or(TapError::InvalidSignature))
        }
    }
}

/// Merchant-visible association between a verified TAP identity and a `LayerX`
/// agent. It is explicitly non-authoritative and carries no signing grant,
/// capability, budget, or session material.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CredentialBinding {
    pub layerx_agent: [u8; 32],
    pub trusted_agent_id: String,
    pub trusted_agent_domain: String,
    pub key_id: String,
    pub evidence_digest: [u8; 32],
}

/// Merchant middleware storage for non-authoritative credential status.
pub trait CredentialBindingStore {
    /// Stores a verified association under the authenticated merchant scope.
    ///
    /// # Errors
    ///
    /// Returns a typed storage refusal and grants no protocol authority.
    fn put(
        &mut self,
        principal: &PrincipalId,
        binding: &CredentialBinding,
        trace: &TraceId,
    ) -> Result<(), TapError>;
}

/// Associates an already verified TAP identity with a `LayerX` agent for seller
/// and merchant display only.
///
/// # Errors
///
/// Refuses a zero agent identifier and propagates store failures.
pub fn bind_verified_agent(
    principal: &PrincipalId,
    layerx_agent: [u8; 32],
    verified: &VerifiedTrustedAgent,
    store: &mut impl CredentialBindingStore,
    trace: &TraceId,
) -> Result<CredentialBinding, TapError> {
    if layerx_agent == [0; 32] {
        return Err(TapError::InvalidLayerxAgent);
    }
    if verified.layerx_agent != Some(layerx_agent) {
        return Err(TapError::LayerxAgentMismatch);
    }
    let mut hash = Sha256::new();
    hash.update(BINDING_DOMAIN);
    hash.update(layerx_agent);
    hash.update(verified.agent_id.as_bytes());
    hash.update([0]);
    hash.update(verified.agent_domain.as_bytes());
    hash.update([0]);
    hash.update(verified.key_id.as_bytes());
    hash.update(verified.signature_digest);
    let binding = CredentialBinding {
        layerx_agent,
        trusted_agent_id: verified.agent_id.clone(),
        trusted_agent_domain: verified.agent_domain.clone(),
        key_id: verified.key_id.clone(),
        evidence_digest: hash.finalize().into(),
    };
    store.put(principal, &binding, trace)?;
    Ok(binding)
}

/// Builds the typed, non-authoritative intent handoff after persisting the
/// merchant-visible credential binding.
///
/// # Errors
///
/// Returns a typed binding or storage refusal.
pub fn prepare_trusted_intent(
    principal: &PrincipalId,
    layerx_agent: [u8; 32],
    verified: &VerifiedTrustedAgent,
    store: &mut impl CredentialBindingStore,
    trace: &TraceId,
) -> Result<TrustedCommerceIntent, TapError> {
    let binding = bind_verified_agent(principal, layerx_agent, verified, store, trace)?;
    Ok(TrustedCommerceIntent {
        principal: principal.clone(),
        layerx_agent,
        trusted_agent_id: verified.agent_id.clone(),
        intent: verified.intent,
        credential_evidence: binding.evidence_digest,
    })
}

/// Merchant-facing closed status for a TAP credential attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MerchantCredentialStatus {
    Verified,
    Expired,
    Revoked,
    Replayed,
    Invalid,
    RegistryUnavailable,
}

impl TapError {
    #[must_use]
    pub const fn merchant_status(self) -> MerchantCredentialStatus {
        match self {
            Self::Expired | Self::ExpiredKey => MerchantCredentialStatus::Expired,
            Self::Revoked => MerchantCredentialStatus::Revoked,
            Self::Replay => MerchantCredentialStatus::Replayed,
            Self::RegistryUnavailable | Self::UnknownKey => {
                MerchantCredentialStatus::RegistryUnavailable
            }
            _ => MerchantCredentialStatus::Invalid,
        }
    }
}

fn set_once<T>(slot: &mut Option<T>, value: T) -> Result<(), TapError> {
    if slot.replace(value).is_some() {
        Err(TapError::DuplicateSignatureParameter)
    } else {
        Ok(())
    }
}

fn parse_number(value: &str) -> Result<u64, TapError> {
    value
        .trim()
        .parse()
        .map_err(|_| TapError::MalformedSignatureInput)
}

fn parse_quoted(value: &str) -> Result<&str, TapError> {
    let value = value.trim();
    let unquoted = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .ok_or(TapError::MalformedSignatureInput)?;
    if unquoted.is_empty()
        || unquoted.len() > VALUE_LIMIT
        || unquoted
            .bytes()
            .any(|byte| matches!(byte, b'"' | b'\\' | b'\r' | b'\n' | 0))
    {
        return Err(TapError::MalformedSignatureInput);
    }
    Ok(unquoted)
}

fn validate_extension_parameter(name: &str, value: &str) -> Result<(), TapError> {
    if !valid_token(name) || value.trim().is_empty() || value.len() > VALUE_LIMIT {
        return Err(TapError::MalformedSignatureInput);
    }
    Ok(())
}

fn valid_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= VALUE_LIMIT
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~'))
}

fn valid_nonce(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= VALUE_LIMIT
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'=' | b'-' | b'_')
        })
}

/// Canonicalizes a TAP HTTP authority to a lowercase DNS-style host and canonical
/// optional decimal port.
///
/// # Errors
///
/// Refuses schemes, user information, empty host labels, unsafe characters,
/// and invalid ports.
pub fn canonical_tap_authority(value: &str) -> Result<String, TapError> {
    if value.is_empty()
        || value.len() > VALUE_LIMIT
        || value.contains(['/', '@', '?', '#', '\\'])
        || value.bytes().any(|byte| !byte.is_ascii_graphic())
    {
        return Err(TapError::InvalidTarget);
    }
    let (host, port) = value.rsplit_once(':').map_or_else(
        || Ok::<_, TapError>((value, None)),
        |(host, port)| {
            if host.contains(':') || port.is_empty() {
                return Err(TapError::InvalidTarget);
            }
            let port = port
                .parse::<u16>()
                .map_err(|_| TapError::InvalidTarget)?;
            if port == 0 {
                return Err(TapError::InvalidTarget);
            }
            Ok((host, Some(port)))
        },
    )?;
    if host.is_empty() || host.ends_with('.') {
        return Err(TapError::InvalidTarget);
    }
    let canonical_host = host.to_ascii_lowercase();
    if canonical_host.split('.').any(|label| {
        label.is_empty()
            || label.len() > 63
            || !label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            || !label
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphanumeric)
            || !label
                .as_bytes()
                .last()
                .is_some_and(u8::is_ascii_alphanumeric)
    }) {
        return Err(TapError::InvalidTarget);
    }
    Ok(port.map_or(canonical_host.clone(), |port| {
        format!("{canonical_host}:{port}")
    }))
}

/// Validates one canonical, query-free TAP target path.
///
/// # Errors
///
/// Refuses empty or dot segments, repeated or trailing separators, percent
/// aliases, query/fragment material, and non-ASCII path bytes.
pub fn canonical_tap_path(value: &str) -> Result<String, TapError> {
    if value == "/" {
        return Ok(value.to_owned());
    }
    if !value.starts_with('/')
        || value.len() > VALUE_LIMIT
        || value.ends_with('/')
        || value.split('/').skip(1).any(|segment| {
            segment.is_empty()
                || matches!(segment, "." | "..")
                || !segment.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~')
                })
        })
    {
        return Err(TapError::InvalidTarget);
    }
    Ok(value.to_owned())
}

fn valid_https_origin(value: &str) -> bool {
    value.starts_with("https://")
        && value.len() <= VALUE_LIMIT
        && !value[8..].contains('/')
        && !value.contains('@')
        && !value.contains('?')
        && !value.contains('#')
        && value[8..].contains('.')
        && value.bytes().all(|byte| byte.is_ascii_graphic())
}

fn adapter_id() -> Result<AdapterId, TapError> {
    AdapterId::new(ADAPTER_ID).map_err(|error| TapError::Gateway(error.into()))
}

/// Declares the TAP adapter against a content-pinned Visa specification and
/// its real conformance suite.
///
/// # Errors
///
/// Returns an adapter declaration refusal if the stable identifier is invalid.
pub fn visa_tap_adapter_descriptor(
    spec: PinnedSpec,
    conformance: ConformanceSuite,
) -> Result<AdapterDescriptor, TapError> {
    spec.verify_document(VISA_TAP_SPEC_DOCUMENT)
        .map_err(|error| TapError::Gateway(error.into()))?;
    Ok(AdapterDescriptor::new(adapter_id()?, spec, conformance))
}

/// Stable merchant-facing TAP refusals.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TapError {
    MalformedSignatureInput,
    MissingCoveredComponent,
    MissingSignatureParameter,
    DuplicateSignatureParameter,
    UnknownSignatureParameter,
    InvalidTag,
    UnsupportedAlgorithm,
    InvalidTarget,
    MalformedSignature,
    NotYetValid,
    Expired,
    WindowTooLong,
    ClockSkewTooLarge,
    RegistryUnavailable,
    UnknownKey,
    RegistryMismatch,
    Revoked,
    ExpiredKey,
    AlgorithmMismatch,
    InvalidKey,
    InvalidSignature,
    Replay,
    InvalidLayerxAgent,
    LayerxAgentMismatch,
    StorageRefused,
    IntentRefused,
    ReceiptMismatch,
    Gateway(GatewayError),
}

impl Display for TapError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::MalformedSignatureInput => "TAP signature input is malformed",
            Self::MissingCoveredComponent => "TAP signature omits a required request component",
            Self::MissingSignatureParameter => "TAP signature omits a required parameter",
            Self::DuplicateSignatureParameter => "TAP signature repeats a parameter",
            Self::UnknownSignatureParameter => "TAP signature contains an unknown parameter",
            Self::InvalidTag => "TAP interaction tag is invalid",
            Self::UnsupportedAlgorithm => "TAP signature algorithm is unsupported",
            Self::InvalidTarget => "TAP request target is invalid",
            Self::MalformedSignature => "TAP signature is malformed",
            Self::NotYetValid => "TAP signature is not yet valid",
            Self::Expired => "TAP signature expired",
            Self::WindowTooLong => "TAP signature validity exceeds eight minutes",
            Self::ClockSkewTooLarge => "TAP clock skew exceeds its configured bound",
            Self::RegistryUnavailable => "trusted-agent registry is unavailable",
            Self::UnknownKey => "trusted-agent key is unknown",
            Self::RegistryMismatch => "trusted-agent registry facts do not match",
            Self::Revoked => "trusted-agent key is revoked",
            Self::ExpiredKey => "trusted-agent key expired",
            Self::AlgorithmMismatch => "trusted-agent key algorithm does not match",
            Self::InvalidKey => "trusted-agent public key is invalid",
            Self::InvalidSignature => "TAP signature verification failed",
            Self::Replay => "TAP nonce was already consumed",
            Self::InvalidLayerxAgent => "LayerX agent identifier is invalid",
            Self::LayerxAgentMismatch => "trusted credential is not bound to this LayerX agent",
            Self::StorageRefused => "credential binding storage refused",
            Self::IntentRefused => "LayerX typed-intent authority refused the request",
            Self::ReceiptMismatch => "LayerX receipt verification failed",
            Self::Gateway(_) => "TAP adapter declaration failed",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for TapError {}

/// Codify anchor for Visa Trusted Agent Protocol verification.
#[must_use]
pub const fn interop_visa_tap() -> &'static str {
    "rfc9421-visa-trusted-agent-verification"
}

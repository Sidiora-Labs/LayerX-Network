use layerx_ap2::{ap2_adapter_descriptor, AP2_SPEC_SHA256};
use layerx_fiat::fiat_adapter_descriptor;
use layerx_interop_gateway::adapter::{
    AdapterDescriptor, AdapterId, ConformanceSuite, PinnedSpec, SpecVersion,
};
use layerx_interop_gateway::server::EvidencePolicy;
use layerx_interop_gateway::trace::TraceId;
use layerx_interop_gateway::GatewayCore;
use layerx_platform_gateway::http::{Client, Endpoint};
use layerx_platform_gateway::store::{RedisEndpoint, RedisStore};
use layerx_ucp::ucp_adapter_descriptor;
use layerx_visa_tap::visa_tap_adapter_descriptor;
use layerx_x402::facilitator::SupportedResponse;
use layerx_x402::{x402_adapter_descriptor, X402_SPEC_SHA256};
use native_tls::{Certificate, Identity};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::ServerConfig;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::net::SocketAddr;
use std::sync::Arc;
use zeroize::{Zeroize, Zeroizing};

const MAX_IDEMPOTENCY_SECONDS: u64 = 2_592_000;
const REQUIRED_ADAPTERS: [&str; 5] = ["x402", "ap2", "ucp", "visa-tap", "fiat"];
const REQUIRED_TRANSPORTS: [&str; 3] = ["http", "mcp", "a2a"];

pub struct Config {
    pub listen: SocketAddr,
    pub tls: Arc<ServerConfig>,
    pub client: Client,
    pub hosted_gateway: Endpoint,
    pub receipt_authority: Endpoint,
    pub receipt_authority_token: Zeroizing<String>,
    pub store: RedisStore,
    pub trusted_sequencer_key: [u8; 32],
    pub network_id: String,
    pub wire_version: String,
    pub idempotency_seconds: u64,
    pub manifest: RuntimeManifest,
}

#[derive(Clone)]
pub struct RuntimeManifest {
    pub adapters: BTreeMap<String, RegisteredAdapter>,
    pub transports: BTreeMap<String, TransportPin>,
    pub x402_supported: SupportedResponse,
    pub ap2_keys: Vec<Ap2KeyPin>,
    pub ap2_assets: Vec<Ap2AssetBinding>,
    pub visa_agents: Vec<VisaAgentPin>,
    pub fiat_providers: Vec<FiatProviderPin>,
}

#[derive(Clone)]
pub struct RegisteredAdapter {
    pub descriptor: AdapterDescriptor,
    pub evidence: EvidencePolicy,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransportPin {
    pub id: String,
    pub version: String,
    pub specification_sha256: String,
    pub conformance_sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestFile {
    adapters: Vec<AdapterPin>,
    transports: Vec<TransportPin>,
    x402_supported: SupportedResponse,
    ap2_keys: Vec<Ap2KeyPin>,
    ap2_assets: Vec<Ap2AssetBinding>,
    visa_agents: Vec<VisaAgentPin>,
    fiat_providers: Vec<FiatProviderPin>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ap2KeyPin {
    pub use_case: String,
    pub key_id: String,
    pub public_key_sec1: String,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ap2AssetBinding {
    pub principal_digest: String,
    pub currency: String,
    pub minor_unit_exponent: u8,
    pub atomic_units_per_minor_unit: String,
    pub asset: String,
    pub payer_account: String,
    pub payee_account: String,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VisaAgentPin {
    pub key_id: String,
    pub agent_id: String,
    pub agent_domain: String,
    pub layerx_agent: String,
    pub algorithm: String,
    pub public_key: String,
    pub expires_at: u64,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FiatProviderPin {
    pub provider: String,
    pub public_key_ed25519: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdapterPin {
    id: String,
    specification: String,
    version: String,
    specification_sha256: String,
    conformance_suite: String,
    conformance_vectors: u64,
    conformance_sha256: String,
    evidence_policy: String,
}

pub fn load() -> Result<Config, String> {
    let manifest_path = env::var("LAYERX_INTEROP_CONFIG")
        .map_err(|_| "LAYERX_INTEROP_CONFIG is required".to_owned())?;
    let manifest_bytes = fs::read(manifest_path).map_err(|error| error.to_string())?;
    if manifest_bytes.is_empty() || manifest_bytes.len() > 1024 * 1024 {
        return Err("interop configuration exceeds its bound".to_owned());
    }
    let manifest_file: ManifestFile =
        serde_json::from_slice(&manifest_bytes).map_err(|error| error.to_string())?;
    let manifest = runtime_manifest(manifest_file)?;

    let outbound_ca =
        Certificate::from_der(&read_file("LAYERX_INTEROP_OUTBOUND_CA_DER", 64 * 1024)?)
            .map_err(|error| error.to_string())?;
    let identity_password = read_secret("LAYERX_INTEROP_CLIENT_IDENTITY_PASSWORD_FILE")?;
    let identity = Identity::from_pkcs12(
        &read_file("LAYERX_INTEROP_CLIENT_IDENTITY_PKCS12", 128 * 1024)?,
        identity_password.as_str(),
    )
    .map_err(|error| error.to_string())?;
    let redis_username = read_secret("LAYERX_INTEROP_REDIS_USERNAME_FILE")?;
    let redis_password = read_secret("LAYERX_INTEROP_REDIS_PASSWORD_FILE")?;
    let redis = RedisEndpoint::parse(
        &env::var("LAYERX_INTEROP_REDIS_URL")
            .map_err(|_| "LAYERX_INTEROP_REDIS_URL is required".to_owned())?,
    )?;
    let trusted_key = read_secret("LAYERX_INTEROP_SEQUENCER_PUBLIC_KEY_FILE")?;
    let trusted_sequencer_key = parse_hex32(trusted_key.as_str())?;
    let idempotency_seconds = env::var("LAYERX_INTEROP_IDEMPOTENCY_SECONDS")
        .unwrap_or_else(|_| "604800".to_owned())
        .parse::<u64>()
        .map_err(|_| "interop idempotency retention is invalid".to_owned())?;
    if idempotency_seconds == 0 || idempotency_seconds > MAX_IDEMPOTENCY_SECONDS {
        return Err("interop idempotency retention exceeds its bound".to_owned());
    }
    let network_id = required_label("LAYERX_INTEROP_NETWORK_ID", 128)?;
    let wire_version = required_label("LAYERX_INTEROP_WIRE_VERSION", 64)?;
    Ok(Config {
        listen: env::var("LAYERX_INTEROP_LISTEN")
            .map_err(|_| "LAYERX_INTEROP_LISTEN is required".to_owned())?
            .parse::<SocketAddr>()
            .map_err(|_| "interop listen address is invalid".to_owned())?,
        tls: tls_config()?,
        client: Client::new(outbound_ca.clone(), identity),
        hosted_gateway: Endpoint::parse(
            &env::var("LAYERX_INTEROP_HOSTED_GATEWAY_URL")
                .map_err(|_| "LAYERX_INTEROP_HOSTED_GATEWAY_URL is required".to_owned())?,
        )?,
        receipt_authority: Endpoint::parse(
            &env::var("LAYERX_INTEROP_RECEIPT_AUTHORITY_URL")
                .map_err(|_| "LAYERX_INTEROP_RECEIPT_AUTHORITY_URL is required".to_owned())?,
        )?,
        receipt_authority_token: read_secret("LAYERX_INTEROP_RECEIPT_AUTHORITY_TOKEN_FILE")?,
        store: RedisStore::new(redis, outbound_ca, redis_username, redis_password),
        trusted_sequencer_key,
        network_id,
        wire_version,
        idempotency_seconds,
        manifest,
    })
}

fn runtime_manifest(file: ManifestFile) -> Result<RuntimeManifest, String> {
    let mut adapters = BTreeMap::new();
    let mut gateway = GatewayCore::new();
    let trace = TraceId::mint([0x22; 16]);
    for pin in file.adapters {
        if adapters.contains_key(&pin.id) {
            return Err(format!("duplicate adapter declaration: {}", pin.id));
        }
        let evidence = EvidencePolicy::parse(&pin.evidence_policy)
            .ok_or_else(|| format!("adapter {} has no declared evidence policy", pin.id))?;
        require_evidence(&pin.id, evidence)?;
        let descriptor = descriptor(&pin)?;
        gateway
            .register_adapter(descriptor.clone(), &trace, 1)
            .map_err(|error| error.error().to_string())?;
        adapters.insert(
            pin.id.clone(),
            RegisteredAdapter {
                descriptor,
                evidence,
            },
        );
    }
    let actual: BTreeSet<_> = adapters.keys().map(String::as_str).collect();
    let required: BTreeSet<_> = REQUIRED_ADAPTERS.into_iter().collect();
    if actual != required {
        return Err(
            "interop configuration must declare exactly x402, AP2, UCP, Visa TAP and fiat adapters"
                .to_owned(),
        );
    }
    let mut transports = BTreeMap::new();
    for pin in file.transports {
        validate_transport(&pin)?;
        if transports.insert(pin.id.clone(), pin).is_some() {
            return Err("duplicate transport pin".to_owned());
        }
    }
    let actual: BTreeSet<_> = transports.keys().map(String::as_str).collect();
    let required: BTreeSet<_> = REQUIRED_TRANSPORTS.into_iter().collect();
    if actual != required {
        return Err(
            "interop configuration must pin exactly HTTP, MCP and A2A transports".to_owned(),
        );
    }
    if file.ap2_keys.is_empty()
        || file.ap2_assets.is_empty()
        || file.visa_agents.is_empty()
        || file.fiat_providers.is_empty()
    {
        return Err(
            "interop trust roots for AP2, Visa TAP and fiat providers are required".to_owned(),
        );
    }
    let mut ap2_key_identities = BTreeSet::new();
    for key in &file.ap2_keys {
        if !matches!(
            key.use_case.as_str(),
            "checkout-mandate" | "payment-mandate" | "merchant-checkout"
        ) || key.key_id.is_empty()
            || key.key_id.len() > 512
            || decode_hex(&key.public_key_sec1, 65).is_err()
            || !ap2_key_identities.insert((key.use_case.as_str(), key.key_id.as_str()))
        {
            return Err("AP2 trust-root declaration is invalid".to_owned());
        }
    }
    let mut ap2_asset_identities = BTreeSet::new();
    for binding in &file.ap2_assets {
        let atomic_units = binding
            .atomic_units_per_minor_unit
            .parse::<u128>()
            .map_err(|_| "AP2 asset binding declaration is invalid".to_owned())?;
        if parse_hex32(&binding.principal_digest).is_err()
            || binding.currency.len() != 3
            || !binding
                .currency
                .bytes()
                .all(|byte| byte.is_ascii_uppercase())
            || binding.minor_unit_exponent > 18
            || atomic_units == 0
            || parse_hex32(&binding.asset).is_err()
            || parse_hex32(&binding.payer_account).is_err()
            || parse_hex32(&binding.payee_account).is_err()
            || !ap2_asset_identities
                .insert((binding.principal_digest.as_str(), binding.currency.as_str()))
        {
            return Err("AP2 asset binding declaration is invalid".to_owned());
        }
    }
    let mut visa_key_ids = BTreeSet::new();
    for key in &file.visa_agents {
        if key.key_id.is_empty()
            || key.agent_id.is_empty()
            || !key.agent_domain.starts_with("https://")
            || parse_hex32(&key.layerx_agent).is_err()
            || !matches!(key.algorithm.as_str(), "ed25519" | "rsa-pss-sha256")
            || key.public_key.is_empty()
            || key.expires_at == 0
            || !visa_key_ids.insert(key.key_id.as_str())
        {
            return Err("Visa TAP trust-root declaration is invalid".to_owned());
        }
    }
    let mut fiat_provider_ids = BTreeSet::new();
    for key in &file.fiat_providers {
        if key.provider.is_empty()
            || parse_hex32(&key.public_key_ed25519).is_err()
            || !fiat_provider_ids.insert(key.provider.as_str())
        {
            return Err("fiat provider trust-root declaration is invalid".to_owned());
        }
    }
    let manifest = RuntimeManifest {
        adapters,
        transports,
        x402_supported: file.x402_supported,
        ap2_keys: file.ap2_keys,
        ap2_assets: file.ap2_assets,
        visa_agents: file.visa_agents,
        fiat_providers: file.fiat_providers,
    };
    let _ = gateway;
    Ok(manifest)
}

fn descriptor(pin: &AdapterPin) -> Result<AdapterDescriptor, String> {
    let suite = ConformanceSuite::new(
        AdapterId::new(pin.conformance_suite.clone()).map_err(|error| error.to_string())?,
        pin.conformance_vectors,
        parse_hex32(&pin.conformance_sha256)?,
    )
    .map_err(|error| error.to_string())?;
    match pin.id.as_str() {
        "x402" => {
            if pin.version != "2.0.0" || parse_hex32(&pin.specification_sha256)? != X402_SPEC_SHA256
            {
                return Err(
                    "x402 configuration does not match the compiled v2 specification pin"
                        .to_owned(),
                );
            }
            x402_adapter_descriptor(suite).map_err(|error| error.to_string())
        }
        "ap2" => {
            if pin.version != "1.0.0" || parse_hex32(&pin.specification_sha256)? != AP2_SPEC_SHA256
            {
                return Err(
                    "AP2 configuration does not match the compiled v1 specification pin".to_owned(),
                );
            }
            ap2_adapter_descriptor(suite).map_err(|error| error.to_string())
        }
        "ucp" | "visa-tap" | "fiat" => {
            let spec = PinnedSpec::new(
                AdapterId::new(pin.specification.clone()).map_err(|error| error.to_string())?,
                SpecVersion::parse(&pin.version).map_err(|error| error.to_string())?,
                parse_hex32(&pin.specification_sha256)?,
            )
            .map_err(|error| error.to_string())?;
            match pin.id.as_str() {
                "ucp" => ucp_adapter_descriptor(spec, suite).map_err(|error| error.to_string()),
                "visa-tap" => {
                    visa_tap_adapter_descriptor(spec, suite).map_err(|error| error.to_string())
                }
                "fiat" => fiat_adapter_descriptor(spec, suite).map_err(|error| error.to_string()),
                _ => Err("unreachable adapter declaration".to_owned()),
            }
        }
        _ => Err(format!("unknown adapter declaration: {}", pin.id)),
    }
}

fn require_evidence(adapter: &str, evidence: EvidencePolicy) -> Result<(), String> {
    let valid = matches!(
        (adapter, evidence),
        ("x402" | "ucp", EvidencePolicy::LayerXReceipt)
            | ("ap2", EvidencePolicy::VerifiedMandateAndLayerXReceipt)
            | ("visa-tap", EvidencePolicy::TrustedAgentCredential)
            | ("fiat", EvidencePolicy::ExternalSettlementAndLayerXReceipt)
    );
    if valid {
        Ok(())
    } else {
        Err(format!(
            "adapter {adapter} declares an incompatible evidence policy"
        ))
    }
}

fn validate_transport(pin: &TransportPin) -> Result<(), String> {
    if !REQUIRED_TRANSPORTS.contains(&pin.id.as_str())
        || !valid_transport_version(&pin.version)
        || parse_hex32(&pin.specification_sha256)? == [0; 32]
        || parse_hex32(&pin.conformance_sha256)? == [0; 32]
    {
        return Err(format!(
            "transport {} is not version and content pinned",
            pin.id
        ));
    }
    Ok(())
}

fn valid_transport_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

fn tls_config() -> Result<Arc<ServerConfig>, String> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| "failed to install TLS crypto provider".to_owned())?;
    let cert = CertificateDer::from(read_file("LAYERX_INTEROP_TLS_CERT_DER", 64 * 1024)?);
    let key = PrivateKeyDer::from(PrivatePkcs8KeyDer::from(read_file(
        "LAYERX_INTEROP_TLS_KEY_DER",
        64 * 1024,
    )?));
    ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .map(Arc::new)
        .map_err(|error| error.to_string())
}

fn read_file(name: &str, maximum: usize) -> Result<Vec<u8>, String> {
    let path = env::var(name).map_err(|_| format!("{name} is required"))?;
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    if bytes.is_empty() || bytes.len() > maximum {
        return Err(format!("{name} exceeds its bound"));
    }
    Ok(bytes)
}

fn read_secret(name: &str) -> Result<Zeroizing<String>, String> {
    let path = env::var(name).map_err(|_| format!("{name} is required"))?;
    let mut secret = fs::read_to_string(path).map_err(|error| error.to_string())?;
    while matches!(secret.as_bytes().last(), Some(b'\r' | b'\n')) {
        secret.pop();
    }
    if secret.is_empty() || secret.len() > 4096 {
        secret.zeroize();
        return Err(format!("{name} does not contain a bounded secret"));
    }
    Ok(Zeroizing::new(secret))
}

fn required_label(name: &str, maximum: usize) -> Result<String, String> {
    let value = env::var(name).map_err(|_| format!("{name} is required"))?;
    if value.is_empty()
        || value.len() > maximum
        || value.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(format!("{name} is invalid"));
    }
    Ok(value)
}

pub fn parse_hex32(value: &str) -> Result<[u8; 32], String> {
    if value.len() != 64 {
        return Err("expected a 32-byte hexadecimal value".to_owned());
    }
    let mut bytes = [0_u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let text = std::str::from_utf8(pair).map_err(|_| "hexadecimal value is invalid")?;
        bytes[index] =
            u8::from_str_radix(text, 16).map_err(|_| "hexadecimal value is invalid".to_owned())?;
    }
    Ok(bytes)
}

pub fn decode_hex(value: &str, maximum: usize) -> Result<Vec<u8>, String> {
    if value.len() % 2 != 0 || value.len() / 2 > maximum {
        return Err("hexadecimal payload exceeds its bound".to_owned());
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).map_err(|_| "hexadecimal payload is invalid")?;
            u8::from_str_radix(text, 16).map_err(|_| "hexadecimal payload is invalid".to_owned())
        })
        .collect()
}

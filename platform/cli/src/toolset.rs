use std::collections::BTreeSet;

use ed25519_dalek::{Signer as _, SigningKey};
use layerx_mcp::server::{DeploymentMode, ToolDefinition, ToolKind};
use layerx_types::activity::{Authority, EnvelopeBuilder, Signature, TimestampBound};
use layerx_types::amount::Amount;
use layerx_types::ids::{Did, IdempotencyKey};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry, Payload};
use layerx_wire::activity::{encode_signed_envelope, encode_unsigned_envelope};
use layerx_wire::encode::Encoder;
use layerx_wire::hash::Domain;
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};

use crate::config::Configuration;
use crate::credential;
use crate::encoding::{fixed_hex, hex_encode};
use crate::http::{validate_resource_id, Client};

const MAX_ARGUMENT_BYTES: usize = 256;
const MAX_PAYMENT_WINDOW_MS: u64 = 300_000;
const SEND_ACTIVITY: u16 = 5;
const SERVED: [ToolDefinition; 2] = [
    ToolDefinition {
        name: "receipt.get",
        kind: ToolKind::Read,
        required_scope: "receipt:read",
        mutation: "none",
        evidence: "canonical receipt verified by the hosted gateway authority boundary",
    },
    ToolDefinition {
        name: "activity.submit",
        kind: ToolKind::Write,
        required_scope: "activity:write",
        mutation: "canonical Asset SEND through the hosted gateway",
        evidence: "verified receipt or an honest pending, unknown, or refused state",
    },
];

pub struct Runtime {
    client: Client,
    network_id: u32,
    key: String,
    source: Option<[u8; 32]>,
    asset: Option<[u8; 32]>,
}

impl Runtime {
    pub fn new(
        configuration: &Configuration,
        gateway_credential: &str,
        key: &str,
        source: Option<&str>,
        asset: Option<&str>,
        mode: DeploymentMode,
    ) -> Result<Self, String> {
        let (_, environment) = configuration.active_environment()?;
        let credential = credential::gateway(gateway_credential)?.ok_or_else(|| {
            format!(
                "gateway credential alias {gateway_credential} is absent; rerun layerx install for this runtime"
            )
        })?;
        if let Ok(expected) = std::env::var("LAYERX_GATEWAY_KEY_ID") {
            let actual = credential
                .split_once(':')
                .map(|(id, _)| id)
                .ok_or_else(|| "stored gateway credential is malformed".to_owned())?;
            if actual != expected {
                return Err(
                    "installed gateway credential does not match its non-secret key identity"
                        .into(),
                );
            }
        }
        let source = source
            .map(|value| fixed_hex::<32>("source account", value))
            .transpose()?;
        let asset = asset
            .map(|value| fixed_hex::<32>("asset", value))
            .transpose()?;
        if mode == DeploymentMode::Full && (source.is_none() || asset.is_none()) {
            return Err(
                "payment mode requires the installed source account and asset binding".into(),
            );
        }
        if mode == DeploymentMode::ReadOnly && (source.is_some() || asset.is_some()) {
            return Err("read-only mode cannot carry a payment source binding".into());
        }
        let metadata = configuration
            .keys
            .get(key)
            .ok_or_else(|| format!("key {key} does not exist"))?;
        fixed_hex::<32>("public key", &metadata.public_key)?;
        Ok(Self {
            client: Client::new_gateway(&environment.endpoint, credential)?,
            network_id: environment.network_id,
            key: key.to_owned(),
            source,
            asset,
        })
    }
}

pub fn surface(mode: DeploymentMode) -> Result<Vec<ToolDefinition>, String> {
    let mut tools = Vec::with_capacity(SERVED.len());
    for definition in SERVED {
        if mode == DeploymentMode::ReadOnly && definition.kind != ToolKind::Read {
            continue;
        }
        tools.push(definition);
    }
    if tools.is_empty() {
        return Err("the selected deployment mode would serve no tool".into());
    }
    Ok(tools)
}

pub fn scopes(tools: &[ToolDefinition]) -> Vec<String> {
    let mut unique = BTreeSet::new();
    for tool in tools {
        unique.insert(tool.required_scope.to_owned());
    }
    unique.into_iter().collect()
}

pub const fn mode_name(mode: DeploymentMode) -> &'static str {
    match mode {
        DeploymentMode::Full => "full",
        DeploymentMode::ReadOnly => "read-only",
    }
}

pub const fn kind_name(kind: ToolKind) -> &'static str {
    match kind {
        ToolKind::Read => "read",
        ToolKind::Write => "write",
    }
}

pub fn descriptor(tool: ToolDefinition) -> Value {
    json!({
        "name": tool.name,
        "kind": kind_name(tool.kind),
        "scope": tool.required_scope,
        "mutation": tool.mutation,
        "evidence": tool.evidence,
        "arguments": schema(tool.name),
    })
}

pub fn description(name: &str) -> &'static str {
    match name {
        "receipt.get" => "Fetch gateway-verified receipt material for one activity.",
        "activity.submit" => {
            "Sign and submit a canonical payment from the installation-bound account and asset."
        }
        _ => "This tool is not served by this deployment.",
    }
}

pub fn schema(name: &str) -> Value {
    match name {
        "receipt.get" => json!({
            "type": "object",
            "properties": {"activity_id": {"type": "string", "pattern": "^[0-9a-fA-F]{64}$"}},
            "required": ["activity_id"],
            "additionalProperties": false,
        }),
        "activity.submit" => json!({
            "type": "object",
            "properties": {
                "destination": {"type": "string", "pattern": "^[0-9a-fA-F]{64}$"},
                "amount": {"type": "string", "pattern": "^[1-9][0-9]*$"},
                "account_sequence": {"type": "string", "pattern": "^[0-9]+$"},
                "not_before_ms": {"type": "string", "pattern": "^[0-9]+$"},
                "expires_at_ms": {"type": "string", "pattern": "^[0-9]+$"},
                "fee_limit": {"type": "string", "pattern": "^[0-9]+$"},
                "idempotency_key": {"type": "string", "pattern": "^[0-9a-fA-F]{64}$"}
            },
            "required": [
                "destination", "amount", "account_sequence", "not_before_ms",
                "expires_at_ms", "fee_limit", "idempotency_key"
            ],
            "additionalProperties": false,
        }),
        _ => json!({"type": "object", "additionalProperties": false}),
    }
}

pub fn invoke(runtime: &Runtime, tool: ToolDefinition, arguments: &Value) -> Result<Value, String> {
    match tool.name {
        "receipt.get" => {
            let activity = text(arguments, "activity_id")?;
            validate_hex32(&activity, "activity id")?;
            runtime
                .client
                .get(&format!("/v1/receipts/{}", activity.to_ascii_lowercase()))
        }
        "activity.submit" => submit(runtime, arguments),
        _ => Err(format!(
            "tool {} is not served by this deployment",
            tool.name
        )),
    }
}

fn submit(runtime: &Runtime, arguments: &Value) -> Result<Value, String> {
    let source = runtime
        .source
        .ok_or_else(|| "the runtime has no payment source binding".to_owned())?;
    let asset = runtime
        .asset
        .ok_or_else(|| "the runtime has no payment asset binding".to_owned())?;
    let destination = fixed_hex::<32>("destination", &text(arguments, "destination")?)?;
    let amount = integer::<u128>(arguments, "amount")?;
    if amount == 0 {
        return Err("amount must be greater than zero".into());
    }
    let account_sequence = integer::<u64>(arguments, "account_sequence")?;
    let not_before = integer::<u64>(arguments, "not_before_ms")?;
    let expires_at = integer::<u64>(arguments, "expires_at_ms")?;
    if expires_at <= not_before || expires_at - not_before > MAX_PAYMENT_WINDOW_MS {
        return Err(format!(
            "payment validity must be non-empty and no wider than {MAX_PAYMENT_WINDOW_MS} milliseconds"
        ));
    }
    let fee_limit = integer::<u128>(arguments, "fee_limit")?;
    let idempotency = fixed_hex::<32>("idempotency key", &text(arguments, "idempotency_key")?)?;
    let seed = credential::key_seed(&runtime.key)?;
    let signing_key = SigningKey::from_bytes(&seed);
    let public_key = signing_key.verifying_key().to_bytes();
    let context = context_hash(&source, &destination, &asset, amount, &idempotency);
    let payload = send_payload(
        &signing_key,
        source,
        destination,
        asset,
        amount,
        account_sequence,
        idempotency,
        expires_at,
        context,
        runtime.network_id,
    )?;
    let activity_type = ActivityType::new(ModuleId::Asset, SEND_ACTIVITY)
        .map_err(|error| format!("asset send activity is unavailable: {error:?}"))?;
    let registration = ModuleRegistration::new(ModuleId::Asset, &[activity_type])
        .map_err(|error| format!("asset module registration is invalid: {error:?}"))?;
    let registry = ModuleRegistry::new(&[registration])
        .map_err(|error| format!("asset module registry is invalid: {error:?}"))?;
    let payload = Payload::new(&registry, activity_type, &payload)
        .map_err(|error| format!("payment payload is invalid: {error:?}"))?;
    let payload_hash = domain_hash(Domain::PayloadHash, payload.as_bytes());
    let actor = Did::new(&source).map_err(|error| format!("source DID is invalid: {error:?}"))?;
    let authority = Authority::owner(&public_key)
        .map_err(|error| format!("owner authority is invalid: {error:?}"))?;
    let timestamp = TimestampBound::new(not_before, expires_at)
        .map_err(|error| format!("payment timestamp is invalid: {error:?}"))?;
    let mut builder = EnvelopeBuilder::new();
    builder
        .protocol_version(1)
        .and_then(|value| value.network_id(runtime.network_id))
        .and_then(|value| value.activity_type(activity_type))
        .and_then(|value| value.actor_did(actor))
        .and_then(|value| value.authority(authority))
        .and_then(|value| value.account_sequence(account_sequence))
        .and_then(|value| value.timestamp_bound(timestamp))
        .and_then(|value| value.idempotency_key(IdempotencyKey::new(idempotency)))
        .and_then(|value| value.fee_limit(Amount::from_u128(fee_limit)))
        .and_then(|value| value.payload_hash(payload_hash))
        .and_then(|value| value.payload(payload))
        .map_err(|error| format!("payment envelope is invalid: {error:?}"))?;
    let unsigned = builder
        .build()
        .map_err(|error| format!("payment envelope is incomplete: {error:?}"))?;
    let unsigned_bytes = encode_unsigned_envelope(&unsigned)
        .map_err(|error| format!("payment signing bytes are invalid: {error:?}"))?;
    let signature = signing_key
        .sign(&domain_hash(Domain::SignaturePreimage, &unsigned_bytes))
        .to_bytes();
    let signed = unsigned.attach_signature(
        Signature::new(&signature)
            .map_err(|error| format!("payment signature is invalid: {error:?}"))?,
    );
    let canonical = encode_signed_envelope(&signed)
        .map_err(|error| format!("signed payment is invalid: {error:?}"))?;
    let idempotency = hex_encode(&idempotency);
    let response = runtime.client.post_stateful(
        "/v1/activities",
        &json!({"activity": hex_encode(&canonical)}),
        &idempotency,
    )?;
    Ok(json!({
        "source": hex_encode(&source),
        "asset": hex_encode(&asset),
        "idempotency_key": idempotency,
        "gateway": response,
    }))
}

#[allow(clippy::too_many_arguments)]
fn send_payload(
    signing_key: &SigningKey,
    source: [u8; 32],
    destination: [u8; 32],
    asset: [u8; 32],
    amount: u128,
    sequence: u64,
    idempotency: [u8; 32],
    expires_at: u64,
    context: [u8; 32],
    network_id: u32,
) -> Result<Vec<u8>, String> {
    let public_key = signing_key.verifying_key().to_bytes();
    let mut authorization = Encoder::new(512);
    authorization
        .u16(0x5301)
        .and_then(|()| authorization.fixed(&source))
        .and_then(|()| authorization.fixed(&destination))
        .and_then(|()| authorization.fixed(&asset))
        .and_then(|()| authorization.u128(amount))
        .and_then(|()| authorization.u64(sequence))
        .and_then(|()| authorization.fixed(&idempotency))
        .and_then(|()| authorization.u64(expires_at))
        .and_then(|()| authorization.fixed(&context))
        .and_then(|()| authorization.u8(0))
        .and_then(|()| authorization.u8(1))
        .and_then(|()| authorization.fixed(&source))
        .and_then(|()| authorization.fixed(&context))
        .and_then(|()| authorization.u32(network_id))
        .and_then(|()| authorization.u16(1))
        .map_err(|error| format!("payment authorization is too large: {error:?}"))?;
    let signature = signing_key
        .sign(&domain_hash(
            Domain::SignaturePreimage,
            &authorization.finish(),
        ))
        .to_bytes();
    let mut payload = Encoder::new(512);
    payload
        .u16(0x5301)
        .and_then(|()| payload.u16(10))
        .and_then(|()| payload.fixed(&source))
        .and_then(|()| payload.fixed(&destination))
        .and_then(|()| payload.fixed(&asset))
        .and_then(|()| payload.u128(amount))
        .and_then(|()| payload.u64(sequence))
        .and_then(|()| payload.fixed(&idempotency))
        .and_then(|()| payload.u64(expires_at))
        .and_then(|()| payload.fixed(&context))
        .and_then(|()| payload.u8(0))
        .and_then(|()| payload.u8(1))
        .and_then(|()| payload.fixed(&source))
        .and_then(|()| payload.fixed(&public_key))
        .and_then(|()| payload.fixed(&signature))
        .and_then(|()| payload.fixed(&context))
        .and_then(|()| payload.u32(network_id))
        .and_then(|()| payload.u16(1))
        .map_err(|error| format!("payment payload is too large: {error:?}"))?;
    Ok(payload.finish())
}

fn context_hash(
    source: &[u8; 32],
    destination: &[u8; 32],
    asset: &[u8; 32],
    amount: u128,
    idempotency: &[u8; 32],
) -> [u8; 32] {
    let mut bytes = Vec::with_capacity(144);
    bytes.extend_from_slice(source);
    bytes.extend_from_slice(destination);
    bytes.extend_from_slice(asset);
    bytes.extend_from_slice(&amount.to_be_bytes());
    bytes.extend_from_slice(idempotency);
    domain_hash(Domain::ContextHash, &bytes)
}

fn domain_hash(domain: Domain, bytes: &[u8]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(domain.tag());
    digest.update(bytes);
    digest.finalize().into()
}

fn text(arguments: &Value, field: &str) -> Result<String, String> {
    let value = arguments
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("argument {field} must be a string"))?;
    if value.is_empty() || value.len() > MAX_ARGUMENT_BYTES {
        return Err(format!(
            "argument {field} must be 1-{MAX_ARGUMENT_BYTES} bytes"
        ));
    }
    Ok(value.to_owned())
}

fn integer<T: std::str::FromStr>(arguments: &Value, field: &str) -> Result<T, String> {
    text(arguments, field)?
        .parse::<T>()
        .map_err(|_| format!("argument {field} is outside its unsigned integer range"))
}

fn validate_hex32(value: &str, name: &str) -> Result<(), String> {
    validate_resource_id(value, name)?;
    fixed_hex::<32>(name, value).map(|_| ())
}

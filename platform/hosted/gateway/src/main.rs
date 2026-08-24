use layerx_platform_gateway::http::{
    self, Client, Endpoint, IncomingRequest, OutgoingResponse, UpstreamResponse,
};
use layerx_platform_gateway::store::{KeyRecord, RedisEndpoint, RedisStore, Reservation};
use layerx_platform_gateway::{
    authenticate_gateway_key, production_route, verify_activity_operation, verify_submission,
    AccessError, AuthorityFacts, IssuedKey, PrincipalId, ProductionRoute, Quota,
};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use native_tls::{Certificate, Identity};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::io::Write;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, Zeroizing};

const MAX_REQUEST: usize = 512 * 1024;
const MAX_CONNECTIONS: usize = 256;
const MAX_IDEMPOTENCY_SECONDS: u64 = 2_592_000;
static ACTIVE_CONNECTIONS: AtomicUsize = AtomicUsize::new(0);

struct Config {
    listen: SocketAddr,
    tls: Arc<ServerConfig>,
    client: Client,
    component: Endpoint,
    component_token: Zeroizing<String>,
    authority: Endpoint,
    authority_token: Zeroizing<String>,
    identity: Endpoint,
    identity_token: Zeroizing<String>,
    store: RedisStore,
    trusted_sequencer_key: [u8; 32],
    key_provisioning_key: Zeroizing<[u8; 32]>,
    network_id: String,
    wire_version: String,
    protocol_version: u16,
    protocol_network_id: u32,
    modules: ModuleRegistry,
    idempotency_seconds: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionResponse {
    active: bool,
    sub: String,
    allowed_signer_public_keys: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IssueRequest {
    signer_public_key: String,
    scopes: Vec<String>,
    quota_requests: u64,
    quota_window_seconds: u64,
}

#[derive(Serialize)]
struct PublicKeyRecord {
    id: String,
    signer_public_key: String,
    scopes: Vec<String>,
    quota_requests: u64,
    quota_window_seconds: u64,
    state: &'static str,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ComponentActivity {
    state: String,
    activity_id: String,
    #[serde(default)]
    receipt: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ComponentReceipt {
    activity_id: String,
    receipt: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthorityResponse {
    activity_id: String,
    batch_id: String,
    asset: String,
    previous_state_root: String,
    resulting_state_root: String,
    sequencer_public_key: String,
    network_id: String,
    wire_version: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadinessResponse {
    ready: bool,
    network_id: String,
    wire_version: String,
    #[serde(default)]
    synchronous_receipts: bool,
    #[serde(default)]
    state_snapshot: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModuleFile {
    modules: Vec<ModuleDeclaration>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModuleDeclaration {
    module: u16,
    ordinals: Vec<u16>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JsonActivity {
    activity: String,
}

fn now() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| "system clock precedes Unix epoch".to_owned())
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

fn parse_hex32(value: &str) -> Result<[u8; 32], String> {
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

fn decode_hex(value: &str, maximum: usize) -> Result<Vec<u8>, String> {
    if value.len() % 2 != 0 || value.len() / 2 > maximum {
        return Err("hexadecimal payload exceeds its bound".to_owned());
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).map_err(|_| "hexadecimal payload is invalid")?;
            u8::from_str_radix(text, 16).map_err(|_| "hexadecimal payload is invalid")
        })
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(char::from(DIGITS[usize::from(byte >> 4)]));
        value.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    value
}

fn digest(parts: &[&[u8]]) -> String {
    let mut hash = Sha256::new();
    for part in parts {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part);
    }
    format!("{hash:x}")
}

fn valid_identifier(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn tls_config() -> Result<Arc<ServerConfig>, String> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| "failed to install TLS crypto provider".to_owned())?;
    let cert = CertificateDer::from(
        fs::read(
            env::var("LAYERX_GATEWAY_TLS_CERT_DER")
                .map_err(|_| "gateway TLS certificate is required")?,
        )
        .map_err(|error| error.to_string())?,
    );
    let key = PrivateKeyDer::from(PrivatePkcs8KeyDer::from(
        fs::read(
            env::var("LAYERX_GATEWAY_TLS_KEY_DER").map_err(|_| "gateway TLS key is required")?,
        )
        .map_err(|error| error.to_string())?,
    ));
    ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .map(Arc::new)
        .map_err(|error| error.to_string())
}

fn config() -> Result<Config, String> {
    let ca = Certificate::from_der(
        &fs::read(
            env::var("LAYERX_GATEWAY_OUTBOUND_CA_DER")
                .map_err(|_| "gateway outbound CA is required")?,
        )
        .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let identity_password = read_secret("LAYERX_GATEWAY_CLIENT_IDENTITY_PASSWORD_FILE")?;
    let identity = Identity::from_pkcs12(
        &fs::read(
            env::var("LAYERX_GATEWAY_CLIENT_IDENTITY_PKCS12")
                .map_err(|_| "gateway client identity is required")?,
        )
        .map_err(|error| error.to_string())?,
        identity_password.as_str(),
    )
    .map_err(|error| error.to_string())?;
    let trusted_key = read_secret("LAYERX_GATEWAY_SEQUENCER_PUBLIC_KEY_FILE")?;
    let trusted_sequencer_key = parse_hex32(trusted_key.as_str())?;
    let provisioning_key = read_secret("LAYERX_GATEWAY_KEY_PROVISIONING_KEY_FILE")?;
    let key_provisioning_key = Zeroizing::new(parse_hex32(provisioning_key.as_str())?);
    let idempotency_seconds = env::var("LAYERX_GATEWAY_IDEMPOTENCY_SECONDS")
        .unwrap_or_else(|_| "604800".to_owned())
        .parse::<u64>()
        .map_err(|_| "gateway idempotency retention is invalid".to_owned())?;
    if !(3600..=MAX_IDEMPOTENCY_SECONDS).contains(&idempotency_seconds) {
        return Err("gateway idempotency retention is outside its bound".to_owned());
    }
    let network_id = env::var("LAYERX_GATEWAY_NETWORK_ID")
        .map_err(|_| "gateway network identifier is required")?;
    let wire_version = env::var("LAYERX_GATEWAY_LXP_WIRE_VERSION")
        .map_err(|_| "gateway LXP wire version is required")?;
    if !valid_identifier(&network_id, 64) || !valid_identifier(&wire_version, 32) {
        return Err("gateway network or wire version is invalid".to_owned());
    }
    let protocol_version = wire_version
        .parse::<u16>()
        .map_err(|_| "gateway LXP wire version must be numeric".to_owned())?;
    let protocol_network_id = env::var("LAYERX_GATEWAY_PROTOCOL_NETWORK_ID")
        .map_err(|_| "gateway protocol network identifier is required")?
        .parse::<u32>()
        .map_err(|_| "gateway protocol network identifier is invalid".to_owned())?;
    let module_file: ModuleFile = serde_json::from_slice(
        &fs::read(
            env::var("LAYERX_GATEWAY_MODULE_REGISTRY_FILE")
                .map_err(|_| "gateway module registry is required")?,
        )
        .map_err(|error| error.to_string())?,
    )
    .map_err(|_| "gateway module registry is invalid".to_owned())?;
    if module_file.modules.is_empty() || module_file.modules.len() > 8 {
        return Err("gateway module registry is outside its bound".to_owned());
    }
    let mut registrations = Vec::with_capacity(module_file.modules.len());
    for declaration in module_file.modules {
        let module = ModuleId::from_u16(declaration.module)
            .map_err(|_| "gateway module registry names an unknown module".to_owned())?;
        let activity_types = declaration
            .ordinals
            .into_iter()
            .map(|ordinal| ActivityType::new(module, ordinal))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| "gateway module registry contains an invalid ordinal".to_owned())?;
        let registration = ModuleRegistration::new(module, &activity_types)
            .map_err(|_| "gateway module registry declaration is invalid".to_owned())?;
        registrations.push(registration);
    }
    let modules = ModuleRegistry::new(&registrations)
        .map_err(|_| "gateway module registry contains duplicates".to_owned())?;
    Ok(Config {
        listen: env::var("LAYERX_GATEWAY_LISTEN")
            .unwrap_or_else(|_| "0.0.0.0:9443".to_owned())
            .parse::<SocketAddr>()
            .map_err(|_| "gateway listen address is invalid".to_owned())?,
        tls: tls_config()?,
        client: Client::new(ca.clone(), identity),
        component: Endpoint::parse(
            &env::var("LAYERX_GATEWAY_COMPONENT_URL")
                .map_err(|_| "gateway component URL is required")?,
        )?,
        component_token: read_secret("LAYERX_GATEWAY_COMPONENT_TOKEN_FILE")?,
        authority: Endpoint::parse(
            &env::var("LAYERX_GATEWAY_AUTHORITY_URL")
                .map_err(|_| "gateway authority URL is required")?,
        )?,
        authority_token: read_secret("LAYERX_GATEWAY_AUTHORITY_TOKEN_FILE")?,
        identity: Endpoint::parse(
            &env::var("LAYERX_GATEWAY_IDENTITY_URL")
                .map_err(|_| "gateway identity URL is required")?,
        )?,
        identity_token: read_secret("LAYERX_GATEWAY_IDENTITY_TOKEN_FILE")?,
        store: RedisStore::new(
            RedisEndpoint::parse(
                &env::var("LAYERX_GATEWAY_REDIS_URL")
                    .map_err(|_| "gateway Redis URL is required")?,
            )?,
            ca,
            read_secret("LAYERX_GATEWAY_REDIS_USERNAME_FILE")?,
            read_secret("LAYERX_GATEWAY_REDIS_PASSWORD_FILE")?,
        ),
        trusted_sequencer_key,
        key_provisioning_key,
        network_id,
        wire_version,
        protocol_version,
        protocol_network_id,
        modules,
        idempotency_seconds,
    })
}

fn response(status: u16, code: &str, retry_after: Option<u64>) -> OutgoingResponse {
    OutgoingResponse {
        status,
        body: serde_json::json!({ "ok": false, "error": { "code": code } })
            .to_string()
            .into_bytes(),
        retry_after,
    }
}

fn json_response(status: u16, value: serde_json::Value) -> OutgoingResponse {
    OutgoingResponse {
        status,
        body: value.to_string().into_bytes(),
        retry_after: None,
    }
}

fn trace(request: &IncomingRequest) -> String {
    let supplied = request
        .headers
        .get("x-trace-id")
        .map(String::as_str)
        .unwrap_or("");
    if supplied.strip_prefix("trc_").is_some_and(|digits| {
        digits.len() == 32
            && digits
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }) {
        supplied.to_owned()
    } else if valid_identifier(supplied, 64) {
        format!("gw-{supplied}")
    } else {
        format!(
            "gw-{}",
            &digest(&[
                request.method.as_bytes(),
                request.path.as_bytes(),
                &now().unwrap_or(0).to_be_bytes()
            ])[..24]
        )
    }
}

fn upstream_json(
    config: &Config,
    endpoint: &Endpoint,
    token: &str,
    method: &str,
    path: &str,
    idempotency: Option<&str>,
    body: &[u8],
) -> Result<UpstreamResponse, OutgoingResponse> {
    config
        .client
        .request(
            endpoint,
            method,
            path,
            token,
            idempotency,
            "application/json",
            body,
        )
        .map_err(|_| response(503, "component_unavailable", Some(5)))
}

fn session(
    config: &Config,
    request: &IncomingRequest,
) -> Result<(PrincipalId, SessionResponse), OutgoingResponse> {
    let token = request
        .headers
        .get("authorization")
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| !value.is_empty() && value.len() <= 4096)
        .ok_or_else(|| response(401, "session_required", None))?;
    let body = Zeroizing::new(
        serde_json::to_vec(&serde_json::json!({ "token": token }))
            .map_err(|_| response(503, "identity_unavailable", Some(5)))?,
    );
    let upstream = upstream_json(
        config,
        &config.identity,
        config.identity_token.as_str(),
        "POST",
        "/v1/sessions/introspect",
        None,
        &body,
    )?;
    if upstream.status != 200 || upstream.content_type != "application/json" {
        return Err(response(401, "session_required", None));
    }
    let session: SessionResponse = serde_json::from_slice(&upstream.body)
        .map_err(|_| response(503, "identity_unavailable", Some(5)))?;
    let principal = PrincipalId::new(session.sub.clone())
        .map_err(|_| response(401, "session_required", None))?;
    if !session.active
        || session.allowed_signer_public_keys.len() > 128
        || session
            .allowed_signer_public_keys
            .iter()
            .any(|key| parse_hex32(key).is_err())
    {
        return Err(response(401, "session_required", None));
    }
    Ok((principal, session))
}

fn principal_digest(principal: &PrincipalId) -> String {
    hex(&principal.audit_digest())
}

fn key_record(
    issued: &IssuedKey,
    principal: &PrincipalId,
    signer: &str,
    scopes: &str,
    quota: Quota,
    epoch: u64,
) -> KeyRecord {
    let salt = digest(&[b"gateway-key-salt-v1", issued.id().as_bytes()]);
    KeyRecord {
        key_id: issued.id().to_owned(),
        principal_digest: principal_digest(principal),
        secret_digest: digest(&[
            b"gateway-key-v1",
            salt.as_bytes(),
            issued.secret().as_bytes(),
        ]),
        salt,
        signer_public_key: signer.to_ascii_lowercase(),
        scopes: scopes.to_owned(),
        quota_requests: quota.requests(),
        quota_window_seconds: quota.window_seconds(),
        epoch,
        disabled: false,
    }
}

fn canonical_scopes(scopes: &[String]) -> Result<String, ()> {
    if scopes.is_empty() || scopes.len() > 3 {
        return Err(());
    }
    let mut previous = None;
    for scope in scopes {
        if !matches!(
            scope.as_str(),
            "activity:write" | "receipt:read" | "state:read"
        ) || previous.is_some_and(|value: &str| value >= scope.as_str())
        {
            return Err(());
        }
        previous = Some(scope.as_str());
    }
    Ok(scopes.join(","))
}

fn record_scopes(record: &KeyRecord) -> Vec<&str> {
    record.scopes.split(',').collect()
}

fn permits(record: &KeyRecord, route: &ProductionRoute<'_>) -> bool {
    let required = match route {
        ProductionRoute::Activity => "activity:write",
        ProductionRoute::State => "state:read",
        ProductionRoute::Receipt(_) => "receipt:read",
    };
    record.scopes.split(',').any(|scope| scope == required)
}

fn audit_event(principal_digest: &str, action: &str, subject: &str, outcome: &str) -> String {
    let event = digest(&[
        b"gateway-audit-v1",
        action.as_bytes(),
        subject.as_bytes(),
        outcome.as_bytes(),
        &now().unwrap_or(0).to_be_bytes(),
    ]);
    format!("{principal_digest}:{event}")
}

fn manage_keys(config: &Config, request: &IncomingRequest) -> OutgoingResponse {
    let (principal, session) = match session(config, request) {
        Ok(value) => value,
        Err(error) => return error,
    };
    let principal_hash = principal_digest(&principal);
    if request.method == "POST" && request.path == "/v1/keys" {
        let issuance_idempotency = match request.headers.get("idempotency-key") {
            Some(value) if valid_identifier(value, 128) => value,
            _ => return response(400, "idempotency_key_required", None),
        };
        let issue: IssueRequest = match serde_json::from_slice(&request.body) {
            Ok(value) => value,
            Err(_) => return response(400, "invalid_key_request", None),
        };
        if !session
            .allowed_signer_public_keys
            .iter()
            .any(|key| key.eq_ignore_ascii_case(&issue.signer_public_key))
        {
            return response(403, "signer_not_owned", None);
        }
        let quota = match Quota::new(issue.quota_requests, issue.quota_window_seconds) {
            Ok(value) => value,
            Err(_) => return response(400, "invalid_quota", None),
        };
        let scopes = match canonical_scopes(&issue.scopes) {
            Ok(value) => value,
            Err(()) => return response(400, "invalid_scopes", None),
        };
        let context = digest(&[
            b"gateway-key-issuance-v1",
            principal_hash.as_bytes(),
            issuance_idempotency.as_bytes(),
        ]);
        let issued = IssuedKey::derive(&config.key_provisioning_key, context.as_bytes());
        let record = key_record(
            &issued,
            &principal,
            &issue.signer_public_key,
            &scopes,
            quota,
            1,
        );
        let written = config
            .store
            .issue_key(
                &record,
                &audit_event(&principal_hash, "key_issue", &record.key_id, "issued"),
            )
            .is_ok();
        let existing = if written {
            Ok(None)
        } else {
            config.store.key(&record.key_id)
        };
        let replayed = matches!(&existing, Ok(Some(value)) if value == &record);
        if !written && !replayed {
            return match existing {
                Ok(Some(_)) => response(409, "idempotency_conflict", None),
                _ => response(503, "persistence_unavailable", Some(5)),
            };
        }
        return json_response(
            if written { 201 } else { 200 },
            serde_json::json!({
                "ok": true,
                "key": {
                    "id": issued.id(),
                    "secret": issued.secret(),
                    "authorization_scheme": "LayerX-Key",
                    "signer_public_key": record.signer_public_key,
                    "scopes": record_scopes(&record),
                    "quota_requests": record.quota_requests,
                    "quota_window_seconds": record.quota_window_seconds
                }
            }),
        );
    }
    if request.method == "GET" && request.path == "/v1/keys" {
        let ids = match config.store.list_keys(&principal_hash) {
            Ok(value) => value,
            Err(_) => return response(503, "persistence_unavailable", Some(5)),
        };
        let mut records = Vec::with_capacity(ids.len());
        for id in ids {
            let Some(record) = config.store.key(&id).ok().flatten() else {
                return response(503, "persistence_unavailable", Some(5));
            };
            if record
                .principal_digest
                .as_bytes()
                .ct_eq(principal_hash.as_bytes())
                .unwrap_u8()
                != 1
            {
                return response(503, "persistence_unavailable", Some(5));
            }
            let public_scopes = record_scopes(&record)
                .into_iter()
                .map(str::to_owned)
                .collect();
            records.push(PublicKeyRecord {
                id: record.key_id,
                signer_public_key: record.signer_public_key,
                scopes: public_scopes,
                quota_requests: record.quota_requests,
                quota_window_seconds: record.quota_window_seconds,
                state: if record.disabled { "revoked" } else { "active" },
            });
        }
        return json_response(200, serde_json::json!({ "ok": true, "keys": records }));
    }
    let Some(suffix) = request.path.strip_prefix("/v1/keys/") else {
        return response(404, "not_found", None);
    };
    let (key_id, rotate) = suffix
        .strip_suffix("/rotate")
        .map_or((suffix, false), |id| (id, true));
    if !valid_identifier(key_id, 64) {
        return response(404, "not_found", None);
    }
    let Some(old) = config.store.key(key_id).ok().flatten() else {
        return response(404, "not_found", None);
    };
    if old
        .principal_digest
        .as_bytes()
        .ct_eq(principal_hash.as_bytes())
        .unwrap_u8()
        != 1
    {
        return response(404, "not_found", None);
    }
    if request.method == "DELETE" && !rotate {
        return match config.store.revoke_key(
            key_id,
            &principal_hash,
            &audit_event(&principal_hash, "key_revoke", key_id, "revoked"),
        ) {
            Ok(true) => json_response(
                200,
                serde_json::json!({ "ok": true, "id": key_id, "state": "revoked" }),
            ),
            Ok(false) => response(404, "not_found", None),
            Err(_) => response(503, "persistence_unavailable", Some(5)),
        };
    }
    if request.method == "POST" && rotate {
        if !session
            .allowed_signer_public_keys
            .iter()
            .any(|key| key.eq_ignore_ascii_case(&old.signer_public_key))
        {
            return response(403, "signer_not_owned", None);
        }
        let rotation_idempotency = match request.headers.get("idempotency-key") {
            Some(value) if valid_identifier(value, 128) => value,
            _ => return response(400, "idempotency_key_required", None),
        };
        let context = digest(&[
            b"gateway-key-rotation-v1",
            principal_hash.as_bytes(),
            key_id.as_bytes(),
            rotation_idempotency.as_bytes(),
        ]);
        let issued = IssuedKey::derive(&config.key_provisioning_key, context.as_bytes());
        let quota = match Quota::new(old.quota_requests, old.quota_window_seconds) {
            Ok(value) => value,
            Err(_) => return response(503, "persistence_unavailable", Some(5)),
        };
        let replacement = key_record(
            &issued,
            &principal,
            &old.signer_public_key,
            &old.scopes,
            quota,
            1,
        );
        let written = config
            .store
            .rotate_key(
                &old,
                &replacement,
                &audit_event(&principal_hash, "key_rotate", key_id, "rotated"),
            )
            .is_ok();
        let replayed = !written
            && config
                .store
                .key(&replacement.key_id)
                .ok()
                .flatten()
                .is_some_and(|existing| existing == replacement);
        return if written || replayed {
            json_response(
                if written { 201 } else { 200 },
                serde_json::json!({
                    "ok": true,
                    "key": {
                        "id": issued.id(),
                        "secret": issued.secret(),
                        "authorization_scheme": "LayerX-Key",
                        "scopes": record_scopes(&replacement),
                        "replaces": key_id
                    }
                }),
            )
        } else {
            response(409, "rotation_conflict", None)
        };
    }
    response(404, "not_found", None)
}

fn authenticate_key(
    config: &Config,
    request: &IncomingRequest,
) -> Result<KeyRecord, OutgoingResponse> {
    let authorization = request
        .headers
        .get("authorization")
        .ok_or_else(|| response(401, "api_key_required", None))?;
    authenticate_gateway_key(&config.store, authorization).map_err(|error| match error {
        AccessError::Unauthenticated => response(401, "api_key_required", None),
        AccessError::PersistenceUnavailable => response(503, "persistence_unavailable", Some(5)),
    })
}

fn authority(config: &Config, activity_id: &str) -> Result<AuthorityFacts, OutgoingResponse> {
    let upstream = upstream_json(
        config,
        &config.authority,
        config.authority_token.as_str(),
        "GET",
        &format!("/v1/authorized-batches/by-activity/{activity_id}"),
        None,
        &[],
    )?;
    if upstream.status != 200 || upstream.content_type != "application/json" {
        return Err(response(503, "authority_unavailable", Some(5)));
    }
    let facts: AuthorityResponse = serde_json::from_slice(&upstream.body)
        .map_err(|_| response(503, "authority_invalid", Some(5)))?;
    if !facts.activity_id.eq_ignore_ascii_case(activity_id)
        || facts.network_id != config.network_id
        || facts.wire_version != config.wire_version
    {
        return Err(response(503, "authority_mismatch", Some(5)));
    }
    Ok(AuthorityFacts::new(
        parse_hex32(&facts.batch_id).map_err(|_| response(503, "authority_invalid", Some(5)))?,
        parse_hex32(&facts.asset).map_err(|_| response(503, "authority_invalid", Some(5)))?,
        parse_hex32(&facts.previous_state_root)
            .map_err(|_| response(503, "authority_invalid", Some(5)))?,
        parse_hex32(&facts.resulting_state_root)
            .map_err(|_| response(503, "authority_invalid", Some(5)))?,
        parse_hex32(&facts.sequencer_public_key)
            .map_err(|_| response(503, "authority_invalid", Some(5)))?,
    ))
}

fn verified_result(
    config: &Config,
    activity_id: &str,
    receipt_hex: &str,
) -> Result<(Vec<u8>, Vec<u8>), OutgoingResponse> {
    let expected =
        parse_hex32(activity_id).map_err(|_| response(503, "component_invalid", Some(5)))?;
    let receipt = decode_hex(receipt_hex, 256 * 1024)
        .map_err(|_| response(503, "component_invalid", Some(5)))?;
    let facts = authority(config, activity_id)?;
    let verified = verify_activity_operation(
        &receipt,
        facts,
        &config.trusted_sequencer_key,
        Some(expected),
    )
    .map_err(|_| response(503, "receipt_verification_failed", Some(5)))?;
    Ok((verified.response().to_vec(), verified.receipt().to_vec()))
}

fn activity(
    config: &Config,
    request: &IncomingRequest,
    record: &KeyRecord,
    trace_id: &str,
) -> OutgoingResponse {
    let idempotency = match request.headers.get("idempotency-key") {
        Some(value) if valid_identifier(value, 128) => value,
        _ => return response(400, "idempotency_key_required", None),
    };
    let content_type = request
        .headers
        .get("content-type")
        .map(String::as_str)
        .unwrap_or("");
    if !matches!(
        content_type,
        "application/json" | "application/octet-stream"
    ) || request.body.is_empty()
    {
        return response(415, "activity_content_type_required", None);
    }
    let canonical = if content_type == "application/octet-stream" {
        request.body.clone()
    } else {
        let body: JsonActivity = match serde_json::from_slice(&request.body) {
            Ok(value) => value,
            Err(_) => return response(400, "invalid_activity", None),
        };
        match decode_hex(&body.activity, 512 * 1024) {
            Ok(value) => value,
            Err(_) => return response(400, "invalid_activity", None),
        }
    };
    let signer_public_key = match parse_hex32(&record.signer_public_key) {
        Ok(value) => value,
        Err(_) => return response(503, "persistence_unavailable", Some(5)),
    };
    let verified_submission = match verify_submission(
        &canonical,
        &config.modules,
        config.protocol_version,
        config.protocol_network_id,
        &signer_public_key,
    ) {
        Ok(value) => value,
        Err(_) => return response(403, "activity_authorization_refused", None),
    };
    if !idempotency.eq_ignore_ascii_case(&hex(&verified_submission.idempotency_key())) {
        return response(409, "protocol_idempotency_mismatch", None);
    }
    let protocol_idempotency = hex(&verified_submission.idempotency_key());
    let submitted_activity_id = hex(&verified_submission.activity_id());
    let request_digest = digest(&[
        b"gateway-activity-v1",
        record.signer_public_key.as_bytes(),
        content_type.as_bytes(),
        &canonical,
    ]);
    let scope = digest(&[
        record.principal_digest.as_bytes(),
        record.key_id.as_bytes(),
        protocol_idempotency.as_bytes(),
    ]);
    let audit = audit_event(
        &record.principal_digest,
        "activity",
        &record.key_id,
        "attempted",
    );
    let reservation = match config.store.reserve(
        record,
        &scope,
        &request_digest,
        now().unwrap_or(0),
        config.idempotency_seconds,
        &submitted_activity_id,
        &record.principal_digest,
        &audit,
    ) {
        Ok(value) => value,
        Err(_) => return response(503, "persistence_unavailable", Some(5)),
    };
    match reservation {
        Reservation::Revoked => return response(401, "api_key_required", None),
        Reservation::RateLimited {
            retry_after_seconds,
        } => return response(429, "quota_exceeded", Some(retry_after_seconds)),
        Reservation::Existing {
            digest: existing,
            state,
            response: stored,
            ..
        } => {
            if existing
                .as_bytes()
                .ct_eq(request_digest.as_bytes())
                .unwrap_u8()
                != 1
            {
                return response(409, "idempotency_conflict", None);
            }
            if state == "completed" {
                let Ok(result) = decode_hex(&stored, 512 * 1024) else {
                    return response(503, "persistence_unavailable", Some(5));
                };
                let Ok(result) = serde_json::from_slice::<serde_json::Value>(&result) else {
                    return response(503, "persistence_unavailable", Some(5));
                };
                return json_response(
                    200,
                    serde_json::json!({ "ok": true, "result": result, "trace": trace_id }),
                );
            }
            if let Some(status) = state
                .strip_prefix("refused_")
                .and_then(|value| value.parse::<u16>().ok())
            {
                let Ok(body) = decode_hex(&stored, 64 * 1024) else {
                    return response(503, "persistence_unavailable", Some(5));
                };
                return OutgoingResponse {
                    status,
                    body,
                    retry_after: None,
                };
            }
            if state != "pending" {
                return response(503, "operation_state_unknown", Some(5));
            }
        }
        Reservation::Reserved => {}
    }
    let upstream = match config.client.request(
        &config.component,
        "POST",
        "/v1/activities",
        config.component_token.as_str(),
        Some(&protocol_idempotency),
        "application/octet-stream",
        &canonical,
    ) {
        Ok(value) => value,
        Err(_) => {
            return json_response(
                202,
                serde_json::json!({ "ok": true, "result": { "state": "unknown" }, "trace": trace_id }),
            )
        }
    };
    if upstream.status == 202 {
        return json_response(
            202,
            serde_json::json!({ "ok": true, "result": { "state": "acknowledged" }, "trace": trace_id }),
        );
    }
    if upstream.status != 200 || upstream.content_type != "application/json" {
        return if (400..500).contains(&upstream.status) {
            let refusal = response(upstream.status, "activity_refused", None);
            if config
                .store
                .complete(
                    &scope,
                    &request_digest,
                    &format!("refused_{}", upstream.status),
                    &hex(&refusal.body),
                    "",
                    None,
                    &record.principal_digest,
                    &audit_event(
                        &record.principal_digest,
                        "activity",
                        &record.key_id,
                        "refused",
                    ),
                )
                .is_err()
            {
                response(503, "persistence_unavailable", Some(5))
            } else {
                refusal
            }
        } else {
            json_response(
                202,
                serde_json::json!({ "ok": true, "result": { "state": "unknown" }, "trace": trace_id }),
            )
        };
    }
    let component: ComponentActivity = match serde_json::from_slice(&upstream.body) {
        Ok(value) => value,
        Err(_) => return response(503, "component_invalid", Some(5)),
    };
    if component.state != "completed"
        || !component
            .activity_id
            .eq_ignore_ascii_case(&hex(&verified_submission.activity_id()))
        || component.receipt.is_empty()
    {
        return response(503, "component_invalid", Some(5));
    }
    let (result, receipt) =
        match verified_result(config, &component.activity_id, &component.receipt) {
            Ok(value) => value,
            Err(error) => return error,
        };
    if config
        .store
        .complete(
            &scope,
            &request_digest,
            "completed",
            &hex(&result),
            &hex(&receipt),
            Some(&component.activity_id.to_ascii_lowercase()),
            &record.principal_digest,
            &audit_event(
                &record.principal_digest,
                "activity",
                &record.key_id,
                "receipt_verified",
            ),
        )
        .is_err()
    {
        return response(503, "persistence_unavailable", Some(5));
    }
    let result = match serde_json::from_slice::<serde_json::Value>(&result) {
        Ok(value) => value,
        Err(_) => return response(503, "receipt_encoding_failed", Some(5)),
    };
    json_response(
        200,
        serde_json::json!({ "ok": true, "result": result, "trace": trace_id }),
    )
}

fn read_route(
    config: &Config,
    record: &KeyRecord,
    route: ProductionRoute<'_>,
    trace_id: &str,
) -> OutgoingResponse {
    if matches!(route, ProductionRoute::State) {
        return response(503, "principal_state_proof_unavailable", Some(30));
    }
    match config.store.consume_read(
        record,
        now().unwrap_or(0),
        &audit_event(
            &record.principal_digest,
            "read",
            &record.key_id,
            "attempted",
        ),
    ) {
        Ok(None) => {}
        Ok(Some(retry)) => return response(429, "quota_exceeded", Some(retry)),
        Err(_) => return response(503, "persistence_unavailable", Some(5)),
    }
    match route {
        ProductionRoute::State => response(503, "principal_state_proof_unavailable", Some(30)),
        ProductionRoute::Receipt(activity_id) => {
            let owner = match config
                .store
                .activity_owner(&activity_id.to_ascii_lowercase())
            {
                Ok(Some(value)) => value,
                Ok(None) => return response(404, "receipt_not_found", None),
                Err(_) => return response(503, "persistence_unavailable", Some(5)),
            };
            if owner
                .as_bytes()
                .ct_eq(record.principal_digest.as_bytes())
                .unwrap_u8()
                != 1
            {
                return response(404, "receipt_not_found", None);
            }
            let upstream = match config.client.request(
                &config.component,
                "GET",
                &format!("/v1/receipts/{activity_id}"),
                config.component_token.as_str(),
                None,
                "application/json",
                &[],
            ) {
                Ok(value) => value,
                Err(_) => return response(503, "component_unavailable", Some(5)),
            };
            if upstream.status == 404 {
                return response(404, "receipt_not_found", None);
            }
            let component: ComponentReceipt = match serde_json::from_slice(&upstream.body) {
                Ok(value)
                    if upstream.status == 200 && upstream.content_type == "application/json" =>
                {
                    value
                }
                _ => return response(503, "component_invalid", Some(5)),
            };
            if !component.activity_id.eq_ignore_ascii_case(activity_id) {
                return response(503, "component_invalid", Some(5));
            }
            let (_, receipt) = match verified_result(config, activity_id, &component.receipt) {
                Ok(value) => value,
                Err(error) => return error,
            };
            json_response(
                200,
                serde_json::json!({
                    "ok": true,
                    "result": { "activity_id": activity_id.to_ascii_lowercase(), "receipt": hex(&receipt) },
                    "trace": trace_id
                }),
            )
        }
        ProductionRoute::Activity => response(404, "not_found", None),
    }
}

fn dependency_ready(
    config: &Config,
    endpoint: &Endpoint,
    token: &str,
    require_routes: bool,
) -> bool {
    let Ok(upstream) = config.client.request(
        endpoint,
        "GET",
        "/readyz",
        token,
        None,
        "application/json",
        &[],
    ) else {
        return false;
    };
    let Ok(readiness) = serde_json::from_slice::<ReadinessResponse>(&upstream.body) else {
        return false;
    };
    upstream.status == 200
        && upstream.content_type == "application/json"
        && readiness.ready
        && readiness.network_id == config.network_id
        && readiness.wire_version == config.wire_version
        && (!require_routes || (readiness.synchronous_receipts && readiness.state_snapshot))
}

fn route(config: &Config, request: &IncomingRequest) -> OutgoingResponse {
    if request.headers.contains_key("x-layerx-principal")
        || request.headers.contains_key("x-layerx-api-key")
    {
        return response(400, "untrusted_identity_header", None);
    }
    if request.method == "GET" && request.path == "/livez" {
        return json_response(
            200,
            serde_json::json!({
                "status": "live",
                "service": "layerx-gateway",
                "package_semver": env!("CARGO_PKG_VERSION")
            }),
        );
    }
    if request.method == "GET" && request.path == "/readyz" {
        let store = config.store.ready();
        let component = dependency_ready(
            config,
            &config.component,
            config.component_token.as_str(),
            true,
        );
        let authority = dependency_ready(
            config,
            &config.authority,
            config.authority_token.as_str(),
            false,
        );
        let ready = false;
        return json_response(
            if ready { 200 } else { 503 },
            serde_json::json!({
                "status": if ready { "ready" } else { "degraded" },
                "service": "layerx-gateway",
                "package_semver": env!("CARGO_PKG_VERSION"),
                "lxp_wire_version": config.wire_version,
                "network_id": config.network_id,
                "components": {
                    "durable_store": if store { "ready" } else { "unavailable" },
                    "core_agent_boundary": if component { "ready" } else { "unavailable" },
                    "independent_receipt_authority": if authority { "ready" } else { "unavailable" },
                    "principal_state_boundary": "unavailable"
                }
            }),
        );
    }
    if request.method == "GET" && request.path == "/v1/status" {
        let gateway = config.store.ready();
        let core = dependency_ready(
            config,
            &config.component,
            config.component_token.as_str(),
            true,
        );
        let authority = dependency_ready(
            config,
            &config.authority,
            config.authority_token.as_str(),
            false,
        );
        return json_response(
            200,
            serde_json::json!({
                "ok": true,
                "services": {
                    "hosted_gateway": if gateway { "degraded" } else { "unavailable" },
                    "testnet_core": if core { "available" } else { "unavailable" },
                    "receipt_authority": if authority { "available" } else { "unavailable" },
                    "paxeer": "not_configured"
                },
                "lxp_wire_version": config.wire_version,
                "package_semver": env!("CARGO_PKG_VERSION")
            }),
        );
    }
    if request.path == "/v1/keys" || request.path.starts_with("/v1/keys/") {
        return manage_keys(config, request);
    }
    let parsed = match production_route(&request.method, &request.path) {
        Ok(value) => value,
        Err(_) => return response(404, "not_found", None),
    };
    let record = match authenticate_key(config, request) {
        Ok(value) => value,
        Err(error) => return error,
    };
    if !permits(&record, &parsed) {
        return response(403, "insufficient_scope", None);
    }
    let trace_id = trace(request);
    match parsed {
        ProductionRoute::Activity => activity(config, request, &record, &trace_id),
        read => read_route(config, &record, read, &trace_id),
    }
}

struct ConnectionGuard;

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        ACTIVE_CONNECTIONS.fetch_sub(1, Ordering::AcqRel);
    }
}

fn serve(config: &Arc<Config>, tcp: TcpStream) -> Result<(), String> {
    tcp.set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|error| error.to_string())?;
    tcp.set_write_timeout(Some(Duration::from_secs(10)))
        .map_err(|error| error.to_string())?;
    let connection =
        ServerConnection::new(Arc::clone(&config.tls)).map_err(|error| error.to_string())?;
    let mut stream = StreamOwned::new(connection, tcp);
    let request = match http::read_request(&mut stream, MAX_REQUEST) {
        Ok(value) => value,
        Err(_) => {
            return http::write_response(&mut stream, &response(400, "invalid_http_request", None));
        }
    };
    http::write_response(&mut stream, &route(config, &request))
}

fn run() -> Result<(), String> {
    let config = Arc::new(config()?);
    let listener = TcpListener::bind(config.listen).map_err(|error| error.to_string())?;
    for incoming in listener.incoming() {
        let tcp = incoming.map_err(|error| error.to_string())?;
        if ACTIVE_CONNECTIONS.fetch_add(1, Ordering::AcqRel) >= MAX_CONNECTIONS {
            ACTIVE_CONNECTIONS.fetch_sub(1, Ordering::AcqRel);
            let _ = tcp.shutdown(std::net::Shutdown::Both);
            continue;
        }
        let config = Arc::clone(&config);
        thread::spawn(move || {
            let _guard = ConnectionGuard;
            let _ = serve(&config, tcp);
        });
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        let _ = writeln!(std::io::stderr(), "layerx-gateway refused startup: {error}");
        std::process::exit(1);
    }
}

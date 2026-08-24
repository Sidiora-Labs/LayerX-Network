use layerx_platform_webhooks::error::WebhookError;
use layerx_platform_webhooks::events::{
    DeliveryId, EndpointId, EventKind, Principal, Verification,
};
use layerx_platform_webhooks::hosted::HostedService;
use layerx_platform_webhooks::http::{self, Reply, Request};
use layerx_platform_webhooks::scheme;
use layerx_platform_webhooks::trusted::{DeveloperIdentity, SourceTrigger, TrustedSources};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MAX_CONNECTIONS: usize = 256;
const DEFAULT_PAGE: usize = 50;
static ACTIVE_CONNECTIONS: AtomicUsize = AtomicUsize::new(0);

struct Config {
    listen: SocketAddr,
    tls: Arc<ServerConfig>,
    service: Arc<HostedService>,
    sources: Arc<TrustedSources>,
    identity: Arc<DeveloperIdentity>,
    source_trigger: Arc<SourceTrigger>,
    operator_trigger: Arc<SourceTrigger>,
    dispatch_interval: Duration,
    dispatch_budget: u32,
    retention_events: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegisterBody {
    url: String,
    #[serde(default)]
    kinds: Vec<String>,
    #[serde(default)]
    minimum_verification: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SuspendBody {
    reason: String,
}

#[derive(Serialize)]
struct SchemeDocument {
    scheme: &'static str,
    algorithm: &'static str,
    signed_message: &'static str,
    receiver_obligation: &'static str,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

fn number(name: &str, default: u64) -> Result<u64, String> {
    env::var(name).map_or(Ok(default), |value| {
        value
            .parse::<u64>()
            .map_err(|_| format!("{name} is not an integer"))
    })
}

fn tls_config() -> Result<Arc<ServerConfig>, String> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| "failed to install TLS provider".to_owned())?;
    let cert = CertificateDer::from(
        fs::read(
            env::var("LAYERX_WEBHOOKS_TLS_CERT_DER")
                .map_err(|_| "webhook TLS certificate is required")?,
        )
        .map_err(|error| error.to_string())?,
    );
    let key = PrivateKeyDer::from(PrivatePkcs8KeyDer::from(
        fs::read(
            env::var("LAYERX_WEBHOOKS_TLS_KEY_DER").map_err(|_| "webhook TLS key is required")?,
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
    Ok(Config {
        listen: env::var("LAYERX_WEBHOOKS_LISTEN")
            .unwrap_or_else(|_| "0.0.0.0:9444".to_owned())
            .parse::<SocketAddr>()
            .map_err(|_| "webhook listen address is invalid".to_owned())?,
        tls: tls_config()?,
        service: Arc::new(HostedService::from_environment()?),
        sources: Arc::new(TrustedSources::from_environment()?),
        identity: Arc::new(DeveloperIdentity::from_environment()?),
        source_trigger: Arc::new(SourceTrigger::from_environment()?),
        operator_trigger: Arc::new(SourceTrigger::operator_from_environment()?),
        dispatch_interval: Duration::from_secs(number(
            "LAYERX_WEBHOOKS_DISPATCH_INTERVAL_SECONDS",
            1,
        )?),
        dispatch_budget: u32::try_from(number("LAYERX_WEBHOOKS_DISPATCH_BUDGET", 64)?)
            .map_err(|_| "webhook dispatch budget is invalid".to_owned())?,
        retention_events: usize::try_from(number("LAYERX_WEBHOOKS_RETENTION_EVENTS", 10_000)?)
            .map_err(|_| "webhook retention bound is invalid".to_owned())?
            .clamp(1, 20_000),
    })
}

fn refusal(error: &WebhookError) -> Reply {
    let (status, code, retry) = match error {
        WebhookError::InvalidRequest => (400, "invalid_request", None),
        WebhookError::UnknownEndpoint => (404, "unknown_endpoint", None),
        WebhookError::UnknownDelivery => (404, "unknown_delivery", None),
        WebhookError::NotDeadLettered => (409, "not_dead_lettered", None),
        WebhookError::EndpointSuspended => (409, "endpoint_suspended", None),
        WebhookError::EventConflict => (409, "conflict", None),
        WebhookError::OrderViolation => (409, "order_violation", None),
        WebhookError::InvalidCursor => (400, "invalid_cursor", None),
        WebhookError::CursorExpired => (410, "cursor_expired", None),
        WebhookError::VerificationRequired => (422, "verification_required", None),
        WebhookError::SignatureRejected => (401, "signature_rejected", None),
        WebhookError::ReplayRejected => (409, "replay_rejected", None),
        WebhookError::StaleTimestamp => (400, "stale_timestamp", None),
        WebhookError::ReplayCapacity => (503, "replay_capacity", Some(10)),
        WebhookError::Entropy => (503, "entropy_unavailable", Some(5)),
        WebhookError::CorruptStore | WebhookError::Unavailable | WebhookError::Io(_) => {
            (503, "dependency_unavailable", Some(5))
        }
        WebhookError::Gateway(_) => (422, "verification_refused", None),
    };
    Reply::refusal(status, code, retry)
}

fn encoded<T: Serialize>(status: u16, value: &T) -> Reply {
    serde_json::to_string(value).map_or_else(
        |_| Reply::refusal(503, "encoding_failed", Some(5)),
        |body| Reply::json(status, body),
    )
}

fn body<T: for<'de> Deserialize<'de>>(request: &Request) -> Result<T, WebhookError> {
    serde_json::from_slice(&request.body).map_err(|_| WebhookError::InvalidRequest)
}

fn page(request: &Request) -> usize {
    request
        .parameter("limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(DEFAULT_PAGE)
        .clamp(1, 200)
}

fn principal(config: &Config, request: &Request) -> Result<Principal, WebhookError> {
    config.identity.authenticate(
        request.header("authorization"),
        request.header("cookie"),
        request.header("x-layerx-csrf"),
        matches!(request.method.as_str(), "POST" | "DELETE"),
    )
}

fn endpoints(config: &Config, request: &Request, principal: &Principal, at: u64) -> Reply {
    if request.method == "GET" {
        return config
            .service
            .snapshot(principal, at, page(request))
            .map_or_else(
                |error| refusal(&error),
                |value| encoded(200, &value.endpoints),
            );
    }
    if request.method != "POST" {
        return Reply::refusal(404, "not_found", None);
    }
    let idempotency = match request.header("idempotency-key") {
        Some(value) => value,
        None => return Reply::refusal(400, "idempotency_key_required", None),
    };
    let body = match body::<RegisterBody>(request) {
        Ok(value) => value,
        Err(error) => return refusal(&error),
    };
    let kinds = match body
        .kinds
        .iter()
        .map(|value| EventKind::parse(value))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(value) => value,
        Err(error) => return refusal(&error),
    };
    let minimum = match body.minimum_verification.as_deref() {
        Some(value) => match Verification::parse(value) {
            Ok(value) => value,
            Err(error) => return refusal(&error),
        },
        None => Verification::Unverified,
    };
    config
        .service
        .register(principal, &body.url, &kinds, minimum, idempotency, at)
        .map_or_else(|error| refusal(&error), |value| encoded(201, &value))
}

fn endpoint_route(
    config: &Config,
    request: &Request,
    principal: &Principal,
    endpoint: &str,
    action: &str,
    at: u64,
) -> Reply {
    let endpoint = match EndpointId::new(endpoint) {
        Ok(value) => value,
        Err(error) => return refusal(&error),
    };
    match (request.method.as_str(), action) {
        ("GET", "events") => config
            .service
            .events_since(
                principal,
                &endpoint,
                request.parameter("cursor"),
                page(request),
            )
            .map_or_else(|error| refusal(&error), |value| encoded(200, &value)),
        ("GET", "keys") => config
            .service
            .signing_keys(principal, &endpoint, at)
            .map_or_else(|error| refusal(&error), |value| encoded(200, &value)),
        ("POST", "keys") => match request.header("idempotency-key") {
            Some(idempotency) => config
                .service
                .rotate_key(principal, &endpoint, idempotency, at)
                .map_or_else(|error| refusal(&error), |value| encoded(201, &value)),
            None => Reply::refusal(400, "idempotency_key_required", None),
        },
        ("POST", "redeliveries") => config
            .service
            .redeliver(
                principal,
                &endpoint,
                request.parameter("cursor"),
                page(request),
                match request.header("idempotency-key") {
                    Some(value) => value,
                    None => return Reply::refusal(400, "idempotency_key_required", None),
                },
                at,
            )
            .map_or_else(|error| refusal(&error), |value| encoded(202, &value)),
        ("POST", "suspensions") => body::<SuspendBody>(request)
            .and_then(|body| {
                config
                    .service
                    .suspend(principal, &endpoint, &body.reason, at)
            })
            .map_or_else(
                |error| refusal(&error),
                |()| Reply::json(200, "{\"suspended\":true}".to_owned()),
            ),
        ("POST", "resumptions") => config.service.resume(principal, &endpoint).map_or_else(
            |error| refusal(&error),
            |()| Reply::json(200, "{\"suspended\":false}".to_owned()),
        ),
        _ => Reply::refusal(404, "not_found", None),
    }
}

fn owned_route(config: &Config, request: &Request, principal: &Principal, at: u64) -> Reply {
    let segments = request.segments();
    match (request.method.as_str(), segments.as_slice()) {
        (_, ["v1", "webhooks", "endpoints"]) => endpoints(config, request, principal, at),
        (_, ["v1", "webhooks", "endpoints", endpoint, action]) => {
            endpoint_route(config, request, principal, endpoint, action, at)
        }
        ("GET", ["v1", "webhooks", "events"]) => config
            .service
            .snapshot(principal, at, page(request))
            .map_or_else(|error| refusal(&error), |value| encoded(200, &value.events)),
        ("GET", ["v1", "webhooks", "deliveries"]) => config
            .service
            .snapshot(principal, at, page(request))
            .map_or_else(
                |error| refusal(&error),
                |value| encoded(200, &value.deliveries),
            ),
        ("GET", ["v1", "webhooks", "dead-letters"]) => config
            .service
            .snapshot(principal, at, page(request))
            .map_or_else(
                |error| refusal(&error),
                |value| encoded(200, &value.dead_letters),
            ),
        ("POST", ["v1", "webhooks", "dead-letters", delivery, "replay"]) => {
            let idempotency = match request.header("idempotency-key") {
                Some(value) => value,
                None => return Reply::refusal(400, "idempotency_key_required", None),
            };
            DeliveryId::new(*delivery)
                .and_then(|delivery| {
                    config
                        .service
                        .replay_dead_letter(principal, &delivery, idempotency, at)
                })
                .map_or_else(|error| refusal(&error), |value| encoded(202, &value))
        }
        _ => Reply::refusal(404, "not_found", None),
    }
}

fn internal_route(config: &Config, request: &Request, at: u64) -> Reply {
    let segments = request.segments();
    match (request.method.as_str(), segments.as_slice()) {
        ("POST", ["internal", "v1", "events", kind, source_event]) => {
            if !config
                .source_trigger
                .authorizes(request.header("authorization"))
            {
                return Reply::refusal(401, "source_authentication_required", None);
            }
            EventKind::parse(kind)
                .and_then(|kind| config.sources.fetch(kind, source_event))
                .and_then(|event| config.service.publish(&event, at))
                .map_or_else(|error| refusal(&error), |value| encoded(202, &value))
        }
        ("POST", ["internal", "v1", "dispatch"]) => {
            if !config
                .operator_trigger
                .authorizes(request.header("authorization"))
            {
                return Reply::refusal(401, "operator_authentication_required", None);
            }
            config
                .service
                .dispatch(at, config.dispatch_budget)
                .map_or_else(|error| refusal(&error), |value| encoded(200, &value))
        }
        _ => Reply::refusal(404, "not_found", None),
    }
}

fn route(config: &Config, request: &Request) -> Reply {
    let at = now();
    if request.method == "GET" && request.path == "/healthz" {
        let delivery = config.service.ready();
        let sources = config.sources.ready();
        return encoded(
            if delivery && sources { 200 } else { 503 },
            &serde_json::json!({
                "ready": delivery && sources,
                "components": {
                    "delivery_state_and_signer": delivery,
                    "canonical_sources_and_receipt_authority": sources
                }
            }),
        );
    }
    if request.method == "GET" && request.path == "/v1/webhooks/scheme" {
        return encoded(
            200,
            &SchemeDocument {
                scheme: scheme::SCHEME_VERSION,
                algorithm: "ed25519",
                signed_message: "<event-id>.<timestamp>. followed by exact body bytes",
                receiver_obligation: scheme::RECEIVER_OBLIGATION,
            },
        );
    }
    if request.path.starts_with("/internal/") {
        return internal_route(config, request, at);
    }
    match principal(config, request) {
        Ok(principal) => owned_route(config, request, &principal, at),
        Err(_) => Reply::refusal(401, "session_required", None),
    }
}

fn serve(config: Arc<Config>) -> Result<(), String> {
    if config.dispatch_interval.is_zero() {
        return Err("webhook dispatch interval must be positive".to_owned());
    }
    let worker_config = Arc::clone(&config);
    thread::spawn(move || loop {
        thread::sleep(worker_config.dispatch_interval);
        let _ = worker_config
            .service
            .dispatch(now(), worker_config.dispatch_budget);
        let _ = worker_config
            .service
            .prune_all(worker_config.retention_events);
    });
    let listener = TcpListener::bind(config.listen).map_err(|error| error.to_string())?;
    for accepted in listener.incoming() {
        let Ok(tcp) = accepted else {
            continue;
        };
        if ACTIVE_CONNECTIONS.fetch_add(1, Ordering::AcqRel) >= MAX_CONNECTIONS {
            ACTIVE_CONNECTIONS.fetch_sub(1, Ordering::AcqRel);
            continue;
        }
        let request_config = Arc::clone(&config);
        thread::spawn(move || {
            let _guard = ConnectionGuard;
            let _ = handle(tcp, &request_config);
        });
    }
    Ok(())
}

struct ConnectionGuard;

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        ACTIVE_CONNECTIONS.fetch_sub(1, Ordering::AcqRel);
    }
}

fn handle(tcp: TcpStream, config: &Config) -> Result<(), String> {
    tcp.set_read_timeout(Some(Duration::from_secs(15)))
        .map_err(|error| error.to_string())?;
    tcp.set_write_timeout(Some(Duration::from_secs(15)))
        .map_err(|error| error.to_string())?;
    let connection =
        ServerConnection::new(Arc::clone(&config.tls)).map_err(|error| error.to_string())?;
    let mut stream = StreamOwned::new(connection, tcp);
    let reply = http::read_request(&mut stream).map_or_else(
        |_| Reply::refusal(400, "invalid_request", None),
        |request| route(config, &request),
    );
    http::write_reply(&mut stream, &reply).map_err(|error| error.to_string())
}

fn main() {
    if let Err(error) = config().and_then(|config| serve(Arc::new(config))) {
        eprintln!("layerx-webhooks: {error}");
        std::process::exit(2);
    }
}

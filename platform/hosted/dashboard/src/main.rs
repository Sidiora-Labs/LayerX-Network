use layerx_platform_dashboard::error::DashboardError;
use layerx_platform_dashboard::service::Dashboard;
use layerx_platform_webhooks::events::{EndpointId, Principal};
use layerx_platform_webhooks::http::{self, Reply, Request};
use layerx_platform_webhooks::trusted::DeveloperIdentity;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use serde::Serialize;
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
    dashboard: Arc<Dashboard>,
    identity: Arc<DeveloperIdentity>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

fn tls_config() -> Result<Arc<ServerConfig>, String> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| "failed to install TLS provider".to_owned())?;
    let cert = CertificateDer::from(
        fs::read(
            env::var("LAYERX_DASHBOARD_TLS_CERT_DER")
                .map_err(|_| "dashboard TLS certificate is required")?,
        )
        .map_err(|error| error.to_string())?,
    );
    let key = PrivateKeyDer::from(PrivatePkcs8KeyDer::from(
        fs::read(
            env::var("LAYERX_DASHBOARD_TLS_KEY_DER")
                .map_err(|_| "dashboard TLS key is required")?,
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
        listen: env::var("LAYERX_DASHBOARD_LISTEN")
            .unwrap_or_else(|_| "0.0.0.0:9445".to_owned())
            .parse::<SocketAddr>()
            .map_err(|_| "dashboard listen address is invalid".to_owned())?,
        tls: tls_config()?,
        dashboard: Arc::new(Dashboard::from_environment()?),
        identity: Arc::new(DeveloperIdentity::from_dashboard_environment()?),
    })
}

fn refusal(error: &DashboardError) -> Reply {
    match error {
        DashboardError::InvalidRequest => Reply::refusal(400, "invalid_request", None),
        DashboardError::UnknownReceipt => Reply::refusal(404, "receipt_not_found", None),
        DashboardError::Gateway(_) => Reply::refusal(403, "principal_refused", None),
        DashboardError::UnknownRoot
        | DashboardError::CorruptStore
        | DashboardError::Webhooks(_)
        | DashboardError::Io(_) => Reply::refusal(503, "state_unavailable", Some(5)),
    }
}

fn encoded<T: Serialize>(value: &T) -> Reply {
    serde_json::to_string(value).map_or_else(
        |_| Reply::refusal(503, "encoding_failed", Some(5)),
        |body| Reply::json(200, body),
    )
}

fn page(request: &Request) -> usize {
    request
        .parameter("limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(DEFAULT_PAGE)
        .clamp(1, 200)
}

fn principal(config: &Config, request: &Request) -> Result<Principal, DashboardError> {
    Ok(config.identity.authenticate(
        request.header("authorization"),
        request.header("cookie"),
        None,
        false,
    )?)
}

fn owned_route(config: &Config, request: &Request, principal: &Principal, at: u64) -> Reply {
    let segments = request.segments();
    if request.method != "GET" {
        return Reply::refusal(404, "not_found", None);
    }
    let result = match segments.as_slice() {
        ["v1", "dashboard", "overview"] => config
            .dashboard
            .overview(principal, at)
            .map(|value| serde_json::to_value(value)),
        ["v1", "dashboard", "keys"] => config
            .dashboard
            .keys(principal, at)
            .map(|value| serde_json::to_value(value)),
        ["v1", "dashboard", "usage"] => config
            .dashboard
            .usage(principal, at)
            .map(|value| serde_json::to_value(value)),
        ["v1", "dashboard", "requests"] => config
            .dashboard
            .requests(principal, page(request))
            .map(|value| serde_json::to_value(value)),
        ["v1", "dashboard", "webhooks"] => config
            .dashboard
            .endpoints(principal, at)
            .map(|value| serde_json::to_value(value)),
        ["v1", "dashboard", "webhook-deliveries"] => {
            let endpoint = request
                .parameter("endpoint")
                .map(EndpointId::new)
                .transpose();
            match endpoint {
                Ok(endpoint) => config
                    .dashboard
                    .deliveries(principal, endpoint.as_ref(), page(request), at)
                    .map(|value| serde_json::to_value(value)),
                Err(error) => Err(DashboardError::from(error)),
            }
        }
        ["v1", "dashboard", "webhook-dead-letters"] => config
            .dashboard
            .dead_letters(principal, page(request), at)
            .map(|value| serde_json::to_value(value)),
        ["v1", "dashboard", "test-payments"] => config
            .dashboard
            .payments(principal, page(request), at)
            .map(|value| serde_json::to_value(value)),
        ["v1", "dashboard", "receipts", activity] => config
            .dashboard
            .receipt(principal, activity, at)
            .map(|value| serde_json::to_value(value)),
        _ => return Reply::refusal(404, "not_found", None),
    };
    result
        .and_then(|value| value.map_err(|_| DashboardError::CorruptStore))
        .map_or_else(|error| refusal(&error), |value| encoded(&value))
}

fn route(config: &Config, request: &Request) -> Reply {
    if request.method == "GET" && request.path == "/healthz" {
        let ready = config.dashboard.ready();
        return Reply::json(
            if ready { 200 } else { 503 },
            serde_json::json!({ "ready": ready }).to_string(),
        );
    }
    match principal(config, request) {
        Ok(principal) => owned_route(config, request, &principal, now()),
        Err(_) => Reply::refusal(401, "session_required", None),
    }
}

fn serve(config: Arc<Config>) -> Result<(), String> {
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
        eprintln!("layerx-dashboard: {error}");
        std::process::exit(2);
    }
}

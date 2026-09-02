use layerx_platform_testnet::{
    platform_testnet, PendingRelease, LXP_WIRE_PROTOCOL_VERSION, TESTNET_NETWORK_ID,
};
use native_tls::{Certificate, TlsConnector, TlsStream};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, Zeroizing};

const MAX_MESSAGE: usize = 64 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const IO_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_CONNECTIONS: usize = 128;
static ACTIVE_CONNECTIONS: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone)]
struct Endpoint {
    host: String,
    port: u16,
    path: String,
}

impl Endpoint {
    fn parse(value: &str) -> Result<Self, String> {
        Self::parse_scheme(value, "https", 443)
    }

    fn parse_redis(value: &str) -> Result<Self, String> {
        let endpoint = Self::parse_scheme(value, "rediss", 6379)?;
        if !endpoint.path.is_empty() {
            return Err("Redis endpoint must not carry a path".to_owned());
        }
        Ok(endpoint)
    }

    fn parse_scheme(value: &str, scheme: &str, default_port: u16) -> Result<Self, String> {
        let rest = value.strip_prefix(&format!("{scheme}://")).ok_or_else(|| {
            format!(
                "component endpoint must use {}",
                scheme.to_ascii_uppercase()
            )
        })?;
        let (authority, tail) = rest.split_once('/').unwrap_or((rest, ""));
        if authority.is_empty()
            || authority.contains(['@', '?', '#', '\\'])
            || tail.contains(['?', '#', '\\'])
        {
            return Err("component endpoint is not canonical".to_owned());
        }
        let (host, port) = authority.rsplit_once(':').map_or_else(
            || Ok::<_, String>((authority.to_owned(), default_port)),
            |(host, port)| {
                Ok((
                    host.to_owned(),
                    port.parse::<u16>()
                        .map_err(|_| "component endpoint port is invalid".to_owned())?,
                ))
            },
        )?;
        if host.is_empty() {
            return Err("component endpoint host is missing".to_owned());
        }
        Ok(Self {
            host,
            port,
            path: if tail.is_empty() {
                String::new()
            } else {
                format!("/{tail}")
            },
        })
    }

    fn with_path(&self, path: &str) -> Self {
        Self {
            host: self.host.clone(),
            port: self.port,
            path: format!("{}{path}", self.path),
        }
    }

    fn authority(&self) -> String {
        if self.port == 443 {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

struct Config {
    public_listen: SocketAddr,
    admin_listen: SocketAddr,
    tls: Arc<ServerConfig>,
    outbound_ca: Certificate,
    release: ReleaseGate,
    identity: Endpoint,
    faucet: Endpoint,
    core: Endpoint,
    core_admin: Endpoint,
    receipt_authority: Endpoint,
    registry: Endpoint,
    redis: Endpoint,
    gateway: Endpoint,
    paxeer: Endpoint,
    backend_admin_token: Zeroizing<String>,
    inbound_admin_token: Zeroizing<String>,
}

struct ReleaseGate {
    package_semver: String,
    pending_package_semver: String,
    pending_wire_version: u16,
}

struct ReleaseCompatible;

impl ReleaseGate {
    fn verify(&self) -> Result<ReleaseCompatible, String> {
        if self.package_semver != self.pending_package_semver {
            return Err(format!(
                "package release {} does not match the pending release {}",
                self.package_semver, self.pending_package_semver
            ));
        }
        if self.pending_wire_version != LXP_WIRE_PROTOCOL_VERSION {
            return Err(format!(
                "LXP wire protocol version {LXP_WIRE_PROTOCOL_VERSION} does not match the pending version {}",
                self.pending_wire_version
            ));
        }
        Ok(ReleaseCompatible)
    }

    fn view(&self) -> ReleaseView {
        let (state, detail) = match self.verify() {
            Ok(ReleaseCompatible) => ("ready", "pending release matches".to_owned()),
            Err(detail) => ("degraded", detail),
        };
        ReleaseView {
            state,
            detail,
            package_semver: self.package_semver.clone(),
            pending_package_semver: self.pending_package_semver.clone(),
            lxp_wire_protocol_version: LXP_WIRE_PROTOCOL_VERSION,
            pending_lxp_wire_protocol_version: self.pending_wire_version,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Dependency {
    Identity,
    Faucet,
    Core,
    CoreAdmin,
    ReceiptAuthority,
    Registry,
    Redis,
    Gateway,
    Paxeer,
}

impl Dependency {
    const ALL: [Self; 9] = [
        Self::Identity,
        Self::Faucet,
        Self::Core,
        Self::CoreAdmin,
        Self::ReceiptAuthority,
        Self::Registry,
        Self::Redis,
        Self::Gateway,
        Self::Paxeer,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Identity => "identity",
            Self::Faucet => "faucet",
            Self::Core => "core",
            Self::CoreAdmin => "core_admin",
            Self::ReceiptAuthority => "receipt_authority",
            Self::Registry => "registry",
            Self::Redis => "redis",
            Self::Gateway => "gateway",
            Self::Paxeer => "paxeer",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Journey {
    Funding,
    Payment,
    ReceiptInspection,
    Programs,
}

impl Journey {
    const ALL: [Self; 4] = [
        Self::Funding,
        Self::Payment,
        Self::ReceiptInspection,
        Self::Programs,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Funding => "funding",
            Self::Payment => "payment",
            Self::ReceiptInspection => "receipt_inspection",
            Self::Programs => "programs",
        }
    }

    fn route(self) -> &'static str {
        match self {
            Self::Funding => "/v1/journeys/funding",
            Self::Payment => "/v1/journeys/payment",
            Self::ReceiptInspection => "/v1/journeys/receipt-inspection",
            Self::Programs => "/v1/journeys/programs",
        }
    }

    fn from_route(path: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|journey| journey.route() == path)
    }

    fn dependencies(self) -> &'static [Dependency] {
        match self {
            Self::Funding => &[
                Dependency::Identity,
                Dependency::Faucet,
                Dependency::Redis,
                Dependency::CoreAdmin,
                Dependency::Core,
            ],
            Self::Payment => &[
                Dependency::Identity,
                Dependency::Gateway,
                Dependency::Core,
                Dependency::ReceiptAuthority,
            ],
            Self::ReceiptInspection => &[
                Dependency::Gateway,
                Dependency::ReceiptAuthority,
                Dependency::Core,
            ],
            Self::Programs => &[
                Dependency::Gateway,
                Dependency::Registry,
                Dependency::Core,
                Dependency::ReceiptAuthority,
            ],
        }
    }
}

#[derive(Clone)]
struct DependencyReport {
    dependency: Dependency,
    outcome: Result<String, String>,
}

#[derive(Clone)]
struct ReadyDependency {
    name: &'static str,
    detail: String,
}

#[derive(Clone)]
struct FailedDependency {
    name: &'static str,
    detail: String,
}

impl DependencyReport {
    fn classify(&self) -> Result<ReadyDependency, FailedDependency> {
        match &self.outcome {
            Ok(detail) => Ok(ReadyDependency {
                name: self.dependency.name(),
                detail: detail.clone(),
            }),
            Err(detail) => Err(FailedDependency {
                name: self.dependency.name(),
                detail: detail.clone(),
            }),
        }
    }
}

impl ReadyDependency {
    fn view(&self) -> DependencyView {
        DependencyView {
            name: self.name,
            ready: true,
            detail: self.detail.clone(),
        }
    }
}

impl FailedDependency {
    fn view(&self) -> DependencyView {
        DependencyView {
            name: self.name,
            ready: false,
            detail: self.detail.clone(),
        }
    }
}

fn dependency_view(dependency: &Result<ReadyDependency, FailedDependency>) -> DependencyView {
    match dependency {
        Ok(ready) => ready.view(),
        Err(failed) => failed.view(),
    }
}

struct DependencyReports {
    reports: [DependencyReport; 9],
}

impl DependencyReports {
    #[cfg(test)]
    fn new(outcome: impl Fn(Dependency) -> Result<String, String>) -> Self {
        Self {
            reports: Dependency::ALL.map(|dependency| DependencyReport {
                dependency,
                outcome: outcome(dependency),
            }),
        }
    }

    fn probe(config: &Config) -> Self {
        let mut probed = probe_dependencies(config, &Dependency::ALL).into_iter();
        Self {
            reports: Dependency::ALL.map(|dependency| {
                probed
                    .next()
                    .filter(|report| report.dependency == dependency)
                    .unwrap_or_else(|| DependencyReport {
                        dependency,
                        outcome: Err("dependency was not probed".to_owned()),
                    })
            }),
        }
    }

    fn report(&self, dependency: Dependency) -> &DependencyReport {
        &self.reports[dependency as usize]
    }

    fn classify(&self) -> Vec<Result<ReadyDependency, FailedDependency>> {
        self.reports
            .iter()
            .map(DependencyReport::classify)
            .collect()
    }
}

struct ReadyJourney {
    journey: Journey,
    dependencies: Vec<ReadyDependency>,
}

struct DegradedJourney {
    journey: Journey,
    failing: FailedDependency,
    dependencies: Vec<Result<ReadyDependency, FailedDependency>>,
}

enum JourneyReadiness {
    Ready(ReadyJourney),
    Degraded(DegradedJourney),
}

impl ReadyJourney {
    fn view(&self) -> JourneyView {
        JourneyView {
            journey: self.journey.name(),
            ready: true,
            dependencies: self
                .dependencies
                .iter()
                .map(ReadyDependency::view)
                .collect(),
            failing: Vec::new(),
        }
    }
}

impl DegradedJourney {
    fn failing_names(&self) -> Vec<&'static str> {
        std::iter::once(self.failing.name)
            .chain(
                self.dependencies
                    .iter()
                    .filter_map(|dependency| dependency.as_ref().err())
                    .map(|failed| failed.name)
                    .filter(|name| *name != self.failing.name),
            )
            .collect()
    }

    fn view(&self) -> JourneyView {
        JourneyView {
            journey: self.journey.name(),
            ready: false,
            dependencies: self.dependencies.iter().map(dependency_view).collect(),
            failing: self.failing_names(),
        }
    }

    fn refusal(&self) -> serde_json::Value {
        serde_json::json!({
            "error": {
                "code": "journey_degraded",
                "journey": self.journey.name(),
                "failing": self.failing_names(),
                "retry": "after"
            }
        })
    }
}

impl JourneyReadiness {
    fn compute(journey: Journey, reports: &DependencyReports) -> Self {
        Self::from_reports(
            journey,
            &journey
                .dependencies()
                .iter()
                .map(|dependency| reports.report(*dependency).clone())
                .collect::<Vec<_>>(),
        )
    }

    fn probe(config: &Config, journey: Journey) -> Self {
        Self::from_reports(journey, &probe_dependencies(config, journey.dependencies()))
    }

    fn from_reports(journey: Journey, reports: &[DependencyReport]) -> Self {
        let dependencies: Vec<Result<ReadyDependency, FailedDependency>> =
            reports.iter().map(DependencyReport::classify).collect();
        match dependencies
            .iter()
            .cloned()
            .collect::<Result<Vec<ReadyDependency>, FailedDependency>>()
        {
            Ok(ready) => Self::Ready(ReadyJourney {
                journey,
                dependencies: ready,
            }),
            Err(failing) => Self::Degraded(DegradedJourney {
                journey,
                failing,
                dependencies,
            }),
        }
    }

    fn is_ready(&self) -> bool {
        matches!(self, Self::Ready(_))
    }

    fn view(&self) -> JourneyView {
        match self {
            Self::Ready(ready) => ready.view(),
            Self::Degraded(degraded) => degraded.view(),
        }
    }

    fn admission(&self) -> JourneyAdmission {
        JourneyAdmission {
            admitted: self.is_ready(),
            readiness: self.view(),
        }
    }
}

struct ReadyJourneys {
    funding: ReadyJourney,
    payment: ReadyJourney,
    receipt_inspection: ReadyJourney,
    programs: ReadyJourney,
}

enum HostedReadiness {
    Ready {
        journeys: ReadyJourneys,
        dependencies: Vec<ReadyDependency>,
        release: ReleaseCompatible,
    },
    Degraded {
        journeys: Vec<JourneyReadiness>,
        dependencies: Vec<Result<ReadyDependency, FailedDependency>>,
        release: Result<ReleaseCompatible, String>,
    },
}

impl HostedReadiness {
    fn compute(config: &Config) -> Self {
        Self::assemble(&DependencyReports::probe(config), &config.release)
    }

    fn assemble(reports: &DependencyReports, release: &ReleaseGate) -> Self {
        let [funding, payment, receipt_inspection, programs] =
            Journey::ALL.map(|journey| JourneyReadiness::compute(journey, reports));
        let dependencies = reports.classify();
        let every_dependency: Result<Vec<ReadyDependency>, FailedDependency> =
            dependencies.iter().cloned().collect();
        match (
            funding,
            payment,
            receipt_inspection,
            programs,
            every_dependency,
            release.verify(),
        ) {
            (
                JourneyReadiness::Ready(funding),
                JourneyReadiness::Ready(payment),
                JourneyReadiness::Ready(receipt_inspection),
                JourneyReadiness::Ready(programs),
                Ok(dependencies),
                Ok(release),
            ) => Self::Ready {
                journeys: ReadyJourneys {
                    funding,
                    payment,
                    receipt_inspection,
                    programs,
                },
                dependencies,
                release,
            },
            (funding, payment, receipt_inspection, programs, _, release) => Self::Degraded {
                journeys: vec![funding, payment, receipt_inspection, programs],
                dependencies,
                release,
            },
        }
    }

    fn is_ready(&self) -> bool {
        matches!(self, Self::Ready { .. })
    }

    fn state(&self) -> &'static str {
        if self.is_ready() {
            "ready"
        } else {
            "degraded"
        }
    }

    fn release_ready(&self) -> bool {
        match self {
            Self::Ready {
                release: ReleaseCompatible,
                ..
            } => true,
            Self::Degraded { release, .. } => release.is_ok(),
        }
    }

    fn dependency_ready(&self, dependency: Dependency) -> bool {
        match self {
            Self::Ready { .. } => true,
            Self::Degraded { dependencies, .. } => dependencies
                .iter()
                .any(|entry| matches!(entry, Ok(ready) if ready.name == dependency.name())),
        }
    }

    fn dependency_views(&self) -> Vec<DependencyView> {
        match self {
            Self::Ready { dependencies, .. } => {
                dependencies.iter().map(ReadyDependency::view).collect()
            }
            Self::Degraded { dependencies, .. } => {
                dependencies.iter().map(dependency_view).collect()
            }
        }
    }

    fn journey_views(&self) -> Vec<JourneyView> {
        match self {
            Self::Ready { journeys, .. } => vec![
                journeys.funding.view(),
                journeys.payment.view(),
                journeys.receipt_inspection.view(),
                journeys.programs.view(),
            ],
            Self::Degraded { journeys, .. } => {
                journeys.iter().map(JourneyReadiness::view).collect()
            }
        }
    }

    fn document<'a>(&self, release: &'a ReleaseGate) -> ReadinessDocument<'a> {
        ReadinessDocument {
            service: "layerx-hosted-testnet",
            state: self.state(),
            package_semver: &release.package_semver,
            lxp_wire_protocol_version: LXP_WIRE_PROTOCOL_VERSION,
            network_id: TESTNET_NETWORK_ID,
            release: release.view(),
            dependencies: self.dependency_views(),
            journeys: self.journey_views(),
        }
    }

    fn public_status<'a>(&self, release: &'a ReleaseGate) -> PublicStatus<'a> {
        let component = |dependency: Dependency| ComponentStatus {
            name: dependency.name(),
            state: if self.dependency_ready(dependency) {
                "ready"
            } else {
                "unavailable"
            },
        };
        PublicStatus {
            service: "layerx-hosted-testnet",
            state: self.state(),
            package_semver: &release.package_semver,
            lxp_wire_protocol_version: LXP_WIRE_PROTOCOL_VERSION,
            network_id: TESTNET_NETWORK_ID,
            components: vec![
                ComponentStatus {
                    name: "testnet",
                    state: if self.release_ready() {
                        "ready"
                    } else {
                        "degraded"
                    },
                },
                component(Dependency::Gateway),
                component(Dependency::Core),
                component(Dependency::Paxeer),
            ],
        }
    }
}

struct Request {
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

impl Drop for Request {
    fn drop(&mut self) {
        for value in self.headers.values_mut() {
            value.zeroize();
        }
        self.body.zeroize();
    }
}

struct Response {
    status: u16,
    body: Vec<u8>,
}

#[derive(Serialize)]
struct ComponentStatus {
    name: &'static str,
    state: &'static str,
}

#[derive(Serialize)]
struct PublicStatus<'a> {
    service: &'static str,
    state: &'static str,
    package_semver: &'a str,
    lxp_wire_protocol_version: u16,
    network_id: u32,
    components: Vec<ComponentStatus>,
}

#[derive(Serialize)]
struct DependencyView {
    name: &'static str,
    ready: bool,
    detail: String,
}

#[derive(Serialize)]
struct JourneyView {
    journey: &'static str,
    ready: bool,
    dependencies: Vec<DependencyView>,
    failing: Vec<&'static str>,
}

#[derive(Serialize)]
struct JourneyAdmission {
    admitted: bool,
    #[serde(flatten)]
    readiness: JourneyView,
}

#[derive(Serialize)]
struct ReleaseView {
    state: &'static str,
    detail: String,
    package_semver: String,
    pending_package_semver: String,
    lxp_wire_protocol_version: u16,
    pending_lxp_wire_protocol_version: u16,
}

#[derive(Serialize)]
struct ReadinessDocument<'a> {
    service: &'static str,
    state: &'static str,
    package_semver: &'a str,
    lxp_wire_protocol_version: u16,
    network_id: u32,
    release: ReleaseView,
    dependencies: Vec<DependencyView>,
    journeys: Vec<JourneyView>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FundingCommand {
    funding_id: String,
    did: String,
    public_key: String,
    amount: u64,
}

fn read_secret(variable: &str) -> Result<Zeroizing<String>, String> {
    let path = env::var(variable).map_err(|_| format!("{variable} is required"))?;
    let mut secret = fs::read_to_string(path).map_err(|error| error.to_string())?;
    while matches!(secret.as_bytes().last(), Some(b'\n' | b'\r')) {
        secret.pop();
    }
    if secret.is_empty() || secret.len() > 4096 {
        secret.zeroize();
        return Err(format!("{variable} is empty or exceeds its bound"));
    }
    Ok(Zeroizing::new(secret))
}

fn tls_config() -> Result<Arc<ServerConfig>, String> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| "failed to install TLS crypto provider".to_owned())?;
    let cert = fs::read(
        env::var("LAYERX_TESTNET_TLS_CERT_DER")
            .map_err(|_| "LAYERX_TESTNET_TLS_CERT_DER is required")?,
    )
    .map_err(|error| error.to_string())?;
    let key = fs::read(
        env::var("LAYERX_TESTNET_TLS_KEY_DER")
            .map_err(|_| "LAYERX_TESTNET_TLS_KEY_DER is required")?,
    )
    .map_err(|error| error.to_string())?;
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(cert)],
            PrivateKeyDer::from(PrivatePkcs8KeyDer::from(key)),
        )
        .map_err(|error| error.to_string())?;
    Ok(Arc::new(config))
}

fn config() -> Result<Config, String> {
    let testnet = platform_testnet();
    let pending_package_semver = env::var("LAYERX_PENDING_PACKAGE_SEMVER")
        .map_err(|_| "LAYERX_PENDING_PACKAGE_SEMVER is required")?;
    let pending_wire_version = env::var("LAYERX_PENDING_WIRE_PROTOCOL_VERSION")
        .map_err(|_| "LAYERX_PENDING_WIRE_PROTOCOL_VERSION is required")?
        .parse::<u16>()
        .map_err(|_| "pending wire protocol version is invalid".to_owned())?;
    testnet
        .validate(&PendingRelease {
            package_semver: pending_package_semver.clone(),
            wire_protocol_version: pending_wire_version,
        })
        .map_err(str::to_owned)?;
    Ok(Config {
        public_listen: env::var("LAYERX_TESTNET_PUBLIC_LISTEN")
            .unwrap_or_else(|_| "0.0.0.0:9443".to_owned())
            .parse::<SocketAddr>()
            .map_err(|_| "public listen address is invalid".to_owned())?,
        admin_listen: env::var("LAYERX_TESTNET_ADMIN_LISTEN")
            .unwrap_or_else(|_| "0.0.0.0:9444".to_owned())
            .parse::<SocketAddr>()
            .map_err(|_| "admin listen address is invalid".to_owned())?,
        tls: tls_config()?,
        outbound_ca: Certificate::from_der(
            &fs::read(
                env::var("LAYERX_OUTBOUND_CA_DER")
                    .map_err(|_| "LAYERX_OUTBOUND_CA_DER is required")?,
            )
            .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?,
        release: ReleaseGate {
            package_semver: testnet.package_semver,
            pending_package_semver,
            pending_wire_version,
        },
        identity: Endpoint::parse(
            &env::var("LAYERX_TESTNET_IDENTITY_URL")
                .map_err(|_| "LAYERX_TESTNET_IDENTITY_URL is required")?,
        )?,
        faucet: Endpoint::parse(
            &env::var("LAYERX_TESTNET_FAUCET_URL")
                .map_err(|_| "LAYERX_TESTNET_FAUCET_URL is required")?,
        )?,
        core: Endpoint::parse(
            &env::var("LAYERX_TESTNET_CORE_URL")
                .map_err(|_| "LAYERX_TESTNET_CORE_URL is required")?,
        )?,
        core_admin: Endpoint::parse(
            &env::var("LAYERX_TESTNET_CORE_ADMIN_URL")
                .map_err(|_| "LAYERX_TESTNET_CORE_ADMIN_URL is required")?,
        )?,
        receipt_authority: Endpoint::parse(
            &env::var("LAYERX_TESTNET_RECEIPT_AUTHORITY_URL")
                .map_err(|_| "LAYERX_TESTNET_RECEIPT_AUTHORITY_URL is required")?,
        )?,
        registry: Endpoint::parse(
            &env::var("LAYERX_TESTNET_REGISTRY_URL")
                .map_err(|_| "LAYERX_TESTNET_REGISTRY_URL is required")?,
        )?,
        redis: Endpoint::parse_redis(
            &env::var("LAYERX_TESTNET_REDIS_URL")
                .map_err(|_| "LAYERX_TESTNET_REDIS_URL is required")?,
        )?,
        gateway: Endpoint::parse(
            &env::var("LAYERX_TESTNET_GATEWAY_URL")
                .map_err(|_| "LAYERX_TESTNET_GATEWAY_URL is required")?,
        )?,
        paxeer: Endpoint::parse(
            &env::var("LAYERX_TESTNET_PAXEER_URL")
                .map_err(|_| "LAYERX_TESTNET_PAXEER_URL is required")?,
        )?,
        backend_admin_token: read_secret("LAYERX_TESTNET_BACKEND_ADMIN_TOKEN_FILE")?,
        inbound_admin_token: read_secret("LAYERX_TESTNET_CONTROL_ADMIN_TOKEN_FILE")?,
    })
}

fn connect(endpoint: &Endpoint) -> Result<TcpStream, String> {
    let mut last = None;
    for address in (endpoint.host.as_str(), endpoint.port)
        .to_socket_addrs()
        .map_err(|error| error.to_string())?
        .take(8)
    {
        match TcpStream::connect_timeout(&address, CONNECT_TIMEOUT) {
            Ok(stream) => {
                stream
                    .set_read_timeout(Some(IO_TIMEOUT))
                    .map_err(|error| error.to_string())?;
                stream
                    .set_write_timeout(Some(IO_TIMEOUT))
                    .map_err(|error| error.to_string())?;
                return Ok(stream);
            }
            Err(error) => last = Some(error),
        }
    }
    Err(last.map_or_else(
        || "component did not resolve".to_owned(),
        |error| error.to_string(),
    ))
}

fn tls_stream(ca: &Certificate, endpoint: &Endpoint) -> Result<TlsStream<TcpStream>, String> {
    let connector = TlsConnector::builder()
        .add_root_certificate(ca.clone())
        .min_protocol_version(Some(native_tls::Protocol::Tlsv12))
        .build()
        .map_err(|error| error.to_string())?;
    let tcp = connect(endpoint)?;
    connector
        .connect(&endpoint.host, tcp)
        .map_err(|error| error.to_string())
}

fn upstream(
    ca: &Certificate,
    endpoint: &Endpoint,
    method: &str,
    bearer: Option<&str>,
    idempotency: Option<&str>,
    body: &[u8],
) -> Result<Response, String> {
    let mut stream = tls_stream(ca, endpoint)?;
    let authorization = bearer.map_or(String::new(), |token| {
        format!("Authorization: Bearer {token}\r\n")
    });
    let idempotency =
        idempotency.map_or(String::new(), |key| format!("Idempotency-Key: {key}\r\n"));
    write!(
        stream,
        "{method} {} HTTP/1.1\r\nHost: {}\r\nAccept: application/json\r\nContent-Type: application/json\r\n{authorization}{idempotency}Content-Length: {}\r\nConnection: close\r\n\r\n",
        endpoint.path,
        endpoint.authority(),
        body.len()
    )
    .map_err(|error| error.to_string())?;
    stream.write_all(body).map_err(|error| error.to_string())?;
    stream.flush().map_err(|error| error.to_string())?;
    let mut message = read_message(&mut stream)?;
    let start = message
        .headers
        .get("")
        .ok_or_else(|| "component response has no status".to_owned())?;
    let mut parts = start.split_whitespace();
    if parts.next() != Some("HTTP/1.1") {
        return Err("component response is not HTTP/1.1".to_owned());
    }
    let status = parts
        .next()
        .ok_or_else(|| "component status is missing".to_owned())?
        .parse::<u16>()
        .map_err(|_| "component status is invalid".to_owned())?;
    Ok(Response {
        status,
        body: std::mem::take(&mut message.body),
    })
}

fn read_message(stream: &mut impl Read) -> Result<Request, String> {
    let mut bytes = Vec::with_capacity(2048);
    let mut chunk = [0_u8; 2048];
    let header_end = loop {
        let count = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if count == 0 || bytes.len().saturating_add(count) > MAX_MESSAGE {
            return Err("HTTP message is empty or exceeds its bound".to_owned());
        }
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
    };
    let source = std::str::from_utf8(&bytes[..header_end])
        .map_err(|_| "HTTP headers are not UTF-8".to_owned())?;
    let mut lines = source.split("\r\n");
    let start = lines
        .next()
        .ok_or_else(|| "HTTP start line is missing".to_owned())?
        .to_owned();
    let mut headers = BTreeMap::new();
    headers.insert(String::new(), start);
    let mut content_length = 0_usize;
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| "HTTP header is malformed".to_owned())?;
        let name = name.trim().to_ascii_lowercase();
        if headers.contains_key(&name) || name == "transfer-encoding" {
            return Err("duplicate or transfer-encoded header is rejected".to_owned());
        }
        let value = value.trim().to_owned();
        if name == "content-length" {
            content_length = value
                .parse::<usize>()
                .map_err(|_| "content length is invalid".to_owned())?;
        }
        headers.insert(name, value);
    }
    if header_end.saturating_add(content_length) > MAX_MESSAGE {
        return Err("HTTP body exceeds its bound".to_owned());
    }
    while bytes.len() < header_end + content_length {
        let count = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if count == 0 || bytes.len().saturating_add(count) > MAX_MESSAGE {
            return Err("HTTP body is truncated or exceeds its bound".to_owned());
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    Ok(Request {
        method: String::new(),
        path: String::new(),
        headers,
        body: bytes[header_end..header_end + content_length].to_vec(),
    })
}

fn client_request(stream: &mut impl Read) -> Result<Request, String> {
    let mut request = read_message(stream)?;
    let start = request
        .headers
        .remove("")
        .ok_or_else(|| "request line is missing".to_owned())?;
    let mut parts = start.split_whitespace();
    parts
        .next()
        .unwrap_or_default()
        .clone_into(&mut request.method);
    parts
        .next()
        .unwrap_or_default()
        .clone_into(&mut request.path);
    if parts.next() != Some("HTTP/1.1")
        || parts.next().is_some()
        || request.method.is_empty()
        || !request.path.starts_with('/')
        || request.path.contains('?')
    {
        return Err("request line is invalid".to_owned());
    }
    if !request.headers.contains_key("host") {
        return Err("HTTP/1.1 Host header is required".to_owned());
    }
    Ok(request)
}

fn probe(ca: &Certificate, endpoint: &Endpoint) -> Result<String, String> {
    match upstream(ca, &endpoint.with_path("/readyz"), "GET", None, None, &[]) {
        Ok(Response { status: 200, .. }) => Ok("GET /readyz answered 200".to_owned()),
        Ok(Response { status, .. }) => Err(format!("GET /readyz answered {status}")),
        Err(error) => Err(format!("GET /readyz failed: {error}")),
    }
}

fn probe_tls(ca: &Certificate, endpoint: &Endpoint) -> Result<String, String> {
    let mut stream =
        tls_stream(ca, endpoint).map_err(|error| format!("TLS handshake failed: {error}"))?;
    let _ = stream.shutdown();
    Ok("TLS handshake completed".to_owned())
}

fn probe_tcp(endpoint: &Endpoint) -> Result<String, String> {
    let stream = connect(endpoint).map_err(|error| format!("TCP connect failed: {error}"))?;
    let _ = stream.shutdown(std::net::Shutdown::Both);
    Ok(
        "TCP connection accepted; the registry requires client-certificate TLS beyond this probe"
            .to_owned(),
    )
}

fn probe_redis(ca: &Certificate, endpoint: &Endpoint) -> Result<String, String> {
    let mut stream =
        tls_stream(ca, endpoint).map_err(|error| format!("TLS handshake failed: {error}"))?;
    stream
        .write_all(b"*1\r\n$4\r\nPING\r\n")
        .and_then(|()| stream.flush())
        .map_err(|error| format!("PING could not be sent: {error}"))?;
    let mut line = Vec::with_capacity(64);
    let mut byte = [0_u8; 1];
    while !line.ends_with(b"\r\n") {
        let count = stream
            .read(&mut byte)
            .map_err(|error| format!("PING answer could not be read: {error}"))?;
        if count == 0 {
            return Err("Redis closed the connection before answering PING".to_owned());
        }
        if line.len() >= 256 {
            return Err("Redis answer exceeds its bound".to_owned());
        }
        line.push(byte[0]);
    }
    let _ = stream.shutdown();
    let answer = std::str::from_utf8(&line)
        .map_err(|_| "Redis answer is not UTF-8".to_owned())?
        .trim_end();
    let word = answer.split_whitespace().next().unwrap_or_default();
    if word == "+PONG" {
        Ok("PING answered PONG".to_owned())
    } else if word == "-NOAUTH" {
        Ok(
            "PING answered NOAUTH; the TLS listener is reachable and enforces ACL credentials"
                .to_owned(),
        )
    } else {
        Err(format!("PING answered {word}"))
    }
}

fn probe_dependency(config: &Config, dependency: Dependency) -> DependencyReport {
    let ca = &config.outbound_ca;
    let outcome = match dependency {
        Dependency::Identity => probe(ca, &config.identity),
        Dependency::Faucet => probe(ca, &config.faucet),
        Dependency::Core => probe(ca, &config.core),
        Dependency::CoreAdmin => probe_tls(ca, &config.core_admin),
        Dependency::ReceiptAuthority => probe(ca, &config.receipt_authority),
        Dependency::Registry => probe_tcp(&config.registry),
        Dependency::Redis => probe_redis(ca, &config.redis),
        Dependency::Gateway => probe(ca, &config.gateway),
        Dependency::Paxeer => probe(ca, &config.paxeer),
    };
    DependencyReport {
        dependency,
        outcome,
    }
}

fn probe_dependencies(config: &Config, dependencies: &[Dependency]) -> Vec<DependencyReport> {
    thread::scope(|scope| {
        let handles: Vec<_> = dependencies
            .iter()
            .map(|dependency| {
                let dependency = *dependency;
                (
                    dependency,
                    scope.spawn(move || probe_dependency(config, dependency)),
                )
            })
            .collect();
        handles
            .into_iter()
            .map(|(dependency, handle)| {
                handle.join().unwrap_or_else(|_| DependencyReport {
                    dependency,
                    outcome: Err("probe thread did not complete".to_owned()),
                })
            })
            .collect()
    })
}

fn status(config: &Config) -> HostedReadiness {
    HostedReadiness::compute(config)
}

fn public_route(config: &Config, request: &Request) -> Response {
    if request.method != "GET" {
        return json_response(404, serde_json::json!({ "error": { "code": "not_found" } }));
    }
    match request.path.as_str() {
        "/livez" => json_response(200, serde_json::json!({ "status": "live" })),
        "/readyz" => {
            let readiness = status(config);
            json_response(
                if readiness.is_ready() { 200 } else { 503 },
                readiness.document(&config.release),
            )
        }
        "/v1/status" => json_response(200, status(config).public_status(&config.release)),
        "/v1/parameters" => json_response(
            200,
            serde_json::json!({
                "network": "layerx-testnet",
                "network_id": TESTNET_NETWORK_ID,
                "package_semver": config.release.package_semver,
                "lxp_wire_protocol_version": LXP_WIRE_PROTOCOL_VERSION,
                "reset_schedule": "09:00 UTC on the first Tuesday of every month"
            }),
        ),
        path => match Journey::from_route(path) {
            Some(journey) => {
                let readiness = JourneyReadiness::probe(config, journey);
                json_response(
                    if readiness.is_ready() { 200 } else { 503 },
                    readiness.admission(),
                )
            }
            None => json_response(404, serde_json::json!({ "error": { "code": "not_found" } })),
        },
    }
}

fn admin_authorized(config: &Config, request: &Request) -> bool {
    let Some(token) = request
        .headers
        .get("authorization")
        .and_then(|value| value.strip_prefix("Bearer "))
    else {
        return false;
    };
    token.len() == config.inbound_admin_token.len()
        && token
            .as_bytes()
            .ct_eq(config.inbound_admin_token.as_bytes())
            .unwrap_u8()
            == 1
}

fn valid_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

fn admin_route(config: &Config, request: &Request) -> Response {
    if !admin_authorized(config, request) {
        return json_response(
            401,
            serde_json::json!({ "error": { "code": "unauthorized" } }),
        );
    }
    if request.headers.get("content-type").map(String::as_str) != Some("application/json") {
        return json_response(
            400,
            serde_json::json!({ "error": { "code": "content_type_required" } }),
        );
    }
    let Some(idempotency) = request.headers.get("idempotency-key") else {
        return json_response(
            400,
            serde_json::json!({ "error": { "code": "idempotency_key_required" } }),
        );
    };
    if !valid_key(idempotency) {
        return json_response(
            400,
            serde_json::json!({ "error": { "code": "invalid_idempotency_key" } }),
        );
    }
    let endpoint = match (request.method.as_str(), request.path.as_str()) {
        ("POST", "/admin/v1/testnet/fund") => {
            let Ok(command) = serde_json::from_slice::<FundingCommand>(&request.body) else {
                return json_response(
                    400,
                    serde_json::json!({ "error": { "code": "invalid_argument" } }),
                );
            };
            if !valid_key(&command.funding_id)
                || !command.did.starts_with("did:")
                || command.did.len() > 512
                || command.public_key.len() != 64
                || !command
                    .public_key
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
                || command.amount == 0
            {
                return json_response(
                    400,
                    serde_json::json!({ "error": { "code": "invalid_argument" } }),
                );
            }
            if let JourneyReadiness::Degraded(degraded) =
                JourneyReadiness::probe(config, Journey::Funding)
            {
                return json_response(503, degraded.refusal());
            }
            config.core_admin.with_path("/admin/v1/testnet/fund")
        }
        ("POST", "/admin/v1/testnet/reset") => {
            if request.body != b"{}" {
                return json_response(
                    400,
                    serde_json::json!({ "error": { "code": "invalid_argument" } }),
                );
            }
            config.core_admin.with_path("/admin/v1/testnet/reset")
        }
        _ => return json_response(404, serde_json::json!({ "error": { "code": "not_found" } })),
    };
    match upstream(
        &config.outbound_ca,
        &endpoint,
        "POST",
        Some(config.backend_admin_token.as_str()),
        Some(idempotency),
        &request.body,
    ) {
        Ok(response) if response.status == 200 || response.status == 202 => response,
        Ok(response) if (400..500).contains(&response.status) => response,
        _ => json_response(
            503,
            serde_json::json!({ "error": { "code": "core_unavailable", "retry": "after" } }),
        ),
    }
}

fn json_response(status: u16, value: impl Serialize) -> Response {
    match serde_json::to_vec(&value) {
        Ok(body) => Response { status, body },
        Err(_) => Response {
            status: 500,
            body: b"{\"error\":{\"code\":\"serialization_failure\"}}".to_vec(),
        },
    }
}

fn write_response(stream: &mut impl Write, response: &Response) -> Result<(), String> {
    let reason = match response.status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        _ => "Service Unavailable",
    };
    write!(
        stream,
        "HTTP/1.1 {} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        response.status,
        response.body.len()
    )
    .map_err(|error| error.to_string())?;
    stream
        .write_all(&response.body)
        .map_err(|error| error.to_string())
}

fn serve(listener: &TcpListener, config: &Arc<Config>, admin: bool) -> Result<(), String> {
    for connection in listener.incoming() {
        match connection {
            Ok(tcp) => {
                let Some(permit) = ConnectionPermit::acquire() else {
                    continue;
                };
                tcp.set_read_timeout(Some(IO_TIMEOUT))
                    .map_err(|e| e.to_string())?;
                tcp.set_write_timeout(Some(IO_TIMEOUT))
                    .map_err(|e| e.to_string())?;
                let shared = Arc::clone(config);
                thread::spawn(move || {
                    let _permit = permit;
                    let result = (|| -> Result<(), String> {
                        let connection = ServerConnection::new(Arc::clone(&shared.tls))
                            .map_err(|error| error.to_string())?;
                        let mut stream = StreamOwned::new(connection, tcp);
                        let response = client_request(&mut stream).map_or_else(
                            |_| {
                                json_response(
                                    400,
                                    serde_json::json!({ "error": { "code": "invalid_request" } }),
                                )
                            },
                            |request| {
                                if admin {
                                    admin_route(&shared, &request)
                                } else {
                                    public_route(&shared, &request)
                                }
                            },
                        );
                        write_response(&mut stream, &response)
                    })();
                    if let Err(error) = result {
                        eprintln!("layerx-testnet-control connection failed: {error}");
                    }
                });
            }
            Err(error) => eprintln!("layerx-testnet-control accept failed: {error}"),
        }
    }
    Ok(())
}

struct ConnectionPermit;

impl ConnectionPermit {
    fn acquire() -> Option<Self> {
        ACTIVE_CONNECTIONS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
                (active < MAX_CONNECTIONS).then_some(active + 1)
            })
            .ok()
            .map(|_| Self)
    }
}

impl Drop for ConnectionPermit {
    fn drop(&mut self) {
        ACTIVE_CONNECTIONS.fetch_sub(1, Ordering::AcqRel);
    }
}

fn run(config: Config) -> Result<(), String> {
    let public = TcpListener::bind(config.public_listen).map_err(|error| error.to_string())?;
    let admin = TcpListener::bind(config.admin_listen).map_err(|error| error.to_string())?;
    let config = Arc::new(config);
    let admin_config = Arc::clone(&config);
    thread::spawn(move || {
        if let Err(error) = serve(&admin, &admin_config, true) {
            eprintln!("layerx-testnet-control admin listener failed: {error}");
        }
    });
    eprintln!("layerx-testnet-control public and private TLS listeners started");
    serve(&public, &config, false)
}

fn main() {
    if let Err(error) = config().and_then(run) {
        eprintln!("layerx-testnet-control: {error}");
        std::process::exit(2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release() -> ReleaseGate {
        ReleaseGate {
            package_semver: "0.1.0".to_owned(),
            pending_package_semver: "0.1.0".to_owned(),
            pending_wire_version: LXP_WIRE_PROTOCOL_VERSION,
        }
    }

    fn reports(failing: &[Dependency]) -> DependencyReports {
        DependencyReports::new(|dependency| {
            if failing.contains(&dependency) {
                Err(format!("{} is unreachable", dependency.name()))
            } else {
                Ok(format!("{} answered", dependency.name()))
            }
        })
    }

    fn json(value: impl Serialize) -> Result<serde_json::Value, String> {
        serde_json::to_value(value).map_err(|error| error.to_string())
    }

    fn keys(value: &serde_json::Value) -> Vec<String> {
        value
            .as_object()
            .map(|object| object.keys().cloned().collect())
            .unwrap_or_default()
    }

    #[test]
    fn dependency_reports_are_indexed_by_dependency() {
        let reports = reports(&[]);
        for dependency in Dependency::ALL {
            assert_eq!(reports.report(dependency).dependency, dependency);
        }
        assert_eq!(reports.classify().len(), Dependency::ALL.len());
    }

    #[test]
    fn journeys_declare_their_dependency_sets() {
        assert_eq!(
            Journey::Funding.dependencies(),
            &[
                Dependency::Identity,
                Dependency::Faucet,
                Dependency::Redis,
                Dependency::CoreAdmin,
                Dependency::Core
            ]
        );
        assert_eq!(
            Journey::Payment.dependencies(),
            &[
                Dependency::Identity,
                Dependency::Gateway,
                Dependency::Core,
                Dependency::ReceiptAuthority
            ]
        );
        assert_eq!(
            Journey::ReceiptInspection.dependencies(),
            &[
                Dependency::Gateway,
                Dependency::ReceiptAuthority,
                Dependency::Core
            ]
        );
        assert_eq!(
            Journey::Programs.dependencies(),
            &[
                Dependency::Gateway,
                Dependency::Registry,
                Dependency::Core,
                Dependency::ReceiptAuthority
            ]
        );
        assert_eq!(
            Journey::from_route("/v1/journeys/receipt-inspection"),
            Some(Journey::ReceiptInspection)
        );
        assert_eq!(
            Journey::from_route("/v1/journeys/funding"),
            Some(Journey::Funding)
        );
        assert_eq!(
            Journey::from_route("/v1/journeys/payment"),
            Some(Journey::Payment)
        );
        assert_eq!(
            Journey::from_route("/v1/journeys/programs"),
            Some(Journey::Programs)
        );
        assert_eq!(Journey::from_route("/v1/journeys/settlement"), None);
    }

    #[test]
    fn journey_readiness_is_the_conjunction_of_its_declared_dependencies() {
        let reports = reports(&[Dependency::Identity]);
        for journey in Journey::ALL {
            let declares_identity = journey.dependencies().contains(&Dependency::Identity);
            match JourneyReadiness::compute(journey, &reports) {
                JourneyReadiness::Ready(ready) => {
                    assert!(!declares_identity, "{} must be degraded", journey.name());
                    assert_eq!(ready.dependencies.len(), journey.dependencies().len());
                    assert!(ready.view().failing.is_empty());
                }
                JourneyReadiness::Degraded(degraded) => {
                    assert!(declares_identity, "{} must be ready", journey.name());
                    assert_eq!(degraded.failing.name, "identity");
                    assert_eq!(degraded.failing_names(), vec!["identity"]);
                    let view = degraded.view();
                    assert!(!view.ready);
                    assert_eq!(view.dependencies.len(), journey.dependencies().len());
                    assert_eq!(
                        view.dependencies
                            .iter()
                            .filter(|dependency| !dependency.ready)
                            .map(|dependency| dependency.name)
                            .collect::<Vec<_>>(),
                        vec!["identity"]
                    );
                }
            }
        }
    }

    #[test]
    fn degraded_journey_names_every_failing_dependency_once() -> Result<(), String> {
        let reports = reports(&[Dependency::Faucet, Dependency::Core]);
        let JourneyReadiness::Degraded(degraded) =
            JourneyReadiness::compute(Journey::Funding, &reports)
        else {
            return Err("funding must be degraded".to_owned());
        };
        assert_eq!(degraded.failing_names(), vec!["faucet", "core"]);
        let refusal = degraded.refusal();
        assert_eq!(refusal["error"]["code"], "journey_degraded");
        assert_eq!(refusal["error"]["journey"], "funding");
        assert_eq!(
            refusal["error"]["failing"],
            serde_json::json!(["faucet", "core"])
        );
        let admission = json(JourneyReadiness::Degraded(degraded).admission())?;
        assert_eq!(admission["admitted"], false);
        assert_eq!(admission["journey"], "funding");
        assert_eq!(admission["failing"], serde_json::json!(["faucet", "core"]));
        Ok(())
    }

    #[test]
    fn global_ready_needs_every_journey_every_dependency_and_the_release() -> Result<(), String> {
        let ready = HostedReadiness::assemble(&reports(&[]), &release());
        assert!(ready.is_ready());
        let document = json(ready.document(&release()))?;
        assert_eq!(document["state"], "ready");
        assert_eq!(document["release"]["state"], "ready");
        assert_eq!(document["dependencies"].as_array().map(Vec::len), Some(9));
        assert_eq!(document["journeys"].as_array().map(Vec::len), Some(4));
        assert!(document["dependencies"]
            .as_array()
            .is_some_and(|entries| entries.iter().all(|entry| entry["ready"] == true)));
        assert!(document["journeys"]
            .as_array()
            .is_some_and(|entries| entries.iter().all(|entry| entry["ready"] == true)));

        let paxeer_down = HostedReadiness::assemble(&reports(&[Dependency::Paxeer]), &release());
        assert!(!paxeer_down.is_ready());
        let document = json(paxeer_down.document(&release()))?;
        assert_eq!(document["state"], "degraded");
        assert!(document["journeys"]
            .as_array()
            .is_some_and(|entries| entries.iter().all(|entry| entry["ready"] == true)));
        assert_eq!(document["dependencies"][8]["name"], "paxeer");
        assert_eq!(document["dependencies"][8]["ready"], false);

        let registry_down =
            HostedReadiness::assemble(&reports(&[Dependency::Registry]), &release());
        assert!(!registry_down.is_ready());
        let document = json(registry_down.document(&release()))?;
        assert_eq!(document["state"], "degraded");
        assert_eq!(document["journeys"][3]["journey"], "programs");
        assert_eq!(document["journeys"][3]["ready"], false);
        assert_eq!(
            document["journeys"][3]["failing"],
            serde_json::json!(["registry"])
        );
        for index in 0..3 {
            assert_eq!(document["journeys"][index]["ready"], true);
        }
        Ok(())
    }

    #[test]
    fn release_mismatch_degrades_the_global_state_without_touching_journeys() -> Result<(), String>
    {
        let mismatch = ReleaseGate {
            package_semver: "0.1.0".to_owned(),
            pending_package_semver: "0.1.1".to_owned(),
            pending_wire_version: LXP_WIRE_PROTOCOL_VERSION,
        };
        let readiness = HostedReadiness::assemble(&reports(&[]), &mismatch);
        assert!(!readiness.is_ready());
        assert!(!readiness.release_ready());
        let document = json(readiness.document(&mismatch))?;
        assert_eq!(document["state"], "degraded");
        assert_eq!(document["release"]["state"], "degraded");
        assert!(document["journeys"]
            .as_array()
            .is_some_and(|entries| entries.iter().all(|entry| entry["ready"] == true)));
        let status = json(readiness.public_status(&mismatch))?;
        assert_eq!(status["state"], "degraded");
        assert_eq!(status["components"][0]["name"], "testnet");
        assert_eq!(status["components"][0]["state"], "degraded");
        Ok(())
    }

    #[test]
    fn public_status_keeps_the_published_four_component_shape() -> Result<(), String> {
        let readiness = HostedReadiness::assemble(&reports(&[Dependency::Gateway]), &release());
        let status = json(readiness.public_status(&release()))?;
        assert_eq!(
            keys(&status),
            vec![
                "components",
                "lxp_wire_protocol_version",
                "network_id",
                "package_semver",
                "service",
                "state"
            ]
        );
        assert_eq!(status["state"], "degraded");
        assert_eq!(status["network_id"], TESTNET_NETWORK_ID);
        let components = status["components"]
            .as_array()
            .ok_or_else(|| "components must be an array".to_owned())?;
        assert_eq!(
            components
                .iter()
                .map(|component| component["name"].clone())
                .collect::<Vec<_>>(),
            vec!["testnet", "gateway", "core", "paxeer"]
        );
        for component in components {
            assert_eq!(keys(component), vec!["name", "state"]);
        }
        assert_eq!(components[1]["state"], "unavailable");
        assert_eq!(components[2]["state"], "ready");
        Ok(())
    }

    #[test]
    fn redis_endpoint_requires_the_rediss_scheme() -> Result<(), String> {
        let endpoint = Endpoint::parse_redis(
            "rediss://layerx-faucet-redis.layerx-testnet.svc.cluster.local:6379",
        )?;
        assert_eq!(endpoint.port, 6379);
        assert!(endpoint.path.is_empty());
        assert_eq!(Endpoint::parse_redis("rediss://redis.internal")?.port, 6379);
        assert!(Endpoint::parse_redis("redis://redis.internal:6379").is_err());
        assert!(Endpoint::parse_redis("rediss://redis.internal:6379/0").is_err());
        assert!(Endpoint::parse("rediss://redis.internal:6379").is_err());
        assert_eq!(
            Endpoint::parse("https://layerx-gateway.layerx-testnet.svc.cluster.local:443")?.port,
            443
        );
        Ok(())
    }
}

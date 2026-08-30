//! Production HTTPS+JSON boundary for the versioned human-api contract.

pub mod backend;
mod component;
mod component_protocol;
mod http;
mod limits;
mod privileged;
pub mod schema;

use std::io;
use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use layerx_client::lni::transport::ConnectionGate;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use zeroize::Zeroize;

pub use backend::{
    default_component_limits, ApiFailure, ComponentState, HumanApiComponents, Readiness,
    UnixComponents,
};
pub use component::{
    BoundHumanComponentServer, ComponentServerConfig, ComponentServerError, ComponentShutdown,
    HumanComponentServer,
};
pub use http::{HttpConfig, Router};
pub use limits::PrincipalLimits;
pub use privileged::{
    AuthorizationGrantPolicy, AuthorizedSession, ComponentOperationRequest,
    PrivilegedHumanComponents, PrivilegedHumanServices,
};

/// Explicit finite HTTPS listener configuration.
pub struct HttpsConfig {
    pub bind: SocketAddr,
    pub certificate_der: Vec<u8>,
    pub private_key_der: Vec<u8>,
    pub maximum_connections: usize,
    pub io_deadline: Duration,
}

impl HttpsConfig {
    fn rustls(&mut self) -> Result<Arc<ServerConfig>, ApiFailure> {
        if self.certificate_der.is_empty()
            || self.private_key_der.is_empty()
            || self.maximum_connections == 0
            || self.io_deadline.is_zero()
        {
            return Err(ApiFailure::unavailable());
        }
        let private_key_bytes = std::mem::take(&mut self.private_key_der);
        let private_key = PrivateKeyDer::try_from(private_key_bytes)
            .map_err(|_| ApiFailure::unavailable())?;
        let configuration = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(self.certificate_der.clone())],
                private_key,
            )
            .map_err(|_| ApiFailure::unavailable())?;
        Ok(Arc::new(configuration))
    }
}

impl Drop for HttpsConfig {
    fn drop(&mut self) {
        self.private_key_der.zeroize();
    }
}

/// Runnable HTTPS server which accepts one bounded request per TLS connection.
pub struct HttpsServer<B: HumanApiComponents> {
    router: Arc<Router<B>>,
    configuration: HttpsConfig,
}

impl<B: HumanApiComponents> HttpsServer<B> {
    #[must_use]
    pub const fn new(router: Arc<Router<B>>, configuration: HttpsConfig) -> Self {
        Self {
            router,
            configuration,
        }
    }

    /// Binds the configured HTTPS address and serves until the listener fails.
    ///
    /// # Errors
    ///
    /// Refuses invalid TLS material and propagates listener failures. Individual
    /// connection failures are isolated to their bounded worker.
    pub fn run(mut self) -> Result<(), ServerError> {
        let tls = self.configuration.rustls().map_err(ServerError::Configuration)?;
        let listener = TcpListener::bind(self.configuration.bind).map_err(ServerError::Io)?;
        let gate = ConnectionGate::new(self.configuration.maximum_connections);
        loop {
            let (tcp, peer) = listener.accept().map_err(ServerError::Io)?;
            let permit = match gate.acquire() {
                Ok(permit) => permit,
                Err(_) => continue,
            };
            let router = Arc::clone(&self.router);
            let tls = Arc::clone(&tls);
            let deadline = self.configuration.io_deadline;
            thread::spawn(move || {
                let _permit = permit;
                if tcp.set_read_timeout(Some(deadline)).is_err()
                    || tcp.set_write_timeout(Some(deadline)).is_err()
                {
                    return;
                }
                let Ok(connection) = ServerConnection::new(tls) else {
                    return;
                };
                let mut stream = StreamOwned::new(connection, tcp);
                let public_rate_key = public_rate_key(peer);
                let _ = router.serve_one(&mut stream, &public_rate_key);
            });
        }
    }
}

fn public_rate_key(peer: SocketAddr) -> String {
    format!("bootstrap:{}", peer.ip())
}

#[derive(Debug)]
pub enum ServerError {
    Configuration(ApiFailure),
    Io(io::Error),
}

impl std::fmt::Display for ServerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Configuration(_) => formatter.write_str("invalid human service configuration"),
            Self::Io(error) => write!(formatter, "human service listener failed: {error}"),
        }
    }
}

impl std::error::Error for ServerError {}

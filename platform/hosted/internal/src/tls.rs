//! TLS in both directions: the rustls listener the services answer on and the
//! native-tls client the event sources dial their upstream with.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

use native_tls::{Certificate, Identity, TlsConnector};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::server::WebPkiClientVerifier;
use rustls::{RootCertStore, ServerConfig};
use zeroize::Zeroizing;

use crate::http::read_http_message;
use crate::secret::read_secret_file;

/// Largest upstream response body the client accepts.
pub const MAX_UPSTREAM_BYTES: usize = 512 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const IO_TIMEOUT: Duration = Duration::from_secs(8);

/// Builds the listener configuration from `<prefix>_TLS_CERT_DER`,
/// `<prefix>_TLS_KEY_DER` and the optional `<prefix>_CLIENT_CA_DER`. When a
/// client CA is configured peers may present a certificate it issued; the
/// request records whether one was verified so routes can insist on it while
/// liveness and readiness stay reachable by the kubelet.
///
/// # Errors
/// Returns a description when a file is missing or the material is invalid.
pub fn server_config(prefix: &str) -> Result<Arc<ServerConfig>, String> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let certificate_path = env::var(format!("{prefix}_TLS_CERT_DER"))
        .map_err(|_| format!("{prefix}_TLS_CERT_DER is required"))?;
    let key_path = env::var(format!("{prefix}_TLS_KEY_DER"))
        .map_err(|_| format!("{prefix}_TLS_KEY_DER is required"))?;
    let certificate =
        CertificateDer::from(fs::read(certificate_path).map_err(|error| error.to_string())?);
    let key = PrivateKeyDer::from(PrivatePkcs8KeyDer::from(
        fs::read(key_path).map_err(|error| error.to_string())?,
    ));
    let builder = ServerConfig::builder();
    let config = match env::var(format!("{prefix}_CLIENT_CA_DER")) {
        Ok(path) => {
            let mut roots = RootCertStore::empty();
            roots
                .add(CertificateDer::from(
                    fs::read(path).map_err(|error| error.to_string())?,
                ))
                .map_err(|error| error.to_string())?;
            let verifier = WebPkiClientVerifier::builder(Arc::new(roots))
                .allow_unauthenticated()
                .build()
                .map_err(|error| error.to_string())?;
            builder.with_client_cert_verifier(verifier)
        }
        Err(_) => builder.with_no_client_auth(),
    }
    .with_single_cert(vec![certificate], key)
    .map_err(|error| error.to_string())?;
    Ok(Arc::new(config))
}

/// True when `<prefix>_CLIENT_CA_DER` is configured, i.e. authenticated
/// routes must see a verified peer certificate.
#[must_use]
pub fn client_ca_configured(prefix: &str) -> bool {
    env::var(format!("{prefix}_CLIENT_CA_DER")).is_ok()
}

/// An `https://host[:port]` origin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Origin {
    pub host: String,
    pub port: u16,
}

impl Origin {
    /// Parses an `https://` origin with an optional port and no path.
    ///
    /// # Errors
    /// Returns a description when the URL is not a bare https origin with a
    /// DNS host name.
    pub fn parse(url: &str) -> Result<Self, String> {
        let rest = url
            .strip_prefix("https://")
            .ok_or_else(|| "upstream URL must use https".to_owned())?;
        let rest = rest.strip_suffix('/').unwrap_or(rest);
        if rest.is_empty() || rest.contains(['/', '?', '#', '@', ' ']) {
            return Err("upstream URL must be a bare origin".to_owned());
        }
        let (host, port) = match rest.rsplit_once(':') {
            Some((host, port)) => (
                host,
                port.parse::<u16>()
                    .ok()
                    .filter(|port| *port != 0)
                    .ok_or_else(|| "upstream port is invalid".to_owned())?,
            ),
            None => (rest, 443),
        };
        if host.is_empty()
            || host.len() > 253
            || host.parse::<IpAddr>().is_ok()
            || !host
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.'))
        {
            return Err("upstream host must be a DNS name".to_owned());
        }
        Ok(Self {
            host: host.to_ascii_lowercase(),
            port,
        })
    }
}

/// An upstream response.
pub struct UpstreamResponse {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
}

/// Reasons an upstream request produced no response.
#[derive(Debug)]
pub enum UpstreamFailure {
    /// The upstream could not be reached or the exchange was malformed.
    Unavailable(String),
}

/// The outbound TLS client: pinned CA, optional client identity, bearer.
pub struct Upstream {
    origin: Origin,
    ca: Certificate,
    identity: Option<Identity>,
    token: Option<Zeroizing<String>>,
    cookie: Option<Zeroizing<String>>,
}

impl Upstream {
    /// Builds the client from `<prefix>_UPSTREAM_URL`, `<prefix>_UPSTREAM_CA_DER`,
    /// the optional `<prefix>_UPSTREAM_TOKEN_FILE` and the optional
    /// `<prefix>_UPSTREAM_CLIENT_IDENTITY_PKCS12` plus its
    /// `<prefix>_UPSTREAM_CLIENT_IDENTITY_PASSWORD_FILE`.
    ///
    /// # Errors
    /// Returns a description when the configuration is incomplete or invalid.
    pub fn from_environment(prefix: &str) -> Result<Self, String> {
        let origin = Origin::parse(
            &env::var(format!("{prefix}_UPSTREAM_URL"))
                .map_err(|_| format!("{prefix}_UPSTREAM_URL is required"))?,
        )?;
        let ca_path = env::var(format!("{prefix}_UPSTREAM_CA_DER"))
            .map_err(|_| format!("{prefix}_UPSTREAM_CA_DER is required"))?;
        let ca = Certificate::from_der(&fs::read(ca_path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
        let token = match env::var(format!("{prefix}_UPSTREAM_TOKEN_FILE")) {
            Ok(path) => Some(read_secret_file(std::path::Path::new(&path))?),
            Err(_) => None,
        };
        let identity = match env::var(format!("{prefix}_UPSTREAM_CLIENT_IDENTITY_PKCS12")) {
            Ok(path) => {
                let password_path =
                    env::var(format!("{prefix}_UPSTREAM_CLIENT_IDENTITY_PASSWORD_FILE")).map_err(
                        |_| format!("{prefix}_UPSTREAM_CLIENT_IDENTITY_PASSWORD_FILE is required"),
                    )?;
                let password = read_secret_file(std::path::Path::new(&password_path))?;
                let bundle = fs::read(path).map_err(|error| error.to_string())?;
                Some(
                    Identity::from_pkcs12(&bundle, password.as_str())
                        .map_err(|error| error.to_string())?,
                )
            }
            Err(_) => None,
        };
        Ok(Self {
            origin,
            ca,
            identity,
            token,
            cookie: env::var(format!("{prefix}_UPSTREAM_COOKIE_FILE"))
                .ok()
                .map(|path| read_secret_file(std::path::Path::new(&path)))
                .transpose()?,
        })
    }

    /// The configured origin.
    #[must_use]
    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    /// Performs a GET against `path` on the origin.
    ///
    /// # Errors
    /// Returns [`UpstreamFailure::Unavailable`] when no well-formed response
    /// arrives.
    pub fn get(&self, path: &str) -> Result<UpstreamResponse, UpstreamFailure> {
        self.request("GET", path, &[], None)
    }

    /// Performs a GET with an explicit principal credential.
    /// # Errors
    /// Refuses malformed credentials and TLS or HTTP failures.
    pub fn get_as(
        &self,
        path: &str,
        header: &str,
        credential: &str,
    ) -> Result<UpstreamResponse, UpstreamFailure> {
        if !matches!(header, "Cookie" | "Authorization")
            || credential.bytes().any(|byte| byte.is_ascii_control())
            || credential.len() > 4096
        {
            return Err(UpstreamFailure::Unavailable(
                "invalid principal credential".to_owned(),
            ));
        }
        self.request("GET", path, &[], Some((header, credential)))
    }

    fn request(
        &self,
        method: &str,
        path: &str,
        body: &[u8],
        credential: Option<(&str, &str)>,
    ) -> Result<UpstreamResponse, UpstreamFailure> {
        if !path.starts_with('/') || path.contains(['?', '#', ' ', '\r', '\n']) || path.len() > 1024
        {
            return Err(UpstreamFailure::Unavailable(
                "upstream path is invalid".to_owned(),
            ));
        }
        let mut builder = TlsConnector::builder();
        builder
            .disable_built_in_roots(true)
            .add_root_certificate(self.ca.clone())
            .min_protocol_version(Some(native_tls::Protocol::Tlsv12));
        if let Some(identity) = &self.identity {
            builder.identity(identity.clone());
        }
        let connector = builder
            .build()
            .map_err(|error| UpstreamFailure::Unavailable(error.to_string()))?;
        let addresses = resolve(&self.origin)?;
        let mut last = "upstream has no addresses".to_owned();
        for address in addresses {
            let tcp = match TcpStream::connect_timeout(&address, CONNECT_TIMEOUT) {
                Ok(tcp) => tcp,
                Err(error) => {
                    last = error.to_string();
                    continue;
                }
            };
            tcp.set_read_timeout(Some(IO_TIMEOUT))
                .map_err(|error| UpstreamFailure::Unavailable(error.to_string()))?;
            tcp.set_write_timeout(Some(IO_TIMEOUT))
                .map_err(|error| UpstreamFailure::Unavailable(error.to_string()))?;
            let mut stream = connector
                .connect(&self.origin.host, tcp)
                .map_err(|error| UpstreamFailure::Unavailable(error.to_string()))?;
            let authorization = self.token.as_ref().map_or_else(String::new, |token| {
                format!("Authorization: Bearer {}\r\n", token.as_str())
            });
            let cookie = self.cookie.as_ref().map_or_else(String::new, |cookie| {
                format!("Cookie: __Host-layerx_access={}\r\n", cookie.as_str())
            });
            let principal = credential.map_or_else(String::new, |(header, value)| {
                format!("{header}: {value}\r\n")
            });
            let (authorization, cookie) = if credential.is_some() {
                (String::new(), String::new())
            } else {
                (authorization, cookie)
            };
            let host = if self.origin.port == 443 {
                self.origin.host.clone()
            } else {
                format!("{}:{}", self.origin.host, self.origin.port)
            };
            write!(
                stream,
                "{method} {path} HTTP/1.1\r\nHost: {host}\r\nAccept: application/json\r\nContent-Type: application/json\r\nUser-Agent: LayerX-Internal/1\r\n{authorization}{cookie}{principal}Content-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .map_err(|error| UpstreamFailure::Unavailable(error.to_string()))?;
            stream
                .write_all(body)
                .map_err(|error| UpstreamFailure::Unavailable(error.to_string()))?;
            stream
                .flush()
                .map_err(|error| UpstreamFailure::Unavailable(error.to_string()))?;
            return read_response(&mut stream);
        }
        Err(UpstreamFailure::Unavailable(last))
    }
}

fn resolve(origin: &Origin) -> Result<Vec<SocketAddr>, UpstreamFailure> {
    let addresses: Vec<SocketAddr> = (origin.host.as_str(), origin.port)
        .to_socket_addrs()
        .map_err(|error| UpstreamFailure::Unavailable(error.to_string()))?
        .collect();
    if addresses.is_empty() {
        return Err(UpstreamFailure::Unavailable(
            "upstream host did not resolve".to_owned(),
        ));
    }
    Ok(addresses)
}

fn read_response(stream: &mut impl Read) -> Result<UpstreamResponse, UpstreamFailure> {
    let message =
        read_http_message(stream, MAX_UPSTREAM_BYTES).map_err(UpstreamFailure::Unavailable)?;
    let status_line = message
        .headers
        .get("")
        .ok_or_else(|| UpstreamFailure::Unavailable("status line is missing".to_owned()))?;
    let mut parts = status_line.split_whitespace();
    if parts.next() != Some("HTTP/1.1") {
        return Err(UpstreamFailure::Unavailable(
            "upstream is not HTTP/1.1".to_owned(),
        ));
    }
    let status = parts
        .next()
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|status| (100..600).contains(status))
        .ok_or_else(|| UpstreamFailure::Unavailable("status code is invalid".to_owned()))?;
    let headers: BTreeMap<String, String> = message.headers;
    if headers
        .get("content-length")
        .is_none_or(|value| value.parse::<usize>().ok() != Some(message.body.len()))
    {
        return Err(UpstreamFailure::Unavailable(
            "upstream response length is unframed".to_owned(),
        ));
    }
    Ok(UpstreamResponse {
        status,
        content_type: headers.get("content-type").cloned().unwrap_or_default(),
        body: message.body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_are_bare_https_dns_names() {
        assert_eq!(
            Origin::parse("https://kms.layerx-internal.svc"),
            Ok(Origin {
                host: "kms.layerx-internal.svc".to_owned(),
                port: 443
            })
        );
        assert_eq!(
            Origin::parse("https://Localhost:9443/"),
            Ok(Origin {
                host: "localhost".to_owned(),
                port: 9443
            })
        );
        assert!(Origin::parse("http://kms").is_err());
        assert!(Origin::parse("https://127.0.0.1:9443").is_err());
        assert!(Origin::parse("https://kms/v1").is_err());
        assert!(Origin::parse("https://kms:0").is_err());
        assert!(Origin::parse("https://user@kms").is_err());
    }

    #[test]
    fn responses_must_be_framed_by_content_length() {
        let mut framed = std::io::Cursor::new(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}"
                .to_vec(),
        );
        let response = read_response(&mut framed).unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, "application/json");
        assert_eq!(response.body, b"{}");
        let mut unframed =
            std::io::Cursor::new(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\n".to_vec());
        assert!(read_response(&mut unframed).is_err());
        let mut chunked = std::io::Cursor::new(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n".to_vec(),
        );
        assert!(read_response(&mut chunked).is_err());
    }
}

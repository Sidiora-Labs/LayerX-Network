//! Opaque-handle remote chain signer over UDS or mutually authenticated TLS.

use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ed25519_dalek::{Signature as Ed25519Signature, VerifyingKey as Ed25519VerifyingKey};
use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
use layerx_crypto::{ed25519, secp256k1};
use openssl::ssl::{SslConnector, SslFiletype, SslMethod, SslVerifyMode, SslVersion};
use sha2::Digest as _;
use sha3::Keccak256;

const MAGIC: &[u8; 4] = b"LXCS";
const VERSION: u16 = 1;
const MAX_HANDLE_BYTES: usize = 256;
const MAX_DOMAIN_BYTES: usize = 128;
const MAX_RESPONSE_BYTES: usize = 128;
const MAX_MESSAGE_BYTES: usize = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SigningAlgorithm {
    Secp256k1Recoverable,
    Ed25519,
}

impl SigningAlgorithm {
    const fn tag(self) -> u8 {
        match self {
            Self::Secp256k1Recoverable => 1,
            Self::Ed25519 => 2,
        }
    }
}

/// Authenticated transport only. Chain private key material is never present
/// in this process; `key_handle` is an opaque signer policy identifier.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SignerEndpoint {
    Uds {
        socket: PathBuf,
    },
    MutualTls {
        endpoint: SocketAddr,
        server_name: String,
        trust_anchor: PathBuf,
        client_certificate: PathBuf,
        client_private_key: PathBuf,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteSignerConfig {
    pub endpoint: SignerEndpoint,
    pub algorithm: SigningAlgorithm,
    pub key_handle: String,
    /// SEC1 compressed/uncompressed secp256k1 key or 32-byte Ed25519 key.
    pub public_key: Vec<u8>,
    pub timeout: Duration,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChainSignature {
    Secp256k1([u8; 65]),
    Ed25519([u8; 64]),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignerError {
    Configuration,
    Authentication,
    Unavailable,
    Deadline,
    Refused,
    MalformedResponse,
    InvalidSignature,
}

/// Remote signing client that validates every returned signature locally
/// before exposing it to transaction assembly.
pub struct RemoteChainSigner {
    config: RemoteSignerConfig,
    tls: Option<SslConnector>,
}

impl RemoteChainSigner {
    /// Validates endpoint, handle and public verification identity without
    /// opening a connection or importing any chain private key.
    pub fn new(config: RemoteSignerConfig) -> Result<Self, SignerError> {
        if config.key_handle.is_empty()
            || config.key_handle.len() > MAX_HANDLE_BYTES
            || config.key_handle.as_bytes().contains(&0)
            || config.timeout.is_zero()
        {
            return Err(SignerError::Configuration);
        }
        match config.algorithm {
            SigningAlgorithm::Secp256k1Recoverable => {
                if config.public_key.len() != 33 && config.public_key.len() != 65 {
                    return Err(SignerError::Configuration);
                }
                VerifyingKey::from_sec1_bytes(&config.public_key)
                    .map_err(|_| SignerError::Configuration)?;
            }
            SigningAlgorithm::Ed25519 => {
                let key: [u8; 32] = config
                    .public_key
                    .as_slice()
                    .try_into()
                    .map_err(|_| SignerError::Configuration)?;
                let key = Ed25519VerifyingKey::from_bytes(&key)
                    .map_err(|_| SignerError::Configuration)?;
                if key.is_weak() {
                    return Err(SignerError::Configuration);
                }
            }
        }
        let tls = match &config.endpoint {
            SignerEndpoint::Uds { socket } => {
                if socket.as_os_str().is_empty() {
                    return Err(SignerError::Configuration);
                }
                None
            }
            SignerEndpoint::MutualTls {
                server_name,
                trust_anchor,
                client_certificate,
                client_private_key,
                ..
            } => Some(tls_connector(
                server_name,
                trust_anchor,
                client_certificate,
                client_private_key,
            )?),
        };
        Ok(Self { config, tls })
    }

    #[must_use]
    pub fn key_handle(&self) -> &str {
        &self.config.key_handle
    }

    #[must_use]
    pub fn public_key(&self) -> &[u8] {
        &self.config.public_key
    }

    /// Requests a signature under an explicit policy domain and validates the
    /// exact returned signature against both digest and configured key.
    pub fn sign_digest(
        &self,
        policy_domain: &[u8],
        digest: [u8; 32],
    ) -> Result<ChainSignature, SignerError> {
        if policy_domain.is_empty()
            || policy_domain.len() > MAX_DOMAIN_BYTES
            || policy_domain.contains(&0)
        {
            return Err(SignerError::Configuration);
        }
        let request = encode_request(&self.config, policy_domain, digest, &[])?;
        let response = match &self.config.endpoint {
            SignerEndpoint::Uds { socket } => {
                let stream = UnixStream::connect(socket).map_err(|error| map_io(&error))?;
                stream
                    .set_read_timeout(Some(self.config.timeout))
                    .and_then(|()| stream.set_write_timeout(Some(self.config.timeout)))
                    .map_err(|error| map_io(&error))?;
                exchange(stream, &request)?
            }
            SignerEndpoint::MutualTls {
                endpoint,
                server_name,
                ..
            } => {
                let stream = TcpStream::connect_timeout(endpoint, self.config.timeout)
                    .map_err(|error| map_io(&error))?;
                stream
                    .set_read_timeout(Some(self.config.timeout))
                    .and_then(|()| stream.set_write_timeout(Some(self.config.timeout)))
                    .map_err(|error| map_io(&error))?;
                let connector = self.tls.as_ref().ok_or(SignerError::Authentication)?;
                let tls = connector
                    .connect(server_name, stream)
                    .map_err(|_| SignerError::Authentication)?;
                exchange(tls, &request)?
            }
        };
        verify_response(&self.config, digest, &response)
    }

    /// Requests an Ed25519 signature over the exact canonical message bytes.
    /// The digest is included for signer policy indexing but is not substituted
    /// for the message in signature verification (required by Solana).
    pub fn sign_message(
        &self,
        policy_domain: &[u8],
        message: &[u8],
    ) -> Result<ChainSignature, SignerError> {
        if self.config.algorithm != SigningAlgorithm::Ed25519
            || message.is_empty()
            || message.len() > MAX_MESSAGE_BYTES
        {
            return Err(SignerError::Configuration);
        }
        if policy_domain.is_empty()
            || policy_domain.len() > MAX_DOMAIN_BYTES
            || policy_domain.contains(&0)
        {
            return Err(SignerError::Configuration);
        }
        let digest: [u8; 32] = sha2::Sha256::digest(message).into();
        let request = encode_request(&self.config, policy_domain, digest, message)?;
        let response = match &self.config.endpoint {
            SignerEndpoint::Uds { socket } => {
                let stream = UnixStream::connect(socket).map_err(|error| map_io(&error))?;
                stream
                    .set_read_timeout(Some(self.config.timeout))
                    .and_then(|()| stream.set_write_timeout(Some(self.config.timeout)))
                    .map_err(|error| map_io(&error))?;
                exchange(stream, &request)?
            }
            SignerEndpoint::MutualTls {
                endpoint,
                server_name,
                ..
            } => {
                let stream = TcpStream::connect_timeout(endpoint, self.config.timeout)
                    .map_err(|error| map_io(&error))?;
                stream
                    .set_read_timeout(Some(self.config.timeout))
                    .and_then(|()| stream.set_write_timeout(Some(self.config.timeout)))
                    .map_err(|error| map_io(&error))?;
                let connector = self.tls.as_ref().ok_or(SignerError::Authentication)?;
                let tls = connector
                    .connect(server_name, stream)
                    .map_err(|_| SignerError::Authentication)?;
                exchange(tls, &request)?
            }
        };
        let Some(status) = response.first().copied() else {
            return Err(SignerError::MalformedResponse);
        };
        if status == 1 && response.len() == 1 {
            return Err(SignerError::Refused);
        }
        if status != 0 {
            return Err(SignerError::MalformedResponse);
        }
        let signature: [u8; 64] = response
            .get(1..)
            .ok_or(SignerError::MalformedResponse)?
            .try_into()
            .map_err(|_| SignerError::MalformedResponse)?;
        let public_key: [u8; 32] = self
            .config
            .public_key
            .as_slice()
            .try_into()
            .map_err(|_| SignerError::Configuration)?;
        let key =
            Ed25519VerifyingKey::from_bytes(&public_key).map_err(|_| SignerError::Configuration)?;
        key.verify_strict(message, &Ed25519Signature::from_bytes(&signature))
            .map_err(|_| SignerError::InvalidSignature)?;
        Ok(ChainSignature::Ed25519(signature))
    }

    /// Ethereum address derived from the independently configured secp256k1
    /// public key. It is never accepted from the signer response.
    pub fn ethereum_address(&self) -> Result<[u8; 20], SignerError> {
        if self.config.algorithm != SigningAlgorithm::Secp256k1Recoverable {
            return Err(SignerError::Configuration);
        }
        let key = VerifyingKey::from_sec1_bytes(&self.config.public_key)
            .map_err(|_| SignerError::Configuration)?;
        let point = key.to_encoded_point(false);
        let bytes = point.as_bytes();
        let body = bytes.get(1..).ok_or(SignerError::Configuration)?;
        let digest = Keccak256::digest(body);
        digest[12..]
            .try_into()
            .map_err(|_| SignerError::Configuration)
    }
}

fn tls_connector(
    server_name: &str,
    trust_anchor: &Path,
    client_certificate: &Path,
    client_private_key: &Path,
) -> Result<SslConnector, SignerError> {
    if server_name.is_empty() || server_name.as_bytes().contains(&0) {
        return Err(SignerError::Configuration);
    }
    for secret in [client_certificate, client_private_key] {
        let metadata = std::fs::metadata(secret).map_err(|_| SignerError::Authentication)?;
        if !metadata.is_file() || metadata.permissions().mode() & 0o037 != 0 {
            return Err(SignerError::Authentication);
        }
    }
    let mut connector =
        SslConnector::builder(SslMethod::tls_client()).map_err(|_| SignerError::Authentication)?;
    connector
        .set_min_proto_version(Some(SslVersion::TLS1_3))
        .map_err(|_| SignerError::Authentication)?;
    connector
        .set_ca_file(trust_anchor)
        .and_then(|()| connector.set_certificate_file(client_certificate, SslFiletype::PEM))
        .and_then(|()| connector.set_private_key_file(client_private_key, SslFiletype::PEM))
        .and_then(|()| connector.check_private_key())
        .map_err(|_| SignerError::Authentication)?;
    connector.set_verify(SslVerifyMode::PEER);
    Ok(connector.build())
}

fn encode_request(
    config: &RemoteSignerConfig,
    policy_domain: &[u8],
    digest: [u8; 32],
    message: &[u8],
) -> Result<Vec<u8>, SignerError> {
    let handle = config.key_handle.as_bytes();
    if message.len() > MAX_MESSAGE_BYTES {
        return Err(SignerError::Configuration);
    }
    let mut output = Vec::with_capacity(
        4 + 2 + 1 + 2 + handle.len() + 2 + policy_domain.len() + 32 + 4 + message.len(),
    );
    output.extend_from_slice(MAGIC);
    output.extend_from_slice(&VERSION.to_be_bytes());
    output.push(config.algorithm.tag());
    output.extend_from_slice(
        &u16::try_from(handle.len())
            .map_err(|_| SignerError::Configuration)?
            .to_be_bytes(),
    );
    output.extend_from_slice(handle);
    output.extend_from_slice(
        &u16::try_from(policy_domain.len())
            .map_err(|_| SignerError::Configuration)?
            .to_be_bytes(),
    );
    output.extend_from_slice(policy_domain);
    output.extend_from_slice(&digest);
    output.extend_from_slice(
        &u32::try_from(message.len())
            .map_err(|_| SignerError::Configuration)?
            .to_be_bytes(),
    );
    output.extend_from_slice(message);
    Ok(output)
}

fn exchange<S: Read + Write>(mut stream: S, request: &[u8]) -> Result<Vec<u8>, SignerError> {
    let length = u32::try_from(request.len()).map_err(|_| SignerError::Configuration)?;
    stream
        .write_all(&length.to_be_bytes())
        .map_err(|error| map_io(&error))?;
    stream.write_all(request).map_err(|error| map_io(&error))?;
    stream.flush().map_err(|error| map_io(&error))?;
    let mut response_length = [0_u8; 4];
    stream
        .read_exact(&mut response_length)
        .map_err(|error| map_io(&error))?;
    let length = usize::try_from(u32::from_be_bytes(response_length))
        .map_err(|_| SignerError::MalformedResponse)?;
    if length == 0 || length > MAX_RESPONSE_BYTES {
        return Err(SignerError::MalformedResponse);
    }
    let mut response = vec![0_u8; length];
    stream
        .read_exact(&mut response)
        .map_err(|error| map_io(&error))?;
    Ok(response)
}

fn verify_response(
    config: &RemoteSignerConfig,
    digest: [u8; 32],
    response: &[u8],
) -> Result<ChainSignature, SignerError> {
    let Some(status) = response.first().copied() else {
        return Err(SignerError::MalformedResponse);
    };
    if status == 1 && response.len() == 1 {
        return Err(SignerError::Refused);
    }
    if status != 0 {
        return Err(SignerError::MalformedResponse);
    }
    match config.algorithm {
        SigningAlgorithm::Secp256k1Recoverable => {
            let signature: [u8; 65] = response
                .get(1..)
                .ok_or(SignerError::MalformedResponse)?
                .try_into()
                .map_err(|_| SignerError::MalformedResponse)?;
            if signature[64] > 1 {
                return Err(SignerError::InvalidSignature);
            }
            let compact: [u8; 64] = signature[..64]
                .try_into()
                .map_err(|_| SignerError::MalformedResponse)?;
            secp256k1::verify_digest(&config.public_key, &compact, &digest)
                .map_err(|_| SignerError::InvalidSignature)?;
            let parsed =
                Signature::from_slice(&compact).map_err(|_| SignerError::InvalidSignature)?;
            if parsed.normalize_s().is_some() {
                return Err(SignerError::InvalidSignature);
            }
            let recovery =
                RecoveryId::try_from(signature[64]).map_err(|_| SignerError::InvalidSignature)?;
            let recovered = VerifyingKey::recover_from_prehash(&digest, &parsed, recovery)
                .map_err(|_| SignerError::InvalidSignature)?;
            let expected = VerifyingKey::from_sec1_bytes(&config.public_key)
                .map_err(|_| SignerError::Configuration)?;
            if recovered != expected {
                return Err(SignerError::InvalidSignature);
            }
            Ok(ChainSignature::Secp256k1(signature))
        }
        SigningAlgorithm::Ed25519 => {
            let signature: [u8; 64] = response
                .get(1..)
                .ok_or(SignerError::MalformedResponse)?
                .try_into()
                .map_err(|_| SignerError::MalformedResponse)?;
            let public_key: [u8; 32] = config
                .public_key
                .as_slice()
                .try_into()
                .map_err(|_| SignerError::Configuration)?;
            ed25519::verify_digest(&public_key, &signature, &digest)
                .map_err(|_| SignerError::InvalidSignature)?;
            Ok(ChainSignature::Ed25519(signature))
        }
    }
}

fn map_io(error: &std::io::Error) -> SignerError {
    match error.kind() {
        ErrorKind::TimedOut | ErrorKind::WouldBlock => SignerError::Deadline,
        _ => SignerError::Unavailable,
    }
}

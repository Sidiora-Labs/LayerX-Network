//! Single-attempt mutually authenticated remote signing transport.

use std::fmt;
use std::io::{ErrorKind, Write as _};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::time::Duration;

use ed25519_dalek::VerifyingKey;
use openssl::ssl::{HandshakeError, SslConnector, SslFiletype, SslMethod, SslVerifyMode};

use crate::ed25519;
use crate::signer::{AgentSignature, SignError, SignFuture, Signer, SigningRequest};

const REQUEST_MAGIC: &[u8; 4] = b"LXRS";
const PROTOCOL_VERSION: u16 = 1;
const MAX_REQUEST_BYTES: usize = 2_097_152;
const RESPONSE_SIGNATURE: u8 = 0;
const RESPONSE_REFUSED: u8 = 1;
const SIGNATURE_BYTES: usize = 64;

/// Complete refusal taxonomy for one remote signing attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignRefusal {
    /// Authenticated endpoint declined to sign.
    Refused,
    /// Connect, handshake, write, or read exceeded the deadline.
    Timeout,
    /// Endpoint could not be reached or disconnected mid-attempt.
    Unavailable,
    /// Server or client TLS identity could not be authenticated.
    Authentication,
    /// Framing or response encoding was invalid.
    MalformedResponse,
    /// Returned signature did not cover the exact requested bytes.
    InvalidSignature,
}

impl fmt::Display for SignRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Refused => "remote signer refused signing",
            Self::Timeout => "remote signer timed out",
            Self::Unavailable => "remote signer is unavailable",
            Self::Authentication => "remote signer mutual authentication failed",
            Self::MalformedResponse => "remote signer returned a malformed response",
            Self::InvalidSignature => "remote signer returned an invalid signature",
        })
    }
}

impl std::error::Error for SignRefusal {}

impl From<SignRefusal> for SignError {
    fn from(refusal: SignRefusal) -> Self {
        match refusal {
            SignRefusal::Refused => Self::RemoteRefused,
            SignRefusal::Timeout => Self::RemoteTimeout,
            SignRefusal::Unavailable => Self::RemoteUnavailable,
            SignRefusal::Authentication => Self::RemoteAuthentication,
            SignRefusal::MalformedResponse => Self::RemoteMalformedResponse,
            SignRefusal::InvalidSignature => Self::ReturnedSignatureInvalid,
        }
    }
}

/// Remote Ed25519 signer reached over a pinned mutual-TLS connection.
pub struct RemoteSigner {
    endpoint: SocketAddr,
    server_name: String,
    connector: SslConnector,
    public_key: [u8; 32],
    timeout: Duration,
}

impl RemoteSigner {
    /// Configures one remote signer and validates all local TLS material.
    ///
    /// # Errors
    ///
    /// Refuses invalid signer keys, missing trust/client credentials, invalid
    /// server names, and a zero timeout before any request can be sent.
    pub fn new(
        endpoint: SocketAddr,
        server_name: impl Into<String>,
        public_key: [u8; 32],
        trust_anchor: impl AsRef<Path>,
        client_certificate: impl AsRef<Path>,
        client_private_key: impl AsRef<Path>,
        timeout: Duration,
    ) -> Result<Self, SignRefusal> {
        let key =
            VerifyingKey::from_bytes(&public_key).map_err(|_| SignRefusal::InvalidSignature)?;
        if key.is_weak() || timeout.is_zero() {
            return Err(SignRefusal::InvalidSignature);
        }
        let server_name = server_name.into();
        if server_name.is_empty() || server_name.as_bytes().contains(&0) {
            return Err(SignRefusal::Authentication);
        }
        let mut connector = SslConnector::builder(SslMethod::tls_client())
            .map_err(|_| SignRefusal::Authentication)?;
        connector
            .set_ca_file(trust_anchor)
            .and_then(|()| connector.set_certificate_file(client_certificate, SslFiletype::PEM))
            .and_then(|()| connector.set_private_key_file(client_private_key, SslFiletype::PEM))
            .and_then(|()| connector.check_private_key())
            .map_err(|_| SignRefusal::Authentication)?;
        connector.set_verify(SslVerifyMode::PEER);
        Ok(Self {
            endpoint,
            server_name,
            connector: connector.build(),
            public_key,
            timeout,
        })
    }

    fn sign_once(&self, request: SigningRequest<'_>) -> Result<AgentSignature, SignRefusal> {
        let body = encode_request(request)?;
        let stream = TcpStream::connect_timeout(&self.endpoint, self.timeout)
            .map_err(|error| io_refusal(&error))?;
        stream
            .set_read_timeout(Some(self.timeout))
            .and_then(|()| stream.set_write_timeout(Some(self.timeout)))
            .map_err(|error| io_refusal(&error))?;
        let mut tls = self
            .connector
            .connect(&self.server_name, stream)
            .map_err(|error| handshake_refusal(&error))?;
        let length = u32::try_from(body.len()).map_err(|_| SignRefusal::MalformedResponse)?;
        tls.write_all(&length.to_be_bytes())
            .and_then(|()| tls.write_all(&body))
            .map_err(|error| io_refusal(&error))?;
        let response = read_response(&mut tls)?;
        ed25519::verify(&self.public_key, &response, request.message())
            .map_err(|_| SignRefusal::InvalidSignature)?;
        Ok(AgentSignature::new(response))
    }
}

impl fmt::Debug for RemoteSigner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RemoteSigner([mutually authenticated endpoint])")
    }
}

impl Signer for RemoteSigner {
    fn public_key(&self) -> [u8; 32] {
        self.public_key
    }

    fn sign<'a>(&'a self, request: SigningRequest<'a>) -> SignFuture<'a> {
        Box::pin(async move { self.sign_once(request).map_err(SignError::from) })
    }
}

fn encode_request(request: SigningRequest<'_>) -> Result<Vec<u8>, SignRefusal> {
    let message = request.message();
    let canonical = message.canonical();
    let disclosure = request
        .disclosure()
        .transport_bytes()
        .map_err(|_| SignRefusal::MalformedResponse)?;
    let canonical_length =
        u32::try_from(canonical.len()).map_err(|_| SignRefusal::MalformedResponse)?;
    let disclosure_length =
        u32::try_from(disclosure.len()).map_err(|_| SignRefusal::MalformedResponse)?;
    let capacity = REQUEST_MAGIC
        .len()
        .checked_add(2 + 2 + 4 + 32 + 4 + 4)
        .and_then(|length| length.checked_add(canonical.len()))
        .and_then(|length| length.checked_add(disclosure.len()))
        .ok_or(SignRefusal::MalformedResponse)?;
    if capacity > MAX_REQUEST_BYTES {
        return Err(SignRefusal::MalformedResponse);
    }
    let mut output = Vec::with_capacity(capacity);
    output.extend_from_slice(REQUEST_MAGIC);
    output.extend_from_slice(&PROTOCOL_VERSION.to_be_bytes());
    output.extend_from_slice(&message.protocol_version().to_be_bytes());
    output.extend_from_slice(&message.network_id().to_be_bytes());
    output.extend_from_slice(&message.digest());
    output.extend_from_slice(&canonical_length.to_be_bytes());
    output.extend_from_slice(canonical);
    output.extend_from_slice(&disclosure_length.to_be_bytes());
    output.extend_from_slice(&disclosure);
    Ok(output)
}

fn io_refusal(error: &std::io::Error) -> SignRefusal {
    match error.kind() {
        ErrorKind::TimedOut | ErrorKind::WouldBlock => SignRefusal::Timeout,
        _ => SignRefusal::Unavailable,
    }
}

fn handshake_refusal(error: &HandshakeError<TcpStream>) -> SignRefusal {
    let deadline_exceeded = match error {
        HandshakeError::Failure(mid) | HandshakeError::WouldBlock(mid) => mid
            .error()
            .io_error()
            .is_some_and(|io| matches!(io.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock)),
        HandshakeError::SetupFailure(_) => false,
    };
    if deadline_exceeded {
        SignRefusal::Timeout
    } else {
        SignRefusal::Authentication
    }
}

fn read_response(stream: &mut impl std::io::Read) -> Result<[u8; SIGNATURE_BYTES], SignRefusal> {
    let mut length = [0_u8; 4];
    stream
        .read_exact(&mut length)
        .map_err(|error| io_refusal(&error))?;
    let length =
        usize::try_from(u32::from_be_bytes(length)).map_err(|_| SignRefusal::MalformedResponse)?;
    if length == 1 {
        let mut status = [0_u8; 1];
        stream
            .read_exact(&mut status)
            .map_err(|error| io_refusal(&error))?;
        return if status[0] == RESPONSE_REFUSED {
            Err(SignRefusal::Refused)
        } else {
            Err(SignRefusal::MalformedResponse)
        };
    }
    if length != 1 + SIGNATURE_BYTES {
        return Err(SignRefusal::MalformedResponse);
    }
    let mut response = [0_u8; 1 + SIGNATURE_BYTES];
    stream
        .read_exact(&mut response)
        .map_err(|error| io_refusal(&error))?;
    if response[0] != RESPONSE_SIGNATURE {
        return Err(SignRefusal::MalformedResponse);
    }
    response[1..]
        .try_into()
        .map_err(|_| SignRefusal::MalformedResponse)
}

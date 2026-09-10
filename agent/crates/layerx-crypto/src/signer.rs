//! Object-safe signing interface and external OpenSSH-agent keystore signer.

use std::fmt;
use std::future::Future;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::Duration;

use ed25519_dalek::VerifyingKey;
use layerx_types::payload::ModuleRegistry;
use layerx_wire::hash::Domain;

use crate::disclosure::{Disclosure, DisclosureError};
use crate::{ct, ed25519, SignatureMessage};

pub use crate::local::LocalSigner;

const SSH_AGENT_SIGN_REQUEST: u8 = 13;
const SSH_AGENT_SIGN_RESPONSE: u8 = 14;
const SSH_AGENT_FAILURE: u8 = 5;
const SSH_ED25519: &[u8] = b"ssh-ed25519";
const MAX_AGENT_RESPONSE: usize = 4096;

/// The only input accepted by a signer: scoped canonical bytes plus their
/// validated structured disclosure.
#[derive(Clone, Copy)]
pub struct SigningRequest<'a> {
    message: SignatureMessage<'a>,
    disclosure: SigningDisclosure<'a>,
}

#[derive(Clone, Copy, Debug)]
pub enum SigningDisclosure<'a> {
    Activity(&'a Disclosure),
    SendDebit(&'a crate::send::SendDebit),
}

impl<'a> SigningRequest<'a> {
    /// Couples canonical bytes to a complete validated disclosure.
    ///
    /// # Errors
    ///
    /// Returns a typed refusal when any disclosed field was changed or the
    /// disclosure was created for another byte string or scope.
    pub fn new(
        message: SignatureMessage<'a>,
        disclosure: &'a Disclosure,
    ) -> Result<Self, SignError> {
        let disclosure_digest = disclosure.validated_digest().map_err(SignError::from)?;
        if !ct::eq_fixed(&disclosure_digest, &message.digest()) {
            return Err(SignError::DisclosureMismatch("canonical_bytes"));
        }
        Ok(Self {
            message,
            disclosure: SigningDisclosure::Activity(disclosure),
        })
    }

    pub(crate) const fn send_debit(
        message: SignatureMessage<'a>,
        disclosure: &'a crate::send::SendDebit,
    ) -> Self {
        Self {
            message,
            disclosure: SigningDisclosure::SendDebit(disclosure),
        }
    }

    #[must_use]
    pub const fn message(self) -> SignatureMessage<'a> {
        self.message
    }

    #[must_use]
    pub const fn disclosure(self) -> SigningDisclosure<'a> {
        self.disclosure
    }
}

impl SigningDisclosure<'_> {
    pub(crate) fn transport_bytes(self) -> Result<Vec<u8>, DisclosureError> {
        match self {
            Self::Activity(disclosure) => disclosure.transport_bytes(),
            Self::SendDebit(debit) => {
                let mut bytes = b"LXP/agent/send-debit-disclosure/v1\0".to_vec();
                bytes.extend_from_slice(&debit.authorization_message()?);
                Ok(bytes)
            }
        }
    }
}

impl fmt::Debug for SigningRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SigningRequest")
            .field("message", &self.message)
            .field("disclosure", &"[validated]")
            .finish()
    }
}

/// Public signature material returned by every signer location.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct AgentSignature([u8; 64]);

impl AgentSignature {
    /// Borrows the canonical Ed25519 signature bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 64] {
        &self.0
    }

    pub(crate) const fn new(bytes: [u8; 64]) -> Self {
        Self(bytes)
    }
}

impl fmt::Debug for AgentSignature {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AgentSignature([public signature])")
    }
}

/// Fixed, non-secret-bearing signing refusal taxonomy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignError {
    /// One named disclosure field differs from the canonical bytes.
    DisclosureMismatch(&'static str),
    /// The canonical bytes could not produce a complete disclosure.
    InvalidDisclosure,
    /// The configured key is malformed or weak.
    KeyRejected,
    /// The external key agent could not be reached or did not respond in time.
    KeystoreUnavailable,
    /// The external key agent explicitly refused the operation.
    KeystoreRefused,
    /// The external key agent returned a malformed protocol response.
    MalformedResponse,
    /// The external signature failed verification against the requested bytes.
    ReturnedSignatureInvalid,
    /// The authenticated remote signer explicitly refused the request.
    RemoteRefused,
    /// The remote signer exceeded the configured deadline.
    RemoteTimeout,
    /// The remote signer endpoint was unreachable or disconnected.
    RemoteUnavailable,
    /// Mutual authentication of the remote signer failed.
    RemoteAuthentication,
    /// The remote signer returned a malformed framed response.
    RemoteMalformedResponse,
}

impl fmt::Display for SignError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DisclosureMismatch(field) => {
                write!(
                    formatter,
                    "signing disclosure field {field} does not match canonical bytes"
                )
            }
            Self::InvalidDisclosure => {
                formatter.write_str("canonical bytes have no complete disclosure")
            }
            Self::KeyRejected => formatter.write_str("signing key was rejected"),
            Self::KeystoreUnavailable => {
                formatter.write_str("operating-system keystore is unavailable")
            }
            Self::KeystoreRefused => {
                formatter.write_str("operating-system keystore refused signing")
            }
            Self::MalformedResponse => {
                formatter.write_str("operating-system keystore returned malformed data")
            }
            Self::ReturnedSignatureInvalid => {
                formatter.write_str("returned signer signature is invalid")
            }
            Self::RemoteRefused => formatter.write_str("remote signer refused signing"),
            Self::RemoteTimeout => formatter.write_str("remote signer timed out"),
            Self::RemoteUnavailable => formatter.write_str("remote signer is unavailable"),
            Self::RemoteAuthentication => {
                formatter.write_str("remote signer mutual authentication failed")
            }
            Self::RemoteMalformedResponse => {
                formatter.write_str("remote signer returned a malformed response")
            }
        }
    }
}

impl std::error::Error for SignError {}

impl From<DisclosureError> for SignError {
    fn from(error: DisclosureError) -> Self {
        match error {
            DisclosureError::FieldMismatch(field) => Self::DisclosureMismatch(field),
            DisclosureError::Wire(_)
            | DisclosureError::UnsupportedActivity(_)
            | DisclosureError::MalformedPayload
            | DisclosureError::PayloadHash => Self::InvalidDisclosure,
        }
    }
}

/// Boxed future used to keep the signer trait object-safe without imposing one
/// asynchronous runtime on the workspace.
pub type SignFuture<'a> =
    Pin<Box<dyn Future<Output = Result<AgentSignature, SignError>> + Send + 'a>>;

/// Identical asynchronous-friendly interface for local, OS-keystore, and
/// remote key locations.
pub trait Signer: fmt::Debug + Send + Sync {
    /// Returns the public verification key without exposing private material.
    fn public_key(&self) -> [u8; 32];

    /// Signs one disclosure-bound canonical request or refuses it.
    fn sign<'a>(&'a self, request: SigningRequest<'a>) -> SignFuture<'a>;
}

/// Validates, re-encodes, and signs one canonical disclosed activity.
///
/// This is the only public route from activity bytes to a signer request.
#[must_use]
pub fn sign_disclosed<'a>(
    signer: &'a dyn Signer,
    canonical: &'a [u8],
    disclosure: &'a Disclosure,
    registry: &'a ModuleRegistry,
) -> SignFuture<'a> {
    Box::pin(async move {
        disclosure
            .validate_bytes(canonical, registry)
            .map_err(SignError::from)?;
        let (protocol_version, network_id) = disclosure.scope();
        let message = SignatureMessage::new(
            Domain::SignaturePreimage,
            protocol_version,
            network_id,
            canonical,
        )
        .map_err(|_| SignError::InvalidDisclosure)?;
        let request = SigningRequest::new(message, disclosure)?;
        signer.sign(request).await
    })
}

/// Ed25519 signer backed by an external OpenSSH-compatible OS key agent.
pub struct KeystoreSigner {
    socket: PathBuf,
    key_blob: Vec<u8>,
    public_key: [u8; 32],
    timeout: Duration,
}

impl KeystoreSigner {
    /// Configures an external key by public identity and agent socket only.
    ///
    /// # Errors
    ///
    /// Rejects malformed or weak public keys before any agent request.
    pub fn new(socket: impl Into<PathBuf>, public_key: [u8; 32]) -> Result<Self, SignError> {
        let key = VerifyingKey::from_bytes(&public_key).map_err(|_| SignError::KeyRejected)?;
        if key.is_weak() {
            return Err(SignError::KeyRejected);
        }
        let mut key_blob = Vec::with_capacity(4 + SSH_ED25519.len() + 4 + public_key.len());
        push_string(&mut key_blob, SSH_ED25519)?;
        push_string(&mut key_blob, &public_key)?;
        Ok(Self {
            socket: socket.into(),
            key_blob,
            public_key,
            timeout: Duration::from_secs(5),
        })
    }

    /// Overrides the bounded agent I/O timeout.
    #[must_use]
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

impl fmt::Debug for KeystoreSigner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("KeystoreSigner([external key])")
    }
}

impl Signer for KeystoreSigner {
    fn public_key(&self) -> [u8; 32] {
        self.public_key
    }

    fn sign<'a>(&'a self, request: SigningRequest<'a>) -> SignFuture<'a> {
        Box::pin(async move {
            let signature = sign_with_agent(
                &self.socket,
                &self.key_blob,
                request.message().digest(),
                self.timeout,
            )?;
            ed25519::verify(&self.public_key, &signature, request.message())
                .map_err(|_| SignError::ReturnedSignatureInvalid)?;
            Ok(AgentSignature::new(signature))
        })
    }
}

fn push_string(output: &mut Vec<u8>, value: &[u8]) -> Result<(), SignError> {
    let length = u32::try_from(value.len()).map_err(|_| SignError::MalformedResponse)?;
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(value);
    Ok(())
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn byte(&mut self) -> Result<u8, SignError> {
        let Some(value) = self.bytes.get(self.offset).copied() else {
            return Err(SignError::MalformedResponse);
        };
        self.offset += 1;
        Ok(value)
    }

    fn string(&mut self) -> Result<&'a [u8], SignError> {
        let Some(length_bytes) = self.bytes.get(self.offset..self.offset + 4) else {
            return Err(SignError::MalformedResponse);
        };
        let length = usize::try_from(u32::from_be_bytes([
            length_bytes[0],
            length_bytes[1],
            length_bytes[2],
            length_bytes[3],
        ]))
        .map_err(|_| SignError::MalformedResponse)?;
        self.offset += 4;
        let Some(end) = self.offset.checked_add(length) else {
            return Err(SignError::MalformedResponse);
        };
        let Some(value) = self.bytes.get(self.offset..end) else {
            return Err(SignError::MalformedResponse);
        };
        self.offset = end;
        Ok(value)
    }

    fn finish(self) -> Result<(), SignError> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(SignError::MalformedResponse)
        }
    }
}

#[cfg(unix)]
fn sign_with_agent(
    socket: &Path,
    key_blob: &[u8],
    digest: [u8; 32],
    timeout: Duration,
) -> Result<[u8; 64], SignError> {
    use std::os::unix::net::UnixStream;

    let mut body = Vec::with_capacity(1 + 4 + key_blob.len() + 4 + digest.len() + 4);
    body.push(SSH_AGENT_SIGN_REQUEST);
    push_string(&mut body, key_blob)?;
    push_string(&mut body, &digest)?;
    body.extend_from_slice(&0_u32.to_be_bytes());
    let body_length = u32::try_from(body.len()).map_err(|_| SignError::MalformedResponse)?;

    let mut stream = UnixStream::connect(socket).map_err(|_| SignError::KeystoreUnavailable)?;
    stream
        .set_read_timeout(Some(timeout))
        .and_then(|()| stream.set_write_timeout(Some(timeout)))
        .map_err(|_| SignError::KeystoreUnavailable)?;
    stream
        .write_all(&body_length.to_be_bytes())
        .and_then(|()| stream.write_all(&body))
        .map_err(|_| SignError::KeystoreUnavailable)?;

    let mut response_length = [0_u8; 4];
    stream
        .read_exact(&mut response_length)
        .map_err(|_| SignError::KeystoreUnavailable)?;
    let response_length = usize::try_from(u32::from_be_bytes(response_length))
        .map_err(|_| SignError::MalformedResponse)?;
    if response_length == 0 || response_length > MAX_AGENT_RESPONSE {
        return Err(SignError::MalformedResponse);
    }
    let mut response = vec![0_u8; response_length];
    stream
        .read_exact(&mut response)
        .map_err(|_| SignError::KeystoreUnavailable)?;
    parse_agent_response(&response)
}

#[cfg(not(unix))]
fn sign_with_agent(
    _socket: &Path,
    _key_blob: &[u8],
    _digest: [u8; 32],
    _timeout: Duration,
) -> Result<[u8; 64], SignError> {
    Err(SignError::KeystoreUnavailable)
}

fn parse_agent_response(response: &[u8]) -> Result<[u8; 64], SignError> {
    let mut response_reader = Reader::new(response);
    let response_type = response_reader.byte()?;
    if response_type == SSH_AGENT_FAILURE {
        return Err(SignError::KeystoreRefused);
    }
    if response_type != SSH_AGENT_SIGN_RESPONSE {
        return Err(SignError::MalformedResponse);
    }
    let signature_blob = response_reader.string()?;
    response_reader.finish()?;
    let mut signature_reader = Reader::new(signature_blob);
    if signature_reader.string()? != SSH_ED25519 {
        return Err(SignError::MalformedResponse);
    }
    let signature = signature_reader.string()?;
    signature_reader.finish()?;
    signature
        .try_into()
        .map_err(|_| SignError::MalformedResponse)
}

//! Treasury custody boundary: the identity that signs on behalf of the
//! treasury, held in process by tests and tools or reached by the core
//! boundary over the treasury signer socket.
//!
//! The signer speaks one request line and answers one JSON line before the
//! connection closes (`platform/hosted/node/signer/signer.py`):
//!
//! ```text
//! public-key\n     -> {"public_key":"<64 hex>","did":"did:layerx:<64 hex>","provider":"file"}
//! sign <64 hex>\n  -> {"public_key":"<64 hex>","digest":"<64 hex>","signature":"<128 hex>"}
//! refusal          -> {"error":{"code":"...","retry":"never"}}
//! ```

use crate::{fixed_hex, hex_encode};
use ed25519_dalek::{Signer as _, SigningKey, VerifyingKey};
use serde::Deserialize;
use std::fmt;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Longest wait for the signer to accept one request and answer it.
pub const SIGNER_TIMEOUT: Duration = Duration::from_secs(10);
const MAXIMUM_REPLY_BYTES: usize = 4096;

/// One Ed25519 identity that signs 32-byte domain digests for the treasury.
pub trait TreasurySigner: Send + Sync {
    /// Returns the public key every signature verifies under.
    fn public_key(&self) -> [u8; 32];

    /// Signs one 32-byte digest.
    ///
    /// # Errors
    ///
    /// Returns the refusal as text when the identity cannot sign the digest.
    fn sign_digest(&self, digest: &[u8; 32]) -> Result<[u8; 64], String>;
}

/// Why one SEND could not be built and signed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SendError {
    /// The treasury signer refused, answered out of contract or was unreachable.
    Signer(String),
    /// The request, its compilation or its encoding is invalid.
    Invalid(String),
}

impl fmt::Display for SendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Signer(reason) => write!(formatter, "treasury signer: {reason}"),
            Self::Invalid(reason) => formatter.write_str(reason),
        }
    }
}

impl std::error::Error for SendError {}

/// Derives the beta treasury DID `did:layerx:<public key hex>` from a public key.
#[must_use]
pub fn did_for_public_key(public_key: &[u8; 32]) -> String {
    format!("did:layerx:{}", hex_encode(public_key))
}

/// A seed held in process: the identity that tests and tools own themselves.
///
/// The core boundary binary never constructs one; it signs through
/// [`SocketSigner`] and never reads a seed.
#[derive(Clone)]
pub struct SeedSigner {
    key: SigningKey,
}

impl SeedSigner {
    /// Derives the identity of a 32-byte Ed25519 seed.
    #[must_use]
    pub fn new(seed: &[u8; 32]) -> Self {
        Self {
            key: SigningKey::from_bytes(seed),
        }
    }
}

impl fmt::Debug for SeedSigner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SeedSigner")
            .field("public_key", &hex_encode(&self.public_key()))
            .finish()
    }
}

impl TreasurySigner for SeedSigner {
    fn public_key(&self) -> [u8; 32] {
        self.key.verifying_key().to_bytes()
    }

    fn sign_digest(&self, digest: &[u8; 32]) -> Result<[u8; 64], String> {
        Ok(self.key.sign(digest).to_bytes())
    }
}

/// Client of the treasury signer socket: learns the treasury public key once
/// at connection time and asks for one signature per request afterwards.
#[derive(Clone, Debug)]
pub struct SocketSigner {
    path: PathBuf,
    public_key: [u8; 32],
    timeout: Duration,
}

#[derive(Deserialize)]
struct Reply {
    public_key: Option<String>,
    did: Option<String>,
    digest: Option<String>,
    signature: Option<String>,
    error: Option<Refusal>,
}

#[derive(Deserialize)]
struct Refusal {
    code: String,
}

impl SocketSigner {
    /// Connects to the signer at `path`, asks for the treasury public key and
    /// refuses any answer that is not one canonical Ed25519 key with its DID.
    ///
    /// # Errors
    ///
    /// Returns the connection, protocol or key refusal as text.
    pub fn connect(path: &Path) -> Result<Self, String> {
        Self::connect_with_timeout(path, SIGNER_TIMEOUT)
    }

    /// [`Self::connect`] with an explicit per-request deadline.
    ///
    /// # Errors
    ///
    /// Returns the connection, protocol or key refusal as text.
    pub fn connect_with_timeout(path: &Path, timeout: Duration) -> Result<Self, String> {
        if !path.is_absolute() {
            return Err(format!(
                "treasury signer socket {} is not an absolute path",
                path.display()
            ));
        }
        let mut signer = Self {
            path: path.to_path_buf(),
            public_key: [0; 32],
            timeout,
        };
        let reply = signer.request("public-key")?;
        let public_key_hex = reply
            .public_key
            .ok_or_else(|| "the treasury signer answered no public key".to_owned())?;
        let public_key = canonical_public_key(&public_key_hex)?;
        if reply.did.as_deref() != Some(did_for_public_key(&public_key).as_str()) {
            return Err("the treasury signer answered a DID for another key".into());
        }
        signer.public_key = public_key;
        Ok(signer)
    }

    /// Returns the socket path this client signs through.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn request(&self, line: &str) -> Result<Reply, String> {
        let mut stream = UnixStream::connect(&self.path).map_err(|error| {
            format!(
                "treasury signer socket {} is not available: {error}",
                self.path.display()
            )
        })?;
        stream
            .set_read_timeout(Some(self.timeout))
            .and_then(|()| stream.set_write_timeout(Some(self.timeout)))
            .map_err(|error| format!("treasury signer deadline cannot be set: {error}"))?;
        stream
            .write_all(format!("{line}\n").as_bytes())
            .and_then(|()| stream.flush())
            .map_err(|error| format!("the treasury signer did not accept the request: {error}"))?;
        let mut buffered = Vec::with_capacity(512);
        let mut chunk = [0_u8; 512];
        while !buffered.contains(&b'\n') && buffered.len() <= MAXIMUM_REPLY_BYTES {
            let count = stream
                .read(&mut chunk)
                .map_err(|error| format!("the treasury signer did not answer: {error}"))?;
            if count == 0 {
                break;
            }
            buffered.extend_from_slice(&chunk[..count]);
        }
        let end = buffered
            .iter()
            .position(|byte| *byte == b'\n')
            .ok_or_else(|| "the treasury signer reply is malformed".to_owned())?;
        if buffered.len() > MAXIMUM_REPLY_BYTES {
            return Err("the treasury signer reply is malformed".into());
        }
        let reply: Reply = serde_json::from_slice(&buffered[..end])
            .map_err(|_| "the treasury signer reply is not a reply object".to_owned())?;
        if let Some(refusal) = reply.error {
            return Err(format!(
                "the treasury signer refused the request: {}",
                refusal.code
            ));
        }
        Ok(reply)
    }
}

impl TreasurySigner for SocketSigner {
    fn public_key(&self) -> [u8; 32] {
        self.public_key
    }

    fn sign_digest(&self, digest: &[u8; 32]) -> Result<[u8; 64], String> {
        let digest_hex = hex_encode(digest);
        let reply = self.request(&format!("sign {digest_hex}"))?;
        if reply.digest.as_deref() != Some(digest_hex.as_str()) {
            return Err("the treasury signer answered a different digest".into());
        }
        if reply.public_key.as_deref() != Some(hex_encode(&self.public_key).as_str()) {
            return Err("the treasury signer answered under another key".into());
        }
        let signature_hex = reply
            .signature
            .ok_or_else(|| "the treasury signer answered no signature".to_owned())?;
        if signature_hex.len() != 128 || !signature_hex.bytes().all(lowercase_hex) {
            return Err("the treasury signer answered no signature".into());
        }
        let signature = fixed_hex::<64>("treasury signature", &signature_hex)?;
        layerx_crypto::ed25519::verify_digest(&self.public_key, &signature, digest)
            .map_err(|_| "the treasury signature does not verify".to_owned())?;
        Ok(signature)
    }
}

fn canonical_public_key(text: &str) -> Result<[u8; 32], String> {
    if text.len() != 64 || !text.bytes().all(lowercase_hex) {
        return Err("the treasury signer answered no public key".into());
    }
    let public_key = fixed_hex::<32>("treasury public key", text)?;
    let verifying_key = VerifyingKey::from_bytes(&public_key)
        .map_err(|_| "the treasury signer public key is not an Ed25519 key".to_owned())?;
    if verifying_key.is_weak() {
        return Err("the treasury signer public key is weak".into());
    }
    Ok(public_key)
}

fn lowercase_hex(byte: u8) -> bool {
    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
}

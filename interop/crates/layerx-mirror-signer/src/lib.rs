//! Reference remote signer for the `LayerX` mirror publisher.
//!
//! Serves `interop/deploy/mirror/signer-protocol.md` over a Unix domain socket
//! for the two publisher identities the mirror holds: a recoverable low-S
//! secp256k1 key for Ethereum and an Ed25519 key for Solana. Each handle is
//! bound to one algorithm and one policy domain, so a request that carries the
//! wrong handle, algorithm or domain is refused rather than signed.

use std::fmt;
use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process;
use std::time::Duration;

use ed25519_dalek::{Signer as _, SigningKey as Ed25519SigningKey};
use k256::ecdsa::SigningKey as Secp256k1SigningKey;
use sha2::{Digest as _, Sha256};
use zeroize::Zeroizing;

/// Policy domain the mirror sends with every Ethereum transaction digest.
pub const ETHEREUM_POLICY_DOMAIN: &[u8] = b"LayerX/mirror/ethereum-eip1559/v1";
/// Policy domain the mirror sends with every Solana transaction message.
pub const SOLANA_POLICY_DOMAIN: &[u8] = b"LayerX/mirror/solana-transaction/v1";
/// Socket the co-located publisher container reaches the signer through.
pub const DEFAULT_SOCKET: &str = "/run/mirror-signer/signer.sock";
/// Ethereum secp256k1 private key file of the `layerx-mirror-signer` secret.
pub const DEFAULT_ETHEREUM_KEY_FILE: &str = "/etc/layerx/mirror-signer/ethereum.key";
/// Solana Ed25519 keypair file of the `layerx-mirror-signer` secret.
pub const DEFAULT_SOLANA_KEYPAIR_FILE: &str = "/etc/layerx/mirror-signer/solana.json";
/// Handle the rendered mirror configuration carries for the Ethereum key.
pub const DEFAULT_ETHEREUM_KEY_HANDLE: &str = "mirror/ethereum/beta";
/// Handle the rendered mirror configuration carries for the Solana key.
pub const DEFAULT_SOLANA_KEY_HANDLE: &str = "mirror/solana/beta";
/// Environment variable naming the socket the signer publishes.
pub const SOCKET_VARIABLE: &str = "LAYERX_MIRROR_SIGNER_SOCKET";
/// Environment variable naming the Ethereum private key file.
pub const ETHEREUM_KEY_FILE_VARIABLE: &str = "LAYERX_MIRROR_SIGNER_ETHEREUM_KEY_FILE";
/// Environment variable naming the Solana keypair file.
pub const SOLANA_KEY_FILE_VARIABLE: &str = "LAYERX_MIRROR_SIGNER_SOLANA_KEY_FILE";
/// Environment variable naming the handle the Ethereum key answers to.
pub const ETHEREUM_KEY_HANDLE_VARIABLE: &str = "LAYERX_MIRROR_SIGNER_ETHEREUM_KEY_HANDLE";
/// Environment variable naming the handle the Solana key answers to.
pub const SOLANA_KEY_HANDLE_VARIABLE: &str = "LAYERX_MIRROR_SIGNER_SOLANA_KEY_HANDLE";
/// Command line the binary accepts. Every option also has an environment
/// variable, and every option has a container default, so the deployed signer
/// runs with no arguments at all.
pub const USAGE: &str = "usage: layerx-mirror-signer [--socket PATH] [--ethereum-key PATH] \
[--ethereum-handle NAME] [--solana-keypair PATH] [--solana-handle NAME]";

const MAGIC: &[u8; 4] = b"LXCS";
const VERSION: u16 = 1;
const ALGORITHM_SECP256K1_RECOVERABLE: u8 = 1;
const ALGORITHM_ED25519: u8 = 2;
const MAX_HANDLE_BYTES: usize = 256;
const MAX_DOMAIN_BYTES: usize = 128;
const MAX_MESSAGE_BYTES: usize = 4096;
const MAX_KEY_FILE_BYTES: u64 = 4096;
const MAX_REQUEST_BYTES: usize =
    4 + 2 + 1 + 2 + MAX_HANDLE_BYTES + 2 + MAX_DOMAIN_BYTES + 32 + 4 + MAX_MESSAGE_BYTES;
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(30);
/// The publisher runs beside the signer under a shared group on a private
/// in-memory volume, so the socket is reachable by its owner and that group
/// and by nobody else.
const SOCKET_MODE: u32 = 0o660;

/// Startup refusal. The signer never starts with key material or a socket it
/// cannot vouch for.
#[derive(Debug)]
pub enum StartupError {
    /// The command line is not the one `USAGE` describes.
    Usage(String),
    /// A key file is unreadable, too permissive or not the declared key.
    KeyMaterial {
        /// Key file the refusal is about.
        path: PathBuf,
        /// What the signer observed.
        reason: String,
    },
    /// The listening socket could not be published with owner and group access.
    Socket {
        /// Socket path the refusal is about.
        path: PathBuf,
        /// What the signer observed.
        reason: String,
    },
}

impl fmt::Display for StartupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(reason) => write!(formatter, "{reason}\n{USAGE}"),
            Self::KeyMaterial { path, reason } => {
                write!(formatter, "key material {}: {reason}", path.display())
            }
            Self::Socket { path, reason } => {
                write!(formatter, "socket {}: {reason}", path.display())
            }
        }
    }
}

/// Where the signer listens and which key file answers to which handle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Options {
    /// Unix domain socket the publisher connects to.
    pub socket: PathBuf,
    /// Ethereum secp256k1 private key file, 32 bytes of hexadecimal.
    pub ethereum_key_file: PathBuf,
    /// Handle the Ethereum key answers to.
    pub ethereum_key_handle: String,
    /// Solana keypair file, the 64-byte JSON array `solana-keygen` writes.
    pub solana_keypair_file: PathBuf,
    /// Handle the Solana key answers to.
    pub solana_key_handle: String,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            socket: PathBuf::from(DEFAULT_SOCKET),
            ethereum_key_file: PathBuf::from(DEFAULT_ETHEREUM_KEY_FILE),
            ethereum_key_handle: DEFAULT_ETHEREUM_KEY_HANDLE.to_owned(),
            solana_keypair_file: PathBuf::from(DEFAULT_SOLANA_KEYPAIR_FILE),
            solana_key_handle: DEFAULT_SOLANA_KEY_HANDLE.to_owned(),
        }
    }
}

fn environment(variable: &str) -> Option<String> {
    std::env::var(variable)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

impl Options {
    /// Reads the deployment options the mirror pod sets, leaving every unset
    /// option at its container default.
    #[must_use]
    pub fn from_environment() -> Self {
        let mut options = Self::default();
        if let Some(value) = environment(SOCKET_VARIABLE) {
            options.socket = PathBuf::from(value);
        }
        if let Some(value) = environment(ETHEREUM_KEY_FILE_VARIABLE) {
            options.ethereum_key_file = PathBuf::from(value);
        }
        if let Some(value) = environment(SOLANA_KEY_FILE_VARIABLE) {
            options.solana_keypair_file = PathBuf::from(value);
        }
        if let Some(value) = environment(ETHEREUM_KEY_HANDLE_VARIABLE) {
            options.ethereum_key_handle = value;
        }
        if let Some(value) = environment(SOLANA_KEY_HANDLE_VARIABLE) {
            options.solana_key_handle = value;
        }
        options
    }

    /// Parses the command line over the deployment environment, leaving every
    /// unset option at the container default the mirror pod is deployed with.
    ///
    /// # Errors
    /// Returns an error for an unknown flag, a flag without a value or a key
    /// handle the protocol cannot carry.
    pub fn parse<I: IntoIterator<Item = String>>(arguments: I) -> Result<Self, StartupError> {
        let mut options = Self::from_environment();
        let mut arguments = arguments.into_iter();
        while let Some(argument) = arguments.next() {
            let Some(value) = arguments.next() else {
                return Err(StartupError::Usage(format!("{argument} takes a value")));
            };
            match argument.as_str() {
                "--socket" => options.socket = PathBuf::from(value),
                "--ethereum-key" => options.ethereum_key_file = PathBuf::from(value),
                "--ethereum-handle" => options.ethereum_key_handle = value,
                "--solana-keypair" => options.solana_keypair_file = PathBuf::from(value),
                "--solana-handle" => options.solana_key_handle = value,
                other => {
                    return Err(StartupError::Usage(format!("unknown argument {other}")));
                }
            }
        }
        for handle in [&options.ethereum_key_handle, &options.solana_key_handle] {
            if handle.is_empty()
                || handle.len() > MAX_HANDLE_BYTES
                || handle.as_bytes().contains(&0)
            {
                return Err(StartupError::Usage(format!(
                    "key handle {handle:?} is empty, longer than {MAX_HANDLE_BYTES} bytes or carries a NUL"
                )));
            }
        }
        if options.ethereum_key_handle == options.solana_key_handle {
            return Err(StartupError::Usage(
                "the Ethereum and Solana key handles must differ".to_owned(),
            ));
        }
        Ok(options)
    }
}

enum ChainKey {
    Secp256k1(Box<Secp256k1SigningKey>),
    Ed25519(Box<Ed25519SigningKey>),
}

impl ChainKey {
    const fn algorithm(&self) -> u8 {
        match self {
            Self::Secp256k1(_) => ALGORITHM_SECP256K1_RECOVERABLE,
            Self::Ed25519(_) => ALGORITHM_ED25519,
        }
    }
}

struct KeyPolicy {
    handle: String,
    domain: &'static [u8],
    key: ChainKey,
}

struct Request<'a> {
    algorithm: u8,
    handle: &'a str,
    domain: &'a [u8],
    digest: [u8; 32],
    message: &'a [u8],
}

fn take<'a>(input: &'a [u8], offset: &mut usize, length: usize) -> Option<&'a [u8]> {
    let end = offset.checked_add(length)?;
    let slice = input.get(*offset..end)?;
    *offset = end;
    Some(slice)
}

fn take_u16(input: &[u8], offset: &mut usize) -> Option<u16> {
    let raw: [u8; 2] = take(input, offset, 2)?.try_into().ok()?;
    Some(u16::from_be_bytes(raw))
}

impl<'a> Request<'a> {
    fn parse(input: &'a [u8]) -> Option<Self> {
        let mut offset = 0;
        if take(input, &mut offset, 4)? != MAGIC {
            return None;
        }
        if take_u16(input, &mut offset)? != VERSION {
            return None;
        }
        let algorithm = *take(input, &mut offset, 1)?.first()?;
        let handle_length = usize::from(take_u16(input, &mut offset)?);
        if handle_length == 0 || handle_length > MAX_HANDLE_BYTES {
            return None;
        }
        let handle = std::str::from_utf8(take(input, &mut offset, handle_length)?).ok()?;
        if handle.as_bytes().contains(&0) {
            return None;
        }
        let domain_length = usize::from(take_u16(input, &mut offset)?);
        if domain_length == 0 || domain_length > MAX_DOMAIN_BYTES {
            return None;
        }
        let domain = take(input, &mut offset, domain_length)?;
        if domain.contains(&0) || std::str::from_utf8(domain).is_err() {
            return None;
        }
        let digest: [u8; 32] = take(input, &mut offset, 32)?.try_into().ok()?;
        let message_length: [u8; 4] = take(input, &mut offset, 4)?.try_into().ok()?;
        let message_length = usize::try_from(u32::from_be_bytes(message_length)).ok()?;
        if message_length > MAX_MESSAGE_BYTES {
            return None;
        }
        let message = take(input, &mut offset, message_length)?;
        if offset != input.len() {
            return None;
        }
        Some(Self {
            algorithm,
            handle,
            domain,
            digest,
            message,
        })
    }
}

/// The loaded publisher keys and the policy each one is bound to.
pub struct SignerService {
    keys: Vec<KeyPolicy>,
}

impl SignerService {
    /// Loads both publisher keys from the files the mirror signer secret
    /// carries and binds each one to its algorithm and policy domain.
    ///
    /// # Errors
    /// Returns an error when a key file is missing, writable by anyone but its
    /// owner, malformed, or carries a public half that does not match its
    /// secret half.
    pub fn load(options: &Options) -> Result<Self, StartupError> {
        let ethereum = load_secp256k1(&options.ethereum_key_file)?;
        let solana = load_ed25519(&options.solana_keypair_file)?;
        Ok(Self {
            keys: vec![
                KeyPolicy {
                    handle: options.ethereum_key_handle.clone(),
                    domain: ETHEREUM_POLICY_DOMAIN,
                    key: ChainKey::Secp256k1(Box::new(ethereum)),
                },
                KeyPolicy {
                    handle: options.solana_key_handle.clone(),
                    domain: SOLANA_POLICY_DOMAIN,
                    key: ChainKey::Ed25519(Box::new(solana)),
                },
            ],
        })
    }

    fn sign(&self, request: &Request<'_>) -> Option<Vec<u8>> {
        let policy = self
            .keys
            .iter()
            .find(|policy| policy.handle == request.handle)?;
        if policy.key.algorithm() != request.algorithm || policy.domain != request.domain {
            return None;
        }
        match &policy.key {
            ChainKey::Secp256k1(key) => {
                if !request.message.is_empty() {
                    return None;
                }
                let (signature, recovery) = key.sign_prehash_recoverable(&request.digest).ok()?;
                let recovery = u8::from(recovery);
                if recovery > 1 {
                    return None;
                }
                let mut output = Vec::with_capacity(65);
                output.extend_from_slice(&signature.to_bytes());
                output.push(recovery);
                Some(output)
            }
            ChainKey::Ed25519(key) => {
                if request.message.is_empty() {
                    return None;
                }
                let digest: [u8; 32] = Sha256::digest(request.message).into();
                if digest != request.digest {
                    return None;
                }
                Some(key.sign(request.message).to_bytes().to_vec())
            }
        }
    }

    fn answer(&self, mut stream: UnixStream) -> Result<(), std::io::Error> {
        let mut framing = [0_u8; 4];
        stream.read_exact(&mut framing)?;
        let length = usize::try_from(u32::from_be_bytes(framing))
            .map_err(|_| malformed("the request frame does not fit this platform"))?;
        if length == 0 || length > MAX_REQUEST_BYTES {
            return Err(malformed("the request frame is out of bounds"));
        }
        let mut request = vec![0_u8; length];
        stream.read_exact(&mut request)?;
        let signature = Request::parse(&request).and_then(|request| self.sign(&request));
        let mut response = Vec::with_capacity(66);
        match signature {
            Some(signature) => {
                response.push(0);
                response.extend_from_slice(&signature);
            }
            None => response.push(1),
        }
        let length = u32::try_from(response.len())
            .map_err(|_| malformed("the response frame does not fit the protocol"))?;
        stream.write_all(&length.to_be_bytes())?;
        stream.write_all(&response)?;
        stream.flush()
    }
}

/// A bound signer socket together with the keys it answers with.
pub struct SignerListener {
    listener: UnixListener,
    service: SignerService,
    socket: PathBuf,
}

impl SignerListener {
    /// Loads the keys and publishes a socket reachable by its owner and group at the configured
    /// path, replacing a socket left behind by an earlier run.
    ///
    /// # Errors
    /// Returns an error when the key material is refused, the socket directory
    /// is missing, the path is occupied by something other than a socket, or
    /// the socket cannot be published with `0660` access.
    pub fn bind(options: &Options) -> Result<Self, StartupError> {
        let service = SignerService::load(options)?;
        let socket = options.socket.clone();
        let refuse = |reason: String| StartupError::Socket {
            path: socket.clone(),
            reason,
        };
        let (Some(parent), Some(name)) = (socket.parent(), socket.file_name()) else {
            return Err(refuse("is not a path inside a directory".to_owned()));
        };
        if !parent.is_dir() {
            return Err(refuse(format!(
                "directory {} does not exist",
                parent.display()
            )));
        }
        match fs::symlink_metadata(&socket) {
            Ok(metadata) if metadata.file_type().is_socket() => {}
            Ok(_) => return Err(refuse("exists and is not a socket".to_owned())),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(refuse(format!("cannot be inspected: {error}"))),
        }
        let staging = parent.join(format!(
            ".{}.{}.staging",
            name.to_string_lossy(),
            process::id()
        ));
        let _ = fs::remove_file(&staging);
        let listener = UnixListener::bind(&staging)
            .map_err(|error| refuse(format!("cannot bind: {error}")))?;
        let published = fs::set_permissions(&staging, fs::Permissions::from_mode(SOCKET_MODE))
            .and_then(|()| fs::rename(&staging, &socket))
            .and_then(|()| fs::metadata(&socket));
        let metadata = match published {
            Ok(metadata) => metadata,
            Err(error) => {
                let _ = fs::remove_file(&staging);
                return Err(refuse(format!("cannot be published: {error}")));
            }
        };
        let mode = metadata.permissions().mode() & 0o777;
        if mode != SOCKET_MODE {
            let _ = fs::remove_file(&socket);
            return Err(refuse(format!(
                "was published with mode {mode:04o} instead of {SOCKET_MODE:04o}"
            )));
        }
        Ok(Self {
            listener,
            service,
            socket,
        })
    }

    /// Socket the publisher reaches this signer through.
    #[must_use]
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// Answers one connection. A client that sends an unusable frame is
    /// disconnected without a response; every frame the protocol can parse is
    /// answered with a signature or a refusal.
    ///
    /// # Errors
    /// Returns an error when the connection cannot be accepted, read or
    /// answered.
    pub fn serve_once(&self) -> Result<(), std::io::Error> {
        let (stream, _) = self.listener.accept()?;
        stream.set_read_timeout(Some(CONNECTION_TIMEOUT))?;
        stream.set_write_timeout(Some(CONNECTION_TIMEOUT))?;
        self.service.answer(stream)
    }

    /// Answers connections until the process is stopped. A failed connection
    /// is reported without key material and never ends the loop.
    pub fn serve(&self) -> ! {
        loop {
            if let Err(error) = self.serve_once() {
                eprintln!("layerx-mirror-signer: connection failed: {error}");
            }
        }
    }
}

fn malformed(reason: &'static str) -> std::io::Error {
    std::io::Error::new(ErrorKind::InvalidData, reason)
}

fn key_file_bytes(path: &Path) -> Result<Zeroizing<Vec<u8>>, StartupError> {
    let refuse = |reason: String| StartupError::KeyMaterial {
        path: path.to_path_buf(),
        reason,
    };
    let metadata = fs::metadata(path).map_err(|error| refuse(format!("is unreadable: {error}")))?;
    if !metadata.is_file() {
        return Err(refuse("is not a regular file".to_owned()));
    }
    if metadata.permissions().mode() & 0o022 != 0 {
        return Err(refuse("is writable by group or other".to_owned()));
    }
    if metadata.len() > MAX_KEY_FILE_BYTES {
        return Err(refuse(format!("is larger than {MAX_KEY_FILE_BYTES} bytes")));
    }
    let bytes = fs::read(path).map_err(|error| refuse(format!("cannot be read: {error}")))?;
    Ok(Zeroizing::new(bytes))
}

fn load_secp256k1(path: &Path) -> Result<Secp256k1SigningKey, StartupError> {
    let refuse = |reason: String| StartupError::KeyMaterial {
        path: path.to_path_buf(),
        reason,
    };
    let bytes = key_file_bytes(path)?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| refuse("is not a hexadecimal secp256k1 private key".to_owned()))?;
    let text = text.trim();
    let text = text.strip_prefix("0x").unwrap_or(text);
    if text.len() != 64 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(refuse(
            "is not 32 hexadecimal bytes of secp256k1 private key".to_owned(),
        ));
    }
    let mut raw = Zeroizing::new([0_u8; 32]);
    for index in 0..32 {
        let (Some(pair), Some(slot)) = (text.get(index * 2..index * 2 + 2), raw.get_mut(index))
        else {
            return Err(refuse("is not 32 hexadecimal bytes".to_owned()));
        };
        *slot =
            u8::from_str_radix(pair, 16).map_err(|_| refuse("is not hexadecimal".to_owned()))?;
    }
    Secp256k1SigningKey::from_slice(raw.as_ref())
        .map_err(|_| refuse("is not a secp256k1 scalar".to_owned()))
}

fn load_ed25519(path: &Path) -> Result<Ed25519SigningKey, StartupError> {
    let refuse = |reason: String| StartupError::KeyMaterial {
        path: path.to_path_buf(),
        reason,
    };
    let bytes = key_file_bytes(path)?;
    let values: Zeroizing<Vec<u8>> = serde_json::from_slice(&bytes)
        .map(Zeroizing::new)
        .map_err(|_| refuse("is not a JSON array of keypair bytes".to_owned()))?;
    if values.len() != 64 {
        return Err(refuse(format!(
            "carries {} bytes instead of a 64-byte keypair",
            values.len()
        )));
    }
    let (Some(secret), Some(public)) = (values.get(..32), values.get(32..)) else {
        return Err(refuse("is not a 64-byte keypair".to_owned()));
    };
    let secret: [u8; 32] = secret
        .try_into()
        .map_err(|_| refuse("carries no 32-byte secret half".to_owned()))?;
    let secret = Zeroizing::new(secret);
    let key = Ed25519SigningKey::from_bytes(&secret);
    let verifying = key.verifying_key();
    if verifying.to_bytes().as_slice() != public {
        return Err(refuse(
            "carries a public half that its secret half does not produce".to_owned(),
        ));
    }
    if verifying.is_weak() {
        return Err(refuse("carries a small-order public key".to_owned()));
    }
    Ok(key)
}

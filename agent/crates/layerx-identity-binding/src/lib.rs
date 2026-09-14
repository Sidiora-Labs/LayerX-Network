#![forbid(unsafe_code)]

use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use layerx_types::ids::Did;
use serde::Deserialize;
use sha2::{Digest, Sha256};

const MAX_FRAME: usize = 2048;
const MAGIC: &[u8; 5] = b"LXIB\x01";

#[derive(Clone, Debug)]
pub struct Config {
    pub socket: PathBuf,
    pub tenant: String,
    pub peer_uid: u32,
    pub peer_gid: u32,
    pub deadline: Duration,
}

#[derive(Clone, Debug)]
pub struct Client {
    config: Config,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Binding {
    tenant: String,
    principal: String,
    did: Did,
    agent_tenant: String,
}

impl Binding {
    #[must_use]
    pub fn tenant(&self) -> &str {
        &self.tenant
    }
    #[must_use]
    pub fn principal(&self) -> &str {
        &self.principal
    }
    #[must_use]
    pub const fn did(&self) -> &Did {
        &self.did
    }
    #[must_use]
    pub fn agent_tenant(&self) -> &str {
        &self.agent_tenant
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Response {
    status: String,
    #[serde(default)]
    tenant: Option<String>,
    #[serde(default)]
    principal: Option<String>,
    #[serde(default)]
    did: Option<String>,
}

/// Derives a storage partition without accepting a caller-supplied tenant.
///
/// # Errors
/// Refuses empty, control-bearing or overlong tenant and principal names.
pub fn subject_namespace(tenant: &str, principal: &str) -> io::Result<String> {
    valid_text(tenant)?;
    valid_text(principal)?;
    let mut digest = Sha256::new();
    digest.update(b"layerx-human/subject-tenant/v1\0");
    for value in [tenant, principal] {
        digest.update(
            u16::try_from(value.len())
                .map_err(|_| invalid("subject length"))?
                .to_be_bytes(),
        );
        digest.update(value.as_bytes());
    }
    let digest: [u8; 32] = digest.finalize().into();
    let mut namespace = String::from("human-v1:");
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    for byte in digest {
        namespace.push(char::from(DIGITS[usize::from(byte >> 4)]));
        namespace.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    Ok(namespace)
}

impl Client {
    /// Pins the provider identity and one service tenant.
    ///
    /// # Errors
    /// Refuses noncanonical socket paths, invalid text and unbounded deadlines.
    pub fn new(config: Config) -> io::Result<Self> {
        valid_text(&config.tenant)?;
        if !config.socket.is_absolute()
            || config.deadline.is_zero()
            || config.deadline > Duration::from_secs(60)
        {
            return Err(invalid("binding configuration"));
        }
        protected_parent(&config.socket, config.peer_uid)?;
        Ok(Self { config })
    }

    /// Reads only the provider's durable principal-to-DID binding.
    ///
    /// # Errors
    /// Refuses unknown principals, transport identity changes, malformed responses and cross-tenant bindings.
    pub fn lookup(&self, principal: &str) -> io::Result<Binding> {
        valid_text(principal)?;
        let request = serde_json::to_vec(
            &serde_json::json!({"operation":"principal", "tenant":self.config.tenant, "principal":principal}),
        )?;
        let mut frame = MAGIC.to_vec();
        frame.extend(request);
        let expires = Instant::now()
            .checked_add(self.config.deadline)
            .ok_or_else(|| invalid("binding deadline"))?;
        let mut stream = self.connect(expires)?;
        let mut encoded = u32::try_from(frame.len())
            .map_err(|_| invalid("frame length"))?
            .to_be_bytes()
            .to_vec();
        encoded.extend(frame);
        write_before(&mut stream, &encoded, expires)?;
        let mut length = [0; 4];
        read_before(&mut stream, &mut length, expires)?;
        let length = u32::from_be_bytes(length) as usize;
        if !(6..=MAX_FRAME).contains(&length) {
            return Err(invalid("binding response length"));
        }
        let mut bytes = vec![0; length];
        read_before(&mut stream, &mut bytes, expires)?;
        if &bytes[..5] != MAGIC {
            return Err(invalid("binding response version"));
        }
        let response: Response =
            serde_json::from_slice(&bytes[5..]).map_err(|_| invalid("binding response"))?;
        if response.status != "bound"
            || response.tenant.as_deref() != Some(self.config.tenant.as_str())
            || response.principal.as_deref() != Some(principal)
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "principal binding refused",
            ));
        }
        let did = response.did.ok_or_else(|| invalid("binding DID missing"))?;
        valid_text(&did)?;
        let did = Did::new(did.as_bytes()).map_err(|_| invalid("binding DID invalid"))?;
        Ok(Binding {
            tenant: self.config.tenant.clone(),
            principal: principal.to_owned(),
            did,
            agent_tenant: subject_namespace(&self.config.tenant, principal)?,
        })
    }

    fn connect(&self, expires: Instant) -> io::Result<UnixStream> {
        protected_parent(&self.config.socket, self.config.peer_uid)?;
        let before = socket_identity(&self.config)?;
        let socket = rustix::net::socket_with(
            rustix::net::AddressFamily::UNIX,
            rustix::net::SocketType::STREAM,
            rustix::net::SocketFlags::NONBLOCK | rustix::net::SocketFlags::CLOEXEC,
            None,
        )?;
        let address = rustix::net::SocketAddrUnix::new(&self.config.socket)?;
        loop {
            match rustix::net::connect(&socket, &address) {
                Ok(()) => break,
                Err(error)
                    if error == rustix::io::Errno::AGAIN || error == rustix::io::Errno::INTR =>
                {
                    std::thread::sleep(remaining(expires)?.min(Duration::from_millis(5)))
                }
                Err(error) => return Err(error.into()),
            }
        }
        let stream = UnixStream::from(socket);
        stream.set_nonblocking(false)?;
        let peer = rustix::net::sockopt::socket_peercred(&stream)?;
        if peer.uid.as_raw() != self.config.peer_uid
            || peer.gid.as_raw() != self.config.peer_gid
            || socket_identity(&self.config)? != before
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "binding peer differs",
            ));
        }
        Ok(stream)
    }
}

fn socket_identity(config: &Config) -> io::Result<(u64, u64)> {
    let metadata = fs::symlink_metadata(&config.socket)?;
    if !metadata.file_type().is_socket()
        || metadata.uid() != config.peer_uid
        || metadata.gid() != config.peer_gid
        || metadata.mode() & 0o007 != 0
    {
        return Err(invalid("unprotected binding socket"));
    }
    Ok((metadata.dev(), metadata.ino()))
}

fn protected_parent(socket: &Path, owner: u32) -> io::Result<()> {
    let parent = socket.parent().ok_or_else(|| invalid("binding parent"))?;
    let metadata = fs::symlink_metadata(parent)?;
    if fs::canonicalize(parent)? != parent
        || !metadata.is_dir()
        || metadata.uid() != owner
        || metadata.mode() & 0o022 != 0
    {
        return Err(invalid("unprotected binding directory"));
    }
    Ok(())
}

fn valid_text(value: &str) -> io::Result<()> {
    if value.is_empty() || value.len() > 255 || value.chars().any(char::is_control) {
        return Err(invalid("binding text"));
    }
    Ok(())
}

fn remaining(expires: Instant) -> io::Result<Duration> {
    expires
        .checked_duration_since(Instant::now())
        .filter(|value| !value.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "binding deadline"))
}

fn read_before(stream: &mut UnixStream, mut bytes: &mut [u8], expires: Instant) -> io::Result<()> {
    while !bytes.is_empty() {
        stream.set_read_timeout(Some(remaining(expires)?))?;
        match stream.read(bytes) {
            Ok(0) => return Err(io::Error::from(io::ErrorKind::UnexpectedEof)),
            Ok(count) => bytes = &mut bytes[count..],
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn write_before(stream: &mut UnixStream, mut bytes: &[u8], expires: Instant) -> io::Result<()> {
    while !bytes.is_empty() {
        stream.set_write_timeout(Some(remaining(expires)?))?;
        match stream.write(bytes) {
            Ok(0) => return Err(io::Error::from(io::ErrorKind::WriteZero)),
            Ok(count) => bytes = &bytes[count..],
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

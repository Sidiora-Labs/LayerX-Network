use super::super::ComponentShutdown;
use super::*;
use crate::custody::SettlementRecipientRequest;
use crate::store::PrincipalId;
use layerx_types::clock::{Clock, Deadline};
use rustix::net::{
    self, AddressFamily, RecvFlags, SendFlags, SocketAddrUnix, SocketFlags, SocketType,
};
use serde::Deserialize;
use std::io;
use std::os::fd::OwnedFd;
use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _, PermissionsExt as _};

const MAX_PACKET: usize = 8192;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Packet {
    version: u8,
    operation: String,
    body: Request,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    principal: String,
    asset: [u8; 32],
    checkpoint: [u8; 32],
}

pub struct RecipientServerConfig {
    pub socket: PathBuf,
    pub caller_uid: u32,
    pub caller_gid: u32,
    pub deadline: Duration,
    pub clock: Arc<dyn Clock>,
}

pub struct RecipientServer {
    socket: OwnedFd,
    config: RecipientServerConfig,
    identity: (u64, u64),
    components: Arc<ProductionComponents>,
}

impl RecipientServer {
    /// # Errors
    /// Refuses unprotected sockets, invalid peers or an unbounded request policy.
    pub fn bind(
        components: Arc<ProductionComponents>,
        config: RecipientServerConfig,
    ) -> io::Result<Self> {
        let parent = config.socket.parent().ok_or_else(invalid)?;
        let info = fs::symlink_metadata(parent)?;
        if !config.socket.is_absolute()
            || fs::canonicalize(parent)? != parent
            || !info.is_dir()
            || info.uid() != rustix::process::geteuid().as_raw()
            || info.gid() != rustix::process::getegid().as_raw()
            || info.mode() & 0o027 != 0
            || config.caller_uid == 0
            || config.caller_uid == rustix::process::geteuid().as_raw()
            || config.deadline.is_zero()
            || config.deadline > Duration::from_secs(30)
        {
            return Err(invalid());
        }
        prepare_socket(&config.socket)?;
        let socket = net::socket_with(
            AddressFamily::UNIX,
            SocketType::SEQPACKET,
            SocketFlags::CLOEXEC,
            None,
        )?;
        net::bind(&socket, &SocketAddrUnix::new(&config.socket)?)?;
        fs::set_permissions(&config.socket, fs::Permissions::from_mode(0o660))?;
        let info = fs::symlink_metadata(&config.socket)?;
        net::listen(&socket, 4)?;
        net::sockopt::set_socket_timeout(
            &socket,
            net::sockopt::Timeout::Recv,
            Some(Duration::from_millis(100)),
        )?;
        Ok(Self {
            socket,
            config,
            identity: (info.dev(), info.ino()),
            components,
        })
    }

    /// # Errors
    /// Stops on listener failures; individual unauthorized requests are isolated.
    pub fn run(self, shutdown: &ComponentShutdown) -> io::Result<()> {
        while !shutdown.requested() {
            match net::accept_with(&self.socket, SocketFlags::CLOEXEC) {
                Ok(peer) => {
                    let _ = self.serve(&peer);
                }
                Err(rustix::io::Errno::AGAIN | rustix::io::Errno::INTR) => (),
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    fn serve(&self, peer: &OwnedFd) -> io::Result<()> {
        let mut deadline = Deadline::start(self.config.clock.as_ref(), self.config.deadline)
            .map_err(|_| invalid())?;
        let credentials = net::sockopt::socket_peercred(peer)?;
        if credentials.uid.as_raw() != self.config.caller_uid
            || credentials.gid.as_raw() != self.config.caller_gid
        {
            return Err(invalid());
        }
        net::sockopt::set_socket_timeout(
            peer,
            net::sockopt::Timeout::Recv,
            Some(self.remaining(&mut deadline)?),
        )?;
        let mut bytes = [0; MAX_PACKET];
        let (_, count) = net::recv(peer, &mut bytes[..], RecvFlags::TRUNC)?;
        if count == 0 || count > bytes.len() {
            return Err(invalid());
        }
        self.remaining(&mut deadline)?;
        let response = serde_json::from_slice::<Packet>(&bytes[..count])
            .ok()
            .filter(|packet| packet.version == 1 && packet.operation == "settlement-recipient")
            .and_then(|packet| self.components.sign_recipient(packet.body).ok());
        let value = response.map_or_else(
            || json!({"version": 1, "error": "recipient_request_refused"}),
            |result| json!({"version": 1, "result": result}),
        );
        let bytes = serde_json::to_vec(&value).map_err(|_| invalid())?;
        net::sockopt::set_socket_timeout(
            peer,
            net::sockopt::Timeout::Send,
            Some(self.remaining(&mut deadline)?),
        )?;
        if bytes.len() > MAX_PACKET || net::send(peer, &bytes, SendFlags::NOSIGNAL)? != bytes.len()
        {
            return Err(invalid());
        }
        Ok(())
    }

    fn remaining(&self, deadline: &mut Deadline) -> io::Result<Duration> {
        let remaining = deadline
            .remaining(self.config.clock.as_ref())
            .map_err(|_| invalid())?;
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "recipient operation deadline elapsed",
            ));
        }
        Ok(remaining)
    }
}

impl Drop for RecipientServer {
    fn drop(&mut self) {
        if let Ok(info) = fs::symlink_metadata(&self.config.socket) {
            if info.file_type().is_socket() && (info.dev(), info.ino()) == self.identity {
                let _ = fs::remove_file(&self.config.socket);
            }
        }
    }
}

impl ProductionComponents {
    fn sign_recipient(&self, request: Request) -> Result<serde_json::Value, ApiFailure> {
        let principal = PrincipalId::new(request.principal).map_err(|_| ApiFailure::forbidden())?;
        let mut store = self.store.lock().map_err(|_| ApiFailure::unavailable())?;
        let mut scope = store
            .principal(&principal)
            .map_err(|_| ApiFailure::forbidden())?;
        let asset = self
            .agent
            .lock()
            .map_err(|_| ApiFailure::unavailable())?
            .native_fee_policy()
            .map_err(agent_failure)?
            .asset_id;
        if request.asset != asset {
            return Err(ApiFailure::forbidden());
        }
        let key = KeyId::new("human-primary").map_err(|_| ApiFailure::forbidden())?;
        let recipient = self
            .custody
            .evm_wallet(&principal, &key)
            .map_err(|_| ApiFailure::forbidden())?;
        let trace = TraceId::mint(
            request.checkpoint[..16]
                .try_into()
                .map_err(|_| ApiFailure::forbidden())?,
        );
        let signature = self
            .custody
            .settlement_recipient_in_scope(
                &mut scope,
                &key,
                SettlementRecipientRequest {
                    checkpoint: request.checkpoint,
                    asset: request.asset,
                    recipient,
                },
                &trace,
                self.now()?,
            )
            .map_err(|_| ApiFailure::forbidden())?;
        let (actor, account) = movement_principal_account(&scope)?;
        let public_key = self
            .custody
            .describe_key(&principal, &key)
            .map_err(|_| ApiFailure::forbidden())?
            .public_key;
        Ok(
            json!({"network_id": self.network_id, "principal": principal.as_str(), "did": actor.as_str(),
            "account": hex_bytes(&layerx_intents::canonical::account_id_for_protocol(&account, 3).map_err(|_| ApiFailure::forbidden())?),
            "public_key": hex_bytes(&public_key), "asset": hex_bytes(&request.asset),
            "checkpoint": hex_bytes(&request.checkpoint), "recipient": hex_bytes(&recipient),
            "signature": hex_bytes(&signature)}),
        )
    }
}

fn prepare_socket(path: &std::path::Path) -> io::Result<()> {
    let info = match fs::symlink_metadata(path) {
        Ok(info) => info,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if !info.file_type().is_socket()
        || info.uid() != rustix::process::geteuid().as_raw()
        || info.gid() != rustix::process::getegid().as_raw()
        || info.mode() & 0o777 != 0o660
    {
        return Err(invalid());
    }
    let probe = net::socket_with(
        AddressFamily::UNIX,
        SocketType::SEQPACKET,
        SocketFlags::CLOEXEC | SocketFlags::NONBLOCK,
        None,
    )?;
    if net::connect(&probe, &SocketAddrUnix::new(path)?) != Err(rustix::io::Errno::CONNREFUSED) {
        return Err(io::Error::new(
            io::ErrorKind::AddrInUse,
            "recipient socket is live",
        ));
    }
    let current = fs::symlink_metadata(path)?;
    if (current.dev(), current.ino()) != (info.dev(), info.ino()) {
        return Err(invalid());
    }
    fs::remove_file(path)
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "recipient authority refused")
}

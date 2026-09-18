use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use layerx_human_security_provider::{Error, Result};
use layerx_human_service::server::production_auth::{
    RemoteSecurityProvider, SecurityProviderConfig,
};

const MAXIMUM_FRAME_BYTES: usize = 1_048_576;

pub(crate) struct Settings {
    pub socket: PathBuf,
    pub deadline: Duration,
}

impl Settings {
    pub fn from_environment() -> Result<Self> {
        let socket = std::env::var_os("LAYERX_HUMAN_SECURITY_PROVIDER_SOCKET")
            .map(PathBuf::from)
            .ok_or(Error::Configuration)?;
        let seconds = match std::env::var("LAYERX_HUMAN_SECURITY_PROVIDER_DEADLINE_SECONDS") {
            Ok(value) => value.parse::<u64>().map_err(|_| Error::Configuration)?,
            Err(std::env::VarError::NotPresent) => 5,
            Err(_) => return Err(Error::Configuration),
        };
        if !socket.is_absolute() || !(1..=60).contains(&seconds) {
            return Err(Error::Configuration);
        }
        Ok(Self {
            socket,
            deadline: Duration::from_secs(seconds),
        })
    }
}

pub(crate) fn run() -> Result<()> {
    let settings = Settings::from_environment()?;
    probe(&settings.socket, settings.deadline)
}

/// Performs the provider's own readiness operation against the live socket and
/// answers within `deadline`, whatever the transport does.
pub(crate) fn probe(socket: &Path, deadline: Duration) -> Result<()> {
    let provider = RemoteSecurityProvider::new(SecurityProviderConfig {
        socket: socket.to_owned(),
        deadline,
        maximum_frame_bytes: MAXIMUM_FRAME_BYTES,
    })
    .map_err(|_| Error::Configuration)?;
    let (finished, answer) = mpsc::sync_channel(1);
    thread::Builder::new().spawn(move || {
        let _ = finished.send(provider.probe().is_ok());
    })?;
    match answer.recv_timeout(deadline) {
        Ok(true) => Ok(()),
        Ok(false) | Err(_) => Err(Error::Refused),
    }
}

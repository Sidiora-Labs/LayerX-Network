use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use layerx_human_service::server::movement_provider::{
    MovementProviderCodec, MovementProviderRequest, MovementProviderResponse, NativeMovementCodec,
    MOVEMENT_PROTOCOL_VERSION,
};

use crate::config::{bounded, path, MAX_FRAME};
use crate::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Outcome {
    Ready,
    NotReady,
}

pub(crate) struct Settings {
    pub socket: PathBuf,
    pub deadline: Duration,
    pub maximum_frame_bytes: usize,
    pub protocol: u16,
}

impl Settings {
    /// Reads exactly the transport variables the serving path already reads.
    pub fn from_environment() -> Result<Self, Error> {
        Ok(Self {
            socket: path("SOCKET")?,
            deadline: Duration::from_secs(bounded("DEADLINE_SECONDS", 1, 60)?),
            maximum_frame_bytes: usize::try_from(bounded("MAX_FRAME_BYTES", 2, MAX_FRAME as u64)?)
                .map_err(|_| Error::Configuration)?,
            protocol: u16::try_from(bounded("PROTOCOL_VERSION", 2, 3)?)
                .map_err(|_| Error::Configuration)?,
        })
    }
}

pub(crate) fn run() -> Result<(), Error> {
    match probe(&Settings::from_environment()?)? {
        Outcome::Ready => Ok(()),
        Outcome::NotReady => Err(Error::Integrity),
    }
}

/// Asks the live provider its own readiness question and answers within
/// `settings.deadline` whatever the transport does.
pub(crate) fn probe(settings: &Settings) -> Result<Outcome, Error> {
    let socket = settings.socket.clone();
    let deadline = settings.deadline;
    let maximum_frame_bytes = settings.maximum_frame_bytes;
    let protocol = settings.protocol;
    let (finished, answer) = mpsc::sync_channel(1);
    thread::Builder::new().spawn(move || {
        let _ = finished.send(exchange(&socket, deadline, maximum_frame_bytes, protocol));
    })?;
    match answer.recv_timeout(deadline) {
        Ok(result) => result,
        Err(_) => Err(Error::Io(std::io::Error::from(
            std::io::ErrorKind::TimedOut,
        ))),
    }
}

fn exchange(
    socket: &Path,
    deadline: Duration,
    maximum_frame_bytes: usize,
    protocol: u16,
) -> Result<Outcome, Error> {
    let codec = NativeMovementCodec::for_protocol(protocol).map_err(|_| Error::Configuration)?;
    let payload = codec
        .encode_request(&MovementProviderRequest::Readiness)
        .map_err(|_| Error::Configuration)?;
    if payload.is_empty() || payload.len() > maximum_frame_bytes {
        return Err(Error::Configuration);
    }
    let mut stream = UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(deadline))?;
    stream.set_write_timeout(Some(deadline))?;
    let mut frame = MOVEMENT_PROTOCOL_VERSION.to_be_bytes().to_vec();
    frame.extend(
        u64::try_from(payload.len())
            .map_err(|_| Error::Configuration)?
            .to_be_bytes(),
    );
    frame.extend(payload);
    stream.write_all(&frame)?;
    stream.flush()?;
    let mut header = [0; 10];
    stream.read_exact(&mut header)?;
    if u16::from_be_bytes([header[0], header[1]]) != MOVEMENT_PROTOCOL_VERSION {
        return Err(Error::Integrity);
    }
    let length = usize::try_from(u64::from_be_bytes(
        header[2..10].try_into().map_err(|_| Error::Integrity)?,
    ))
    .map_err(|_| Error::Integrity)?;
    if length == 0 || length > maximum_frame_bytes {
        return Err(Error::Integrity);
    }
    let mut response = vec![0; length];
    stream.read_exact(&mut response)?;
    interpret(protocol, &response)
}

/// Only the provider's own `Ready` answer counts as ready.
pub(crate) fn interpret(protocol: u16, response: &[u8]) -> Result<Outcome, Error> {
    let codec = NativeMovementCodec::for_protocol(protocol).map_err(|_| Error::Configuration)?;
    match codec.decode_response(response) {
        Ok(MovementProviderResponse::Ready) => Ok(Outcome::Ready),
        Ok(_) => Ok(Outcome::NotReady),
        Err(_) => Err(Error::Integrity),
    }
}

use crate::config::Config;
use crate::store::Store;
use crate::wire::{self, Error, Request, Result};
use layerx_client::lni::framing::{read_frame, write_frame};
use rustls::{ServerConnection, StreamOwned};
use sha2::{Digest, Sha256};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use zeroize::Zeroizing;

struct Permit(Arc<AtomicUsize>);
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
pub(crate) fn run(config: Config) -> std::result::Result<(), String> {
    let store = Arc::new(Mutex::new(Store::open(&config)?));
    let listener = TcpListener::bind(config.listen).map_err(|_| "KMS listener unavailable")?;
    let config = Arc::new(config);
    let active = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        let Ok(stream) = stream else {
            return Err("KMS accept failed".into());
        };
        if active
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < 16).then_some(count + 1)
            })
            .is_err()
        {
            continue;
        }
        let permit = Permit(Arc::clone(&active));
        let config = Arc::clone(&config);
        let store = Arc::clone(&store);
        thread::Builder::new()
            .name("human-kms".into())
            .spawn(move || {
                let _permit = permit;
                let _ = connection(stream, &config, &store);
            })
            .map_err(|_| "KMS worker unavailable")?;
    }
    Err("KMS listener stopped".into())
}
fn connection(mut tcp: TcpStream, config: &Config, store: &Mutex<Store>) -> Result<()> {
    tcp.set_read_timeout(Some(config.deadline))
        .map_err(|_| Error::Unavailable)?;
    tcp.set_write_timeout(Some(config.deadline))
        .map_err(|_| Error::Unavailable)?;
    let mut connection =
        ServerConnection::new(Arc::clone(&config.tls)).map_err(|_| Error::Unavailable)?;
    while connection.is_handshaking() {
        connection
            .complete_io(&mut tcp)
            .map_err(|_| Error::Refused)?;
    }
    let leaf = connection
        .peer_certificates()
        .and_then(|chain| chain.first())
        .ok_or(Error::Refused)?;
    let observed: [u8; 32] = Sha256::digest(leaf.as_ref()).into();
    if !layerx_crypto::ct::eq_fixed(&observed, &config.client_pin) {
        return Err(Error::Refused);
    }
    let mut tls = StreamOwned::new(connection, tcp);
    let frame = Zeroizing::new(read_frame(&mut tls, wire::MAX_FRAME).map_err(|_| Error::Refused)?);
    let request = Request::decode(&frame)?;
    let answer = validate_sign(&request, config).and_then(|digest| {
        store
            .lock()
            .map_err(|_| Error::Unavailable)?
            .dispatch(&request, digest)
    });
    write_frame(
        &mut tls,
        &wire::response(request.version, request.operation, answer),
        wire::MAX_FRAME,
    )
    .map_err(|_| Error::Unavailable)
}
fn validate_sign(request: &Request<'_>, config: &Config) -> Result<Option<[u8; 32]>> {
    if request.operation != 5 {
        return Ok(None);
    }
    let activity = layerx_wire::activity::decode_unsigned(request.canonical, &config.registry)
        .map_err(|_| Error::Refused)?;
    if activity.network_id() != config.network
        || activity.protocol_version() != config.protocol
        || request.network != config.network
    {
        return Err(Error::Refused);
    }
    let disclosure = layerx_crypto::disclosure::bind(request.canonical, &config.registry)
        .map_err(|_| Error::Refused)?;
    if disclosure.reencode().map_err(|_| Error::Refused)? != request.canonical
        || wire::disclosure(&disclosure)? != request.disclosure
    {
        return Err(Error::Refused);
    }
    let mut hash = Sha256::new();
    hash.update(b"LXP/v1/signature-preimage\0");
    hash.update(request.canonical);
    let digest: [u8; 32] = hash.finalize().into();
    if !layerx_crypto::ct::eq_fixed(&digest, &request.digest) {
        return Err(Error::Integrity);
    }
    Ok(Some(digest))
}

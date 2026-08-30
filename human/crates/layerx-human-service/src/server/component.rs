use std::fs;
use std::io;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use layerx_client::lni::framing::{read_frame, write_frame};
use layerx_client::lni::transport::Limits;
use rustix::net::sockopt::socket_peercred;
use rustix::net::{recv, RecvFlags};
use zeroize::Zeroize;

use crate::store::{AgentTenantId, PrincipalId};

use super::backend::{
    component_owner, ApiFailure, HumanApiComponents, PrincipalContext, ScopedRequest,
    SessionCredentials,
};
use super::component_protocol::{
    authorized_request_digest, encode_authorized, encode_backend, encode_failure, encode_readiness,
    json_digest, parse_digest, validate_execute, ComponentRequest, WirePrincipal,
};
use super::schema::ApiSchema;

const ACCEPT_POLL: Duration = Duration::from_millis(10);

/// Bounded periodic recovery/retention work owned by a concrete component graph.
pub trait ComponentMaintenance: Send + Sync + 'static {
    fn maintain(&self, maximum_items: usize, now: u64) -> Result<usize, ApiFailure>;
    fn set_maintenance_health(&self, healthy: bool);
}

/// Finite filesystem and worker policy for the privileged component boundary.
#[derive(Clone, Debug)]
pub struct ComponentServerConfig {
    pub socket_path: PathBuf,
    pub allowed_uid: u32,
    pub worker_count: usize,
    pub queue_capacity: usize,
    pub limits: Limits,
}

impl ComponentServerConfig {
    fn validate(&self) -> Result<(), ComponentServerError> {
        let limits = self
            .limits
            .validate()
            .map_err(|_| ComponentServerError::Configuration)?;
        if !self.socket_path.is_absolute()
            || self.socket_path.file_name().is_none()
            || self.worker_count == 0
            || self.queue_capacity == 0
            || self.worker_count > limits.maximum_connections
            || self.worker_count > limits.maximum_streams
            || self
                .worker_count
                .checked_add(self.queue_capacity)
                .is_none_or(|total| total > limits.maximum_connections)
            || self
                .queue_capacity
                .checked_mul(limits.maximum_frame_bytes)
                .is_none_or(|bytes| bytes > limits.maximum_queued_bytes)
            || rustix::process::getuid().as_raw() != self.allowed_uid
        {
            return Err(ComponentServerError::Configuration);
        }
        let parent = self
            .socket_path
            .parent()
            .ok_or(ComponentServerError::Configuration)?;
        let metadata = fs::symlink_metadata(parent).map_err(ComponentServerError::Io)?;
        if !metadata.file_type().is_dir()
            || metadata.uid() != self.allowed_uid
            || metadata.mode() & 0o022 != 0
        {
            return Err(ComponentServerError::Configuration);
        }
        Ok(())
    }
}

/// Cooperative stop signal shared by production shutdown handling and tests.
#[derive(Clone, Debug, Default)]
pub struct ComponentShutdown(Arc<AtomicBool>);

impl ComponentShutdown {
    pub fn request(&self) {
        self.0.store(true, Ordering::Release);
    }

    #[must_use]
    pub fn requested(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// Production server for the existing bounded human component protocol.
pub struct HumanComponentServer {
    backend: Arc<dyn HumanApiComponents>,
    maintenance: Option<MaintenanceRuntime>,
}

#[derive(Clone)]
struct MaintenanceRuntime {
    backend: Arc<dyn ComponentMaintenance>,
    interval: Duration,
    maximum_items: usize,
}

impl HumanComponentServer {
    #[must_use]
    pub fn new(backend: Arc<dyn HumanApiComponents>) -> Self {
        Self {
            backend,
            maintenance: None,
        }
    }

    /// Creates a production server whose retention/recovery maintenance is
    /// part of readiness and runs under a finite cadence and work bound.
    pub fn new_maintained<B>(
        backend: Arc<B>,
        interval: Duration,
        maximum_items: usize,
    ) -> Result<Self, ComponentServerError>
    where
        B: HumanApiComponents + ComponentMaintenance,
    {
        if interval.is_zero() || maximum_items == 0 {
            return Err(ComponentServerError::Configuration);
        }
        Ok(Self {
            backend: Arc::clone(&backend) as Arc<dyn HumanApiComponents>,
            maintenance: Some(MaintenanceRuntime {
                backend,
                interval,
                maximum_items,
            }),
        })
    }

    /// Validates the embedded schema and filesystem authority before binding.
    pub fn bind(
        self,
        configuration: ComponentServerConfig,
    ) -> Result<BoundHumanComponentServer, ComponentServerError> {
        configuration.validate()?;
        prepare_socket_path(&configuration.socket_path, configuration.allowed_uid)?;
        let listener =
            UnixListener::bind(&configuration.socket_path).map_err(ComponentServerError::Io)?;
        fs::set_permissions(
            &configuration.socket_path,
            fs::Permissions::from_mode(0o600),
        )
        .map_err(ComponentServerError::Io)?;
        let metadata =
            fs::symlink_metadata(&configuration.socket_path).map_err(ComponentServerError::Io)?;
        if !metadata.file_type().is_socket()
            || metadata.uid() != configuration.allowed_uid
            || metadata.mode() & 0o777 != 0o600
        {
            return Err(ComponentServerError::Configuration);
        }
        listener
            .set_nonblocking(true)
            .map_err(ComponentServerError::Io)?;
        let schema = ApiSchema::v1().map_err(|_| ComponentServerError::Configuration)?;
        Ok(BoundHumanComponentServer {
            listener,
            dispatcher: Arc::new(Dispatcher {
                backend: self.backend,
                schema,
            }),
            socket: SocketGuard {
                path: configuration.socket_path.clone(),
                uid: configuration.allowed_uid,
                device: metadata.dev(),
                inode: metadata.ino(),
            },
            configuration,
            shutdown: ComponentShutdown::default(),
            maintenance: self.maintenance,
        })
    }
}

/// A fully validated listener that owns and cleans up exactly its socket inode.
pub struct BoundHumanComponentServer {
    listener: UnixListener,
    dispatcher: Arc<Dispatcher>,
    socket: SocketGuard,
    configuration: ComponentServerConfig,
    shutdown: ComponentShutdown,
    maintenance: Option<MaintenanceRuntime>,
}

impl BoundHumanComponentServer {
    #[must_use]
    pub fn shutdown(&self) -> ComponentShutdown {
        self.shutdown.clone()
    }

    #[must_use]
    pub fn local_path(&self) -> &Path {
        &self.socket.path
    }

    /// Runs a fixed worker pool and bounded admission queue until shutdown.
    pub fn run(self) -> Result<(), ComponentServerError> {
        let maintenance_worker = if let Some(maintenance) = self.maintenance.clone() {
            let now = epoch_seconds()?;
            maintenance
                .backend
                .maintain(maintenance.maximum_items, now)
                .map_err(|_| ComponentServerError::Protocol)?;
            let shutdown = self.shutdown.clone();
            Some(thread::spawn(move || {
                while !shutdown.requested() {
                    thread::sleep(maintenance.interval);
                    if shutdown.requested() {
                        break;
                    }
                    let Ok(now) = epoch_seconds() else {
                        maintenance.backend.set_maintenance_health(false);
                        break;
                    };
                    if maintenance
                        .backend
                        .maintain(maintenance.maximum_items, now)
                        .is_err()
                    {
                        maintenance.backend.set_maintenance_health(false);
                        break;
                    }
                }
            }))
        } else {
            None
        };
        let (sender, receiver) = mpsc::sync_channel(self.configuration.queue_capacity);
        let receiver = Arc::new(Mutex::new(receiver));
        let mut workers = Vec::with_capacity(self.configuration.worker_count);
        for _ in 0..self.configuration.worker_count {
            let receiver = Arc::clone(&receiver);
            let dispatcher = Arc::clone(&self.dispatcher);
            let shutdown = self.shutdown.clone();
            let limits = self.configuration.limits;
            let allowed_uid = self.configuration.allowed_uid;
            workers.push(thread::spawn(move || {
                worker(receiver, dispatcher, shutdown, limits, allowed_uid);
            }));
        }
        let accepted = accept_loop(
            &self.listener,
            &sender,
            &self.shutdown,
            self.configuration.allowed_uid,
        );
        drop(sender);
        self.shutdown.request();
        for worker in workers {
            if worker.join().is_err() {
                return Err(ComponentServerError::Worker);
            }
        }
        if let Some(worker) = maintenance_worker {
            if worker.join().is_err() {
                return Err(ComponentServerError::Worker);
            }
        }
        accepted
    }
}

fn epoch_seconds() -> Result<u64, ComponentServerError> {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|value| value.as_secs())
        .map_err(|_| ComponentServerError::Protocol)
}

struct Dispatcher {
    backend: Arc<dyn HumanApiComponents>,
    schema: ApiSchema,
}

impl Dispatcher {
    fn dispatch(&self, request: &mut ComponentRequest) -> Result<Vec<u8>, ApiFailure> {
        request.validate()?;
        match request {
            ComponentRequest::Authorize {
                operation,
                access_token,
                csrf_token,
                intended_destination,
                refresh,
                request_digest,
                disclosure_digest,
                path_parameters,
                body,
                idempotency_key,
                trace,
                ..
            } => {
                let operation = self
                    .schema
                    .operation(operation)
                    .ok_or_else(ApiFailure::not_found)?;
                let request_digest = parse_digest(request_digest, "request_digest")?;
                let disclosure_digest = parse_digest(disclosure_digest, "disclosure_digest")?;
                if disclosure_digest != json_digest(body)?
                    || request_digest
                        != authorized_request_digest(
                            operation,
                            intended_destination,
                            path_parameters,
                            body,
                            idempotency_key.as_deref(),
                            trace,
                        )?
                {
                    return Err(ApiFailure::unauthenticated());
                }
                let context = self.backend.authorize(
                    operation,
                    SessionCredentials {
                        access_token,
                        csrf_token: csrf_token.as_deref(),
                        intended_destination,
                        refresh: *refresh,
                        request_digest,
                        disclosure_digest,
                        path_parameters,
                        body,
                        idempotency_key: idempotency_key.as_deref(),
                    },
                    trace,
                )?;
                encode_authorized(&context)
            }
            ComponentRequest::Execute {
                component,
                operation,
                principal,
                path_parameters,
                body,
                idempotency_key,
                trace,
                ..
            } => {
                let operation = self
                    .schema
                    .operation(operation)
                    .ok_or_else(ApiFailure::not_found)?;
                let owner = component_owner(&operation.name)?;
                if component != owner {
                    return Err(ApiFailure::forbidden());
                }
                validate_execute(operation, path_parameters, idempotency_key.as_deref())?;
                if operation.is_public_bootstrap() != principal.is_none() {
                    return Err(ApiFailure::forbidden());
                }
                let principal = principal_context(principal.as_ref())?;
                let body = self
                    .schema
                    .decode_request(operation, Some(std::mem::take(body)))
                    .map_err(|_| ApiFailure::invalid_request(Some("body")))?;
                let response = self.backend.execute(ScopedRequest {
                    operation,
                    principal,
                    path_parameters: std::mem::take(path_parameters),
                    body,
                    idempotency_key: idempotency_key.take(),
                    trace: std::mem::take(trace),
                })?;
                self.schema
                    .encode_response(operation, &response.result)
                    .map_err(|_| ApiFailure::upstream_degraded())?;
                encode_backend(&response)
            }
            ComponentRequest::Readiness { trace, .. } => {
                encode_readiness(self.backend.readiness(trace)?)
            }
        }
    }

    fn dispatch_frame(&self, frame: &mut [u8]) -> Vec<u8> {
        let parsed = serde_json::from_slice::<ComponentRequest>(frame);
        frame.zeroize();
        let response = match parsed {
            Ok(mut request) => {
                let result = self.dispatch(&mut request);
                request.zeroize();
                result
            }
            Err(_) => Err(ApiFailure::invalid_request(None)),
        };
        match response {
            Ok(encoded) => encoded,
            Err(failure) => encode_failure(&failure).unwrap_or_else(|_| fallback_failure()),
        }
    }
}

fn principal_context(
    value: Option<&WirePrincipal>,
) -> Result<Option<PrincipalContext>, ApiFailure> {
    value
        .map(|principal| {
            let context = PrincipalContext::authorized(
                PrincipalId::new(&principal.principal_id)
                    .map_err(|_| ApiFailure::invalid_request(Some("principal_id")))?,
                AgentTenantId::new(&principal.tenant_id)
                    .map_err(|_| ApiFailure::invalid_request(Some("tenant_id")))?,
                principal.session_id.clone(),
                principal.capability.clone(),
                parse_digest(&principal.request_digest, "request_digest")?,
                parse_digest(&principal.disclosure_digest, "disclosure_digest")?,
                principal.operation.clone(),
                principal.destination.clone(),
                principal.trace.clone(),
                principal.issued_at,
                principal.expires_at,
            )?;
            match (&principal.refresh_token, &principal.refresh_csrf) {
                (Some(token), Some(csrf)) => context.with_refresh(token.clone(), csrf.clone()),
                (None, None) => Ok(context),
                _ => Err(ApiFailure::unauthenticated()),
            }
        })
        .transpose()
}

fn accept_loop(
    listener: &UnixListener,
    sender: &SyncSender<UnixStream>,
    shutdown: &ComponentShutdown,
    allowed_uid: u32,
) -> Result<(), ComponentServerError> {
    while !shutdown.requested() {
        match listener.accept() {
            Ok((stream, _)) => {
                let peer = socket_peercred(&stream)
                    .map_err(|error| ComponentServerError::Io(error.into()))?;
                if peer.uid.as_raw() != allowed_uid {
                    continue;
                }
                match sender.try_send(stream) {
                    Ok(()) | Err(TrySendError::Full(_)) => {}
                    Err(TrySendError::Disconnected(_)) => {
                        return Err(ComponentServerError::Worker);
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(ACCEPT_POLL);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(ComponentServerError::Io(error)),
        }
    }
    Ok(())
}

fn worker(
    receiver: Arc<Mutex<Receiver<UnixStream>>>,
    dispatcher: Arc<Dispatcher>,
    shutdown: ComponentShutdown,
    limits: Limits,
    allowed_uid: u32,
) {
    loop {
        let received = match receiver.lock() {
            Ok(receiver) => receiver.try_recv(),
            Err(_) => return,
        };
        match received {
            Ok(mut stream) => {
                if socket_peercred(&stream).is_ok_and(|peer| peer.uid.as_raw() == allowed_uid) {
                    let _ = serve_one(&mut stream, &dispatcher, limits);
                }
            }
            Err(TryRecvError::Empty) if !shutdown.requested() => thread::sleep(ACCEPT_POLL),
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => return,
        }
    }
}

fn serve_one(
    stream: &mut UnixStream,
    dispatcher: &Dispatcher,
    limits: Limits,
) -> Result<(), ComponentServerError> {
    stream
        .set_read_timeout(Some(limits.deadline))
        .and_then(|()| stream.set_write_timeout(Some(limits.deadline)))
        .map_err(ComponentServerError::Io)?;
    let mut frame = read_frame(stream, limits.maximum_frame_bytes)
        .map_err(|_| ComponentServerError::Protocol)?;
    reject_buffered_second_frame(stream)?;
    let mut response = dispatcher.dispatch_frame(&mut frame);
    let result = write_frame(stream, &response, limits.maximum_frame_bytes)
        .map_err(|_| ComponentServerError::Protocol);
    response.zeroize();
    result
}

fn reject_buffered_second_frame(stream: &UnixStream) -> Result<(), ComponentServerError> {
    let mut extra = [0_u8; 1];
    match recv(stream, &mut extra, RecvFlags::PEEK | RecvFlags::DONTWAIT).map_err(io::Error::from) {
        Ok((0, _)) => Ok(()),
        Ok((_, _)) => Err(ComponentServerError::Protocol),
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::Interrupted => {
            Err(ComponentServerError::Protocol)
        }
        Err(error) => Err(ComponentServerError::Io(error)),
    }
}

fn prepare_socket_path(path: &Path, allowed_uid: u32) -> Result<(), ComponentServerError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(ComponentServerError::Io(error)),
    };
    if !metadata.file_type().is_socket() || metadata.uid() != allowed_uid {
        return Err(ComponentServerError::Configuration);
    }
    match UnixStream::connect(path) {
        Ok(_) => Err(ComponentServerError::AlreadyRunning),
        Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => {
            let current = fs::symlink_metadata(path).map_err(ComponentServerError::Io)?;
            if !current.file_type().is_socket()
                || current.uid() != allowed_uid
                || current.dev() != metadata.dev()
                || current.ino() != metadata.ino()
            {
                return Err(ComponentServerError::Configuration);
            }
            fs::remove_file(path).map_err(ComponentServerError::Io)
        }
        Err(_) => Err(ComponentServerError::Configuration),
    }
}

struct SocketGuard {
    path: PathBuf,
    uid: u32,
    device: u64,
    inode: u64,
}

impl Drop for SocketGuard {
    fn drop(&mut self) {
        let Ok(metadata) = fs::symlink_metadata(&self.path) else {
            return;
        };
        if metadata.file_type().is_socket()
            && metadata.uid() == self.uid
            && metadata.dev() == self.device
            && metadata.ino() == self.inode
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn fallback_failure() -> Vec<u8> {
    br#"{"version":1,"ok":false,"error":{"status":503,"code":"upstream-degraded","copy_key":"error.upstream.degraded","retry":"retriable"}}"#.to_vec()
}

#[derive(Debug)]
pub enum ComponentServerError {
    Configuration,
    AlreadyRunning,
    Io(io::Error),
    Protocol,
    Worker,
}

impl std::fmt::Display for ComponentServerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Configuration => formatter.write_str("invalid component server configuration"),
            Self::AlreadyRunning => formatter.write_str("the component server is already running"),
            Self::Io(error) => write!(formatter, "component server I/O failed: {error}"),
            Self::Protocol => formatter.write_str("component protocol failed"),
            Self::Worker => formatter.write_str("component worker failed"),
        }
    }
}

impl std::error::Error for ComponentServerError {}

#[cfg(test)]
mod tests {
    use std::io::{Read as _, Write as _};

    use super::{reject_buffered_second_frame, ComponentServerError, UnixStream};

    #[test]
    fn second_frame_probe_is_nonblocking_and_does_not_consume_bytes() {
        let (server, mut client) = UnixStream::pair().expect("socket pair");
        reject_buffered_second_frame(&server).expect("empty socket");

        client.write_all(&[0x7f]).expect("write extra byte");
        assert!(matches!(
            reject_buffered_second_frame(&server),
            Err(ComponentServerError::Protocol)
        ));

        let mut retained = [0_u8; 1];
        (&server)
            .read_exact(&mut retained)
            .expect("peek retained byte");
        assert_eq!(retained, [0x7f]);
    }
}

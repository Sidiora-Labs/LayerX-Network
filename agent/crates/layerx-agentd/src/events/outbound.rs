//! Mutually authenticated local endpoint delivery with payload binding.

use std::fmt::{Debug, Display, Formatter};
use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, RwLock, Weak};
use std::thread;
use std::time::Duration;

use layerx_agent_api::subscription::{ReceiptReference, SubscriptionTarget};
use sha2::{Digest, Sha256};

use super::delivery::{delivery_attempt, DeliveryEngine, DeliveryError, DeliveryItem, RetryPlan};
use super::subscription::Termination;

const MAGIC: &[u8; 4] = b"LXOW";
const ACK_MAGIC: &[u8; 4] = b"LXOA";
const VERSION: u8 = 1;

/// Operating-system identity required of a local external endpoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeerIdentity {
    pub uid: u32,
    pub gid: u32,
}

/// Finite endpoint I/O limits and required peer identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Endpoint {
    pub path: PathBuf,
    pub expected_peer: PeerIdentity,
    pub maximum_frame_bytes: usize,
    pub deadline: Duration,
    pub cancellation_poll: Duration,
}

impl Endpoint {
    /// Constructs a mutually authenticated local endpoint with finite bounds.
    ///
    /// # Errors
    ///
    /// Returns `InvalidConfiguration` for a relative path, a zero frame bound, or a
    /// zero deadline or cancellation poll.
    pub fn new(
        path: impl Into<PathBuf>,
        expected_peer: PeerIdentity,
        maximum_frame_bytes: usize,
        deadline: Duration,
        cancellation_poll: Duration,
    ) -> Result<Self, OutboundError> {
        let path = path.into();
        if !path.is_absolute()
            || maximum_frame_bytes == 0
            || deadline.is_zero()
            || cancellation_poll.is_zero()
        {
            return Err(OutboundError::InvalidConfiguration);
        }
        Ok(Self {
            path,
            expected_peer,
            maximum_frame_bytes,
            deadline,
            cancellation_poll,
        })
    }
}

struct BindingKey([u8; 32]);

impl Drop for BindingKey {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

impl Debug for BindingKey {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("BindingKey([REDACTED])")
    }
}

/// Shared payload-binding key and its non-secret receiver-visible identifier.
pub struct Authenticator {
    key_id: String,
    key: BindingKey,
}

impl Authenticator {
    /// Imports a fixed binding key into redacted zeroing storage.
    ///
    /// # Errors
    ///
    /// Returns `InvalidConfiguration` for an empty, oversized, or NUL-bearing key
    /// identifier.
    pub fn new(key_id: impl Into<String>, key: [u8; 32]) -> Result<Self, OutboundError> {
        let key_id = key_id.into();
        if key_id.is_empty() || key_id.len() > 255 || key_id.as_bytes().contains(&0) {
            return Err(OutboundError::InvalidConfiguration);
        }
        Ok(Self {
            key_id,
            key: BindingKey(key),
        })
    }

    /// Verifies the receiver-bound frame after the caller authenticated the
    /// sender through operating-system peer credentials.
    ///
    /// # Errors
    ///
    /// Returns `Binding` for a truncated frame, an unequal payload tag, or an event
    /// identity that differs from its deduplication identifier, `Authentication` when
    /// the encoded receiver or key identifier differs, and `Protocol` for a malformed,
    /// unknown, or trailing-byte frame.
    pub fn verify(
        &self,
        frame: &[u8],
        receiver: PeerIdentity,
    ) -> Result<VerifiedOutbound, OutboundError> {
        if frame.len() < 32 {
            return Err(OutboundError::Binding);
        }
        let payload_length = frame.len() - 32;
        let mut supplied = [0_u8; 32];
        supplied.copy_from_slice(&frame[payload_length..]);
        let expected = hmac_sha256(&self.key.0, &frame[..payload_length]);
        if !constant_time_eq(&supplied, &expected) {
            return Err(OutboundError::Binding);
        }
        let mut decoder = Decoder::new(&frame[..payload_length]);
        if decoder.take(4)? != MAGIC || decoder.byte()? != VERSION {
            return Err(OutboundError::Protocol);
        }
        let encoded_receiver = PeerIdentity {
            uid: decoder.u32()?,
            gid: decoder.u32()?,
        };
        if encoded_receiver != receiver || decoder.string()? != self.key_id {
            return Err(OutboundError::Authentication);
        }
        let subscription_id = decoder.string()?;
        let tenant = decoder.string()?;
        let agent = decoder.string()?;
        let capability = decoder.string()?;
        let item = match decoder.byte()? {
            1 => {
                let global_sequence = decoder.u64()?;
                let phase = decoder.byte()?;
                if phase > 1 {
                    return Err(OutboundError::Protocol);
                }
                let cursor = decoder.u64()?;
                let mut event_identity = [0_u8; 32];
                event_identity.copy_from_slice(decoder.take(32)?);
                let mut deduplication_id = [0_u8; 32];
                deduplication_id.copy_from_slice(decoder.take(32)?);
                if event_identity != deduplication_id {
                    return Err(OutboundError::Binding);
                }
                let receipt_reference = match decoder.byte()? {
                    0 => None,
                    1 => Some((decoder.string()?, decoder.byte()?)),
                    _ => return Err(OutboundError::Protocol),
                };
                let event_bytes = decoder.bytes()?.to_vec();
                if event_bytes.is_empty() {
                    return Err(OutboundError::Protocol);
                }
                VerifiedItem::Event {
                    global_sequence,
                    phase,
                    cursor,
                    event_identity,
                    deduplication_id,
                    receipt_reference,
                    event_bytes,
                }
            }
            2 => VerifiedItem::BackfillComplete {
                live_starts_at: decoder.u64()?,
                resume_cursor: decoder.u64()?,
            },
            _ => return Err(OutboundError::Protocol),
        };
        if !decoder.is_empty() {
            return Err(OutboundError::Protocol);
        }
        Ok(VerifiedOutbound {
            binding: supplied,
            receiver,
            key_id: self.key_id.clone(),
            subscription_id,
            tenant,
            agent,
            capability,
            item,
        })
    }

    /// Builds the receiver acknowledgement bound to one verified request.
    #[must_use]
    pub fn acknowledgement(&self, binding: [u8; 32]) -> Vec<u8> {
        let mut payload = ACK_MAGIC.to_vec();
        payload.extend_from_slice(&binding);
        let mac = hmac_sha256(&self.key.0, &payload);
        payload.extend_from_slice(&mac);
        payload
    }

    fn bind(
        &self,
        target: &SubscriptionTarget,
        receiver: PeerIdentity,
        item: &DeliveryItem,
    ) -> Result<BoundFrame, OutboundError> {
        let mut payload = Vec::new();
        payload.extend_from_slice(MAGIC);
        payload.push(VERSION);
        payload.extend_from_slice(&receiver.uid.to_be_bytes());
        payload.extend_from_slice(&receiver.gid.to_be_bytes());
        push_string(&mut payload, &self.key_id)?;
        push_string(&mut payload, target.subscription_id.as_str())?;
        push_string(&mut payload, target.scope.tenant.as_str())?;
        push_string(&mut payload, target.scope.agent.as_str())?;
        push_string(&mut payload, target.scope.capability.as_str())?;
        match item {
            DeliveryItem::Event(event) => {
                payload.push(1);
                payload.extend_from_slice(&event.global_sequence.to_be_bytes());
                payload.push(match event.phase {
                    super::delivery::DeliveryPhase::Backfill => 0,
                    super::delivery::DeliveryPhase::Live => 1,
                });
                payload.extend_from_slice(&event.delivery.cursor.0 .0.to_be_bytes());
                payload.extend_from_slice(&event.delivery.event_identity.as_bytes());
                payload.extend_from_slice(&event.delivery.deduplication_id.as_bytes());
                match &event.delivery.receipt_reference {
                    ReceiptReference::None => payload.push(0),
                    ReceiptReference::Verified {
                        receipt_ref,
                        verification_level,
                    } => {
                        payload.push(1);
                        push_string(&mut payload, receipt_ref.as_str())?;
                        payload.push(*verification_level as u8);
                    }
                }
                push_bytes(&mut payload, &event.delivery.event_bytes)?;
            }
            DeliveryItem::BackfillComplete(transition) => {
                payload.push(2);
                payload.extend_from_slice(&transition.live_starts_at.to_be_bytes());
                payload.extend_from_slice(&transition.resume_cursor.0 .0.to_be_bytes());
            }
        }
        let binding = hmac_sha256(&self.key.0, &payload);
        payload.extend_from_slice(&binding);
        Ok(BoundFrame {
            bytes: payload,
            binding,
        })
    }

    fn verify_acknowledgement(
        &self,
        acknowledgement: &[u8],
        binding: [u8; 32],
    ) -> Result<(), OutboundError> {
        if acknowledgement.len() != 68
            || &acknowledgement[..4] != ACK_MAGIC
            || acknowledgement[4..36] != binding
        {
            return Err(OutboundError::Acknowledgement);
        }
        let expected = hmac_sha256(&self.key.0, &acknowledgement[..36]);
        let mut supplied = [0_u8; 32];
        supplied.copy_from_slice(&acknowledgement[36..]);
        if !constant_time_eq(&expected, &supplied) {
            return Err(OutboundError::Acknowledgement);
        }
        Ok(())
    }
}

impl Debug for Authenticator {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Authenticator")
            .field("key_id", &self.key_id)
            .field("key", &self.key)
            .finish()
    }
}

/// Verified receiver view containing the exact event bytes needed for proof.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedOutbound {
    pub binding: [u8; 32],
    pub receiver: PeerIdentity,
    pub key_id: String,
    pub subscription_id: String,
    pub tenant: String,
    pub agent: String,
    pub capability: String,
    pub item: VerifiedItem,
}

/// Decoded authenticated outbound item.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VerifiedItem {
    Event {
        global_sequence: u64,
        phase: u8,
        cursor: u64,
        event_identity: [u8; 32],
        deduplication_id: [u8; 32],
        receipt_reference: Option<(String, u8)>,
        event_bytes: Vec<u8>,
    },
    BackfillComplete {
        live_starts_at: u64,
        resume_cursor: u64,
    },
}

/// Successful acknowledged delivery.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutboundReceipt {
    pub binding: [u8; 32],
    pub peer: PeerIdentity,
    pub item: DeliveryItem,
}

/// Endpoint or protocol failure recorded against the retrying subscription.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EndpointFailure {
    Unreachable(ErrorKind),
    Authentication,
    Timeout,
    Frame,
    Protocol,
}

/// Shared stop signal checked throughout nonblocking endpoint I/O.
#[derive(Clone, Debug)]
pub struct StopSignal(Arc<AtomicU8>);

#[derive(Clone, Debug)]
pub(crate) struct StopWatcher(Weak<AtomicU8>);

impl StopSignal {
    #[must_use]
    pub fn active() -> Self {
        Self(Arc::new(AtomicU8::new(0)))
    }

    pub fn stop(&self, reason: Termination) {
        self.0.store(encode_termination(reason), Ordering::Release);
    }

    #[must_use]
    pub fn reason(&self) -> Option<Termination> {
        decode_termination(self.0.load(Ordering::Acquire))
    }

    #[must_use]
    pub(crate) fn watcher(&self) -> StopWatcher {
        StopWatcher(Arc::downgrade(&self.0))
    }
}

impl StopWatcher {
    #[must_use]
    pub(crate) fn live(&self) -> bool {
        self.0.strong_count() != 0
    }

    pub(crate) fn stop(&self, reason: Termination) {
        if let Some(signal) = self.0.upgrade() {
            signal.store(encode_termination(reason), Ordering::Release);
        }
    }
}

/// Outbound delivery failures and durable retry decisions.
#[derive(Debug)]
pub enum OutboundError {
    InvalidConfiguration,
    NoPendingDelivery,
    RetryScheduled {
        plan: RetryPlan,
        failure: EndpointFailure,
    },
    Stopped(Termination),
    AuthorizationUnavailable,
    Authentication,
    Binding,
    Acknowledgement,
    Protocol,
    Delivery(DeliveryError),
}

impl Display for OutboundError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidConfiguration => formatter.write_str("outbound configuration is invalid"),
            Self::NoPendingDelivery => formatter.write_str("no outbound delivery is pending"),
            Self::RetryScheduled { failure, .. } => {
                write!(formatter, "outbound retry scheduled after {failure:?}")
            }
            Self::Stopped(reason) => write!(formatter, "outbound delivery stopped: {reason:?}"),
            Self::AuthorizationUnavailable => {
                formatter.write_str("outbound authorization registry is unavailable")
            }
            Self::Authentication => formatter.write_str("endpoint authentication failed"),
            Self::Binding => formatter.write_str("outbound payload binding failed"),
            Self::Acknowledgement => formatter.write_str("endpoint acknowledgement is invalid"),
            Self::Protocol => formatter.write_str("outbound frame is malformed"),
            Self::Delivery(error) => Display::fmt(error, formatter),
        }
    }
}

impl std::error::Error for OutboundError {}

impl From<DeliveryError> for OutboundError {
    fn from(value: DeliveryError) -> Self {
        Self::Delivery(value)
    }
}

struct BoundFrame {
    bytes: Vec<u8>,
    binding: [u8; 32],
}

/// Delivers exactly the current front item, waits for a bound acknowledgement,
/// and only then advances the at-least-once delivery engine.
///
/// # Errors
///
/// Returns `Stopped` when the stop signal is set, `NoPendingDelivery` when the
/// engine has nothing queued, and `RetryScheduled` when the frame exceeds the
/// endpoint bound or the connect, peer authentication, bounded I/O, or
/// acknowledgement step fails. Delivery-engine failures propagate unchanged.
pub fn deliver(
    engine: &mut DeliveryEngine,
    endpoint: &Endpoint,
    authenticator: &Authenticator,
    stop: &StopSignal,
    now_ms: u64,
) -> Result<OutboundReceipt, OutboundError> {
    engine.require_unbound()?;
    let pending = exchange(engine, endpoint, authenticator, stop, now_ms)?;
    finish_delivery(engine, pending, now_ms)
}

/// Delivers through the exact durable session binding, reauthorizing both before endpoint I/O and
/// immediately before accepting the delivery. The registered stop signal also interrupts bounded
/// I/O after a persist-before-publish revocation.
pub fn deliver_authorized(
    engine: &mut DeliveryEngine,
    sessions: &crate::session::SessionRegistry,
    endpoint: &Endpoint,
    authenticator: &Authenticator,
    core_sequence: u64,
    observability: &mut crate::tenant::TenantObservability,
    now_ms: u64,
) -> Result<OutboundReceipt, OutboundError> {
    authorize_engine(engine, sessions, core_sequence, observability)?;
    let stop = engine
        .authorization_stop()
        .ok_or(OutboundError::Delivery(DeliveryError::UnboundSession))?;
    let pending = exchange(engine, endpoint, authenticator, &stop, now_ms)?;
    authorize_engine(engine, sessions, core_sequence, observability)?;
    finish_delivery(engine, pending, now_ms)
}

/// Delivers with a shared registry, releasing the read lock during endpoint I/O so close,
/// authority revocation, and scope restriction can durably advance the exact session generation.
/// The registry is read again while the acknowledged item is accepted, making invalidation and
/// completion mutually ordered.
pub fn deliver_shared_authorized(
    engine: &mut DeliveryEngine,
    sessions: &RwLock<crate::session::SessionRegistry>,
    endpoint: &Endpoint,
    authenticator: &Authenticator,
    core_sequence: u64,
    observability: &mut crate::tenant::TenantObservability,
    now_ms: u64,
) -> Result<OutboundReceipt, OutboundError> {
    {
        let registry = sessions
            .read()
            .map_err(|_| OutboundError::AuthorizationUnavailable)?;
        authorize_engine(engine, &registry, core_sequence, observability)?;
    }
    let stop = engine
        .authorization_stop()
        .ok_or(OutboundError::Delivery(DeliveryError::UnboundSession))?;
    let pending = exchange(engine, endpoint, authenticator, &stop, now_ms)?;
    let registry = sessions
        .read()
        .map_err(|_| OutboundError::AuthorizationUnavailable)?;
    authorize_engine(engine, &registry, core_sequence, observability)?;
    let result = finish_delivery(engine, pending, now_ms);
    drop(registry);
    result
}

struct PendingOutbound {
    binding: [u8; 32],
    peer: PeerIdentity,
    item: DeliveryItem,
}

fn exchange(
    engine: &mut DeliveryEngine,
    endpoint: &Endpoint,
    authenticator: &Authenticator,
    stop: &StopSignal,
    now_ms: u64,
) -> Result<PendingOutbound, OutboundError> {
    if let Some(reason) = stop.reason() {
        apply_stop(engine, reason)?;
        return Err(OutboundError::Stopped(reason));
    }
    let item = delivery_attempt(engine)?.ok_or(OutboundError::NoPendingDelivery)?;
    let frame = authenticator.bind(engine.target(), endpoint.expected_peer, &item)?;
    if frame.bytes.len() > endpoint.maximum_frame_bytes {
        return schedule_retry(engine, now_ms, EndpointFailure::Frame);
    }
    let mut stream = match UnixStream::connect(&endpoint.path) {
        Ok(value) => value,
        Err(error) => {
            return schedule_retry(engine, now_ms, EndpointFailure::Unreachable(error.kind()));
        }
    };
    if let Err(error) = authenticate_peer(&stream, endpoint.expected_peer) {
        let _ = error;
        return schedule_retry(engine, now_ms, EndpointFailure::Authentication);
    }
    if stream.set_nonblocking(true).is_err() {
        return schedule_retry(engine, now_ms, EndpointFailure::Protocol);
    }
    let mut budget = WaitBudget::new(endpoint.deadline, endpoint.cancellation_poll);
    let framed = frame_bytes(&frame.bytes)?;
    if let Err(failure) = write_all_bounded(&mut stream, &framed, &mut budget, stop) {
        if let EndpointIo::Stopped(reason) = failure {
            apply_stop(engine, reason)?;
            return Err(OutboundError::Stopped(reason));
        }
        return schedule_retry(engine, now_ms, failure.into_failure());
    }
    let acknowledgement =
        match read_frame_bounded(&mut stream, endpoint.maximum_frame_bytes, &mut budget, stop) {
            Ok(value) => value,
            Err(EndpointIo::Stopped(reason)) => {
                apply_stop(engine, reason)?;
                return Err(OutboundError::Stopped(reason));
            }
            Err(error) => return schedule_retry(engine, now_ms, error.into_failure()),
        };
    if authenticator
        .verify_acknowledgement(&acknowledgement, frame.binding)
        .is_err()
    {
        return schedule_retry(engine, now_ms, EndpointFailure::Protocol);
    }
    if let Some(reason) = stop.reason() {
        apply_stop(engine, reason)?;
        return Err(OutboundError::Stopped(reason));
    }
    Ok(PendingOutbound {
        binding: frame.binding,
        peer: endpoint.expected_peer,
        item,
    })
}

fn authorize_engine(
    engine: &mut DeliveryEngine,
    sessions: &crate::session::SessionRegistry,
    core_sequence: u64,
    observability: &mut crate::tenant::TenantObservability,
) -> Result<(), OutboundError> {
    match engine.authorize_boundary(
        sessions,
        crate::tenant::Operation::SubscriptionHealth,
        core_sequence,
        observability,
    ) {
        Ok(()) => Ok(()),
        Err(DeliveryError::Revoked) => Err(OutboundError::Stopped(Termination::SessionRevoked)),
        Err(error) => Err(OutboundError::Delivery(error)),
    }
}

fn finish_delivery(
    engine: &mut DeliveryEngine,
    pending: PendingOutbound,
    delivered_at_ms: u64,
) -> Result<OutboundReceipt, OutboundError> {
    engine.accept_front_inner(delivered_at_ms)?;
    Ok(OutboundReceipt {
        binding: pending.binding,
        peer: pending.peer,
        item: pending.item,
    })
}

/// Returns the ownership identity of the connected endpoint socket inode.
///
/// # Errors
///
/// Returns `Authentication` when the descriptor's `/proc/self/fd` metadata cannot
/// be read.
pub fn peer_identity(stream: &UnixStream) -> Result<PeerIdentity, OutboundError> {
    let descriptor = stream.as_raw_fd();
    let metadata = fs::metadata(format!("/proc/self/fd/{descriptor}"))
        .map_err(|_| OutboundError::Authentication)?;
    Ok(PeerIdentity {
        uid: metadata.uid(),
        gid: metadata.gid(),
    })
}

fn authenticate_peer(stream: &UnixStream, expected: PeerIdentity) -> Result<(), OutboundError> {
    if peer_identity(stream)? == expected {
        Ok(())
    } else {
        Err(OutboundError::Authentication)
    }
}

fn apply_stop(engine: &mut DeliveryEngine, reason: Termination) -> Result<(), OutboundError> {
    match reason {
        Termination::Deleted => engine.stop_deleted()?,
        other => engine.stop_revoked(other)?,
    }
    Ok(())
}

fn schedule_retry<T>(
    engine: &mut DeliveryEngine,
    now_ms: u64,
    failure: EndpointFailure,
) -> Result<T, OutboundError> {
    let plan = engine.fail_front(now_ms, &format!("{failure:?}"))?;
    Err(OutboundError::RetryScheduled { plan, failure })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EndpointIo {
    Timeout,
    Frame,
    Protocol,
    Stopped(Termination),
}

impl EndpointIo {
    const fn into_failure(self) -> EndpointFailure {
        match self {
            Self::Timeout => EndpointFailure::Timeout,
            Self::Frame => EndpointFailure::Frame,
            Self::Protocol | Self::Stopped(_) => EndpointFailure::Protocol,
        }
    }
}

/// Cancellation polls an endpoint exchange may still wait out before it is
/// abandoned, so a stalled peer is bounded without reading an ambient clock.
struct WaitBudget {
    remaining: u64,
    poll: Duration,
}

impl WaitBudget {
    fn new(deadline: Duration, poll: Duration) -> Self {
        let polls = deadline.as_nanos() / poll.as_nanos().max(1);
        Self {
            remaining: u64::try_from(polls).unwrap_or(u64::MAX).max(1),
            poll,
        }
    }

    fn wait(&mut self) -> Result<(), EndpointIo> {
        let Some(remaining) = self.remaining.checked_sub(1) else {
            return Err(EndpointIo::Timeout);
        };
        self.remaining = remaining;
        thread::sleep(self.poll);
        Ok(())
    }
}

fn write_all_bounded(
    stream: &mut UnixStream,
    bytes: &[u8],
    budget: &mut WaitBudget,
    stop: &StopSignal,
) -> Result<(), EndpointIo> {
    let mut written = 0;
    while written < bytes.len() {
        if let Some(reason) = stop.reason() {
            return Err(EndpointIo::Stopped(reason));
        }
        match stream.write(&bytes[written..]) {
            Ok(0) => return Err(EndpointIo::Protocol),
            Ok(count) => written += count,
            Err(error) if error.kind() == ErrorKind::WouldBlock => budget.wait()?,
            Err(_) => return Err(EndpointIo::Protocol),
        }
    }
    Ok(())
}

fn read_frame_bounded(
    stream: &mut UnixStream,
    maximum: usize,
    budget: &mut WaitBudget,
    stop: &StopSignal,
) -> Result<Vec<u8>, EndpointIo> {
    let mut length = [0_u8; 4];
    read_exact_bounded(stream, &mut length, budget, stop)?;
    let length = usize::try_from(u32::from_be_bytes(length)).map_err(|_| EndpointIo::Frame)?;
    if length == 0 || length > maximum {
        return Err(EndpointIo::Frame);
    }
    let mut body = vec![0_u8; length];
    read_exact_bounded(stream, &mut body, budget, stop)?;
    Ok(body)
}

fn read_exact_bounded(
    stream: &mut UnixStream,
    bytes: &mut [u8],
    budget: &mut WaitBudget,
    stop: &StopSignal,
) -> Result<(), EndpointIo> {
    let mut read = 0;
    while read < bytes.len() {
        if let Some(reason) = stop.reason() {
            return Err(EndpointIo::Stopped(reason));
        }
        match stream.read(&mut bytes[read..]) {
            Ok(0) => return Err(EndpointIo::Protocol),
            Ok(count) => read += count,
            Err(error) if error.kind() == ErrorKind::WouldBlock => budget.wait()?,
            Err(_) => return Err(EndpointIo::Protocol),
        }
    }
    Ok(())
}

fn frame_bytes(body: &[u8]) -> Result<Vec<u8>, OutboundError> {
    let length = u32::try_from(body.len()).map_err(|_| OutboundError::Protocol)?;
    let mut frame = Vec::with_capacity(4 + body.len());
    frame.extend_from_slice(&length.to_be_bytes());
    frame.extend_from_slice(body);
    Ok(frame)
}

fn push_string(output: &mut Vec<u8>, value: &str) -> Result<(), OutboundError> {
    let length = u16::try_from(value.len()).map_err(|_| OutboundError::Protocol)?;
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(value.as_bytes());
    Ok(())
}

fn push_bytes(output: &mut Vec<u8>, value: &[u8]) -> Result<(), OutboundError> {
    let length = u32::try_from(value.len()).map_err(|_| OutboundError::Protocol)?;
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(value);
    Ok(())
}

fn hmac_sha256(key: &[u8; 32], message: &[u8]) -> [u8; 32] {
    let mut inner_key = [0x36_u8; 64];
    let mut outer_key = [0x5c_u8; 64];
    for (index, byte) in key.iter().enumerate() {
        inner_key[index] ^= byte;
        outer_key[index] ^= byte;
    }
    let mut inner = Sha256::new();
    inner.update(inner_key);
    inner.update(message);
    let inner_digest = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(outer_key);
    outer.update(inner_digest);
    outer.finalize().into()
}

fn constant_time_eq(left: &[u8; 32], right: &[u8; 32]) -> bool {
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

const fn encode_termination(reason: Termination) -> u8 {
    match reason {
        Termination::Deleted => 1,
        Termination::SessionRevoked => 2,
        Termination::CapabilityRevoked => 3,
        Termination::TenantRevoked => 4,
    }
}

const fn decode_termination(value: u8) -> Option<Termination> {
    match value {
        1 => Some(Termination::Deleted),
        2 => Some(Termination::SessionRevoked),
        3 => Some(Termination::CapabilityRevoked),
        4 => Some(Termination::TenantRevoked),
        _ => None,
    }
}

struct Decoder<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Decoder<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], OutboundError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(OutboundError::Protocol)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(OutboundError::Protocol)?;
        self.offset = end;
        Ok(value)
    }

    fn byte(&mut self) -> Result<u8, OutboundError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, OutboundError> {
        let mut value = [0_u8; 2];
        value.copy_from_slice(self.take(2)?);
        Ok(u16::from_be_bytes(value))
    }

    fn u32(&mut self) -> Result<u32, OutboundError> {
        let mut value = [0_u8; 4];
        value.copy_from_slice(self.take(4)?);
        Ok(u32::from_be_bytes(value))
    }

    fn u64(&mut self) -> Result<u64, OutboundError> {
        let mut value = [0_u8; 8];
        value.copy_from_slice(self.take(8)?);
        Ok(u64::from_be_bytes(value))
    }

    fn string(&mut self) -> Result<String, OutboundError> {
        let length = usize::from(self.u16()?);
        let value = std::str::from_utf8(self.take(length)?).map_err(|_| OutboundError::Protocol)?;
        Ok(value.to_owned())
    }

    fn bytes(&mut self) -> Result<&'a [u8], OutboundError> {
        let length = usize::try_from(self.u32()?).map_err(|_| OutboundError::Protocol)?;
        self.take(length)
    }

    const fn is_empty(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

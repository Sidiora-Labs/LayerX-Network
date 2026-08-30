//! Authenticated same-process preparation snapshots from the production node.

use layerx_types::ids::Did;
use layerx_types::limits::{MAX_DID_BYTES, MAX_MODULE_ACTIVITY_TYPES};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_types::result::ResultCode;

use super::refusal::decode_core_refusal;
use super::schema::{decode_envelope, encode_envelope, Envelope, SchemaError, Version};
use super::transport::{FrameTransport, TransportError};

const PREPARATION_STATE_REQUEST_TAG: u16 = 26;
const PREPARATION_STATE_RESPONSE_TAG: u16 = 27;
const ERROR_RESPONSE_TAG: u16 = 25;
const PREPARATION_PAYLOAD_VERSION: u16 = 1;
const MAX_PREPARATION_PAYLOAD_BYTES: usize = 4096;
const MAX_MODULES: usize = 9;

/// Immutable protocol facts captured under the node process-owner mutex.
///
/// This is authenticated same-process boundary data. `observed_state_root` is
/// a snapshot binding, not an LNI-created Merkle proof or verification-level
/// claim.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparationState {
    pub actor: Did,
    pub network_id: u32,
    pub account_sequence: u64,
    pub protocol_timestamp: u64,
    pub observed_head_sequence: u64,
    pub observed_state_root: [u8; 32],
    pub kernel_epoch: u64,
    pub module_registry: ModuleRegistry,
}

/// Request identity and anti-regression expectations from the accepted LNI
/// handshake.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreparationStateContext {
    pub interface_version: Version,
    pub expected_network_id: u32,
    pub minimum_observed_head: u64,
    pub correlation_id: u64,
}

/// Fail-closed preparation-state boundary error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreparationStateError {
    Transport(TransportError),
    Envelope(SchemaError),
    CoreRefusal { class: u8, result: ResultCode },
    UnavailableCapability,
    Disconnected,
    InvalidCorrelation,
    InterfaceVersion(Version),
    MalformedRequest,
    MalformedResponse,
    ActorMismatch,
    Network { expected: u32, actual: u32 },
    StaleSnapshot { minimum: u64, observed: u64 },
}

impl From<TransportError> for PreparationStateError {
    fn from(value: TransportError) -> Self {
        Self::Transport(value)
    }
}

impl From<SchemaError> for PreparationStateError {
    fn from(value: SchemaError) -> Self {
        Self::Envelope(value)
    }
}

/// Encodes the strict bounded request payload independently of its LNI
/// envelope.
///
/// # Errors
///
/// Refuses an actor outside the protocol DID bound.
pub fn encode_preparation_request(actor: &Did) -> Result<Vec<u8>, PreparationStateError> {
    let actor_length = u16::try_from(actor.as_bytes().len())
        .map_err(|_| PreparationStateError::MalformedRequest)?;
    if actor_length == 0 || usize::from(actor_length) > MAX_DID_BYTES {
        return Err(PreparationStateError::MalformedRequest);
    }
    let mut payload = Vec::with_capacity(4 + usize::from(actor_length));
    payload.extend_from_slice(&PREPARATION_PAYLOAD_VERSION.to_be_bytes());
    payload.extend_from_slice(&actor_length.to_be_bytes());
    payload.extend_from_slice(actor.as_bytes());
    Ok(payload)
}

/// Decodes and validates one strict snapshot payload without claiming proof
/// material that the authenticated same-process boundary did not provide.
///
/// # Errors
///
/// Refuses truncation, trailing data, a different actor or network, stale
/// state, zero timestamp/root/epoch, unrepresentable modules, duplicate
/// modules, and non-canonical activity registration ordering.
pub fn decode_preparation_response(
    payload: &[u8],
    actor: &Did,
    expected_network_id: u32,
    minimum_observed_head: u64,
) -> Result<PreparationState, PreparationStateError> {
    if payload.is_empty() || payload.len() > MAX_PREPARATION_PAYLOAD_BYTES {
        return Err(PreparationStateError::MalformedResponse);
    }
    let mut cursor = Cursor::new(payload);
    if cursor.u16()? != PREPARATION_PAYLOAD_VERSION {
        return Err(PreparationStateError::MalformedResponse);
    }
    let actor_length = usize::from(cursor.u16()?);
    if actor_length == 0 || actor_length > MAX_DID_BYTES {
        return Err(PreparationStateError::MalformedResponse);
    }
    let observed_actor = cursor.take(actor_length)?;
    if observed_actor != actor.as_bytes() {
        return Err(PreparationStateError::ActorMismatch);
    }
    let network_id = cursor.u32()?;
    if network_id == 0 {
        return Err(PreparationStateError::MalformedResponse);
    }
    if network_id != expected_network_id {
        return Err(PreparationStateError::Network {
            expected: expected_network_id,
            actual: network_id,
        });
    }
    let account_sequence = cursor.u64()?;
    let protocol_timestamp = cursor.u64()?;
    let observed_head_sequence = cursor.u64()?;
    let observed_state_root = cursor.array()?;
    let kernel_epoch = cursor.u64()?;
    if observed_head_sequence < minimum_observed_head {
        return Err(PreparationStateError::StaleSnapshot {
            minimum: minimum_observed_head,
            observed: observed_head_sequence,
        });
    }
    if protocol_timestamp == 0
        || kernel_epoch == 0
        || observed_state_root.iter().all(|byte| *byte == 0)
    {
        return Err(PreparationStateError::MalformedResponse);
    }
    let module_count = usize::from(cursor.u16()?);
    if module_count == 0 || module_count > MAX_MODULES {
        return Err(PreparationStateError::MalformedResponse);
    }
    let mut registrations = Vec::with_capacity(module_count);
    let mut previous_module = None;
    for _ in 0..module_count {
        let raw_module = cursor.u16()?;
        if previous_module.is_some_and(|previous| previous >= raw_module) {
            return Err(PreparationStateError::MalformedResponse);
        }
        let module =
            ModuleId::from_u16(raw_module).map_err(|_| PreparationStateError::MalformedResponse)?;
        let activity_count = usize::from(cursor.u16()?);
        if activity_count == 0 || activity_count > MAX_MODULE_ACTIVITY_TYPES {
            return Err(PreparationStateError::MalformedResponse);
        }
        let mut activity_types = Vec::with_capacity(activity_count);
        for _ in 0..activity_count {
            let activity_type = ActivityType::from_u32(cursor.u32()?)
                .map_err(|_| PreparationStateError::MalformedResponse)?;
            activity_types.push(activity_type);
        }
        let registration = ModuleRegistration::new(module, &activity_types)
            .map_err(|_| PreparationStateError::MalformedResponse)?;
        registrations.push(registration);
        previous_module = Some(raw_module);
    }
    if !cursor.finished() {
        return Err(PreparationStateError::MalformedResponse);
    }
    let module_registry = ModuleRegistry::new(&registrations)
        .map_err(|_| PreparationStateError::MalformedResponse)?;
    Ok(PreparationState {
        actor: actor.clone(),
        network_id,
        account_sequence,
        protocol_timestamp,
        observed_head_sequence,
        observed_state_root,
        kernel_epoch,
        module_registry,
    })
}

/// Requests one atomic node-owned preparation snapshot.
///
/// # Errors
///
/// Preserves transport and typed core refusals and rejects every malformed,
/// mismatched, or stale response.
pub fn preparation_state(
    transport: &mut dyn FrameTransport,
    actor: &Did,
    context: PreparationStateContext,
) -> Result<PreparationState, PreparationStateError> {
    if context.correlation_id == 0 {
        return Err(PreparationStateError::InvalidCorrelation);
    }
    if context.interface_version.major != Version::V1_1.major
        || context.interface_version.minor < Version::V1_1.minor
    {
        return Err(PreparationStateError::InterfaceVersion(
            context.interface_version,
        ));
    }
    let payload = encode_preparation_request(actor)?;
    let request = encode_envelope(Envelope {
        version: context.interface_version,
        message_tag: PREPARATION_STATE_REQUEST_TAG,
        correlation_id: context.correlation_id,
        canonical_payload: &payload,
        proof_material: &[],
    })?;
    transport.send(&request)?;
    let response_bytes = transport.receive()?;
    let response = decode_envelope(&response_bytes)?;
    if response.version != context.interface_version
        || response.correlation_id != context.correlation_id
        || !response.proof_material.is_empty()
    {
        return Err(PreparationStateError::MalformedResponse);
    }
    if response.message_tag == ERROR_RESPONSE_TAG {
        let refusal = decode_core_refusal(response.canonical_payload)
            .ok_or(PreparationStateError::MalformedResponse)?;
        return Err(PreparationStateError::CoreRefusal {
            class: refusal.class,
            result: refusal.result,
        });
    }
    if response.message_tag != PREPARATION_STATE_RESPONSE_TAG {
        return Err(PreparationStateError::MalformedResponse);
    }
    decode_preparation_response(
        response.canonical_payload,
        actor,
        context.expected_network_id,
        context.minimum_observed_head,
    )
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], PreparationStateError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(PreparationStateError::MalformedResponse)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(PreparationStateError::MalformedResponse)?;
        self.offset = end;
        Ok(value)
    }

    fn u16(&mut self) -> Result<u16, PreparationStateError> {
        Ok(u16::from_be_bytes(self.array()?))
    }

    fn u32(&mut self) -> Result<u32, PreparationStateError> {
        Ok(u32::from_be_bytes(self.array()?))
    }

    fn u64(&mut self) -> Result<u64, PreparationStateError> {
        Ok(u64::from_be_bytes(self.array()?))
    }

    fn array<const LENGTH: usize>(&mut self) -> Result<[u8; LENGTH], PreparationStateError> {
        self.take(LENGTH)?
            .try_into()
            .map_err(|_| PreparationStateError::MalformedResponse)
    }

    const fn finished(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

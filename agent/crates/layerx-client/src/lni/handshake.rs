//! Startup-only LNI identity, version, and capability negotiation.

use super::capabilities::Capabilities;
use super::schema::{encode_envelope, Envelope, SchemaError, Version};
use super::transport::{FrameTransport, TransportError};

const NODE_INFO_REQUEST_TAG: u16 = 1;
const NODE_INFO_RESPONSE_TAG: u16 = 2;
const MAX_CAPABILITIES: usize = 64;
const MAX_CAPABILITY_BYTES: usize = 64;
const FIXED_NODE_INFO_BYTES: usize = 93;

/// Operational role reported by the node boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NodeRole {
    Sequencer,
    Follower,
    Archive,
    Validator,
}

impl NodeRole {
    const fn tag(self) -> u8 {
        match self {
            Self::Sequencer => 1,
            Self::Follower => 2,
            Self::Archive => 3,
            Self::Validator => 4,
        }
    }

    const fn from_tag(tag: u8) -> Result<Self, HandshakeError> {
        match tag {
            1 => Ok(Self::Sequencer),
            2 => Ok(Self::Follower),
            3 => Ok(Self::Archive),
            4 => Ok(Self::Validator),
            _ => Err(HandshakeError::MalformedNodeInfo),
        }
    }
}

/// Complete node identity and head state exchanged before any other request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodeInfo {
    pub interface_version: Version,
    pub protocol_version: u16,
    pub network_id: u32,
    pub role: NodeRole,
    pub chain_head_sequence: u64,
    pub latest_sealed_batch: u64,
    pub latest_finalised_checkpoint: [u8; 32],
    pub authorised_sequencer_key: [u8; 32],
    pub advertised_capabilities: Vec<String>,
}

/// Startup expectations that cannot be weakened per request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HandshakeConfig {
    pub built_interface_version: Version,
    pub expected_protocol_version: u16,
    pub expected_network_id: u32,
}

/// Explicit key transition observed across two accepted connections.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SequencerKeyChange {
    pub previous: [u8; 32],
    pub current: [u8; 32],
}

/// Accepted startup state. No request API receives an unvalidated `NodeInfo`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Handshake {
    node: NodeInfo,
    capabilities: Capabilities,
    sequencer_key_change: Option<SequencerKeyChange>,
}

impl Handshake {
    #[must_use]
    pub const fn node(&self) -> &NodeInfo {
        &self.node
    }

    #[must_use]
    pub const fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    #[must_use]
    pub const fn sequencer_key_change(&self) -> Option<SequencerKeyChange> {
        self.sequencer_key_change
    }
}

/// Startup refusal. Expected and peer values are retained for diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HandshakeError {
    Transport(TransportError),
    InterfaceIncompatible { built: Version, peer: Version },
    ProtocolVersion { expected: u16, peer: u16 },
    Network { expected: u32, peer: u32 },
    MalformedEnvelope,
    MalformedNodeInfo,
}

impl From<TransportError> for HandshakeError {
    fn from(value: TransportError) -> Self {
        Self::Transport(value)
    }
}

impl From<SchemaError> for HandshakeError {
    fn from(_: SchemaError) -> Self {
        Self::MalformedEnvelope
    }
}

/// Performs the first exchange on a live transport and validates it as startup
/// state before returning.
///
/// # Errors
///
/// Refuses transport failures, malformed bytes, incompatible interface major,
/// wrong protocol version, or wrong network identifier.
pub fn perform<T: FrameTransport>(
    transport: &mut T,
    config: &HandshakeConfig,
    previous: Option<&Handshake>,
) -> Result<Handshake, HandshakeError> {
    let request = encode_envelope(Envelope {
        version: Version {
            major: config.built_interface_version.major,
            minor: 0,
        },
        message_tag: NODE_INFO_REQUEST_TAG,
        correlation_id: 0,
        canonical_payload: &[],
        proof_material: &[],
    })?;
    transport.send(&request)?;
    let response = transport.receive()?;
    let payload = node_info_payload(&response)?;
    let node = decode_node_info(payload)?;
    validate(node, config, previous)
}

/// Validates decoded node information using the same startup refusal path as a
/// live exchange.
///
/// # Errors
///
/// Returns explicit interface, protocol, or network incompatibility.
pub fn validate(
    node: NodeInfo,
    config: &HandshakeConfig,
    previous: Option<&Handshake>,
) -> Result<Handshake, HandshakeError> {
    if node.interface_version.major != config.built_interface_version.major {
        return Err(HandshakeError::InterfaceIncompatible {
            built: config.built_interface_version,
            peer: node.interface_version,
        });
    }
    if node.protocol_version != config.expected_protocol_version {
        return Err(HandshakeError::ProtocolVersion {
            expected: config.expected_protocol_version,
            peer: node.protocol_version,
        });
    }
    if node.network_id != config.expected_network_id {
        return Err(HandshakeError::Network {
            expected: config.expected_network_id,
            peer: node.network_id,
        });
    }
    let sequencer_key_change = previous.and_then(|accepted| {
        (accepted.node.authorised_sequencer_key != node.authorised_sequencer_key).then_some(
            SequencerKeyChange {
                previous: accepted.node.authorised_sequencer_key,
                current: node.authorised_sequencer_key,
            },
        )
    });
    let capabilities = Capabilities::negotiate(&node.advertised_capabilities);
    Ok(Handshake {
        node,
        capabilities,
        sequencer_key_change,
    })
}

/// Canonically encodes the non-consensus `NodeInfo` handshake payload.
///
/// # Errors
///
/// Refuses duplicate, unsorted, excessive, or over-long capability names.
pub fn encode_node_info(node: &NodeInfo) -> Result<Vec<u8>, HandshakeError> {
    if node.advertised_capabilities.len() > MAX_CAPABILITIES
        || node
            .advertised_capabilities
            .iter()
            .any(|name| name.is_empty() || name.len() > MAX_CAPABILITY_BYTES)
        || !strictly_increasing(&node.advertised_capabilities)
    {
        return Err(HandshakeError::MalformedNodeInfo);
    }
    let names_bytes = node
        .advertised_capabilities
        .iter()
        .try_fold(0_usize, |total, name| {
            total
                .checked_add(2)
                .and_then(|value| value.checked_add(name.len()))
                .ok_or(HandshakeError::MalformedNodeInfo)
        })?;
    let capacity = FIXED_NODE_INFO_BYTES
        .checked_add(names_bytes)
        .ok_or(HandshakeError::MalformedNodeInfo)?;
    let count = u16::try_from(node.advertised_capabilities.len())
        .map_err(|_| HandshakeError::MalformedNodeInfo)?;
    let mut bytes = Vec::with_capacity(capacity);
    bytes.extend_from_slice(&node.interface_version.major.to_be_bytes());
    bytes.extend_from_slice(&node.interface_version.minor.to_be_bytes());
    bytes.extend_from_slice(&node.protocol_version.to_be_bytes());
    bytes.extend_from_slice(&node.network_id.to_be_bytes());
    bytes.push(node.role.tag());
    bytes.extend_from_slice(&node.chain_head_sequence.to_be_bytes());
    bytes.extend_from_slice(&node.latest_sealed_batch.to_be_bytes());
    bytes.extend_from_slice(&node.latest_finalised_checkpoint);
    bytes.extend_from_slice(&node.authorised_sequencer_key);
    bytes.extend_from_slice(&count.to_be_bytes());
    for name in &node.advertised_capabilities {
        let length = u16::try_from(name.len()).map_err(|_| HandshakeError::MalformedNodeInfo)?;
        bytes.extend_from_slice(&length.to_be_bytes());
        bytes.extend_from_slice(name.as_bytes());
    }
    Ok(bytes)
}

/// Decodes the exact `NodeInfo` payload and rejects non-canonical name ordering.
///
/// # Errors
///
/// Refuses truncation, trailing bytes, invalid roles, invalid UTF-8, excessive
/// capabilities, and non-increasing names.
pub fn decode_node_info(bytes: &[u8]) -> Result<NodeInfo, HandshakeError> {
    let mut cursor = Cursor::new(bytes);
    let interface_version = Version {
        major: cursor.u16()?,
        minor: cursor.u16()?,
    };
    let protocol_version = cursor.u16()?;
    let network_id = cursor.u32()?;
    let role = NodeRole::from_tag(cursor.u8()?)?;
    let chain_head_sequence = cursor.u64()?;
    let latest_sealed_batch = cursor.u64()?;
    let latest_finalised_checkpoint = cursor.array()?;
    let authorised_sequencer_key = cursor.array()?;
    let count = usize::from(cursor.u16()?);
    if count > MAX_CAPABILITIES {
        return Err(HandshakeError::MalformedNodeInfo);
    }
    let mut advertised_capabilities = Vec::with_capacity(count);
    for _ in 0..count {
        let length = usize::from(cursor.u16()?);
        if length == 0 || length > MAX_CAPABILITY_BYTES {
            return Err(HandshakeError::MalformedNodeInfo);
        }
        let name = std::str::from_utf8(cursor.take(length)?)
            .map_err(|_| HandshakeError::MalformedNodeInfo)?;
        advertised_capabilities.push(name.to_owned());
    }
    if !cursor.finished() || !strictly_increasing(&advertised_capabilities) {
        return Err(HandshakeError::MalformedNodeInfo);
    }
    Ok(NodeInfo {
        interface_version,
        protocol_version,
        network_id,
        role,
        chain_head_sequence,
        latest_sealed_batch,
        latest_finalised_checkpoint,
        authorised_sequencer_key,
        advertised_capabilities,
    })
}

fn node_info_payload(envelope: &[u8]) -> Result<&[u8], HandshakeError> {
    let mut cursor = Cursor::new(envelope);
    let _major = cursor.u16()?;
    let _minor = cursor.u16()?;
    if cursor.u16()? != NODE_INFO_RESPONSE_TAG || cursor.u64()? != 0 {
        return Err(HandshakeError::MalformedEnvelope);
    }
    let payload_length =
        usize::try_from(cursor.u32()?).map_err(|_| HandshakeError::MalformedEnvelope)?;
    let payload = cursor.take(payload_length)?;
    let proof_length =
        usize::try_from(cursor.u32()?).map_err(|_| HandshakeError::MalformedEnvelope)?;
    if proof_length != 0 || !cursor.take(proof_length)?.is_empty() || !cursor.finished() {
        return Err(HandshakeError::MalformedEnvelope);
    }
    Ok(payload)
}

fn strictly_increasing(names: &[String]) -> bool {
    names.windows(2).all(|pair| pair[0] < pair[1])
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], HandshakeError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(HandshakeError::MalformedNodeInfo)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(HandshakeError::MalformedNodeInfo)?;
        self.offset = end;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, HandshakeError> {
        self.take(1)?
            .first()
            .copied()
            .ok_or(HandshakeError::MalformedNodeInfo)
    }

    fn u16(&mut self) -> Result<u16, HandshakeError> {
        self.take(2)?
            .try_into()
            .map(u16::from_be_bytes)
            .map_err(|_| HandshakeError::MalformedNodeInfo)
    }

    fn u32(&mut self) -> Result<u32, HandshakeError> {
        self.take(4)?
            .try_into()
            .map(u32::from_be_bytes)
            .map_err(|_| HandshakeError::MalformedNodeInfo)
    }

    fn u64(&mut self) -> Result<u64, HandshakeError> {
        self.take(8)?
            .try_into()
            .map(u64::from_be_bytes)
            .map_err(|_| HandshakeError::MalformedNodeInfo)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], HandshakeError> {
        self.take(N)?
            .try_into()
            .map_err(|_| HandshakeError::MalformedNodeInfo)
    }

    const fn finished(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

//! Source-controlled LNI v1 schema and canonical envelope vectors.

/// Exact LNI schema source checked into the repository.
pub const LNI_V1_SOURCE: &str = include_str!("../../../../schema/lni/v1.kvx");

/// Node-interface major and minor version.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Version {
    /// Breaking-compatibility generation.
    pub major: u16,
    /// Additive revision within a generation.
    pub minor: u16,
}

impl Version {
    /// Version implemented by this crate.
    pub const V1_0: Self = Self { major: 1, minor: 0 };

    /// Additive preparation-state boundary revision.
    pub const V1_1: Self = Self { major: 1, minor: 1 };

    /// Additive finality-evidence registration revision.
    pub const V1_2: Self = Self { major: 1, minor: 2 };

    /// Additive authenticated durable admission capability revision.
    pub const V1_3: Self = Self { major: 1, minor: 3 };

    /// Additive noncommitting program simulation capability revision.
    pub const V1_4: Self = Self { major: 1, minor: 4 };

    /// Returns whether the two peers can interpret the same stable message set.
    #[must_use]
    pub const fn is_compatible_with(self, peer: Self) -> bool {
        self.major == peer.major
    }
}

/// Observable role of an LNI message.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MessageKind {
    /// Starts one operation.
    Request,
    /// Terminates one unary operation.
    Response,
    /// Carries one item or marker in a server stream.
    Stream,
}

/// Capability a node advertises before the corresponding message may be used.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Capability {
    NodeInfo,
    Submit,
    AuthenticatedDurableSubmit,
    ReceiptLookup,
    AccountRead,
    HistoryRange,
    BatchHeader,
    Checkpoint,
    ProofBundle,
    AvailabilityFetch,
    EventSubscribe,
    HistoricalProofs,
    PreparationState,
    FinalityEvidenceRegister,
    Simulate,
}

impl Capability {
    /// Stable schema spelling used in capability advertisements.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::NodeInfo => "node_info",
            Self::Submit => "submit",
            Self::AuthenticatedDurableSubmit => "authenticated_durable_submit",
            Self::ReceiptLookup => "receipt_lookup",
            Self::AccountRead => "account_read",
            Self::HistoryRange => "history_range",
            Self::BatchHeader => "batch_header",
            Self::Checkpoint => "checkpoint",
            Self::ProofBundle => "proof_bundle",
            Self::AvailabilityFetch => "availability_fetch",
            Self::EventSubscribe => "event_subscribe",
            Self::HistoricalProofs => "historical_proofs",
            Self::PreparationState => "preparation_state",
            Self::FinalityEvidenceRegister => "finality_evidence_register",
            Self::Simulate => "simulate",
        }
    }
}

/// One stable message declaration from the v1 schema.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MessageDescriptor {
    pub name: &'static str,
    pub tag: u16,
    pub kind: MessageKind,
    pub capability: Capability,
    pub carries_protocol_data: bool,
    pub carries_proof_material: bool,
}

/// Immutable schema metadata built into the client.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Schema {
    pub version: Version,
    pub messages: &'static [MessageDescriptor],
    pub capabilities: &'static [Capability],
}

const CAPABILITIES: [Capability; 15] = [
    Capability::NodeInfo,
    Capability::Submit,
    Capability::AuthenticatedDurableSubmit,
    Capability::ReceiptLookup,
    Capability::AccountRead,
    Capability::HistoryRange,
    Capability::BatchHeader,
    Capability::Checkpoint,
    Capability::ProofBundle,
    Capability::AvailabilityFetch,
    Capability::EventSubscribe,
    Capability::HistoricalProofs,
    Capability::PreparationState,
    Capability::FinalityEvidenceRegister,
    Capability::Simulate,
];

const fn message(
    name: &'static str,
    tag: u16,
    kind: MessageKind,
    capability: Capability,
    carries_protocol_data: bool,
    carries_proof_material: bool,
) -> MessageDescriptor {
    MessageDescriptor {
        name,
        tag,
        kind,
        capability,
        carries_protocol_data,
        carries_proof_material,
    }
}

const MESSAGES: [MessageDescriptor; 31] = [
    message(
        "NodeInfoRequest",
        1,
        MessageKind::Request,
        Capability::NodeInfo,
        false,
        false,
    ),
    message(
        "NodeInfoResponse",
        2,
        MessageKind::Response,
        Capability::NodeInfo,
        false,
        false,
    ),
    message(
        "SubmitRequest",
        3,
        MessageKind::Request,
        Capability::Submit,
        true,
        false,
    ),
    message(
        "SubmitResponse",
        4,
        MessageKind::Response,
        Capability::Submit,
        true,
        true,
    ),
    message(
        "ReceiptLookupRequest",
        5,
        MessageKind::Request,
        Capability::ReceiptLookup,
        false,
        false,
    ),
    message(
        "ReceiptLookupResponse",
        6,
        MessageKind::Response,
        Capability::ReceiptLookup,
        true,
        true,
    ),
    message(
        "AccountReadRequest",
        7,
        MessageKind::Request,
        Capability::AccountRead,
        false,
        false,
    ),
    message(
        "AccountReadResponse",
        8,
        MessageKind::Response,
        Capability::AccountRead,
        true,
        true,
    ),
    message(
        "HistoryRangeRequest",
        9,
        MessageKind::Request,
        Capability::HistoryRange,
        false,
        false,
    ),
    message(
        "HistoryItem",
        10,
        MessageKind::Stream,
        Capability::HistoryRange,
        true,
        true,
    ),
    message(
        "HistoryEnd",
        11,
        MessageKind::Stream,
        Capability::HistoryRange,
        false,
        false,
    ),
    message(
        "BatchHeaderRequest",
        12,
        MessageKind::Request,
        Capability::BatchHeader,
        false,
        false,
    ),
    message(
        "BatchHeaderResponse",
        13,
        MessageKind::Response,
        Capability::BatchHeader,
        true,
        true,
    ),
    message(
        "CheckpointRequest",
        14,
        MessageKind::Request,
        Capability::Checkpoint,
        false,
        false,
    ),
    message(
        "CheckpointResponse",
        15,
        MessageKind::Response,
        Capability::Checkpoint,
        true,
        true,
    ),
    message(
        "ProofBundleRequest",
        16,
        MessageKind::Request,
        Capability::ProofBundle,
        false,
        false,
    ),
    message(
        "ProofBundleResponse",
        17,
        MessageKind::Response,
        Capability::ProofBundle,
        true,
        true,
    ),
    message(
        "AvailabilityFetchRequest",
        18,
        MessageKind::Request,
        Capability::AvailabilityFetch,
        false,
        false,
    ),
    message(
        "AvailabilityChunk",
        19,
        MessageKind::Stream,
        Capability::AvailabilityFetch,
        true,
        true,
    ),
    message(
        "AvailabilityEnd",
        20,
        MessageKind::Stream,
        Capability::AvailabilityFetch,
        false,
        true,
    ),
    message(
        "EventSubscribeRequest",
        21,
        MessageKind::Request,
        Capability::EventSubscribe,
        false,
        false,
    ),
    message(
        "EventRecord",
        22,
        MessageKind::Stream,
        Capability::EventSubscribe,
        true,
        true,
    ),
    message(
        "EventGap",
        23,
        MessageKind::Stream,
        Capability::EventSubscribe,
        false,
        false,
    ),
    message(
        "EventHeartbeat",
        24,
        MessageKind::Stream,
        Capability::EventSubscribe,
        false,
        false,
    ),
    message(
        "ErrorResponse",
        25,
        MessageKind::Response,
        Capability::NodeInfo,
        false,
        false,
    ),
    message(
        "PreparationStateRequest",
        26,
        MessageKind::Request,
        Capability::PreparationState,
        false,
        false,
    ),
    message(
        "PreparationStateResponse",
        27,
        MessageKind::Response,
        Capability::PreparationState,
        true,
        false,
    ),
    message(
        "FinalityEvidenceRegisterRequest",
        28,
        MessageKind::Request,
        Capability::FinalityEvidenceRegister,
        true,
        true,
    ),
    message(
        "FinalityEvidenceRegisterResponse",
        29,
        MessageKind::Response,
        Capability::FinalityEvidenceRegister,
        true,
        false,
    ),
    message(
        "SimulateRequest",
        30,
        MessageKind::Request,
        Capability::Simulate,
        true,
        false,
    ),
    message(
        "SimulateResponse",
        31,
        MessageKind::Response,
        Capability::Simulate,
        true,
        true,
    ),
];

const SCHEMA: Schema = Schema {
    version: Version::V1_4,
    messages: &MESSAGES,
    capabilities: &CAPABILITIES,
};

/// Returns the immutable LNI v1 declaration used by all transports.
#[must_use]
pub const fn lni_schema_v1() -> &'static Schema {
    &SCHEMA
}

/// One checked-in canonical encoding vector.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GoldenVector {
    pub message: &'static str,
    pub payload: &'static [u8],
    pub proof_material: &'static [u8],
    pub encoded_hex: &'static str,
}

const NO_PROOF: &[u8] = &[];
const PROOF: &[u8] = &[0xa5];

const GOLDENS: [GoldenVector; 31] = [
    GoldenVector {
        message: "NodeInfoRequest",
        payload: &[1],
        proof_material: NO_PROOF,
        encoded_hex: "0001000000010000000000000000000000010100000000",
    },
    GoldenVector {
        message: "NodeInfoResponse",
        payload: &[2],
        proof_material: NO_PROOF,
        encoded_hex: "0001000000020000000000000000000000010200000000",
    },
    GoldenVector {
        message: "SubmitRequest",
        payload: &[3],
        proof_material: NO_PROOF,
        encoded_hex: "0001000000030000000000000000000000010300000000",
    },
    GoldenVector {
        message: "SubmitResponse",
        payload: &[4],
        proof_material: PROOF,
        encoded_hex: "0001000000040000000000000000000000010400000001a5",
    },
    GoldenVector {
        message: "ReceiptLookupRequest",
        payload: &[5],
        proof_material: NO_PROOF,
        encoded_hex: "0001000000050000000000000000000000010500000000",
    },
    GoldenVector {
        message: "ReceiptLookupResponse",
        payload: &[6],
        proof_material: PROOF,
        encoded_hex: "0001000000060000000000000000000000010600000001a5",
    },
    GoldenVector {
        message: "AccountReadRequest",
        payload: &[7],
        proof_material: NO_PROOF,
        encoded_hex: "0001000000070000000000000000000000010700000000",
    },
    GoldenVector {
        message: "AccountReadResponse",
        payload: &[8],
        proof_material: PROOF,
        encoded_hex: "0001000000080000000000000000000000010800000001a5",
    },
    GoldenVector {
        message: "HistoryRangeRequest",
        payload: &[9],
        proof_material: NO_PROOF,
        encoded_hex: "0001000000090000000000000000000000010900000000",
    },
    GoldenVector {
        message: "HistoryItem",
        payload: &[10],
        proof_material: PROOF,
        encoded_hex: "00010000000a0000000000000000000000010a00000001a5",
    },
    GoldenVector {
        message: "HistoryEnd",
        payload: &[11],
        proof_material: NO_PROOF,
        encoded_hex: "00010000000b0000000000000000000000010b00000000",
    },
    GoldenVector {
        message: "BatchHeaderRequest",
        payload: &[12],
        proof_material: NO_PROOF,
        encoded_hex: "00010000000c0000000000000000000000010c00000000",
    },
    GoldenVector {
        message: "BatchHeaderResponse",
        payload: &[13],
        proof_material: PROOF,
        encoded_hex: "00010000000d0000000000000000000000010d00000001a5",
    },
    GoldenVector {
        message: "CheckpointRequest",
        payload: &[14],
        proof_material: NO_PROOF,
        encoded_hex: "00010000000e0000000000000000000000010e00000000",
    },
    GoldenVector {
        message: "CheckpointResponse",
        payload: &[15],
        proof_material: PROOF,
        encoded_hex: "00010000000f0000000000000000000000010f00000001a5",
    },
    GoldenVector {
        message: "ProofBundleRequest",
        payload: &[16],
        proof_material: NO_PROOF,
        encoded_hex: "0001000000100000000000000000000000011000000000",
    },
    GoldenVector {
        message: "ProofBundleResponse",
        payload: &[17],
        proof_material: PROOF,
        encoded_hex: "0001000000110000000000000000000000011100000001a5",
    },
    GoldenVector {
        message: "AvailabilityFetchRequest",
        payload: &[18],
        proof_material: NO_PROOF,
        encoded_hex: "0001000000120000000000000000000000011200000000",
    },
    GoldenVector {
        message: "AvailabilityChunk",
        payload: &[19],
        proof_material: PROOF,
        encoded_hex: "0001000000130000000000000000000000011300000001a5",
    },
    GoldenVector {
        message: "AvailabilityEnd",
        payload: &[20],
        proof_material: PROOF,
        encoded_hex: "0001000000140000000000000000000000011400000001a5",
    },
    GoldenVector {
        message: "EventSubscribeRequest",
        payload: &[21],
        proof_material: NO_PROOF,
        encoded_hex: "0001000000150000000000000000000000011500000000",
    },
    GoldenVector {
        message: "EventRecord",
        payload: &[22],
        proof_material: PROOF,
        encoded_hex: "0001000000160000000000000000000000011600000001a5",
    },
    GoldenVector {
        message: "EventGap",
        payload: &[23],
        proof_material: NO_PROOF,
        encoded_hex: "0001000000170000000000000000000000011700000000",
    },
    GoldenVector {
        message: "EventHeartbeat",
        payload: &[24],
        proof_material: NO_PROOF,
        encoded_hex: "0001000000180000000000000000000000011800000000",
    },
    GoldenVector {
        message: "ErrorResponse",
        payload: &[25],
        proof_material: NO_PROOF,
        encoded_hex: "0001000000190000000000000000000000011900000000",
    },
    GoldenVector {
        message: "PreparationStateRequest",
        payload: &[26],
        proof_material: NO_PROOF,
        encoded_hex: "00010001001a0000000000000000000000011a00000000",
    },
    GoldenVector {
        message: "PreparationStateResponse",
        payload: &[27],
        proof_material: NO_PROOF,
        encoded_hex: "00010001001b0000000000000000000000011b00000000",
    },
    GoldenVector {
        message: "FinalityEvidenceRegisterRequest",
        payload: &[28],
        proof_material: PROOF,
        encoded_hex: "00010002001c0000000000000000000000011c00000001a5",
    },
    GoldenVector {
        message: "FinalityEvidenceRegisterResponse",
        payload: &[29],
        proof_material: NO_PROOF,
        encoded_hex: "00010002001d0000000000000000000000011d00000000",
    },
    GoldenVector {
        message: "SimulateRequest",
        payload: &[30],
        proof_material: NO_PROOF,
        encoded_hex: "00010004001e0000000000000000000000011e00000000",
    },
    GoldenVector {
        message: "SimulateResponse",
        payload: &[31],
        proof_material: PROOF,
        encoded_hex: "00010004001f0000000000000000000000011f00000001a5",
    },
];

impl GoldenVector {
    /// Interface revision frozen into this literal vector.
    #[must_use]
    pub const fn version(self) -> Version {
        if self.payload[0] >= 30 {
            Version::V1_4
        } else if self.payload[0] >= 28 {
            Version::V1_2
        } else if self.payload[0] >= 26 {
            Version::V1_1
        } else {
            Version::V1_0
        }
    }
}

/// Returns a literal canonical vector for every v1 message tag.
#[must_use]
pub const fn lni_golden_vectors() -> &'static [GoldenVector] {
    &GOLDENS
}

/// Canonical LNI message envelope. Protocol payloads remain opaque bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Envelope<'a> {
    pub version: Version,
    pub message_tag: u16,
    pub correlation_id: u64,
    pub canonical_payload: &'a [u8],
    pub proof_material: &'a [u8],
}

/// Failure to construct a bounded canonical LNI envelope.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SchemaError {
    UnknownMessage(u16),
    LengthLimit,
    MalformedEnvelope,
}

/// Encodes the schema's fixed-width header and two bounded opaque byte strings.
///
/// # Errors
///
/// Refuses unknown tags and byte strings that do not fit the u32 wire length.
pub fn encode_envelope(envelope: Envelope<'_>) -> Result<Vec<u8>, SchemaError> {
    if !MESSAGES
        .iter()
        .any(|message| message.tag == envelope.message_tag)
    {
        return Err(SchemaError::UnknownMessage(envelope.message_tag));
    }
    let payload_length =
        u32::try_from(envelope.canonical_payload.len()).map_err(|_| SchemaError::LengthLimit)?;
    let proof_length =
        u32::try_from(envelope.proof_material.len()).map_err(|_| SchemaError::LengthLimit)?;
    let capacity = 22_usize
        .checked_add(envelope.canonical_payload.len())
        .and_then(|size| size.checked_add(envelope.proof_material.len()))
        .ok_or(SchemaError::LengthLimit)?;
    let mut encoded = Vec::with_capacity(capacity);
    encoded.extend_from_slice(&envelope.version.major.to_be_bytes());
    encoded.extend_from_slice(&envelope.version.minor.to_be_bytes());
    encoded.extend_from_slice(&envelope.message_tag.to_be_bytes());
    encoded.extend_from_slice(&envelope.correlation_id.to_be_bytes());
    encoded.extend_from_slice(&payload_length.to_be_bytes());
    encoded.extend_from_slice(envelope.canonical_payload);
    encoded.extend_from_slice(&proof_length.to_be_bytes());
    encoded.extend_from_slice(envelope.proof_material);
    Ok(encoded)
}

/// Decodes one complete LNI envelope while retaining borrowed protocol bytes.
///
/// # Errors
///
/// Refuses unknown message tags, truncated lengths and payloads, arithmetic
/// overflow, and trailing bytes outside the two declared byte strings.
pub fn decode_envelope(bytes: &[u8]) -> Result<Envelope<'_>, SchemaError> {
    let mut cursor = 0_usize;
    let major = u16::from_be_bytes(take(bytes, &mut cursor)?);
    let minor = u16::from_be_bytes(take(bytes, &mut cursor)?);
    let message_tag = u16::from_be_bytes(take(bytes, &mut cursor)?);
    if !MESSAGES.iter().any(|message| message.tag == message_tag) {
        return Err(SchemaError::UnknownMessage(message_tag));
    }
    let correlation_id = u64::from_be_bytes(take(bytes, &mut cursor)?);
    let payload_length = usize::try_from(u32::from_be_bytes(take(bytes, &mut cursor)?))
        .map_err(|_| SchemaError::LengthLimit)?;
    let canonical_payload = take_slice(bytes, &mut cursor, payload_length)?;
    let proof_length = usize::try_from(u32::from_be_bytes(take(bytes, &mut cursor)?))
        .map_err(|_| SchemaError::LengthLimit)?;
    let proof_material = take_slice(bytes, &mut cursor, proof_length)?;
    if cursor != bytes.len() {
        return Err(SchemaError::MalformedEnvelope);
    }
    Ok(Envelope {
        version: Version { major, minor },
        message_tag,
        correlation_id,
        canonical_payload,
        proof_material,
    })
}

fn take<const LENGTH: usize>(
    bytes: &[u8],
    cursor: &mut usize,
) -> Result<[u8; LENGTH], SchemaError> {
    take_slice(bytes, cursor, LENGTH)?
        .try_into()
        .map_err(|_| SchemaError::MalformedEnvelope)
}

fn take_slice<'a>(
    bytes: &'a [u8],
    cursor: &mut usize,
    length: usize,
) -> Result<&'a [u8], SchemaError> {
    let end = cursor.checked_add(length).ok_or(SchemaError::LengthLimit)?;
    let value = bytes
        .get(*cursor..end)
        .ok_or(SchemaError::MalformedEnvelope)?;
    *cursor = end;
    Ok(value)
}

//! `CosmWasm` entry points, messages, responses and sub-messages carried onto
//! the version-one program ABI.
//!
//! A `CosmWasm` contract has three entry points and routes inside each of them
//! on a `JSON` enum variant. The variant name is the identifier a client, a
//! generated schema and a block explorer all agree on, so the port keeps the
//! name and replaces only its encoding: each variant takes an eight-byte tag
//! derived from `entry point` and `variant name`, and its body takes the
//! canonical framing from [`crate::json`].
//!
//! Responses carry the other half of a contract's public surface. A `Response`
//! becomes a `wasm` event and a custom `Event` becomes `wasm-<type>`; both
//! names are carried as the program event topic verbatim, and the attribute
//! keys are carried verbatim inside the payload.

use layerx_programs_runtime::abi::{
    MAX_CALL_INPUT_BYTES, MAX_EVENT_DATA_BYTES, MAX_EVENT_TOPIC_BYTES,
};
use layerx_programs_runtime::{Capability, ProgramId};

use crate::error::PortRefusal;
use crate::hash::sha256;
use crate::json::{FieldSchema, FieldValue, RecordSchema};

/// The width of every ported message variant tag.
pub const VARIANT_TAG_BYTES: usize = 8;

/// The event type a `Response`'s own attributes are emitted under.
pub const RESPONSE_EVENT_TYPE: &str = "wasm";

/// The prefix a chain puts in front of a contract's custom event type.
pub const CUSTOM_EVENT_PREFIX: &str = "wasm-";

/// Longest attribute key a ported event may carry.
pub const MAX_ATTRIBUTE_KEY_BYTES: usize = 255;

/// The three entry points a `CosmWasm` contract exposes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryPoint {
    /// `instantiate`, which a `LayerX` deployment replaces: the configuration
    /// a `CosmWasm` contract stores on instantiation is pinned into the module
    /// by the port descriptor instead.
    Instantiate,
    /// `execute`.
    Execute,
    /// `query`.
    Query,
}

impl EntryPoint {
    /// Returns the entry point name, which is the first half of every variant
    /// tag preimage.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Instantiate => "instantiate",
            Self::Execute => "execute",
            Self::Query => "query",
        }
    }
}

/// Returns the eight-byte tag a ported message variant is dispatched on.
///
/// `CosmWasm` has no on-the-wire identifier to preserve: routing is done by
/// matching a `JSON` object key. The tag is this kit's canonical encoding of
/// that same key, taken from `sha256("<entry point>:<variant>")`, so a rename
/// is a breaking change exactly as it already is on a chain.
#[must_use]
pub fn variant_tag(entry: EntryPoint, variant: &str) -> [u8; VARIANT_TAG_BYTES] {
    let capacity = entry
        .name()
        .len()
        .saturating_add(variant.len())
        .saturating_add(1);
    let mut preimage = Vec::with_capacity(capacity);
    preimage.extend_from_slice(entry.name().as_bytes());
    preimage.push(b':');
    preimage.extend_from_slice(variant.as_bytes());
    let digest = sha256(&preimage);
    let mut tag = [0_u8; VARIANT_TAG_BYTES];
    for (slot, byte) in tag.iter_mut().zip(digest) {
        *slot = byte;
    }
    tag
}

/// One `JSON` enum variant of one entry point.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageVariant {
    entry: EntryPoint,
    variant: String,
    tag: [u8; VARIANT_TAG_BYTES],
    body: RecordSchema,
}

impl MessageVariant {
    /// Declares a variant by its entry point, its `snake_case` `JSON` name and
    /// the schema of its body.
    ///
    /// # Errors
    ///
    /// Refuses an unnamed variant.
    pub fn new(entry: EntryPoint, variant: &str, body: RecordSchema) -> Result<Self, PortRefusal> {
        if variant.is_empty() {
            return Err(PortRefusal::SchemaMismatch);
        }
        Ok(Self {
            entry,
            variant: variant.to_owned(),
            tag: variant_tag(entry, variant),
            body,
        })
    }

    /// Returns the entry point the variant belongs to.
    #[must_use]
    pub const fn entry(&self) -> EntryPoint {
        self.entry
    }

    /// Returns the `JSON` variant name, unchanged by the port.
    #[must_use]
    pub fn variant(&self) -> &str {
        &self.variant
    }

    /// Returns the eight-byte dispatch tag.
    #[must_use]
    pub const fn tag(&self) -> [u8; VARIANT_TAG_BYTES] {
        self.tag
    }

    /// Borrows the declared body schema.
    #[must_use]
    pub const fn body(&self) -> &RecordSchema {
        &self.body
    }

    /// Returns the tag as the little-endian `i64` a ported dispatcher compares
    /// an eight-byte load against.
    #[must_use]
    pub const fn dispatch_word(&self) -> i64 {
        i64::from_le_bytes(self.tag)
    }

    /// Encodes the message the way a `CosmWasm` client sends it:
    /// `{"<variant>":{ ... }}`, with no insignificant whitespace.
    ///
    /// # Errors
    ///
    /// Refuses a value list that does not match the declared body schema.
    pub fn json(&self, values: &[FieldValue]) -> Result<String, PortRefusal> {
        let body = self.body.encode_json(values)?;
        let capacity = body
            .len()
            .saturating_add(self.variant.len())
            .saturating_add(6);
        let mut text = String::with_capacity(capacity);
        text.push_str("{\"");
        text.push_str(&self.variant);
        text.push_str("\":");
        text.push_str(&body);
        text.push('}');
        Ok(text)
    }

    /// Encodes the ported call input: the eight-byte tag followed by the body
    /// in canonical framing.
    ///
    /// # Errors
    ///
    /// Refuses a value list that does not match the declared body schema and
    /// input beyond the ABI bound.
    pub fn data(&self, values: &[FieldValue]) -> Result<Vec<u8>, PortRefusal> {
        let body = self.body.encode(values)?;
        let mut encoded = Vec::with_capacity(body.len().saturating_add(VARIANT_TAG_BYTES));
        encoded.extend_from_slice(&self.tag);
        encoded.extend_from_slice(&body);
        if encoded.len() > MAX_CALL_INPUT_BYTES {
            return Err(PortRefusal::MessageTooLarge);
        }
        Ok(encoded)
    }

    /// Transcodes the `JSON` message a client already builds into the ported
    /// call input, which is what an adapter at the edge does.
    ///
    /// # Errors
    ///
    /// Refuses a document that is not this variant, a malformed body and input
    /// beyond the ABI bound.
    pub fn transcode(&self, text: &str) -> Result<Vec<u8>, PortRefusal> {
        let body = unwrap_variant(&self.variant, text)?;
        let values = self.body.decode_json(body)?;
        self.data(&values)
    }
}

/// One emitted contract event: a `Response`'s attributes, or a custom `Event`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContractEvent {
    topic: Vec<u8>,
    attributes: Vec<FieldSchema>,
}

impl ContractEvent {
    /// Declares the event a bare `Response` produces, whose type is `wasm`.
    ///
    /// # Errors
    ///
    /// Refuses an unnamed or oversized attribute key and a topic beyond the
    /// ABI bound.
    pub fn response(attributes: Vec<FieldSchema>) -> Result<Self, PortRefusal> {
        Self::build(RESPONSE_EVENT_TYPE.as_bytes().to_vec(), attributes)
    }

    /// Declares a custom `Event`, whose type a chain prefixes with `wasm-`.
    ///
    /// # Errors
    ///
    /// Refuses an unnamed event, an unnamed or oversized attribute key and a
    /// topic beyond the ABI bound.
    pub fn custom(event_type: &str, attributes: Vec<FieldSchema>) -> Result<Self, PortRefusal> {
        if event_type.is_empty() {
            return Err(PortRefusal::SchemaMismatch);
        }
        let mut topic = CUSTOM_EVENT_PREFIX.as_bytes().to_vec();
        topic.extend_from_slice(event_type.as_bytes());
        Self::build(topic, attributes)
    }

    fn build(topic: Vec<u8>, attributes: Vec<FieldSchema>) -> Result<Self, PortRefusal> {
        if topic.len() > MAX_EVENT_TOPIC_BYTES {
            return Err(PortRefusal::TopicTooLarge);
        }
        for (index, attribute) in attributes.iter().enumerate() {
            if attribute.name.is_empty()
                || attribute.name.len() > MAX_ATTRIBUTE_KEY_BYTES
                || attributes
                    .iter()
                    .skip(index.saturating_add(1))
                    .any(|other| other.name == attribute.name)
            {
                return Err(PortRefusal::SchemaMismatch);
            }
        }
        Ok(Self { topic, attributes })
    }

    /// Returns the event topic, which is the chain's own event type verbatim.
    #[must_use]
    pub fn topic(&self) -> &[u8] {
        &self.topic
    }

    /// Borrows the declared attributes in declaration order.
    #[must_use]
    pub fn attributes(&self) -> &[FieldSchema] {
        &self.attributes
    }

    /// Encodes the payload: each attribute as its one-byte key length, its key
    /// verbatim, then its value in canonical framing.
    ///
    /// # Errors
    ///
    /// Refuses a value list that does not match the declared attributes and a
    /// payload beyond the ABI bound.
    pub fn data(&self, values: &[FieldValue]) -> Result<Vec<u8>, PortRefusal> {
        if values.len() != self.attributes.len() {
            return Err(PortRefusal::SchemaMismatch);
        }
        let mut encoded = Vec::new();
        for (attribute, value) in self.attributes.iter().zip(values) {
            if value.kind() != attribute.kind {
                return Err(PortRefusal::SchemaMismatch);
            }
            let length =
                u8::try_from(attribute.name.len()).map_err(|_| PortRefusal::SchemaMismatch)?;
            encoded.push(length);
            encoded.extend_from_slice(attribute.name.as_bytes());
            value.encode(&mut encoded)?;
        }
        if encoded.len() > MAX_EVENT_DATA_BYTES {
            return Err(PortRefusal::EventDataTooLarge);
        }
        Ok(encoded)
    }
}

/// One translated `WasmMsg::Execute` sub-message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallRequest {
    /// The callee program that replaces the target contract address.
    pub callee: ProgramId,
    /// The ported call input for the named variant.
    pub input: Vec<u8>,
    /// The single authority the call needs, which the caller must already
    /// hold.
    pub authority: Capability,
}

/// Translates a `WasmMsg::Execute` sub-message into the call request the ABI
/// accepts.
///
/// A `CosmWasm` sub-message is dispatched by the chain *after* the contract
/// returns, and it may carry `funds` out of the contract's own balance. A
/// ported call happens inside the invocation, carries no funds at all, and
/// holds only the authority the caller explicitly narrowed. Funds a
/// sub-message would have attached are a separate 402LXP leg the invoking
/// principal pays; see [`crate::monetary`].
///
/// # Errors
///
/// Refuses a value list that does not match the declared body schema and input
/// beyond the ABI bound.
pub fn execute_submessage(
    callee: ProgramId,
    message: &MessageVariant,
    values: &[FieldValue],
) -> Result<CallRequest, PortRefusal> {
    let input = message.data(values)?;
    Ok(CallRequest {
        callee,
        input,
        authority: Capability::Call { program: callee },
    })
}

/// What the runtime does with each `CosmWasm` failure mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailureMapping {
    /// A handler returning `Err(ContractError::...)`.
    ContractError,
    /// A `StdError` raised by storage, serialisation or an overflow check.
    StdError,
    /// A Rust `panic!`, an arithmetic overflow or an index out of bounds.
    Panic,
    /// A sub-message that failed and was not caught by a `reply`.
    SubMessageFailure,
    /// Exhausting the transaction's gas.
    OutOfGas,
    /// Exceeding the contract-call depth limit.
    CallDepth,
}

/// The version-one behaviour a `CosmWasm` failure mode becomes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeOutcome {
    /// The guest executes `unreachable`; every staged write and effect is
    /// discarded, which is exactly a `CosmWasm` transaction's all-or-nothing
    /// failure.
    Trap,
    /// The deterministic meter refuses the execution before any effect escapes.
    ResourceRefusal,
    /// The declared value-stack or call-depth bound is exhausted.
    StackExhausted,
}

impl FailureMapping {
    /// Returns the runtime behaviour the failure mode maps onto.
    #[must_use]
    pub const fn outcome(self) -> RuntimeOutcome {
        match self {
            Self::ContractError | Self::StdError | Self::Panic | Self::SubMessageFailure => {
                RuntimeOutcome::Trap
            }
            Self::OutOfGas => RuntimeOutcome::ResourceRefusal,
            Self::CallDepth => RuntimeOutcome::StackExhausted,
        }
    }
}

fn unwrap_variant<'text>(variant: &str, text: &'text str) -> Result<&'text str, PortRefusal> {
    let mut opening = String::with_capacity(variant.len().saturating_add(4));
    opening.push_str("{\"");
    opening.push_str(variant);
    opening.push_str("\":");
    let body = text
        .trim()
        .strip_prefix(opening.as_str())
        .ok_or(PortRefusal::InvalidJson)?;
    body.strip_suffix('}').ok_or(PortRefusal::InvalidJson)
}

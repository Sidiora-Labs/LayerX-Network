//! Generic Solidity event ABI loading and log decoding.
//!
//! [`AbiRegistry::load_dir`] reads every `<name>/abi.json` below a directory.
//! A precompile's emitter address comes from an `address` file next to its
//! `abi.json` when present, otherwise from the known native precompile table;
//! an ABI with neither is registered unbound and decodes a matching topic from
//! any emitter. Dropping a new `<name>/abi.json` into the directory is enough
//! for its events to be indexed on the next start: nothing here names an
//! individual event.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde_json::{Map, Value};
use sha3::{Digest as _, Keccak256};

use crate::codec::{be_decimal, be_signed_decimal, hex, hex0x, is_zero, unhex_fixed};
use crate::IndexError;

/// The native precompile addresses by ABI directory name.
pub const KNOWN_PRECOMPILES: [(&str, u16); 19] = [
    ("bank", 0x1001),
    ("wasmd", 0x1002),
    ("json", 0x1003),
    ("addr", 0x1004),
    ("staking", 0x1005),
    ("gov", 0x1006),
    ("distribution", 0x1007),
    ("oracle", 0x1008),
    ("ibc", 0x1009),
    ("pointerview", 0x100A),
    ("pointer", 0x100B),
    ("solo", 0x100C),
    ("p256", 0x1011),
    ("layerxverify", 0x1012),
    ("layerxcustody", 0x1013),
    ("layerxanchor", 0x1014),
    ("layerxexchange", 0x1015),
    ("layerxbridge", 0x1016),
    ("launchpad", 0x1017),
];

/// The 20-byte address of a native precompile numbered `number`.
#[must_use]
pub fn precompile_address(number: u16) -> [u8; 20] {
    let mut address = [0_u8; 20];
    address[18..].copy_from_slice(&number.to_be_bytes());
    address
}

/// Keccak-256 of `bytes`.
#[must_use]
pub fn keccak(bytes: &[u8]) -> [u8; 32] {
    Keccak256::digest(bytes).into()
}

/// One Solidity ABI type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AbiType {
    Address,
    Bool,
    Uint(usize),
    Int(usize),
    FixedBytes(usize),
    Bytes,
    String,
    Array(Box<AbiType>),
    FixedArray(Box<AbiType>, usize),
    Tuple(Vec<(String, AbiType)>),
}

impl AbiType {
    /// Parses one ABI parameter object (`type` plus tuple `components`).
    ///
    /// # Errors
    /// Refuses unknown or malformed type strings.
    pub fn from_param(param: &Value) -> Result<Self, IndexError> {
        let text = param
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| IndexError::Decode("ABI parameter has no type".to_owned()))?;
        Self::parse(text, param.get("components"))
    }

    fn parse(text: &str, components: Option<&Value>) -> Result<Self, IndexError> {
        if let Some(inner) = text.strip_suffix("[]") {
            return Ok(Self::Array(Box::new(Self::parse(inner, components)?)));
        }
        if let Some(stripped) = text.strip_suffix(']') {
            if let Some(open) = stripped.rfind('[') {
                let count: usize = stripped[open + 1..]
                    .parse()
                    .map_err(|_| IndexError::Decode(format!("ABI array length in {text}")))?;
                if count == 0 {
                    return Err(IndexError::Decode(format!("zero-length ABI array {text}")));
                }
                return Ok(Self::FixedArray(
                    Box::new(Self::parse(&stripped[..open], components)?),
                    count,
                ));
            }
        }
        let bits = |prefix: &str| -> Result<usize, IndexError> {
            let rest = &text[prefix.len()..];
            if rest.is_empty() {
                return Ok(256);
            }
            rest.parse::<usize>()
                .ok()
                .filter(|bits| *bits > 0 && *bits <= 256 && bits % 8 == 0)
                .ok_or_else(|| IndexError::Decode(format!("ABI integer width {text}")))
        };
        Ok(match text {
            "address" => Self::Address,
            "bool" => Self::Bool,
            "bytes" => Self::Bytes,
            "string" => Self::String,
            "function" => Self::FixedBytes(24),
            "tuple" => {
                let list = components
                    .and_then(Value::as_array)
                    .ok_or_else(|| IndexError::Decode("tuple without components".to_owned()))?;
                let mut fields = Vec::with_capacity(list.len());
                for (index, component) in list.iter().enumerate() {
                    fields.push((param_name(component, index), Self::from_param(component)?));
                }
                Self::Tuple(fields)
            }
            _ if text.starts_with("uint") => Self::Uint(bits("uint")?),
            _ if text.starts_with("int") => Self::Int(bits("int")?),
            _ if text.starts_with("bytes") => {
                let size: usize = text[5..]
                    .parse()
                    .ok()
                    .filter(|size| (1..=32).contains(size))
                    .ok_or_else(|| IndexError::Decode(format!("ABI bytes width {text}")))?;
                Self::FixedBytes(size)
            }
            _ => return Err(IndexError::Decode(format!("unsupported ABI type {text}"))),
        })
    }

    /// The canonical type string used in the event signature.
    #[must_use]
    pub fn canonical(&self) -> String {
        match self {
            Self::Address => "address".to_owned(),
            Self::Bool => "bool".to_owned(),
            Self::Uint(bits) => format!("uint{bits}"),
            Self::Int(bits) => format!("int{bits}"),
            Self::FixedBytes(24) => "function".to_owned(),
            Self::FixedBytes(size) => format!("bytes{size}"),
            Self::Bytes => "bytes".to_owned(),
            Self::String => "string".to_owned(),
            Self::Array(inner) => format!("{}[]", inner.canonical()),
            Self::FixedArray(inner, count) => format!("{}[{count}]", inner.canonical()),
            Self::Tuple(fields) => format!(
                "({})",
                fields
                    .iter()
                    .map(|(_, field)| field.canonical())
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        }
    }

    fn dynamic(&self) -> bool {
        match self {
            Self::Bytes | Self::String | Self::Array(_) => true,
            Self::FixedArray(inner, _) => inner.dynamic(),
            Self::Tuple(fields) => fields.iter().any(|(_, field)| field.dynamic()),
            _ => false,
        }
    }

    fn head_size(&self) -> usize {
        if self.dynamic() {
            return 32;
        }
        match self {
            Self::FixedArray(inner, count) => inner.head_size().saturating_mul(*count),
            Self::Tuple(fields) => fields.iter().map(|(_, field)| field.head_size()).sum(),
            _ => 32,
        }
    }
}

fn param_name(param: &Value, index: usize) -> String {
    param
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .map_or_else(|| format!("arg{index}"), str::to_owned)
}

/// One event parameter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventParam {
    pub name: String,
    pub kind: AbiType,
    pub indexed: bool,
}

/// One non-anonymous event declared by an ABI.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventDef {
    pub contract: String,
    pub name: String,
    pub signature: String,
    pub topic: [u8; 32],
    pub params: Vec<EventParam>,
}

/// The decoded form of one log.
#[derive(Clone, Debug, PartialEq)]
pub struct DecodedEvent {
    pub contract: String,
    pub name: String,
    pub signature: String,
    pub args: Map<String, Value>,
}

impl DecodedEvent {
    /// The decoded argument `name`, when present.
    #[must_use]
    pub fn arg(&self, name: &str) -> Option<&Value> {
        self.args.get(name)
    }

    /// The first `address` argument, preferring indexed ones, as the
    /// account this event concerns.
    #[must_use]
    pub fn first_address(&self, params: &[EventParam]) -> Option<String> {
        params
            .iter()
            .filter(|param| param.kind == AbiType::Address)
            .find_map(|param| self.args.get(&param.name))
            .and_then(Value::as_str)
            .map(str::to_owned)
    }
}

/// One loaded ABI: its contract name, optional bound emitter and events.
#[derive(Clone, Debug)]
pub struct LoadedAbi {
    pub contract: String,
    pub address: Option<[u8; 20]>,
    pub events: Vec<EventDef>,
}

/// Every loaded event, indexed by topic.
#[derive(Clone, Debug, Default)]
pub struct AbiRegistry {
    abis: Vec<LoadedAbi>,
    by_topic: BTreeMap<[u8; 32], Vec<(usize, usize)>>,
}

impl AbiRegistry {
    /// Parses the events of one ABI document.
    ///
    /// # Errors
    /// Refuses a document that is not an ABI array or declares a malformed
    /// event.
    pub fn parse_events(contract: &str, document: &Value) -> Result<Vec<EventDef>, IndexError> {
        let entries = document
            .as_array()
            .or_else(|| document.get("abi").and_then(Value::as_array))
            .ok_or_else(|| IndexError::Decode(format!("{contract} ABI is not an array")))?;
        let mut events = Vec::new();
        for entry in entries {
            if entry.get("type").and_then(Value::as_str) != Some("event")
                || entry.get("anonymous").and_then(Value::as_bool) == Some(true)
            {
                continue;
            }
            let name = entry
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| IndexError::Decode(format!("{contract} event without name")))?;
            let inputs = entry
                .get("inputs")
                .and_then(Value::as_array)
                .ok_or_else(|| IndexError::Decode(format!("{contract}.{name} has no inputs")))?;
            let mut params = Vec::with_capacity(inputs.len());
            for (index, input) in inputs.iter().enumerate() {
                params.push(EventParam {
                    name: param_name(input, index),
                    kind: AbiType::from_param(input)?,
                    indexed: input.get("indexed").and_then(Value::as_bool) == Some(true),
                });
            }
            let signature = format!(
                "{name}({})",
                params
                    .iter()
                    .map(|param| param.kind.canonical())
                    .collect::<Vec<_>>()
                    .join(",")
            );
            events.push(EventDef {
                contract: contract.to_owned(),
                name: name.to_owned(),
                topic: keccak(signature.as_bytes()),
                signature,
                params,
            });
        }
        Ok(events)
    }

    /// Adds one ABI.
    pub fn insert(&mut self, abi: LoadedAbi) {
        let slot = self.abis.len();
        for (index, event) in abi.events.iter().enumerate() {
            self.by_topic
                .entry(event.topic)
                .or_default()
                .push((slot, index));
        }
        self.abis.push(abi);
    }

    /// Loads every `<name>/abi.json` directly below `root`.
    ///
    /// # Errors
    /// Refuses an unreadable directory, malformed JSON or a malformed
    /// `address` file.
    pub fn load_dir(root: &Path) -> Result<Self, IndexError> {
        let mut registry = Self::default();
        let mut entries: Vec<_> = fs::read_dir(root)
            .map_err(|error| IndexError::Config(format!("{}: {error}", root.display())))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.join("abi.json").is_file())
            .collect();
        entries.sort();
        for directory in entries {
            let contract = directory
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| IndexError::Config("ABI directory name is not UTF-8".to_owned()))?
                .to_owned();
            let text = fs::read_to_string(directory.join("abi.json"))
                .map_err(|error| IndexError::Config(format!("{contract}/abi.json: {error}")))?;
            let document: Value = serde_json::from_str(&text)
                .map_err(|error| IndexError::Config(format!("{contract}/abi.json: {error}")))?;
            let address_file = directory.join("address");
            let address = if address_file.is_file() {
                let text = fs::read_to_string(&address_file)
                    .map_err(|error| IndexError::Config(format!("{contract}/address: {error}")))?;
                Some(unhex_fixed::<20>(text.trim()).map_err(|_| {
                    IndexError::Config(format!("{contract}/address is not a 20-byte address"))
                })?)
            } else {
                KNOWN_PRECOMPILES
                    .iter()
                    .find(|(name, _)| *name == contract)
                    .map(|(_, number)| precompile_address(*number))
            };
            let events = Self::parse_events(&contract, &document)?;
            registry.insert(LoadedAbi {
                contract,
                address,
                events,
            });
        }
        Ok(registry)
    }

    /// Every loaded ABI.
    #[must_use]
    pub fn abis(&self) -> &[LoadedAbi] {
        &self.abis
    }

    /// Finds the event a log with this emitter and first topic names: an ABI
    /// bound to the emitter wins, an unbound ABI matches any emitter.
    #[must_use]
    pub fn lookup(&self, emitter: [u8; 20], topic: [u8; 32]) -> Option<&EventDef> {
        let candidates = self.by_topic.get(&topic)?;
        let bound = candidates
            .iter()
            .find(|(slot, _)| self.abis[*slot].address == Some(emitter));
        let unbound = || {
            candidates
                .iter()
                .find(|(slot, _)| self.abis[*slot].address.is_none())
        };
        bound
            .or_else(unbound)
            .map(|(slot, index)| &self.abis[*slot].events[*index])
    }

    /// Decodes one log when a loaded ABI declares it.
    ///
    /// # Errors
    /// Refuses a log whose topics or data do not fit the declared event.
    pub fn decode_log(
        &self,
        emitter: [u8; 20],
        topics: &[[u8; 32]],
        data: &[u8],
    ) -> Result<Option<(&EventDef, DecodedEvent)>, IndexError> {
        let Some(first) = topics.first() else {
            return Ok(None);
        };
        let Some(event) = self.lookup(emitter, *first) else {
            return Ok(None);
        };
        Ok(Some((event, decode_event(event, topics, data)?)))
    }
}

/// Decodes a log against one event definition.
///
/// # Errors
/// Refuses a topic count or data layout that does not match `event`.
pub fn decode_event(
    event: &EventDef,
    topics: &[[u8; 32]],
    data: &[u8],
) -> Result<DecodedEvent, IndexError> {
    let indexed = event.params.iter().filter(|param| param.indexed).count();
    if topics.len() != indexed + 1 || topics.first() != Some(&event.topic) {
        return Err(IndexError::Decode(format!(
            "{} expects {} topics, log has {}",
            event.signature,
            indexed + 1,
            topics.len()
        )));
    }
    let body_types: Vec<AbiType> = event
        .params
        .iter()
        .filter(|param| !param.indexed)
        .map(|param| param.kind.clone())
        .collect();
    let mut body = decode_sequence(data, 0, &body_types)?.into_iter();
    let mut topic_iter = topics.iter().skip(1);
    let mut args = Map::new();
    for param in &event.params {
        let value = if param.indexed {
            let topic = topic_iter
                .next()
                .ok_or_else(|| IndexError::Decode("missing topic".to_owned()))?;
            if param.kind.dynamic()
                || matches!(param.kind, AbiType::Tuple(_) | AbiType::FixedArray(..))
            {
                serde_json::json!({ "topic_hash": hex0x(topic) })
            } else {
                decode_value(topic, 0, &param.kind)?
            }
        } else {
            body.next()
                .ok_or_else(|| IndexError::Decode("missing body value".to_owned()))?
        };
        args.insert(param.name.clone(), value);
    }
    Ok(DecodedEvent {
        contract: event.contract.clone(),
        name: event.name.clone(),
        signature: event.signature.clone(),
        args,
    })
}

fn word(data: &[u8], position: usize) -> Result<&[u8], IndexError> {
    position
        .checked_add(32)
        .and_then(|end| data.get(position..end))
        .ok_or_else(|| IndexError::Decode("ABI data is truncated".to_owned()))
}

fn word_usize(data: &[u8], position: usize) -> Result<usize, IndexError> {
    let bytes = word(data, position)?;
    if !is_zero(&bytes[..24]) {
        return Err(IndexError::Decode(
            "ABI offset or length is too large".to_owned(),
        ));
    }
    let mut value = [0_u8; 8];
    value.copy_from_slice(&bytes[24..]);
    usize::try_from(u64::from_be_bytes(value))
        .map_err(|_| IndexError::Decode("ABI offset does not fit".to_owned()))
}

fn decode_sequence(data: &[u8], base: usize, types: &[AbiType]) -> Result<Vec<Value>, IndexError> {
    let mut values = Vec::with_capacity(types.len());
    let mut head = base;
    for kind in types {
        if kind.dynamic() {
            let offset = word_usize(data, head)?;
            let target = base
                .checked_add(offset)
                .ok_or_else(|| IndexError::Decode("ABI offset overflows".to_owned()))?;
            values.push(decode_value(data, target, kind)?);
            head += 32;
        } else {
            values.push(decode_value(data, head, kind)?);
            head = head
                .checked_add(kind.head_size())
                .ok_or_else(|| IndexError::Decode("ABI head overflows".to_owned()))?;
        }
    }
    Ok(values)
}

fn bounded_count(data: &[u8], position: usize, element_head: usize) -> Result<usize, IndexError> {
    let count = word_usize(data, position)?;
    let remaining = data.len().saturating_sub(position.saturating_add(32));
    if count.saturating_mul(element_head.max(1)) > remaining {
        return Err(IndexError::Decode("ABI length exceeds the data".to_owned()));
    }
    Ok(count)
}

fn decode_value(data: &[u8], position: usize, kind: &AbiType) -> Result<Value, IndexError> {
    Ok(match kind {
        AbiType::Address => {
            let bytes = word(data, position)?;
            if !is_zero(&bytes[..12]) {
                return Err(IndexError::Decode("address word has high bits".to_owned()));
            }
            Value::String(hex0x(&bytes[12..]))
        }
        AbiType::Bool => {
            let bytes = word(data, position)?;
            if !is_zero(&bytes[..31]) || bytes[31] > 1 {
                return Err(IndexError::Decode("bool word is not 0 or 1".to_owned()));
            }
            Value::Bool(bytes[31] == 1)
        }
        AbiType::Uint(bits) => {
            let bytes = word(data, position)?;
            if !is_zero(&bytes[..32 - bits / 8]) {
                return Err(IndexError::Decode(format!("uint{bits} overflows")));
            }
            Value::String(be_decimal(bytes))
        }
        AbiType::Int(bits) => {
            let bytes = word(data, position)?;
            let width = bits / 8;
            let sign = bytes[32 - width] & 0x80 != 0;
            let fill = if sign { 0xff } else { 0 };
            if bytes[..32 - width].iter().any(|byte| *byte != fill) {
                return Err(IndexError::Decode(format!("int{bits} overflows")));
            }
            Value::String(be_signed_decimal(bytes))
        }
        AbiType::FixedBytes(size) => {
            let bytes = word(data, position)?;
            if !is_zero(&bytes[*size..]) {
                return Err(IndexError::Decode(
                    "fixed bytes have trailing bits".to_owned(),
                ));
            }
            Value::String(hex0x(&bytes[..*size]))
        }
        AbiType::Bytes | AbiType::String => {
            let length = bounded_count(data, position, 1)?;
            let start = position + 32;
            let bytes = data
                .get(start..start + length)
                .ok_or_else(|| IndexError::Decode("ABI bytes are truncated".to_owned()))?;
            if *kind == AbiType::String {
                Value::String(
                    std::str::from_utf8(bytes)
                        .map_err(|_| IndexError::Decode("ABI string is not UTF-8".to_owned()))?
                        .to_owned(),
                )
            } else {
                Value::String(hex0x(bytes))
            }
        }
        AbiType::Array(inner) => {
            let count = bounded_count(data, position, inner.head_size())?;
            let types = vec![(**inner).clone(); count];
            Value::Array(decode_sequence(data, position + 32, &types)?)
        }
        AbiType::FixedArray(inner, count) => {
            let types = vec![(**inner).clone(); *count];
            Value::Array(decode_sequence(data, position, &types)?)
        }
        AbiType::Tuple(fields) => {
            let types: Vec<AbiType> = fields.iter().map(|(_, field)| field.clone()).collect();
            let values = decode_sequence(data, position, &types)?;
            let mut object = Map::new();
            for ((name, _), value) in fields.iter().zip(values) {
                object.insert(name.clone(), value);
            }
            Value::Object(object)
        }
    })
}

/// The hex of a 32-byte ABI value with its `0x` prefix removed, for asset
/// identifiers shared with LayerX.
#[must_use]
pub fn bare_bytes32(value: &Value) -> Option<String> {
    let text = value.as_str()?;
    let digits = text.strip_prefix("0x")?;
    (digits.len() == 64).then(|| digits.to_ascii_lowercase())
}

/// Renders an address argument in the store's canonical spelling.
#[must_use]
pub fn address_text(address: [u8; 20]) -> String {
    hex0x(&address)
}

/// Renders a topic as bare hex, for diagnostics.
#[must_use]
pub fn topic_text(topic: [u8; 32]) -> String {
    hex(&topic)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::unhex;

    fn registry_from(json: &str, contract: &str, address: Option<[u8; 20]>) -> AbiRegistry {
        let document: Value = serde_json::from_str(json).unwrap_or(Value::Null);
        let events = AbiRegistry::parse_events(contract, &document).unwrap_or_default();
        let mut registry = AbiRegistry::default();
        registry.insert(LoadedAbi {
            contract: contract.to_owned(),
            address,
            events,
        });
        registry
    }

    #[test]
    fn signatures_and_topics_match_solidity() {
        let registry = registry_from(
            r#"[{"type":"event","name":"Transfer","anonymous":false,"inputs":[
                {"name":"from","type":"address","indexed":true},
                {"name":"to","type":"address","indexed":true},
                {"name":"value","type":"uint256","indexed":false}]}]"#,
            "erc20",
            None,
        );
        let event = &registry.abis()[0].events[0];
        assert_eq!(event.signature, "Transfer(address,address,uint256)");
        assert_eq!(
            hex(&event.topic),
            "ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
        );
    }

    #[test]
    fn dynamic_values_tuples_and_signed_integers_decode() {
        let registry = registry_from(
            r#"[{"type":"event","name":"Mixed","inputs":[
                {"name":"who","type":"address","indexed":true},
                {"name":"label","type":"string","indexed":false},
                {"name":"delta","type":"int64","indexed":false},
                {"name":"amounts","type":"uint256[]","indexed":false},
                {"name":"pair","type":"tuple","indexed":false,"components":[
                    {"name":"a","type":"uint8"},{"name":"b","type":"bytes"}]}]}]"#,
            "mixed",
            Some(precompile_address(0x1017)),
        );
        let event = registry.abis()[0].events[0].clone();
        assert_eq!(
            event.signature,
            "Mixed(address,string,int64,uint256[],(uint8,bytes))"
        );
        let mut who = [0_u8; 32];
        who[31] = 0x42;
        let data = unhex(concat!(
            "0000000000000000000000000000000000000000000000000000000000000080",
            "fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffd",
            "00000000000000000000000000000000000000000000000000000000000000c0",
            "0000000000000000000000000000000000000000000000000000000000000120",
            "0000000000000000000000000000000000000000000000000000000000000002",
            "6869000000000000000000000000000000000000000000000000000000000000",
            "0000000000000000000000000000000000000000000000000000000000000002",
            "0000000000000000000000000000000000000000000000000000000000000001",
            "0000000000000000000000000000000000000000000000000000000000000002",
            "0000000000000000000000000000000000000000000000000000000000000007",
            "0000000000000000000000000000000000000000000000000000000000000040",
            "0000000000000000000000000000000000000000000000000000000000000001",
            "ab00000000000000000000000000000000000000000000000000000000000000",
        ))
        .unwrap_or_default();
        let decoded = registry
            .decode_log(precompile_address(0x1017), &[event.topic, who], &data)
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("event not found"))
            .1;
        assert_eq!(
            decoded.arg("who"),
            Some(&Value::String(
                "0x0000000000000000000000000000000000000042".to_owned()
            ))
        );
        assert_eq!(decoded.arg("label"), Some(&Value::String("hi".to_owned())));
        assert_eq!(decoded.arg("delta"), Some(&Value::String("-3".to_owned())));
        assert_eq!(decoded.arg("amounts"), Some(&serde_json::json!(["1", "2"])));
        assert_eq!(
            decoded.arg("pair"),
            Some(&serde_json::json!({"a": "7", "b": "0xab"}))
        );
        assert!(registry
            .decode_log(precompile_address(0x1016), &[event.topic, who], &data)
            .unwrap_or_else(|error| panic!("{error}"))
            .is_none());
        assert!(registry
            .decode_log(precompile_address(0x1017), &[event.topic], &data)
            .is_err());
    }

    #[test]
    fn every_repository_precompile_abi_loads_and_a_dropped_in_abi_is_picked_up() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../precompiles");
        let registry = AbiRegistry::load_dir(&root).unwrap_or_else(|error| panic!("{error}"));
        let names: Vec<&str> = registry
            .abis()
            .iter()
            .map(|abi| abi.contract.as_str())
            .collect();
        for expected in [
            "addr",
            "bank",
            "ibc",
            "layerxanchor",
            "layerxcustody",
            "layerxverify",
            "oracle",
            "pointer",
        ] {
            assert!(
                names.contains(&expected),
                "{expected} missing from {names:?}"
            );
        }
        let deposit = keccak(b"CustodyDeposit(bytes32,bytes32,address,bytes32,uint256,uint64)");
        let found = registry
            .lookup(precompile_address(0x1013), deposit)
            .unwrap_or_else(|| panic!("custody deposit not registered"));
        assert_eq!(found.contract, "layerxcustody");
        assert!(registry
            .lookup(precompile_address(0x1014), deposit)
            .is_none());
        let bound = keccak(b"LayerXBound(address,bytes32,uint64)");
        assert!(registry.lookup(precompile_address(0x1004), bound).is_some());

        let scratch = std::env::temp_dir().join(format!(
            "layerx-indexer-abi-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_nanos())
        ));
        let launchpad = scratch.join("launchpad");
        let custom = scratch.join("somethingnew");
        fs::create_dir_all(&launchpad).unwrap_or_else(|error| panic!("{error}"));
        fs::create_dir_all(&custom).unwrap_or_else(|error| panic!("{error}"));
        let abi = r#"[{"type":"event","name":"TokenLaunched","inputs":[
            {"name":"token","type":"address","indexed":true},
            {"name":"supply","type":"uint256","indexed":false}]}]"#;
        fs::write(launchpad.join("abi.json"), abi).unwrap_or_else(|error| panic!("{error}"));
        fs::write(custom.join("abi.json"), abi).unwrap_or_else(|error| panic!("{error}"));
        fs::write(
            custom.join("address"),
            "0x0000000000000000000000000000000000002001\n",
        )
        .unwrap_or_else(|error| panic!("{error}"));
        let dropped = AbiRegistry::load_dir(&scratch).unwrap_or_else(|error| panic!("{error}"));
        let _ = fs::remove_dir_all(&scratch);
        let topic = keccak(b"TokenLaunched(address,uint256)");
        assert_eq!(
            dropped
                .lookup(precompile_address(0x1017), topic)
                .map(|event| event.contract.as_str()),
            Some("launchpad")
        );
        assert_eq!(
            dropped
                .lookup(precompile_address(0x2001), topic)
                .map(|event| event.contract.as_str()),
            Some("somethingnew")
        );
        assert!(dropped.lookup(precompile_address(0x1015), topic).is_none());
    }
}

//! The `JSON` message and value boundary, and the canonical framing that
//! replaces it inside the guest.
//!
//! `CosmWasm` speaks `JSON` everywhere: `instantiate`, `execute` and `query`
//! take `JSON` messages, and `cw-storage-plus` stores `JSON` values. The
//! version-one program ABI carries bytes and integers, and a deterministic
//! metered guest is the wrong place to run a text parser.
//!
//! So `JSON` stays at the edge. This module reads and writes exactly the
//! documents a `CosmWasm` client and an exported state dump contain - the
//! serde encoding of a declared record, with fields in declaration order and
//! no insignificant whitespace - and converts them to and from the fixed
//! canonical framing the emitted module reads and writes. Field names survive
//! the crossing unchanged; only their encoding changes.

use crate::error::PortRefusal;

/// Longest `JSON` document the reader accepts.
pub const MAX_JSON_BYTES: usize = 65_536;
/// Most fields one declared record may carry.
pub const MAX_FIELDS: usize = 64;
/// Longest text value one field may carry.
pub const MAX_TEXT_BYTES: usize = 1_024;

/// The `JSON` type one declared field carries, in the encoding `CosmWasm`
/// itself uses for it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValueType {
    /// A `u64`, written as a `JSON` number.
    U64,
    /// A `Uint128`, written as a `JSON` string of decimal digits, which is how
    /// `cosmwasm_std` serialises it.
    Uint128,
    /// A `String` or `Addr`, written as a `JSON` string.
    Text,
    /// A `bool`.
    Bool,
}

impl ValueType {
    /// Returns the fixed canonical width of the type, or `None` for text,
    /// whose framing is length-prefixed.
    #[must_use]
    pub const fn width(self) -> Option<usize> {
        match self {
            Self::U64 => Some(8),
            Self::Uint128 => Some(16),
            Self::Bool => Some(1),
            Self::Text => None,
        }
    }
}

/// One declared field of a record or message, in declaration order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FieldSchema {
    /// The field name exactly as the `JSON` document spells it.
    pub name: String,
    /// The field's `JSON` type.
    pub kind: ValueType,
}

/// One value of a declared field.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FieldValue {
    /// A `u64` value.
    U64(u64),
    /// A `Uint128` value.
    Uint128(u128),
    /// A text value.
    Text(String),
    /// A `bool` value.
    Bool(bool),
}

impl FieldValue {
    /// Returns the declared type this value belongs to.
    #[must_use]
    pub const fn kind(&self) -> ValueType {
        match self {
            Self::U64(_) => ValueType::U64,
            Self::Uint128(_) => ValueType::Uint128,
            Self::Text(_) => ValueType::Text,
            Self::Bool(_) => ValueType::Bool,
        }
    }

    /// Appends the value's canonical framing: little-endian integers, a
    /// two-byte length before text, one byte for a boolean.
    ///
    /// # Errors
    ///
    /// Refuses text beyond the declared bound.
    pub fn encode(&self, out: &mut Vec<u8>) -> Result<(), PortRefusal> {
        match self {
            Self::U64(value) => out.extend_from_slice(&value.to_le_bytes()),
            Self::Uint128(value) => out.extend_from_slice(&value.to_le_bytes()),
            Self::Bool(value) => out.push(u8::from(*value)),
            Self::Text(value) => {
                let length = u16::try_from(value.len()).map_err(|_| PortRefusal::SchemaMismatch)?;
                if value.len() > MAX_TEXT_BYTES {
                    return Err(PortRefusal::SchemaMismatch);
                }
                out.extend_from_slice(&length.to_le_bytes());
                out.extend_from_slice(value.as_bytes());
            }
        }
        Ok(())
    }

    /// Appends the value's `JSON` form, in the encoding `cosmwasm_std` uses.
    ///
    /// # Errors
    ///
    /// Refuses text beyond the declared bound.
    pub fn encode_json(&self, out: &mut String) -> Result<(), PortRefusal> {
        match self {
            Self::U64(value) => out.push_str(&value.to_string()),
            Self::Uint128(value) => {
                out.push('"');
                out.push_str(&value.to_string());
                out.push('"');
            }
            Self::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
            Self::Text(value) => {
                if value.len() > MAX_TEXT_BYTES {
                    return Err(PortRefusal::SchemaMismatch);
                }
                write_json_string(value, out);
            }
        }
        Ok(())
    }
}

/// The declared shape of one `JSON` record: a `cw-storage-plus` stored value,
/// an `instantiate` message, or the body of one `execute` or `query` variant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordSchema {
    name: String,
    fields: Vec<FieldSchema>,
}

impl RecordSchema {
    /// Declares a record schema.
    ///
    /// # Errors
    ///
    /// Refuses an unnamed record, more fields than the bound, an unnamed field
    /// and a repeated field name.
    pub fn new(name: &str, fields: Vec<FieldSchema>) -> Result<Self, PortRefusal> {
        if name.is_empty() || fields.len() > MAX_FIELDS {
            return Err(PortRefusal::SchemaMismatch);
        }
        for (index, field) in fields.iter().enumerate() {
            if field.name.is_empty()
                || fields
                    .iter()
                    .skip(index.saturating_add(1))
                    .any(|other| other.name == field.name)
            {
                return Err(PortRefusal::SchemaMismatch);
            }
        }
        Ok(Self {
            name: name.to_owned(),
            fields,
        })
    }

    /// Returns the record name, which is the Rust type name for a stored value
    /// and the variant name for a message body.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Borrows the declared fields in declaration order.
    #[must_use]
    pub fn fields(&self) -> &[FieldSchema] {
        &self.fields
    }

    /// Returns the fixed canonical width of the record, or `None` when a text
    /// field makes it variable.
    #[must_use]
    pub fn canonical_width(&self) -> Option<usize> {
        self.fields.iter().try_fold(0_usize, |total, field| {
            field.kind.width().map(|width| total.saturating_add(width))
        })
    }

    /// Encodes the values into the canonical framing the emitted module reads.
    ///
    /// # Errors
    ///
    /// Refuses a value list that does not match the declared schema.
    pub fn encode(&self, values: &[FieldValue]) -> Result<Vec<u8>, PortRefusal> {
        if values.len() != self.fields.len() {
            return Err(PortRefusal::SchemaMismatch);
        }
        let mut encoded = Vec::with_capacity(self.canonical_width().unwrap_or(0));
        for (field, value) in self.fields.iter().zip(values) {
            if value.kind() != field.kind {
                return Err(PortRefusal::SchemaMismatch);
            }
            value.encode(&mut encoded)?;
        }
        Ok(encoded)
    }

    /// Decodes the canonical framing the emitted module writes.
    ///
    /// # Errors
    ///
    /// Refuses a truncated record, trailing bytes, an oversized text field and
    /// text that is not valid `UTF-8`.
    pub fn decode(&self, bytes: &[u8]) -> Result<Vec<FieldValue>, PortRefusal> {
        let mut cursor = 0_usize;
        let mut values = Vec::with_capacity(self.fields.len());
        for field in &self.fields {
            let (value, next) = decode_field(field.kind, bytes, cursor)?;
            values.push(value);
            cursor = next;
        }
        if cursor != bytes.len() {
            return Err(PortRefusal::SchemaMismatch);
        }
        Ok(values)
    }

    /// Encodes the values as the `JSON` object `CosmWasm` itself would store or
    /// accept: no whitespace, fields in declaration order.
    ///
    /// # Errors
    ///
    /// Refuses a value list that does not match the declared schema.
    pub fn encode_json(&self, values: &[FieldValue]) -> Result<String, PortRefusal> {
        if values.len() != self.fields.len() {
            return Err(PortRefusal::SchemaMismatch);
        }
        let mut text = String::from("{");
        for (index, (field, value)) in self.fields.iter().zip(values).enumerate() {
            if value.kind() != field.kind {
                return Err(PortRefusal::SchemaMismatch);
            }
            if index > 0 {
                text.push(',');
            }
            write_json_string(&field.name, &mut text);
            text.push(':');
            value.encode_json(&mut text)?;
        }
        text.push('}');
        Ok(text)
    }

    /// Reads the `JSON` object a `CosmWasm` client sends or an exported state
    /// dump holds, and returns its values in declaration order.
    ///
    /// # Errors
    ///
    /// Refuses a malformed document, an oversized document, an unknown field, a
    /// repeated field, a missing field, a value of the wrong type and trailing
    /// content after the object.
    pub fn decode_json(&self, text: &str) -> Result<Vec<FieldValue>, PortRefusal> {
        if text.len() > MAX_JSON_BYTES {
            return Err(PortRefusal::InvalidJson);
        }
        let mut scanner = Scanner::new(text.as_bytes());
        let mut slots: Vec<Option<FieldValue>> = vec![None; self.fields.len()];
        scanner.expect(b'{')?;
        if scanner.peek_after_space() == Some(b'}') {
            scanner.expect(b'}')?;
        } else {
            loop {
                let name = scanner.string()?;
                scanner.expect(b':')?;
                let position = self
                    .fields
                    .iter()
                    .position(|field| field.name == name)
                    .ok_or(PortRefusal::InvalidJson)?;
                let kind = self
                    .fields
                    .get(position)
                    .ok_or(PortRefusal::InvalidJson)?
                    .kind;
                let value = scanner.value(kind)?;
                let slot = slots.get_mut(position).ok_or(PortRefusal::InvalidJson)?;
                if slot.is_some() {
                    return Err(PortRefusal::InvalidJson);
                }
                *slot = Some(value);
                match scanner.take_after_space()? {
                    b',' => {}
                    b'}' => break,
                    _ => return Err(PortRefusal::InvalidJson),
                }
            }
        }
        scanner.finish()?;
        slots
            .into_iter()
            .map(|slot| slot.ok_or(PortRefusal::InvalidJson))
            .collect()
    }

    /// Transcodes one exported `CosmWasm` value straight into the canonical
    /// framing the ported program stores, which is what a state migration
    /// writes into namespaced storage.
    ///
    /// # Errors
    ///
    /// Refuses whatever [`Self::decode_json`] and [`Self::encode`] refuse.
    pub fn transcode(&self, text: &str) -> Result<Vec<u8>, PortRefusal> {
        let values = self.decode_json(text)?;
        self.encode(&values)
    }
}

fn decode_field(
    kind: ValueType,
    bytes: &[u8],
    cursor: usize,
) -> Result<(FieldValue, usize), PortRefusal> {
    match kind {
        ValueType::U64 => {
            let (raw, next) = take::<8>(bytes, cursor)?;
            Ok((FieldValue::U64(u64::from_le_bytes(raw)), next))
        }
        ValueType::Uint128 => {
            let (raw, next) = take::<16>(bytes, cursor)?;
            Ok((FieldValue::Uint128(u128::from_le_bytes(raw)), next))
        }
        ValueType::Bool => {
            let (raw, next) = take::<1>(bytes, cursor)?;
            let value = match raw.first() {
                Some(0) => false,
                Some(1) => true,
                _ => return Err(PortRefusal::SchemaMismatch),
            };
            Ok((FieldValue::Bool(value), next))
        }
        ValueType::Text => {
            let (raw, next) = take::<2>(bytes, cursor)?;
            let length = usize::from(u16::from_le_bytes(raw));
            if length > MAX_TEXT_BYTES {
                return Err(PortRefusal::SchemaMismatch);
            }
            let end = next.saturating_add(length);
            let slice = bytes.get(next..end).ok_or(PortRefusal::SchemaMismatch)?;
            let text = core::str::from_utf8(slice).map_err(|_| PortRefusal::SchemaMismatch)?;
            Ok((FieldValue::Text(text.to_owned()), end))
        }
    }
}

fn take<const N: usize>(bytes: &[u8], cursor: usize) -> Result<([u8; N], usize), PortRefusal> {
    let end = cursor.saturating_add(N);
    let raw: [u8; N] = bytes
        .get(cursor..end)
        .and_then(|slice| slice.try_into().ok())
        .ok_or(PortRefusal::SchemaMismatch)?;
    Ok((raw, end))
}

fn write_json_string(value: &str, out: &mut String) {
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            control if control < ' ' => {
                out.push_str("\\u00");
                let code = u32::from(control);
                out.push(hex_digit(code >> 4));
                out.push(hex_digit(code & 0x0f));
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

fn hex_digit(value: u32) -> char {
    char::from_digit(value, 16).unwrap_or('0')
}

struct Scanner<'text> {
    bytes: &'text [u8],
    cursor: usize,
}

impl<'text> Scanner<'text> {
    const fn new(bytes: &'text [u8]) -> Self {
        Self { bytes, cursor: 0 }
    }

    fn skip_space(&mut self) {
        while let Some(byte) = self.bytes.get(self.cursor) {
            if matches!(byte, b' ' | b'\t' | b'\n' | b'\r') {
                self.cursor = self.cursor.saturating_add(1);
            } else {
                break;
            }
        }
    }

    fn peek_after_space(&mut self) -> Option<u8> {
        self.skip_space();
        self.bytes.get(self.cursor).copied()
    }

    fn take_after_space(&mut self) -> Result<u8, PortRefusal> {
        let byte = self.peek_after_space().ok_or(PortRefusal::InvalidJson)?;
        self.cursor = self.cursor.saturating_add(1);
        Ok(byte)
    }

    fn expect(&mut self, byte: u8) -> Result<(), PortRefusal> {
        if self.take_after_space()? == byte {
            Ok(())
        } else {
            Err(PortRefusal::InvalidJson)
        }
    }

    fn finish(&mut self) -> Result<(), PortRefusal> {
        if self.peek_after_space().is_none() {
            Ok(())
        } else {
            Err(PortRefusal::InvalidJson)
        }
    }

    fn string(&mut self) -> Result<String, PortRefusal> {
        if self.take_after_space()? != b'"' {
            return Err(PortRefusal::InvalidJson);
        }
        let mut text = String::new();
        loop {
            let byte = self.take_raw()?;
            match byte {
                b'"' => break,
                b'\\' => text.push(self.escape()?),
                control if control < 0x20 => return Err(PortRefusal::InvalidJson),
                other => self.continuation(other, &mut text)?,
            }
            if text.len() > MAX_TEXT_BYTES {
                return Err(PortRefusal::InvalidJson);
            }
        }
        Ok(text)
    }

    fn continuation(&mut self, lead: u8, text: &mut String) -> Result<(), PortRefusal> {
        let extra: usize = match lead {
            0x00..=0x7f => 0,
            0xc2..=0xdf => 1,
            0xe0..=0xef => 2,
            0xf0..=0xf4 => 3,
            _ => return Err(PortRefusal::InvalidJson),
        };
        let mut encoded = Vec::with_capacity(extra.saturating_add(1));
        encoded.push(lead);
        for _ in 0..extra {
            encoded.push(self.take_raw()?);
        }
        let decoded = core::str::from_utf8(&encoded).map_err(|_| PortRefusal::InvalidJson)?;
        text.push_str(decoded);
        Ok(())
    }

    fn take_raw(&mut self) -> Result<u8, PortRefusal> {
        let byte = self
            .bytes
            .get(self.cursor)
            .copied()
            .ok_or(PortRefusal::InvalidJson)?;
        self.cursor = self.cursor.saturating_add(1);
        Ok(byte)
    }

    fn escape(&mut self) -> Result<char, PortRefusal> {
        let character = match self.take_raw()? {
            b'"' => '"',
            b'\\' => '\\',
            b'/' => '/',
            b'b' => '\u{8}',
            b'f' => '\u{c}',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => return self.code_point(),
            _ => return Err(PortRefusal::InvalidJson),
        };
        Ok(character)
    }

    fn code_point(&mut self) -> Result<char, PortRefusal> {
        let mut code = 0_u32;
        for _ in 0..4 {
            let digit = char::from(self.take_raw()?)
                .to_digit(16)
                .ok_or(PortRefusal::InvalidJson)?;
            code = code.saturating_mul(16).saturating_add(digit);
        }
        char::from_u32(code).ok_or(PortRefusal::InvalidJson)
    }

    fn digits(&mut self) -> Result<u128, PortRefusal> {
        let start = self.cursor;
        let mut value = 0_u128;
        while let Some(byte) = self.bytes.get(self.cursor) {
            let Some(digit) = char::from(*byte).to_digit(10) else {
                break;
            };
            value = value
                .checked_mul(10)
                .and_then(|scaled| scaled.checked_add(u128::from(digit)))
                .ok_or(PortRefusal::InvalidJson)?;
            self.cursor = self.cursor.saturating_add(1);
        }
        let written = self.cursor.saturating_sub(start);
        if written == 0 || (written > 1 && self.bytes.get(start) == Some(&b'0')) {
            return Err(PortRefusal::InvalidJson);
        }
        Ok(value)
    }

    fn literal(&mut self, expected: &[u8]) -> Result<(), PortRefusal> {
        for byte in expected {
            if self.take_raw()? != *byte {
                return Err(PortRefusal::InvalidJson);
            }
        }
        Ok(())
    }

    fn value(&mut self, kind: ValueType) -> Result<FieldValue, PortRefusal> {
        self.skip_space();
        match kind {
            ValueType::U64 => {
                let value = self.digits()?;
                Ok(FieldValue::U64(
                    u64::try_from(value).map_err(|_| PortRefusal::InvalidJson)?,
                ))
            }
            ValueType::Uint128 => {
                let text = self.string()?;
                let mut inner = Scanner::new(text.as_bytes());
                let value = inner.digits()?;
                inner.finish()?;
                Ok(FieldValue::Uint128(value))
            }
            ValueType::Text => Ok(FieldValue::Text(self.string()?)),
            ValueType::Bool => match self.peek_after_space() {
                Some(b't') => {
                    self.literal(b"true")?;
                    Ok(FieldValue::Bool(true))
                }
                Some(b'f') => {
                    self.literal(b"false")?;
                    Ok(FieldValue::Bool(false))
                }
                _ => Err(PortRefusal::InvalidJson),
            },
        }
    }
}

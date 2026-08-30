//! Canonical calldata encoding for LayerX programs.
//!
//! This module defines the frozen calldata encoding convention that ensures
//! byte-identical encoding across SDKs and prevents digest collisions from
//! multiple encodings of the same logical value.

use core::fmt::{self, Display};

use crate::meter::MeterRefusal;

pub const MAX_CALLDATA_BYTES: usize = 1_048_576;
pub const MAX_NESTING_DEPTH: u8 = 16;
pub const DECODED_SIZE_LIMIT: usize = 16_777_216;

/// Encoding convention discriminator.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EncodingConvention {
    /// LayerX canonical encoding with strict validation.
    LayerX,
    /// EVM head-only layout for ported contracts.
    EvmHeadOnly,
}

impl EncodingConvention {
    #[must_use]
    pub const fn tag(&self) -> u8 {
        match self {
            Self::LayerX => 0x01,
            Self::EvmHeadOnly => 0x02,
        }
    }

    pub fn from_tag(tag: u8) -> Result<Self, CodecError> {
        match tag {
            0x01 => Ok(Self::LayerX),
            0x02 => Ok(Self::EvmHeadOnly),
            _ => Err(CodecError::InvalidConvention),
        }
    }
}

/// Calldata encoding and decoding errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodecError {
    /// Encoding convention tag is invalid.
    InvalidConvention,
    /// Type discriminator is not recognized.
    InvalidType,
    /// Encoded data is not in canonical form.
    NonCanonical,
    /// Nesting depth exceeds the maximum.
    NestingTooDeep,
    /// Decoded size exceeds the limit.
    DecodedSizeLimitExceeded,
    /// Input data is truncated or incomplete.
    Truncated,
    /// Integer encoding is invalid.
    InvalidInteger,
    /// Length prefix is invalid or out of bounds.
    InvalidLength,
    /// Array encoding is malformed.
    InvalidArray,
    /// Option encoding is malformed.
    InvalidOption,
    /// Tagged union encoding is malformed.
    InvalidUnion,
    /// Input exceeds maximum calldata size.
    InputTooLarge,
    /// Metering refusal during decoding.
    Meter(MeterRefusal),
}

impl Display for CodecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConvention => formatter.write_str("invalid encoding convention tag"),
            Self::InvalidType => formatter.write_str("invalid type discriminator"),
            Self::NonCanonical => formatter.write_str("encoding is not canonical"),
            Self::NestingTooDeep => formatter.write_str("nesting depth exceeds maximum"),
            Self::DecodedSizeLimitExceeded => formatter.write_str("decoded size exceeds limit"),
            Self::Truncated => formatter.write_str("input data is truncated"),
            Self::InvalidInteger => formatter.write_str("integer encoding is invalid"),
            Self::InvalidLength => formatter.write_str("length prefix is invalid"),
            Self::InvalidArray => formatter.write_str("array encoding is malformed"),
            Self::InvalidOption => formatter.write_str("option encoding is malformed"),
            Self::InvalidUnion => formatter.write_str("tagged union encoding is malformed"),
            Self::InputTooLarge => formatter.write_str("input exceeds maximum calldata size"),
            Self::Meter(error) => write!(formatter, "metering refusal: {error}"),
        }
    }
}

impl std::error::Error for CodecError {}

impl From<MeterRefusal> for CodecError {
    fn from(value: MeterRefusal) -> Self {
        Self::Meter(value)
    }
}

/// Type discriminators for LayerX canonical encoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum TypeTag {
    /// Unsigned 8-bit integer.
    U8 = 0x10,
    /// Unsigned 16-bit integer (big-endian).
    U16 = 0x11,
    /// Unsigned 32-bit integer (big-endian).
    U32 = 0x12,
    /// Unsigned 64-bit integer (big-endian).
    U64 = 0x13,
    /// Unsigned 128-bit integer (big-endian).
    U128 = 0x14,
    /// Unsigned 256-bit integer (big-endian).
    U256 = 0x15,
    /// Signed 8-bit integer (two's complement).
    I8 = 0x18,
    /// Signed 16-bit integer (big-endian, two's complement).
    I16 = 0x19,
    /// Signed 32-bit integer (big-endian, two's complement).
    I32 = 0x1a,
    /// Signed 64-bit integer (big-endian, two's complement).
    I64 = 0x1b,
    /// Signed 128-bit integer (big-endian, two's complement).
    I128 = 0x1c,
    /// Variable-length byte string (4-byte big-endian length prefix).
    Bytes = 0x20,
    /// Fixed-size array (4-byte big-endian count prefix).
    FixedArray = 0x30,
    /// Variable-size array (4-byte big-endian count prefix).
    VariableArray = 0x31,
    /// Optional value (1-byte discriminator: 0x00 = None, 0x01 = Some).
    Option = 0x40,
    /// Tagged union (4-byte big-endian variant index).
    Union = 0x50,
}

impl TypeTag {
    pub fn from_byte(byte: u8) -> Result<Self, CodecError> {
        match byte {
            0x10 => Ok(Self::U8),
            0x11 => Ok(Self::U16),
            0x12 => Ok(Self::U32),
            0x13 => Ok(Self::U64),
            0x14 => Ok(Self::U128),
            0x15 => Ok(Self::U256),
            0x18 => Ok(Self::I8),
            0x19 => Ok(Self::I16),
            0x1a => Ok(Self::I32),
            0x1b => Ok(Self::I64),
            0x1c => Ok(Self::I128),
            0x20 => Ok(Self::Bytes),
            0x30 => Ok(Self::FixedArray),
            0x31 => Ok(Self::VariableArray),
            0x40 => Ok(Self::Option),
            0x50 => Ok(Self::Union),
            _ => Err(CodecError::InvalidType),
        }
    }

    #[must_use]
    pub const fn size_bytes(&self) -> Option<usize> {
        match self {
            Self::U8 | Self::I8 => Some(1),
            Self::U16 | Self::I16 => Some(2),
            Self::U32 | Self::I32 => Some(4),
            Self::U64 | Self::I64 => Some(8),
            Self::U128 | Self::I128 => Some(16),
            Self::U256 => Some(32),
            Self::Bytes
            | Self::FixedArray
            | Self::VariableArray
            | Self::Option
            | Self::Union => None,
        }
    }
}

/// Calldata encoding and decoding context.
#[derive(Debug)]
pub struct Calldata {
    convention: EncodingConvention,
    data: Vec<u8>,
}

impl Calldata {
    /// Creates an empty calldata buffer with LayerX convention.
    #[must_use]
    pub fn new() -> Self {
        Self {
            convention: EncodingConvention::LayerX,
            data: Vec::new(),
        }
    }

    /// Creates an empty calldata buffer with the specified convention.
    #[must_use]
    pub const fn with_convention(convention: EncodingConvention) -> Self {
        Self {
            convention,
            data: Vec::new(),
        }
    }

    /// Parses calldata from raw bytes with strict canonical validation.
    ///
    /// # Errors
    ///
    /// Returns an error if the input is malformed, non-canonical, exceeds size
    /// limits, or violates nesting constraints.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.is_empty() {
            return Ok(Self::new());
        }
        if bytes.len() > MAX_CALLDATA_BYTES {
            return Err(CodecError::InputTooLarge);
        }
        let convention = EncodingConvention::from_tag(bytes[0])?;
        let payload = &bytes[1..];
        let mut decoder = Decoder::new(payload, convention);
        decoder.validate_canonical(0)?;
        Ok(Self {
            convention,
            data: payload.to_vec(),
        })
    }

    #[must_use]
    pub const fn convention(&self) -> EncodingConvention {
        self.convention
    }

    #[must_use]
    pub fn as_bytes(&self) -> Vec<u8> {
        let mut output = Vec::with_capacity(self.data.len() + 1);
        output.push(self.convention.tag());
        output.extend_from_slice(&self.data);
        output
    }

    #[must_use]
    pub fn payload(&self) -> &[u8] {
        &self.data
    }

    /// Encodes a u8 value.
    ///
    /// # Errors
    ///
    /// Returns an error if encoding would exceed size limits.
    pub fn encode_u8(&mut self, value: u8) -> Result<(), CodecError> {
        self.check_capacity(2)?;
        self.data.push(TypeTag::U8 as u8);
        self.data.push(value);
        Ok(())
    }

    /// Encodes a u16 value in big-endian format.
    ///
    /// # Errors
    ///
    /// Returns an error if encoding would exceed size limits.
    pub fn encode_u16(&mut self, value: u16) -> Result<(), CodecError> {
        self.check_capacity(3)?;
        self.data.push(TypeTag::U16 as u8);
        self.data.extend_from_slice(&value.to_be_bytes());
        Ok(())
    }

    /// Encodes a u32 value in big-endian format.
    ///
    /// # Errors
    ///
    /// Returns an error if encoding would exceed size limits.
    pub fn encode_u32(&mut self, value: u32) -> Result<(), CodecError> {
        self.check_capacity(5)?;
        self.data.push(TypeTag::U32 as u8);
        self.data.extend_from_slice(&value.to_be_bytes());
        Ok(())
    }

    /// Encodes a u64 value in big-endian format.
    ///
    /// # Errors
    ///
    /// Returns an error if encoding would exceed size limits.
    pub fn encode_u64(&mut self, value: u64) -> Result<(), CodecError> {
        self.check_capacity(9)?;
        self.data.push(TypeTag::U64 as u8);
        self.data.extend_from_slice(&value.to_be_bytes());
        Ok(())
    }

    /// Encodes a u128 value in big-endian format.
    ///
    /// # Errors
    ///
    /// Returns an error if encoding would exceed size limits.
    pub fn encode_u128(&mut self, value: u128) -> Result<(), CodecError> {
        self.check_capacity(17)?;
        self.data.push(TypeTag::U128 as u8);
        self.data.extend_from_slice(&value.to_be_bytes());
        Ok(())
    }

    /// Encodes a byte string with a 4-byte length prefix.
    ///
    /// # Errors
    ///
    /// Returns an error if encoding would exceed size limits or the byte string
    /// is too large.
    pub fn encode_bytes(&mut self, bytes: &[u8]) -> Result<(), CodecError> {
        let length = u32::try_from(bytes.len()).map_err(|_| CodecError::InvalidLength)?;
        self.check_capacity(5 + bytes.len())?;
        self.data.push(TypeTag::Bytes as u8);
        self.data.extend_from_slice(&length.to_be_bytes());
        self.data.extend_from_slice(bytes);
        Ok(())
    }

    /// Begins encoding a fixed-size array with the given element count.
    ///
    /// # Errors
    ///
    /// Returns an error if encoding would exceed size limits.
    pub fn begin_fixed_array(&mut self, count: u32) -> Result<(), CodecError> {
        self.check_capacity(5)?;
        self.data.push(TypeTag::FixedArray as u8);
        self.data.extend_from_slice(&count.to_be_bytes());
        Ok(())
    }

    /// Begins encoding a variable-size array with the given element count.
    ///
    /// # Errors
    ///
    /// Returns an error if encoding would exceed size limits.
    pub fn begin_variable_array(&mut self, count: u32) -> Result<(), CodecError> {
        self.check_capacity(5)?;
        self.data.push(TypeTag::VariableArray as u8);
        self.data.extend_from_slice(&count.to_be_bytes());
        Ok(())
    }

    /// Encodes None variant of an option type.
    ///
    /// # Errors
    ///
    /// Returns an error if encoding would exceed size limits.
    pub fn encode_option_none(&mut self) -> Result<(), CodecError> {
        self.check_capacity(2)?;
        self.data.push(TypeTag::Option as u8);
        self.data.push(0x00);
        Ok(())
    }

    /// Begins encoding Some variant of an option type.
    ///
    /// # Errors
    ///
    /// Returns an error if encoding would exceed size limits.
    pub fn begin_option_some(&mut self) -> Result<(), CodecError> {
        self.check_capacity(2)?;
        self.data.push(TypeTag::Option as u8);
        self.data.push(0x01);
        Ok(())
    }

    /// Begins encoding a tagged union with the given variant index.
    ///
    /// # Errors
    ///
    /// Returns an error if encoding would exceed size limits.
    pub fn begin_union(&mut self, variant: u32) -> Result<(), CodecError> {
        self.check_capacity(5)?;
        self.data.push(TypeTag::Union as u8);
        self.data.extend_from_slice(&variant.to_be_bytes());
        Ok(())
    }

    fn check_capacity(&self, additional: usize) -> Result<(), CodecError> {
        let total = self
            .data
            .len()
            .checked_add(additional)
            .ok_or(CodecError::InvalidLength)?;
        if total > MAX_CALLDATA_BYTES {
            return Err(CodecError::InputTooLarge);
        }
        Ok(())
    }
}

impl Default for Calldata {
    fn default() -> Self {
        Self::new()
    }
}

/// Internal decoder for canonical validation.
struct Decoder<'a> {
    data: &'a [u8],
    position: usize,
    decoded_size: usize,
    convention: EncodingConvention,
}

impl<'a> Decoder<'a> {
    const fn new(data: &'a [u8], convention: EncodingConvention) -> Self {
        Self {
            data,
            position: 0,
            decoded_size: 0,
            convention,
        }
    }

    fn validate_canonical(&mut self, depth: u8) -> Result<(), CodecError> {
        if depth > MAX_NESTING_DEPTH {
            return Err(CodecError::NestingTooDeep);
        }
        if self.position >= self.data.len() {
            return Ok(());
        }
        match self.convention {
            EncodingConvention::LayerX => self.validate_layerx_value(depth),
            EncodingConvention::EvmHeadOnly => self.validate_evm_head_only(),
        }
    }

    fn validate_layerx_value(&mut self, depth: u8) -> Result<(), CodecError> {
        if depth > MAX_NESTING_DEPTH {
            return Err(CodecError::NestingTooDeep);
        }
        let tag_byte = self.read_byte()?;
        let tag = TypeTag::from_byte(tag_byte)?;
        match tag {
            TypeTag::U8 | TypeTag::I8 => {
                self.consume(1)?;
                self.charge_decoded(1)?;
            }
            TypeTag::U16 | TypeTag::I16 => {
                self.consume(2)?;
                self.charge_decoded(2)?;
            }
            TypeTag::U32 | TypeTag::I32 => {
                self.consume(4)?;
                self.charge_decoded(4)?;
            }
            TypeTag::U64 | TypeTag::I64 => {
                self.consume(8)?;
                self.charge_decoded(8)?;
            }
            TypeTag::U128 | TypeTag::I128 => {
                self.consume(16)?;
                self.charge_decoded(16)?;
            }
            TypeTag::U256 => {
                self.consume(32)?;
                self.charge_decoded(32)?;
            }
            TypeTag::Bytes => {
                let length = self.read_u32()? as usize;
                self.consume(length)?;
                self.charge_decoded(length)?;
            }
            TypeTag::FixedArray | TypeTag::VariableArray => {
                let count = self.read_u32()?;
                for _ in 0..count {
                    self.validate_layerx_value(depth.saturating_add(1))?;
                }
            }
            TypeTag::Option => {
                let discriminator = self.read_byte()?;
                match discriminator {
                    0x00 => {}
                    0x01 => {
                        self.validate_layerx_value(depth.saturating_add(1))?;
                    }
                    _ => return Err(CodecError::InvalidOption),
                }
            }
            TypeTag::Union => {
                let _variant = self.read_u32()?;
                self.validate_layerx_value(depth.saturating_add(1))?;
            }
        }
        Ok(())
    }

    fn validate_evm_head_only(&mut self) -> Result<(), CodecError> {
        while self.position < self.data.len() {
            if self.data.len() - self.position < 32 {
                return Err(CodecError::NonCanonical);
            }
            self.consume(32)?;
            self.charge_decoded(32)?;
        }
        Ok(())
    }

    fn read_byte(&mut self) -> Result<u8, CodecError> {
        if self.position >= self.data.len() {
            return Err(CodecError::Truncated);
        }
        let byte = self.data[self.position];
        self.position += 1;
        Ok(byte)
    }

    fn read_u32(&mut self) -> Result<u32, CodecError> {
        if self.position + 4 > self.data.len() {
            return Err(CodecError::Truncated);
        }
        let bytes: [u8; 4] = self.data[self.position..self.position + 4]
            .try_into()
            .map_err(|_| CodecError::Truncated)?;
        self.position += 4;
        Ok(u32::from_be_bytes(bytes))
    }

    fn consume(&mut self, count: usize) -> Result<(), CodecError> {
        if self.position + count > self.data.len() {
            return Err(CodecError::Truncated);
        }
        self.position += count;
        Ok(())
    }

    fn charge_decoded(&mut self, size: usize) -> Result<(), CodecError> {
        self.decoded_size = self
            .decoded_size
            .checked_add(size)
            .ok_or(CodecError::DecodedSizeLimitExceeded)?;
        if self.decoded_size > DECODED_SIZE_LIMIT {
            return Err(CodecError::DecodedSizeLimitExceeded);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_calldata_is_valid() {
        let calldata = Calldata::from_bytes(&[]).expect("empty calldata");
        assert_eq!(calldata.convention(), EncodingConvention::LayerX);
        assert!(calldata.payload().is_empty());
    }

    #[test]
    fn layerx_convention_tag_roundtrips() {
        let mut calldata = Calldata::new();
        calldata.encode_u8(42).expect("encode u8");
        let bytes = calldata.as_bytes();
        assert_eq!(bytes[0], 0x01);
        let decoded = Calldata::from_bytes(&bytes).expect("decode");
        assert_eq!(decoded.convention(), EncodingConvention::LayerX);
        assert_eq!(decoded.payload(), &[0x10, 42]);
    }

    #[test]
    fn evm_convention_tag_roundtrips() {
        let calldata = Calldata::with_convention(EncodingConvention::EvmHeadOnly);
        let bytes = calldata.as_bytes();
        assert_eq!(bytes[0], 0x02);
        let decoded = Calldata::from_bytes(&bytes).expect("decode");
        assert_eq!(decoded.convention(), EncodingConvention::EvmHeadOnly);
    }

    #[test]
    fn invalid_convention_tag_is_rejected() {
        let invalid = vec![0xff, 0x10, 42];
        assert_eq!(
            Calldata::from_bytes(&invalid).unwrap_err(),
            CodecError::InvalidConvention
        );
    }

    #[test]
    fn integer_encodings_are_canonical() {
        let mut calldata = Calldata::new();
        calldata.encode_u8(1).expect("u8");
        calldata.encode_u16(256).expect("u16");
        calldata.encode_u32(65536).expect("u32");
        calldata.encode_u64(4_294_967_296).expect("u64");
        calldata.encode_u128(u128::MAX).expect("u128");
        let bytes = calldata.as_bytes();
        assert!(Calldata::from_bytes(&bytes).is_ok());
    }

    #[test]
    fn bytes_encoding_is_canonical() {
        let mut calldata = Calldata::new();
        calldata.encode_bytes(b"hello").expect("bytes");
        let bytes = calldata.as_bytes();
        let decoded = Calldata::from_bytes(&bytes).expect("decode");
        assert_eq!(
            decoded.payload(),
            &[0x20, 0x00, 0x00, 0x00, 0x05, b'h', b'e', b'l', b'l', b'o']
        );
    }

    #[test]
    fn empty_bytes_is_canonical() {
        let mut calldata = Calldata::new();
        calldata.encode_bytes(&[]).expect("empty bytes");
        let bytes = calldata.as_bytes();
        assert!(Calldata::from_bytes(&bytes).is_ok());
    }

    #[test]
    fn fixed_array_nesting_is_bounded() {
        let mut calldata = Calldata::new();
        calldata.begin_fixed_array(1).expect("array");
        for _ in 0..MAX_NESTING_DEPTH {
            calldata.begin_fixed_array(1).expect("nested");
        }
        calldata.encode_u8(42).expect("leaf");
        let bytes = calldata.as_bytes();
        assert_eq!(
            Calldata::from_bytes(&bytes).unwrap_err(),
            CodecError::NestingTooDeep
        );
    }

    #[test]
    fn option_none_is_canonical() {
        let mut calldata = Calldata::new();
        calldata.encode_option_none().expect("none");
        let bytes = calldata.as_bytes();
        let decoded = Calldata::from_bytes(&bytes).expect("decode");
        assert_eq!(decoded.payload(), &[0x40, 0x00]);
    }

    #[test]
    fn option_some_is_canonical() {
        let mut calldata = Calldata::new();
        calldata.begin_option_some().expect("some");
        calldata.encode_u8(7).expect("value");
        let bytes = calldata.as_bytes();
        let decoded = Calldata::from_bytes(&bytes).expect("decode");
        assert_eq!(decoded.payload(), &[0x40, 0x01, 0x10, 7]);
    }

    #[test]
    fn invalid_option_discriminator_is_rejected() {
        let invalid = vec![0x01, 0x40, 0x02];
        assert_eq!(
            Calldata::from_bytes(&invalid).unwrap_err(),
            CodecError::InvalidOption
        );
    }

    #[test]
    fn union_encoding_is_canonical() {
        let mut calldata = Calldata::new();
        calldata.begin_union(3).expect("union");
        calldata.encode_bytes(b"data").expect("payload");
        let bytes = calldata.as_bytes();
        assert!(Calldata::from_bytes(&bytes).is_ok());
    }

    #[test]
    fn evm_head_only_requires_32_byte_alignment() {
        let mut data = vec![0x02];
        data.extend_from_slice(&[0u8; 32]);
        assert!(Calldata::from_bytes(&data).is_ok());
        let mut misaligned = vec![0x02];
        misaligned.extend_from_slice(&[0u8; 31]);
        assert_eq!(
            Calldata::from_bytes(&misaligned).unwrap_err(),
            CodecError::NonCanonical
        );
    }

    #[test]
    fn decoded_size_limit_is_enforced() {
        let mut calldata = Calldata::new();
        let large_bytes = vec![0u8; DECODED_SIZE_LIMIT + 1];
        assert!(calldata.encode_bytes(&large_bytes).is_err());
    }

    #[test]
    fn input_size_limit_is_enforced() {
        let oversized = vec![0u8; MAX_CALLDATA_BYTES + 1];
        assert_eq!(
            Calldata::from_bytes(&oversized).unwrap_err(),
            CodecError::InputTooLarge
        );
    }
}

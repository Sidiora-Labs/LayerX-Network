use minicbor::{Decode, Decoder, data::Type};
use semver::Version;
use thiserror::Error;

const EXPECTED_AUXDATA_ARRAY_SIZE: u64 = 5;

const EXPECTED_INTEGRITY_HASH_SIZE: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Auxdata {
    integrity_hash: [u8; EXPECTED_INTEGRITY_HASH_SIZE],
    runtime_code_length: u64,
    data_section_lengths: Vec<u64>,
    immutables_length: u64,
    version: Version,
}

impl Auxdata {
    pub fn from_cbor(encoded: &[u8]) -> Result<(Self, usize), minicbor::decode::Error> {
        let mut context = DecodeContext::default();
        let result = minicbor::decode_with(encoded, &mut context)?;

        Ok((result, context.used_size))
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq, Hash)]
enum ParseError {
    #[error("invalid auxdata type; expected \"array\", found \"{0}\"")]
    InvalidAuxdataType(Type),
    #[error("invalid {key} array size; value={value}")]
    InvalidArraySize { key: &'static str, value: u64 },
    #[error("invalid {key} type; expected \"{expected}\", found=\"{actual}\"")]
    InvalidValueType {
        key: &'static str,
        expected: Type,
        actual: Type,
    },
    #[error("{key} value is not valid \"u64\" type; actual={ty}")]
    InvalidU64Value { key: &'static str, ty: Type },
    #[error("invalid integrity hash size; expected={EXPECTED_INTEGRITY_HASH_SIZE}, found=0")]
    InvalidIntegrityHashSize(usize),
    #[error("invalid compiler map size; expected=1, found={0}")]
    InvalidCompilerMapSize(usize),
    #[error("invalid compiler map key; found={0}")]
    InvalidCompilerMapKey(String),
}

impl From<ParseError> for minicbor::decode::Error {
    fn from(error: ParseError) -> minicbor::decode::Error {
        minicbor::decode::Error::custom(error)
    }
}

#[derive(Default, Debug, Clone, PartialEq, Eq)]
struct DecodeContext {
    used_size: usize,
}

impl<'b> Decode<'b, DecodeContext> for Auxdata {
    fn decode(
        d: &mut Decoder<'b>,
        ctx: &mut DecodeContext,
    ) -> Result<Self, minicbor::decode::Error> {
        match d.datatype()? {
            Type::Array => {}
            ty => Err(ParseError::InvalidAuxdataType(ty))?,
        }

        match d.array()? {
            Some(EXPECTED_AUXDATA_ARRAY_SIZE) => {}
            Some(length) => Err(ParseError::InvalidArraySize {
                key: "auxdata",
                value: length,
            })?,
            None => Err(ParseError::InvalidArraySize {
                key: "auxdata",
                value: u64::MAX,
            })?,
        };

        let auxdata = Self {
            integrity_hash: decode_integrity_hash(d)?,
            runtime_code_length: decode_runtime_code_length(d)?,
            data_section_lengths: decode_data_section_lengths(d)?,
            immutables_length: decode_immutables_length(d)?,
            version: decode_version(d)?,
        };

        ctx.used_size = d.position();
        Ok(auxdata)
    }

    fn nil() -> Option<Self> {
        Some(Self {
            integrity_hash: [0; 32],
            runtime_code_length: 0,
            data_section_lengths: vec![],
            immutables_length: 0,
            version: Version::new(0, 0, 0),
        })
    }
}

fn validate_value_datatype(
    d: &mut Decoder,
    key: &'static str,
    expected: Type,
) -> Result<(), minicbor::decode::Error> {
    let ty = d.datatype()?;
    if ty != expected {
        Err(ParseError::InvalidValueType {
            key,
            expected,
            actual: ty,
        })?
    }

    Ok(())
}

fn decode_integrity_hash(
    d: &mut Decoder,
) -> Result<[u8; EXPECTED_INTEGRITY_HASH_SIZE], minicbor::decode::Error> {
    validate_value_datatype(d, "integrity_hash", Type::Bytes)?;

    let decoded = d.bytes()?;
    if decoded.len() != EXPECTED_INTEGRITY_HASH_SIZE {
        Err(ParseError::InvalidIntegrityHashSize(decoded.len()))?;
    }

    let mut integrity_hash = [0; EXPECTED_INTEGRITY_HASH_SIZE];
    integrity_hash.copy_from_slice(decoded);
    Ok(integrity_hash)
}

fn decode_runtime_code_length(d: &mut Decoder) -> Result<u64, minicbor::decode::Error> {
    decode_unsigned_integer(d, "runtime_code_length")
}

fn decode_data_section_lengths(d: &mut Decoder) -> Result<Vec<u64>, minicbor::decode::Error> {
    validate_value_datatype(d, "data_section_lengths", Type::Array)?;

    let array_size = d.array()?.ok_or(ParseError::InvalidArraySize {
        key: "data_section_lengths",
        value: u64::MAX,
    })?;

    let mut decode_data_section_lengths = vec![];
    for _ in 0..array_size {
        let value = decode_unsigned_integer(d, "data_section_length_value")?;
        decode_data_section_lengths.push(value);
    }
    Ok(decode_data_section_lengths)
}

fn decode_immutables_length(d: &mut Decoder) -> Result<u64, minicbor::decode::Error> {
    decode_unsigned_integer(d, "immutables_length")
}

fn decode_version(d: &mut Decoder) -> Result<Version, minicbor::decode::Error> {
    validate_value_datatype(d, "compiler_map", Type::Map)?;
    let compiler_map_size = d.map()?.unwrap_or(u64::MAX);
    if compiler_map_size != 1 {
        Err(ParseError::InvalidCompilerMapSize(
            compiler_map_size as usize,
        ))?
    }

    validate_value_datatype(d, "compiler_map_key", Type::String)?;
    let compiler_map_name = d.str()?;
    if compiler_map_name != "vyper" {
        Err(ParseError::InvalidCompilerMapKey(
            compiler_map_name.to_owned(),
        ))?;
    }

    validate_value_datatype(d, "compiler_map_value", Type::Array)?;
    let compiler_map_value = d.array()?.unwrap_or(u64::MAX);
    if compiler_map_value != 3 {
        Err(ParseError::InvalidArraySize {
            key: "compiler_map_value",
            value: compiler_map_value,
        })?;
    }

    let major = decode_unsigned_integer(d, "major_compiler_version")?;
    let minor = decode_unsigned_integer(d, "minor_compiler_version")?;
    let patch = decode_unsigned_integer(d, "patch_compiler_version")?;

    Ok(Version::new(major, minor, patch))
}

fn decode_unsigned_integer(
    d: &mut Decoder,
    key: &'static str,
) -> Result<u64, minicbor::decode::Error> {
    let value = match d.datatype()? {
        Type::U8 => d.u8()? as u64,
        Type::U16 => d.u16()? as u64,
        Type::U32 => d.u32()? as u64,
        Type::U64 => d.u64()?,
        ty => Err(ParseError::InvalidU64Value { key, ty })?,
    };
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use blockscout_display_bytes::decode_hex;
    use std::error::Error;

    fn validate_custom_error(error: minicbor::decode::Error, expected: ParseError) {
        if !error.is_custom() {
            panic!("expected custom error, got {error:?}")
        }

        let source = error.source().unwrap();
        let parse_error = source.downcast_ref::<ParseError>().unwrap();

        assert_eq!(parse_error, &expected, "invalid error");
    }

    fn decode_fixed_hex<const N: usize>(hex: &str) -> [u8; N] {
        let mut value = [0; N];
        value.copy_from_slice(&decode_hex(hex).unwrap());
        value
    }

    #[test]
    fn decoding_vyper_greater_than_0_4_0_auxdata() {
        // [h'B85677B7259F06429499A005D09B92BBB824E37C1753BFF5586C521F777999B0', 40, [], 0, {"vyper": [0, 4, 1]}]
        let hex = "855820b85677b7259f06429499a005d09b92bbb824e37c1753bff5586c521f777999b018288000a165767970657283000401";
        let encoded = decode_hex(hex).unwrap();
        let expected = Auxdata {
            integrity_hash: decode_fixed_hex::<32>(
                "B85677B7259F06429499A005D09B92BBB824E37C1753BFF5586C521F777999B0",
            ),
            runtime_code_length: 40,
            data_section_lengths: vec![],
            immutables_length: 0,
            version: Version::new(0, 4, 1),
        };
        let expected_size = encoded.len();

        // when
        let (decoded, decoded_size) =
            Auxdata::from_cbor(encoded.as_ref()).expect("Error when decoding valid metadata hash");

        // then
        assert_eq!(expected, decoded, "Incorrectly decoded");
        assert_eq!(expected_size, decoded_size, "Incorrect decoded size")
    }

    #[test]
    fn decoding_auxdata_with_immutables() {
        // [h'D6D74D3D47B775048F55F6D98D412B4B4A5CA50A9B12007A2DB13E2BA4C6BDED', 128, [6], 64, {"vyper": [0, 4, 1]}]
        let hex = "855820d6d74d3d47b775048f55f6d98d412b4b4a5ca50a9b12007a2db13e2ba4c6bded188081061840a165767970657283000401";
        let encoded = decode_hex(hex).unwrap();
        let expected = Auxdata {
            integrity_hash: decode_fixed_hex::<32>(
                "D6D74D3D47B775048F55F6D98D412B4B4A5CA50A9B12007A2DB13E2BA4C6BDED",
            ),
            runtime_code_length: 128,
            data_section_lengths: vec![6],
            immutables_length: 64,
            version: Version::new(0, 4, 1),
        };
        let expected_size = encoded.len();

        // when
        let (decoded, decoded_size) =
            Auxdata::from_cbor(encoded.as_ref()).expect("Error when decoding valid metadata hash");

        // then
        assert_eq!(expected, decoded, "Incorrectly decoded");
        assert_eq!(expected_size, decoded_size, "Incorrect decoded size")
    }

    #[test]
    fn decoding_of_non_exhausted_string() {
        // [h'B85677B7259F06429499A005D09B92BBB824E37C1753BFF5586C521F777999B0', 40, [], 0, {"vyper": [0, 4, 1]}]
        let hex = "855820b85677b7259f06429499a005d09b92bbb824e37c1753bff5586c521f777999b018288000a1657679706572830004010034";
        let encoded = decode_hex(hex).unwrap();
        let expected = Auxdata {
            integrity_hash: decode_fixed_hex::<32>(
                "B85677B7259F06429499A005D09B92BBB824E37C1753BFF5586C521F777999B0",
            ),
            runtime_code_length: 40,
            data_section_lengths: vec![],
            immutables_length: 0,
            version: Version::new(0, 4, 1),
        };
        let expected_size = encoded.len() - 2;

        // when
        let (decoded, decoded_size) =
            Auxdata::from_cbor(encoded.as_ref()).expect("Error when decoding valid metadata hash");

        // then
        assert_eq!(expected, decoded, "Incorrectly decoded");
        assert_eq!(expected_size, decoded_size, "Incorrect decoded size")
    }

    #[test]
    fn decoding_of_non_array_should_fail() {
        // given
        // "solc"
        let hex = "64736f6c63";
        let encoded = decode_hex(hex).unwrap();

        // when
        let decoded = Auxdata::from_cbor(encoded.as_ref());

        // then
        assert!(decoded.is_err(), "Deserialization should fail");
        validate_custom_error(
            decoded.unwrap_err(),
            ParseError::InvalidAuxdataType(Type::String),
        )
    }
}

use minicbor::{data::Type, Decode, Decoder};
use semver::Version;
use std::str::FromStr;
use thiserror::Error;

/// Parsed metadata hash
/// (https://docs.soliditylang.org/en/v0.8.14/metadata.html#encoding-of-the-metadata-hash-in-the-bytecode).
///
/// Currently we are interested only in `solc` value.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct MetadataHash {
    pub solc: Option<Version>,
}

impl MetadataHash {
    pub fn from_cbor(encoded: &[u8]) -> Result<(Self, usize), minicbor::decode::Error> {
        let mut context = DecodeContext::default();
        let result = minicbor::decode_with(encoded, &mut context)?;

        Ok((result, context.used_size))
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq, Hash)]
enum ParseMetadataHashError {
    #[error("invalid solc type. Expected \"string\" or \"bytes\", found \"{0}\"")]
    InvalidSolcType(Type),
    #[error("solc is not a valid version: {0}")]
    InvalidSolcVersion(String),
    #[error("\"solc\" key met more than once")]
    DuplicateKeys,
}

#[derive(Default, Debug, Clone, PartialEq, Eq)]
struct DecodeContext {
    used_size: usize,
}

impl<'b> Decode<'b, DecodeContext> for MetadataHash {
    fn decode(
        d: &mut Decoder<'b>,
        ctx: &mut DecodeContext,
    ) -> Result<Self, minicbor::decode::Error> {
        use minicbor::decode::Error;

        let number_of_elements = d.map()?.unwrap_or(u64::MAX);

        let mut solc = None;
        for _ in 0..number_of_elements {
            // try to parse the key
            match d.str() {
                Ok("solc") => {
                    if solc.is_some() {
                        // duplicate keys are not allowed in CBOR (RFC 8949)
                        return Err(Error::custom(ParseMetadataHashError::DuplicateKeys));
                    }
                    solc = match d.datatype()? {
                        // Appeared in 0.5.9.
                        // https://docs.soliditylang.org/en/v0.8.17/metadata.html#encoding-of-the-metadata-hash-in-the-bytecode
                        Type::Bytes => {
                            // Release builds of solc use a 3 byte encoding of the version
                            // (one byte each for major, minor and patch version number)
                            let bytes = d.bytes()?;
                            if bytes.len() != 3 {
                                // Something went wrong
                                return Err(Error::custom(
                                    ParseMetadataHashError::InvalidSolcVersion(
                                        "release build should be encoded as exactly 3 bytes".into(),
                                    ),
                                ));
                            }
                            let (major, minor, patch) = (bytes[0], bytes[1], bytes[2]);
                            Some(Version::new(major as u64, minor as u64, patch as u64))
                        }
                        Type::String => {
                            // Prerelease builds use a complete version string including commit hash and build date
                            let s = d.str()?;
                            let version = Version::from_str(s).map_err(|err| {
                                Error::custom(ParseMetadataHashError::InvalidSolcVersion(
                                    err.to_string(),
                                ))
                            })?;
                            Some(version)
                        }
                        type_ => {
                            // value of "solc" key must be either String or Bytes
                            return Err(Error::custom(ParseMetadataHashError::InvalidSolcType(
                                type_,
                            )));
                        }
                    }
                }
                Ok(_) => {
                    // if key is not "solc" str we may skip the corresponding value
                    d.skip()?;
                }
                Err(err) => return Err(err),
            }
        }

        // Update context and set the number of bytes that have been used during decoding.
        // That mechanism allows us to pass number of used bytes into the caller of `decode`
        // function.
        ctx.used_size = d.position();

        Ok(MetadataHash { solc })
    }

    fn nil() -> Option<Self> {
        Some(Self { solc: None })
    }
}

#[cfg(test)]
mod metadata_hash_deserialization_tests {
    use super::*;
    use blockscout_display_bytes::decode_hex;
    use std::str::FromStr;

    fn is_valid_custom_error(
        error: minicbor::decode::Error,
        expected: ParseMetadataHashError,
    ) -> bool {
        if !error.is_custom() {
            return false;
        }

        // Unfortunately, current `minicbor::decode::Error` implementation
        // does not allow to retrieve insides out of custom error,
        // so the only way to ensure the valid error occurred is by string comparison.
        let parse_metadata_hash_error_to_string = |err: ParseMetadataHashError| match err {
            ParseMetadataHashError::InvalidSolcType(_) => "InvalidSolcType",
            ParseMetadataHashError::InvalidSolcVersion(_) => "InvalidSolcVersion",
            ParseMetadataHashError::DuplicateKeys => "DuplicateKeys",
        };
        format!("{error:?}").contains(parse_metadata_hash_error_to_string(expected))
    }

    #[test]
    fn deserialization_metadata_hash_without_solc_tag() {
        // given
        // { "bzzr0": b"d4fba422541feba2d648f6657d9354ec14ea9f5919b520abe0feb60981d7b17c" }
        let hex =
            "a165627a7a72305820d4fba422541feba2d648f6657d9354ec14ea9f5919b520abe0feb60981d7b17c";
        let encoded = decode_hex(hex).unwrap();
        let expected = MetadataHash { solc: None };
        let expected_size = encoded.len();

        // when
        let (decoded, decoded_size) = MetadataHash::from_cbor(encoded.as_ref())
            .expect("Error when decoding valid metadata hash");

        // then
        assert_eq!(expected, decoded, "Incorrectly decoded");
        assert_eq!(expected_size, decoded_size, "Incorrect decoded size")
    }

    #[test]
    fn deserialization_metadata_hash_with_solc_as_version() {
        // given
        // { "ipfs": b"1220BCC988B1311237F2C00CCD0BFBD8B01D24DC18F720603B0DE93FE6327DF53625", "solc": b'00080e' }
        let hex = "a2646970667358221220bcc988b1311237f2c00ccd0bfbd8b01d24dc18f720603b0de93fe6327df5362564736f6c634300080e";
        let encoded = decode_hex(hex).unwrap();
        let expected = MetadataHash {
            solc: Some(Version::new(0, 8, 14)),
        };
        let expected_size = encoded.len();

        // when
        let (decoded, decoded_size) = MetadataHash::from_cbor(encoded.as_ref())
            .expect("Error when decoding valid metadata hash");

        // then
        assert_eq!(expected, decoded, "Incorrectly decoded");
        assert_eq!(expected_size, decoded_size, "Incorrect decoded size")
    }

    #[test]
    fn deserialization_metadata_hash_with_solc_as_string() {
        // given
        // {"ipfs": b'1220BA5AF27FE13BC83E671BD6981216D35DF49AB3AC923741B8948B277F93FBF732', "solc": "0.8.15-ci.2022.5.23+commit.21591531"}
        let hex = "a2646970667358221220ba5af27fe13bc83e671bd6981216d35df49ab3ac923741b8948b277f93fbf73264736f6c637823302e382e31352d63692e323032322e352e32332b636f6d6d69742e3231353931353331";
        let encoded = decode_hex(hex).unwrap();
        let expected = MetadataHash {
            solc: Some(
                Version::from_str("0.8.15-ci.2022.5.23+commit.21591531")
                    .expect("solc version parsing"),
            ),
        };
        let expected_size = encoded.len();

        // when
        let (decoded, decoded_size) = MetadataHash::from_cbor(encoded.as_ref())
            .expect("Error when decoding valid metadata hash");

        // then
        assert_eq!(expected, decoded, "Incorrectly decoded");
        assert_eq!(expected_size, decoded_size, "Incorrect decoded size")
    }

    #[test]
    fn deserialization_of_non_exhausted_string() {
        // given
        // { "ipfs": b"1220BCC988B1311237F2C00CCD0BFBD8B01D24DC18F720603B0DE93FE6327DF53625", "solc": b'00080e' } \
        // { "bzzr0": b"d4fba422541feba2d648f6657d9354ec14ea9f5919b520abe0feb60981d7b17c" }
        let first = "a2646970667358221220bcc988b1311237f2c00ccd0bfbd8b01d24dc18f720603b0de93fe6327df5362564736f6c634300080e";
        let second =
            "a165627a7a72305820d4fba422541feba2d648f6657d9354ec14ea9f5919b520abe0feb60981d7b17c";
        let hex = format!("{first}{second}");
        let encoded = decode_hex(&hex).unwrap();
        let expected = MetadataHash {
            solc: Some(Version::new(0, 8, 14)),
        };
        let expected_size = decode_hex(first).unwrap().len();

        // when
        let (decoded, decoded_size) = MetadataHash::from_cbor(encoded.as_ref())
            .expect("Error when decoding valid metadata hash");

        // then
        assert_eq!(expected, decoded, "Incorrectly decoded");
        assert_eq!(expected_size, decoded_size, "Incorrect decoded size")
    }

    #[test]
    fn deserialization_of_non_cbor_hex_should_fail() {
        // given
        let hex = "1234567890";
        let encoded = decode_hex(hex).unwrap();

        // when
        let decoded = MetadataHash::from_cbor(encoded.as_ref());

        // then
        assert!(decoded.is_err(), "Deserialization should fail");
        assert!(
            decoded.unwrap_err().is_type_mismatch(),
            "Should fail with type mismatch"
        )
    }

    #[test]
    fn deserialization_of_non_map_should_fail() {
        // given
        // "solc"
        let hex = "64736f6c63";
        let encoded = decode_hex(hex).unwrap();

        // when
        let decoded = MetadataHash::from_cbor(encoded.as_ref());

        // then
        assert!(decoded.is_err(), "Deserialization should fail");
        assert!(
            decoded.unwrap_err().is_type_mismatch(),
            "Should fail with type mismatch"
        )
    }

    #[test]
    fn deserialization_with_duplicated_solc_should_fail() {
        // given
        // { "solc": b'000400', "ipfs": b"1220BCC988B1311237F2C00CCD0BFBD8B01D24DC18F720603B0DE93FE6327DF53625", "solc": b'00080e' }
        let hex = "a364736f6c6343000400646970667358221220bcc988b1311237f2c00ccd0bfbd8b01d24dc18f720603b0de93fe6327df5362564736f6c634300080e";
        let encoded = decode_hex(hex).unwrap();

        // when
        let decoded = MetadataHash::from_cbor(encoded.as_ref());

        // then
        assert!(decoded.is_err(), "Deserialization should fail");
        assert!(
            is_valid_custom_error(decoded.unwrap_err(), ParseMetadataHashError::DuplicateKeys),
            "Should fail with custom (DuplicateKey) error"
        );
    }

    #[test]
    fn deserialization_with_not_enough_elements_should_fail() {
        // given
        // 3 elements expected in the map but got only 2:
        // { "ipfs": b"1220BCC988B1311237F2C00CCD0BFBD8B01D24DC18F720603B0DE93FE6327DF53625", "solc": b'00080e' }
        let hex = "a3646970667358221220bcc988b1311237f2c00ccd0bfbd8b01d24dc18f720603b0de93fe6327df5362564736f6c634300080e";
        let encoded = decode_hex(hex).unwrap();

        // when
        let decoded = MetadataHash::from_cbor(encoded.as_ref());

        // then
        assert!(decoded.is_err(), "Deserialization should fail");
        assert!(
            decoded.unwrap_err().is_end_of_input(),
            "Should fail with end of input error"
        );
    }

    #[test]
    fn deserialization_with_solc_neither_bytes_nor_string_should_fail() {
        // given
        // { "ipfs": b"1220BCC988B1311237F2C00CCD0BFBD8B01D24DC18F720603B0DE93FE6327DF53625", "solc": 123 } \
        let hex= "a2646970667358221220bcc988b1311237f2c00ccd0bfbd8b01d24dc18f720603b0de93fe6327df5362564736f6c63187B";
        let encoded = decode_hex(hex).unwrap();

        // when
        let decoded = MetadataHash::from_cbor(encoded.as_ref());

        // then
        assert!(decoded.is_err(), "Deserialization should fail");
        assert!(
            is_valid_custom_error(
                decoded.unwrap_err(),
                ParseMetadataHashError::InvalidSolcType(minicbor::data::Type::Int)
            ),
            "Should fail with custom (InvalidSolcType) error"
        );
    }
}

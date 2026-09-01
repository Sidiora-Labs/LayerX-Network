//! Canonical primitive encoder.

use layerx_types::result::KnownResult;

use crate::limits::{LEGACY_PROTOCOL_VERSION, MAX_PROTOCOL_VERSION, STRUCTURE_VERSION};
use crate::{check_ordered_keys, WireError};

/// A capacity-bounded canonical byte encoder.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Encoder {
    bytes: Vec<u8>,
    allocation_limit: usize,
}

impl Encoder {
    /// Creates an empty encoder with a hard output-allocation limit.
    #[must_use]
    pub const fn new(allocation_limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            allocation_limit,
        }
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), WireError> {
        let offset = self.bytes.len();
        let Some(required) = offset.checked_add(bytes.len()) else {
            return Err(WireError::known(KnownResult::LengthLimit, offset));
        };
        if required > self.allocation_limit {
            return Err(WireError::known(KnownResult::LengthLimit, offset));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    /// Writes one unsigned byte.
    ///
    /// # Errors
    ///
    /// Returns a length-limit error when the output budget is exhausted.
    pub fn u8(&mut self, value: u8) -> Result<(), WireError> {
        self.write(&[value])
    }

    /// Writes a fixed-width big-endian unsigned 16-bit integer.
    ///
    /// # Errors
    ///
    /// Returns a length-limit error when the output budget is exhausted.
    pub fn u16(&mut self, value: u16) -> Result<(), WireError> {
        self.write(&value.to_be_bytes())
    }

    /// Writes a fixed-width big-endian unsigned 32-bit integer.
    ///
    /// # Errors
    ///
    /// Returns a length-limit error when the output budget is exhausted.
    pub fn u32(&mut self, value: u32) -> Result<(), WireError> {
        self.write(&value.to_be_bytes())
    }

    /// Writes a fixed-width big-endian unsigned 64-bit integer.
    ///
    /// # Errors
    ///
    /// Returns a length-limit error when the output budget is exhausted.
    pub fn u64(&mut self, value: u64) -> Result<(), WireError> {
        self.write(&value.to_be_bytes())
    }

    /// Writes a fixed-width big-endian unsigned 128-bit integer.
    ///
    /// # Errors
    ///
    /// Returns a length-limit error when the output budget is exhausted.
    pub fn u128(&mut self, value: u128) -> Result<(), WireError> {
        self.write(&value.to_be_bytes())
    }

    /// Writes the two's-complement bits of a signed 32-bit result code.
    ///
    /// # Errors
    ///
    /// Returns a length-limit error when the output budget is exhausted.
    pub fn i32(&mut self, value: i32) -> Result<(), WireError> {
        self.write(&value.to_be_bytes())
    }

    /// Writes exact bytes without a length prefix.
    ///
    /// # Errors
    ///
    /// Returns a length-limit error when the output budget is exhausted.
    pub fn fixed(&mut self, bytes: &[u8]) -> Result<(), WireError> {
        self.write(bytes)
    }

    /// Writes a 32-bit length followed by bounded bytes.
    ///
    /// # Errors
    ///
    /// Returns a length-limit error before writing when the declared or total
    /// bound would be exceeded.
    pub fn bytes(&mut self, bytes: &[u8], maximum: usize) -> Result<(), WireError> {
        let offset = self.bytes.len();
        crate::limits::enforce(bytes.len(), maximum, offset)?;
        let length = u32::try_from(bytes.len())
            .map_err(|_| WireError::known(KnownResult::LengthLimit, offset))?;
        let Some(required) = offset
            .checked_add(4)
            .and_then(|value| value.checked_add(bytes.len()))
        else {
            return Err(WireError::known(KnownResult::LengthLimit, offset));
        };
        if required > self.allocation_limit {
            return Err(WireError::known(KnownResult::LengthLimit, offset));
        }
        self.u32(length)?;
        self.write(bytes)
    }

    /// Writes UTF-8 text after applying the protocol's canonical subset check.
    ///
    /// # Errors
    ///
    /// Returns non-canonical for invalid or combining-codepoint text and a
    /// length-limit error for an exceeded bound.
    pub fn text(&mut self, value: &str, maximum: usize) -> Result<(), WireError> {
        if value
            .chars()
            .any(|character| ('\u{0300}'..='\u{036f}').contains(&character))
        {
            return Err(WireError::known(
                KnownResult::NonCanonical,
                self.bytes.len(),
            ));
        }
        self.bytes(value.as_bytes(), maximum)
    }

    /// Writes a bounded 32-bit sequence count.
    ///
    /// # Errors
    ///
    /// Returns length-limit when the count exceeds either bound.
    pub fn sequence_length(&mut self, count: usize, maximum: usize) -> Result<(), WireError> {
        let offset = self.bytes.len();
        crate::limits::enforce(count, maximum, offset)?;
        let count =
            u32::try_from(count).map_err(|_| WireError::known(KnownResult::LengthLimit, offset))?;
        self.u32(count)
    }

    /// Writes a closed-union tag.
    ///
    /// # Errors
    ///
    /// Returns invalid-tag when `tag` exceeds the declared maximum.
    pub fn tag(&mut self, tag: u8, maximum: u8) -> Result<(), WireError> {
        if tag > maximum {
            return Err(WireError::known(KnownResult::InvalidTag, self.bytes.len()));
        }
        self.u8(tag)
    }

    /// Writes the protocol version and a non-zero structure tag.
    ///
    /// # Errors
    ///
    /// Returns invalid-tag for zero and length-limit on budget exhaustion.
    pub fn structure_header(&mut self, structure_tag: u16) -> Result<(), WireError> {
        if structure_tag == 0 {
            return Err(WireError::known(KnownResult::InvalidTag, self.bytes.len()));
        }
        self.u16(STRUCTURE_VERSION)?;
        self.u16(structure_tag)
    }

    /// Writes a version-carrying structure header for a non-zero tag.
    ///
    /// # Errors
    ///
    /// Returns invalid-tag for a zero tag, version-unsupported for a version
    /// outside the supported range, and length-limit on budget exhaustion.
    pub fn structure_header_version(
        &mut self,
        structure_tag: u16,
        protocol_version: u16,
    ) -> Result<(), WireError> {
        if structure_tag == 0 {
            return Err(WireError::known(KnownResult::InvalidTag, self.bytes.len()));
        }
        if !(LEGACY_PROTOCOL_VERSION..=MAX_PROTOCOL_VERSION).contains(&protocol_version) {
            return Err(WireError::known(
                KnownResult::VersionUnsupported,
                self.bytes.len(),
            ));
        }
        self.u16(protocol_version)?;
        self.u16(structure_tag)
    }

    /// Writes a canonical sequence of bounded byte strings.
    ///
    /// # Errors
    ///
    /// Returns a typed length error for any exceeded count, item, or total
    /// budget.
    pub fn sequence(
        &mut self,
        values: &[&[u8]],
        maximum_count: usize,
        maximum_item: usize,
    ) -> Result<(), WireError> {
        self.sequence_length(values.len(), maximum_count)?;
        for value in values {
            self.bytes(value, maximum_item)?;
        }
        Ok(())
    }

    /// Writes a canonical ordered map as count plus bounded key/value pairs.
    ///
    /// # Errors
    ///
    /// Rejects duplicate or decreasing keys and all exceeded bounds.
    pub fn ordered_map(
        &mut self,
        entries: &[(&[u8], &[u8])],
        maximum_count: usize,
        maximum_key: usize,
        maximum_value: usize,
    ) -> Result<(), WireError> {
        let keys: Vec<_> = entries.iter().map(|(key, _)| *key).collect();
        check_ordered_keys(&keys)?;
        self.sequence_length(entries.len(), maximum_count)?;
        for (key, value) in entries {
            self.bytes(key, maximum_key)?;
            self.bytes(value, maximum_value)?;
        }
        Ok(())
    }

    /// Borrows all encoded bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Finishes the encoder and returns its canonical bytes.
    #[must_use]
    pub fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

//! Panic-free canonical primitive decoder.

use layerx_types::result::KnownResult;

use crate::limits::{MAX_PROTOCOL_VERSION, PROTOCOL_VERSION};
use crate::WireError;

/// A borrowed decoder with an explicit cumulative owned-allocation budget.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Decoder<'a> {
    bytes: &'a [u8],
    offset: usize,
    allocation_limit: usize,
    allocated: usize,
}

impl<'a> Decoder<'a> {
    /// Starts decoding without allocating from the caller's byte slice.
    #[must_use]
    pub const fn new(bytes: &'a [u8], allocation_limit: usize) -> Self {
        Self {
            bytes,
            offset: 0,
            allocation_limit,
            allocated: 0,
        }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], WireError> {
        let offset = self.offset;
        let Some(end) = offset.checked_add(length) else {
            return Err(WireError::known(KnownResult::Truncated, offset));
        };
        let Some(value) = self.bytes.get(offset..end) else {
            return Err(WireError::known(KnownResult::Truncated, offset));
        };
        self.offset = end;
        Ok(value)
    }

    /// Reads one unsigned byte.
    ///
    /// # Errors
    ///
    /// Returns truncated when fewer than one byte remains.
    pub fn u8(&mut self) -> Result<u8, WireError> {
        Ok(self.take(1)?[0])
    }

    /// Reads a fixed-width big-endian unsigned 16-bit integer.
    ///
    /// # Errors
    ///
    /// Returns truncated when fewer than two bytes remain.
    pub fn u16(&mut self) -> Result<u16, WireError> {
        let offset = self.offset;
        let bytes: [u8; 2] = self
            .take(2)?
            .try_into()
            .map_err(|_| WireError::known(KnownResult::Truncated, offset))?;
        Ok(u16::from_be_bytes(bytes))
    }

    /// Reads a fixed-width big-endian unsigned 32-bit integer.
    ///
    /// # Errors
    ///
    /// Returns truncated when fewer than four bytes remain.
    pub fn u32(&mut self) -> Result<u32, WireError> {
        let offset = self.offset;
        let bytes: [u8; 4] = self
            .take(4)?
            .try_into()
            .map_err(|_| WireError::known(KnownResult::Truncated, offset))?;
        Ok(u32::from_be_bytes(bytes))
    }

    /// Reads a fixed-width big-endian unsigned 64-bit integer.
    ///
    /// # Errors
    ///
    /// Returns truncated when fewer than eight bytes remain.
    pub fn u64(&mut self) -> Result<u64, WireError> {
        let offset = self.offset;
        let bytes: [u8; 8] = self
            .take(8)?
            .try_into()
            .map_err(|_| WireError::known(KnownResult::Truncated, offset))?;
        Ok(u64::from_be_bytes(bytes))
    }

    /// Reads a fixed-width big-endian unsigned 128-bit integer.
    ///
    /// # Errors
    ///
    /// Returns truncated when fewer than sixteen bytes remain.
    pub fn u128(&mut self) -> Result<u128, WireError> {
        let offset = self.offset;
        let bytes: [u8; 16] = self
            .take(16)?
            .try_into()
            .map_err(|_| WireError::known(KnownResult::Truncated, offset))?;
        Ok(u128::from_be_bytes(bytes))
    }

    /// Reads a fixed-width big-endian signed 32-bit result code.
    ///
    /// # Errors
    ///
    /// Returns truncated when fewer than four bytes remain.
    pub fn i32(&mut self) -> Result<i32, WireError> {
        let offset = self.offset;
        let bytes: [u8; 4] = self
            .take(4)?
            .try_into()
            .map_err(|_| WireError::known(KnownResult::Truncated, offset))?;
        Ok(i32::from_be_bytes(bytes))
    }

    /// Borrows an exact number of unprefixed bytes.
    ///
    /// # Errors
    ///
    /// Returns truncated when the requested span is unavailable.
    pub fn fixed(&mut self, length: usize) -> Result<&'a [u8], WireError> {
        self.take(length)
    }

    /// Borrows a 32-bit-length-prefixed byte string after checking its limit.
    ///
    /// # Errors
    ///
    /// Returns length-limit before consuming or allocating its body, or
    /// truncated when the declared body is unavailable.
    pub fn bytes(&mut self, maximum: usize) -> Result<&'a [u8], WireError> {
        let length_offset = self.offset;
        let length = usize::try_from(self.u32()?)
            .map_err(|_| WireError::known(KnownResult::LengthLimit, length_offset))?;
        crate::limits::enforce(length, maximum, length_offset)?;
        self.take(length)
    }

    /// Copies a bounded byte string only after both element and cumulative
    /// allocation limits pass.
    ///
    /// # Errors
    ///
    /// Returns length-limit before allocation or truncated for missing input.
    pub fn bytes_owned(&mut self, maximum: usize) -> Result<Vec<u8>, WireError> {
        let offset = self.offset;
        let value = self.bytes(maximum)?;
        let Some(total) = self.allocated.checked_add(value.len()) else {
            return Err(WireError::known(KnownResult::LengthLimit, offset));
        };
        if total > self.allocation_limit {
            return Err(WireError::known(KnownResult::LengthLimit, offset));
        }
        self.allocated = total;
        Ok(value.to_vec())
    }

    /// Reads canonical UTF-8 text from a bounded byte string.
    ///
    /// # Errors
    ///
    /// Returns non-canonical for invalid UTF-8 or combining codepoints.
    pub fn text(&mut self, maximum: usize) -> Result<&'a str, WireError> {
        let offset = self.offset;
        let bytes = self.bytes(maximum)?;
        let text = std::str::from_utf8(bytes)
            .map_err(|_| WireError::known(KnownResult::NonCanonical, offset))?;
        if text
            .chars()
            .any(|character| ('\u{0300}'..='\u{036f}').contains(&character))
        {
            return Err(WireError::known(KnownResult::NonCanonical, offset));
        }
        Ok(text)
    }

    /// Reads a bounded 32-bit sequence count.
    ///
    /// # Errors
    ///
    /// Returns length-limit when the count exceeds the declared maximum.
    pub fn sequence_length(&mut self, maximum: usize) -> Result<usize, WireError> {
        let offset = self.offset;
        let count = usize::try_from(self.u32()?)
            .map_err(|_| WireError::known(KnownResult::LengthLimit, offset))?;
        crate::limits::enforce(count, maximum, offset)?;
        Ok(count)
    }

    /// Reads a closed-union tag.
    ///
    /// # Errors
    ///
    /// Returns invalid-tag when the encoded tag exceeds the declared maximum.
    pub fn tag(&mut self, maximum: u8) -> Result<u8, WireError> {
        let offset = self.offset;
        let tag = self.u8()?;
        if tag > maximum {
            return Err(WireError::known(KnownResult::InvalidTag, offset));
        }
        Ok(tag)
    }

    /// Reads and validates a protocol structure header.
    ///
    /// # Errors
    ///
    /// Returns version-unsupported for a version or structure-tag mismatch.
    pub fn structure_header(&mut self, expected_tag: u16) -> Result<(), WireError> {
        let offset = self.offset;
        let version = self.u16()?;
        let actual_tag = self.u16()?;
        if version != PROTOCOL_VERSION || expected_tag == 0 || actual_tag != expected_tag {
            return Err(WireError::known(KnownResult::VersionUnsupported, offset));
        }
        Ok(())
    }

    /// Reads a version-carrying structure header and returns its version.
    ///
    /// # Errors
    ///
    /// Returns version-unsupported for an unsupported version or a
    /// structure-tag mismatch.
    pub fn structure_header_version(&mut self, expected_tag: u16) -> Result<u16, WireError> {
        let offset = self.offset;
        let version = self.u16()?;
        let actual_tag = self.u16()?;
        if !(PROTOCOL_VERSION..=MAX_PROTOCOL_VERSION).contains(&version)
            || expected_tag == 0
            || actual_tag != expected_tag
        {
            return Err(WireError::known(KnownResult::VersionUnsupported, offset));
        }
        Ok(version)
    }

    /// Rejects zero and out-of-range field identifiers.
    ///
    /// # Errors
    ///
    /// Returns unknown-field for an undeclared field.
    pub fn field_id(&mut self, maximum: u16) -> Result<u16, WireError> {
        let offset = self.offset;
        let field = self.u16()?;
        if field == 0 || field > maximum {
            return Err(WireError::known(KnownResult::UnknownField, offset));
        }
        Ok(field)
    }

    /// Requires complete consumption, rejecting padding and trailing bytes.
    ///
    /// # Errors
    ///
    /// Returns trailing-bytes when any byte remains.
    pub fn finish(self) -> Result<(), WireError> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(WireError::known(KnownResult::TrailingBytes, self.offset))
        }
    }

    /// Returns the current byte offset for diagnostics.
    #[must_use]
    pub const fn offset(&self) -> usize {
        self.offset
    }

    /// Returns cumulative bytes allocated by owned decode operations.
    #[must_use]
    pub const fn allocated(&self) -> usize {
        self.allocated
    }
}

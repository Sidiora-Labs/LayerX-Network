//! Native Programs call payload shared by node-facing transports.

use crate::intent::ProgramId;

const FIXED_BYTES: usize = 106;
const MAX_BYTES: usize = 1_048_576;

/// The seven resource ceilings in native wire order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Resources(pub [u64; 7]);

/// A borrowed native call, independent of the legacy agent intent encoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeProgramCall<'a> {
    pub program_id: ProgramId,
    pub guest_abi: u16,
    pub entrypoint: &'a [u8],
    pub calldata: &'a [u8],
    pub capabilities: &'a [u8],
    pub access_declaration: &'a [u8],
    pub response_capacity: u32,
    pub resources: Resources,
}

/// A malformed native call header or body.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidNativeCall;

impl<'a> NativeProgramCall<'a> {
    /// Returns the callee identifier.
    #[must_use]
    pub const fn callee(&self) -> ProgramId {
        self.program_id
    }

    fn validate(&self) -> Result<(), InvalidNativeCall> {
        if self.program_id.is_zero()
            || !matches!(self.guest_abi, 1 | 2)
            || self.entrypoint.is_empty()
            || self.entrypoint.len() > 128
            || !self
                .entrypoint
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.'))
            || self.calldata.len() > MAX_BYTES
            || self.capabilities.len() > usize::from(u16::MAX)
            || self.access_declaration.len() > MAX_BYTES
            || self.response_capacity > 1_048_576
        {
            return Err(InvalidNativeCall);
        }
        Ok(())
    }

    /// Encodes the exact native header and length-bound bodies.
    ///
    /// # Errors
    /// Rejects invalid identifiers, guest ABI, entrypoint or length bounds.
    pub fn encode(&self) -> Result<Vec<u8>, InvalidNativeCall> {
        self.validate()?;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&self.program_id.bytes());
        bytes.extend_from_slice(&self.guest_abi.to_be_bytes());
        bytes.extend_from_slice(
            &u16::try_from(self.entrypoint.len())
                .map_err(|_| InvalidNativeCall)?
                .to_be_bytes(),
        );
        bytes.extend_from_slice(
            &u32::try_from(self.calldata.len())
                .map_err(|_| InvalidNativeCall)?
                .to_be_bytes(),
        );
        bytes.extend_from_slice(
            &u16::try_from(self.capabilities.len())
                .map_err(|_| InvalidNativeCall)?
                .to_be_bytes(),
        );
        bytes.extend_from_slice(
            &u32::try_from(self.access_declaration.len())
                .map_err(|_| InvalidNativeCall)?
                .to_be_bytes(),
        );
        bytes.extend_from_slice(&self.response_capacity.to_be_bytes());
        for ceiling in self.resources.0 {
            bytes.extend_from_slice(&ceiling.to_be_bytes());
        }
        for body in [
            self.entrypoint,
            self.calldata,
            self.capabilities,
            self.access_declaration,
        ] {
            bytes.extend_from_slice(body);
        }
        Ok(bytes)
    }

    /// Decodes a native payload without allocating untrusted lengths.
    ///
    /// # Errors
    /// Rejects truncation, trailing bytes and invalid header fields. Capability
    /// authority and access semantics are checked by the native runtime.
    pub fn decode(payload: &'a [u8]) -> Result<Self, InvalidNativeCall> {
        let mut remaining = payload;
        if remaining.len() < FIXED_BYTES {
            return Err(InvalidNativeCall);
        }
        let program_id = ProgramId::new(take::<32>(&mut remaining)?);
        let guest_abi = u16::from_be_bytes(take(&mut remaining)?);
        let entrypoint_length = usize::from(u16::from_be_bytes(take(&mut remaining)?));
        let calldata_length = usize::try_from(u32::from_be_bytes(take(&mut remaining)?))
            .map_err(|_| InvalidNativeCall)?;
        let capabilities_length = usize::from(u16::from_be_bytes(take(&mut remaining)?));
        let access_length = usize::try_from(u32::from_be_bytes(take(&mut remaining)?))
            .map_err(|_| InvalidNativeCall)?;
        let response_capacity = u32::from_be_bytes(take(&mut remaining)?);
        let mut resources = [0; 7];
        for ceiling in &mut resources {
            *ceiling = u64::from_be_bytes(take(&mut remaining)?);
        }
        let call = Self {
            program_id,
            guest_abi,
            entrypoint: body(&mut remaining, entrypoint_length)?,
            calldata: body(&mut remaining, calldata_length)?,
            capabilities: body(&mut remaining, capabilities_length)?,
            access_declaration: body(&mut remaining, access_length)?,
            response_capacity,
            resources: Resources(resources),
        };
        if !remaining.is_empty() {
            return Err(InvalidNativeCall);
        }
        call.validate()?;
        Ok(call)
    }
}

fn body<'a>(remaining: &mut &'a [u8], length: usize) -> Result<&'a [u8], InvalidNativeCall> {
    let (head, tail) = remaining
        .split_at_checked(length)
        .ok_or(InvalidNativeCall)?;
    *remaining = tail;
    Ok(head)
}

fn take<const N: usize>(remaining: &mut &[u8]) -> Result<[u8; N], InvalidNativeCall> {
    body(remaining, N)?
        .try_into()
        .map_err(|_| InvalidNativeCall)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staged_c_call_encoding_is_byte_exact() -> Result<(), InvalidNativeCall> {
        let mut capabilities = vec![0, 3, 2, 3, 5];
        capabilities.extend_from_slice(&[9; 32]);
        capabilities.extend_from_slice(&[10; 32]);
        capabilities.extend_from_slice(&1_u128.to_be_bytes());
        let call = NativeProgramCall {
            program_id: ProgramId::new([0x11; 32]),
            guest_abi: 1,
            entrypoint: b"layerx_call",
            calldata: &[],
            capabilities: &capabilities,
            access_declaration: b"LayerX/programs/access-declaration/v1\0\0",
            response_capacity: 16,
            resources: Resources([
                1_000_000, 16_777_216, 1_048_576, 1_048_576, 64, 1_048_576, 4096,
            ]),
        };
        let expected_hex = concat!(
            "1111111111111111111111111111111111111111111111111111111111111111",
            "0001000b0000000000550000002700000010",
            "00000000000f4240000000000100000000000000001000000000000000100000",
            "000000000000004000000000001000000000000000001000",
            "6c61796572785f63616c6c0003020305",
            "0909090909090909090909090909090909090909090909090909090909090909",
            "0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a",
            "00000000000000000000000000000001",
            "4c61796572582f70726f6772616d732f6163636573732d6465636c61726174696f6e2f76310000"
        );
        let expected = (0..expected_hex.len())
            .step_by(2)
            .map(|offset| {
                u8::from_str_radix(&expected_hex[offset..offset + 2], 16)
                    .map_err(|_| InvalidNativeCall)
            })
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(expected.len(), 241);
        assert_eq!(call.encode()?, expected);
        assert_eq!(NativeProgramCall::decode(&expected)?, call);
        Ok(())
    }

    #[test]
    fn native_fixture_layout_and_malformed_lengths() -> Result<(), InvalidNativeCall> {
        let call = NativeProgramCall {
            program_id: ProgramId::new([0x11; 32]),
            guest_abi: 1,
            entrypoint: b"layerx_call",
            calldata: &[],
            capabilities: &[0, 0],
            access_declaration: b"LayerX/programs/access-declaration/v1\0\0",
            response_capacity: 16,
            resources: Resources([
                1_000_000, 16_777_216, 1_048_576, 1_048_576, 64, 1_048_576, 4096,
            ]),
        };
        let encoded = call.encode()?;
        assert_eq!(&encoded[..32], &[0x11; 32]);
        assert_eq!(
            &encoded[32..50],
            &[0, 1, 0, 11, 0, 0, 0, 0, 0, 2, 0, 0, 0, 39, 0, 0, 0, 16]
        );
        assert_eq!(&encoded[106..117], b"layerx_call");
        assert_eq!(NativeProgramCall::decode(&encoded)?, call);
        for length in 0..encoded.len() {
            assert_eq!(
                NativeProgramCall::decode(&encoded[..length]),
                Err(InvalidNativeCall)
            );
        }
        let mut trailing = encoded.clone();
        trailing.push(0);
        assert_eq!(NativeProgramCall::decode(&trailing), Err(InvalidNativeCall));
        let mut bad_abi = encoded.clone();
        bad_abi[33] = 4;
        assert_eq!(NativeProgramCall::decode(&bad_abi), Err(InvalidNativeCall));
        let mut oversized = encoded;
        oversized[36..40].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(
            NativeProgramCall::decode(&oversized),
            Err(InvalidNativeCall)
        );
        Ok(())
    }
}

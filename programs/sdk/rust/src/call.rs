//! Program-to-program call bindings.
//!
//! A call carries an explicitly narrowed capability list. The host refuses any
//! grant this program does not already hold and any transfer ceiling above the
//! one it was given, so authority can only shrink as a call graph deepens.

use crate::abi::{MAX_CALL_INPUT_BYTES, MAX_CAPABILITY_ENCODING_BYTES};
use crate::error::{Field, ProgramError, Reason};

#[cfg(any(target_arch = "wasm32", test))]
use crate::capability::CapabilitySet;
#[cfg(target_arch = "wasm32")]
use crate::host;
#[cfg(target_arch = "wasm32")]
use crate::ids::ProgramId;

const MINIMUM_CAPABILITY_ENCODING_BYTES: usize = 2;

/// The input bytes handed to a called program.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallInput<'a>(&'a [u8]);

impl<'a> CallInput<'a> {
    /// Constructs call input inside the version-one bound.
    ///
    /// # Errors
    ///
    /// Refuses input past the declared bound.
    pub const fn new(bytes: &'a [u8]) -> Result<Self, ProgramError> {
        if bytes.len() > MAX_CALL_INPUT_BYTES {
            return Err(ProgramError::value(Field::CallInput, Reason::TooLarge));
        }
        Ok(Self(bytes))
    }

    /// Borrows the canonical input bytes.
    #[must_use]
    pub const fn bytes(self) -> &'a [u8] {
        self.0
    }
}

/// A non-negative result code a called program returns.
///
/// The runtime treats a negative code from a callee as a refusal that aborts
/// the whole call graph, so this type holds no negative value at all.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct CallResult(i32);

impl CallResult {
    /// The code a callee returns for an unqualified success.
    pub const OK: Self = Self(0);

    /// Builds a non-negative result code.
    ///
    /// # Errors
    ///
    /// Refuses a code wider than the boundary's signed integer.
    pub fn new(code: u32) -> Result<Self, ProgramError> {
        i32::try_from(code)
            .map(Self)
            .map_err(|_| ProgramError::value(Field::CallResult, Reason::TooLarge))
    }

    /// Returns the code as it crosses the boundary.
    #[must_use]
    pub const fn code(self) -> i32 {
        self.0
    }
}

/// Successful candidate response borrowing the initialized caller buffer prefix.
#[derive(Debug, Eq, PartialEq)]
pub struct CallResponse<'a> {
    code: i32,
    bytes: &'a [u8],
}

impl CallResponse<'_> {
    #[must_use]
    pub const fn code(&self) -> i32 {
        self.code
    }
    #[must_use]
    pub const fn bytes(&self) -> &[u8] {
        self.bytes
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn decode_response(output: &mut [u8], packed: i64) -> Result<CallResponse<'_>, ProgramError> {
    if packed < 0 {
        return Err(ProgramError::Host(crate::error::HostRefusal::from_status(
            i32::try_from(packed).unwrap_or(crate::error::STATUS_INVALID),
        )));
    }
    let packed = u64::try_from(packed)
        .map_err(|_| ProgramError::Host(crate::error::HostRefusal::Invalid))?;
    let code = i32::try_from(packed >> 32)
        .map_err(|_| ProgramError::value(Field::CallResult, Reason::TooLarge))?;
    let length = usize::try_from(packed & u64::from(u32::MAX))
        .map_err(|_| ProgramError::value(Field::Buffer, Reason::TooLarge))?;
    let bytes = output
        .get(..length)
        .ok_or_else(|| ProgramError::value(Field::Buffer, Reason::TooSmall))?;
    Ok(CallResponse { code, bytes })
}

/// An encoded capability list narrowed for one call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GrantedCapabilities<'a>(&'a [u8]);

impl<'a> GrantedCapabilities<'a> {
    /// Wraps bytes already produced by
    /// [`CapabilitySet::encode_into`](crate::capability::CapabilitySet::encode_into).
    ///
    /// # Errors
    ///
    /// Refuses an encoding shorter than the grant count and an encoding past
    /// the declared bound.
    pub const fn new(encoded: &'a [u8]) -> Result<Self, ProgramError> {
        if encoded.len() < MINIMUM_CAPABILITY_ENCODING_BYTES {
            return Err(ProgramError::value(
                Field::CapabilityEncoding,
                Reason::Malformed,
            ));
        }
        if encoded.len() > MAX_CAPABILITY_ENCODING_BYTES {
            return Err(ProgramError::value(
                Field::CapabilityEncoding,
                Reason::TooLarge,
            ));
        }
        Ok(Self(encoded))
    }

    /// Borrows the canonical encoded grant list.
    #[must_use]
    pub const fn bytes(self) -> &'a [u8] {
        self.0
    }
}

/// Calls another program with an already-encoded grant list, returning the
/// callee's non-negative result code.
///
/// # Errors
///
/// Refuses missing call authority, an oversized input, any escalation of the
/// held grants, and every meter refusal.
#[cfg(target_arch = "wasm32")]
pub fn invoke(
    callee: ProgramId,
    input: CallInput<'_>,
    capabilities: GrantedCapabilities<'_>,
) -> Result<i32, ProgramError> {
    let program = callee.bytes();
    host::program_call(&program, input.bytes(), capabilities.bytes())
}

/// Encodes a capability set into a caller-owned scratch buffer and calls
/// another program with it, returning the callee's non-negative result code.
///
/// # Errors
///
/// Refuses a scratch buffer shorter than the encoding, missing call
/// authority, an oversized input, any escalation of the held grants, and
/// every meter refusal.
#[cfg(target_arch = "wasm32")]
pub fn invoke_with<const N: usize>(
    callee: ProgramId,
    input: CallInput<'_>,
    capabilities: &CapabilitySet<N>,
    scratch: &mut [u8],
) -> Result<i32, ProgramError> {
    invoke(callee, input, encode_capabilities(capabilities, scratch)?)
}

#[cfg(any(target_arch = "wasm32", test))]
fn encode_capabilities<'a, const N: usize>(
    capabilities: &CapabilitySet<N>,
    scratch: &'a mut [u8],
) -> Result<GrantedCapabilities<'a>, ProgramError> {
    let written = capabilities.encode_into(scratch)?;
    let encoded = scratch
        .get(..written)
        .ok_or_else(|| ProgramError::value(Field::Buffer, Reason::TooSmall))?;
    GrantedCapabilities::new(encoded)
}

/// Calls through the explicitly selected candidate response operation.
#[cfg(target_arch = "wasm32")]
pub fn invoke_response<'a>(
    callee: ProgramId,
    input: CallInput<'_>,
    capabilities: GrantedCapabilities<'_>,
    output: &'a mut [u8],
) -> Result<CallResponse<'a>, ProgramError> {
    if output.len() > crate::abi::MAX_CALL_RESPONSE_BYTES {
        return Err(ProgramError::value(Field::Buffer, Reason::TooLarge));
    }
    let packed =
        host::program_call_response(&callee.bytes(), input.bytes(), capabilities.bytes(), output)?;
    decode_response(output, packed)
}

/// Encodes narrowed capabilities into caller-owned scratch and invokes the
/// candidate response operation into a distinct caller-owned output buffer.
///
/// # Errors
///
/// Refuses an undersized capability scratch buffer, an oversized response
/// buffer, missing call authority, escalation, and every host refusal.
#[cfg(target_arch = "wasm32")]
pub fn invoke_response_with<'a, const N: usize>(
    callee: ProgramId,
    input: CallInput<'_>,
    capabilities: &CapabilitySet<N>,
    scratch: &mut [u8],
    output: &'a mut [u8],
) -> Result<CallResponse<'a>, ProgramError> {
    invoke_response(
        callee,
        input,
        encode_capabilities(capabilities, scratch)?,
        output,
    )
}

#[cfg(test)]
mod response_tests {
    use super::{decode_response, encode_capabilities};
    use crate::{Capability, CapabilitySet, Field, ProgramError, Reason, ValueError};

    #[test]
    fn decoded_response_borrows_exact_initialized_prefix() {
        let mut empty = [];
        let response = decode_response(&mut empty, 7i64 << 32)
            .unwrap_or_else(|error| panic!("empty: {error}"));
        assert_eq!(response.code(), 7);
        assert!(response.bytes().is_empty());

        let mut maximum = std::vec![0xa5; crate::MAX_CALL_RESPONSE_BYTES];
        let packed = (9i64 << 32)
            | i64::try_from(crate::MAX_CALL_RESPONSE_BYTES)
                .unwrap_or_else(|error| panic!("response bound: {error}"));
        let response = decode_response(&mut maximum, packed)
            .unwrap_or_else(|error| panic!("maximum: {error}"));
        assert_eq!(response.bytes().len(), crate::MAX_CALL_RESPONSE_BYTES);
    }

    #[test]
    fn over_capacity_decode_refuses_without_touching_sentinel() {
        let mut output = [0x5a; 4];
        assert_eq!(
            decode_response(&mut output, (1i64 << 32) | 5),
            Err(ProgramError::Value(ValueError::new(
                Field::Buffer,
                Reason::TooSmall
            )))
        );
        assert_eq!(output, [0x5a; 4]);
    }

    #[test]
    fn response_convenience_uses_the_same_exact_capability_encoding() {
        let capabilities = CapabilitySet::<1>::from_grants(&[Capability::StorageWrite])
            .unwrap_or_else(|error| panic!("capabilities: {error}"));
        let mut direct = [0u8; 3];
        let written = capabilities
            .encode_into(&mut direct)
            .unwrap_or_else(|error| panic!("direct encoding: {error}"));
        let mut convenience = [0u8; 3];
        let encoded = encode_capabilities(&capabilities, &mut convenience)
            .unwrap_or_else(|error| panic!("convenience encoding: {error}"));

        assert_eq!(encoded.bytes(), &direct[..written]);
        assert_eq!(encoded.bytes(), &[0, 1, 2]);
    }

    #[test]
    fn response_convenience_refuses_an_undersized_scratch_buffer() {
        let capabilities = CapabilitySet::<1>::from_grants(&[Capability::StorageWrite])
            .unwrap_or_else(|error| panic!("capabilities: {error}"));
        let mut scratch = [0xa5; 2];

        assert_eq!(
            encode_capabilities(&capabilities, &mut scratch),
            Err(ProgramError::Value(ValueError::new(
                Field::Buffer,
                Reason::TooSmall,
            )))
        );
    }
}

/// Publishes a successful candidate response synchronously.
#[cfg(target_arch = "wasm32")]
pub fn publish_response(result: CallResult, bytes: &[u8]) -> Result<(), ProgramError> {
    if bytes.len() > crate::abi::MAX_CALL_RESPONSE_BYTES {
        return Err(ProgramError::value(Field::Buffer, Reason::TooLarge));
    }
    host::response_write(result.code(), bytes).map(|_| ())
}

/// Publishes a candidate program refusal synchronously.
#[cfg(target_arch = "wasm32")]
pub fn publish_refusal(refusal: crate::ProgramRefusal<'_>) -> Result<(), ProgramError> {
    crate::host::refusal_write(refusal.class(), refusal.reason()).map(|_| ())
}

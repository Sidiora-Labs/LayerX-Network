//! Candidate-only typed program failure payloads.

use core::fmt::{self, Display};

use crate::storage::ProgramId;

/// Maximum opaque reason carried by one candidate program refusal.
pub const MAX_REFUSAL_REASON_BYTES: usize = 4_096;

/// Candidate entry return reserved for a successfully published refusal.
pub const CANDIDATE_REFUSAL_SENTINEL: i32 = -64;
/// Canonical class vocabulary mirrored by candidate SDK parity checks.
pub const REFUSAL_CLASS_MANIFEST: &str = "Rejected=1\0InvalidInput=2\0Unauthorized=3\0Conflict=4\0NotFound=5\0RuntimeFault=254\0Legacy=255\0";

/// Stable receipt refusal vocabulary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum RefusalClass {
    Rejected = 1,
    InvalidInput = 2,
    Unauthorized = 3,
    Conflict = 4,
    NotFound = 5,
    RuntimeFault = 254,
    Legacy = 255,
}

impl RefusalClass {
    #[must_use]
    pub const fn code(self) -> u32 {
        self as u32
    }

    #[must_use]
    pub const fn is_guest_publishable(self) -> bool {
        matches!(
            self,
            Self::Rejected
                | Self::InvalidInput
                | Self::Unauthorized
                | Self::Conflict
                | Self::NotFound
        )
    }

    /// Decodes the closed receipt vocabulary.
    ///
    /// # Errors
    ///
    /// Refuses unknown or reserved numeric values.
    pub const fn decode(code: u32) -> Result<Self, FailureEncodingError> {
        match code {
            1 => Ok(Self::Rejected),
            2 => Ok(Self::InvalidInput),
            3 => Ok(Self::Unauthorized),
            4 => Ok(Self::Conflict),
            5 => Ok(Self::NotFound),
            254 => Ok(Self::RuntimeFault),
            255 => Ok(Self::Legacy),
            _ => Err(FailureEncodingError::UnknownClass),
        }
    }
}

/// Owned bounded opaque program-supplied reason.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RefusalReason(Vec<u8>);

impl RefusalReason {
    #[must_use]
    pub const fn empty() -> Self {
        Self(Vec::new())
    }

    /// Copies a reason only after validating its bound.
    ///
    /// # Errors
    ///
    /// Refuses bytes beyond [`MAX_REFUSAL_REASON_BYTES`].
    pub fn new(bytes: &[u8]) -> Result<Self, FailureEncodingError> {
        if bytes.len() > MAX_REFUSAL_REASON_BYTES {
            return Err(FailureEncodingError::ReasonTooLarge);
        }
        Ok(Self(bytes.to_vec()))
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.0
    }

    #[must_use]
    pub fn canonical_encode(&self) -> Vec<u8> {
        let mut encoded = Vec::with_capacity(4usize.saturating_add(self.0.len()));
        encoded.extend_from_slice(
            &u32::try_from(self.0.len())
                .unwrap_or(u32::MAX)
                .to_be_bytes(),
        );
        encoded.extend_from_slice(&self.0);
        encoded
    }

    /// Strictly decodes `u32 length || bytes`.
    ///
    /// # Errors
    ///
    /// Refuses oversized, truncated, or trailing input.
    pub fn canonical_decode(encoded: &[u8]) -> Result<Self, FailureEncodingError> {
        let length = u32::from_be_bytes(take::<4>(encoded, 0)?) as usize;
        if length > MAX_REFUSAL_REASON_BYTES {
            return Err(FailureEncodingError::ReasonTooLarge);
        }
        let end = 4usize
            .checked_add(length)
            .ok_or(FailureEncodingError::Malformed)?;
        if end != encoded.len() {
            return Err(FailureEncodingError::Malformed);
        }
        Self::new(encoded.get(4..end).ok_or(FailureEncodingError::Malformed)?)
    }
}

/// Host-authenticated candidate program failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramFailure {
    program: ProgramId,
    class: RefusalClass,
    reason: RefusalReason,
}

impl ProgramFailure {
    /// Constructs a canonical host-authenticated failure.
    ///
    /// # Errors
    ///
    /// Host-only fault and legacy classes require an empty reason.
    pub fn new(
        program: ProgramId,
        class: RefusalClass,
        reason: RefusalReason,
    ) -> Result<Self, FailureEncodingError> {
        if matches!(class, RefusalClass::RuntimeFault | RefusalClass::Legacy)
            && !reason.bytes().is_empty()
        {
            return Err(FailureEncodingError::InvalidHostReason);
        }
        Ok(Self {
            program,
            class,
            reason,
        })
    }

    pub(crate) fn authenticated(
        program: ProgramId,
        class: RefusalClass,
        reason: RefusalReason,
    ) -> Self {
        Self::new(program, class, reason)
            .unwrap_or_else(|_| unreachable!("runtime creates canonical host failures"))
    }

    #[must_use]
    pub const fn program(&self) -> ProgramId {
        self.program
    }

    #[must_use]
    pub const fn class(&self) -> RefusalClass {
        self.class
    }

    #[must_use]
    pub const fn reason(&self) -> &RefusalReason {
        &self.reason
    }

    #[must_use]
    pub fn canonical_encode(&self) -> Vec<u8> {
        let reason = self.reason.canonical_encode();
        let mut encoded = Vec::with_capacity(36usize.saturating_add(reason.len()));
        encoded.extend_from_slice(&self.program.bytes());
        encoded.extend_from_slice(&self.class.code().to_be_bytes());
        encoded.extend_from_slice(&reason);
        encoded
    }

    /// Strictly decodes a host-authenticated failure payload.
    ///
    /// # Errors
    ///
    /// Refuses invalid program IDs, classes, reason encodings, and trailing data.
    pub fn canonical_decode(encoded: &[u8]) -> Result<Self, FailureEncodingError> {
        let program = ProgramId::new(take::<32>(encoded, 0)?)
            .map_err(|_| FailureEncodingError::InvalidProgram)?;
        let class = RefusalClass::decode(u32::from_be_bytes(take::<4>(encoded, 32)?))?;
        let reason = RefusalReason::canonical_decode(
            encoded.get(36..).ok_or(FailureEncodingError::Malformed)?,
        )?;
        Self::new(program, class, reason)
    }
}

/// Strict failure payload construction or decoding refusal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailureEncodingError {
    ReasonTooLarge,
    UnknownClass,
    InvalidProgram,
    InvalidHostReason,
    Malformed,
}

impl Display for FailureEncodingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReasonTooLarge => formatter.write_str("refusal reason exceeds its bound"),
            Self::UnknownClass => formatter.write_str("unknown refusal class"),
            Self::InvalidProgram => formatter.write_str("invalid refusing program"),
            Self::InvalidHostReason => {
                formatter.write_str("host-only refusal classes require an empty reason")
            }
            Self::Malformed => formatter.write_str("malformed failure encoding"),
        }
    }
}

impl std::error::Error for FailureEncodingError {}

fn take<const N: usize>(encoded: &[u8], offset: usize) -> Result<[u8; N], FailureEncodingError> {
    let end = offset
        .checked_add(N)
        .ok_or(FailureEncodingError::Malformed)?;
    let bytes = encoded
        .get(offset..end)
        .ok_or(FailureEncodingError::Malformed)?;
    let mut output = [0u8; N];
    output.copy_from_slice(bytes);
    Ok(output)
}

pub use crate::lxt20::REFERENCE_ASSET;

use crate::{Amount, Bytes, Field, Principal, ProgramError, Reason};

pub const MIN_NAME_BYTES: usize = 3;
pub const MAX_NAME_BYTES: usize = 63;
pub const LABEL_BYTES: usize = 64;
pub const RECORD_BYTES: usize = 40;
pub const MAX_REQUEST_PAYLOAD_BYTES: usize = 100;
pub const MAX_REQUEST_BYTES: usize = 110;

pub const NAME_KEY_PREFIX: &[u8] = b"lx.ref.naming.name/";
pub const DID_KEY_PREFIX: &[u8] = b"lx.ref.naming.did/";

pub const REFERENCE_OCCUPANCY_SEED: &[u8] = b"lx.ref.naming.occupancy";
pub const REFERENCE_PERIOD_PRICE: u128 = 1_000;
pub const REFERENCE_PERIOD_BATCHES: u64 = 2_592_000;
pub const REFERENCE_MAX_PERIODS: u32 = 10;
pub const REFERENCE_OCCUPANCY_CEILING: u128 = REFERENCE_PERIOD_PRICE * 10;

/// One registrable label of the naming grammar.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Name<'a>(&'a [u8]);

impl<'a> Name<'a> {
    /// Admits 3 to 63 bytes drawn from `[a-z0-9-]` without a leading or
    /// trailing hyphen. The grammar is ASCII, so an admitted label is
    /// byte-identical to its UTF-8 NFC normalisation and a decomposed or
    /// otherwise unnormalised encoding never reaches storage.
    ///
    /// # Errors
    ///
    /// Refuses a label outside the length bound, a byte outside the grammar
    /// and a leading or trailing hyphen.
    pub const fn new(bytes: &'a [u8]) -> Result<Self, ProgramError> {
        if bytes.len() < MIN_NAME_BYTES || bytes.len() > MAX_NAME_BYTES {
            return Err(malformed());
        }
        if bytes[0] == b'-' || bytes[bytes.len() - 1] == b'-' {
            return Err(malformed());
        }
        let mut index = 0;
        while index < bytes.len() {
            match bytes[index] {
                b'a'..=b'z' | b'0'..=b'9' | b'-' => index += 1,
                _ => return Err(malformed()),
            }
        }
        Ok(Self(bytes))
    }

    #[must_use]
    pub const fn bytes(self) -> &'a [u8] {
        self.0
    }
}

/// The occupancy record one name holds: the DID it resolves to and the batch
/// height its paid occupancy runs out at.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Record {
    pub did: Principal,
    pub expiry: u64,
}

impl Record {
    /// # Errors
    /// Refuses an encoding past the record bound.
    pub fn encode(self) -> Result<Bytes<RECORD_BYTES>, ProgramError> {
        let mut output = Bytes::empty();
        output.extend(&self.did.bytes())?;
        output.extend(&self.expiry.to_be_bytes())?;
        Ok(output)
    }

    /// # Errors
    /// Refuses a wrong length, the reserved identifier and a zero expiry.
    pub fn decode(bytes: &[u8]) -> Result<Self, ProgramError> {
        if bytes.len() != RECORD_BYTES {
            return Err(malformed());
        }
        let did = Principal::new(array(bytes, 0)?)?;
        let expiry = u64::from_be_bytes(array(bytes, 32)?);
        if expiry == 0 {
            return Err(malformed());
        }
        Ok(Self { did, expiry })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Request<'a> {
    Register {
        name: Name<'a>,
        did: Principal,
        periods: u32,
    },
    Transfer {
        name: Name<'a>,
        did: Principal,
    },
    Renew {
        name: Name<'a>,
        periods: u32,
    },
    Resolve {
        name: Name<'a>,
    },
    ReverseResolve {
        did: Principal,
    },
}

impl<'a> Request<'a> {
    #[must_use]
    pub const fn method(self) -> &'static str {
        match self {
            Self::Register { .. } => "register",
            Self::Transfer { .. } => "transfer",
            Self::Renew { .. } => "renew",
            Self::Resolve { .. } => "resolve",
            Self::ReverseResolve { .. } => "reverse_resolve",
        }
    }

    #[must_use]
    pub const fn discriminator(self) -> [u8; 4] {
        [
            b'L',
            b'X',
            b'N',
            match self {
                Self::Register { .. } => 1,
                Self::Transfer { .. } => 2,
                Self::Renew { .. } => 3,
                Self::Resolve { .. } => 4,
                Self::ReverseResolve { .. } => 5,
            },
        ]
    }

    /// Encodes a discriminator followed by canonical `LayerX` bounded bytes.
    ///
    /// # Errors
    ///
    /// Refuses an occupancy term outside the reference bound and a payload
    /// past the reference bound.
    pub fn encode(self) -> Result<Bytes<MAX_REQUEST_BYTES>, ProgramError> {
        let mut payload = Bytes::<MAX_REQUEST_PAYLOAD_BYTES>::empty();
        match self {
            Self::Register { name, did, periods } => {
                periods_in_range(periods)?;
                payload.extend(&did.bytes())?;
                payload.extend(&periods.to_be_bytes())?;
                payload.extend(encode_label(name)?.as_slice())?;
            }
            Self::Transfer { name, did } => {
                payload.extend(&did.bytes())?;
                payload.extend(encode_label(name)?.as_slice())?;
            }
            Self::Renew { name, periods } => {
                periods_in_range(periods)?;
                payload.extend(&periods.to_be_bytes())?;
                payload.extend(encode_label(name)?.as_slice())?;
            }
            Self::Resolve { name } => payload.extend(encode_label(name)?.as_slice())?,
            Self::ReverseResolve { did } => payload.extend(&did.bytes())?,
        }
        let mut output = Bytes::empty();
        output.extend(&self.discriminator())?;
        output.extend(&[1, 0x20])?;
        let length = u32::try_from(payload.len()).map_err(|_| malformed())?;
        output.extend(&length.to_be_bytes())?;
        output.extend(payload.as_slice())?;
        Ok(output)
    }

    /// # Errors
    /// Refuses unknown selectors, wrong tags, lengths, trailing bytes,
    /// reserved identifiers, labels outside the grammar and occupancy terms
    /// outside the reference bound.
    pub fn decode(input: &'a [u8]) -> Result<Self, ProgramError> {
        if input.get(..3) != Some(b"LXN") || input.get(4..6) != Some(&[1, 0x20]) {
            return Err(malformed());
        }
        let length =
            usize::try_from(u32::from_be_bytes(array(input, 6)?)).map_err(|_| malformed())?;
        let payload = input.get(10..).ok_or_else(malformed)?;
        if payload.len() != length || length > MAX_REQUEST_PAYLOAD_BYTES {
            return Err(malformed());
        }
        let request = match input[3] {
            1 => Self::Register {
                did: Principal::new(array(payload, 0)?)?,
                periods: u32::from_be_bytes(array(payload, 32)?),
                name: tail_label(payload, 36)?,
            },
            2 => Self::Transfer {
                did: Principal::new(array(payload, 0)?)?,
                name: tail_label(payload, 32)?,
            },
            3 => Self::Renew {
                periods: u32::from_be_bytes(array(payload, 0)?),
                name: tail_label(payload, 4)?,
            },
            4 => Self::Resolve {
                name: tail_label(payload, 0)?,
            },
            5 if length == 32 => Self::ReverseResolve {
                did: Principal::new(array(payload, 0)?)?,
            },
            _ => return Err(malformed()),
        };
        request.encode()?;
        Ok(request)
    }
}

/// Encodes one length-prefixed label.
///
/// # Errors
///
/// Refuses a label past the reference bound.
pub fn encode_label(name: Name<'_>) -> Result<Bytes<LABEL_BYTES>, ProgramError> {
    let mut output = Bytes::empty();
    output.push(u8::try_from(name.bytes().len()).map_err(|_| malformed())?)?;
    output.extend(name.bytes())?;
    Ok(output)
}

/// Decodes one length-prefixed label that consumes its input exactly.
///
/// # Errors
///
/// Refuses a short or trailing encoding and a label outside the grammar.
pub fn decode_label(bytes: &[u8]) -> Result<Name<'_>, ProgramError> {
    let length = usize::from(*bytes.first().ok_or_else(malformed)?);
    let end = length.checked_add(1).ok_or_else(malformed)?;
    if end != bytes.len() {
        return Err(malformed());
    }
    Name::new(bytes.get(1..end).ok_or_else(malformed)?)
}

/// Returns the occupancy a term of `periods` costs in the reference asset.
///
/// # Errors
///
/// Refuses an occupancy term outside the reference bound and a price past the
/// protocol width.
pub fn occupancy_price(periods: u32) -> Result<Amount, ProgramError> {
    periods_in_range(periods)?;
    Amount::from_u128(REFERENCE_PERIOD_PRICE).checked_mul(Amount::from_u128(u128::from(periods)))
}

/// Returns the batch height a term of `periods` bought from `from` runs to.
///
/// # Errors
///
/// Refuses an occupancy term outside the reference bound and an expiry past
/// the batch height width.
pub fn occupancy_expiry(from: u64, periods: u32) -> Result<u64, ProgramError> {
    periods_in_range(periods)?;
    u64::from(periods)
        .checked_mul(REFERENCE_PERIOD_BATCHES)
        .and_then(|span| from.checked_add(span))
        .ok_or_else(overflowed)
}

const fn periods_in_range(periods: u32) -> Result<(), ProgramError> {
    if periods == 0 {
        return Err(ProgramError::value(Field::Amount, Reason::Zero));
    }
    if periods > REFERENCE_MAX_PERIODS {
        return Err(ProgramError::value(Field::Amount, Reason::TooLarge));
    }
    Ok(())
}

fn tail_label(payload: &[u8], offset: usize) -> Result<Name<'_>, ProgramError> {
    decode_label(payload.get(offset..).ok_or_else(malformed)?)
}

fn array<const N: usize>(input: &[u8], offset: usize) -> Result<[u8; N], ProgramError> {
    input
        .get(offset..offset + N)
        .ok_or_else(malformed)?
        .try_into()
        .map_err(|_| malformed())
}

const fn malformed() -> ProgramError {
    ProgramError::value(Field::CallInput, Reason::Malformed)
}

const fn overflowed() -> ProgramError {
    ProgramError::value(Field::Amount, Reason::Overflow)
}

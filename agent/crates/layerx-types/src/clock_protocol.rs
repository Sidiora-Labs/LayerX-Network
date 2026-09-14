use crate::clock::{ClockError, ClockReading};

pub const REQUEST_BYTES: usize = 24;
pub const RESPONSE_BYTES: usize = 48;
pub const MAX_WAIT_NANOSECONDS: u64 = 300_000_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Request {
    pub counter: u64,
    pub wait_nanoseconds: u64,
}

impl Request {
    /// # Errors
    /// Refuses a zero counter or an excessive wait.
    pub fn encode(self) -> Result<[u8; REQUEST_BYTES], ClockError> {
        if self.counter == 0 || self.wait_nanoseconds > MAX_WAIT_NANOSECONDS {
            return Err(ClockError::Invalid);
        }
        let mut bytes = [0; REQUEST_BYTES];
        bytes[..4].copy_from_slice(b"LXCK");
        bytes[4] = 1;
        bytes[5] = if self.wait_nanoseconds == 0 { 1 } else { 2 };
        bytes[8..16].copy_from_slice(&self.counter.to_be_bytes());
        bytes[16..24].copy_from_slice(&self.wait_nanoseconds.to_be_bytes());
        Ok(bytes)
    }

    /// # Errors
    /// Refuses every noncanonical request field.
    pub fn decode(bytes: &[u8; REQUEST_BYTES]) -> Result<Self, ClockError> {
        let request = Self {
            counter: integer(&bytes[8..16])?,
            wait_nanoseconds: integer(&bytes[16..24])?,
        };
        if &request.encode()? != bytes {
            return Err(ClockError::Invalid);
        }
        Ok(request)
    }
}

#[must_use]
pub fn response(counter: u64, result: Result<ClockReading, ClockError>) -> [u8; RESPONSE_BYTES] {
    let mut bytes = [0; RESPONSE_BYTES];
    bytes[..4].copy_from_slice(b"LXCR");
    bytes[4] = 1;
    bytes[8..16].copy_from_slice(&counter.to_be_bytes());
    match result {
        Ok(reading) => {
            bytes[16..32].copy_from_slice(&reading.generation);
            bytes[32..40].copy_from_slice(&reading.unix_milliseconds.to_be_bytes());
            bytes[40..48].copy_from_slice(&reading.monotonic_nanoseconds.to_be_bytes());
        }
        Err(error) => {
            bytes[5] = match error {
                ClockError::Unavailable => 1,
                ClockError::Invalid => 2,
                ClockError::Overflow => 3,
                ClockError::Regression => 4,
            };
        }
    }
    bytes
}

/// # Errors
/// Refuses malformed, uncorrelated and failed authority responses.
pub fn decode_response(
    bytes: &[u8; RESPONSE_BYTES],
    expected_counter: u64,
) -> Result<ClockReading, ClockError> {
    if &bytes[..4] != b"LXCR"
        || bytes[4] != 1
        || bytes[6..8] != [0; 2]
        || expected_counter == 0
        || integer(&bytes[8..16])? != expected_counter
    {
        return Err(ClockError::Invalid);
    }
    if bytes[5] != 0 {
        if bytes[16..] != [0; 32] {
            return Err(ClockError::Invalid);
        }
        return Err(match bytes[5] {
            1 => ClockError::Unavailable,
            3 => ClockError::Overflow,
            4 => ClockError::Regression,
            _ => ClockError::Invalid,
        });
    }
    let generation = bytes[16..32].try_into().map_err(|_| ClockError::Invalid)?;
    if generation == [0; 16] {
        return Err(ClockError::Invalid);
    }
    Ok(ClockReading {
        generation,
        unix_milliseconds: integer(&bytes[32..40])?,
        monotonic_nanoseconds: integer(&bytes[40..48])?,
    })
}

fn integer(bytes: &[u8]) -> Result<u64, ClockError> {
    Ok(u64::from_be_bytes(
        bytes.try_into().map_err(|_| ClockError::Invalid)?,
    ))
}

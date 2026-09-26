//! Web answers committed for requests the calling program owns.
//!
//! The host serves only answers already committed in module storage, so a
//! read never waits on the network. A request owned by any other program
//! reads as absent, exactly like a request with nothing committed.
use crate::error::{Field, ProgramError, Reason};

/// Fixed bytes preceding the response: content digest, full response length
/// and returned response length, both lengths little-endian.
pub const ANSWER_HEADER_BYTES: usize = 40;
/// Largest response a committed answer carries.
pub const MAX_RESPONSE_BYTES: usize = 4_096;
/// Buffer length that holds every committed answer record.
pub const RECORD_BYTES: usize = ANSWER_HEADER_BYTES + MAX_RESPONSE_BYTES;
/// Host status for a request with no committed answer.
pub const STATUS_ABSENT: i32 = -7;

/// One committed web answer borrowed from the caller's record buffer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Answer<'a> {
    pub content_digest: [u8; 32],
    pub full_length: u32,
    pub response: &'a [u8],
}

impl<'a> Answer<'a> {
    /// Decodes the exact record the host writes into guest memory.
    ///
    /// # Errors
    ///
    /// Refuses a record whose header and length disagree or whose response
    /// exceeds the declared bounds.
    pub fn from_record(record: &'a [u8]) -> Result<Self, ProgramError> {
        let malformed = || ProgramError::value(Field::Buffer, Reason::Malformed);
        if record.len() < ANSWER_HEADER_BYTES {
            return Err(malformed());
        }
        let (header, response) = record.split_at(ANSWER_HEADER_BYTES);
        let mut content_digest = [0u8; 32];
        content_digest.copy_from_slice(&header[..32]);
        let mut full_length = [0u8; 4];
        full_length.copy_from_slice(&header[32..36]);
        let full_length = u32::from_le_bytes(full_length);
        let mut response_length = [0u8; 4];
        response_length.copy_from_slice(&header[36..]);
        let response_length = u32::from_le_bytes(response_length);
        if response_length > full_length
            || usize::try_from(response_length).map_err(|_| malformed())? != response.len()
            || response.len() > MAX_RESPONSE_BYTES
        {
            return Err(malformed());
        }
        Ok(Self {
            content_digest,
            full_length,
            response,
        })
    }

    /// Reports whether the committed response is a prefix of a longer body.
    #[must_use]
    pub fn is_truncated(&self) -> bool {
        u32::try_from(self.response.len()).is_ok_and(|length| length < self.full_length)
    }
}

/// Reads the committed answer for one request this program owns.
///
/// Returns `None` when nothing is committed for the request.
///
/// # Errors
///
/// Returns the typed refusal the host produced for a malformed request or
/// buffer, or a value error for a record that does not decode.
#[cfg(target_arch = "wasm32")]
pub fn read(
    request_id: u64,
    buffer: &mut [u8; RECORD_BYTES],
) -> Result<Option<Answer<'_>>, ProgramError> {
    let Some(written) = crate::host::web_read(request_id, buffer)? else {
        return Ok(None);
    };
    let written = usize::try_from(written)
        .map_err(|_| ProgramError::value(Field::Buffer, Reason::Malformed))?;
    let record = buffer
        .get(..written)
        .ok_or(ProgramError::value(Field::Buffer, Reason::Malformed))?;
    Answer::from_record(record).map(Some)
}

#[cfg(test)]
mod tests {
    use super::{Answer, ANSWER_HEADER_BYTES, MAX_RESPONSE_BYTES, RECORD_BYTES};
    use crate::error::{Field, ProgramError, Reason};

    fn record(digest: u8, full_length: u32, response: &[u8]) -> std::vec::Vec<u8> {
        let mut bytes = std::vec![digest; 32];
        bytes.extend_from_slice(&full_length.to_le_bytes());
        bytes.extend_from_slice(
            &u32::try_from(response.len())
                .unwrap_or_else(|_| panic!("response length"))
                .to_le_bytes(),
        );
        bytes.extend_from_slice(response);
        bytes
    }

    #[test]
    fn decodes_the_exact_host_record() {
        let bytes = record(0x5a, 16, b"Paxeer X Network");
        let answer = Answer::from_record(&bytes).unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(answer.content_digest, [0x5a; 32]);
        assert_eq!(answer.full_length, 16);
        assert_eq!(answer.response, b"Paxeer X Network");
        assert!(!answer.is_truncated());
        assert_eq!(RECORD_BYTES, ANSWER_HEADER_BYTES + MAX_RESPONSE_BYTES);
    }

    #[test]
    fn a_prefix_of_a_longer_body_reports_truncation() {
        let bytes = record(1, 9_000, b"prefix");
        let answer = Answer::from_record(&bytes).unwrap_or_else(|error| panic!("{error:?}"));
        assert!(answer.is_truncated());
    }

    #[test]
    fn malformed_records_are_refused() {
        let malformed = Err(ProgramError::value(Field::Buffer, Reason::Malformed));
        assert_eq!(
            Answer::from_record(&[0u8; ANSWER_HEADER_BYTES - 1]),
            malformed
        );
        let mut short = record(2, 8, b"abcdefgh");
        short.pop();
        assert_eq!(Answer::from_record(&short), malformed);
        assert_eq!(Answer::from_record(&record(3, 2, b"abc")), malformed);
        let oversized = std::vec![0u8; MAX_RESPONSE_BYTES + 1];
        assert_eq!(
            Answer::from_record(&record(4, u32::MAX, &oversized)),
            malformed
        );
    }
}

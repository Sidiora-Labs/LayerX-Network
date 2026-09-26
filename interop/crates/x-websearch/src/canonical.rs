use sha3::{Digest as _, Keccak256};
use unicode_normalization::UnicodeNormalization as _;

/// The ASCII domain every canonical content encoding starts with.
pub const CONTENT_DOMAIN: &[u8] = b"PAXEERX_WEB_CONTENT_V1";

const WINDOWS_1252_HIGH: [Option<char>; 32] = [
    Some('\u{20ac}'),
    None,
    Some('\u{201a}'),
    Some('\u{0192}'),
    Some('\u{201e}'),
    Some('\u{2026}'),
    Some('\u{2020}'),
    Some('\u{2021}'),
    Some('\u{02c6}'),
    Some('\u{2030}'),
    Some('\u{0160}'),
    Some('\u{2039}'),
    Some('\u{0152}'),
    None,
    Some('\u{017d}'),
    None,
    None,
    Some('\u{2018}'),
    Some('\u{2019}'),
    Some('\u{201c}'),
    Some('\u{201d}'),
    Some('\u{2022}'),
    Some('\u{2013}'),
    Some('\u{2014}'),
    Some('\u{02dc}'),
    Some('\u{2122}'),
    Some('\u{0161}'),
    Some('\u{203a}'),
    Some('\u{0153}'),
    None,
    Some('\u{017e}'),
    Some('\u{0178}'),
];

/// What the canonical bytes describe: a fetched page, a search, or an api
/// answer, which is the canonical form of the selected fields or the raw
/// bounded body.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ContentKind {
    Fetch,
    Search,
    Api,
}

impl ContentKind {
    #[must_use]
    pub const fn byte(self) -> u8 {
        match self {
            Self::Fetch => 1,
            Self::Search => 2,
            Self::Api => 3,
        }
    }

    #[must_use]
    pub const fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            1 => Some(Self::Fetch),
            2 => Some(Self::Search),
            3 => Some(Self::Api),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CanonicalError {
    UnsupportedCharset,
    InvalidEncoding,
    InvalidMediaType,
    TooLong,
    Malformed,
}

impl CanonicalError {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::UnsupportedCharset => "unsupported_charset",
            Self::InvalidEncoding => "invalid_encoding",
            Self::InvalidMediaType => "invalid_media_type",
            Self::TooLong => "content_too_long",
            Self::Malformed => "malformed_content",
        }
    }
}

impl std::fmt::Display for CanonicalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}

impl std::error::Error for CanonicalError {}

/// Decodes a body to UTF-8 from its declared charset. No charset means
/// UTF-8; a leading UTF-8 byte order mark is dropped.
///
/// # Errors
/// Refuses a charset other than UTF-8, US-ASCII, ISO-8859-1 and
/// windows-1252, and every byte sequence invalid in the declared charset.
pub fn decode(bytes: &[u8], charset: Option<&str>) -> Result<String, CanonicalError> {
    let label = charset.map_or_else(
        || "utf-8".to_owned(),
        |label| label.trim().trim_matches('"').to_ascii_lowercase(),
    );
    match label.as_str() {
        "utf-8" | "utf8" | "unicode-1-1-utf-8" => {
            let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
            String::from_utf8(bytes.to_vec()).map_err(|_| CanonicalError::InvalidEncoding)
        }
        "us-ascii" | "ascii" => {
            if bytes.is_ascii() {
                Ok(bytes.iter().map(|byte| char::from(*byte)).collect())
            } else {
                Err(CanonicalError::InvalidEncoding)
            }
        }
        "iso-8859-1" | "iso8859-1" | "iso_8859-1" | "latin1" | "l1" => {
            Ok(bytes.iter().map(|byte| char::from(*byte)).collect())
        }
        "windows-1252" | "cp1252" | "x-cp1252" => bytes
            .iter()
            .map(|byte| match byte {
                0x80..=0x9f => WINDOWS_1252_HIGH[usize::from(byte - 0x80)]
                    .ok_or(CanonicalError::InvalidEncoding),
                _ => Ok(char::from(*byte)),
            })
            .collect(),
        _ => Err(CanonicalError::UnsupportedCharset),
    }
}

/// Turns CRLF and CR into LF.
#[must_use]
pub fn unify_line_breaks(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Removes every control character other than LF and TAB.
#[must_use]
pub fn remove_controls(text: &str) -> String {
    text.chars()
        .filter(|character| matches!(character, '\n' | '\t') || !character.is_control())
        .collect()
}

/// Collapses each run of spaces and tabs to one space.
#[must_use]
pub fn collapse_spaces(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut in_run = false;
    for character in text.chars() {
        if matches!(character, ' ' | '\t') {
            if !in_run {
                result.push(' ');
            }
            in_run = true;
        } else {
            result.push(character);
            in_run = false;
        }
    }
    result
}

/// Removes trailing spaces from every line.
#[must_use]
pub fn trim_line_ends(text: &str) -> String {
    text.split('\n')
        .map(|line| line.trim_end_matches(' '))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Collapses three or more consecutive LF to two.
#[must_use]
pub fn collapse_blank_lines(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut breaks = 0_usize;
    for character in text.chars() {
        if character == '\n' {
            breaks += 1;
            if breaks > 2 {
                continue;
            }
        } else {
            breaks = 0;
        }
        result.push(character);
    }
    result
}

/// Applies the canonicalisation steps, in order, to decoded text.
#[must_use]
pub fn canonicalise(text: &str) -> String {
    let text = unify_line_breaks(text);
    let text = remove_controls(&text);
    let text = collapse_spaces(&text);
    let text = trim_line_ends(&text);
    let text = collapse_blank_lines(&text);
    text.trim().nfc().collect()
}

/// The media type without parameters, in lower case.
///
/// # Errors
/// Refuses anything that is not a `type/subtype` pair of tokens.
pub fn media_type_essence(media_type: &str) -> Result<String, CanonicalError> {
    let essence = media_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let valid = essence.split_once('/').is_some_and(|(kind, subtype)| {
        [kind, subtype].iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric()
                        || matches!(
                            byte,
                            b'!' | b'#' | b'$' | b'&' | b'-' | b'^' | b'_' | b'.' | b'+'
                        )
                })
        })
    });
    if valid {
        Ok(essence)
    } else {
        Err(CanonicalError::InvalidMediaType)
    }
}

/// The canonical bytes: the domain, the kind, the payload with a big-endian
/// uint32 length, the media type essence with a uint32 length and the text
/// with a big-endian uint64 length.
///
/// # Errors
/// Refuses a malformed media type and a payload or media type longer than a
/// uint32 length can carry.
pub fn canonical_bytes(
    kind: ContentKind,
    payload: &[u8],
    media_type: &str,
    text: &str,
) -> Result<Vec<u8>, CanonicalError> {
    encode(kind, payload, media_type, text.as_bytes())
}

/// The canonical bytes of an api answer: the same layout with the kind byte
/// 3 and the answer's bytes as they are, since a raw bounded body need not
/// be UTF-8.
///
/// # Errors
/// Refuses a malformed media type and a payload or media type longer than a
/// uint32 length can carry.
pub fn answer_canonical_bytes(
    payload: &[u8],
    media_type: &str,
    answer: &[u8],
) -> Result<Vec<u8>, CanonicalError> {
    encode(ContentKind::Api, payload, media_type, answer)
}

fn encode(
    kind: ContentKind,
    payload: &[u8],
    media_type: &str,
    text: &[u8],
) -> Result<Vec<u8>, CanonicalError> {
    let media_type = media_type_essence(media_type)?;
    let payload_length = u32::try_from(payload.len()).map_err(|_| CanonicalError::TooLong)?;
    let media_length = u32::try_from(media_type.len()).map_err(|_| CanonicalError::TooLong)?;
    let text_length = u64::try_from(text.len()).map_err(|_| CanonicalError::TooLong)?;
    let mut bytes = Vec::with_capacity(
        CONTENT_DOMAIN.len() + 1 + 4 + payload.len() + 4 + media_type.len() + 8 + text.len(),
    );
    bytes.extend_from_slice(CONTENT_DOMAIN);
    bytes.push(kind.byte());
    bytes.extend_from_slice(&payload_length.to_be_bytes());
    bytes.extend_from_slice(payload);
    bytes.extend_from_slice(&media_length.to_be_bytes());
    bytes.extend_from_slice(media_type.as_bytes());
    bytes.extend_from_slice(&text_length.to_be_bytes());
    bytes.extend_from_slice(text);
    Ok(bytes)
}

/// keccak256 of the canonical bytes.
#[must_use]
pub fn content_digest(canonical: &[u8]) -> [u8; 32] {
    Keccak256::digest(canonical).into()
}

/// The digest as 64 lower-case hexadecimal digits.
#[must_use]
pub fn digest_hex(digest: &[u8; 32]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(64);
    for byte in digest {
        text.push(char::from(DIGITS[usize::from(byte >> 4)]));
        text.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    text
}

/// Canonical bytes decoded back into their fields.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalContent {
    pub kind: ContentKind,
    pub payload: Vec<u8>,
    pub media_type: String,
    pub text: String,
}

impl CanonicalContent {
    /// # Errors
    /// Refuses bytes that are not exactly one canonical encoding with UTF-8
    /// text. An api answer whose raw body is not UTF-8 is accepted by
    /// [`check`] and refused here.
    pub fn parse(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let fields = Fields::split(bytes)?;
        let text =
            String::from_utf8(fields.text.to_vec()).map_err(|_| CanonicalError::Malformed)?;
        Ok(Self {
            kind: fields.kind,
            payload: fields.payload.to_vec(),
            media_type: fields.media_type,
            text,
        })
    }
}

/// Checks that bytes are exactly one canonical encoding and returns their
/// kind: the text of a fetch or a search is UTF-8, an api answer's bytes are
/// taken as they are.
///
/// # Errors
/// Refuses an unknown domain or kind, a length that does not match, a media
/// type that is not its own lower-case essence, and a fetch or search text
/// that is not UTF-8.
pub fn check(bytes: &[u8]) -> Result<ContentKind, CanonicalError> {
    let fields = Fields::split(bytes)?;
    if fields.kind != ContentKind::Api && std::str::from_utf8(fields.text).is_err() {
        return Err(CanonicalError::Malformed);
    }
    Ok(fields.kind)
}

/// The fields of one canonical encoding, the text still as bytes.
struct Fields<'a> {
    kind: ContentKind,
    payload: &'a [u8],
    media_type: String,
    text: &'a [u8],
}

impl<'a> Fields<'a> {
    fn split(bytes: &'a [u8]) -> Result<Self, CanonicalError> {
        let rest = bytes
            .strip_prefix(CONTENT_DOMAIN)
            .ok_or(CanonicalError::Malformed)?;
        let (&kind, rest) = rest.split_first().ok_or(CanonicalError::Malformed)?;
        let kind = ContentKind::from_byte(kind).ok_or(CanonicalError::Malformed)?;
        let (payload, rest) = take_u32_prefixed(rest)?;
        let (media_type, rest) = take_u32_prefixed(rest)?;
        let (length, rest) = rest
            .split_first_chunk::<8>()
            .ok_or(CanonicalError::Malformed)?;
        let length =
            usize::try_from(u64::from_be_bytes(*length)).map_err(|_| CanonicalError::Malformed)?;
        if rest.len() != length {
            return Err(CanonicalError::Malformed);
        }
        let media_type =
            String::from_utf8(media_type.to_vec()).map_err(|_| CanonicalError::Malformed)?;
        if media_type_essence(&media_type)? != media_type {
            return Err(CanonicalError::Malformed);
        }
        Ok(Self {
            kind,
            payload,
            media_type,
            text: rest,
        })
    }
}

fn take_u32_prefixed(bytes: &[u8]) -> Result<(&[u8], &[u8]), CanonicalError> {
    let (length, rest) = bytes
        .split_first_chunk::<4>()
        .ok_or(CanonicalError::Malformed)?;
    let length =
        usize::try_from(u32::from_be_bytes(*length)).map_err(|_| CanonicalError::Malformed)?;
    if rest.len() < length {
        return Err(CanonicalError::Malformed);
    }
    Ok(rest.split_at(length))
}

use std::cmp::Ordering;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::num::FpCategory;
use std::sync::Arc;
use std::time::{Duration, Instant};

use k256::ecdsa::{SigningKey, VerifyingKey};
use k256::elliptic_curve::sec1::ToEncodedPoint as _;
use k256::{NonZeroScalar, PublicKey};
use openssl::md::Md;
use openssl::pkey::Id;
use openssl::pkey_ctx::PkeyCtx;
use openssl::symm::{encrypt_aead, Cipher, Crypter, Mode};
use zeroize::Zeroizing;

use crate::attest::{address_of, signer_address, Level};
use crate::canonical::{self, CanonicalError, ContentKind};
use crate::fetch::{FetchError, Fetcher, Url};
use crate::watch::hex0x;

// The api request kind, as the sidecar performs it.
//
// The payload codec mirrors modules/xweb/types/api.go byte for byte and rule
// for rule, and testdata/api-vectors.json pins it:
//
//   uint8   version        1
//   uint8   method         1 GET, 2 POST
//   uint8   level          0 majority, 1 single
//   address attestor       20 bytes: the named attestor under single, zero under majority
//   uint16  urlLength      then urlLength bytes of URL
//   uint8   headerCount    then per header: uint16 nameLength, name, uint16 valueLength, value
//   uint16  bodyLength     then bodyLength bytes of body
//   uint8   pointerCount   then per pointer: uint16 length, the pointer's UTF-8 bytes
//   uint8   envelopeCount  then per envelope: uint16 length, the envelope
//
// A credential envelope is ECIES over secp256k1 with HKDF-SHA256 and
// AES-256-GCM, as modules/xweb/types/envelope.go seals it and
// testdata/envelope-vectors.json pins it:
//
//   envelope = attestor (20) || ephemeralKey (33) || nonce (12) || ciphertext || tag (16)
//   shared   = x of attestorPrivate * ephemeralPublic, 32 bytes big-endian
//   key      = HKDF-SHA256(ikm = shared, salt = ephemeralKey, info = ENVELOPE_INFO, length 32)
//   aad      = attestor (20) || "https://" and the URL's authority
//
// The opened credential lives only in zeroised memory for the one call. The
// attested answer is the RFC 8785 form of the JSON array of the values the
// pointers select, in pointer order, or the raw body when there is no
// pointer. Its canonical bytes follow the content layout of canonical.rs with
// the kind byte 3.

/// The request kind of an api call.
pub const KIND_API: u8 = ContentKind::Api.byte();

/// The first byte of every api payload.
pub const API_VERSION: u8 = 1;

pub const METHOD_GET: u8 = 1;
pub const METHOD_POST: u8 = 2;

/// Attestation levels: the threshold of attestors, or one named attestor.
pub const LEVEL_MAJORITY: u8 = 0;
pub const LEVEL_SINGLE: u8 = 1;

pub const MAX_API_URL_BYTES: usize = 2_048;
pub const MAX_API_HEADERS: usize = 16;
pub const MAX_API_HEADER_NAME_BYTES: usize = 64;
pub const MAX_API_HEADER_VALUE_BYTES: usize = 1_024;
pub const MAX_API_BODY_BYTES: usize = 4_096;
pub const MAX_API_POINTERS: usize = 16;
pub const MAX_API_POINTER_BYTES: usize = 256;

/// The most envelopes one payload carries: the attestor set bound.
pub const MAX_ENVELOPES: usize = 64;

pub const MAX_CREDENTIAL_HEADERS: usize = 8;
pub const MAX_CREDENTIAL_BYTES: usize = 1_024;

/// The HKDF info string of every envelope key.
pub const ENVELOPE_INFO: &[u8] = b"PAXEERX_WEB_API_ENVELOPE_V1";

pub const ENVELOPE_KEY_LENGTH: usize = 33;
pub const ENVELOPE_NONCE_LENGTH: usize = 12;
pub const ENVELOPE_TAG_LENGTH: usize = 16;
const ENVELOPE_HEADER: usize = 20 + ENVELOPE_KEY_LENGTH + ENVELOPE_NONCE_LENGTH;

/// An envelope's length minus its plaintext's.
pub const ENVELOPE_OVERHEAD: usize = ENVELOPE_HEADER + ENVELOPE_TAG_LENGTH;

/// The longest envelope.
pub const MAX_ENVELOPE_BYTES: usize = ENVELOPE_OVERHEAD + MAX_CREDENTIAL_BYTES;

/// The media type of an answer selected by pointers.
pub const ANSWER_MEDIA_TYPE: &str = "application/json";

/// The deepest JSON nesting an api response may have.
pub const MAX_JSON_DEPTH: usize = 128;

const HTTPS_PREFIX: &str = "https://";
const RESTRICTED_HEADERS: [&str; 8] = [
    "host",
    "content-length",
    "transfer-encoding",
    "connection",
    "keep-alive",
    "te",
    "trailer",
    "upgrade",
];
const MAX_RESPONSE_HEAD: usize = 65_536;
const MAX_CHUNK_LINE: usize = 1_024;
const READ_CHUNK: usize = 16_384;

/// Why an api request was refused. No variant carries a credential byte.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ApiError {
    /// The payload breaks a rule of the codec.
    Payload(String),
    /// The envelope addressed to this attestor does not open.
    Envelope(String),
    /// The opened credential breaks a rule of the credential encoding.
    Credential(String),
    /// The payload carries envelopes and none is addressed to this attestor.
    NoEnvelope,
    /// The call was refused on the fetch path's limits or destinations.
    Fetch(FetchError),
    /// The API answered with a status other than 2xx.
    Status(u16),
    /// Pointers were given and the response is not JSON.
    NotJson(String),
    /// A pointer selects nothing in the response.
    Pointer(String),
    /// The raw body carries no media type the content layout accepts.
    MediaType,
    /// The answer is larger than the body limit.
    TooLarge,
}

impl ApiError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Payload(_) => "invalid_api_payload",
            Self::Envelope(_) => "envelope_refused",
            Self::Credential(_) => "invalid_credential",
            Self::NoEnvelope => "no_envelope_for_attestor",
            Self::Fetch(error) => error.code(),
            Self::Status(_) => "upstream_status",
            Self::NotJson(_) => "response_not_json",
            Self::Pointer(_) => "pointer_unresolved",
            Self::MediaType => "unsupported_media_type",
            Self::TooLarge => "answer_too_large",
        }
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Payload(message) => write!(f, "invalid api payload: {message}"),
            Self::Envelope(message) => write!(f, "credential envelope refused: {message}"),
            Self::Credential(message) => write!(f, "credential refused: {message}"),
            Self::NoEnvelope => f.write_str("no credential envelope is addressed to this attestor"),
            Self::Fetch(error) => write!(f, "api call refused: {error}"),
            Self::Status(status) => write!(f, "api answered {status}"),
            Self::NotJson(message) => write!(f, "api response is not JSON: {message}"),
            Self::Pointer(pointer) => write!(f, "pointer {pointer:?} selects nothing"),
            Self::MediaType => f.write_str("api response carries no usable media type"),
            Self::TooLarge => f.write_str("api answer is larger than the body limit"),
        }
    }
}

impl std::error::Error for ApiError {}

/// One public HTTP request header.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApiHeader {
    pub name: String,
    pub value: String,
}

impl ApiHeader {
    #[must_use]
    pub fn new(name: &str, value: &str) -> Self {
        Self {
            name: name.to_owned(),
            value: value.to_owned(),
        }
    }
}

/// One credential envelope; the ciphertext carries the GCM tag.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Envelope {
    pub attestor: [u8; 20],
    pub ephemeral_key: [u8; ENVELOPE_KEY_LENGTH],
    pub nonce: [u8; ENVELOPE_NONCE_LENGTH],
    pub ciphertext: Vec<u8>,
}

impl Envelope {
    /// The envelope's wire form.
    #[must_use]
    pub fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(ENVELOPE_HEADER + self.ciphertext.len());
        out.extend_from_slice(&self.attestor);
        out.extend_from_slice(&self.ephemeral_key);
        out.extend_from_slice(&self.nonce);
        out.extend_from_slice(&self.ciphertext);
        out
    }

    fn validate(&self) -> Result<(), String> {
        if self.attestor == [0; 20] {
            return Err("addressed to the zero attestor".to_owned());
        }
        let length = ENVELOPE_HEADER + self.ciphertext.len();
        if !(ENVELOPE_OVERHEAD + 1..=MAX_ENVELOPE_BYTES).contains(&length) {
            return Err(envelope_length(length));
        }
        PublicKey::from_sec1_bytes(&self.ephemeral_key)
            .map_err(|_| "ephemeral key: not a compressed secp256k1 point".to_owned())?;
        Ok(())
    }

    /// Splits an envelope into its parts and checks its length, its
    /// attestor and that its ephemeral key is a compressed secp256k1 point.
    ///
    /// # Errors
    /// Returns the rule the envelope breaks.
    pub fn parse(raw: &[u8]) -> Result<Self, String> {
        if !(ENVELOPE_OVERHEAD + 1..=MAX_ENVELOPE_BYTES).contains(&raw.len()) {
            return Err(envelope_length(raw.len()));
        }
        let mut envelope = Self {
            attestor: [0; 20],
            ephemeral_key: [0; ENVELOPE_KEY_LENGTH],
            nonce: [0; ENVELOPE_NONCE_LENGTH],
            ciphertext: raw[ENVELOPE_HEADER..].to_vec(),
        };
        envelope.attestor.copy_from_slice(&raw[..20]);
        envelope
            .ephemeral_key
            .copy_from_slice(&raw[20..20 + ENVELOPE_KEY_LENGTH]);
        envelope
            .nonce
            .copy_from_slice(&raw[20 + ENVELOPE_KEY_LENGTH..ENVELOPE_HEADER]);
        envelope.validate()?;
        Ok(envelope)
    }

    /// Opens the envelope with the attestor key for an origin, into memory
    /// that is zeroised when dropped.
    ///
    /// # Errors
    /// Refuses an envelope addressed to another attestor and one that does
    /// not authenticate.
    pub fn open(&self, key: &SigningKey, origin: &str) -> Result<Zeroizing<Vec<u8>>, ApiError> {
        let own = signer_address(key);
        if self.attestor != own {
            return Err(ApiError::Envelope(format!(
                "addressed to {}, this key is {}",
                hex0x(&self.attestor),
                hex0x(&own)
            )));
        }
        let ephemeral = PublicKey::from_sec1_bytes(&self.ephemeral_key)
            .map_err(|_| ApiError::Envelope("ephemeral key is not on secp256k1".to_owned()))?;
        let shared = envelope_shared_x(key.as_nonzero_scalar(), &ephemeral)?;
        let aes_key = envelope_key(&shared[..], &self.ephemeral_key)?;
        let refused = || ApiError::Envelope(format!("does not authenticate for {origin}"));
        let split = self
            .ciphertext
            .len()
            .checked_sub(ENVELOPE_TAG_LENGTH)
            .ok_or_else(refused)?;
        let (sealed, tag) = self.ciphertext.split_at(split);
        let cipher = Cipher::aes_256_gcm();
        let mut crypter = Crypter::new(cipher, Mode::Decrypt, &aes_key[..], Some(&self.nonce))
            .map_err(|_| refused())?;
        crypter
            .aad_update(&envelope_aad(&self.attestor, origin))
            .map_err(|_| refused())?;
        let mut plaintext = Zeroizing::new(vec![0_u8; sealed.len() + cipher.block_size()]);
        let mut count = crypter
            .update(sealed, &mut plaintext[..])
            .map_err(|_| refused())?;
        crypter.set_tag(tag).map_err(|_| refused())?;
        count += crypter
            .finalize(&mut plaintext[count..])
            .map_err(|_| refused())?;
        plaintext.truncate(count);
        Ok(plaintext)
    }
}

fn envelope_length(length: usize) -> String {
    format!(
        "{length} bytes, want {} to {MAX_ENVELOPE_BYTES}",
        ENVELOPE_OVERHEAD + 1
    )
}

/// The GCM associated data: the attestor then the origin.
#[must_use]
pub fn envelope_aad(attestor: &[u8; 20], origin: &str) -> Vec<u8> {
    let mut aad = Vec::with_capacity(20 + origin.len());
    aad.extend_from_slice(attestor);
    aad.extend_from_slice(origin.as_bytes());
    aad
}

/// The 32-byte x coordinate of `secret * public`.
///
/// # Errors
/// Refuses a product that is the identity.
pub fn envelope_shared_x(
    secret: &NonZeroScalar,
    public: &PublicKey,
) -> Result<Zeroizing<[u8; 32]>, ApiError> {
    let point = (public.to_projective() * *secret.as_ref()).to_affine();
    let encoded = point.to_encoded_point(false);
    let x = encoded
        .x()
        .ok_or_else(|| ApiError::Envelope("shared point is the identity".to_owned()))?;
    let mut shared = Zeroizing::new([0_u8; 32]);
    shared.copy_from_slice(x);
    Ok(shared)
}

/// HKDF-SHA256 of the shared x coordinate, salted with the compressed
/// ephemeral key, under [`ENVELOPE_INFO`]: the 32-byte AES-256 key.
///
/// # Errors
/// Returns a derivation the cryptographic library refuses.
pub fn envelope_key(
    shared_x: &[u8],
    ephemeral_key: &[u8],
) -> Result<Zeroizing<[u8; 32]>, ApiError> {
    let failed = |_| ApiError::Envelope("key derivation failed".to_owned());
    let mut context = PkeyCtx::new_id(Id::HKDF).map_err(failed)?;
    context.derive_init().map_err(failed)?;
    context.set_hkdf_md(Md::sha256()).map_err(failed)?;
    context.set_hkdf_key(shared_x).map_err(failed)?;
    context.set_hkdf_salt(ephemeral_key).map_err(failed)?;
    context.add_hkdf_info(ENVELOPE_INFO).map_err(failed)?;
    let mut key = Zeroizing::new([0_u8; 32]);
    let length = context.derive(Some(&mut key[..])).map_err(failed)?;
    if length != 32 {
        return Err(ApiError::Envelope("key derivation is short".to_owned()));
    }
    Ok(key)
}

/// Seals a credential plaintext to an attestor's public key for an origin
/// with the given ephemeral key and nonce, which must never be used twice.
///
/// # Errors
/// Refuses an empty or oversized plaintext and returns a sealing failure.
pub fn seal_envelope_with(
    recipient: &PublicKey,
    ephemeral: &SigningKey,
    nonce: [u8; ENVELOPE_NONCE_LENGTH],
    origin: &str,
    plaintext: &[u8],
) -> Result<Vec<u8>, ApiError> {
    if plaintext.is_empty() || plaintext.len() > MAX_CREDENTIAL_BYTES {
        return Err(ApiError::Envelope(format!(
            "plaintext is {} bytes, want 1 to {MAX_CREDENTIAL_BYTES}",
            plaintext.len()
        )));
    }
    let shared = envelope_shared_x(ephemeral.as_nonzero_scalar(), recipient)?;
    let attestor = address_of(&VerifyingKey::from(recipient));
    let compressed = ephemeral.verifying_key().to_encoded_point(true);
    let aes_key = envelope_key(&shared[..], compressed.as_bytes())?;
    let mut tag = [0_u8; ENVELOPE_TAG_LENGTH];
    let sealed = encrypt_aead(
        Cipher::aes_256_gcm(),
        &aes_key[..],
        Some(&nonce),
        &envelope_aad(&attestor, origin),
        plaintext,
        &mut tag,
    )
    .map_err(|_| ApiError::Envelope("sealing failed".to_owned()))?;
    let mut envelope = Vec::with_capacity(ENVELOPE_OVERHEAD + sealed.len());
    envelope.extend_from_slice(&attestor);
    envelope.extend_from_slice(compressed.as_bytes());
    envelope.extend_from_slice(&nonce);
    envelope.extend_from_slice(&sealed);
    envelope.extend_from_slice(&tag);
    Ok(envelope)
}

/// Parses an envelope and opens it with the attestor key for an origin.
///
/// # Errors
/// Refuses a malformed envelope, one addressed to another attestor and one
/// that does not authenticate.
pub fn open_envelope(
    raw: &[u8],
    key: &SigningKey,
    origin: &str,
) -> Result<Zeroizing<Vec<u8>>, ApiError> {
    Envelope::parse(raw)
        .map_err(ApiError::Envelope)?
        .open(key, origin)
}

#[derive(Clone, Copy)]
enum Part {
    Public,
    Credential,
}

impl Part {
    const fn what(self) -> &'static str {
        match self {
            Self::Public => "header",
            Self::Credential => "credential header",
        }
    }
}

const fn token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Checks header names, values, repeats and the count. A credential value's
/// bytes never enter a message.
/// A header name and value as payload bytes.
type RawHeader<'a> = (&'a [u8], &'a [u8]);

fn validate_headers(part: Part, headers: &[RawHeader<'_>], bound: usize) -> Result<(), String> {
    let what = part.what();
    if headers.len() > bound {
        return Err(format!("{} {what}s, bound {bound}", headers.len()));
    }
    let mut seen: Vec<String> = Vec::with_capacity(headers.len());
    for (index, (name, value)) in headers.iter().enumerate() {
        if name.is_empty() || name.len() > MAX_API_HEADER_NAME_BYTES {
            return Err(format!(
                "{what} {index} name is {} bytes, want 1 to {MAX_API_HEADER_NAME_BYTES}",
                name.len()
            ));
        }
        if let Some(byte) = name.iter().find(|byte| !token_byte(**byte)) {
            return Err(format!(
                "{what} {index} name {:?} holds {:?}, not a token character",
                lossy(name),
                char::from(*byte)
            ));
        }
        let display = lossy(name);
        let lower = display.to_ascii_lowercase();
        if RESTRICTED_HEADERS.contains(&lower.as_str()) {
            return Err(format!(
                "{what} {index} is {display}, which the sidecar sets itself"
            ));
        }
        if seen.contains(&lower) {
            return Err(format!("{what} {index} repeats {display}"));
        }
        seen.push(lower);
        if value.len() > MAX_API_HEADER_VALUE_BYTES {
            return Err(format!(
                "{what} {display} value is {} bytes, bound {MAX_API_HEADER_VALUE_BYTES}",
                value.len()
            ));
        }
        if let Some(at) = value
            .iter()
            .position(|byte| *byte != b'\t' && !(0x20..=0x7e).contains(byte))
        {
            return Err(match part {
                Part::Public => format!("{what} {display} value byte {at} is 0x{:02x}", value[at]),
                Part::Credential => format!("{what} {display} value byte {at} is not printable"),
            });
        }
    }
    Ok(())
}

fn validate_pointers(pointers: &[&[u8]]) -> Result<(), String> {
    if pointers.len() > MAX_API_POINTERS {
        return Err(format!(
            "{} pointers, bound {MAX_API_POINTERS}",
            pointers.len()
        ));
    }
    let mut seen: Vec<&[u8]> = Vec::with_capacity(pointers.len());
    for (index, pointer) in pointers.iter().enumerate() {
        if pointer.len() > MAX_API_POINTER_BYTES {
            return Err(format!(
                "pointer {index} is {} bytes, bound {MAX_API_POINTER_BYTES}",
                pointer.len()
            ));
        }
        let Ok(text) = std::str::from_utf8(pointer) else {
            return Err(format!("pointer {index} is not UTF-8"));
        };
        if !text.is_empty() && !text.starts_with('/') {
            return Err(format!("pointer {text:?} does not start with /"));
        }
        let bytes = text.as_bytes();
        let bad_escape = bytes
            .iter()
            .enumerate()
            .any(|(at, byte)| *byte == b'~' && !matches!(bytes.get(at + 1), Some(b'0' | b'1')));
        if bad_escape {
            return Err(format!("pointer {text:?} has a ~ not followed by 0 or 1"));
        }
        if seen.contains(pointer) {
            return Err(format!("pointer {text:?} is repeated"));
        }
        seen.push(pointer);
    }
    Ok(())
}

fn valid_optional_port(port: &str) -> bool {
    port.is_empty()
        || port
            .strip_prefix(':')
            .is_some_and(|digits| digits.bytes().all(|byte| byte.is_ascii_digit()))
}

/// The host name of an authority as a URL parser reads it, or why the
/// authority does not parse.
fn authority_host(authority: &str) -> Result<&str, String> {
    if let Some(inner) = authority.strip_prefix('[') {
        let close = authority
            .rfind(']')
            .ok_or_else(|| "missing ']' in host".to_owned())?;
        let port = &authority[close + 1..];
        if !valid_optional_port(port) {
            return Err(format!("invalid port {port:?} after host"));
        }
        return Ok(&inner[..close - 1]);
    }
    match authority.rfind(':') {
        Some(colon) if valid_optional_port(&authority[colon..]) => Ok(&authority[..colon]),
        Some(colon) => Err(format!("invalid port {:?} after host", &authority[colon..])),
        None => Ok(authority),
    }
}

/// `https://` and the URL's authority, or the rule the URL breaks.
fn api_origin(raw: &[u8]) -> Result<String, String> {
    if raw.is_empty() || raw.len() > MAX_API_URL_BYTES {
        return Err(format!(
            "url is {} bytes, want 1 to {MAX_API_URL_BYTES}",
            raw.len()
        ));
    }
    if let Some(at) = raw.iter().position(|byte| !(0x21..=0x7e).contains(byte)) {
        return Err(format!(
            "url byte {at} is 0x{:02x}, not printable ASCII",
            raw[at]
        ));
    }
    let text = lossy(raw);
    if text.contains('#') {
        return Err("url carries a fragment".to_owned());
    }
    let Some(rest) = text.strip_prefix(HTTPS_PREFIX) else {
        return Err(format!("url {text:?} is not https"));
    };
    let authority = &rest[..rest.find(['/', '?']).unwrap_or(rest.len())];
    if authority.is_empty() {
        return Err(format!("url {text:?} has no host"));
    }
    if let Some(byte) = authority.bytes().find(|byte| {
        !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || b".-:[]".contains(byte))
    }) {
        return Err(format!(
            "url authority {authority:?} holds {:?}: only lower-case letters, digits and .-:[] are accepted",
            char::from(byte)
        ));
    }
    let host = authority_host(authority).map_err(|error| format!("url {text:?}: {error}"))?;
    if host.is_empty() {
        return Err(format!("url {text:?} has no host"));
    }
    Ok(format!("{HTTPS_PREFIX}{authority}"))
}

fn method_name(method: u8) -> Result<&'static str, String> {
    match method {
        METHOD_GET => Ok("GET"),
        METHOD_POST => Ok("POST"),
        other => Err(format!("method {other}, want 1 (GET) or 2 (POST)")),
    }
}

fn validate_level(level: u8, attestor: &[u8; 20]) -> Result<(), String> {
    match level {
        LEVEL_MAJORITY if *attestor != [0; 20] => {
            Err(format!("majority level names attestor {}", hex0x(attestor)))
        }
        LEVEL_SINGLE if *attestor == [0; 20] => Err("single level names no attestor".to_owned()),
        LEVEL_MAJORITY | LEVEL_SINGLE => Ok(()),
        other => Err(format!("level {other}")),
    }
}

/// The payload's fields as bytes, checked in the order the Go codec checks
/// them.
struct Parts<'a> {
    method: u8,
    level: u8,
    attestor: [u8; 20],
    url: &'a [u8],
    headers: Vec<RawHeader<'a>>,
    body: &'a [u8],
    pointers: Vec<&'a [u8]>,
    envelopes: &'a [Envelope],
}

impl Parts<'_> {
    fn validate(&self) -> Result<(), String> {
        method_name(self.method)?;
        validate_level(self.level, &self.attestor)?;
        api_origin(self.url)?;
        validate_headers(Part::Public, &self.headers, MAX_API_HEADERS)?;
        if self.body.len() > MAX_API_BODY_BYTES {
            return Err(format!(
                "body is {} bytes, bound {MAX_API_BODY_BYTES}",
                self.body.len()
            ));
        }
        if self.method == METHOD_GET && !self.body.is_empty() {
            return Err(format!("GET carries a {}-byte body", self.body.len()));
        }
        validate_pointers(&self.pointers)?;
        if self.envelopes.len() > MAX_ENVELOPES {
            return Err(format!(
                "{} envelopes, bound {MAX_ENVELOPES}",
                self.envelopes.len()
            ));
        }
        let mut addressed: Vec<[u8; 20]> = Vec::with_capacity(self.envelopes.len());
        for (index, envelope) in self.envelopes.iter().enumerate() {
            envelope
                .validate()
                .map_err(|error| format!("envelope {index}: {error}"))?;
            if addressed.contains(&envelope.attestor) {
                return Err(format!(
                    "envelope {index} repeats attestor {}",
                    hex0x(&envelope.attestor)
                ));
            }
            addressed.push(envelope.attestor);
            if self.level == LEVEL_SINGLE && envelope.attestor != self.attestor {
                return Err(format!(
                    "envelope {index} is addressed to {}, the single level names {}",
                    hex0x(&envelope.attestor),
                    hex0x(&self.attestor)
                ));
            }
        }
        Ok(())
    }
}

struct Writer {
    out: Vec<u8>,
}

impl Writer {
    fn count(&mut self, what: &str, count: usize) -> Result<(), String> {
        let byte = u8::try_from(count)
            .map_err(|_| format!("{what} count {count} does not fit one byte"))?;
        self.out.push(byte);
        Ok(())
    }

    fn field(&mut self, name: &str, data: &[u8]) -> Result<(), String> {
        let length = u16::try_from(data.len())
            .map_err(|_| format!("{name} is {} bytes, over the uint16 length", data.len()))?;
        self.out.extend_from_slice(&length.to_be_bytes());
        self.out.extend_from_slice(data);
        Ok(())
    }
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, field: &str, count: usize) -> Result<&'a [u8], String> {
        let end = self
            .at
            .checked_add(count)
            .filter(|end| *end <= self.data.len())
            .ok_or_else(|| format!("payload ends inside {field} at byte {}", self.at))?;
        let taken = &self.data[self.at..end];
        self.at = end;
        Ok(taken)
    }

    fn count(&mut self, field: &str) -> Result<usize, String> {
        Ok(usize::from(self.take(field, 1)?[0]))
    }

    fn field(&mut self, name: &str) -> Result<&'a [u8], String> {
        let length = self.take(&format!("{name} length"), 2)?;
        let length = usize::from(u16::from_be_bytes([length[0], length[1]]));
        self.take(name, length)
    }

    fn headers(&mut self, what: &str) -> Result<Vec<RawHeader<'a>>, String> {
        let count = self.count(&format!("{what} count"))?;
        let mut headers = Vec::with_capacity(count);
        for index in 0..count {
            let name = self.field(&format!("{what} {index} name"))?;
            let value = self.field(&format!("{what} {index} value"))?;
            headers.push((name, value));
        }
        Ok(headers)
    }

    const fn rest(&self) -> usize {
        self.data.len() - self.at
    }
}

fn text_of(bytes: &[u8], field: &str) -> Result<String, ApiError> {
    String::from_utf8(bytes.to_vec())
        .map_err(|_| ApiError::Payload(format!("{field} is not UTF-8")))
}

/// The decoded payload of an api request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApiPayload {
    pub method: u8,
    pub level: u8,
    pub attestor: [u8; 20],
    pub url: String,
    pub headers: Vec<ApiHeader>,
    pub body: Vec<u8>,
    pub pointers: Vec<String>,
    pub envelopes: Vec<Envelope>,
}

impl ApiPayload {
    fn parts(&self) -> Parts<'_> {
        Parts {
            method: self.method,
            level: self.level,
            attestor: self.attestor,
            url: self.url.as_bytes(),
            headers: self
                .headers
                .iter()
                .map(|header| (header.name.as_bytes(), header.value.as_bytes()))
                .collect(),
            body: &self.body,
            pointers: self.pointers.iter().map(String::as_bytes).collect(),
            envelopes: &self.envelopes,
        }
    }

    /// Checks every rule of the codec that needs no chain state.
    ///
    /// # Errors
    /// Returns the first rule the payload breaks, in the Go codec's order.
    pub fn validate(&self) -> Result<(), ApiError> {
        self.parts().validate().map_err(ApiError::Payload)
    }

    /// `https://` and the URL's authority: what every envelope of the
    /// payload is bound to.
    ///
    /// # Errors
    /// Returns the rule the URL breaks.
    pub fn origin(&self) -> Result<String, ApiError> {
        api_origin(self.url.as_bytes()).map_err(ApiError::Payload)
    }

    /// `GET` or `POST`.
    ///
    /// # Errors
    /// Refuses a method other than 1 and 2.
    pub fn method_name(&self) -> Result<&'static str, ApiError> {
        method_name(self.method).map_err(ApiError::Payload)
    }

    /// The level the answer is attested under.
    #[must_use]
    pub const fn attestation_level(&self) -> Level {
        if self.level == LEVEL_SINGLE {
            Level::Single(self.attestor)
        } else {
            Level::Majority
        }
    }

    /// The envelope addressed to an attestor.
    #[must_use]
    pub fn envelope_for(&self, attestor: &[u8; 20]) -> Option<&Envelope> {
        self.envelopes
            .iter()
            .find(|envelope| envelope.attestor == *attestor)
    }

    /// Validates the payload and returns its bytes.
    ///
    /// # Errors
    /// Returns the first rule the payload breaks.
    pub fn encode(&self) -> Result<Vec<u8>, ApiError> {
        self.validate()?;
        let mut writer = Writer {
            out: vec![API_VERSION, self.method, self.level],
        };
        writer.out.extend_from_slice(&self.attestor);
        let written = (|| {
            writer.field("url", self.url.as_bytes())?;
            writer.count("headers", self.headers.len())?;
            for header in &self.headers {
                writer.field("header name", header.name.as_bytes())?;
                writer.field("header value", header.value.as_bytes())?;
            }
            writer.field("body", &self.body)?;
            writer.count("pointers", self.pointers.len())?;
            for pointer in &self.pointers {
                writer.field("pointer", pointer.as_bytes())?;
            }
            writer.count("envelopes", self.envelopes.len())?;
            for envelope in &self.envelopes {
                writer.field("envelope", &envelope.bytes())?;
            }
            Ok::<(), String>(())
        })();
        written.map_err(ApiError::Payload)?;
        Ok(writer.out)
    }

    /// Decodes and validates an api payload; nothing may follow the last
    /// envelope.
    ///
    /// # Errors
    /// Returns the first rule the bytes break, in the Go codec's order.
    pub fn decode(raw: &[u8]) -> Result<Self, ApiError> {
        let mut reader = Reader { data: raw, at: 0 };
        let parsed = (|| {
            let head = reader.take("the version, method, level and attestor", 3 + 20)?;
            if head[0] != API_VERSION {
                return Err(format!("version {}, want {API_VERSION}", head[0]));
            }
            let mut attestor = [0; 20];
            attestor.copy_from_slice(&head[3..23]);
            let url = reader.field("url")?;
            let headers = reader.headers("header")?;
            let body = reader.field("body")?;
            let count = reader.count("pointer count")?;
            let mut pointers = Vec::with_capacity(count);
            for index in 0..count {
                pointers.push(reader.field(&format!("pointer {index}"))?);
            }
            let count = reader.count("envelope count")?;
            let mut envelopes = Vec::with_capacity(count);
            for index in 0..count {
                let envelope = reader.field(&format!("envelope {index}"))?;
                envelopes.push(
                    Envelope::parse(envelope)
                        .map_err(|error| format!("envelope {index}: {error}"))?,
                );
            }
            if reader.rest() != 0 {
                return Err(format!("{} bytes follow the last envelope", reader.rest()));
            }
            Ok((head, attestor, url, headers, body, pointers, envelopes))
        })();
        let (head, attestor, url, headers, body, pointers, envelopes) =
            parsed.map_err(ApiError::Payload)?;
        Parts {
            method: head[1],
            level: head[2],
            attestor,
            url,
            headers: headers.clone(),
            body,
            pointers: pointers.clone(),
            envelopes: &envelopes,
        }
        .validate()
        .map_err(ApiError::Payload)?;
        Ok(Self {
            method: head[1],
            level: head[2],
            attestor,
            url: text_of(url, "url")?,
            headers: headers
                .iter()
                .map(|(name, value)| {
                    Ok(ApiHeader {
                        name: text_of(name, "header name")?,
                        value: text_of(value, "header value")?,
                    })
                })
                .collect::<Result<_, ApiError>>()?,
            body: body.to_vec(),
            pointers: pointers
                .iter()
                .map(|pointer| text_of(pointer, "pointer"))
                .collect::<Result<_, ApiError>>()?,
            envelopes,
        })
    }
}

/// An opened credential: header names and values held in memory that is
/// zeroised when dropped. It has no `Debug` and no `Display`, so it never
/// reaches a log line.
pub struct Credential {
    headers: Vec<(String, Zeroizing<String>)>,
}

impl Credential {
    /// The credential plaintext for headers: uint8 count, then per header
    /// uint16 nameLength, name, uint16 valueLength, value.
    ///
    /// # Errors
    /// Refuses no header, a header rule broken and more than
    /// [`MAX_CREDENTIAL_BYTES`].
    pub fn encode(headers: &[ApiHeader]) -> Result<Zeroizing<Vec<u8>>, ApiError> {
        if headers.is_empty() {
            return Err(ApiError::Credential(
                "credential carries no header".to_owned(),
            ));
        }
        let pairs: Vec<RawHeader<'_>> = headers
            .iter()
            .map(|header| (header.name.as_bytes(), header.value.as_bytes()))
            .collect();
        validate_headers(Part::Credential, &pairs, MAX_CREDENTIAL_HEADERS)
            .map_err(ApiError::Credential)?;
        let length = 1 + pairs
            .iter()
            .map(|(name, value)| 4 + name.len() + value.len())
            .sum::<usize>();
        if length > MAX_CREDENTIAL_BYTES {
            return Err(ApiError::Credential(format!(
                "credential is {length} bytes, bound {MAX_CREDENTIAL_BYTES}"
            )));
        }
        let mut writer = Writer {
            out: Vec::with_capacity(length),
        };
        let written = (|| {
            writer.count("credential headers", pairs.len())?;
            for (name, value) in &pairs {
                writer.field("credential header name", name)?;
                writer.field("credential header value", value)?;
            }
            Ok::<(), String>(())
        })();
        let out = Zeroizing::new(writer.out);
        written.map_err(ApiError::Credential)?;
        Ok(out)
    }

    /// Decodes an opened envelope's plaintext, refusing a credential header
    /// whose name repeats one of the payload's public headers.
    ///
    /// # Errors
    /// Returns the rule the plaintext breaks; no message carries a value.
    pub fn decode(plaintext: &[u8], public: &[ApiHeader]) -> Result<Self, ApiError> {
        let refuse = |message: String| ApiError::Credential(message);
        if plaintext.len() > MAX_CREDENTIAL_BYTES {
            return Err(refuse(format!(
                "credential is {} bytes, bound {MAX_CREDENTIAL_BYTES}",
                plaintext.len()
            )));
        }
        let mut reader = Reader {
            data: plaintext,
            at: 0,
        };
        let pairs = reader.headers("credential header").map_err(refuse)?;
        if reader.rest() != 0 {
            return Err(refuse(format!(
                "{} bytes follow the last credential header",
                reader.rest()
            )));
        }
        if pairs.is_empty() {
            return Err(refuse("credential carries no header".to_owned()));
        }
        validate_headers(Part::Credential, &pairs, MAX_CREDENTIAL_HEADERS).map_err(refuse)?;
        let mut headers = Vec::with_capacity(pairs.len());
        for (name, value) in pairs {
            let name = lossy(name);
            if public
                .iter()
                .any(|other| other.name.eq_ignore_ascii_case(&name))
            {
                return Err(refuse(format!(
                    "credential header {name} repeats a public header"
                )));
            }
            let mut text = Zeroizing::new(String::with_capacity(value.len()));
            text.extend(value.iter().map(|byte| char::from(*byte)));
            headers.push((name, text));
        }
        Ok(Self { headers })
    }

    /// The header names the credential carries.
    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        self.headers.iter().map(|(name, _)| name.as_str()).collect()
    }
}

/// A JSON value as RFC 8259 defines it, with members kept in document order.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

struct JsonParser<'a> {
    text: &'a str,
    at: usize,
}

impl JsonParser<'_> {
    fn error(&self, message: &str) -> ApiError {
        ApiError::NotJson(format!("byte {}: {message}", self.at))
    }

    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.at).copied()
    }

    fn skip_space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), ApiError> {
        if self.peek() == Some(byte) {
            self.at += 1;
            Ok(())
        } else {
            Err(self.error(&format!("expected {:?}", char::from(byte))))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, ApiError> {
        if depth > MAX_JSON_DEPTH {
            return Err(self.error("nesting deeper than the bound"));
        }
        match self.peek() {
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => self.string().map(Json::String),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'n') => self.literal("null", Json::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(self.error("expected a value")),
        }
    }

    fn literal(&mut self, word: &str, value: Json) -> Result<Json, ApiError> {
        if self.text[self.at..].starts_with(word) {
            self.at += word.len();
            Ok(value)
        } else {
            Err(self.error("expected a value"))
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, ApiError> {
        self.at += 1;
        self.skip_space();
        let mut members: Vec<(String, Json)> = Vec::new();
        if self.peek() == Some(b'}') {
            self.at += 1;
            return Ok(Json::Object(members));
        }
        loop {
            if self.peek() != Some(b'"') {
                return Err(self.error("expected a member name"));
            }
            let name = self.string()?;
            if members.iter().any(|(existing, _)| *existing == name) {
                return Err(self.error("duplicate member name"));
            }
            self.skip_space();
            self.expect(b':')?;
            self.skip_space();
            let value = self.value(depth + 1)?;
            members.push((name, value));
            self.skip_space();
            match self.peek() {
                Some(b',') => {
                    self.at += 1;
                    self.skip_space();
                }
                Some(b'}') => {
                    self.at += 1;
                    return Ok(Json::Object(members));
                }
                _ => return Err(self.error("expected ',' or '}'")),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Json, ApiError> {
        self.at += 1;
        self.skip_space();
        let mut items = Vec::new();
        if self.peek() == Some(b']') {
            self.at += 1;
            return Ok(Json::Array(items));
        }
        loop {
            items.push(self.value(depth + 1)?);
            self.skip_space();
            match self.peek() {
                Some(b',') => {
                    self.at += 1;
                    self.skip_space();
                }
                Some(b']') => {
                    self.at += 1;
                    return Ok(Json::Array(items));
                }
                _ => return Err(self.error("expected ',' or ']'")),
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, ApiError> {
        let digits = self
            .text
            .get(self.at..self.at + 4)
            .filter(|digits| digits.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .ok_or_else(|| self.error("expected four hexadecimal digits"))?;
        let value = u32::from_str_radix(digits, 16).map_err(|_| self.error("bad escape"))?;
        self.at += 4;
        Ok(value)
    }

    fn escape(&mut self, out: &mut String) -> Result<(), ApiError> {
        let byte = self
            .peek()
            .ok_or_else(|| self.error("unterminated escape"))?;
        self.at += 1;
        let decoded = match byte {
            b'"' => '"',
            b'\\' => '\\',
            b'/' => '/',
            b'b' => '\u{8}',
            b'f' => '\u{c}',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => {
                let high = self.hex4()?;
                let code = if (0xd800..0xdc00).contains(&high) {
                    if !self.text[self.at..].starts_with("\\u") {
                        return Err(self.error("lone surrogate"));
                    }
                    self.at += 2;
                    let low = self.hex4()?;
                    if !(0xdc00..0xe000).contains(&low) {
                        return Err(self.error("lone surrogate"));
                    }
                    0x1_0000 + ((high - 0xd800) << 10) + (low - 0xdc00)
                } else if (0xdc00..0xe000).contains(&high) {
                    return Err(self.error("lone surrogate"));
                } else {
                    high
                };
                char::from_u32(code).ok_or_else(|| self.error("bad escape"))?
            }
            _ => return Err(self.error("bad escape")),
        };
        out.push(decoded);
        Ok(())
    }

    fn string(&mut self) -> Result<String, ApiError> {
        self.at += 1;
        let mut out = String::new();
        loop {
            let start = self.at;
            while matches!(self.peek(), Some(byte) if byte != b'"' && byte != b'\\' && byte >= 0x20)
            {
                self.at += 1;
            }
            out.push_str(&self.text[start..self.at]);
            match self.peek() {
                Some(b'"') => {
                    self.at += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.at += 1;
                    self.escape(&mut out)?;
                }
                Some(_) => return Err(self.error("control character in a string")),
                None => return Err(self.error("unterminated string")),
            }
        }
    }

    fn digits(&mut self) -> usize {
        let start = self.at;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.at += 1;
        }
        self.at - start
    }

    fn number(&mut self) -> Result<Json, ApiError> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        match self.peek() {
            Some(b'0') => self.at += 1,
            Some(b'1'..=b'9') => {
                self.digits();
            }
            _ => return Err(self.error("malformed number")),
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            if self.digits() == 0 {
                return Err(self.error("malformed number"));
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if self.digits() == 0 {
                return Err(self.error("malformed number"));
            }
        }
        let value: f64 = self.text[start..self.at]
            .parse()
            .map_err(|_| self.error("malformed number"))?;
        if !value.is_finite() {
            return Err(self.error("number out of range"));
        }
        Ok(Json::Number(value))
    }
}

/// Parses one JSON text. Duplicate member names, lone surrogates and numbers
/// beyond the double range are refused, as I-JSON requires.
///
/// # Errors
/// Returns where and why the text is not JSON.
pub fn parse_json(text: &str) -> Result<Json, ApiError> {
    let mut parser = JsonParser { text, at: 0 };
    parser.skip_space();
    let value = parser.value(0)?;
    parser.skip_space();
    if parser.at != text.len() {
        return Err(parser.error("data after the value"));
    }
    Ok(value)
}

/// The value an RFC 6901 JSON pointer selects, if any.
#[must_use]
pub fn resolve<'a>(document: &'a Json, pointer: &str) -> Option<&'a Json> {
    if pointer.is_empty() {
        return Some(document);
    }
    let mut current = document;
    for token in pointer.strip_prefix('/')?.split('/') {
        let token = token.replace("~1", "/").replace("~0", "~");
        current = match current {
            Json::Object(members) => members
                .iter()
                .find(|(name, _)| *name == token)
                .map(|(_, value)| value)?,
            Json::Array(items) => {
                let canonical_index = !token.is_empty()
                    && token.bytes().all(|byte| byte.is_ascii_digit())
                    && (token == "0" || !token.starts_with('0'));
                if !canonical_index {
                    return None;
                }
                items.get(token.parse::<usize>().ok()?)?
            }
            _ => return None,
        };
    }
    Some(current)
}

/// The shortest scientific digits that read back as `value`, and among
/// those the ones closest to it, as ECMAScript requires: the shortest form
/// fixes the digit count, the correctly rounded form at that count picks the
/// closest digits.
fn closest_shortest(value: f64) -> String {
    let shortest = format!("{value:e}");
    let precision = shortest
        .split_once('e')
        .map_or(shortest.as_str(), |(mantissa, _)| mantissa)
        .bytes()
        .filter(u8::is_ascii_digit)
        .count()
        .saturating_sub(1);
    let closest = format!("{value:.precision$e}");
    if closest
        .parse::<f64>()
        .is_ok_and(|parsed| parsed.to_bits() == value.to_bits())
    {
        closest
    } else {
        shortest
    }
}

/// A number as ECMAScript's `Number.prototype.toString` writes it, the form
/// RFC 8785 requires.
#[must_use]
pub fn es_number(value: f64) -> String {
    if value.classify() == FpCategory::Zero {
        return "0".to_owned();
    }
    let formatted = closest_shortest(value);
    let (sign, body) = formatted
        .strip_prefix('-')
        .map_or(("", formatted.as_str()), |body| ("-", body));
    let (mantissa, exponent) = body.split_once('e').unwrap_or((body, "0"));
    let exponent: i64 = exponent.parse().unwrap_or(0);
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let k = i64::try_from(digits.len()).unwrap_or(i64::MAX);
    let n = exponent + 1;
    let zeros = |count: i64| "0".repeat(usize::try_from(count).unwrap_or(0));
    let at = |position: i64| usize::try_from(position).unwrap_or(0);
    let text = if k <= n && n <= 21 {
        format!("{digits}{}", zeros(n - k))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..at(n)], &digits[at(n)..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", zeros(-n))
    } else {
        let power = n - 1;
        let sign_of_power = if power >= 0 { '+' } else { '-' };
        let fraction = if k > 1 {
            format!(".{}", &digits[1..])
        } else {
            String::new()
        };
        format!(
            "{}{fraction}e{sign_of_power}{}",
            &digits[..1],
            power.unsigned_abs()
        )
    };
    format!("{sign}{text}")
}

fn write_string(text: &str, out: &mut String) {
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if u32::from(control) < 0x20 => {
                let code = u32::from(control);
                out.push_str("\\u00");
                out.extend(char::from_digit(code >> 4, 16));
                out.extend(char::from_digit(code & 0xf, 16));
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

fn utf16_order(left: &str, right: &str) -> Ordering {
    left.encode_utf16().cmp(right.encode_utf16())
}

fn write_canonical(value: &Json, out: &mut String) {
    match value {
        Json::Null => out.push_str("null"),
        Json::Bool(true) => out.push_str("true"),
        Json::Bool(false) => out.push_str("false"),
        Json::Number(number) => out.push_str(&es_number(*number)),
        Json::String(text) => write_string(text, out),
        Json::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        Json::Object(members) => {
            let mut sorted: Vec<&(String, Json)> = members.iter().collect();
            sorted.sort_by(|left, right| utf16_order(&left.0, &right.0));
            out.push('{');
            for (index, (name, member)) in sorted.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_string(name, out);
                out.push(':');
                write_canonical(member, out);
            }
            out.push('}');
        }
    }
}

/// The RFC 8785 canonical form of a JSON value.
#[must_use]
pub fn canonical_json(value: &Json) -> String {
    let mut out = String::new();
    write_canonical(value, &mut out);
    out
}

/// The answer a response body gives to a list of pointers: the canonical
/// JSON array of the selected values in pointer order, or the raw body when
/// there is no pointer.
///
/// # Errors
/// Refuses a body that is not JSON when pointers are given and a pointer
/// that selects nothing, naming it.
pub fn select_answer(body: &[u8], pointers: &[String]) -> Result<Vec<u8>, ApiError> {
    if pointers.is_empty() {
        return Ok(body.to_vec());
    }
    let text = std::str::from_utf8(body)
        .map_err(|_| ApiError::NotJson("the body is not UTF-8".to_owned()))?;
    let document = parse_json(text)?;
    let mut out = String::from("[");
    for (index, pointer) in pointers.iter().enumerate() {
        let selected =
            resolve(&document, pointer).ok_or_else(|| ApiError::Pointer(pointer.clone()))?;
        if index > 0 {
            out.push(',');
        }
        write_canonical(selected, &mut out);
    }
    out.push(']');
    Ok(out.into_bytes())
}

/// The canonical bytes of an api answer: the content layout of canonical.rs
/// with the kind byte 3, the api payload, the media type essence and the
/// answer.
///
/// # Errors
/// Refuses a media type the content layout does not accept and fields longer
/// than their length prefixes.
pub fn api_canonical_bytes(
    payload: &[u8],
    media_type: &str,
    answer: &[u8],
) -> Result<Vec<u8>, ApiError> {
    canonical::answer_canonical_bytes(payload, media_type, answer).map_err(|error| match error {
        CanonicalError::InvalidMediaType => ApiError::MediaType,
        _ => ApiError::TooLarge,
    })
}

/// What the API answered.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApiResponse {
    pub status: u16,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
}

fn remaining(deadline: Instant) -> Result<Duration, ApiError> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        Err(ApiError::Fetch(FetchError::Timeout))
    } else {
        Ok(left)
    }
}

type TlsStream = native_tls::TlsStream<TcpStream>;

/// The response side of one call, read under the deadline.
struct Wire {
    stream: TlsStream,
    buffer: Vec<u8>,
    start: usize,
    deadline: Instant,
}

const fn fetch(error: FetchError) -> ApiError {
    ApiError::Fetch(error)
}

impl Wire {
    fn available(&self) -> &[u8] {
        &self.buffer[self.start..]
    }

    fn fill(&mut self) -> Result<usize, ApiError> {
        let mut chunk = [0_u8; READ_CHUNK];
        loop {
            let left = remaining(self.deadline)?;
            self.stream
                .get_ref()
                .set_read_timeout(Some(left))
                .map_err(|_| fetch(FetchError::Transport))?;
            match self.stream.read(&mut chunk) {
                Ok(count) => {
                    self.buffer.extend_from_slice(&chunk[..count]);
                    return Ok(count);
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    return Err(fetch(FetchError::Timeout))
                }
                Err(_) => return Err(fetch(FetchError::Transport)),
            }
        }
    }

    fn until(&mut self, marker: &[u8], maximum: usize) -> Result<String, ApiError> {
        loop {
            if let Some(end) = self
                .available()
                .windows(marker.len())
                .position(|window| window == marker)
            {
                let text = std::str::from_utf8(&self.available()[..end])
                    .map_err(|_| fetch(FetchError::MalformedResponse))?
                    .to_owned();
                self.start += end + marker.len();
                return Ok(text);
            }
            if self.available().len() > maximum || self.fill()? == 0 {
                return Err(fetch(FetchError::MalformedResponse));
            }
        }
    }

    fn take(&mut self, count: usize) -> Result<Vec<u8>, ApiError> {
        while self.available().len() < count {
            if self.fill()? == 0 {
                return Err(fetch(FetchError::MalformedResponse));
            }
        }
        let bytes = self.available()[..count].to_vec();
        self.start += count;
        Ok(bytes)
    }

    fn until_close(&mut self, limit: usize) -> Result<Vec<u8>, ApiError> {
        loop {
            if self.available().len() > limit {
                return Err(fetch(FetchError::BodyTooLarge));
            }
            if self.fill()? == 0 {
                let bytes = self.available().to_vec();
                self.start = self.buffer.len();
                return Ok(bytes);
            }
        }
    }

    fn chunked(&mut self, limit: usize) -> Result<Vec<u8>, ApiError> {
        let mut body = Vec::new();
        loop {
            let line = self.until(b"\r\n", MAX_CHUNK_LINE)?;
            let size = line.split(';').next().unwrap_or_default().trim();
            if size.is_empty() || size.len() > 16 {
                return Err(fetch(FetchError::MalformedResponse));
            }
            let size = usize::from_str_radix(size, 16)
                .map_err(|_| fetch(FetchError::MalformedResponse))?;
            if size == 0 {
                while !self.until(b"\r\n", MAX_RESPONSE_HEAD)?.is_empty() {}
                return Ok(body);
            }
            if body.len().saturating_add(size) > limit {
                return Err(fetch(FetchError::BodyTooLarge));
            }
            body.extend_from_slice(&self.take(size)?);
            if self.take(2)? != b"\r\n" {
                return Err(fetch(FetchError::MalformedResponse));
            }
        }
    }
}

fn parse_head(head: &str) -> Result<(u16, Vec<(String, String)>), ApiError> {
    let malformed = || fetch(FetchError::MalformedResponse);
    let mut lines = head.split("\r\n");
    let mut parts = lines.next().unwrap_or_default().splitn(3, ' ');
    let version = parts.next().unwrap_or_default();
    let status = parts.next().unwrap_or_default();
    if !matches!(version, "HTTP/1.1" | "HTTP/1.0")
        || status.len() != 3
        || !status.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(malformed());
    }
    let status: u16 = status.parse().map_err(|_| malformed())?;
    let mut headers: Vec<(String, String)> = Vec::new();
    for line in lines {
        if line.starts_with([' ', '\t']) {
            return Err(malformed());
        }
        let (name, value) = line.split_once(':').ok_or_else(malformed)?;
        if name.is_empty() || name.bytes().any(|byte| byte <= b' ') {
            return Err(malformed());
        }
        let value = value.trim_matches([' ', '\t']);
        let framing = ["content-length", "transfer-encoding", "content-type"]
            .contains(&name.to_ascii_lowercase().as_str());
        match headers
            .iter()
            .find(|(existing, _)| framing && existing.eq_ignore_ascii_case(name))
        {
            Some((_, existing)) if existing == value => {}
            Some(_) => return Err(malformed()),
            None => headers.push((name.to_owned(), value.to_owned())),
        }
    }
    Ok((status, headers))
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

/// Makes api calls over https under the fetch path's limits and destination
/// rules. Redirects are refused, since a credential is bound to one origin.
pub struct ApiClient {
    fetcher: Arc<Fetcher>,
    tls: native_tls::TlsConnector,
}

impl ApiClient {
    /// A client that trusts the system roots and `roots`.
    ///
    /// # Errors
    /// Returns a TLS connector that cannot be built.
    pub fn new(fetcher: Arc<Fetcher>, roots: &[native_tls::Certificate]) -> Result<Self, ApiError> {
        let mut builder = native_tls::TlsConnector::builder();
        for root in roots {
            builder.add_root_certificate(root.clone());
        }
        let tls = builder.build().map_err(|_| fetch(FetchError::Tls))?;
        Ok(Self { fetcher, tls })
    }

    /// The largest body, and so the largest answer, a call accepts.
    #[must_use]
    pub fn body_limit(&self) -> usize {
        usize::try_from(self.fetcher.limits().max_body_bytes).unwrap_or(usize::MAX)
    }

    fn connect(
        &self,
        url: &Url,
        address: SocketAddr,
        deadline: Instant,
    ) -> Result<TlsStream, ApiError> {
        let connect_timeout = self.fetcher.limits().connect_timeout();
        let left = remaining(deadline)?;
        let limited = connect_timeout < left;
        let socket =
            TcpStream::connect_timeout(&address, connect_timeout.min(left)).map_err(|error| {
                match error.kind() {
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock if limited => {
                        fetch(FetchError::ConnectTimeout)
                    }
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => {
                        fetch(FetchError::Timeout)
                    }
                    _ => fetch(FetchError::Connect),
                }
            })?;
        let left = remaining(deadline)?;
        socket
            .set_write_timeout(Some(left))
            .and_then(|()| socket.set_read_timeout(Some(left)))
            .map_err(|_| fetch(FetchError::Transport))?;
        self.tls.connect(url.bare_host(), socket).map_err(|_| {
            if Instant::now() >= deadline {
                fetch(FetchError::Timeout)
            } else {
                fetch(FetchError::Tls)
            }
        })
    }

    /// The request bytes, written into memory sized once so no copy of a
    /// credential value is left behind by a reallocation.
    fn request_bytes(
        method: &str,
        url: &Url,
        payload: &ApiPayload,
        credential: Option<&Credential>,
    ) -> Zeroizing<Vec<u8>> {
        let mut head = format!(
            "{method} {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: {}/{}\r\nAccept-Encoding: identity\r\nConnection: close\r\n",
            url.target,
            url.authority(),
            crate::robots::USER_AGENT_TOKEN,
            env!("CARGO_PKG_VERSION"),
        );
        if payload.method == METHOD_POST {
            head.push_str("Content-Length: ");
            head.push_str(&payload.body.len().to_string());
            head.push_str("\r\n");
        }
        let secret_headers = credential.map_or(&[][..], |credential| &credential.headers[..]);
        let length = head.len()
            + payload
                .headers
                .iter()
                .map(|header| header.name.len() + header.value.len() + 4)
                .sum::<usize>()
            + secret_headers
                .iter()
                .map(|(name, value)| name.len() + value.len() + 4)
                .sum::<usize>()
            + 2
            + payload.body.len();
        let mut out = Zeroizing::new(Vec::with_capacity(length));
        out.extend_from_slice(head.as_bytes());
        let public = payload
            .headers
            .iter()
            .map(|header| (header.name.as_str(), header.value.as_str()));
        let secret = secret_headers
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()));
        for (name, value) in public.chain(secret) {
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(b": ");
            out.extend_from_slice(value.as_bytes());
            out.extend_from_slice(b"\r\n");
        }
        out.extend_from_slice(b"\r\n");
        out.extend_from_slice(&payload.body);
        out
    }

    /// Performs the payload's call with its public headers and the
    /// credential's headers: https only, the destination rules, the connect
    /// and total time limits and the body limit of the fetch path.
    ///
    /// # Errors
    /// Returns the refusal that stopped the call, and a status other than
    /// 2xx, a redirect included.
    pub fn call(
        &self,
        payload: &ApiPayload,
        credential: Option<&Credential>,
    ) -> Result<ApiResponse, ApiError> {
        let method = payload.method_name()?;
        let url = Url::parse(&payload.url).map_err(fetch)?;
        if !url.secure {
            return Err(fetch(FetchError::UnsupportedScheme));
        }
        let deadline = Instant::now() + self.fetcher.limits().total_timeout();
        let address = self.fetcher.destination(&url).map_err(fetch)?;
        let mut stream = self.connect(&url, address, deadline)?;
        let request = Self::request_bytes(method, &url, payload, credential);
        stream
            .write_all(&request)
            .and_then(|()| stream.flush())
            .map_err(|error| match error.kind() {
                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => fetch(FetchError::Timeout),
                _ => fetch(FetchError::Transport),
            })?;
        drop(request);
        let mut wire = Wire {
            stream,
            buffer: Vec::with_capacity(READ_CHUNK),
            start: 0,
            deadline,
        };
        let (status, headers) = loop {
            let (status, headers) = parse_head(&wire.until(b"\r\n\r\n", MAX_RESPONSE_HEAD)?)?;
            if !(100..200).contains(&status) {
                break (status, headers);
            }
        };
        if !(200..300).contains(&status) {
            return Err(ApiError::Status(status));
        }
        if header(&headers, "content-encoding")
            .is_some_and(|encoding| !encoding.eq_ignore_ascii_case("identity"))
        {
            return Err(fetch(FetchError::UnsupportedEncoding));
        }
        let limit = self.body_limit();
        let body = if let Some(encoding) = header(&headers, "transfer-encoding") {
            if !encoding.eq_ignore_ascii_case("chunked") {
                return Err(fetch(FetchError::UnsupportedEncoding));
            }
            wire.chunked(limit)?
        } else if let Some(length) = header(&headers, "content-length") {
            if length.is_empty() || !length.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(fetch(FetchError::MalformedResponse));
            }
            let length: usize = length
                .parse()
                .map_err(|_| fetch(FetchError::BodyTooLarge))?;
            if length > limit {
                return Err(fetch(FetchError::BodyTooLarge));
            }
            wire.take(length)?
        } else {
            wire.until_close(limit)?
        };
        Ok(ApiResponse {
            status,
            content_type: header(&headers, "content-type").map(str::to_owned),
            body,
        })
    }
}

/// An attested api answer: the answer bytes, their media type, the
/// canonical bytes and their digest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApiAnswer {
    pub answer: Vec<u8>,
    pub media_type: String,
    pub canonical: Vec<u8>,
    pub digest: [u8; 32],
}

/// Performs one api request as the attestor `key`: picks the envelope
/// addressed to it and opens it in memory, calls with the public and the
/// credential headers, selects and canonicalises the answer and bounds it.
/// The opened credential is zeroised as soon as the call is sent.
///
/// # Errors
/// Refuses a payload with envelopes none of which is addressed to this
/// attestor before any call, and returns every envelope, call and selection
/// refusal.
pub fn answer(
    client: &ApiClient,
    key: &SigningKey,
    raw_payload: &[u8],
    payload: &ApiPayload,
) -> Result<ApiAnswer, ApiError> {
    let credential = if payload.envelopes.is_empty() {
        None
    } else {
        let envelope = payload
            .envelope_for(&signer_address(key))
            .ok_or(ApiError::NoEnvelope)?;
        let plaintext = envelope.open(key, &payload.origin()?)?;
        Some(Credential::decode(&plaintext, &payload.headers)?)
    };
    let response = client.call(payload, credential.as_ref());
    drop(credential);
    let response = response?;
    let media_type = if payload.pointers.is_empty() {
        let declared = response
            .content_type
            .as_deref()
            .ok_or(ApiError::MediaType)?;
        canonical::media_type_essence(declared).map_err(|_| ApiError::MediaType)?
    } else {
        ANSWER_MEDIA_TYPE.to_owned()
    };
    let answer = select_answer(&response.body, &payload.pointers)?;
    if answer.len() > client.body_limit() {
        return Err(ApiError::TooLarge);
    }
    let canonical = api_canonical_bytes(raw_payload, &media_type, &answer)?;
    Ok(ApiAnswer {
        digest: canonical::content_digest(&canonical),
        answer,
        media_type,
        canonical,
    })
}

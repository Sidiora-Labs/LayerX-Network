use std::collections::{BTreeMap, BTreeSet};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use p256::ecdsa::signature::Verifier as _;
use p256::ecdsa::{Signature, VerifyingKey};
use serde::Deserialize;
use serde_json::{Map, Value};
use sha2::{Digest as _, Sha256, Sha384, Sha512};

use crate::error::Ap2Error;

const TOKEN_LIMIT: usize = 256 * 1_024;
const JWT_LIMIT: usize = 128 * 1_024;
const DISCLOSURE_LIMIT: usize = 512;
const DISCLOSURE_COUNT_LIMIT: usize = 512;
const JSON_DEPTH_LIMIT: usize = 64;

/// Trust purpose supplied to the key resolver. Callers can maintain distinct
/// roots for users, trusted surfaces, merchants and receipt issuers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyUse {
    CheckoutMandateIssuer,
    PaymentMandateIssuer,
    MerchantCheckout,
}

/// Protected JWS header after bounded decoding. A resolver must authenticate
/// either `kid` or `x5c`; merely finding a syntactically valid key is not
/// sufficient trust.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub struct ProtectedHeader {
    alg: String,
    #[serde(default)]
    typ: Option<String>,
    #[serde(default)]
    kid: Option<String>,
    #[serde(default)]
    x5c: Option<Vec<String>>,
    #[serde(default)]
    crit: Option<Vec<String>>,
    #[serde(default)]
    b64: Option<bool>,
}

impl ProtectedHeader {
    #[must_use]
    pub fn algorithm(&self) -> &str {
        &self.alg
    }

    #[must_use]
    pub fn token_type(&self) -> Option<&str> {
        self.typ.as_deref()
    }

    #[must_use]
    pub fn key_id(&self) -> Option<&str> {
        self.kid.as_deref()
    }

    #[must_use]
    pub fn certificate_chain(&self) -> Option<&[String]> {
        self.x5c.as_deref()
    }

    fn validate(&self, key_bound: bool) -> Result<(), Ap2Error> {
        if self.alg != "ES256" {
            return Err(Ap2Error::UnsupportedAlgorithm);
        }
        if self.b64 == Some(false) || self.crit.as_ref().is_some_and(|value| !value.is_empty()) {
            return Err(Ap2Error::UnsupportedAlgorithm);
        }
        if self
            .typ
            .as_ref()
            .is_some_and(|value| !bounded_text(value, 64))
            || self
                .kid
                .as_ref()
                .is_some_and(|value| !bounded_text(value, 512))
            || self.x5c.as_ref().is_some_and(|chain| {
                chain.is_empty()
                    || chain.len() > 8
                    || chain
                        .iter()
                        .any(|certificate| certificate.is_empty() || certificate.len() > 16 * 1_024)
            })
        {
            return Err(Ap2Error::Bounds);
        }
        if !key_bound && self.kid.is_none() && self.x5c.is_none() {
            return Err(Ap2Error::KeyResolution);
        }
        Ok(())
    }
}

/// Application trust-store boundary for root mandates and merchant checkouts.
/// Implementations validate `kid` or the complete `x5c` chain before
/// returning a P-256 key.
pub trait KeyResolver {
    /// Resolves one trusted verification key for the declared use.
    ///
    /// # Errors
    ///
    /// Returns a typed trust or policy refusal.
    fn resolve(&self, usage: KeyUse, header: &ProtectedHeader) -> Result<VerifyingKey, Ap2Error>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HashAlgorithm {
    Sha256,
    Sha384,
    Sha512,
}

impl HashAlgorithm {
    fn parse(value: Option<&Value>) -> Result<Self, Ap2Error> {
        match value.and_then(Value::as_str).unwrap_or("sha-256") {
            "sha-256" => Ok(Self::Sha256),
            "sha-384" => Ok(Self::Sha384),
            "sha-512" => Ok(Self::Sha512),
            _ => Err(Ap2Error::UnsupportedAlgorithm),
        }
    }

    fn digest(self, value: &[u8]) -> Vec<u8> {
        match self {
            Self::Sha256 => Sha256::digest(value).to_vec(),
            Self::Sha384 => Sha384::digest(value).to_vec(),
            Self::Sha512 => Sha512::digest(value).to_vec(),
        }
    }

    pub(crate) fn digest_b64(self, value: &[u8]) -> String {
        URL_SAFE_NO_PAD.encode(self.digest(value))
    }
}

#[derive(Clone, Debug)]
pub(crate) struct VerifiedSdJwt {
    pub(crate) issuer_jwt: String,
    pub(crate) canonical: String,
    pub(crate) header: ProtectedHeader,
    pub(crate) payload: Value,
    pub(crate) effective: Value,
    pub(crate) hash_algorithm: HashAlgorithm,
    pub(crate) verification_key: VerifyingKey,
}

impl VerifiedSdJwt {
    pub(crate) fn sd_hash(&self) -> String {
        self.hash_algorithm.digest_b64(self.canonical.as_bytes())
    }

    pub(crate) fn issuer_jwt_hash(&self) -> String {
        self.hash_algorithm.digest_b64(self.issuer_jwt.as_bytes())
    }

    pub(crate) fn receipt_reference(&self) -> String {
        HashAlgorithm::Sha256.digest_b64(self.issuer_jwt.as_bytes())
    }

    pub(crate) fn confirmation_key(&self) -> Result<VerifyingKey, Ap2Error> {
        let object = self
            .effective
            .as_object()
            .ok_or(Ap2Error::Malformed("mandate payload"))?;
        let jwk = object
            .get("cnf")
            .and_then(Value::as_object)
            .and_then(|confirmation| confirmation.get("jwk"))
            .and_then(Value::as_object)
            .ok_or(Ap2Error::InvalidKeyBinding)?;
        verifying_key_from_jwk(jwk)
    }
}

#[derive(Debug)]
struct ParsedSdJwt {
    issuer_jwt: String,
    disclosures: Vec<String>,
    canonical: String,
    header: ProtectedHeader,
    payload: Value,
    hash_algorithm: HashAlgorithm,
}

pub(crate) fn split_presentation(value: &str) -> Result<Vec<&str>, Ap2Error> {
    if value.is_empty() || value.len() > TOKEN_LIMIT {
        return Err(Ap2Error::Bounds);
    }
    let parts: Vec<_> = value.split("~~~").collect();
    if parts.is_empty() || parts.iter().any(|part| part.is_empty()) {
        return Err(Ap2Error::Malformed("mandate presentation"));
    }
    Ok(parts)
}

pub(crate) fn verify_root(
    value: &str,
    usage: KeyUse,
    resolver: &impl KeyResolver,
) -> Result<VerifiedSdJwt, Ap2Error> {
    let parsed = parse_sd_jwt(value, false)?;
    let key = resolver.resolve(usage, &parsed.header)?;
    verify_parsed(parsed, &key)
}

pub(crate) fn verify_key_bound(value: &str, key: &VerifyingKey) -> Result<VerifiedSdJwt, Ap2Error> {
    let parsed = parse_sd_jwt(value, true)?;
    verify_parsed(parsed, key)
}

pub(crate) fn verify_compact_jws(
    value: &str,
    usage: KeyUse,
    resolver: &impl KeyResolver,
) -> Result<(ProtectedHeader, Value), Ap2Error> {
    let (header, payload, signing_input, signature) = parse_compact_jws(value, false)?;
    let key = resolver.resolve(usage, &header)?;
    key.verify(signing_input.as_bytes(), &signature)
        .map_err(|_| Ap2Error::InvalidSignature)?;
    Ok((header, payload))
}

pub(crate) fn verify_signature(
    signing_input: &[u8],
    signature: &[u8; 64],
    key: &VerifyingKey,
) -> Result<(), Ap2Error> {
    let signature = Signature::from_slice(signature).map_err(|_| Ap2Error::InvalidSignature)?;
    key.verify(signing_input, &signature)
        .map_err(|_| Ap2Error::InvalidSignature)
}

fn parse_sd_jwt(value: &str, key_bound: bool) -> Result<ParsedSdJwt, Ap2Error> {
    if value.is_empty() || value.len() > TOKEN_LIMIT {
        return Err(Ap2Error::Bounds);
    }
    let mut components: Vec<_> = value.split('~').collect();
    if components.last() == Some(&"") {
        components.pop();
    }
    let issuer_jwt = components
        .first()
        .copied()
        .ok_or(Ap2Error::Malformed("SD-JWT"))?;
    if issuer_jwt.is_empty() || issuer_jwt.len() > JWT_LIMIT {
        return Err(Ap2Error::Bounds);
    }
    let disclosures: Vec<String> = components.into_iter().skip(1).map(str::to_owned).collect();
    if disclosures.len() > DISCLOSURE_COUNT_LIMIT
        || disclosures
            .iter()
            .any(|disclosure| disclosure.is_empty() || disclosure.len() > DISCLOSURE_LIMIT)
    {
        return Err(Ap2Error::Bounds);
    }
    let (header, payload, _, _) = parse_compact_jws(issuer_jwt, key_bound)?;
    let hash_algorithm = HashAlgorithm::parse(payload.get("_sd_alg"))?;
    let mut canonical = issuer_jwt.to_owned();
    canonical.push('~');
    if !disclosures.is_empty() {
        canonical.push_str(&disclosures.join("~"));
        canonical.push('~');
    }
    Ok(ParsedSdJwt {
        issuer_jwt: issuer_jwt.to_owned(),
        disclosures,
        canonical,
        header,
        payload,
        hash_algorithm,
    })
}

fn verify_parsed(parsed: ParsedSdJwt, key: &VerifyingKey) -> Result<VerifiedSdJwt, Ap2Error> {
    let (_, _, signing_input, signature) = parse_compact_jws(&parsed.issuer_jwt, true)?;
    key.verify(signing_input.as_bytes(), &signature)
        .map_err(|_| Ap2Error::InvalidSignature)?;
    let effective = resolve_disclosures(
        parsed.payload.clone(),
        &parsed.disclosures,
        parsed.hash_algorithm,
    )?;
    let delegate_payload = effective
        .get("delegate_payload")
        .and_then(Value::as_array)
        .ok_or(Ap2Error::Malformed("delegate_payload"))?;
    let items: Vec<_> = delegate_payload
        .iter()
        .filter(|value| value.is_object())
        .collect();
    if items.len() != 1 {
        return Err(Ap2Error::Malformed("delegate_payload"));
    }
    let mandate = items[0].clone();
    Ok(VerifiedSdJwt {
        issuer_jwt: parsed.issuer_jwt,
        canonical: parsed.canonical,
        header: parsed.header,
        payload: effective,
        effective: mandate,
        hash_algorithm: parsed.hash_algorithm,
        verification_key: *key,
    })
}

fn parse_compact_jws(
    value: &str,
    key_bound: bool,
) -> Result<(ProtectedHeader, Value, String, Signature), Ap2Error> {
    if value.is_empty() || value.len() > JWT_LIMIT {
        return Err(Ap2Error::Bounds);
    }
    let mut parts = value.split('.');
    let header_segment = parts.next().ok_or(Ap2Error::Malformed("JWS"))?;
    let payload_segment = parts.next().ok_or(Ap2Error::Malformed("JWS"))?;
    let signature_segment = parts.next().ok_or(Ap2Error::Malformed("JWS"))?;
    if parts.next().is_some()
        || header_segment.is_empty()
        || payload_segment.is_empty()
        || signature_segment.is_empty()
    {
        return Err(Ap2Error::Malformed("JWS"));
    }
    let header_bytes = decode_b64(header_segment, "JWS header")?;
    let payload_bytes = decode_b64(payload_segment, "JWS payload")?;
    let signature_bytes = decode_b64(signature_segment, "JWS signature")?;
    let header: ProtectedHeader =
        serde_json::from_slice(&header_bytes).map_err(|_| Ap2Error::Malformed("JWS header"))?;
    header.validate(key_bound)?;
    let payload: Value =
        serde_json::from_slice(&payload_bytes).map_err(|_| Ap2Error::Malformed("JWS payload"))?;
    if !payload.is_object() {
        return Err(Ap2Error::Malformed("JWS payload"));
    }
    let signature =
        Signature::from_slice(&signature_bytes).map_err(|_| Ap2Error::InvalidSignature)?;
    Ok((
        header,
        payload,
        format!("{header_segment}.{payload_segment}"),
        signature,
    ))
}

fn decode_b64(value: &str, field: &'static str) -> Result<Vec<u8>, Ap2Error> {
    if value.contains('=') || !value.bytes().all(is_b64url) {
        return Err(Ap2Error::Malformed(field));
    }
    URL_SAFE_NO_PAD
        .decode(value.as_bytes())
        .map_err(|_| Ap2Error::Malformed(field))
}

fn is_b64url(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'
}

#[derive(Clone, Debug)]
enum Disclosure {
    Array(Value),
    Property(String, Value),
}

fn resolve_disclosures(
    mut payload: Value,
    disclosures: &[String],
    algorithm: HashAlgorithm,
) -> Result<Value, Ap2Error> {
    let mut values = BTreeMap::new();
    for encoded in disclosures {
        let bytes = decode_b64(encoded, "disclosure")?;
        let decoded: Value =
            serde_json::from_slice(&bytes).map_err(|_| Ap2Error::InvalidDisclosure)?;
        let items = decoded.as_array().ok_or(Ap2Error::InvalidDisclosure)?;
        let disclosure = match items.as_slice() {
            [salt, value] if valid_salt(salt) => Disclosure::Array(value.clone()),
            [salt, name, value] if valid_salt(salt) => {
                let name = name.as_str().ok_or(Ap2Error::InvalidDisclosure)?;
                if !bounded_text(name, 256) {
                    return Err(Ap2Error::InvalidDisclosure);
                }
                Disclosure::Property(name.to_owned(), value.clone())
            }
            _ => return Err(Ap2Error::InvalidDisclosure),
        };
        let digest = algorithm.digest_b64(encoded.as_bytes());
        if values.insert(digest, disclosure).is_some() {
            return Err(Ap2Error::InvalidDisclosure);
        }
    }
    let mut used = BTreeSet::new();
    resolve_value(&mut payload, &values, &mut used, 0)?;
    if used.len() != values.len() {
        return Err(Ap2Error::InvalidDisclosure);
    }
    Ok(payload)
}

fn resolve_value(
    value: &mut Value,
    disclosures: &BTreeMap<String, Disclosure>,
    used: &mut BTreeSet<String>,
    depth: usize,
) -> Result<(), Ap2Error> {
    if depth > JSON_DEPTH_LIMIT {
        return Err(Ap2Error::Bounds);
    }
    match value {
        Value::Object(object) => resolve_object(object, disclosures, used, depth + 1),
        Value::Array(array) => {
            let mut resolved = Vec::with_capacity(array.len());
            for mut item in std::mem::take(array) {
                let digest = disclosure_marker(&item);
                match digest.and_then(|value| disclosures.get(value).map(|entry| (value, entry))) {
                    Some((
                        digest,
                        Disclosure::Array(disclosed) | Disclosure::Property(_, disclosed),
                    )) => {
                        mark_used(used, digest)?;
                        let mut disclosed = disclosed.clone();
                        resolve_value(&mut disclosed, disclosures, used, depth + 1)?;
                        resolved.push(disclosed);
                    }
                    None if digest.is_some() => {}
                    None => {
                        resolve_value(&mut item, disclosures, used, depth + 1)?;
                        resolved.push(item);
                    }
                }
            }
            *array = resolved;
            Ok(())
        }
        _ => Ok(()),
    }
}

fn resolve_object(
    object: &mut Map<String, Value>,
    disclosures: &BTreeMap<String, Disclosure>,
    used: &mut BTreeSet<String>,
    depth: usize,
) -> Result<(), Ap2Error> {
    let digests = object.remove("_sd");
    for value in object.values_mut() {
        resolve_value(value, disclosures, used, depth + 1)?;
    }
    let Some(digests) = digests else {
        return Ok(());
    };
    let digests = digests.as_array().ok_or(Ap2Error::InvalidDisclosure)?;
    for digest in digests {
        let digest = digest.as_str().ok_or(Ap2Error::InvalidDisclosure)?;
        let Some(disclosure) = disclosures.get(digest) else {
            continue;
        };
        let Disclosure::Property(name, disclosed) = disclosure else {
            return Err(Ap2Error::InvalidDisclosure);
        };
        if object.contains_key(name) {
            return Err(Ap2Error::InvalidDisclosure);
        }
        mark_used(used, digest)?;
        let mut disclosed = disclosed.clone();
        resolve_value(&mut disclosed, disclosures, used, depth + 1)?;
        object.insert(name.clone(), disclosed);
    }
    Ok(())
}

fn disclosure_marker(value: &Value) -> Option<&str> {
    let object = value.as_object()?;
    if object.len() == 1 {
        object.get("...")?.as_str()
    } else {
        None
    }
}

fn mark_used(used: &mut BTreeSet<String>, digest: &str) -> Result<(), Ap2Error> {
    if used.insert(digest.to_owned()) {
        Ok(())
    } else {
        Err(Ap2Error::InvalidDisclosure)
    }
}

fn valid_salt(value: &Value) -> bool {
    value.as_str().is_some_and(|salt| bounded_text(salt, 256))
}

fn verifying_key_from_jwk(jwk: &Map<String, Value>) -> Result<VerifyingKey, Ap2Error> {
    if jwk.get("kty").and_then(Value::as_str) != Some("EC")
        || jwk.get("crv").and_then(Value::as_str) != Some("P-256")
    {
        return Err(Ap2Error::InvalidKeyBinding);
    }
    let x = jwk
        .get("x")
        .and_then(Value::as_str)
        .ok_or(Ap2Error::InvalidKeyBinding)?;
    let y = jwk
        .get("y")
        .and_then(Value::as_str)
        .ok_or(Ap2Error::InvalidKeyBinding)?;
    let x = decode_b64(x, "JWK x")?;
    let y = decode_b64(y, "JWK y")?;
    if x.len() != 32 || y.len() != 32 {
        return Err(Ap2Error::InvalidKeyBinding);
    }
    let mut point = [0_u8; 65];
    point[0] = 4;
    point[1..33].copy_from_slice(&x);
    point[33..].copy_from_slice(&y);
    VerifyingKey::from_sec1_bytes(&point).map_err(|_| Ap2Error::InvalidKeyBinding)
}

fn bounded_text(value: &str, limit: usize) -> bool {
    !value.is_empty() && value.len() <= limit && !value.bytes().any(|byte| byte.is_ascii_control())
}

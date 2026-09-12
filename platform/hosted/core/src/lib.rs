//! Canonical treasury SEND construction shared by the core boundary binary and
//! its real-node tests, together with the treasury signer boundary the binary
//! signs through.

mod custody;

pub use custody::{did_for_public_key, SeedSigner, SendError, SocketSigner, TreasurySigner};

use ed25519_dalek::SigningKey;
use layerx_crypto::signer::SigningRequest;
use layerx_crypto::SignatureMessage;
use layerx_intents::{compile, Intent, IntentKind, LxpSend};
use layerx_types::account::AccountId;
use layerx_types::activity::{Authority, EnvelopeBuilder, Signature, TimestampBound};
use layerx_types::amount::Amount;
use layerx_types::ids::{AssetId, Did, IdempotencyKey};
use layerx_types::intent::{
    AuthorizationSignature, ContextHash, NetworkId, ProtocolVersion, PublicKey, SendAuthorization,
    SendAuthorizationKind, Sequence, TimestampSeconds,
};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_wire::encode::Encoder;
use layerx_wire::hash::Domain;
use sha2::{Digest as _, Sha256};

/// Asset module ordinal of the canonical SEND activity.
pub const SEND_ACTIVITY: u16 = 5;

/// Every input needed to build one owner-authorised asset SEND.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SendRequest {
    pub network_id: u32,
    pub source_did: String,
    pub destination_did: String,
    pub asset: [u8; 32],
    pub amount: u128,
    pub account_sequence: u64,
    pub idempotency_key: [u8; 32],
    pub not_before_ms: u64,
    pub expires_at_ms: u64,
    pub fee_limit: u128,
}

/// A fully signed SEND ready for the LNI together with its identifying facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignedSend {
    pub canonical: Vec<u8>,
    pub activity_id: [u8; 32],
    pub source_account: [u8; 32],
    pub destination_account: [u8; 32],
    pub signer_public_key: [u8; 32],
    pub idempotency_key: [u8; 32],
}

/// Returns the canonical account identifier of `agent:<did>:main`.
///
/// # Errors
///
/// Returns the account-name validation failure as text.
pub fn main_account(did: &str) -> Result<[u8; 32], String> {
    let account = AccountId::parse(&format!("agent:{did}:main"))
        .map_err(|error| format!("account name for {did} is invalid: {error:?}"))?;
    layerx_wire::hash::account_id_for_protocol(
        &account,
        layerx_wire::limits::STATE_COMMITMENT_PROTOCOL_VERSION,
    )
    .map_err(|error| format!("account id for {did} cannot be derived: {error:?}"))
}

/// Builds the asset module registry that declares SEND.
///
/// # Errors
///
/// Returns the registry construction failure as text.
pub fn asset_registry() -> Result<(ModuleRegistry, ActivityType), String> {
    let activity_type = ActivityType::new(ModuleId::Asset, SEND_ACTIVITY)
        .map_err(|error| format!("asset send activity is unavailable: {error:?}"))?;
    let registration = ModuleRegistration::new(ModuleId::Asset, &[activity_type])
        .map_err(|error| format!("asset module registration is invalid: {error:?}"))?;
    let registry = ModuleRegistry::new(&[registration])
        .map_err(|error| format!("asset module registry is invalid: {error:?}"))?;
    Ok((registry, activity_type))
}

/// Computes the send context commitment the core recomputes during execution.
#[must_use]
pub fn send_context_hash(
    source: &[u8; 32],
    destination: &[u8; 32],
    asset: &[u8; 32],
    amount: u128,
    idempotency: &[u8; 32],
) -> [u8; 32] {
    let mut bytes = Vec::with_capacity(144);
    bytes.extend_from_slice(source);
    bytes.extend_from_slice(destination);
    bytes.extend_from_slice(asset);
    bytes.extend_from_slice(&amount.to_be_bytes());
    bytes.extend_from_slice(idempotency);
    domain_hash(Domain::ContextHash, &bytes)
}

/// Hashes bytes under one canonical wire domain tag.
#[must_use]
pub fn domain_hash(domain: Domain, bytes: &[u8]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(domain.tag());
    digest.update(bytes);
    digest.finalize().into()
}

/// Builds, compiles with layerx-intents and signs one SEND from a seed held
/// in process.
///
/// # Errors
///
/// Returns every construction, compilation or encoding failure as text.
pub fn build_send(seed: &[u8; 32], request: &SendRequest) -> Result<SignedSend, String> {
    build_send_with_identity_sequence(seed, request.account_sequence, request)
}

/// Builds one SEND from a seed held in process whose actor sequence is
/// independent of its source-account sequence.
///
/// # Errors
///
/// Returns every construction, compilation or encoding failure as text.
pub fn build_send_with_identity_sequence(
    seed: &[u8; 32],
    identity_sequence: u64,
    request: &SendRequest,
) -> Result<SignedSend, String> {
    build_send_with_signer(&SeedSigner::new(seed), identity_sequence, request)
        .map_err(|error| error.to_string())
}

/// Builds one SEND whose two signatures come from `signer`, which may hold the
/// treasury identity in process or reach it over the treasury signer socket.
///
/// # Errors
///
/// Returns [`SendError::Signer`] when the signer refuses or cannot be reached
/// and [`SendError::Invalid`] for every construction, compilation or encoding
/// failure.
pub fn build_send_with_signer(
    signer: &dyn TreasurySigner,
    identity_sequence: u64,
    request: &SendRequest,
) -> Result<SignedSend, SendError> {
    if request.amount == 0 {
        return Err(SendError::Invalid(
            "amount must be greater than zero".into(),
        ));
    }
    if request.expires_at_ms <= request.not_before_ms {
        return Err(SendError::Invalid(
            "expiry must follow the validity start".into(),
        ));
    }
    let public_key = signer.public_key();
    let source = main_account(&request.source_did).map_err(SendError::Invalid)?;
    let destination = main_account(&request.destination_did).map_err(SendError::Invalid)?;
    let context = send_context_hash(
        &source,
        &destination,
        &request.asset,
        request.amount,
        &request.idempotency_key,
    );
    let authorization = send_authorization(signer, &source, &destination, request, &context)?;
    let from = AccountId::parse(&format!("agent:{}:main", request.source_did))
        .map_err(|error| SendError::Invalid(format!("source account is invalid: {error:?}")))?;
    let to =
        AccountId::parse(&format!("agent:{}:main", request.destination_did)).map_err(|error| {
            SendError::Invalid(format!("destination account is invalid: {error:?}"))
        })?;
    let intent = LxpSend::new(
        from,
        to,
        AssetId::new(request.asset),
        Amount::from_u128(request.amount),
        Sequence::from_u64(request.account_sequence),
        IdempotencyKey::new(request.idempotency_key),
        TimestampSeconds::from_u64(request.expires_at_ms),
        ContextHash::new(context),
        SendAuthorization::new(
            SendAuthorizationKind::Owner,
            PublicKey::new(public_key),
            AuthorizationSignature::new(authorization),
        ),
        NetworkId::new(request.network_id)
            .map_err(|error| SendError::Invalid(format!("network id is invalid: {error:?}")))?,
        ProtocolVersion::new(layerx_wire::limits::STATE_COMMITMENT_PROTOCOL_VERSION).map_err(
            |error| SendError::Invalid(format!("protocol version is invalid: {error:?}")),
        )?,
    )
    .map_err(|error| SendError::Invalid(format!("send intent is invalid: {error:?}")))?;
    let (registry, activity_type) = asset_registry().map_err(SendError::Invalid)?;
    let compiled = compile(&Intent::v1(IntentKind::LxpSend(intent)), &registry)
        .map_err(|error| SendError::Invalid(format!("send intent does not compile: {error:?}")))?;
    if compiled.activity_type() != activity_type {
        return Err(SendError::Invalid(
            "compiled intent is not an asset send".into(),
        ));
    }
    let actor = Did::new(request.source_did.as_bytes())
        .map_err(|error| SendError::Invalid(format!("source DID is invalid: {error:?}")))?;
    let authority = Authority::owner(&public_key)
        .map_err(|error| SendError::Invalid(format!("owner authority is invalid: {error:?}")))?;
    let timestamp = TimestampBound::new(request.not_before_ms, request.expires_at_ms)
        .map_err(|error| SendError::Invalid(format!("timestamp bound is invalid: {error:?}")))?;
    let mut builder = EnvelopeBuilder::new();
    builder
        .protocol_version(layerx_wire::limits::STATE_COMMITMENT_PROTOCOL_VERSION)
        .and_then(|value| value.network_id(request.network_id))
        .and_then(|value| value.activity_type(activity_type))
        .and_then(|value| value.actor_did(actor))
        .and_then(|value| value.authority(authority))
        .and_then(|value| value.account_sequence(identity_sequence))
        .and_then(|value| value.timestamp_bound(timestamp))
        .and_then(|value| value.idempotency_key(IdempotencyKey::new(request.idempotency_key)))
        .and_then(|value| value.fee_limit(Amount::from_u128(request.fee_limit)))
        .and_then(|value| value.payload_hash(compiled.payload_hash()))
        .and_then(|value| value.payload(compiled.payload().clone()))
        .map_err(|error| SendError::Invalid(format!("send envelope is invalid: {error:?}")))?;
    let unsigned = builder
        .build()
        .map_err(|error| SendError::Invalid(format!("send envelope is incomplete: {error:?}")))?;
    let unsigned_bytes =
        layerx_wire::activity::encode_unsigned_envelope(&unsigned).map_err(|error| {
            SendError::Invalid(format!("send signing bytes are invalid: {error:?}"))
        })?;
    let digest = domain_hash(Domain::SignaturePreimage, &unsigned_bytes);
    let signature = disclosed_signature(signer, &unsigned_bytes, &registry, request.network_id)?;
    layerx_crypto::ed25519::verify_digest(&public_key, &signature, &digest).map_err(|error| {
        SendError::Invalid(format!("send signature does not verify: {error:?}"))
    })?;
    let signed = unsigned
        .attach_signature(Signature::new(&signature).map_err(|error| {
            SendError::Invalid(format!("send signature is invalid: {error:?}"))
        })?);
    let canonical = layerx_wire::activity::encode_signed_envelope(&signed)
        .map_err(|error| SendError::Invalid(format!("signed send is invalid: {error:?}")))?;
    let decoded = layerx_wire::activity::decode_signed(&canonical, &registry)
        .map_err(|error| SendError::Invalid(format!("signed send does not decode: {error:?}")))?;
    let activity_id = layerx_wire::hash::activity_id(&decoded)
        .map_err(|error| SendError::Invalid(format!("send activity id is invalid: {error:?}")))?;
    Ok(SignedSend {
        canonical,
        activity_id,
        source_account: source,
        destination_account: destination,
        signer_public_key: public_key,
        idempotency_key: request.idempotency_key,
    })
}

fn disclosed_signature(
    signer: &dyn TreasurySigner,
    canonical: &[u8],
    registry: &ModuleRegistry,
    network_id: u32,
) -> Result<[u8; 64], SendError> {
    let disclosure = layerx_crypto::disclosure::bind(canonical, registry)
        .map_err(|error| SendError::Invalid(format!("send disclosure is invalid: {error:?}")))?;
    let message = SignatureMessage::new(
        Domain::SignaturePreimage,
        layerx_wire::limits::STATE_COMMITMENT_PROTOCOL_VERSION,
        network_id,
        canonical,
    )
    .map_err(|error| SendError::Invalid(format!("send signing scope is invalid: {error:?}")))?;
    let request = SigningRequest::new(message, &disclosure)
        .map_err(|error| SendError::Invalid(format!("send disclosure does not bind: {error:?}")))?;
    let signature = signer
        .sign_digest(&request.message().digest())
        .map_err(SendError::Signer)?;
    layerx_crypto::ed25519::verify(&signer.public_key(), &signature, request.message()).map_err(
        |error| SendError::Signer(format!("treasury signature does not verify: {error:?}")),
    )?;
    Ok(signature)
}

fn send_authorization(
    signer: &dyn TreasurySigner,
    source: &[u8; 32],
    destination: &[u8; 32],
    request: &SendRequest,
    context: &[u8; 32],
) -> Result<[u8; 64], SendError> {
    let mut authorization = Encoder::new(512);
    authorization
        .u16(0x5301)
        .and_then(|()| authorization.fixed(source))
        .and_then(|()| authorization.fixed(destination))
        .and_then(|()| authorization.fixed(&request.asset))
        .and_then(|()| authorization.u128(request.amount))
        .and_then(|()| authorization.u64(request.account_sequence))
        .and_then(|()| authorization.fixed(&request.idempotency_key))
        .and_then(|()| authorization.u64(request.expires_at_ms))
        .and_then(|()| authorization.fixed(context))
        .and_then(|()| authorization.u8(0))
        .and_then(|()| authorization.u8(SendAuthorizationKind::Owner as u8))
        .and_then(|()| authorization.fixed(source))
        .and_then(|()| authorization.fixed(context))
        .and_then(|()| authorization.u32(request.network_id))
        .and_then(|()| authorization.u16(layerx_wire::limits::STATE_COMMITMENT_PROTOCOL_VERSION))
        .map_err(|error| {
            SendError::Invalid(format!("send authorization is too large: {error:?}"))
        })?;
    let digest = domain_hash(Domain::SignaturePreimage, &authorization.finish());
    signer.sign_digest(&digest).map_err(SendError::Signer)
}

/// Encodes bytes as lowercase hexadecimal.
#[must_use]
pub fn hex_encode(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(char::from(DIGITS[usize::from(byte >> 4)]));
        text.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    text
}

/// Decodes hexadecimal text of any case into bytes.
///
/// # Errors
///
/// Returns a description when the text is odd-length or not hexadecimal.
pub fn hex_decode(text: &str) -> Result<Vec<u8>, String> {
    if !text.len().is_multiple_of(2) {
        return Err("hex text has odd length".into());
    }
    let mut bytes = Vec::with_capacity(text.len() / 2);
    let raw = text.as_bytes();
    for pair in raw.chunks(2) {
        let high = hex_nibble(pair[0])?;
        let low = hex_nibble(pair[1])?;
        bytes.push((high << 4) | low);
    }
    Ok(bytes)
}

/// Decodes exactly `N` bytes of hexadecimal text.
///
/// # Errors
///
/// Returns a description naming the expected width on mismatch.
pub fn fixed_hex<const N: usize>(name: &str, text: &str) -> Result<[u8; N], String> {
    let bytes = hex_decode(text).map_err(|error| format!("{name}: {error}"))?;
    <[u8; N]>::try_from(bytes).map_err(|_| format!("{name} must be {N} bytes of hex"))
}

fn hex_nibble(byte: u8) -> Result<u8, String> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err("hex text contains a non-hexadecimal character".into()),
    }
}

/// Derives the beta treasury DID `did:layerx:<public key hex>` from a seed.
#[must_use]
pub fn treasury_did(seed: &[u8; 32]) -> String {
    did_for_public_key(&SigningKey::from_bytes(seed).verifying_key().to_bytes())
}

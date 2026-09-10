use layerx_types::activity::{Authority, EnvelopeBuilder, TimestampBound, UnsignedEnvelope};
use layerx_types::amount::Amount;
use layerx_types::ids::{Did, IdempotencyKey};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry, Payload};
use layerx_wire::encode::Encoder;
use layerx_wire::hash::Domain;
use sha2::{Digest as _, Sha256};

use crate::disclosure::{bind, Disclosure, DisclosureError};
use crate::signer::{SignError, Signer, SigningRequest};
use crate::SignatureMessage;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SendCondition {
    pub kind: u8,
    pub timestamp: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SendDebit {
    pub from: [u8; 32],
    pub to: [u8; 32],
    pub asset: [u8; 32],
    pub amount: u128,
    pub source_sequence: u64,
    pub idempotency_key: [u8; 32],
    pub expires_at: u64,
    pub context_hash: [u8; 32],
    pub conditions: Vec<SendCondition>,
    pub authorization_kind: u8,
    pub network_id: u32,
    pub protocol_version: u16,
}

impl SendDebit {
    fn common(&self) -> Result<Vec<u8>, DisclosureError> {
        if self.amount == 0
            || self.from == self.to
            || self.conditions.len() > 8
            || !(1..=6).contains(&self.authorization_kind)
            || self.network_id == 0
            || !layerx_wire::limits::protocol_version_supported(self.protocol_version)
        {
            return Err(DisclosureError::MalformedPayload);
        }
        let mut e = Encoder::new(512);
        e.fixed(&self.from)?;
        e.fixed(&self.to)?;
        e.fixed(&self.asset)?;
        e.u128(self.amount)?;
        e.u64(self.source_sequence)?;
        e.fixed(&self.idempotency_key)?;
        e.u64(self.expires_at)?;
        e.fixed(&self.context_hash)?;
        e.u8(u8::try_from(self.conditions.len()).map_err(|_| DisclosureError::MalformedPayload)?)?;
        for condition in &self.conditions {
            if !matches!(condition.kind, 1 | 2) {
                return Err(DisclosureError::MalformedPayload);
            }
            e.u8(condition.kind)?;
            e.u64(condition.timestamp)?;
        }
        Ok(e.finish())
    }

    /// # Errors
    /// Rejects malformed debit fields before creating the native authorization message.
    pub fn authorization_message(&self) -> Result<Vec<u8>, DisclosureError> {
        let mut e = Encoder::new(512);
        e.u16(0x5301)?;
        e.fixed(&self.common()?)?;
        e.u8(self.authorization_kind)?;
        e.fixed(&self.from)?;
        e.fixed(&self.context_hash)?;
        e.u32(self.network_id)?;
        e.u16(self.protocol_version)?;
        Ok(e.finish())
    }

    /// # Errors
    /// Refuses a noncanonical or differently scoped message; every debit field is disclosed.
    pub fn signing_request<'a>(
        &'a self,
        canonical: &'a [u8],
    ) -> Result<SigningRequest<'a>, SignError> {
        if self.authorization_message().map_err(SignError::from)? != canonical {
            return Err(SignError::DisclosureMismatch("send_authorization"));
        }
        let message = SignatureMessage::new(
            Domain::SignaturePreimage,
            self.protocol_version,
            self.network_id,
            canonical,
        )
        .map_err(|_| SignError::InvalidDisclosure)?;
        Ok(SigningRequest::send_debit(message, self))
    }

    /// # Errors
    /// Refuses malformed fields, signer refusal, or a signature not bound to the native debit domain.
    pub async fn sign(&self, signer: &dyn Signer) -> Result<Vec<u8>, SignError> {
        let canonical = self.authorization_message().map_err(SignError::from)?;
        let request = self.signing_request(&canonical)?;
        let signature = signer.sign(request).await?;
        self.encode_signed(signer.public_key(), *signature.as_bytes())
            .map_err(SignError::from)
    }

    /// # Errors
    /// Rejects invalid debit fields or authorization signatures.
    pub fn encode_signed(
        &self,
        public_key: [u8; 32],
        signature: [u8; 64],
    ) -> Result<Vec<u8>, DisclosureError> {
        let canonical = self.authorization_message()?;
        let message = SignatureMessage::new(
            Domain::SignaturePreimage,
            self.protocol_version,
            self.network_id,
            &canonical,
        )
        .map_err(|_| DisclosureError::MalformedPayload)?;
        crate::ed25519::verify(&public_key, &signature, message)
            .map_err(|_| DisclosureError::MalformedPayload)?;
        let mut e = Encoder::new(512);
        e.u16(0x5301)?;
        e.u16(10)?;
        e.fixed(&self.common()?)?;
        e.u8(self.authorization_kind)?;
        e.fixed(&self.from)?;
        e.fixed(&public_key)?;
        e.fixed(&signature)?;
        e.fixed(&self.context_hash)?;
        e.u32(self.network_id)?;
        e.u16(self.protocol_version)?;
        Ok(e.finish())
    }
}

pub struct EnvelopeOptions<'a> {
    pub actor: &'a str,
    pub public_key: [u8; 32],
    pub protocol_version: u16,
    pub network_id: u32,
    pub identity_sequence: u64,
    pub idempotency_key: [u8; 32],
    pub fee_limit: u128,
    pub not_before: u64,
    pub not_after: u64,
}

pub struct DisclosedEnvelope {
    pub envelope: UnsignedEnvelope,
    pub registry: ModuleRegistry,
    pub canonical: Vec<u8>,
    pub disclosure: Disclosure,
}

/// # Errors
/// Rejects malformed payloads and preserves independent source and identity sequences.
pub fn encode_send_envelope(
    payload: &[u8],
    options: &EnvelopeOptions<'_>,
) -> Result<DisclosedEnvelope, DisclosureError> {
    encode_payment_envelope(ModuleId::Asset, 5, payload, options)
}

/// # Errors
/// Rejects unsupported types, malformed envelopes, signatures and incomplete disclosures.
pub fn encode_payment_envelope(
    module: ModuleId,
    ordinal: u16,
    payload: &[u8],
    options: &EnvelopeOptions<'_>,
) -> Result<DisclosedEnvelope, DisclosureError> {
    fn bad<E>(_: E) -> DisclosureError {
        DisclosureError::MalformedPayload
    }
    let kind = ActivityType::new(module, ordinal).map_err(bad)?;
    let registration = ModuleRegistration::new(module, &[kind]).map_err(bad)?;
    let registry = ModuleRegistry::new(&[registration]).map_err(bad)?;
    let mut hash = Sha256::new();
    hash.update(Domain::PayloadHash.tag());
    hash.update(payload);
    let mut builder = EnvelopeBuilder::new();
    builder
        .protocol_version(options.protocol_version)
        .map_err(bad)?;
    builder.network_id(options.network_id).map_err(bad)?;
    builder.activity_type(kind).map_err(bad)?;
    builder
        .actor_did(Did::new(options.actor.as_bytes()).map_err(bad)?)
        .map_err(bad)?;
    builder
        .authority(Authority::owner(&options.public_key).map_err(bad)?)
        .map_err(bad)?;
    builder
        .account_sequence(options.identity_sequence)
        .map_err(bad)?;
    builder
        .timestamp_bound(TimestampBound::new(options.not_before, options.not_after).map_err(bad)?)
        .map_err(bad)?;
    builder
        .idempotency_key(IdempotencyKey::new(options.idempotency_key))
        .map_err(bad)?;
    builder
        .fee_limit(Amount::from_u128(options.fee_limit))
        .map_err(bad)?;
    builder.payload_hash(hash.finalize().into()).map_err(bad)?;
    builder
        .payload(Payload::new(&registry, kind, payload).map_err(bad)?)
        .map_err(bad)?;
    let envelope = builder.build().map_err(bad)?;
    let canonical = layerx_wire::activity::encode_unsigned_envelope(&envelope)?;
    let disclosure = bind(&canonical, &registry)?;
    Ok(DisclosedEnvelope {
        envelope,
        registry,
        canonical,
        disclosure,
    })
}

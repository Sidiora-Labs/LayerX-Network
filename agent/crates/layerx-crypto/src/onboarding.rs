use layerx_types::account::AccountId;
use layerx_types::ids::Did;
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_wire::activity::{self, Activity};
use layerx_wire::decode::Decoder;
use layerx_wire::encode::Encoder;
use layerx_wire::hash::{self, Domain};

use crate::{ed25519, SignatureMessage};

const CONSENT_HEADER: [u8; 4] = [0x71, 1, 3, 5];
const REGISTRATION_HEADER: [u8; 4] = [0x71, 1, 2, 1];
const CONSENT_BYTES: usize = 140;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OnboardingError {
    Encoding,
    Binding,
    Signature,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OnboardingConsent {
    pub sponsor: [u8; 32],
    pub target: Did,
    pub target_public_key: [u8; 32],
    pub native_asset: [u8; 32],
    pub action_key: [u8; 32],
    pub expires_at: u64,
}

impl OnboardingConsent {
    /// # Errors
    /// Refuses invalid target ownership, native asset, sponsor or consent bounds.
    pub fn payload(&self) -> Result<Vec<u8>, OnboardingError> {
        let target = hash::did_id_for_protocol(&self.target, 3).map_err(encoding)?;
        if self.sponsor == [0; 32]
            || self.sponsor == target
            || self.native_asset == [0; 32]
            || self.action_key == [0; 32]
            || self.expires_at == 0
            || !ed25519::public_key_is_canonical(&self.target_public_key)
        {
            return Err(OnboardingError::Binding);
        }
        let mut encoded = Encoder::new(CONSENT_BYTES);
        encoded.fixed(&CONSENT_HEADER).map_err(encoding)?;
        encoded.fixed(&self.sponsor).map_err(encoding)?;
        encoded.fixed(&self.native_asset).map_err(encoding)?;
        encoded.fixed(&self.target_account_id()?).map_err(encoding)?;
        encoded.fixed(&self.action_key).map_err(encoding)?;
        encoded.u64(self.expires_at).map_err(encoding)?;
        Ok(encoded.finish())
    }

    /// # Errors
    /// Refuses a target DID which cannot name its canonical native MAIN account.
    pub fn target_account_id(&self) -> Result<[u8; 32], OnboardingError> {
        let did = std::str::from_utf8(self.target.as_bytes()).map_err(|_| OnboardingError::Binding)?;
        let account = AccountId::parse(&format!("agent:{did}:main"))
            .map_err(|_| OnboardingError::Binding)?;
        hash::account_id_for_protocol(&account, 3).map_err(encoding)
    }

    /// # Errors
    /// Refuses unknown versions, extra bytes and any unbound target account or key.
    pub fn decode_payload(
        payload: &[u8],
        target: Did,
        target_public_key: [u8; 32],
    ) -> Result<Self, OnboardingError> {
        if payload.len() != CONSENT_BYTES || payload[..4] != CONSENT_HEADER {
            return Err(OnboardingError::Encoding);
        }
        let mut reader = Decoder::new(&payload[4..], 1024);
        let sponsor = reader.fixed(32).map_err(encoding)?.try_into().map_err(|_| OnboardingError::Encoding)?;
        let native_asset = reader.fixed(32).map_err(encoding)?.try_into().map_err(|_| OnboardingError::Encoding)?;
        let account: [u8; 32] = reader.fixed(32).map_err(encoding)?.try_into().map_err(|_| OnboardingError::Encoding)?;
        let action_key = reader.fixed(32).map_err(encoding)?.try_into().map_err(|_| OnboardingError::Encoding)?;
        let expires_at = reader.u64().map_err(encoding)?;
        reader.finish().map_err(encoding)?;
        let consent = Self { sponsor, target, target_public_key, native_asset, action_key, expires_at };
        if consent.target_account_id()? != account || consent.payload()? != payload {
            return Err(OnboardingError::Binding);
        }
        Ok(consent)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SponsoredRegistration {
    pub consent: OnboardingConsent,
    pub network_id: u32,
    pub not_before: u64,
    signed_consent: Vec<u8>,
}

impl SponsoredRegistration {
    /// # Errors
    /// Refuses anything other than the original owner-signed, signing-only native consent.
    pub fn from_signed_consent(bytes: &[u8]) -> Result<Self, OnboardingError> {
        if bytes.len() > 1016 {
            return Err(OnboardingError::Encoding);
        }
        let activity = activity::decode_signed(bytes, &registry()?).map_err(encoding)?;
        if activity::encode_signed(&activity).map_err(encoding)? != bytes
            || activity.protocol_version() != 3
            || activity.network_id() == 0
            || activity.account_sequence() != 0
            || activity.fee_limit() != 0
            || hash::payload_hash(&activity).map_err(encoding)? != activity.payload_hash()
        {
            return Err(OnboardingError::Binding);
        }
        let key = activity.authority().try_into().map_err(|_| OnboardingError::Binding)?;
        let target = Did::new(activity.actor_did()).map_err(|_| OnboardingError::Binding)?;
        let consent = OnboardingConsent::decode_payload(activity.payload(), target, key)?;
        let bound = activity.timestamp_bound();
        if activity.idempotency_key() != consent.action_key
            || bound.not_after != consent.expires_at
            || bound.not_before >= bound.not_after
        {
            return Err(OnboardingError::Binding);
        }
        let unsigned = activity::encode_unsigned(&activity).map_err(encoding)?;
        let signature = activity.signature().ok_or(OnboardingError::Signature)?
            .try_into().map_err(|_| OnboardingError::Signature)?;
        let message = SignatureMessage::new(Domain::SignaturePreimage, 3, activity.network_id(), &unsigned)
            .map_err(|_| OnboardingError::Signature)?;
        ed25519::verify(&key, signature, message).map_err(|_| OnboardingError::Signature)?;
        Ok(Self { consent, network_id: activity.network_id(), not_before: bound.not_before, signed_consent: bytes.to_vec() })
    }

    /// # Errors
    /// Refuses unknown registration versions and malformed or unauthenticated nested consent.
    pub fn decode(payload: &[u8]) -> Result<Self, OnboardingError> {
        if payload.len() < 8 || payload.len() > 1024 || payload[..4] != REGISTRATION_HEADER {
            return Err(OnboardingError::Encoding);
        }
        let mut reader = Decoder::new(&payload[4..], 1024);
        let encoded = reader.bytes(1016).map_err(encoding)?;
        reader.finish().map_err(encoding)?;
        Self::from_signed_consent(encoded)
    }

    /// # Errors
    /// Refuses changed public consent facts before encoding the original signed bytes.
    pub fn payload(&self) -> Result<Vec<u8>, OnboardingError> {
        if Self::from_signed_consent(&self.signed_consent)? != *self {
            return Err(OnboardingError::Binding);
        }
        let mut encoded = Encoder::new(1024);
        encoded.fixed(&REGISTRATION_HEADER).map_err(encoding)?;
        encoded.bytes(&self.signed_consent, 1016).map_err(encoding)?;
        Ok(encoded.finish())
    }

    #[must_use]
    pub fn signed_consent(&self) -> &[u8] { &self.signed_consent }

    /// # Errors
    /// Refuses a different sponsor, network, action key, operation or validity interval.
    pub fn validate_outer(&self, outer: &Activity) -> Result<(), OnboardingError> {
        let sponsor = Did::new(outer.actor_did()).map_err(|_| OnboardingError::Binding)?;
        let bound = outer.timestamp_bound();
        if outer.protocol_version() != 3
            || outer.network_id() != self.network_id
            || outer.activity_type().value() != 0x0007_0001
            || outer.authority().len() != 32
            || hash::did_id_for_protocol(&sponsor, 3).map_err(encoding)? != self.consent.sponsor
            || outer.idempotency_key() != self.consent.action_key
            || bound.not_before < self.not_before
            || bound.not_after > self.consent.expires_at
            || bound.not_before >= bound.not_after
            || outer.payload() != self.payload()?
        {
            return Err(OnboardingError::Binding);
        }
        Ok(())
    }
}

fn registry() -> Result<ModuleRegistry, OnboardingError> {
    let kind = ActivityType::new(ModuleId::Governance, 1).map_err(|_| OnboardingError::Encoding)?;
    let module = ModuleRegistration::new(ModuleId::Governance, &[kind])
        .map_err(|_| OnboardingError::Encoding)?;
    ModuleRegistry::new(&[module]).map_err(|_| OnboardingError::Encoding)
}

const fn encoding(_: layerx_wire::WireError) -> OnboardingError { OnboardingError::Encoding }

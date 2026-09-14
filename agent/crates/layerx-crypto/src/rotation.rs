use layerx_types::ids::Did;
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_wire::activity::{self, Activity};
use layerx_wire::decode::Decoder;
use layerx_wire::encode::Encoder;
use layerx_wire::hash::{self, Domain};

use crate::{ed25519, SignatureMessage};
use sha2::{Digest as _, Sha256};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerRotationState {
    pub primary_public_key: [u8; 32],
    pub pending_public_key: Option<[u8; 32]>,
    pub begin: u64,
    pub end: u64,
    pub effective_sequence: u64,
    pub revocation_sequence: u64,
    pub observed_sequence: u64,
    pub announcement: [u8; 32],
}

impl OwnerRotationState {
    /// # Errors
    /// Refuses a malformed native identity or a different owner binding.
    pub fn decode(bytes: &[u8], owner: &Did) -> Result<Self, RotationError> {
        if bytes.len() != 223
            || &bytes[..5] != b"LXGI1"
            || bytes[5..37] != hash::did_id_for_protocol(owner, 3).map_err(encoding)?
        {
            return Err(RotationError::Binding);
        }
        let primary_public_key = bytes[37..69]
            .try_into()
            .map_err(|_| RotationError::Encoding)?;
        let pending: [u8; 32] = bytes[111..143]
            .try_into()
            .map_err(|_| RotationError::Encoding)?;
        let number = |offset| -> Result<u64, RotationError> {
            Ok(u64::from_be_bytes(
                bytes[offset..offset + 8]
                    .try_into()
                    .map_err(|_| RotationError::Encoding)?,
            ))
        };
        let begin = number(143)?;
        let end = number(151)?;
        let effective_sequence = number(159)?;
        let revocation_sequence = number(69)?;
        let observed_sequence = number(215)?;
        if !ed25519::public_key_is_canonical(&primary_public_key)
            || revocation_sequence == 0
            || observed_sequence < revocation_sequence
            || (pending != [0; 32]
                && (!ed25519::public_key_is_canonical(&pending)
                    || pending == primary_public_key
                    || begin == 0
                    || end <= begin
                    || effective_sequence == 0))
        {
            return Err(RotationError::Binding);
        }
        let mut digest = Sha256::new();
        digest.update(Domain::ContextHash.tag());
        digest.update(bytes);
        Ok(Self {
            primary_public_key,
            pending_public_key: (pending != [0; 32]).then_some(pending),
            begin,
            end,
            effective_sequence,
            revocation_sequence,
            observed_sequence,
            announcement: digest.finalize().into(),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RotationError {
    Encoding,
    Binding,
    Signature,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerRotationConsent {
    pub owner: Did,
    pub current_public_key: [u8; 32],
    pub pending_public_key: [u8; 32],
    pub announcement: [u8; 32],
    pub action_key: [u8; 32],
    pub expires_at: u64,
}

impl OwnerRotationConsent {
    /// # Errors
    /// Refuses unknown keys, identity, state commitment or consent bounds.
    pub fn payload(&self) -> Result<Vec<u8>, RotationError> {
        if !ed25519::public_key_is_canonical(&self.current_public_key)
            || !ed25519::public_key_is_canonical(&self.pending_public_key)
            || self.current_public_key == self.pending_public_key
            || self.announcement == [0; 32]
            || self.action_key == [0; 32]
            || self.expires_at == 0
        {
            return Err(RotationError::Binding);
        }
        let mut out = Encoder::new(140);
        out.fixed(&[0x71, 2, 2, 5]).map_err(encoding)?;
        out.fixed(&hash::did_id_for_protocol(&self.owner, 3).map_err(encoding)?)
            .map_err(encoding)?;
        out.fixed(&self.current_public_key).map_err(encoding)?;
        out.fixed(&self.announcement).map_err(encoding)?;
        out.fixed(&self.action_key).map_err(encoding)?;
        out.u64(self.expires_at).map_err(encoding)?;
        Ok(out.finish())
    }

    /// # Errors
    /// Refuses any noncanonical field or different owner and pending key binding.
    pub fn decode(
        payload: &[u8],
        owner: Did,
        pending_public_key: [u8; 32],
    ) -> Result<Self, RotationError> {
        if payload.len() != 140 || payload[..4] != [0x71, 2, 2, 5] {
            return Err(RotationError::Encoding);
        }
        let mut reader = Decoder::new(&payload[4..], 1024);
        let did = fixed(&mut reader)?;
        let value = Self {
            owner,
            pending_public_key,
            current_public_key: fixed(&mut reader)?,
            announcement: fixed(&mut reader)?,
            action_key: fixed(&mut reader)?,
            expires_at: reader.u64().map_err(encoding)?,
        };
        reader.finish().map_err(encoding)?;
        if hash::did_id_for_protocol(&value.owner, 3).map_err(encoding)? != did
            || value.payload()? != payload
        {
            return Err(RotationError::Binding);
        }
        Ok(value)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerRotationCommit {
    pub consent: OwnerRotationConsent,
    pub network_id: u32,
    pub not_before: u64,
    signed_consent: Vec<u8>,
}

impl OwnerRotationCommit {
    /// # Errors
    /// Refuses any envelope not signed by the exact prospective owner key.
    pub fn from_signed_consent(bytes: &[u8]) -> Result<Self, RotationError> {
        if bytes.len() > 1016 {
            return Err(RotationError::Encoding);
        }
        let activity = activity::decode_signed(bytes, &registry()?).map_err(encoding)?;
        if activity::encode_signed(&activity).map_err(encoding)? != bytes
            || activity.protocol_version() != 3
            || activity.network_id() == 0
            || activity.account_sequence() != 0
            || activity.fee_limit() != 0
            || hash::payload_hash(&activity).map_err(encoding)? != activity.payload_hash()
        {
            return Err(RotationError::Binding);
        }
        let key = activity
            .authority()
            .try_into()
            .map_err(|_| RotationError::Binding)?;
        let owner = Did::new(activity.actor_did()).map_err(|_| RotationError::Binding)?;
        let consent = OwnerRotationConsent::decode(activity.payload(), owner, key)?;
        let bound = activity.timestamp_bound();
        if bound.not_before >= bound.not_after
            || bound.not_after != consent.expires_at
            || activity.idempotency_key() != consent.action_key
        {
            return Err(RotationError::Binding);
        }
        let unsigned = activity::encode_unsigned(&activity).map_err(encoding)?;
        let signature = activity
            .signature()
            .ok_or(RotationError::Signature)?
            .try_into()
            .map_err(|_| RotationError::Signature)?;
        let message = SignatureMessage::new(
            Domain::SignaturePreimage,
            3,
            activity.network_id(),
            &unsigned,
        )
        .map_err(|_| RotationError::Signature)?;
        ed25519::verify(&key, signature, message).map_err(|_| RotationError::Signature)?;
        Ok(Self {
            consent,
            network_id: activity.network_id(),
            not_before: bound.not_before,
            signed_consent: bytes.to_vec(),
        })
    }

    /// # Errors
    /// Refuses unsupported versions and any trailing or unauthenticated consent bytes.
    pub fn decode(payload: &[u8]) -> Result<Self, RotationError> {
        if payload.len() < 8 || payload.len() > 1024 || payload[..4] != [0x71, 2, 1, 1] {
            return Err(RotationError::Encoding);
        }
        let mut reader = Decoder::new(&payload[4..], 1024);
        let consent = reader.bytes(1016).map_err(encoding)?;
        reader.finish().map_err(encoding)?;
        Self::from_signed_consent(consent)
    }

    /// # Errors
    /// Refuses metadata differing from the original signed consent.
    pub fn payload(&self) -> Result<Vec<u8>, RotationError> {
        if Self::from_signed_consent(&self.signed_consent)? != *self {
            return Err(RotationError::Binding);
        }
        let mut out = Encoder::new(1024);
        out.fixed(&[0x71, 2, 1, 1]).map_err(encoding)?;
        out.bytes(&self.signed_consent, 1016).map_err(encoding)?;
        Ok(out.finish())
    }

    #[must_use]
    pub fn signed_consent(&self) -> &[u8] {
        &self.signed_consent
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OwnerRotation {
    Announce {
        owner: Did,
        pending_public_key: [u8; 32],
        begin: u64,
        end: u64,
        effective_sequence: u64,
    },
    Consent(OwnerRotationConsent),
    Commit(OwnerRotationCommit),
    Cancel {
        owner: Did,
        announcement: [u8; 32],
    },
}

impl OwnerRotation {
    #[must_use]
    pub fn owner(&self) -> &Did {
        match self {
            Self::Announce { owner, .. } | Self::Cancel { owner, .. } => owner,
            Self::Consent(consent) => &consent.owner,
            Self::Commit(commit) => &commit.consent.owner,
        }
    }

    /// # Errors
    /// Refuses a noncanonical key, identity, challenge window or state commitment.
    pub fn payload(&self) -> Result<Vec<u8>, RotationError> {
        let mut out = Encoder::new(1024);
        match self {
            Self::Consent(consent) => return consent.payload(),
            Self::Commit(commit) => return commit.payload(),
            Self::Announce {
                owner,
                pending_public_key,
                begin,
                end,
                effective_sequence,
            } => {
                if !ed25519::public_key_is_canonical(pending_public_key)
                    || *begin == 0
                    || end <= begin
                    || *effective_sequence == 0
                {
                    return Err(RotationError::Binding);
                }
                out.fixed(&[0x71, 2, 0, 4]).map_err(encoding)?;
                out.fixed(&hash::did_id_for_protocol(owner, 3).map_err(encoding)?)
                    .map_err(encoding)?;
                out.fixed(pending_public_key).map_err(encoding)?;
                out.u64(*begin).map_err(encoding)?;
                out.u64(*end).map_err(encoding)?;
                out.u64(*effective_sequence).map_err(encoding)?;
            }
            Self::Cancel {
                owner,
                announcement,
            } => {
                if *announcement == [0; 32] {
                    return Err(RotationError::Binding);
                }
                out.fixed(&[0x71, 2, 3, 2]).map_err(encoding)?;
                out.fixed(&hash::did_id_for_protocol(owner, 3).map_err(encoding)?)
                    .map_err(encoding)?;
                out.fixed(announcement).map_err(encoding)?;
            }
        }
        Ok(out.finish())
    }

    /// # Errors
    /// Refuses unsupported payload forms and unbound native owner semantics.
    pub fn decode(payload: &[u8], owner: Did, authority: [u8; 32]) -> Result<Self, RotationError> {
        if payload.len() < 4 || payload.len() > 1024 || payload[..2] != [0x71, 2] {
            return Err(RotationError::Encoding);
        }
        if payload[2] == 1 {
            let commit = OwnerRotationCommit::decode(payload)?;
            if commit.consent.owner != owner || commit.consent.current_public_key != authority {
                return Err(RotationError::Binding);
            }
            return Ok(Self::Commit(commit));
        }
        if payload[2] == 2 {
            return Ok(Self::Consent(OwnerRotationConsent::decode(
                payload, owner, authority,
            )?));
        }
        let mut reader = Decoder::new(&payload[4..], 1024);
        let did = fixed(&mut reader)?;
        if hash::did_id_for_protocol(&owner, 3).map_err(encoding)? != did {
            return Err(RotationError::Binding);
        }
        let value = match (payload[2], payload[3]) {
            (0, 4) => Self::Announce {
                owner,
                pending_public_key: fixed(&mut reader)?,
                begin: reader.u64().map_err(encoding)?,
                end: reader.u64().map_err(encoding)?,
                effective_sequence: reader.u64().map_err(encoding)?,
            },
            (3, 2) => Self::Cancel {
                owner,
                announcement: fixed(&mut reader)?,
            },
            _ => return Err(RotationError::Encoding),
        };
        reader.finish().map_err(encoding)?;
        if value.payload()? != payload {
            return Err(RotationError::Binding);
        }
        Ok(value)
    }

    /// # Errors
    /// Refuses an operation, network, payer, action, key or timestamp not bound by its original consent.
    pub fn from_activity(activity: &Activity) -> Result<Self, RotationError> {
        let owner = Did::new(activity.actor_did()).map_err(|_| RotationError::Binding)?;
        let authority = activity
            .authority()
            .try_into()
            .map_err(|_| RotationError::Binding)?;
        if activity.protocol_version() != 3
            || activity.network_id() == 0
            || activity.activity_type().value() != 0x0007_0002
            || !ed25519::public_key_is_canonical(&authority)
        {
            return Err(RotationError::Binding);
        }
        let value = Self::decode(activity.payload(), owner, authority)?;
        let bound = activity.timestamp_bound();
        if value.owner().as_bytes() != activity.actor_did() || bound.not_before >= bound.not_after {
            return Err(RotationError::Binding);
        }
        match &value {
            Self::Announce {
                pending_public_key, ..
            } if *pending_public_key == authority => return Err(RotationError::Binding),
            Self::Consent(consent) => {
                if activity.account_sequence() != 0
                    || activity.fee_limit() != 0
                    || activity.idempotency_key() != consent.action_key
                    || bound.not_after != consent.expires_at
                {
                    return Err(RotationError::Binding);
                }
            }
            Self::Commit(commit) => {
                if activity.network_id() != commit.network_id
                    || authority != commit.consent.current_public_key
                    || activity.idempotency_key() != commit.consent.action_key
                    || bound.not_before < commit.not_before
                    || bound.not_after > commit.consent.expires_at
                {
                    return Err(RotationError::Binding);
                }
            }
            _ => {}
        }
        Ok(value)
    }
}

fn fixed(reader: &mut Decoder<'_>) -> Result<[u8; 32], RotationError> {
    reader
        .fixed(32)
        .map_err(encoding)?
        .try_into()
        .map_err(|_| RotationError::Encoding)
}
fn registry() -> Result<ModuleRegistry, RotationError> {
    let activity =
        ActivityType::new(ModuleId::Governance, 2).map_err(|_| RotationError::Encoding)?;
    let module = ModuleRegistration::new(ModuleId::Governance, &[activity])
        .map_err(|_| RotationError::Encoding)?;
    ModuleRegistry::new(&[module]).map_err(|_| RotationError::Encoding)
}
const fn encoding(_: layerx_wire::WireError) -> RotationError {
    RotationError::Encoding
}

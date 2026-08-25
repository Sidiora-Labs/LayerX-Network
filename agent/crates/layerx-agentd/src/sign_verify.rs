//! Exact-byte signature verification gate before submission.

use layerx_crypto::{ed25519, SignatureMessage};
use layerx_types::activity::Signature;
use layerx_types::payload::ModuleRegistry;
use layerx_wire::activity::{
    decode_signed, encode_signed, encode_signed_envelope, encode_unsigned,
};
use layerx_wire::hash::{activity_id, Domain};

use crate::prepare::{verify_disclosure_binding, Prepared};

use super::SigningError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SubmissionBindingAudit {
    pub activity_id: [u8; 32],
    pub idempotency_key: [u8; 32],
    pub signed_byte_length: usize,
}

/// Bytes admitted to submission only after exact signature verification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedSubmission {
    signed_canonical_bytes: Vec<u8>,
    audit: SubmissionBindingAudit,
}

impl VerifiedSubmission {
    #[must_use]
    pub fn exact_bytes(&self) -> &[u8] {
        &self.signed_canonical_bytes
    }

    #[must_use]
    pub fn into_exact_bytes(self) -> Vec<u8> {
        self.signed_canonical_bytes
    }

    #[must_use]
    pub const fn activity_id(&self) -> [u8; 32] {
        self.audit.activity_id
    }

    #[must_use]
    pub const fn idempotency_key(&self) -> [u8; 32] {
        self.audit.idempotency_key
    }

    #[must_use]
    pub const fn signed_byte_length(&self) -> usize {
        self.audit.signed_byte_length
    }
}

pub(crate) fn attach_signature(
    prepared: &Prepared,
    signature: [u8; 64],
) -> Result<Vec<u8>, SigningError> {
    let signature = Signature::new(&signature).map_err(SigningError::SignatureEncoding)?;
    let signed = prepared.envelope.clone().attach_signature(signature);
    encode_signed_envelope(&signed).map_err(SigningError::Wire)
}

pub(crate) fn verify_exact(
    signed_canonical_bytes: &[u8],
    prepared: &Prepared,
    signer_public_key: &[u8; 32],
    registry: &ModuleRegistry,
) -> Result<VerifiedSubmission, SigningError> {
    verify_disclosure_binding(prepared).map_err(SigningError::Disclosure)?;
    let activity = decode_signed(signed_canonical_bytes, registry).map_err(SigningError::Wire)?;
    if encode_signed(&activity).map_err(SigningError::Wire)? != signed_canonical_bytes {
        return Err(SigningError::PreparedBytesChanged);
    }
    let unsigned = encode_unsigned(&activity).map_err(SigningError::Wire)?;
    if unsigned != prepared.canonical_bytes {
        return Err(SigningError::PreparedBytesChanged);
    }
    let signature = activity.signature().ok_or(SigningError::SignatureInvalid)?;
    let signature: &[u8; 64] = signature
        .try_into()
        .map_err(|_| SigningError::SignatureInvalid)?;
    let message = SignatureMessage::new(
        Domain::SignaturePreimage,
        activity.protocol_version(),
        activity.network_id(),
        &unsigned,
    )
    .map_err(|_| SigningError::SignatureInvalid)?;
    ed25519::verify(signer_public_key, signature, message)
        .map_err(|_| SigningError::SignatureInvalid)?;
    let activity_id = activity_id(&activity).map_err(SigningError::Wire)?;
    Ok(VerifiedSubmission {
        signed_canonical_bytes: signed_canonical_bytes.to_vec(),
        audit: SubmissionBindingAudit {
            activity_id,
            idempotency_key: activity.idempotency_key(),
            signed_byte_length: signed_canonical_bytes.len(),
        },
    })
}

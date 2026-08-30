//! External-by-default signing and bounded protocol session-key self-signing.

use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::PathBuf;

use layerx_crypto::local::LocalSigner;
use layerx_crypto::session::{issue_session_key, IssuedSessionKey, SessionKeyRequest};
use layerx_crypto::signer::{sign_disclosed, SignError, Signer};
use layerx_types::activity::{ActivityBuildError, Authority};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistry};
use layerx_wire::decode::Decoder;
use layerx_wire::WireError;

use crate::prepare::{
    verify_disclosure_binding, DisclosureBindingError, DisclosureDigest, Prepared,
};

#[path = "sign_verify.rs"]
mod verification;

pub use verification::{SubmissionBindingAudit, VerifiedSubmission};

/// Signing location made explicit in every response and audit record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SigningMode {
    External,
    ProtocolSessionKey,
}

/// Key-free package returned for signing outside the daemon.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalSigningPackage {
    pub canonical_bytes: Vec<u8>,
    pub signing_preimage: [u8; 32],
    pub disclosure_digest: DisclosureDigest,
    pub mode: SigningMode,
}

/// Distinct audit evidence for one self-signed preparation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelfSigningAudit {
    pub mode: SigningMode,
    pub session_public_key: [u8; 32],
    pub activity_type: ActivityType,
    pub disclosure_digest: DisclosureDigest,
    pub authority_bytes: Vec<u8>,
    pub expires_at: u64,
    pub revocation_sequence: u64,
}

/// Output from a bounded in-daemon protocol session key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignedPreparation {
    pub canonical_bytes: Vec<u8>,
    pub signed_canonical_bytes: Vec<u8>,
    pub signature: [u8; 64],
    pub signer_public_key: [u8; 32],
    pub mode: SigningMode,
    pub audit: SelfSigningAudit,
}

/// Provisioned signer whose scope is the exact issued protocol authority.
pub struct ProvisionedSessionKey {
    signer: LocalSigner,
    authority: Authority,
    permitted_activity_types: BTreeSet<ActivityType>,
    expires_at: u64,
    revocation_sequence: u64,
    revocation_marker: Option<(PathBuf, u32)>,
}

impl std::fmt::Debug for ProvisionedSessionKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProvisionedSessionKey")
            .field("signer", &"[redacted]")
            .field("expires_at", &self.expires_at)
            .field("revocation_sequence", &self.revocation_sequence)
            .finish_non_exhaustive()
    }
}

impl ProvisionedSessionKey {
    /// Reconstructs a provisioned signer while an encrypted keystore lends
    /// its zeroizing plaintext seed to the caller.
    ///
    /// # Errors
    ///
    /// Returns the same exact-scope validation failures as [`Self::new`].
    pub fn from_seed(seed: &[u8; 32], issued: IssuedSessionKey) -> Result<Self, SigningError> {
        Self::new(*seed, issued)
    }

    /// Imports key material only when it matches an issued protocol session authority.
    ///
    /// # Errors
    ///
    /// Returns `KeyMismatch` when the seed derives a different public key, and
    /// `InvalidProvisioning` for an empty permitted activity set or a zero expiry or revocation
    /// sequence.
    pub fn new(seed: [u8; 32], issued: IssuedSessionKey) -> Result<Self, SigningError> {
        let signer = LocalSigner::new(seed);
        if signer.public_key() != issued.session_public_key {
            return Err(SigningError::KeyMismatch);
        }
        if issued.permitted_activity_types.is_empty()
            || issued.expires_at == 0
            || issued.revocation_sequence == 0
        {
            return Err(SigningError::InvalidProvisioning);
        }
        Ok(Self {
            signer,
            authority: issued.authority,
            permitted_activity_types: issued.permitted_activity_types.into_iter().collect(),
            expires_at: issued.expires_at,
            revocation_sequence: issued.revocation_sequence,
            revocation_marker: None,
        })
    }

    pub(crate) fn bind_revocation_marker(mut self, marker: PathBuf, owner_uid: u32) -> Self {
        self.revocation_marker = Some((marker, owner_uid));
        self
    }

    fn ensure_not_durably_revoked(&self) -> Result<(), SigningError> {
        let Some((marker, owner_uid)) = &self.revocation_marker else {
            return Ok(());
        };
        match fs::symlink_metadata(marker) {
            Ok(metadata) => {
                if !metadata.file_type().is_file()
                    || metadata.file_type().is_symlink()
                    || metadata.uid() != *owner_uid
                    || metadata.permissions().mode() & 0o077 != 0
                {
                    return Err(SigningError::Revoked);
                }
                let bytes = fs::read(marker).map_err(|_| SigningError::Revoked)?;
                if bytes == b"LXSRV1" {
                    Err(SigningError::Revoked)
                } else {
                    Err(SigningError::Revoked)
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(SigningError::Revoked),
        }
    }
}

/// Reconstructs and validates the exact issued session grant transferred by Human.
///
/// The daemon's existing permission registry is ordinal-based, while the
/// canonical protocol grant independently carries its complete module mask.
/// This function requires the transferred ordinals to equal the canonical
/// range and then reissues the complete module/ordinal Cartesian scope before
/// accepting byte-for-byte equality.
pub fn validate_issued_session(
    registration_payload: &[u8],
    grantor: [u8; 32],
    session_public_key: [u8; 32],
    not_before: u64,
    expires_at: u64,
    revocation_sequence: u64,
    permitted_ordinals: &[u16],
) -> Result<IssuedSessionKey, SigningError> {
    let mut decoder = Decoder::new(registration_payload, 0);
    decoder
        .structure_header(0x2001)
        .map_err(SigningError::Wire)?;
    if decoder.u8().map_err(SigningError::Wire)? != 1
        || decoder.bytes(32).map_err(SigningError::Wire)? != grantor
        || decoder.bytes(32).map_err(SigningError::Wire)? != grantor
        || decoder.u8().map_err(SigningError::Wire)? != 2
        || decoder.bytes(32).map_err(SigningError::Wire)? != session_public_key
    {
        return Err(SigningError::InvalidProvisioning);
    }
    let module_mask = decoder.u64().map_err(SigningError::Wire)?;
    let ordinal_min = decoder.u16().map_err(SigningError::Wire)?;
    let ordinal_max = decoder.u16().map_err(SigningError::Wire)?;
    let zero32 = [0_u8; 32];
    if module_mask == 0
        || ordinal_min == 0
        || ordinal_max < ordinal_min
        || decoder.bytes(32).map_err(SigningError::Wire)? != zero32
        || decoder.u128().map_err(SigningError::Wire)? != 0
        || decoder.u128().map_err(SigningError::Wire)? != 0
        || decoder.u128().map_err(SigningError::Wire)? != 0
        || decoder.u64().map_err(SigningError::Wire)? != 0
        || decoder.u128().map_err(SigningError::Wire)? != 0
        || decoder.u128().map_err(SigningError::Wire)? != 0
        || decoder.u64().map_err(SigningError::Wire)? != 0
        || decoder.bytes(32).map_err(SigningError::Wire)? != zero32
        || decoder.u64().map_err(SigningError::Wire)? != not_before
        || decoder.u64().map_err(SigningError::Wire)? != expires_at
        || decoder.u64().map_err(SigningError::Wire)? != revocation_sequence
        || decoder.u8().map_err(SigningError::Wire)? != 0
        || decoder.u64().map_err(SigningError::Wire)? != 0
        || decoder.bytes(64).map_err(SigningError::Wire)? != [0_u8; 64]
    {
        return Err(SigningError::InvalidProvisioning);
    }
    decoder.finish().map_err(SigningError::Wire)?;

    let expected_ordinals: BTreeSet<_> = (ordinal_min..=ordinal_max).collect();
    let transferred_ordinals: BTreeSet<_> = permitted_ordinals.iter().copied().collect();
    if permitted_ordinals.len() != transferred_ordinals.len()
        || transferred_ordinals != expected_ordinals
    {
        return Err(SigningError::InvalidProvisioning);
    }
    let mut activities = Vec::new();
    let mut known_mask = 0_u64;
    for module_value in 1_u16..64 {
        let bit = 1_u64 << module_value;
        if module_mask & bit == 0 {
            continue;
        }
        let module =
            ModuleId::from_u16(module_value).map_err(|_| SigningError::InvalidProvisioning)?;
        known_mask |= bit;
        for ordinal in ordinal_min..=ordinal_max {
            activities.push(
                ActivityType::new(module, ordinal)
                    .map_err(|_| SigningError::InvalidProvisioning)?,
            );
        }
    }
    if known_mask != module_mask {
        return Err(SigningError::InvalidProvisioning);
    }
    let issued = issue_session_key(&SessionKeyRequest {
        grantor,
        session_public_key,
        not_before,
        expires_at: Some(expires_at),
        permitted_activity_types: activities,
        revocation_sequence: Some(revocation_sequence),
    })
    .map_err(|_| SigningError::InvalidProvisioning)?;
    if issued.registration_payload != registration_payload {
        return Err(SigningError::InvalidProvisioning);
    }
    Ok(issued)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SigningError {
    Disclosure(DisclosureBindingError),
    NotProvisioned,
    InvalidProvisioning,
    KeyMismatch,
    ScopeDenied,
    Expired,
    Revoked,
    AuthorityMismatch,
    Crypto(SignError),
    SignatureEncoding(ActivityBuildError),
    Wire(WireError),
    PreparedBytesChanged,
    SignatureInvalid,
}

/// Returns the default key-free package for an external signer.
///
/// # Errors
///
/// Returns the disclosure binding failure when the preparation no longer re-encodes to its exact
/// canonical bytes and digest.
pub fn external(prepared: &Prepared) -> Result<ExternalSigningPackage, SigningError> {
    verify_disclosure_binding(prepared).map_err(SigningError::Disclosure)?;
    Ok(ExternalSigningPackage {
        canonical_bytes: prepared.canonical_bytes.clone(),
        signing_preimage: prepared.signing_preimage,
        disclosure_digest: prepared.disclosure_digest,
        mode: SigningMode::External,
    })
}

/// Signs only through an explicitly provisioned, unexpired protocol session key.
///
/// # Errors
///
/// Refuses an absent, expired, or revoked session key, an activity type outside its permitted set,
/// and an authority other than the issued one. Also returns the disclosure, signing, encoding, and
/// wire failures raised while producing the signed bytes.
pub async fn self_sign(
    provisioned: Option<&ProvisionedSessionKey>,
    prepared: &Prepared,
    registry: &ModuleRegistry,
    protocol_time: u64,
    current_revocation_sequence: u64,
) -> Result<SignedPreparation, SigningError> {
    let provisioned = provisioned.ok_or(SigningError::NotProvisioned)?;
    provisioned.ensure_not_durably_revoked()?;
    verify_disclosure_binding(prepared).map_err(SigningError::Disclosure)?;
    if protocol_time >= provisioned.expires_at {
        return Err(SigningError::Expired);
    }
    if current_revocation_sequence != provisioned.revocation_sequence {
        return Err(SigningError::Revoked);
    }
    let activity_type = prepared.envelope.activity_type();
    if !provisioned
        .permitted_activity_types
        .contains(&activity_type)
    {
        return Err(SigningError::ScopeDenied);
    }
    if prepared.envelope.authority().as_bytes() != provisioned.authority.as_bytes() {
        return Err(SigningError::AuthorityMismatch);
    }
    let signature = sign_disclosed(
        &provisioned.signer,
        &prepared.canonical_bytes,
        &prepared.disclosure,
        registry,
    )
    .await
    .map_err(SigningError::Crypto)?;
    let public_key = provisioned.signer.public_key();
    let authority_bytes = provisioned.authority.as_bytes().to_vec();
    let signature_bytes = *signature.as_bytes();
    let signed_canonical_bytes = verification::attach_signature(prepared, signature_bytes)?;
    Ok(SignedPreparation {
        canonical_bytes: prepared.canonical_bytes.clone(),
        signed_canonical_bytes,
        signature: signature_bytes,
        signer_public_key: public_key,
        mode: SigningMode::ProtocolSessionKey,
        audit: SelfSigningAudit {
            mode: SigningMode::ProtocolSessionKey,
            session_public_key: public_key,
            activity_type,
            disclosure_digest: prepared.disclosure_digest,
            authority_bytes,
            expires_at: provisioned.expires_at,
            revocation_sequence: provisioned.revocation_sequence,
        },
    })
}

/// Attaches a returned external signature through the canonical wire encoder.
///
/// # Errors
///
/// Returns the disclosure binding failure, a signature the envelope rejects, or the wire failure
/// raised while encoding the signed envelope.
pub fn attach_external_signature(
    prepared: &Prepared,
    signature: [u8; 64],
) -> Result<Vec<u8>, SigningError> {
    verify_disclosure_binding(prepared).map_err(SigningError::Disclosure)?;
    verification::attach_signature(prepared, signature)
}

/// Verifies the exact signed bytes and returns the only submit-capable wrapper.
///
/// # Errors
///
/// Refuses bytes that do not decode, re-encode, or reduce to the exact prepared canonical form,
/// and an absent, malformed, or invalid signature. Also returns the disclosure binding failure.
pub fn verify_before_submit(
    signed_canonical_bytes: &[u8],
    prepared: &Prepared,
    signer_public_key: &[u8; 32],
    registry: &ModuleRegistry,
) -> Result<VerifiedSubmission, SigningError> {
    verification::verify_exact(
        signed_canonical_bytes,
        prepared,
        signer_public_key,
        registry,
    )
}

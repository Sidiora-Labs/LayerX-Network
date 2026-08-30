//! Agent identity bindings backed only by verified core observations.

use layerx_types::ids::Did;
use layerx_types::verify::VerificationLevel;

use crate::store::{ObjectKind, Store, StoreError, TenantId, TenantKey};

/// Protocol authority currently bound to a DID by core state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProtocolAuthority {
    PrimaryKey([u8; 32]),
    SessionKey([u8; 32]),
    CapabilityGrant([u8; 32]),
}

/// Exact identity answer returned by the boundary adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreIdentity {
    pub canonical_bytes: Vec<u8>,
    pub head_sequence: u64,
    pub revocation_sequence: u64,
    pub verification_level: VerificationLevel,
    pub frozen: bool,
    pub authorities: Vec<ProtocolAuthority>,
}

/// The only seam from identity registration to core identity state.
pub trait IdentityResolver {
    /// Resolves current identity state through the versioned node boundary.
    ///
    /// # Errors
    ///
    /// Returns `BoundaryUnavailable` when the node boundary cannot answer; a DID that
    /// core does not hold is reported as `Ok(None)`.
    fn resolve(&mut self, did: &Did) -> Result<Option<CoreIdentity>, IdentityError>;
}

/// A persisted identity binding and its core provenance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdentityRecord {
    tenant: TenantId,
    did: Did,
    head_sequence: u64,
    verification_level: VerificationLevel,
    authorities: Vec<ProtocolAuthority>,
    canonical_core_bytes: Vec<u8>,
}

impl IdentityRecord {
    #[must_use]
    pub const fn tenant(&self) -> &TenantId {
        &self.tenant
    }

    #[must_use]
    pub const fn did(&self) -> &Did {
        &self.did
    }

    #[must_use]
    pub const fn head_sequence(&self) -> u64 {
        self.head_sequence
    }

    #[must_use]
    pub const fn verification_level(&self) -> VerificationLevel {
        self.verification_level
    }

    #[must_use]
    pub fn authorities(&self) -> &[ProtocolAuthority] {
        &self.authorities
    }

    #[must_use]
    pub fn canonical_core_bytes(&self) -> &[u8] {
        &self.canonical_core_bytes
    }
}

/// Identity registration and revalidation failures.
#[derive(Debug)]
pub enum IdentityError {
    BoundaryUnavailable,
    UnknownDid,
    Frozen,
    Unverified,
    Store(StoreError),
}

impl PartialEq for IdentityError {
    fn eq(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (Self::BoundaryUnavailable, Self::BoundaryUnavailable)
                | (Self::UnknownDid, Self::UnknownDid)
                | (Self::Frozen, Self::Frozen)
                | (Self::Unverified, Self::Unverified)
                | (Self::Store(_), Self::Store(_))
        )
    }
}

impl Eq for IdentityError {}

impl From<StoreError> for IdentityError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

/// Registers a DID only after resolving and verifying it against current core state.
///
/// # Errors
///
/// Returns `UnknownDid` when core holds no binding, `Frozen` or `Unverified` when
/// the observation cannot support use, and propagates resolver and store failures.
pub fn register(
    store: &mut Store,
    tenant: TenantId,
    did: Did,
    resolver: &mut dyn IdentityResolver,
) -> Result<IdentityRecord, IdentityError> {
    let observation = resolver.resolve(&did)?.ok_or(IdentityError::UnknownDid)?;
    validate_observation(&observation)?;
    let key = TenantKey::new(
        tenant.clone(),
        ObjectKind::Identity,
        did.as_bytes().to_vec(),
    )?;
    let record = IdentityRecord {
        tenant,
        did,
        head_sequence: observation.head_sequence,
        verification_level: observation.verification_level,
        authorities: observation.authorities,
        canonical_core_bytes: observation.canonical_bytes,
    };
    store.put_core_cache(key, encode_record(&record)?)?;
    Ok(record)
}

/// Re-resolves a restored binding before permitting its first use.
///
/// # Errors
///
/// Returns `UnknownDid` when core no longer holds the binding, `Frozen` or
/// `Unverified` when the observation cannot support use, and propagates resolver
/// and store failures.
pub fn revalidate(
    store: &mut Store,
    restored: &IdentityRecord,
    resolver: &mut dyn IdentityResolver,
) -> Result<IdentityRecord, IdentityError> {
    register(
        store,
        restored.tenant.clone(),
        restored.did.clone(),
        resolver,
    )
}

fn validate_observation(observation: &CoreIdentity) -> Result<(), IdentityError> {
    if observation.frozen {
        return Err(IdentityError::Frozen);
    }
    if observation.verification_level == VerificationLevel::UNVERIFIED {
        return Err(IdentityError::Unverified);
    }
    if observation.revocation_sequence == 0 {
        return Err(IdentityError::Unverified);
    }
    Ok(())
}

fn encode_record(record: &IdentityRecord) -> Result<Vec<u8>, StoreError> {
    let did_len =
        u16::try_from(record.did.as_bytes().len()).map_err(|_| StoreError::SizeOverflow)?;
    let authority_len =
        u16::try_from(record.authorities.len()).map_err(|_| StoreError::SizeOverflow)?;
    let core_len =
        u32::try_from(record.canonical_core_bytes.len()).map_err(|_| StoreError::SizeOverflow)?;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&did_len.to_be_bytes());
    bytes.extend_from_slice(record.did.as_bytes());
    bytes.extend_from_slice(&record.head_sequence.to_be_bytes());
    bytes.push(record.verification_level.wire_rank());
    bytes.extend_from_slice(&authority_len.to_be_bytes());
    for authority in &record.authorities {
        let (kind, identifier) = match authority {
            ProtocolAuthority::PrimaryKey(identifier) => (1_u8, identifier),
            ProtocolAuthority::SessionKey(identifier) => (2_u8, identifier),
            ProtocolAuthority::CapabilityGrant(identifier) => (3_u8, identifier),
        };
        bytes.push(kind);
        bytes.extend_from_slice(identifier);
    }
    bytes.extend_from_slice(&core_len.to_be_bytes());
    bytes.extend_from_slice(&record.canonical_core_bytes);
    Ok(bytes)
}

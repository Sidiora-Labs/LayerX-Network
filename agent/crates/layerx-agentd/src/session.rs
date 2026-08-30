//! Tenant-bound session lifecycle and daemon-only authentication tokens.

use std::collections::{BTreeMap, BTreeSet};

use layerx_types::ids::Did;

use crate::identity::{IdentityRecord, ProtocolAuthority};
use crate::store::{ObjectKind, Store, StoreError, TenantId, TenantKey};

#[path = "session_revocation.rs"]
mod revocation;

pub use revocation::{
    InvalidationReason, InvalidationReport, PendingActivity, PreparationState, RevocationEvent,
};

/// Stable session identifier supplied by the daemon's secure identifier source.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SessionId(pub [u8; 32]);

/// Complete request required to open a session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenRequest {
    pub session_id: SessionId,
    pub token_id: [u8; 32],
    pub tenant: TenantId,
    pub agent: Did,
    pub authority: ProtocolAuthority,
    pub permitted_activity_types: BTreeSet<u16>,
    pub scopes: BTreeSet<String>,
    pub expiry_sequence: u64,
    pub opening_client: String,
    pub policy_version: String,
}

/// A daemon authenticator. It is never accepted as protocol authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Token {
    id: [u8; 32],
    session_id: SessionId,
    tenant: TenantId,
    agent: Did,
    scopes: BTreeSet<String>,
    expiry_sequence: u64,
}

impl Token {
    /// Checks tenant, agent, scope and core-relative expiry.
    ///
    /// # Errors
    ///
    /// Returns `WrongPrincipal` for a mismatched tenant or agent, `Expired` once the core sequence
    /// reaches the token's expiry, and `ScopeDenied` for a scope the token does not carry.
    pub fn authorize(
        &self,
        tenant: &TenantId,
        agent: &Did,
        scope: &str,
        core_sequence: u64,
    ) -> Result<SessionId, SessionError> {
        if &self.tenant != tenant || &self.agent != agent {
            return Err(SessionError::WrongPrincipal);
        }
        if core_sequence >= self.expiry_sequence {
            return Err(SessionError::Expired);
        }
        if !self.scopes.contains(scope) {
            return Err(SessionError::ScopeDenied);
        }
        Ok(self.session_id)
    }

    /// Returns the daemon token identifier for audit correlation.
    #[must_use]
    pub const fn token_id(&self) -> [u8; 32] {
        self.id
    }

    pub(crate) const fn tenant(&self) -> &TenantId {
        &self.tenant
    }

    pub(crate) const fn agent(&self) -> &Did {
        &self.agent
    }
}

/// Durable session record, including the protocol authority actually used.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionRecord {
    pub request: OpenRequest,
    pub open: bool,
    pub sequence: u64,
    pub budget_reserved: u128,
    pub subscription_cursor: u64,
}

/// In-memory index backed by the tenant-scoped durable store.
#[derive(Default)]
pub struct SessionRegistry {
    records: BTreeMap<SessionId, SessionRecord>,
}

impl SessionRegistry {
    #[must_use]
    pub fn get(&self, id: SessionId) -> Option<&SessionRecord> {
        self.records.get(&id)
    }

    #[must_use]
    pub fn open_count(&self) -> usize {
        self.records.values().filter(|record| record.open).count()
    }

    pub(crate) fn records_mut(&mut self) -> &mut BTreeMap<SessionId, SessionRecord> {
        &mut self.records
    }

    pub fn restore_tenant(&mut self, store: &Store, tenant: &TenantId) -> Result<(), SessionError> {
        for object_id in store.list_object_ids(tenant, ObjectKind::Session) {
            let key = TenantKey::new(tenant.clone(), ObjectKind::Session, object_id)?;
            let value = store.get(&key).ok_or(SessionError::NotFound)?;
            let record = decode(value.bytes(), tenant.clone())?;
            if self
                .records
                .insert(record.request.session_id, record)
                .is_some()
            {
                return Err(SessionError::IdentityMismatch);
            }
        }
        Ok(())
    }
}

/// Session refusal taxonomy suitable for audit recording.
#[derive(Debug)]
pub enum SessionError {
    MissingField(&'static str),
    IdentityMismatch,
    AuthorityMissing,
    Expired,
    WrongPrincipal,
    ScopeDenied,
    NotFound,
    AlreadyClosed,
    Store(StoreError),
}

impl PartialEq for SessionError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::MissingField(left), Self::MissingField(right)) => left == right,
            (Self::Store(_), Self::Store(_)) => true,
            _ => std::mem::discriminant(self) == std::mem::discriminant(other),
        }
    }
}

impl Eq for SessionError {}

impl From<StoreError> for SessionError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

/// Opens and durably records one independently scoped session.
///
/// # Errors
///
/// Returns `IdentityMismatch`, `MissingField`, `Expired` or `AuthorityMissing` from request
/// validation, or `Store` when the session record cannot be encoded or durably written; the
/// registry is left untouched unless the record persisted.
pub fn open(
    store: &mut Store,
    registry: &mut SessionRegistry,
    identity: &IdentityRecord,
    request: OpenRequest,
    core_sequence: u64,
) -> Result<Token, SessionError> {
    validate_request(identity, &request, core_sequence)?;
    let token = Token {
        id: request.token_id,
        session_id: request.session_id,
        tenant: request.tenant.clone(),
        agent: request.agent.clone(),
        scopes: request.scopes.clone(),
        expiry_sequence: request.expiry_sequence,
    };
    let record = SessionRecord {
        request,
        open: true,
        sequence: 0,
        budget_reserved: 0,
        subscription_cursor: 0,
    };
    persist_record(store, &record)?;
    registry.records.insert(record.request.session_id, record);
    Ok(token)
}

/// Closes exactly one session without disturbing any sibling state.
///
/// # Errors
///
/// Returns `NotFound` for a session the registry never held, `AlreadyClosed` for one already
/// closed, or `Store` when the closed record cannot be persisted.
pub fn close(
    store: &mut Store,
    registry: &mut SessionRegistry,
    session_id: SessionId,
) -> Result<(), SessionError> {
    let existing = registry
        .records
        .get(&session_id)
        .cloned()
        .ok_or(SessionError::NotFound)?;
    if !existing.open {
        return Err(SessionError::AlreadyClosed);
    }
    let mut closed = existing;
    closed.open = false;
    persist_record(store, &closed)?;
    registry.records.insert(session_id, closed);
    Ok(())
}

pub fn close_with_companion(
    store: &mut Store,
    registry: &mut SessionRegistry,
    session_id: SessionId,
    companion_key: TenantKey,
    companion_bytes: Vec<u8>,
) -> Result<(), SessionError> {
    let existing = registry
        .records
        .get(&session_id)
        .cloned()
        .ok_or(SessionError::NotFound)?;
    if !existing.open {
        return Err(SessionError::AlreadyClosed);
    }
    let mut closed = existing;
    closed.open = false;
    let session_key = TenantKey::new(
        closed.request.tenant.clone(),
        ObjectKind::Session,
        closed.request.session_id.0.to_vec(),
    )?;
    store.update_local_with_companion(
        session_key,
        encode(&closed)?,
        companion_key,
        companion_bytes,
    )?;
    registry.records.insert(session_id, closed);
    Ok(())
}

/// Applies a core revocation event to sessions and unsubmitted preparations.
///
/// # Errors
///
/// Returns `Store` when a revoked session's closed record cannot be persisted, or `MissingField`
/// when its opening client or policy version exceeds the `u16` length prefix.
pub fn invalidate_on_revocation(
    store: &mut Store,
    registry: &mut SessionRegistry,
    activities: &mut [PendingActivity],
    event: &RevocationEvent,
) -> Result<InvalidationReport, SessionError> {
    revocation::apply_revocation(store, registry, activities, event)
}

fn validate_request(
    identity: &IdentityRecord,
    request: &OpenRequest,
    core_sequence: u64,
) -> Result<(), SessionError> {
    if identity.tenant() != &request.tenant || identity.did() != &request.agent {
        return Err(SessionError::IdentityMismatch);
    }
    if request.permitted_activity_types.is_empty() {
        return Err(SessionError::MissingField("permitted_activity_types"));
    }
    if request.scopes.is_empty() {
        return Err(SessionError::MissingField("scopes"));
    }
    if request.opening_client.is_empty() {
        return Err(SessionError::MissingField("opening_client"));
    }
    if request.policy_version.is_empty() {
        return Err(SessionError::MissingField("policy_version"));
    }
    if request.expiry_sequence <= core_sequence {
        return Err(SessionError::Expired);
    }
    if !identity.authorities().contains(&request.authority) {
        return Err(SessionError::AuthorityMissing);
    }
    Ok(())
}

pub(crate) fn persist_record(
    store: &mut Store,
    record: &SessionRecord,
) -> Result<(), SessionError> {
    let key = TenantKey::new(
        record.request.tenant.clone(),
        ObjectKind::Session,
        record.request.session_id.0.to_vec(),
    )?;
    let encoded = encode(record)?;
    store.put_local(key, encoded)?;
    Ok(())
}

fn encode(record: &SessionRecord) -> Result<Vec<u8>, SessionError> {
    let client_len = u16::try_from(record.request.opening_client.len())
        .map_err(|_| SessionError::MissingField("opening_client"))?;
    let policy_len = u16::try_from(record.request.policy_version.len())
        .map_err(|_| SessionError::MissingField("policy_version"))?;
    let did = record.request.agent.as_bytes();
    let did_len = u16::try_from(did.len()).map_err(|_| SessionError::MissingField("agent"))?;
    let activity_len = u16::try_from(record.request.permitted_activity_types.len())
        .map_err(|_| SessionError::MissingField("permitted_activity_types"))?;
    let scope_len = u16::try_from(record.request.scopes.len())
        .map_err(|_| SessionError::MissingField("scopes"))?;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"LXSR02");
    bytes.extend_from_slice(&record.request.session_id.0);
    bytes.extend_from_slice(&record.request.token_id);
    bytes.extend_from_slice(&record.request.expiry_sequence.to_be_bytes());
    bytes.push(u8::from(record.open));
    bytes.extend_from_slice(&record.sequence.to_be_bytes());
    bytes.extend_from_slice(&record.budget_reserved.to_be_bytes());
    bytes.extend_from_slice(&record.subscription_cursor.to_be_bytes());
    bytes.extend_from_slice(&client_len.to_be_bytes());
    bytes.extend_from_slice(record.request.opening_client.as_bytes());
    bytes.extend_from_slice(&policy_len.to_be_bytes());
    bytes.extend_from_slice(record.request.policy_version.as_bytes());
    bytes.extend_from_slice(&did_len.to_be_bytes());
    bytes.extend_from_slice(did);
    let (authority_kind, authority_id) = match &record.request.authority {
        ProtocolAuthority::PrimaryKey(value) => (1, value),
        ProtocolAuthority::SessionKey(value) => (2, value),
        ProtocolAuthority::CapabilityGrant(value) => (3, value),
    };
    bytes.push(authority_kind);
    bytes.extend_from_slice(authority_id);
    bytes.extend_from_slice(&activity_len.to_be_bytes());
    for value in &record.request.permitted_activity_types {
        bytes.extend_from_slice(&value.to_be_bytes());
    }
    bytes.extend_from_slice(&scope_len.to_be_bytes());
    for value in &record.request.scopes {
        let length =
            u16::try_from(value.len()).map_err(|_| SessionError::MissingField("scopes"))?;
        bytes.extend_from_slice(&length.to_be_bytes());
        bytes.extend_from_slice(value.as_bytes());
    }
    Ok(bytes)
}

fn decode(bytes: &[u8], tenant: TenantId) -> Result<SessionRecord, SessionError> {
    let mut at = 0;
    let take = |at: &mut usize, length: usize| -> Result<&[u8], SessionError> {
        let end = at
            .checked_add(length)
            .ok_or(SessionError::MissingField("record"))?;
        let value = bytes
            .get(*at..end)
            .ok_or(SessionError::MissingField("record"))?;
        *at = end;
        Ok(value)
    };
    if take(&mut at, 6)? != b"LXSR02" {
        return Err(SessionError::MissingField("record_version"));
    }
    let session_id = SessionId(
        take(&mut at, 32)?
            .try_into()
            .map_err(|_| SessionError::MissingField("session_id"))?,
    );
    let token_id = take(&mut at, 32)?
        .try_into()
        .map_err(|_| SessionError::MissingField("token_id"))?;
    let expiry_sequence = u64::from_be_bytes(
        take(&mut at, 8)?
            .try_into()
            .map_err(|_| SessionError::MissingField("expiry"))?,
    );
    let open = match take(&mut at, 1)?[0] {
        0 => false,
        1 => true,
        _ => return Err(SessionError::MissingField("open")),
    };
    let sequence = u64::from_be_bytes(
        take(&mut at, 8)?
            .try_into()
            .map_err(|_| SessionError::MissingField("sequence"))?,
    );
    let budget_reserved = u128::from_be_bytes(
        take(&mut at, 16)?
            .try_into()
            .map_err(|_| SessionError::MissingField("budget"))?,
    );
    let subscription_cursor = u64::from_be_bytes(
        take(&mut at, 8)?
            .try_into()
            .map_err(|_| SessionError::MissingField("cursor"))?,
    );
    let text = |at: &mut usize| -> Result<String, SessionError> {
        let length = usize::from(u16::from_be_bytes(
            take(at, 2)?
                .try_into()
                .map_err(|_| SessionError::MissingField("text"))?,
        ));
        String::from_utf8(take(at, length)?.to_vec())
            .map_err(|_| SessionError::MissingField("text"))
    };
    let opening_client = text(&mut at)?;
    let policy_version = text(&mut at)?;
    let did_len = usize::from(u16::from_be_bytes(
        take(&mut at, 2)?
            .try_into()
            .map_err(|_| SessionError::MissingField("agent"))?,
    ));
    let agent =
        Did::new(take(&mut at, did_len)?).map_err(|_| SessionError::MissingField("agent"))?;
    let authority_id = {
        let kind = take(&mut at, 1)?[0];
        let id = take(&mut at, 32)?
            .try_into()
            .map_err(|_| SessionError::MissingField("authority"))?;
        match kind {
            1 => ProtocolAuthority::PrimaryKey(id),
            2 => ProtocolAuthority::SessionKey(id),
            3 => ProtocolAuthority::CapabilityGrant(id),
            _ => return Err(SessionError::MissingField("authority")),
        }
    };
    let activity_count = usize::from(u16::from_be_bytes(
        take(&mut at, 2)?
            .try_into()
            .map_err(|_| SessionError::MissingField("activities"))?,
    ));
    let mut permitted_activity_types = BTreeSet::new();
    for _ in 0..activity_count {
        permitted_activity_types.insert(u16::from_be_bytes(
            take(&mut at, 2)?
                .try_into()
                .map_err(|_| SessionError::MissingField("activities"))?,
        ));
    }
    let scope_count = usize::from(u16::from_be_bytes(
        take(&mut at, 2)?
            .try_into()
            .map_err(|_| SessionError::MissingField("scopes"))?,
    ));
    let mut scopes = BTreeSet::new();
    for _ in 0..scope_count {
        scopes.insert(text(&mut at)?);
    }
    if at != bytes.len() || permitted_activity_types.is_empty() || scopes.is_empty() {
        return Err(SessionError::MissingField("record"));
    }
    Ok(SessionRecord {
        request: OpenRequest {
            session_id,
            token_id,
            tenant,
            agent,
            authority: authority_id,
            permitted_activity_types,
            scopes,
            expiry_sequence,
            opening_client,
            policy_version,
        },
        open,
        sequence,
        budget_reserved,
        subscription_cursor,
    })
}

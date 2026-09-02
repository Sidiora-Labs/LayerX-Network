//! Durable tenant- and scope-bound subscription state.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Display, Formatter};

use layerx_agent_api::identity::{
    ActivityType, AgentDid, Asset, CapabilityId, ExplicitSet, TenantId as ApiTenantId,
};
use layerx_agent_api::read::{AccountRef, ModuleRef};
use layerx_agent_api::subscription::{
    Cursor as ApiCursor, DeliveryTarget, SubscriptionCreate, SubscriptionFilter, SubscriptionId,
    SubscriptionRecord, SubscriptionScope, SubscriptionTarget, TenantObject,
};
use layerx_agent_api::{subscription::CursorAcknowledgement, Sequence};
use layerx_types::result::ResultCode;

use crate::session::{SessionCredential, SessionRegistry, Token};
use crate::store::{
    ObjectKind, StorageClass, Store as DurableStore, StoreError, TenantId, TenantKey,
};
use crate::tenant::{
    self, AuthorizationError, ObjectOwner, Operation, RequestContext, Surface, TenantObservability,
};
use layerx_types::ids::Did;

const RECORD_MAGIC: &[u8; 4] = b"LXS2";
const LEGACY_RECORD_MAGIC: &[u8; 4] = b"LXSB";

/// Durable subscription position. The value is the next stream cursor after
/// acknowledged delivery, not a locally inferred protocol sequence.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Cursor(pub u64);

/// Durable continuity state that prevents delivery from crossing an
/// unresolved gap or an expired retention window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Continuity {
    Healthy,
    GapBlocked {
        missing_first: u64,
        missing_last: u64,
        backfill_attempted: bool,
        recovered_through: Option<u64>,
    },
    Truncated {
        requested_from: u64,
        oldest_available: u64,
        lost_through: u64,
    },
}

/// Durable reason delivery was permanently stopped while retaining its audit record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Termination {
    Deleted,
    SessionRevoked,
    CapabilityRevoked,
    TenantRevoked,
}

impl From<ApiCursor> for Cursor {
    fn from(value: ApiCursor) -> Self {
        Self(value.0 .0)
    }
}

impl From<Cursor> for ApiCursor {
    fn from(value: Cursor) -> Self {
        Self(Sequence(value.0))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DurableRecord {
    public: SubscriptionRecord,
    delivered_unacknowledged: BTreeSet<Cursor>,
    current_delivery: Cursor,
    continuity: Continuity,
    termination: Option<Termination>,
    session: Option<SessionCredential>,
}

/// Durable subscription store failures.
#[derive(Debug)]
pub enum SubscriptionError {
    InvalidScope,
    InvalidFilter,
    Duplicate,
    NotFound,
    Paused,
    CursorOutOfOrder { expected: Cursor, received: Cursor },
    CursorNeverDelivered { cursor: Cursor },
    CursorRegressed { current: Cursor, received: Cursor },
    SequenceExhausted,
    Corrupt,
    Authorization(AuthorizationError),
    AuthorizationRequired,
    Durable(StoreError),
}

impl Display for SubscriptionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidScope => formatter.write_str("subscription scope is invalid"),
            Self::InvalidFilter => formatter.write_str("subscription filter exceeds its scope"),
            Self::Duplicate => formatter.write_str("subscription already exists"),
            Self::NotFound => formatter.write_str("subscription not found"),
            Self::Paused => formatter.write_str("subscription is paused"),
            Self::CursorOutOfOrder { expected, received } => write!(
                formatter,
                "delivery cursor {} arrived out of order; expected {}",
                received.0, expected.0
            ),
            Self::CursorNeverDelivered { cursor } => {
                write!(formatter, "cursor {} was never delivered", cursor.0)
            }
            Self::CursorRegressed { current, received } => write!(
                formatter,
                "cursor {} precedes acknowledged cursor {}",
                received.0, current.0
            ),
            Self::SequenceExhausted => formatter.write_str("subscription cursor is exhausted"),
            Self::Corrupt => formatter.write_str("durable subscription record is corrupt"),
            Self::Authorization(error) => {
                write!(formatter, "subscription authorization failed: {error:?}")
            }
            Self::AuthorizationRequired => {
                formatter.write_str("session-bound subscription requires an authorized operation")
            }
            Self::Durable(error) => Display::fmt(error, formatter),
        }
    }
}

impl std::error::Error for SubscriptionError {}

impl From<StoreError> for SubscriptionError {
    fn from(value: StoreError) -> Self {
        Self::Durable(value)
    }
}

/// Tenant-owned durable subscription collection.
#[derive(Debug)]
pub struct Store {
    durable: DurableStore,
    tenant: TenantId,
    records: BTreeMap<String, DurableRecord>,
}

impl Store {
    /// Restores every subscription owned by `tenant` from local durable state.
    ///
    /// # Errors
    ///
    /// Returns `Corrupt` for a missing or non-local stored value, a record whose
    /// identifier, tenant, or encoding disagrees with its key, or a duplicate
    /// identifier; `Durable` wraps a rejected object key.
    pub fn open(mut durable: DurableStore, tenant: TenantId) -> Result<Self, SubscriptionError> {
        let mut records = BTreeMap::new();
        let mut legacy_migrations = Vec::new();
        for object_id in durable.list_object_ids(&tenant, ObjectKind::Subscription) {
            let key = TenantKey::new(tenant.clone(), ObjectKind::Subscription, object_id.clone())?;
            let stored = durable.get(&key).ok_or(SubscriptionError::Corrupt)?;
            if stored.class() != StorageClass::LocalOnly {
                return Err(SubscriptionError::Corrupt);
            }
            let (mut record, legacy_without_session) = decode_record(stored.bytes())?;
            if legacy_without_session {
                record.public.paused = true;
                record.delivered_unacknowledged.clear();
                if record.termination.is_none() {
                    record.termination = Some(Termination::SessionRevoked);
                }
                legacy_migrations.push((key, encode_record(&record)?));
            }
            let id = record.public.subscription_id.as_str().to_owned();
            if object_id != id.as_bytes()
                || record.public.scope.tenant.as_str() != tenant.as_str()
                || records.insert(id, record).is_some()
            {
                return Err(SubscriptionError::Corrupt);
            }
        }
        for (key, bytes) in legacy_migrations {
            durable.put_local(key, bytes)?;
        }
        Ok(Self {
            durable,
            tenant,
            records,
        })
    }

    /// Creates one durable subscription after applying scope restrictions to
    /// every filter dimension.
    ///
    /// # Errors
    ///
    /// Returns `InvalidScope` for another tenant's scope, `InvalidFilter` when a filter
    /// dimension escapes that scope, `Duplicate` for a known identifier, or the `Durable`
    /// failure of the first persist.
    pub fn create(
        &mut self,
        subscription_id: SubscriptionId,
        request: SubscriptionCreate,
    ) -> Result<SubscriptionRecord, SubscriptionError> {
        self.create_bound(subscription_id, request, None)
    }

    /// Creates a token-gated durable subscription bound to the exact session credential that
    /// authorized it.
    ///
    /// # Errors
    ///
    /// Returns `Authorization` when the common tenant resolver refuses the current token, plus
    /// the same validation and persistence failures as [`Self::create`].
    pub fn create_authorized(
        &mut self,
        sessions: &SessionRegistry,
        token: &Token,
        observability: &mut TenantObservability,
        core_sequence: u64,
        subscription_id: SubscriptionId,
        request: SubscriptionCreate,
    ) -> Result<SubscriptionRecord, SubscriptionError> {
        let owner = subscription_owner(&request.scope)?;
        let context = RequestContext {
            surface: Surface::Subscription,
            operation: Operation::SubscriptionCreate,
            core_sequence,
            supplied_header_tenant: None,
            supplied_body_tenant: None,
            target_owner: Some(owner),
        };
        tenant::resolve(token, sessions, &context, observability)
            .map_err(SubscriptionError::Authorization)?;
        self.create_bound(subscription_id, request, Some(token.credential()))
    }

    fn create_bound(
        &mut self,
        subscription_id: SubscriptionId,
        request: SubscriptionCreate,
        session: Option<SessionCredential>,
    ) -> Result<SubscriptionRecord, SubscriptionError> {
        self.validate_scope(&request.scope)?;
        validate_filter(&request.scope, &request.filter)?;
        let request = request
            .validate()
            .map_err(|_| SubscriptionError::InvalidFilter)?;
        let id = subscription_id.as_str().to_owned();
        if self.records.contains_key(&id) {
            return Err(SubscriptionError::Duplicate);
        }
        let record = DurableRecord {
            public: SubscriptionRecord {
                subscription_id,
                scope: request.scope,
                filter: request.filter,
                start: request.start,
                last_acknowledged: request.start,
                delivery_target: request.delivery_target,
                paused: false,
            },
            delivered_unacknowledged: BTreeSet::new(),
            current_delivery: Cursor::from(request.start),
            continuity: Continuity::Healthy,
            termination: None,
            session,
        };
        self.persist(&record)?;
        self.records.insert(id, record.clone());
        Ok(record.public)
    }

    pub(super) fn session_binding(
        &self,
        target: &SubscriptionTarget,
    ) -> Result<Option<SessionCredential>, SubscriptionError> {
        Ok(self.record_any(target)?.session.clone())
    }

    /// Lists only intentionally unbound records owned by the exact tenant, agent and capability
    /// scope. Session-bound records require [`Self::list_authorized`].
    #[must_use]
    pub fn list(&self, scope: &SubscriptionScope) -> Vec<SubscriptionRecord> {
        if self.validate_scope(scope).is_err() {
            return Vec::new();
        }
        self.records
            .values()
            .filter(|record| {
                record.public.scope == *scope
                    && record.termination.is_none()
                    && record.session.is_none()
            })
            .map(|record| record.public.clone())
            .collect()
    }

    /// Lists only subscriptions bound to the exact current session credential after passing the
    /// common resolver.
    pub fn list_authorized(
        &self,
        sessions: &SessionRegistry,
        token: &Token,
        observability: &mut TenantObservability,
        core_sequence: u64,
        scope: &SubscriptionScope,
    ) -> Result<Vec<SubscriptionRecord>, SubscriptionError> {
        self.validate_scope(scope)?;
        authorize_scope(
            token,
            sessions,
            scope,
            Operation::SubscriptionList,
            core_sequence,
            observability,
        )?;
        let credential = token.credential();
        Ok(self
            .records
            .values()
            .filter(|record| {
                record.public.scope == *scope
                    && record.termination.is_none()
                    && record.session.as_ref() == Some(&credential)
            })
            .map(|record| record.public.clone())
            .collect())
    }

    /// Reads one subscription without revealing cross-scope existence.
    ///
    /// # Errors
    ///
    /// Returns `NotFound` for an unknown identifier, a foreign scope, and a terminated
    /// subscription alike.
    pub fn get(
        &self,
        target: &SubscriptionTarget,
    ) -> Result<SubscriptionRecord, SubscriptionError> {
        self.require_unbound_target(target)?;
        self.get_inner(target)
    }

    /// Reads one session-bound subscription after reauthorizing its exact credential.
    pub fn health_authorized(
        &self,
        sessions: &SessionRegistry,
        token: &Token,
        observability: &mut TenantObservability,
        core_sequence: u64,
        target: &SubscriptionTarget,
    ) -> Result<SubscriptionRecord, SubscriptionError> {
        self.authorize_target(
            sessions,
            token,
            observability,
            core_sequence,
            target,
            Operation::SubscriptionHealth,
        )?;
        self.get_inner(target)
    }

    pub(super) fn get_inner(
        &self,
        target: &SubscriptionTarget,
    ) -> Result<SubscriptionRecord, SubscriptionError> {
        Ok(self.target_inner(target)?.public.clone())
    }

    /// Records one strictly consecutive delivered cursor durably so a later
    /// acknowledgement cannot claim an unseen position.
    ///
    /// # Errors
    ///
    /// Returns `Paused` while delivery is stopped, `CursorOutOfOrder` for a cursor that does
    /// not immediately follow the last delivery, `SequenceExhausted` at the cursor ceiling,
    /// `NotFound` outside the exact live scope, or the `Durable` persist failure.
    pub fn mark_delivered(
        &mut self,
        target: &SubscriptionTarget,
        cursor: Cursor,
    ) -> Result<(), SubscriptionError> {
        self.require_unbound_target(target)?;
        self.mark_delivered_inner(target, cursor)
    }

    pub(super) fn mark_delivered_inner(
        &mut self,
        target: &SubscriptionTarget,
        cursor: Cursor,
    ) -> Result<(), SubscriptionError> {
        let id = target.subscription_id.as_str().to_owned();
        let record = self.target_inner(target)?;
        if record.public.paused {
            return Err(SubscriptionError::Paused);
        }
        if cursor <= record.current_delivery {
            let expected = Cursor(
                record
                    .current_delivery
                    .0
                    .checked_add(1)
                    .ok_or(SubscriptionError::SequenceExhausted)?,
            );
            return Err(SubscriptionError::CursorOutOfOrder {
                expected,
                received: cursor,
            });
        }
        let mut updated = record.clone();
        updated.current_delivery = cursor;
        if updated.delivered_unacknowledged.insert(cursor) {
            self.persist(&updated)?;
        }
        self.records.insert(id, updated);
        Ok(())
    }

    /// Advances the durable acknowledged cursor only through positions that
    /// were previously delivered in strict order.
    ///
    /// # Errors
    ///
    /// Returns `CursorRegressed` below the acknowledged cursor, `CursorNeverDelivered` for a
    /// position never marked delivered, `NotFound` outside the exact live scope, or the
    /// `Durable` persist failure.
    pub fn acknowledge(
        &mut self,
        acknowledgement: &CursorAcknowledgement,
    ) -> Result<SubscriptionRecord, SubscriptionError> {
        let target = acknowledgement_target(acknowledgement);
        self.require_unbound_target(&target)?;
        self.acknowledge_inner(acknowledgement)
    }

    pub(super) fn acknowledge_inner(
        &mut self,
        acknowledgement: &CursorAcknowledgement,
    ) -> Result<SubscriptionRecord, SubscriptionError> {
        let target = acknowledgement_target(acknowledgement);
        let id = target.subscription_id.as_str().to_owned();
        let record = self.target_inner(&target)?;
        let current = Cursor::from(record.public.last_acknowledged);
        let received = Cursor::from(acknowledgement.cursor);
        if received < current {
            return Err(SubscriptionError::CursorRegressed { current, received });
        }
        if received == current {
            return Ok(record.public.clone());
        }
        if !record.delivered_unacknowledged.contains(&received) {
            return Err(SubscriptionError::CursorNeverDelivered { cursor: received });
        }
        let mut updated = record.clone();
        updated.public.last_acknowledged = acknowledgement.cursor;
        updated
            .delivered_unacknowledged
            .retain(|delivered| *delivered > received);
        self.persist(&updated)?;
        self.records.insert(id, updated.clone());
        Ok(updated.public)
    }

    /// Acknowledges one delivery on an exact session-bound subscription through the common
    /// resolver.
    pub fn acknowledge_authorized(
        &mut self,
        sessions: &SessionRegistry,
        token: &Token,
        observability: &mut TenantObservability,
        core_sequence: u64,
        acknowledgement: &CursorAcknowledgement,
    ) -> Result<SubscriptionRecord, SubscriptionError> {
        let target = acknowledgement_target(acknowledgement);
        self.authorize_target(
            sessions,
            token,
            observability,
            core_sequence,
            &target,
            Operation::SubscriptionAcknowledge,
        )?;
        self.acknowledge_inner(acknowledgement)
    }

    /// Pauses delivery durably.
    ///
    /// # Errors
    ///
    /// Returns `NotFound` for an unknown, foreign-scope, or terminated subscription, or the
    /// `Durable` failure of persisting the raised paused flag.
    pub fn pause(
        &mut self,
        target: &SubscriptionTarget,
    ) -> Result<SubscriptionRecord, SubscriptionError> {
        self.require_unbound_target(target)?;
        self.set_paused_inner(target, true)
    }

    /// Pauses one exact session-bound subscription through the common resolver.
    pub fn pause_authorized(
        &mut self,
        sessions: &SessionRegistry,
        token: &Token,
        observability: &mut TenantObservability,
        core_sequence: u64,
        target: &SubscriptionTarget,
    ) -> Result<SubscriptionRecord, SubscriptionError> {
        self.authorize_target(
            sessions,
            token,
            observability,
            core_sequence,
            target,
            Operation::SubscriptionPause,
        )?;
        self.set_paused_inner(target, true)
    }

    /// Resumes delivery durably from the last acknowledged cursor.
    ///
    /// # Errors
    ///
    /// Returns `NotFound` for an unknown, foreign-scope, or terminated subscription, or the
    /// `Durable` failure of persisting the cleared paused flag.
    pub fn resume(
        &mut self,
        target: &SubscriptionTarget,
    ) -> Result<SubscriptionRecord, SubscriptionError> {
        self.require_unbound_target(target)?;
        self.set_paused_inner(target, false)
    }

    /// Resumes one exact session-bound subscription through the common resolver.
    pub fn resume_authorized(
        &mut self,
        sessions: &SessionRegistry,
        token: &Token,
        observability: &mut TenantObservability,
        core_sequence: u64,
        target: &SubscriptionTarget,
    ) -> Result<SubscriptionRecord, SubscriptionError> {
        self.authorize_target(
            sessions,
            token,
            observability,
            core_sequence,
            target,
            Operation::SubscriptionResume,
        )?;
        self.set_paused_inner(target, false)
    }

    /// Stops one exact-scope subscription while retaining its durable audit record.
    ///
    /// # Errors
    ///
    /// Returns `NotFound` for an unknown, foreign-scope, or already terminated subscription,
    /// or the `Durable` failure of persisting the retained audit record.
    pub fn delete(&mut self, target: &SubscriptionTarget) -> Result<(), SubscriptionError> {
        self.require_unbound_target(target)?;
        self.terminate_inner(target, Termination::Deleted)?;
        Ok(())
    }

    /// Deletes one exact session-bound subscription through the common resolver.
    pub fn delete_authorized(
        &mut self,
        sessions: &SessionRegistry,
        token: &Token,
        observability: &mut TenantObservability,
        core_sequence: u64,
        target: &SubscriptionTarget,
    ) -> Result<(), SubscriptionError> {
        self.authorize_target(
            sessions,
            token,
            observability,
            core_sequence,
            target,
            Operation::SubscriptionDelete,
        )?;
        self.terminate_inner(target, Termination::Deleted)?;
        Ok(())
    }

    pub(super) fn delete_inner(
        &mut self,
        target: &SubscriptionTarget,
    ) -> Result<(), SubscriptionError> {
        self.terminate_inner(target, Termination::Deleted)
    }

    /// Stops a subscription immediately for an owner revocation reason.
    ///
    /// # Errors
    ///
    /// Returns `Corrupt` when `reason` is `Deleted`, `NotFound` for an unknown, foreign-scope,
    /// or already terminated subscription, or the `Durable` persist failure.
    pub fn revoke(
        &mut self,
        target: &SubscriptionTarget,
        reason: Termination,
    ) -> Result<(), SubscriptionError> {
        if reason == Termination::Deleted {
            return Err(SubscriptionError::Corrupt);
        }
        self.terminate_inner(target, reason)
    }

    /// Returns the retained terminal audit state without exposing another scope.
    ///
    /// # Errors
    ///
    /// Returns `NotFound` for an unknown identifier or any scope other than the owning one;
    /// terminated records remain readable here.
    pub fn termination(
        &self,
        target: &SubscriptionTarget,
    ) -> Result<Option<Termination>, SubscriptionError> {
        Ok(self.record_any(target)?.termination)
    }

    /// Returns the cursor from which restart delivery must resume.
    ///
    /// # Errors
    ///
    /// Returns `NotFound` for an unknown identifier, a foreign scope, or a subscription
    /// already terminated.
    pub fn resume_cursor(&self, target: &SubscriptionTarget) -> Result<Cursor, SubscriptionError> {
        self.require_unbound_target(target)?;
        self.resume_cursor_inner(target)
    }

    pub(super) fn resume_cursor_inner(
        &self,
        target: &SubscriptionTarget,
    ) -> Result<Cursor, SubscriptionError> {
        Ok(Cursor::from(
            self.target_inner(target)?.public.last_acknowledged,
        ))
    }

    /// Returns the durable continuity state for the exact subscription scope.
    ///
    /// # Errors
    ///
    /// Returns `NotFound` for an unknown identifier, a foreign scope, or a subscription
    /// already terminated.
    pub fn continuity(&self, target: &SubscriptionTarget) -> Result<Continuity, SubscriptionError> {
        self.require_unbound_target(target)?;
        self.continuity_inner(target)
    }

    pub(super) fn continuity_inner(
        &self,
        target: &SubscriptionTarget,
    ) -> Result<Continuity, SubscriptionError> {
        Ok(self.target_inner(target)?.continuity)
    }

    /// Blocks delivery at an explicit missing global-sequence range.
    ///
    /// # Errors
    ///
    /// Returns `NotFound` outside the exact live scope, or the `Durable` failure of
    /// persisting the gap-blocked continuity.
    pub fn block_gap(
        &mut self,
        target: &SubscriptionTarget,
        missing_first: u64,
        missing_last: u64,
    ) -> Result<(), SubscriptionError> {
        self.require_unbound_target(target)?;
        self.block_gap_inner(target, missing_first, missing_last)
    }

    pub(super) fn block_gap_inner(
        &mut self,
        target: &SubscriptionTarget,
        missing_first: u64,
        missing_last: u64,
    ) -> Result<(), SubscriptionError> {
        let id = target.subscription_id.as_str().to_owned();
        let mut updated = self.target_inner(target)?.clone();
        updated.continuity = Continuity::GapBlocked {
            missing_first,
            missing_last,
            backfill_attempted: false,
            recovered_through: None,
        };
        self.persist(&updated)?;
        self.records.insert(id, updated);
        Ok(())
    }

    /// Records exact backfill progress while leaving the gap blocked.
    ///
    /// # Errors
    ///
    /// Returns `Corrupt` unless the subscription is currently gap-blocked, `NotFound` outside
    /// the exact live scope, or the `Durable` persist failure.
    pub fn record_backfill(
        &mut self,
        target: &SubscriptionTarget,
        recovered_through: Option<u64>,
    ) -> Result<(), SubscriptionError> {
        self.require_unbound_target(target)?;
        self.record_backfill_inner(target, recovered_through)
    }

    pub(super) fn record_backfill_inner(
        &mut self,
        target: &SubscriptionTarget,
        recovered_through: Option<u64>,
    ) -> Result<(), SubscriptionError> {
        let id = target.subscription_id.as_str().to_owned();
        let mut updated = self.target_inner(target)?.clone();
        let Continuity::GapBlocked {
            missing_first,
            missing_last,
            ..
        } = updated.continuity
        else {
            return Err(SubscriptionError::Corrupt);
        };
        updated.continuity = Continuity::GapBlocked {
            missing_first,
            missing_last,
            backfill_attempted: true,
            recovered_through,
        };
        self.persist(&updated)?;
        self.records.insert(id, updated);
        Ok(())
    }

    /// Clears a gap only after complete contiguous backfill was verified.
    ///
    /// # Errors
    ///
    /// Returns `Corrupt` unless a gap is currently blocked, `NotFound` outside the exact live
    /// scope, or the `Durable` persist failure.
    pub fn clear_gap(&mut self, target: &SubscriptionTarget) -> Result<(), SubscriptionError> {
        self.require_unbound_target(target)?;
        self.clear_gap_inner(target)
    }

    pub(super) fn clear_gap_inner(
        &mut self,
        target: &SubscriptionTarget,
    ) -> Result<(), SubscriptionError> {
        let id = target.subscription_id.as_str().to_owned();
        let mut updated = self.target_inner(target)?.clone();
        if !matches!(updated.continuity, Continuity::GapBlocked { .. }) {
            return Err(SubscriptionError::Corrupt);
        }
        updated.continuity = Continuity::Healthy;
        self.persist(&updated)?;
        self.records.insert(id, updated);
        Ok(())
    }

    /// Marks retention loss durably; truncated state is terminal.
    ///
    /// # Errors
    ///
    /// Returns `NotFound` outside the exact live scope, or the `Durable` failure of
    /// persisting the terminal truncated state.
    pub fn mark_truncated(
        &mut self,
        target: &SubscriptionTarget,
        requested_from: u64,
        oldest_available: u64,
        lost_through: u64,
    ) -> Result<(), SubscriptionError> {
        self.require_unbound_target(target)?;
        self.mark_truncated_inner(target, requested_from, oldest_available, lost_through)
    }

    pub(super) fn mark_truncated_inner(
        &mut self,
        target: &SubscriptionTarget,
        requested_from: u64,
        oldest_available: u64,
        lost_through: u64,
    ) -> Result<(), SubscriptionError> {
        let id = target.subscription_id.as_str().to_owned();
        let mut updated = self.target_inner(target)?.clone();
        updated.continuity = Continuity::Truncated {
            requested_from,
            oldest_available,
            lost_through,
        };
        self.persist(&updated)?;
        self.records.insert(id, updated);
        Ok(())
    }

    /// Consumes the subscription collection and returns the underlying store.
    #[must_use]
    pub fn into_durable(self) -> DurableStore {
        self.durable
    }

    /// Borrows the tenant-scoped durable state for event-history delivery.
    #[must_use]
    pub const fn durable(&self) -> &DurableStore {
        &self.durable
    }

    fn set_paused_inner(
        &mut self,
        target: &SubscriptionTarget,
        paused: bool,
    ) -> Result<SubscriptionRecord, SubscriptionError> {
        let id = target.subscription_id.as_str().to_owned();
        let mut updated = self.target_inner(target)?.clone();
        updated.public.paused = paused;
        self.persist(&updated)?;
        self.records.insert(id, updated.clone());
        Ok(updated.public)
    }

    fn target_inner(
        &self,
        target: &SubscriptionTarget,
    ) -> Result<&DurableRecord, SubscriptionError> {
        let record = self.record_any(target)?;
        if record.termination.is_some() {
            Err(SubscriptionError::NotFound)
        } else {
            Ok(record)
        }
    }

    fn record_any(&self, target: &SubscriptionTarget) -> Result<&DurableRecord, SubscriptionError> {
        if self.validate_scope(&target.scope).is_err() {
            return Err(SubscriptionError::NotFound);
        }
        self.records
            .get(target.subscription_id.as_str())
            .filter(|record| record.public.scope == target.scope)
            .ok_or(SubscriptionError::NotFound)
    }

    fn terminate_inner(
        &mut self,
        target: &SubscriptionTarget,
        reason: Termination,
    ) -> Result<(), SubscriptionError> {
        let id = target.subscription_id.as_str().to_owned();
        let mut updated = self.target_inner(target)?.clone();
        updated.public.paused = true;
        updated.delivered_unacknowledged.clear();
        updated.termination = Some(reason);
        self.persist(&updated)?;
        self.records.insert(id, updated);
        Ok(())
    }

    pub(super) fn require_unbound_target(
        &self,
        target: &SubscriptionTarget,
    ) -> Result<(), SubscriptionError> {
        if self.record_any(target)?.session.is_some() {
            Err(SubscriptionError::AuthorizationRequired)
        } else {
            Ok(())
        }
    }

    pub(super) fn authorize_target(
        &self,
        sessions: &SessionRegistry,
        token: &Token,
        observability: &mut TenantObservability,
        core_sequence: u64,
        target: &SubscriptionTarget,
        operation: Operation,
    ) -> Result<(), SubscriptionError> {
        authorize_scope(
            token,
            sessions,
            &target.scope,
            operation,
            core_sequence,
            observability,
        )?;
        let record = self.record_any(target)?;
        if record.session.as_ref() != Some(&token.credential()) {
            return Err(SubscriptionError::Authorization(
                AuthorizationError::NotAuthorized,
            ));
        }
        Ok(())
    }

    fn validate_scope(&self, scope: &SubscriptionScope) -> Result<(), SubscriptionError> {
        (scope.tenant.as_str() == self.tenant.as_str())
            .then_some(())
            .ok_or(SubscriptionError::InvalidScope)
    }

    fn persist(&mut self, record: &DurableRecord) -> Result<(), SubscriptionError> {
        let key = subscription_key(self.tenant.clone(), &record.public.subscription_id)?;
        self.durable.put_local(key, encode_record(record)?)?;
        Ok(())
    }
}

fn validate_filter(
    scope: &SubscriptionScope,
    filter: &SubscriptionFilter,
) -> Result<(), SubscriptionError> {
    let tenant = scope.tenant.as_str();
    if filter
        .agents
        .values()
        .iter()
        .any(|item| item.tenant.as_str() != tenant || item.value != scope.agent)
        || filter
            .accounts
            .values()
            .iter()
            .any(|item| item.tenant.as_str() != tenant)
        || filter
            .modules
            .values()
            .iter()
            .any(|item| item.tenant.as_str() != tenant)
        || filter
            .assets
            .values()
            .iter()
            .any(|item| item.tenant.as_str() != tenant)
        || filter
            .counterparties
            .values()
            .iter()
            .any(|item| item.tenant.as_str() != tenant)
    {
        return Err(SubscriptionError::InvalidFilter);
    }
    Ok(())
}

fn subscription_owner(scope: &SubscriptionScope) -> Result<ObjectOwner, SubscriptionError> {
    let tenant = TenantId::new(scope.tenant.as_str())?;
    let agent =
        Did::new(scope.agent.as_str().as_bytes()).map_err(|_| SubscriptionError::InvalidScope)?;
    Ok(ObjectOwner {
        tenant,
        agent: Some(agent),
    })
}

fn acknowledgement_target(acknowledgement: &CursorAcknowledgement) -> SubscriptionTarget {
    SubscriptionTarget {
        scope: acknowledgement.scope.clone(),
        subscription_id: acknowledgement.subscription_id.clone(),
    }
}

fn authorize_scope(
    token: &Token,
    sessions: &SessionRegistry,
    scope: &SubscriptionScope,
    operation: Operation,
    core_sequence: u64,
    observability: &mut TenantObservability,
) -> Result<(), SubscriptionError> {
    let context = RequestContext {
        surface: Surface::Subscription,
        operation,
        core_sequence,
        supplied_header_tenant: None,
        supplied_body_tenant: None,
        target_owner: Some(subscription_owner(scope)?),
    };
    tenant::resolve(token, sessions, &context, observability)
        .map(|_| ())
        .map_err(SubscriptionError::Authorization)
}

fn subscription_key(tenant: TenantId, id: &SubscriptionId) -> Result<TenantKey, StoreError> {
    TenantKey::new(
        tenant,
        ObjectKind::Subscription,
        id.as_str().as_bytes().to_vec(),
    )
}

fn encode_record(record: &DurableRecord) -> Result<Vec<u8>, SubscriptionError> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(RECORD_MAGIC);
    push_string(&mut bytes, record.public.subscription_id.as_str())?;
    push_string(&mut bytes, record.public.scope.tenant.as_str())?;
    push_string(&mut bytes, record.public.scope.agent.as_str())?;
    push_string(&mut bytes, record.public.scope.capability.as_str())?;
    match &record.session {
        None => bytes.push(0),
        Some(credential) => {
            if credential.tenant().as_str() != record.public.scope.tenant.as_str()
                || credential.token_id() == [0; 32]
                || credential.generation() == 0
            {
                return Err(SubscriptionError::Corrupt);
            }
            bytes.push(1);
            bytes.extend_from_slice(&credential.session_id().0);
            bytes.extend_from_slice(&credential.token_id());
            bytes.extend_from_slice(&credential.generation().to_be_bytes());
        }
    }
    push_string(&mut bytes, record.public.delivery_target.as_str())?;
    bytes.extend_from_slice(&record.public.start.0 .0.to_be_bytes());
    bytes.extend_from_slice(&record.public.last_acknowledged.0 .0.to_be_bytes());
    push_len(&mut bytes, record.delivered_unacknowledged.len())?;
    for cursor in &record.delivered_unacknowledged {
        bytes.extend_from_slice(&cursor.0.to_be_bytes());
    }
    bytes.push(u8::from(record.public.paused));
    encode_continuity(&mut bytes, record.continuity);
    bytes.push(match record.termination {
        None => 0,
        Some(Termination::Deleted) => 1,
        Some(Termination::SessionRevoked) => 2,
        Some(Termination::CapabilityRevoked) => 3,
        Some(Termination::TenantRevoked) => 4,
    });
    encode_filter(&mut bytes, &record.public.filter)?;
    Ok(bytes)
}

fn encode_filter(
    bytes: &mut Vec<u8>,
    filter: &SubscriptionFilter,
) -> Result<(), SubscriptionError> {
    push_len(bytes, filter.agents.values().len())?;
    for item in filter.agents.values() {
        push_string(bytes, item.tenant.as_str())?;
        push_string(bytes, item.value.as_str())?;
    }
    push_len(bytes, filter.accounts.values().len())?;
    for item in filter.accounts.values() {
        push_string(bytes, item.tenant.as_str())?;
        push_string(bytes, item.value.as_str())?;
    }
    push_len(bytes, filter.activity_types.values().len())?;
    for value in filter.activity_types.values() {
        bytes.extend_from_slice(&value.0.to_be_bytes());
    }
    push_len(bytes, filter.modules.values().len())?;
    for item in filter.modules.values() {
        push_string(bytes, item.tenant.as_str())?;
        push_string(bytes, item.value.as_str())?;
    }
    push_len(bytes, filter.assets.values().len())?;
    for item in filter.assets.values() {
        push_string(bytes, item.tenant.as_str())?;
        push_string(bytes, item.value.as_str())?;
    }
    push_len(bytes, filter.counterparties.values().len())?;
    for item in filter.counterparties.values() {
        push_string(bytes, item.tenant.as_str())?;
        push_string(bytes, item.value.as_str())?;
    }
    push_len(bytes, filter.result_classes.values().len())?;
    for value in filter.result_classes.values() {
        bytes.extend_from_slice(&value.raw().to_be_bytes());
    }
    Ok(())
}

fn decode_record(bytes: &[u8]) -> Result<(DurableRecord, bool), SubscriptionError> {
    let mut decoder = Decoder::new(bytes);
    let has_session_binding = match decoder.take(4)? {
        value if value == RECORD_MAGIC => true,
        value if value == LEGACY_RECORD_MAGIC => false,
        _ => return Err(SubscriptionError::Corrupt),
    };
    let subscription_id =
        SubscriptionId::new(decoder.string()?).map_err(|_| SubscriptionError::Corrupt)?;
    let tenant = ApiTenantId::new(decoder.string()?).map_err(|_| SubscriptionError::Corrupt)?;
    let agent = AgentDid::new(decoder.string()?).map_err(|_| SubscriptionError::Corrupt)?;
    let capability =
        CapabilityId::new(decoder.string()?).map_err(|_| SubscriptionError::Corrupt)?;
    let session = if has_session_binding {
        match decoder.byte()? {
            0 => None,
            1 => {
                let mut session_id = [0_u8; 32];
                session_id.copy_from_slice(decoder.take(32)?);
                let mut token_id = [0_u8; 32];
                token_id.copy_from_slice(decoder.take(32)?);
                let generation = decoder.u64()?;
                if token_id == [0; 32] || generation == 0 {
                    return Err(SubscriptionError::Corrupt);
                }
                Some(SessionCredential::new(
                    TenantId::new(tenant.as_str()).map_err(|_| SubscriptionError::Corrupt)?,
                    crate::session::SessionId(session_id),
                    token_id,
                    generation,
                ))
            }
            _ => return Err(SubscriptionError::Corrupt),
        }
    } else {
        None
    };
    let delivery_target =
        DeliveryTarget::new(decoder.string()?).map_err(|_| SubscriptionError::Corrupt)?;
    let start = ApiCursor(Sequence(decoder.u64()?));
    let last_acknowledged = ApiCursor(Sequence(decoder.u64()?));
    let delivered_count = decoder.len()?;
    let mut delivered_unacknowledged = BTreeSet::new();
    for _ in 0..delivered_count {
        if !delivered_unacknowledged.insert(Cursor(decoder.u64()?)) {
            return Err(SubscriptionError::Corrupt);
        }
    }
    let paused = decoder.byte()?;
    if paused > 1 {
        return Err(SubscriptionError::Corrupt);
    }
    let continuity = decode_continuity(&mut decoder)?;
    let termination = match decoder.byte()? {
        0 => None,
        1 => Some(Termination::Deleted),
        2 => Some(Termination::SessionRevoked),
        3 => Some(Termination::CapabilityRevoked),
        4 => Some(Termination::TenantRevoked),
        _ => return Err(SubscriptionError::Corrupt),
    };
    let scope = SubscriptionScope {
        tenant,
        agent,
        capability,
    };
    let filter = decode_filter(&mut decoder)?;
    if !decoder.is_empty()
        || validate_filter(&scope, &filter).is_err()
        || Cursor::from(last_acknowledged) < Cursor::from(start)
    {
        return Err(SubscriptionError::Corrupt);
    }
    if delivered_unacknowledged
        .iter()
        .any(|delivered| *delivered <= Cursor::from(last_acknowledged))
    {
        return Err(SubscriptionError::Corrupt);
    }
    Ok((
        DurableRecord {
            public: SubscriptionRecord {
                subscription_id,
                scope,
                filter,
                start,
                last_acknowledged,
                delivery_target,
                paused: paused == 1,
            },
            delivered_unacknowledged,
            current_delivery: Cursor::from(last_acknowledged),
            continuity,
            termination,
            session,
        },
        !has_session_binding,
    ))
}

fn encode_continuity(bytes: &mut Vec<u8>, continuity: Continuity) {
    match continuity {
        Continuity::Healthy => bytes.push(0),
        Continuity::GapBlocked {
            missing_first,
            missing_last,
            backfill_attempted,
            recovered_through,
        } => {
            bytes.push(1);
            bytes.extend_from_slice(&missing_first.to_be_bytes());
            bytes.extend_from_slice(&missing_last.to_be_bytes());
            bytes.push(u8::from(backfill_attempted));
            bytes.push(u8::from(recovered_through.is_some()));
            bytes.extend_from_slice(&recovered_through.unwrap_or(0).to_be_bytes());
        }
        Continuity::Truncated {
            requested_from,
            oldest_available,
            lost_through,
        } => {
            bytes.push(2);
            bytes.extend_from_slice(&requested_from.to_be_bytes());
            bytes.extend_from_slice(&oldest_available.to_be_bytes());
            bytes.extend_from_slice(&lost_through.to_be_bytes());
        }
    }
}

fn decode_continuity(decoder: &mut Decoder<'_>) -> Result<Continuity, SubscriptionError> {
    match decoder.byte()? {
        0 => Ok(Continuity::Healthy),
        1 => {
            let missing_first = decoder.u64()?;
            let missing_last = decoder.u64()?;
            let attempted = decoder.byte()?;
            let recovered_present = decoder.byte()?;
            let recovered = decoder.u64()?;
            if missing_last < missing_first || attempted > 1 || recovered_present > 1 {
                return Err(SubscriptionError::Corrupt);
            }
            let recovered_through = (recovered_present == 1).then_some(recovered);
            if recovered_through.is_some_and(|value| value < missing_first || value > missing_last)
                || (attempted == 0 && recovered_through.is_some())
            {
                return Err(SubscriptionError::Corrupt);
            }
            Ok(Continuity::GapBlocked {
                missing_first,
                missing_last,
                backfill_attempted: attempted == 1,
                recovered_through,
            })
        }
        2 => {
            let requested_from = decoder.u64()?;
            let oldest_available = decoder.u64()?;
            let lost_through = decoder.u64()?;
            if oldest_available <= requested_from
                || lost_through.saturating_add(1) != oldest_available
            {
                return Err(SubscriptionError::Corrupt);
            }
            Ok(Continuity::Truncated {
                requested_from,
                oldest_available,
                lost_through,
            })
        }
        _ => Err(SubscriptionError::Corrupt),
    }
}

fn decode_filter(decoder: &mut Decoder<'_>) -> Result<SubscriptionFilter, SubscriptionError> {
    let mut agents = Vec::new();
    for _ in 0..decoder.len()? {
        agents.push(TenantObject {
            tenant: api_tenant(decoder.string()?)?,
            value: AgentDid::new(decoder.string()?).map_err(|_| SubscriptionError::Corrupt)?,
        });
    }
    let mut accounts = Vec::new();
    for _ in 0..decoder.len()? {
        accounts.push(TenantObject {
            tenant: api_tenant(decoder.string()?)?,
            value: AccountRef::new(decoder.string()?).map_err(|_| SubscriptionError::Corrupt)?,
        });
    }
    let mut activity_types = Vec::new();
    for _ in 0..decoder.len()? {
        activity_types.push(ActivityType(decoder.u16()?));
    }
    let mut modules = Vec::new();
    for _ in 0..decoder.len()? {
        modules.push(TenantObject {
            tenant: api_tenant(decoder.string()?)?,
            value: ModuleRef::new(decoder.string()?).map_err(|_| SubscriptionError::Corrupt)?,
        });
    }
    let mut assets = Vec::new();
    for _ in 0..decoder.len()? {
        assets.push(TenantObject {
            tenant: api_tenant(decoder.string()?)?,
            value: Asset::new(decoder.string()?).map_err(|_| SubscriptionError::Corrupt)?,
        });
    }
    let mut counterparties = Vec::new();
    for _ in 0..decoder.len()? {
        counterparties.push(TenantObject {
            tenant: api_tenant(decoder.string()?)?,
            value: layerx_agent_api::identity::Counterparty::new(decoder.string()?)
                .map_err(|_| SubscriptionError::Corrupt)?,
        });
    }
    let mut result_classes = Vec::new();
    for _ in 0..decoder.len()? {
        result_classes.push(ResultCode::from_raw(decoder.i32()?));
    }
    Ok(SubscriptionFilter {
        agents: ExplicitSet::allow(agents),
        accounts: ExplicitSet::allow(accounts),
        activity_types: ExplicitSet::allow(activity_types),
        modules: ExplicitSet::allow(modules),
        assets: ExplicitSet::allow(assets),
        counterparties: ExplicitSet::allow(counterparties),
        result_classes: ExplicitSet::allow(result_classes),
    })
}

fn api_tenant(value: String) -> Result<ApiTenantId, SubscriptionError> {
    ApiTenantId::new(value).map_err(|_| SubscriptionError::Corrupt)
}

fn push_len(bytes: &mut Vec<u8>, value: usize) -> Result<(), SubscriptionError> {
    let value = u32::try_from(value).map_err(|_| SubscriptionError::Corrupt)?;
    bytes.extend_from_slice(&value.to_be_bytes());
    Ok(())
}

fn push_string(bytes: &mut Vec<u8>, value: &str) -> Result<(), SubscriptionError> {
    push_len(bytes, value.len())?;
    bytes.extend_from_slice(value.as_bytes());
    Ok(())
}

struct Decoder<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Decoder<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], SubscriptionError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(SubscriptionError::Corrupt)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(SubscriptionError::Corrupt)?;
        self.offset = end;
        Ok(value)
    }

    fn byte(&mut self) -> Result<u8, SubscriptionError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, SubscriptionError> {
        let mut value = [0_u8; 2];
        value.copy_from_slice(self.take(2)?);
        Ok(u16::from_be_bytes(value))
    }

    fn u32(&mut self) -> Result<u32, SubscriptionError> {
        let mut value = [0_u8; 4];
        value.copy_from_slice(self.take(4)?);
        Ok(u32::from_be_bytes(value))
    }

    fn i32(&mut self) -> Result<i32, SubscriptionError> {
        let mut value = [0_u8; 4];
        value.copy_from_slice(self.take(4)?);
        Ok(i32::from_be_bytes(value))
    }

    fn u64(&mut self) -> Result<u64, SubscriptionError> {
        let mut value = [0_u8; 8];
        value.copy_from_slice(self.take(8)?);
        Ok(u64::from_be_bytes(value))
    }

    fn len(&mut self) -> Result<usize, SubscriptionError> {
        usize::try_from(self.u32()?).map_err(|_| SubscriptionError::Corrupt)
    }

    fn string(&mut self) -> Result<String, SubscriptionError> {
        let length = self.len()?;
        let value =
            std::str::from_utf8(self.take(length)?).map_err(|_| SubscriptionError::Corrupt)?;
        Ok(value.to_owned())
    }

    const fn is_empty(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

//! Shared ordering authority for session-gated daemon effects.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard};

use crate::budget::BudgetLimiter;
use crate::events::outbound::StopSignal;
use crate::events::subscription::Termination;
use crate::human::{HumanOperationError, HumanResponse};
use crate::managed_agent;
use crate::prepare::{
    LifecycleError, LifecycleState, PreparationAuthorization, PreparationInvalidationReport,
    PreparationLifecycle, Prepared,
};
use crate::session::{
    self, InvalidationReport, PendingActivity, SessionCredential, SessionError, SessionId,
    SessionRegistry, Token,
};
use crate::store::{Store, TenantId, TenantKey};
use crate::tenant::{
    self, AuthorizationError, ObjectOwner, Operation, RequestContext, ResolvedPrincipal, Surface,
    TenantObservability,
};

/// The one shared owner of durable session state, its in-memory index, and exact-generation
/// operation tracking. Lock order is always registry, then store, then lifecycle/budget state.
#[derive(Clone)]
pub struct SessionControl {
    store: Arc<Mutex<Store>>,
    registry: Arc<RwLock<SessionRegistry>>,
    lifecycle: Arc<PreparationLifecycle>,
    budgets: Arc<BudgetLimiter>,
    observability: Arc<Mutex<TenantObservability>>,
    pending_invalidations: Arc<Mutex<BTreeMap<(session::SessionRef, u64), u64>>>,
}

impl SessionControl {
    #[must_use]
    pub fn new(
        store: Arc<Mutex<Store>>,
        registry: SessionRegistry,
        lifecycle: Arc<PreparationLifecycle>,
        budgets: Arc<BudgetLimiter>,
    ) -> Self {
        Self {
            store,
            registry: Arc::new(RwLock::new(registry)),
            lifecycle,
            budgets,
            observability: Arc::new(Mutex::new(TenantObservability::default())),
            pending_invalidations: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    #[must_use]
    pub fn registry(&self) -> Arc<RwLock<SessionRegistry>> {
        Arc::clone(&self.registry)
    }

    #[must_use]
    pub fn store(&self) -> Arc<Mutex<Store>> {
        Arc::clone(&self.store)
    }

    /// Authenticates an exact external credential, resolves its generated operation, and arms an
    /// exact-generation stop before any effect is allowed to begin.
    pub fn authorize(
        &self,
        credential: &SessionCredential,
        operation: Operation,
        surface: Surface,
        core_sequence: u64,
        target_owner: Option<ObjectOwner>,
    ) -> Result<OperationPermit, SessionControlError> {
        self.retry_preparation_invalidations()?;
        let mut registry = self
            .registry
            .write()
            .map_err(|_| SessionControlError::Unavailable)?;
        let token = registry
            .authenticate(credential)
            .map_err(SessionControlError::Session)?;
        let request = RequestContext {
            surface,
            operation,
            core_sequence,
            supplied_header_tenant: None,
            supplied_body_tenant: None,
            target_owner,
        };
        let principal = {
            let mut observability = self
                .observability
                .lock()
                .map_err(|_| SessionControlError::Unavailable)?;
            tenant::resolve(&token, &registry, &request, &mut observability)
                .map_err(SessionControlError::Authorization)?
        };
        let stop = registry
            .revocation_stop(&token)
            .map_err(SessionControlError::Session)?;
        Ok(OperationPermit {
            token,
            request,
            principal,
            stop,
        })
    }

    /// Closes one session with durable state committed before registry replacement and stop
    /// publication.
    pub fn close(
        &self,
        tenant: &TenantId,
        session_id: SessionId,
        current_sequence: u64,
    ) -> Result<PreparationInvalidationReport, SessionControlError> {
        let mut registry = self
            .registry
            .write()
            .map_err(|_| SessionControlError::Unavailable)?;
        let mut store = self
            .store
            .lock()
            .map_err(|_| SessionControlError::Unavailable)?;
        let generation = registry
            .generation(tenant, session_id)
            .ok_or(SessionControlError::Session(SessionError::NotFound))?;
        session::close(&mut store, &mut registry, tenant, session_id)
            .map_err(SessionControlError::Session)?;
        self.invalidate_preparations(
            &[(
                session::SessionRef::new(tenant.clone(), session_id),
                generation,
            )],
            current_sequence,
        )
    }

    pub fn close_with_companion(
        &self,
        tenant: &TenantId,
        session_id: SessionId,
        current_sequence: u64,
        companion_key: TenantKey,
        companion_bytes: Vec<u8>,
    ) -> Result<PreparationInvalidationReport, SessionControlError> {
        let mut registry = self
            .registry
            .write()
            .map_err(|_| SessionControlError::Unavailable)?;
        let mut store = self
            .store
            .lock()
            .map_err(|_| SessionControlError::Unavailable)?;
        let generation = registry
            .generation(tenant, session_id)
            .ok_or(SessionControlError::Session(SessionError::NotFound))?;
        session::close_with_companion(
            &mut store,
            &mut registry,
            tenant,
            session_id,
            companion_key,
            companion_bytes,
        )
        .map_err(SessionControlError::Session)?;
        self.invalidate_preparations(
            &[(
                session::SessionRef::new(tenant.clone(), session_id),
                generation,
            )],
            current_sequence,
        )
    }

    /// Atomically rotates an opaque bearer while narrowing scope and updating the Human-managed
    /// agent coordinate that distributes the new bearer.
    pub fn restrict_with_companion(
        &self,
        tenant: &TenantId,
        session_id: SessionId,
        replacement_token: [u8; 32],
        scopes: BTreeSet<String>,
        permitted_activity_types: BTreeSet<u16>,
        current_sequence: u64,
        coordinate_key: TenantKey,
        coordinate_bytes: Vec<u8>,
        companion_key: TenantKey,
        companion_bytes: Vec<u8>,
    ) -> Result<(Token, PreparationInvalidationReport), SessionControlError> {
        let mut registry = self
            .registry
            .write()
            .map_err(|_| SessionControlError::Unavailable)?;
        let generation = registry
            .generation(tenant, session_id)
            .ok_or(SessionControlError::Session(SessionError::NotFound))?;
        let mut store = self
            .store
            .lock()
            .map_err(|_| SessionControlError::Unavailable)?;
        let token = session::restrict_scope_with_companion(
            &mut store,
            &mut registry,
            tenant,
            session_id,
            replacement_token,
            scopes,
            permitted_activity_types,
            coordinate_key,
            coordinate_bytes,
            companion_key,
            companion_bytes,
        )
        .map_err(SessionControlError::Session)?;
        let preparations = self.invalidate_preparations(
            &[(
                session::SessionRef::new(tenant.clone(), session_id),
                generation,
            )],
            current_sequence,
        )?;
        Ok((token, preparations))
    }

    /// Human-admin restriction entry point. The replacement bearer is generated inside the
    /// daemon and the session record, managed-agent coordinate, and observation commit together.
    pub fn restrict_managed_agent(
        &self,
        tenant: &TenantId,
        agent_id: &str,
        scopes: BTreeSet<String>,
        permitted_activity_types: BTreeSet<u16>,
        current_sequence: u64,
        action_key: [u8; 32],
    ) -> Result<(Token, PreparationInvalidationReport, HumanResponse), SessionControlError> {
        let mut registry = self
            .registry
            .write()
            .map_err(|_| SessionControlError::Unavailable)?;
        let mut store = self
            .store
            .lock()
            .map_err(|_| SessionControlError::Unavailable)?;
        let (_, session_id, current_token, coordinate_generation) =
            managed_agent::session_coordinates(&store, tenant, agent_id)
                .map_err(SessionControlError::Human)?;
        let session_id = SessionId(session_id);
        let record = registry
            .get(tenant, session_id)
            .cloned()
            .ok_or(SessionControlError::Session(SessionError::NotFound))?;
        if !record.open
            || record.request.token_id != current_token
            || record.generation != coordinate_generation
        {
            return Err(SessionControlError::Session(SessionError::Revoked));
        }
        if let Some(replay) = managed_agent::replay_session_token_restriction(
            &store,
            tenant,
            agent_id,
            action_key,
            current_sequence,
            &scopes,
            &permitted_activity_types,
        )
        .map_err(SessionControlError::Human)?
        {
            if replay.session_id != session_id.0
                || replay.replacement_token != current_token
                || replay.generation != record.generation
                || record.request.scopes != scopes
                || record.request.permitted_activity_types != permitted_activity_types
            {
                return Err(SessionControlError::Session(SessionError::Revoked));
            }
            let token = registry
                .authenticate_bearer(tenant, session_id, replay.replacement_token)
                .map_err(SessionControlError::Session)?;
            let preparations = self.retry_preparation_invalidations()?;
            return Ok((token, preparations, replay.response));
        }
        let replacement_generation =
            record
                .generation
                .checked_add(1)
                .ok_or(SessionControlError::Session(
                    SessionError::GenerationExhausted,
                ))?;
        let mut replacement_token = [0_u8; 32];
        let mut available = false;
        for _ in 0..8 {
            getrandom::fill(&mut replacement_token)
                .map_err(|_| SessionControlError::Unavailable)?;
            if replacement_token != [0; 32]
                && replacement_token != current_token
                && !record.retired_token_ids.contains(&replacement_token)
            {
                available = true;
                break;
            }
        }
        if !available {
            return Err(SessionControlError::Unavailable);
        }
        let (
            response,
            coordinate_key,
            coordinate_bytes,
            companion_key,
            companion_bytes,
            ledger_key,
            ledger_bytes,
        ) = managed_agent::prepare_session_token_restriction(
            &store,
            tenant,
            agent_id,
            session_id.0,
            current_token,
            coordinate_generation,
            replacement_token,
            action_key,
            replacement_generation,
            current_sequence,
            &scopes,
            &permitted_activity_types,
        )
        .map_err(SessionControlError::Human)?;
        let token = session::restrict_scope_with_companions(
            &mut store,
            &mut registry,
            tenant,
            session_id,
            replacement_token,
            scopes,
            permitted_activity_types,
            coordinate_key,
            coordinate_bytes,
            vec![(companion_key, companion_bytes), (ledger_key, ledger_bytes)],
        )
        .map_err(SessionControlError::Session)?;
        let preparations = self.invalidate_preparations(
            &[(
                session::SessionRef::new(tenant.clone(), session_id),
                record.generation,
            )],
            current_sequence,
        )?;
        Ok((token, preparations, response))
    }

    /// Commits a separately verified finalized authority revocation as one short ordered batch.
    /// The opaque value can only be produced by managed-agent receipt/evidence validation.
    pub fn invalidate_finalized(
        &self,
        finalized: &managed_agent::ValidatedAuthorityRevocation,
    ) -> Result<(InvalidationReport, PreparationInvalidationReport), SessionControlError> {
        let event = finalized.event();
        let mut registry = self
            .registry
            .write()
            .map_err(|_| SessionControlError::Unavailable)?;
        let mut store = self
            .store
            .lock()
            .map_err(|_| SessionControlError::Unavailable)?;
        let mut detached: [PendingActivity; 0] = [];
        let report =
            session::invalidate_on_revocation(&mut store, &mut registry, &mut detached, event)
                .map_err(SessionControlError::Session)?;
        let preparations =
            self.invalidate_preparations(&report.invalidated_generations, event.observed_sequence)?;
        Ok((report, preparations))
    }

    fn invalidate_preparations(
        &self,
        invalidated: &[(session::SessionRef, u64)],
        current_sequence: u64,
    ) -> Result<PreparationInvalidationReport, SessionControlError> {
        {
            let mut pending = self
                .pending_invalidations
                .lock()
                .map_err(|_| SessionControlError::Unavailable)?;
            for (session, generation) in invalidated {
                pending
                    .entry((session.clone(), *generation))
                    .and_modify(|sequence| *sequence = (*sequence).max(current_sequence))
                    .or_insert(current_sequence);
            }
        }
        self.retry_preparation_invalidations()
    }

    /// Retries exact-generation preparation cleanup. Authorization of those preparations never
    /// depends on cleanup succeeding: every transition still requires a current exact permit.
    pub fn retry_preparation_invalidations(
        &self,
    ) -> Result<PreparationInvalidationReport, SessionControlError> {
        let selected = {
            let pending = self
                .pending_invalidations
                .lock()
                .map_err(|_| SessionControlError::Unavailable)?;
            pending
                .iter()
                .map(|((session, generation), sequence)| (session.clone(), *generation, *sequence))
                .collect::<Vec<_>>()
        };
        if selected.is_empty() {
            return Ok(PreparationInvalidationReport::default());
        }
        let invalidated = selected
            .iter()
            .map(|(session, generation, _)| (session.clone(), *generation))
            .collect::<Vec<_>>();
        let sequence = selected
            .iter()
            .map(|(_, _, sequence)| *sequence)
            .max()
            .ok_or(SessionControlError::Unavailable)?;
        let report = self
            .lifecycle
            .invalidate_authorizations(&invalidated, sequence, &self.budgets)
            .map_err(SessionControlError::Lifecycle)?;
        let mut pending = self
            .pending_invalidations
            .lock()
            .map_err(|_| SessionControlError::Unavailable)?;
        for (session, generation, _) in selected {
            pending.remove(&(session, generation));
        }
        Ok(report)
    }
}

/// Exact-generation authorization retained across a bounded daemon operation.
pub struct OperationPermit {
    token: Token,
    request: RequestContext,
    principal: ResolvedPrincipal,
    stop: StopSignal,
}

impl OperationPermit {
    #[must_use]
    pub const fn principal(&self) -> &ResolvedPrincipal {
        &self.principal
    }

    #[must_use]
    pub fn credential(&self) -> SessionCredential {
        self.token.credential()
    }

    #[must_use]
    pub const fn operation(&self) -> Operation {
        self.request.operation
    }

    #[must_use]
    pub const fn stop(&self) -> &StopSignal {
        &self.stop
    }

    #[must_use]
    fn preparation_authorization(&self) -> PreparationAuthorization {
        PreparationAuthorization {
            session: session::SessionRef::new(self.token.tenant().clone(), self.token.session_id()),
            generation: self.token.generation(),
        }
    }

    pub fn register_preparation(
        &self,
        control: &SessionControl,
        preparation_id: [u8; 32],
        prepared: &Prepared,
        reservation_ids: Vec<[u8; 32]>,
    ) -> Result<(), SessionControlError> {
        self.require_operation(Operation::Prepare)?;
        let registry = control
            .registry
            .read()
            .map_err(|_| SessionControlError::Unavailable)?;
        self.resolve(control, &registry)?;
        control
            .lifecycle
            .register_authorized(
                preparation_id,
                prepared,
                reservation_ids,
                self.preparation_authorization(),
            )
            .map_err(SessionControlError::Lifecycle)
    }

    pub fn transition_preparation(
        &self,
        control: &SessionControl,
        preparation_id: [u8; 32],
        next: LifecycleState,
        current_sequence: u64,
    ) -> Result<(), SessionControlError> {
        let expected = match next {
            LifecycleState::Signing | LifecycleState::Signed => Operation::Sign,
            LifecycleState::Submitted => Operation::Submit,
            LifecycleState::Acknowledged
            | LifecycleState::Unknown
            | LifecycleState::Executed
            | LifecycleState::Failed
            | LifecycleState::Expired => Operation::Track,
            LifecycleState::Prepared => {
                return Err(SessionControlError::Authorization(
                    AuthorizationError::ScopeDenied,
                ))
            }
        };
        self.require_operation(expected)?;
        let registry = control
            .registry
            .read()
            .map_err(|_| SessionControlError::Unavailable)?;
        self.resolve(control, &registry)?;
        control
            .lifecycle
            .transition_authorized(
                preparation_id,
                next,
                current_sequence,
                &self.preparation_authorization(),
            )
            .map_err(SessionControlError::Lifecycle)
    }

    pub fn admit_submission(
        &self,
        control: &SessionControl,
        preparation_id: [u8; 32],
        core_batch_time: u64,
    ) -> Result<(), SessionControlError> {
        self.require_operation(Operation::Submit)?;
        let registry = control
            .registry
            .read()
            .map_err(|_| SessionControlError::Unavailable)?;
        self.resolve(control, &registry)?;
        control
            .lifecycle
            .admit_submission_authorized(
                preparation_id,
                core_batch_time,
                &self.preparation_authorization(),
            )
            .map_err(SessionControlError::Lifecycle)
    }

    pub fn retain_signed_bytes(
        &self,
        control: &SessionControl,
        preparation_id: [u8; 32],
        signed_bytes: Vec<u8>,
        activity_id: [u8; 32],
    ) -> Result<(), SessionControlError> {
        self.require_operation(Operation::Sign)?;
        let registry = control
            .registry
            .read()
            .map_err(|_| SessionControlError::Unavailable)?;
        self.resolve(control, &registry)?;
        control
            .lifecycle
            .retain_signed_bytes_authorized(
                preparation_id,
                signed_bytes,
                activity_id,
                &self.preparation_authorization(),
            )
            .map_err(SessionControlError::Lifecycle)
    }

    fn require_operation(&self, expected: Operation) -> Result<(), SessionControlError> {
        if self.request.operation == expected {
            Ok(())
        } else {
            Err(SessionControlError::Authorization(
                AuthorizationError::ScopeDenied,
            ))
        }
    }

    /// Re-resolves at a non-mutating boundary. A result computed outside the registry lock must
    /// not be released unless this succeeds afterwards.
    pub fn boundary(&self, control: &SessionControl) -> Result<(), SessionControlError> {
        let registry = control
            .registry
            .read()
            .map_err(|_| SessionControlError::Unavailable)?;
        self.resolve(control, &registry).map(|_| ())
    }

    /// Linearizes one irreversible effect with close, restriction, and revocation. Preparatory,
    /// side-effect-free I/O may occur before this method, but transmission, durable approval, or
    /// any other externally visible commit must occur inside it or behind a stop-aware two-phase
    /// boundary that calls it for the actual commit.
    pub fn commit<T>(
        &self,
        control: &SessionControl,
        commit: impl FnOnce() -> Result<T, SessionControlError>,
    ) -> Result<T, SessionControlError> {
        let registry = control
            .registry
            .read()
            .map_err(|_| SessionControlError::Unavailable)?;
        self.resolve(control, &registry)?;
        let value = commit()?;
        self.resolve(control, &registry)?;
        Ok(value)
    }

    fn resolve<'a>(
        &self,
        control: &SessionControl,
        registry: &'a RwLockReadGuard<'a, SessionRegistry>,
    ) -> Result<ResolvedPrincipal, SessionControlError> {
        if self.stop.reason() == Some(Termination::SessionRevoked) {
            return Err(SessionControlError::Authorization(
                AuthorizationError::Revoked,
            ));
        }
        let mut observability = control
            .observability
            .lock()
            .map_err(|_| SessionControlError::Unavailable)?;
        tenant::resolve(&self.token, registry, &self.request, &mut observability)
            .map_err(SessionControlError::Authorization)
    }
}

#[derive(Debug)]
pub enum SessionControlError {
    Session(SessionError),
    Authorization(AuthorizationError),
    Lifecycle(LifecycleError),
    Human(HumanOperationError),
    Unavailable,
}

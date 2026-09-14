use super::*;
use crate::budget::{NativeBudgetReconciliation, NativeBudgetRuntime};

fn now_ms() -> Result<u64, HumanOperationError> {
    use layerx_types::clock::Clock as _;
    static CLOCK: std::sync::OnceLock<
        Result<Arc<layerx_client::runtime_clock::RuntimeClock>, layerx_types::clock::ClockError>,
    > = std::sync::OnceLock::new();
    CLOCK
        .get_or_init(layerx_client::runtime_clock::RuntimeClock::from_environment)
        .as_ref()
        .map_err(|_| HumanOperationError::Unavailable)?
        .sample(std::time::Duration::from_secs(1))
        .map(|reading| reading.unix_milliseconds)
        .map_err(|_| HumanOperationError::Unavailable)
}

impl<A: HumanAuthorityBoundary> ProductionHumanOperations<A> {
    /// # Errors
    /// Refuses unprotected configured trust or any unresolved native Budget recovery failure.
    pub fn configure_native_budget_recovery(
        &mut self,
        source: &std::path::Path,
        peers: &BTreeMap<u32, (String, String)>,
    ) -> Result<(), HumanOperationError> {
        let node = self.node.handshake().node();
        let authority = crate::protocol_evidence::EvidenceAuthority::native_budget_authority(
            node.protocol_version,
            node.network_id,
            source,
        )
        .map_err(|_| HumanOperationError::Refused)?;
        self.native_budgets = Some(NativeBudgetRuntime::new(authority));
        let peers = {
            let store = self
                .store
                .lock()
                .map_err(|_| HumanOperationError::Unavailable)?;
            subject::restore_peers(&store, peers)?
        };
        for peer in peers {
            let registry = self.authority.registry(&peer).map_err(map_core)?;
            let ids = self.native_budget_ids(&peer, &registry)?;
            for id in ids {
                self.reconcile_native_budget(&peer, id)?;
            }
        }
        Ok(())
    }

    fn native_budget_ids(
        &self,
        peer: &HumanPeer,
        registry: &ModuleRegistry,
    ) -> Result<std::collections::BTreeSet<[u8; 32]>, HumanOperationError> {
        let mut ids = std::collections::BTreeSet::new();
        if let Some(outbox) = self.outboxes.get(&peer.tenant) {
            for status in outbox.statuses() {
                if let Some((id, _)) = crate::budget::native_spend(
                    outbox
                        .exact_signed_bytes(status.submission_id)
                        .map_err(|_| HumanOperationError::Refused)?,
                    registry,
                )
                .map_err(|_| HumanOperationError::Refused)?
                {
                    ids.insert(id);
                }
            }
        }
        Ok(ids)
    }

    fn require_native_budget_recovery(
        &self,
        peers: &BTreeMap<u32, (String, String)>,
    ) -> Result<(), HumanOperationError> {
        let peers = {
            let store = self
                .store
                .lock()
                .map_err(|_| HumanOperationError::Unavailable)?;
            subject::restore_peers(&store, peers)?
        };
        for peer in peers {
            let registry = self.authority.registry(&peer).map_err(map_core)?;
            let tenant =
                TenantId::new(peer.tenant.clone()).map_err(|_| HumanOperationError::Refused)?;
            for id in self.native_budget_ids(&peer, &registry)? {
                if self
                    .native_budgets
                    .as_ref()
                    .and_then(|runtime| runtime.ceiling(&tenant, id))
                    .is_none()
                {
                    return Err(HumanOperationError::Unavailable);
                }
            }
        }
        Ok(())
    }

    /// # Errors
    /// Refuses absent configured identity, untrusted proofs or incomplete history before updating this owner's holds.
    pub fn reconcile_native_budget(
        &mut self,
        peer: &HumanPeer,
        budget: [u8; 32],
    ) -> Result<NativeBudgetReconciliation, HumanOperationError> {
        self.authority.authorize_subject(peer)?;
        let tenant =
            TenantId::new(peer.tenant.clone()).map_err(|_| HumanOperationError::Refused)?;
        let registry = self.authority.registry(peer).map_err(map_core)?;
        let shared = Arc::clone(&self.store);
        let mut store = shared
            .lock()
            .map_err(|_| HumanOperationError::Unavailable)?;
        let scope = managed_agent::native_budget_scope(&store, &tenant, budget)?;
        if scope.network_id != self.node.handshake().node().network_id {
            return Err(HumanOperationError::Refused);
        }
        self.native_budgets
            .as_mut()
            .ok_or(HumanOperationError::Unavailable)?
            .reconcile(
                &mut store,
                &tenant,
                &mut self.node,
                &registry,
                &scope,
                self.outboxes.entry(peer.tenant.clone()).or_default(),
            )
            .map_err(|_| HumanOperationError::Unavailable)
    }

    fn reserve_native_spend(
        &mut self,
        peer: &HumanPeer,
        bytes: &[u8],
        registry: &ModuleRegistry,
    ) -> Result<Option<[u8; 32]>, HumanOperationError> {
        let Some((id, reservation)) = crate::budget::native_spend(bytes, registry)
            .map_err(|_| HumanOperationError::Refused)?
        else {
            return Ok(None);
        };
        let evidence = self.reconcile_native_budget(peer, id)?;
        let tenant =
            TenantId::new(peer.tenant.clone()).map_err(|_| HumanOperationError::Refused)?;
        self.native_budgets
            .as_mut()
            .ok_or(HumanOperationError::Unavailable)?
            .reserve(
                &tenant,
                id,
                reservation,
                now_ms()?,
                evidence.observed_sequence(),
            )
            .map_err(|_| HumanOperationError::Refused)?;
        Ok(Some(id))
    }

    fn authorize_native_preparation(
        &mut self,
        peer: &HumanPeer,
        prepared: &crate::prepare::Prepared,
    ) -> Result<(), HumanOperationError> {
        if prepared.envelope.activity_type().value() != 0x0003_0006 {
            return Ok(());
        }
        let payload = prepared.envelope.payload().as_bytes();
        if payload.len() != 82 || payload[..2] != [0, 1] {
            return Err(HumanOperationError::Refused);
        }
        let budget = payload[2..34]
            .try_into()
            .map_err(|_| HumanOperationError::Refused)?;
        let amount = u128::from_be_bytes(
            payload[66..82]
                .try_into()
                .map_err(|_| HumanOperationError::Refused)?,
        );
        let evidence = self.reconcile_native_budget(peer, budget)?;
        if prepared.envelope.actor_did() != &evidence.binding().owner_did
            || prepared.envelope.authority().as_bytes() != evidence.binding().owner_public_key
            || prepared.observed_head_sequence != evidence.observed_sequence()
        {
            return Err(HumanOperationError::Refused);
        }
        let tenant =
            TenantId::new(peer.tenant.clone()).map_err(|_| HumanOperationError::Refused)?;
        self.native_budgets
            .as_ref()
            .ok_or(HumanOperationError::Unavailable)?
            .authorize_preparation(
                &tenant,
                budget,
                amount,
                prepared.envelope.timestamp_bound().not_after(),
                now_ms()?,
                evidence.observed_sequence(),
            )
            .map_err(|_| HumanOperationError::Refused)
    }

    fn native_dispatch_hold(
        &mut self,
        peer: &HumanPeer,
        id: [u8; 32],
        registry: &ModuleRegistry,
    ) -> Result<(), HumanOperationError> {
        let bytes = self
            .outboxes
            .get(&peer.tenant)
            .ok_or(HumanOperationError::Refused)?
            .exact_signed_bytes(id)
            .map_err(|_| HumanOperationError::Refused)?;
        let Some((budget, _)) = crate::budget::native_spend(bytes, registry)
            .map_err(|_| HumanOperationError::Refused)?
        else {
            return Ok(());
        };
        let tenant =
            TenantId::new(peer.tenant.clone()).map_err(|_| HumanOperationError::Refused)?;
        self.native_budgets
            .as_mut()
            .ok_or(HumanOperationError::Unavailable)?
            .mark_unknown(&tenant, budget, id)
            .map_err(|_| HumanOperationError::Refused)
    }

    fn track_native_budget(
        &mut self,
        peer: &HumanPeer,
        id: [u8; 32],
    ) -> Result<Option<HumanResponse>, HumanOperationError> {
        let registry = self.authority.registry(peer).map_err(map_core)?;
        let bytes = self
            .outboxes
            .get(&peer.tenant)
            .ok_or(HumanOperationError::Refused)?
            .exact_signed_bytes(id)
            .map_err(|_| HumanOperationError::Refused)?
            .to_vec();
        let Some((budget, reservation)) = crate::budget::native_spend(&bytes, &registry)
            .map_err(|_| HumanOperationError::Refused)?
        else {
            return Ok(None);
        };
        let evidence = self.reconcile_native_budget(peer, budget)?;
        let status = self
            .outboxes
            .get(&peer.tenant)
            .and_then(|outbox| outbox.status(id))
            .ok_or(HumanOperationError::Refused)?
            .clone();
        if status.state == SubmissionState::Queued {
            return self.resume_queued(peer, id, status.activity_id).map(Some);
        }
        if status.state == SubmissionState::Unknown
            && now_ms()? < reservation.expiry_ms
            && self.native_retry_due(peer, id)?
        {
            let activity = layerx_wire::activity::decode_signed(&bytes, &registry)
                .map_err(|_| HumanOperationError::Refused)?;
            let signer = activity
                .authority()
                .try_into()
                .map_err(|_| HumanOperationError::Refused)?;
            self.native_dispatch_hold(peer, id, &registry)?;
            let _outcome = self.node.submit_signed(&registry, signer, 6300, 0, &bytes);
        }
        if let Some(outcome) = evidence
            .outcomes()
            .iter()
            .find(|outcome| outcome.activity_id() == status.activity_id)
        {
            self.last_verified_receipt = Some((id, outcome.result_code, outcome.sequence));
        }
        Self::observation(
            self.outboxes
                .get(&peer.tenant)
                .and_then(|outbox| outbox.status(id))
                .ok_or(HumanOperationError::Refused)?,
        )
        .map(Some)
    }

    fn native_retry_due(
        &mut self,
        peer: &HumanPeer,
        id: [u8; 32],
    ) -> Result<bool, HumanOperationError> {
        let mut store = self
            .store
            .lock()
            .map_err(|_| HumanOperationError::Unavailable)?;
        self.outboxes
            .get(&peer.tenant)
            .ok_or(HumanOperationError::Refused)?
            .begin_native_retry(&mut store, id, now_ms()?)
            .map_err(|_| HumanOperationError::Unavailable)
    }
}

impl<A: HumanAuthorityBoundary> UnifiedAgentOwner<A> {
    /// # Errors
    /// Reconciles this same daemon owner only after subject authorization and complete native evidence.
    pub fn reconcile_native_budget(
        &self,
        peer: &HumanPeer,
        budget: [u8; 32],
    ) -> Result<NativeBudgetReconciliation, HumanOperationError> {
        self.lock_operations()?
            .reconcile_native_budget(peer, budget)
    }
}

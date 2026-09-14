use super::{
    native_io, NativeBudgetBinding, NativeBudgetError as Error, NativeBudgetReconciliation,
};
use crate::capability::{NativeCeiling, NativeReservation};
use crate::outbox::{Outbox, SubmissionState};
use crate::protocol_evidence::EvidenceAuthority;
use crate::store::{Store, TenantId};
use layerx_client::Client;
use layerx_types::payload::ModuleRegistry;
use layerx_wire::{activity::decode_signed, hash::activity_id, native_budget::BudgetRecord};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
pub struct NativeBudgetScope {
    pub network_id: u32,
    pub write_enabled: bool,
    pub binding: NativeBudgetBinding,
    pub maximum: u128,
    pub maximum_lifetime_ms: u64,
}

pub struct NativeBudgetRuntime {
    authority: EvidenceAuthority,
    ceilings: BTreeMap<(TenantId, [u8; 32]), NativeCeiling>,
    paused: BTreeSet<(TenantId, [u8; 32])>,
}

pub(crate) fn spend(
    bytes: &[u8],
    registry: &ModuleRegistry,
) -> Result<Option<([u8; 32], NativeReservation)>, Error> {
    let activity = decode_signed(bytes, registry).map_err(|_| Error::Activity)?;
    if activity.activity_type().value() != 0x0003_0006 {
        return Ok(None);
    }
    let payload = activity.payload();
    if payload.len() != 82 || payload[..2] != [0, 1] {
        return Err(Error::Activity);
    }
    let id = payload[2..34].try_into().map_err(|_| Error::Activity)?;
    let amount = u128::from_be_bytes(payload[66..82].try_into().map_err(|_| Error::Activity)?);
    if id == [0; 32] || amount == 0 {
        return Err(Error::Activity);
    }
    Ok(Some((
        id,
        NativeReservation {
            id: activity.idempotency_key(),
            expected_activity_id: activity_id(&activity).map_err(|_| Error::Activity)?,
            amount,
            expiry_ms: activity.timestamp_bound().not_after,
            unknown: false,
        },
    )))
}

impl NativeBudgetRuntime {
    #[must_use]
    pub fn new(authority: EvidenceAuthority) -> Self {
        Self {
            authority,
            ceilings: BTreeMap::new(),
            paused: BTreeSet::new(),
        }
    }

    /// # Errors
    /// Refuses missing configured identity, invalid finality, incomplete history and unmatched durable holds.
    pub fn reconcile(
        &mut self,
        store: &mut Store,
        tenant: &TenantId,
        node: &mut Client,
        registry: &ModuleRegistry,
        scope: &NativeBudgetScope,
        outbox: &mut Outbox,
    ) -> Result<NativeBudgetReconciliation, Error> {
        if scope.network_id != node.handshake().node().network_id {
            return Err(Error::Binding);
        }
        let retained = native_io::baseline(store, tenant, scope.binding.budget_id)?;
        let mut read_binding = scope.binding.clone();
        if let Some(anchor) = &retained {
            read_binding.owner_public_key = layerx_proof::state::decode_account_value(
                read_binding.owner_account,
                &anchor.owner.canonical_value,
            )
            .map_err(|_| Error::AccountProof)?
            .authority_key
            .ok_or(Error::Binding)?;
        }
        let current = native_io::current(node, &self.authority, &read_binding)?;
        let record = BudgetRecord::decode(&current.canonical_record).map_err(|_| Error::Record)?;
        if record.period_length != scope.binding.period_length_ms
            || record
                .expiry
                .checked_sub(record.period_start)
                .ok_or(Error::Window)?
                > scope.maximum_lifetime_ms
        {
            return Err(Error::Window);
        }
        let baseline = retained.as_ref().unwrap_or(&current).clone();
        let base = BudgetRecord::decode(&baseline.canonical_record).map_err(|_| Error::Baseline)?;
        let mut binding = scope.binding.clone();
        binding.period_start_ms = record.period_start;
        if retained.is_some() {
            binding.owner_public_key = layerx_proof::state::decode_account_value(
                binding.owner_account,
                &current.owner.canonical_value,
            )
            .map_err(|_| Error::AccountProof)?
            .authority_key
            .ok_or(Error::Binding)?;
        }
        binding.expiry_ms = base.expiry;
        let evidence = native_io::history(node, &self.authority, registry, baseline, current)?;
        let reconciled = if retained.is_some() {
            let mut old_binding = binding.clone();
            old_binding.period_start_ms = base.period_start;
            old_binding.owner_public_key = read_binding.owner_public_key;
            let previous = self
                .authority
                .restore_native_anchor(&old_binding, &evidence.baseline)?;
            self.authority
                .advance_native_budget(&binding, &evidence, &previous)?
        } else {
            self.authority
                .reconcile_native_budget(&binding, &evidence)?
        };
        if let Some(previous) = self.ceilings.get(&(tenant.clone(), binding.budget_id)) {
            previous
                .clone()
                .reconcile(reconciled.clone())
                .map_err(|_| Error::Consumption)?;
        }
        let reservations =
            self.reconcile_outbox(store, tenant, node, registry, &reconciled, outbox)?;
        let ceiling = NativeCeiling::rebuild(scope.maximum, reconciled.clone(), &reservations)
            .map_err(|_| Error::Consumption)?;
        if retained.is_none() {
            if !reservations.is_empty() {
                return Err(Error::Baseline);
            }
            native_io::retain_baseline(store, tenant, binding.budget_id, &evidence.baseline)?;
        }
        for reservation in &reservations {
            if let Some(outcome) = reconciled
                .outcomes()
                .iter()
                .find(|outcome| outcome.activity_id() == reservation.expected_activity_id)
            {
                outbox
                    .settle_native_budget(store, reservation.id, outcome)
                    .map_err(|_| Error::Outbox)?;
            }
        }
        native_io::advance_anchor(store, tenant, binding.budget_id, &evidence.current)?;
        if scope.write_enabled {
            self.paused.remove(&(tenant.clone(), binding.budget_id));
        } else {
            self.paused.insert((tenant.clone(), binding.budget_id));
        }
        self.ceilings
            .insert((tenant.clone(), binding.budget_id), ceiling);
        Ok(reconciled)
    }

    fn reconcile_outbox(
        &self,
        store: &mut Store,
        tenant: &TenantId,
        node: &mut Client,
        registry: &ModuleRegistry,
        reconciled: &NativeBudgetReconciliation,
        outbox: &mut Outbox,
    ) -> Result<Vec<NativeReservation>, Error> {
        let binding = reconciled.binding();
        let mut reservations = Vec::new();
        for status in outbox.statuses() {
            let Some((budget_id, mut reservation)) = spend(
                outbox
                    .exact_signed_bytes(status.submission_id)
                    .map_err(|_| Error::Outbox)?,
                registry,
            )?
            else {
                continue;
            };
            if budget_id != binding.budget_id {
                continue;
            }
            if reservation.id != status.submission_id
                || reservation.expected_activity_id != status.activity_id
            {
                return Err(Error::Outbox);
            }
            match status.state {
                SubmissionState::Prepared | SubmissionState::Signed => return Err(Error::Outbox),
                SubmissionState::Expired | SubmissionState::Superseded => {
                    if status
                        .transitions
                        .iter()
                        .any(|transition| transition.to == SubmissionState::Submitted)
                    {
                        return Err(Error::Outbox);
                    }
                    continue;
                }
                SubmissionState::Executed | SubmissionState::Failed => {
                    let outcome = native_io::terminal(
                        node,
                        &self.authority,
                        binding,
                        store,
                        tenant,
                        native_io::TerminalRequest {
                            id: status.submission_id,
                            exact: outbox
                                .exact_signed_bytes(status.submission_id)
                                .map_err(|_| Error::Outbox)?,
                            maximum_sequence: reconciled.observed_sequence(),
                        },
                    )?;
                    if outcome.succeeded() != (status.state == SubmissionState::Executed)
                        || outcome.activity_id() != status.activity_id
                        || status
                            .evidence
                            .is_none_or(|value| value.receipt_ref() != outcome.receipt_digest())
                    {
                        return Err(Error::Receipt);
                    }
                    continue;
                }
                SubmissionState::Queued
                | SubmissionState::Submitted
                | SubmissionState::Acknowledged
                | SubmissionState::Unknown => {}
            }
            reservation.unknown = status.state != SubmissionState::Queued;
            if matches!(
                status.state,
                SubmissionState::Submitted | SubmissionState::Acknowledged
            ) {
                outbox
                    .transition(
                        store,
                        status.submission_id,
                        SubmissionState::Unknown,
                        "native Budget restart retained the indeterminate dispatch",
                        None,
                    )
                    .map_err(|_| Error::Outbox)?;
            }
            reservations.push(reservation);
        }
        Ok(reservations)
    }

    /// # Errors
    /// Refuses missing reconciliation, expired timestamps and checked capacity exhaustion.
    pub fn reserve(
        &mut self,
        tenant: &TenantId,
        id: [u8; 32],
        reservation: NativeReservation,
        now_ms: u64,
        sequence: u64,
    ) -> Result<(), Error> {
        if self.paused.contains(&(tenant.clone(), id)) {
            return Err(Error::Binding);
        }
        self.ceilings
            .get_mut(&(tenant.clone(), id))
            .ok_or(Error::Baseline)?
            .reserve(reservation, now_ms, sequence)
            .map_err(|_| Error::Consumption)
    }

    /// # Errors
    /// Refuses paused scopes, insufficient held capacity and timestamps outside the authenticated period.
    pub fn authorize_preparation(
        &self,
        tenant: &TenantId,
        budget: [u8; 32],
        amount: u128,
        expiry_ms: u64,
        current_ms: u64,
        sequence: u64,
    ) -> Result<(), Error> {
        if self.paused.contains(&(tenant.clone(), budget)) {
            return Err(Error::Binding);
        }
        self.ceilings
            .get(&(tenant.clone(), budget))
            .ok_or(Error::Baseline)?
            .authorize_amount(amount, expiry_ms, current_ms, sequence)
            .map_err(|_| Error::Consumption)
    }

    /// # Errors
    /// Refuses absent holds or cancellation after dispatch.
    pub fn cancel_unsubmitted(
        &mut self,
        tenant: &TenantId,
        budget: [u8; 32],
        id: [u8; 32],
    ) -> Result<(), Error> {
        self.ceilings
            .get_mut(&(tenant.clone(), budget))
            .ok_or(Error::Baseline)?
            .cancel_unsubmitted(id)
            .map_err(|_| Error::Consumption)
    }

    /// # Errors
    /// Requires a restored hold before transport may make the result indeterminate.
    pub fn mark_unknown(
        &mut self,
        tenant: &TenantId,
        budget: [u8; 32],
        id: [u8; 32],
    ) -> Result<(), Error> {
        self.ceilings
            .get_mut(&(tenant.clone(), budget))
            .ok_or(Error::Baseline)?
            .mark_unknown(id)
            .map_err(|_| Error::Consumption)
    }

    #[must_use]
    pub fn ceiling(&self, tenant: &TenantId, id: [u8; 32]) -> Option<&NativeCeiling> {
        self.ceilings.get(&(tenant.clone(), id))
    }
}

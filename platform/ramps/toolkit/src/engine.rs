use layerx_paxeer_client::{FinalityReport, FinalityStage, FinalityTracker, TransactionHash};
use layerx_types::payload::ModuleRegistry;

use crate::clients::{
    callback_evidence_digest, ComplianceClient, ComplianceOutcome, LayerxClient, LayerxSubmission,
    PaxeerCustodyClient, ProviderCallback, ProviderClient, ProviderResult, ProviderState,
};
use crate::journal::{Journal, OrderSnapshot, TransitionEvidence, WorkflowStage};
use crate::{RampDirection, RampError, RampOrder};

pub struct RampEngine<'a> {
    pub journal: &'a mut Journal,
    pub compliance: &'a ComplianceClient,
    pub provider: &'a ProviderClient,
    pub layerx: &'a LayerxClient,
    pub registry: &'a ModuleRegistry,
    pub worker_id: &'a str,
    pub lease_seconds: u64,
}

impl RampEngine<'_> {
    pub fn evaluate_compliance(
        &mut self,
        order_digest: [u8; 32],
        now: u64,
    ) -> Result<(), RampError> {
        self.acquire(order_digest, now)?;
        let snapshot = self.snapshot(order_digest)?;
        if !matches!(
            snapshot.stage,
            WorkflowStage::CompliancePending | WorkflowStage::ManualReview
        ) || snapshot.evidence.provider_operation_id.is_some()
        {
            return Err(RampError::IllegalTransition);
        }
        let expected = snapshot.stage;
        let order = snapshot.order;
        let decision = self.compliance.evaluate(&order, now)?;
        let (next, refusal) = match decision.decision {
            ComplianceOutcome::Approved => (
                match order.direction() {
                    RampDirection::OnRamp => WorkflowStage::AwaitingExternalCredit,
                    RampDirection::OffRamp => WorkflowStage::AwaitingLayerxPayment,
                },
                None,
            ),
            ComplianceOutcome::Refused => {
                (WorkflowStage::ComplianceRefused, Some(decision.reason_code))
            }
            ComplianceOutcome::ManualReview => {
                (WorkflowStage::ManualReview, Some(decision.reason_code))
            }
        };
        let mut evidence = TransitionEvidence::empty();
        evidence.refusal_code = refusal;
        self.journal
            .transition(order_digest, expected, next, evidence, self.worker_id, now)
    }

    pub fn submit_provider(&mut self, order_digest: [u8; 32], now: u64) -> Result<(), RampError> {
        self.acquire(order_digest, now)?;
        let snapshot = self.snapshot(order_digest)?;
        let expected = snapshot.stage;
        if !matches!(
            (snapshot.order.direction(), expected),
            (RampDirection::OnRamp, WorkflowStage::AwaitingExternalCredit)
                | (RampDirection::OffRamp, WorkflowStage::LayerxVerified)
        ) {
            return Err(RampError::IllegalTransition);
        }
        let order = snapshot.order;
        let mut planned = TransitionEvidence::empty();
        planned.provider_operation_id =
            Some(format!("idempotency:{}", hex_id(&order.order_digest)));
        planned.retry_at = Some(now.saturating_add(self.lease_seconds));
        self.journal.transition(
            order_digest,
            expected,
            WorkflowStage::ProviderSubmissionPlanned,
            planned.clone(),
            self.worker_id,
            now,
        )?;
        match self.provider.submit(&order) {
            Ok(result) => self.apply_provider_result(
                &order,
                WorkflowStage::ProviderSubmissionPlanned,
                result,
                now,
            ),
            Err(_) => self.journal.transition(
                order_digest,
                WorkflowStage::ProviderSubmissionPlanned,
                WorkflowStage::ProviderSubmittedUnknown,
                planned,
                self.worker_id,
                now,
            ),
        }
    }

    pub fn reconcile_provider(
        &mut self,
        order_digest: [u8; 32],
        now: u64,
    ) -> Result<(), RampError> {
        self.acquire(order_digest, now)?;
        let snapshot = self.snapshot(order_digest)?;
        if !matches!(
            snapshot.stage,
            WorkflowStage::ProviderSubmissionPlanned
                | WorkflowStage::ProviderSubmittedUnknown
                | WorkflowStage::ProviderPending
                | WorkflowStage::ManualReview
        ) {
            return Err(RampError::IllegalTransition);
        }
        let operation = snapshot
            .evidence
            .provider_operation_id
            .clone()
            .ok_or(RampError::IllegalTransition)?;
        let result = self.provider.reconcile(&snapshot.order, &operation)?;
        self.apply_provider_result(&snapshot.order, snapshot.stage, result, now)
    }

    pub fn provider_callback(
        &mut self,
        callback: &ProviderCallback,
        public_key: &[u8; 32],
        now: u64,
    ) -> Result<(), RampError> {
        let digest = callback.result.order_digest;
        let snapshot = self.snapshot(digest)?;
        callback.verify(&snapshot.order, public_key)?;
        let evidence_digest = callback_evidence_digest(callback)?;
        let (next, evidence) = provider_transition(callback.result.clone());
        if !self.journal.apply_provider_callback(
            digest,
            &callback.callback_id,
            callback.provider_sequence,
            evidence_digest,
            snapshot.stage,
            next,
            evidence,
            now,
        )? {
            return Ok(());
        }
        self.finish_if_complete(digest, now)
    }

    pub fn submit_layerx(
        &mut self,
        order_digest: [u8; 32],
        account_sequence: u64,
        now: u64,
    ) -> Result<(), RampError> {
        self.acquire(order_digest, now)?;
        let snapshot = self.snapshot(order_digest)?;
        let expected = snapshot.stage;
        if !matches!(
            (snapshot.order.direction(), expected),
            (RampDirection::OnRamp, WorkflowStage::ProviderSettled)
                | (RampDirection::OffRamp, WorkflowStage::AwaitingLayerxPayment)
        ) {
            return Err(RampError::IllegalTransition);
        }
        let order = snapshot.order;
        let prepared = self
            .layerx
            .prepare_payment(&order, account_sequence, now, self.registry)?;
        let mut planned = TransitionEvidence::empty();
        planned.activity_id = Some(prepared.activity_id());
        planned.canonical_activity = Some(prepared.canonical_activity().to_vec());
        planned.retry_at = Some(now.saturating_add(self.lease_seconds));
        self.journal.transition(
            order_digest,
            expected,
            WorkflowStage::LayerxSubmissionPlanned,
            planned,
            self.worker_id,
            now,
        )?;
        match self.layerx.submit_prepared(&order, prepared) {
            Ok(result) => self.apply_layerx_result(
                &order,
                WorkflowStage::LayerxSubmissionPlanned,
                result,
                now,
            ),
            Err(_) => {
                let mut evidence = TransitionEvidence::empty();
                evidence.activity_id = self.snapshot(order_digest)?.evidence.activity_id;
                self.journal.transition(
                    order_digest,
                    WorkflowStage::LayerxSubmissionPlanned,
                    WorkflowStage::LayerxSubmittedUnknown,
                    evidence,
                    self.worker_id,
                    now,
                )
            }
        }
    }

    pub fn resolve_layerx(&mut self, order_digest: [u8; 32], now: u64) -> Result<(), RampError> {
        self.acquire(order_digest, now)?;
        let snapshot = self.snapshot(order_digest)?;
        if !matches!(
            snapshot.stage,
            WorkflowStage::LayerxSubmissionPlanned
                | WorkflowStage::LayerxSubmittedUnknown
                | WorkflowStage::LayerxPending
        ) {
            return Err(RampError::IllegalTransition);
        }
        let activity = snapshot.evidence.activity_id.ok_or(RampError::Layerx)?;
        let result = self.layerx.resolve(&snapshot.order, activity)?;
        self.apply_layerx_result(&snapshot.order, snapshot.stage, result, now)
    }

    fn apply_provider_result(
        &mut self,
        order: &RampOrder,
        expected: WorkflowStage,
        result: ProviderResult,
        now: u64,
    ) -> Result<(), RampError> {
        let (next, evidence) = provider_transition(result);
        self.journal.transition(
            order.order_digest,
            expected,
            next,
            evidence,
            self.worker_id,
            now,
        )?;
        self.finish_if_complete(order.order_digest, now)
    }

    fn apply_layerx_result(
        &mut self,
        order: &RampOrder,
        expected: WorkflowStage,
        result: LayerxSubmission,
        now: u64,
    ) -> Result<(), RampError> {
        let (next, evidence) = match result {
            LayerxSubmission::Unknown {
                activity_id,
                canonical_activity,
            } => {
                let mut evidence = TransitionEvidence::empty();
                evidence.activity_id = Some(activity_id);
                evidence.canonical_activity = canonical_activity;
                evidence.retry_at = Some(now.saturating_add(self.lease_seconds));
                (WorkflowStage::LayerxSubmittedUnknown, evidence)
            }
            LayerxSubmission::Pending {
                activity_id,
                canonical_activity,
            } => {
                let mut evidence = TransitionEvidence::empty();
                evidence.activity_id = Some(activity_id);
                evidence.canonical_activity = canonical_activity;
                evidence.retry_at = Some(now.saturating_add(self.lease_seconds));
                (WorkflowStage::LayerxPending, evidence)
            }
            LayerxSubmission::Refused {
                activity_id,
                canonical_activity,
                code,
            } => {
                let mut evidence = TransitionEvidence::empty();
                evidence.activity_id = Some(activity_id);
                evidence.canonical_activity = Some(canonical_activity);
                evidence.refusal_code = Some(code);
                (WorkflowStage::LayerxRefused, evidence)
            }
            LayerxSubmission::Verified {
                leg: verified,
                canonical_activity,
            } => {
                let mut evidence = TransitionEvidence::empty();
                evidence.activity_id = Some(verified.activity_id);
                evidence.canonical_activity = canonical_activity;
                evidence.receipt_digest = Some(verified.receipt_digest);
                (WorkflowStage::LayerxVerified, evidence)
            }
        };
        self.journal.transition(
            order.order_digest,
            expected,
            next,
            evidence,
            self.worker_id,
            now,
        )?;
        self.finish_if_complete(order.order_digest, now)
    }

    pub fn finish_if_complete(
        &mut self,
        order_digest: [u8; 32],
        now: u64,
    ) -> Result<(), RampError> {
        self.acquire(order_digest, now)?;
        let snapshot = self.snapshot(order_digest)?;
        let ready = matches!(
            (snapshot.order.direction(), snapshot.stage),
            (RampDirection::OnRamp, WorkflowStage::LayerxVerified)
                | (RampDirection::OffRamp, WorkflowStage::ProviderSettled)
        );
        if !ready {
            return Ok(());
        }
        self.journal.transition(
            order_digest,
            snapshot.stage,
            WorkflowStage::Done,
            snapshot.evidence,
            self.worker_id,
            now,
        )
    }

    fn acquire(&mut self, digest: [u8; 32], now: u64) -> Result<(), RampError> {
        self.journal
            .acquire_lease(digest, self.worker_id, now, self.lease_seconds)
    }

    fn snapshot(&self, digest: [u8; 32]) -> Result<OrderSnapshot, RampError> {
        self.journal
            .order(&digest)
            .cloned()
            .ok_or(RampError::InvalidOrder)
    }
}

fn provider_transition(result: ProviderResult) -> (WorkflowStage, TransitionEvidence) {
    let mut evidence = TransitionEvidence::empty();
    evidence.provider_operation_id = Some(result.operation_id);
    evidence.provider_evidence_digest = result.evidence_digest;
    evidence.refusal_code = result.refusal_code;
    evidence.retry_at = result.retry_at;
    let next = match result.state {
        ProviderState::SubmittedUnknown => WorkflowStage::ProviderSubmittedUnknown,
        ProviderState::Pending => WorkflowStage::ProviderPending,
        ProviderState::Settled => WorkflowStage::ProviderSettled,
        ProviderState::Refused => WorkflowStage::ProviderRefused,
        ProviderState::Reversed => WorkflowStage::ProviderReversed,
        ProviderState::ManualReview => WorkflowStage::ManualReview,
    };
    (next, evidence)
}

pub struct InventoryRebalancer<'a> {
    pub journal: &'a mut Journal,
    pub custody: &'a PaxeerCustodyClient,
}

impl InventoryRebalancer<'_> {
    pub fn submit(
        &mut self,
        asset: [u8; 32],
        amount: u128,
        idempotency_key: [u8; 32],
        now: u64,
    ) -> Result<(String, TransactionHash), RampError> {
        if self.journal.paxeer(&idempotency_key).is_some() {
            return Err(RampError::Conflict);
        }
        self.journal
            .plan_paxeer(idempotency_key, asset, amount, now)?;
        let submission = self.custody.broadcast(asset, amount, idempotency_key)?;
        let transaction = TransactionHash::from_hex(&submission.transaction_hash)
            .map_err(|_| RampError::Paxeer)?;
        self.journal.observe_paxeer(
            idempotency_key,
            &submission.operation_id,
            transaction.bytes(),
            "broadcast_unknown",
            None,
            0,
            now,
        )?;
        Ok((submission.operation_id, transaction))
    }

    pub fn reconcile(
        &mut self,
        idempotency_key: [u8; 32],
        now: u64,
    ) -> Result<(String, TransactionHash), RampError> {
        let snapshot = self
            .journal
            .paxeer(&idempotency_key)
            .cloned()
            .ok_or(RampError::Paxeer)?;
        if snapshot.operation_id.is_some() {
            return Err(RampError::Conflict);
        }
        let submission =
            self.custody
                .reconcile(snapshot.asset, snapshot.amount, idempotency_key)?;
        let transaction = TransactionHash::from_hex(&submission.transaction_hash)
            .map_err(|_| RampError::Paxeer)?;
        self.journal.observe_paxeer(
            idempotency_key,
            &submission.operation_id,
            transaction.bytes(),
            "broadcast_unknown",
            None,
            0,
            now,
        )?;
        Ok((submission.operation_id, transaction))
    }

    pub fn poll(
        &mut self,
        idempotency_key: [u8; 32],
        operation_id: &str,
        tracker: &mut FinalityTracker,
        now: u64,
    ) -> Result<FinalityReport, RampError> {
        let snapshot = self
            .journal
            .paxeer(&idempotency_key)
            .ok_or(RampError::Paxeer)?;
        if snapshot.operation_id.as_deref() != Some(operation_id)
            || snapshot.transaction_hash != Some(tracker.transaction().bytes())
        {
            return Err(RampError::Conflict);
        }
        let report = tracker.poll();
        let (mut stage, block_hash) = match report.stage() {
            FinalityStage::Announced => ("announced", None),
            FinalityStage::Missing { .. } => ("missing", None),
            FinalityStage::Pooled { .. } => ("pooled", None),
            FinalityStage::Confirming { inclusion, .. } => {
                ("confirming", Some(inclusion.block.hash))
            }
            FinalityStage::Final { inclusion, .. } => ("final", Some(inclusion.block.hash)),
            FinalityStage::Displaced { lost, .. } => ("displaced", Some(lost.block.hash)),
        };
        if block_hash.is_some()
            && self
                .journal
                .paxeer(&idempotency_key)
                .and_then(|snapshot| snapshot.block_hash)
                .is_some_and(|previous| Some(previous) != block_hash)
        {
            stage = "displaced";
        }
        self.journal.observe_paxeer(
            idempotency_key,
            operation_id,
            report.transaction().bytes(),
            stage,
            block_hash,
            report.progress().confirmed,
            now,
        )?;
        Ok(report)
    }
}

fn hex_id(value: &[u8; 32]) -> String {
    crate::clients::hex(value)
}

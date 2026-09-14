use super::{
    encode_record, Outbox, OutboxError, ReceiptEvidence, StateTransition, SubmissionState,
    SubmissionStatus,
};
use crate::budget::NativeBudgetOutcome;
use crate::store::{ObjectKind, Store, TenantKey};

impl Outbox {
    /// # Errors
    /// Refuses non-unknown submissions, corrupt clocks and failed durable retry writes before transport.
    pub fn begin_native_retry(
        &self,
        store: &mut Store,
        id: [u8; 32],
        observed_at_ms: u64,
    ) -> Result<bool, super::UnknownResolutionError> {
        super::resolution::begin_native_attempt(self, store, id, observed_at_ms)
    }

    /// # Errors
    /// Refuses cross-activity evidence, inconsistent terminal states and outcomes for an unsubmitted activity.
    pub fn settle_native_budget(
        &mut self,
        store: &mut Store,
        id: [u8; 32],
        outcome: &NativeBudgetOutcome,
    ) -> Result<SubmissionStatus, OutboxError> {
        let mut record = self.records.get(&id).ok_or(OutboxError::NotFound)?.clone();
        if record.status.activity_id != outcome.activity_id() || id != outcome.idempotency_key {
            return Err(OutboxError::ReceiptMismatch);
        }
        let target = if outcome.succeeded() {
            SubmissionState::Executed
        } else {
            SubmissionState::Failed
        };
        let receipt = ReceiptEvidence {
            receipt_ref: outcome.receipt_digest(),
        };
        if record.status.state.terminal() {
            return if record.status.state == target && record.status.evidence == Some(receipt) {
                Ok(record.status)
            } else {
                Err(OutboxError::ReceiptMismatch)
            };
        }
        if !matches!(
            record.status.state,
            SubmissionState::Submitted | SubmissionState::Acknowledged | SubmissionState::Unknown
        ) {
            return Err(OutboxError::InvalidTransition {
                from: record.status.state,
                to: target,
            });
        }
        if record.status.state == SubmissionState::Submitted {
            record.status.transitions.push(StateTransition {
                from: SubmissionState::Submitted,
                to: SubmissionState::Unknown,
                cause: "restart retained the indeterminate dispatch".to_owned(),
                receipt: None,
            });
            record.status.state = SubmissionState::Unknown;
        }
        record.status.transitions.push(StateTransition {
            from: record.status.state,
            to: target,
            cause: "finalized native Budget history resolved the exact activity".to_owned(),
            receipt: Some(receipt),
        });
        record.status.state = target;
        record.status.evidence = Some(receipt);
        crate::receipt::store_native_budget(store, record.tenant.clone(), outcome)
            .map_err(|_| OutboxError::ReceiptMismatch)?;
        crate::budget::persist_native_outcome(store, &record.tenant, outcome)
            .map_err(|_| OutboxError::ReceiptMismatch)?;
        let key = TenantKey::new(record.tenant.clone(), ObjectKind::Outbox, id.to_vec())
            .map_err(OutboxError::Store)?;
        store
            .put_local(key, encode_record(&record)?)
            .map_err(OutboxError::Store)?;
        let status = record.status.clone();
        self.records.insert(id, record);
        Ok(status)
    }
}

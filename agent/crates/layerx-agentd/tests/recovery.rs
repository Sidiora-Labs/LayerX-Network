mod support;

use layerx_agentd::budget::{
    hold_unknown, PersistedReceipt, ProtocolBudgetState, UnknownReservation,
};
use layerx_agentd::capability::ReceiptApplication as CeilingReceipt;
use layerx_agentd::outbox::{
    recover, resolve_unknown, Outbox, ReceiptLookup, RecoveryError, RecoveryInputs,
    SubmissionState, UnknownBoundaryError, UnknownCeilingReservation, UnknownResolutionError,
};
use layerx_agentd::protocol_evidence::RawReceiptEvidence;
use layerx_agentd::shutdown::{graceful, DaemonLifecycle, ShutdownError};
use layerx_agentd::store::{ObjectKind, Store};

use support::{directory, tenant, verified_submission};

fn protocol(consumed: u128) -> ProtocolBudgetState {
    ProtocolBudgetState {
        evidence: support::raw_budget_state(consumed, 1_000 - consumed, 1, 100, 50),
    }
}

fn recovery_inputs<'a>(
    unknown_budget_ids: &'a [[u8; 32]],
    budget_receipts: &'a [PersistedReceipt],
    unknown_ceilings: &'a [UnknownCeilingReservation],
) -> RecoveryInputs<'a> {
    RecoveryInputs {
        unknown_budget_ids,
        budget_receipts,
        protocol_budget: protocol(0),
        ceiling_maximum: 1_000,
        ceiling_receipts: &[] as &[CeilingReceipt],
        unknown_ceiling_reservations: unknown_ceilings,
        current_sequence: 1,
    }
}

fn enqueue(outbox: &mut Outbox, store: &mut Store, id: u8) -> Vec<u8> {
    let verified = verified_submission(id);
    let exact = verified.exact_bytes().to_vec();
    outbox
        .enqueue(store, tenant(), [id; 32], verified)
        .unwrap_or_else(|error| panic!("enqueue {id}: {error:?}"));
    exact
}

fn transition(outbox: &mut Outbox, store: &mut Store, id: u8, to: SubmissionState) {
    let evidence = if to == SubmissionState::Executed {
        let activity_id = outbox
            .status([id; 32])
            .map(|status| status.activity_id)
            .unwrap_or_else(|| panic!("activity missing"));
        Some(layerx_agentd::protocol_evidence::verify_receipt(
            &support::raw_receipt(activity_id, 0, 25),
        ).unwrap_or_else(|error| panic!("receipt: {error:?}")))
    } else {
        None
    };
    outbox
        .transition(store, [id; 32], to, format!("advance to {to:?}"), evidence)
        .unwrap_or_else(|error| panic!("transition {id}: {error:?}"));
}

fn populate_every_stage(store: &mut Store) -> (Outbox, Vec<u8>) {
    let mut outbox = Outbox::default();
    let queued_exact = enqueue(&mut outbox, store, 1);
    for id in 2..=5 {
        let _ = enqueue(&mut outbox, store, id);
        transition(&mut outbox, store, id, SubmissionState::Submitted);
    }
    transition(&mut outbox, store, 3, SubmissionState::Acknowledged);
    transition(&mut outbox, store, 4, SubmissionState::Unknown);
    transition(&mut outbox, store, 5, SubmissionState::Acknowledged);
    transition(&mut outbox, store, 5, SubmissionState::Executed);
    (outbox, queued_exact)
}

#[test]
fn restart_at_every_write_stage_preserves_exactly_one_recovery_action() {
    let root = directory("stages");
    let mut before = Store::open(&root).unwrap_or_else(|error| panic!("store: {error}"));
    let (_, queued_exact) = populate_every_stage(&mut before);
    hold_unknown(
        &mut before,
        &UnknownReservation {
            tenant: tenant(),
            id: [4; 32],
            amount: 300,
            expiry_sequence: 100,
            resolved: None,
        },
    )
    .unwrap_or_else(|error| panic!("budget hold: {error:?}"));
    drop(before);

    let mut after = Store::open(&root).unwrap_or_else(|error| panic!("restart: {error}"));
    let recovered = recover(
        &mut after,
        &tenant(),
        &recovery_inputs(
            &[[4; 32]],
            &[],
            &[UnknownCeilingReservation {
                id: [4; 32],
                amount: 300,
                expiry_sequence: 100,
            }],
        ),
    )
    .unwrap_or_else(|error| panic!("recover: {error:?}"));
    assert_eq!(recovered.queued_for_transmission, vec![[1; 32]]);
    assert_eq!(
        recovered.awaiting_receipt_resolution,
        vec![[2; 32], [3; 32], [4; 32]]
    );
    assert!(!recovered.queued_for_transmission.contains(&[5; 32]));
    assert!(!recovered.awaiting_receipt_resolution.contains(&[5; 32]));
    assert_eq!(
        recovered
            .outbox
            .bytes_for_transmission([1; 32])
            .unwrap_or_else(|error| panic!("queued bytes: {error:?}")),
        queued_exact
    );
    assert_eq!(recovered.budget_accounting.held_unresolved, 300);
    assert_eq!(
        recovered.ceiling.snapshot().map(|value| value.held),
        Ok(300)
    );
    assert!(recovered.require_write_ready().is_ok());
    let _ = std::fs::remove_dir_all(root);
}

struct ExistingReceipts {
    lookups: usize,
    resends: usize,
    receipts: std::collections::BTreeMap<[u8; 32], RawReceiptEvidence>,
}

impl ReceiptLookup for ExistingReceipts {
    fn receipt_by_idempotency_key(
        &mut self,
        idempotency_key: [u8; 32],
    ) -> Result<Option<RawReceiptEvidence>, UnknownBoundaryError> {
        self.lookups += 1;
        Ok(self.receipts.get(&idempotency_key).cloned())
    }

    fn resend_exact(
        &mut self,
        _idempotency_key: [u8; 32],
        _signed_canonical_bytes: &[u8],
    ) -> Result<(), UnknownBoundaryError> {
        self.resends += 1;
        Ok(())
    }
}

#[test]
fn restart_resolution_uses_receipts_without_duplicate_delivery() {
    let root = directory("exactly-once");
    let mut store = Store::open(&root).unwrap_or_else(|error| panic!("store: {error}"));
    let _ = populate_every_stage(&mut store);
    drop(store);
    let mut reopened = Store::open(&root).unwrap_or_else(|error| panic!("reopen: {error}"));
    let mut recovered = recover(&mut reopened, &tenant(), &recovery_inputs(&[], &[], &[]))
        .unwrap_or_else(|error| panic!("recover: {error:?}"));
    let pending = recovered.awaiting_receipt_resolution.clone();
    let receipts = pending
        .iter()
        .map(|submission_id| {
            let activity_id = recovered
                .outbox
                .status(*submission_id)
                .map(|status| status.activity_id)
                .unwrap_or_else(|| panic!("activity missing"));
            (*submission_id, support::raw_receipt(activity_id, 0, 25))
        })
        .collect();
    let mut core = ExistingReceipts {
        lookups: 0,
        resends: 0,
        receipts,
    };
    for (attempt, submission_id) in pending.into_iter().enumerate() {
        resolve_unknown(
            &mut recovered.outbox,
            &mut reopened,
            submission_id,
            10_000 + u64::try_from(attempt).unwrap_or(0),
            &mut core,
        )
        .unwrap_or_else(|error| panic!("resolve {submission_id:?}: {error:?}"));
    }
    assert_eq!(core.lookups, 3);
    assert_eq!(core.resends, 0);
    assert_eq!(
        recovered.outbox.status([5; 32]).map(|status| status.state),
        Some(SubmissionState::Executed)
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn unreconciled_accounting_blocks_writes_after_outbox_recovery() {
    let root = directory("blocked");
    let mut store = Store::open(&root).unwrap_or_else(|error| panic!("store: {error}"));
    let _ = populate_every_stage(&mut store);
    let inputs = RecoveryInputs {
        unknown_budget_ids: &[],
        budget_receipts: &[],
        protocol_budget: protocol(100),
        ceiling_maximum: 1_000,
        ceiling_receipts: &[],
        unknown_ceiling_reservations: &[],
        current_sequence: 1,
    };
    let recovered = recover(&mut store, &tenant(), &inputs)
        .unwrap_or_else(|error| panic!("recover: {error:?}"));
    assert!(matches!(
        recovered.require_write_ready(),
        Err(RecoveryError::WritesBlocked)
    ));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn graceful_shutdown_stops_admission_and_flushes_inflight_and_audit() {
    let root = directory("shutdown");
    let mut store = Store::open(&root).unwrap_or_else(|error| panic!("store: {error}"));
    let mut outbox = Outbox::default();
    let _ = enqueue(&mut outbox, &mut store, 1);
    let mut lifecycle = DaemonLifecycle::running();
    lifecycle
        .begin_work([1; 32], b"queued-before-exit".to_vec())
        .unwrap_or_else(|error| panic!("begin: {error:?}"));
    lifecycle
        .append_audit(b"submission queued".to_vec())
        .unwrap_or_else(|error| panic!("audit: {error:?}"));
    let report = graceful(&mut store, tenant(), &outbox, &mut lifecycle)
        .unwrap_or_else(|error| panic!("shutdown: {error:?}"));
    assert_eq!(report.in_flight_recorded, 1);
    assert_eq!(report.audit_entries_flushed, 1);
    assert!(!report.accepting_work);
    assert!(!lifecycle.accepting_work());
    assert!(matches!(
        lifecycle.begin_work([2; 32], Vec::new()),
        Err(ShutdownError::NotAccepting)
    ));
    drop(store);
    let reopened = Store::open(&root).unwrap_or_else(|error| panic!("reopen: {error}"));
    assert_eq!(
        reopened.list_object_ids(&tenant(), ObjectKind::Audit).len(),
        1
    );
    assert!(reopened
        .list_object_ids(&tenant(), ObjectKind::Configuration)
        .iter()
        .any(|identifier| identifier == b"shutdown-complete"));
    let _ = std::fs::remove_dir_all(root);
}

struct NoReceipt;

impl ReceiptLookup for NoReceipt {
    fn receipt_by_idempotency_key(
        &mut self,
        _idempotency_key: [u8; 32],
    ) -> Result<Option<RawReceiptEvidence>, UnknownBoundaryError> {
        Ok(None)
    }

    fn resend_exact(
        &mut self,
        _idempotency_key: [u8; 32],
        _signed_canonical_bytes: &[u8],
    ) -> Result<(), UnknownBoundaryError> {
        Ok(())
    }
}

#[test]
fn clock_regression_during_restarted_resolution_fails_closed() {
    let root = directory("clock");
    let mut store = Store::open(&root).unwrap_or_else(|error| panic!("store: {error}"));
    let mut outbox = Outbox::default();
    let _ = enqueue(&mut outbox, &mut store, 1);
    transition(&mut outbox, &mut store, 1, SubmissionState::Submitted);
    transition(&mut outbox, &mut store, 1, SubmissionState::Unknown);
    let mut core = NoReceipt;
    let _ = resolve_unknown(&mut outbox, &mut store, [1; 32], 10_000, &mut core)
        .unwrap_or_else(|error| panic!("initial resolution: {error:?}"));
    drop(outbox);
    drop(store);
    let mut reopened = Store::open(&root).unwrap_or_else(|error| panic!("reopen: {error}"));
    let mut recovered = recover(&mut reopened, &tenant(), &recovery_inputs(&[], &[], &[]))
        .unwrap_or_else(|error| panic!("recover: {error:?}"));
    assert!(matches!(
        resolve_unknown(
            &mut recovered.outbox,
            &mut reopened,
            [1; 32],
            9_999,
            &mut core
        ),
        Err(UnknownResolutionError::TimeRegressed)
    ));
    assert_eq!(
        recovered.outbox.status([1; 32]).map(|status| status.state),
        Some(SubmissionState::Unknown)
    );
    let _ = std::fs::remove_dir_all(root);
}

mod support;

use std::fs;

use layerx_agentd::admin::{
    assert_non_mutating, commands, ActionPlan, OperatorCommand, OperatorContext, ProtectedMutation,
    Surface, VerificationBacklog, ORDINARY_CLIENT_WRITE,
};
use layerx_agentd::audit::verify_chain;
use layerx_agentd::budget::{
    divergence_alert, LocalAccounting, ProtocolBudgetState, SpendReceiptEvidence,
};
use layerx_agentd::outbox::{Outbox, SubmissionState};
use layerx_agentd::store::{ObjectKind, Store, TenantKey};
use layerx_types::verify::VerificationLevel;

use support::{directory, tenant, verified_submission};

fn context(id: u8) -> OperatorContext {
    OperatorContext::new("operator:incident-response", [id; 32])
        .unwrap_or_else(|error| panic!("operator context: {error:?}"))
}

fn enqueue_unknown(store: &mut Store, outbox: &mut Outbox, id: u8) {
    outbox
        .enqueue(store, tenant(), [id; 32], verified_submission(id))
        .unwrap_or_else(|error| panic!("enqueue: {error:?}"));
    outbox
        .transition(
            store,
            [id; 32],
            SubmissionState::Submitted,
            "real boundary accepted bytes",
            None,
        )
        .unwrap_or_else(|error| panic!("submitted: {error:?}"));
    outbox
        .transition(
            store,
            [id; 32],
            SubmissionState::Unknown,
            "acknowledgement was not observed",
            None,
        )
        .unwrap_or_else(|error| panic!("unknown: {error:?}"));
}

#[test]
fn catalogue_routes_every_operator_action_without_protocol_mutating_power() {
    assert_eq!(commands().len(), 9);
    assert!(commands().iter().all(|command| !command.protocol_mutating));

    for command in [
        OperatorCommand::InspectUnknown([1; 32]),
        OperatorCommand::InspectStalledSubscription([2; 32]),
        OperatorCommand::InspectBudgetDivergence([3; 32]),
        OperatorCommand::InspectVerificationBacklog([4; 32]),
    ] {
        assert_eq!(
            assert_non_mutating(&command)
                .unwrap_or_else(|error| panic!("inspect route: {error:?}")),
            ActionPlan::InspectOnly
        );
    }
    assert_eq!(
        assert_non_mutating(&OperatorCommand::ResolveUnknown([5; 32]))
            .unwrap_or_else(|error| panic!("unknown route: {error:?}")),
        ActionPlan::ReceiptLookupAndExactResend
    );
    assert_eq!(
        assert_non_mutating(&OperatorCommand::ResumeStalledSubscription([6; 32]))
            .unwrap_or_else(|error| panic!("subscription route: {error:?}")),
        ActionPlan::ResumeDaemonLocalSubscription
    );
    assert_eq!(
        assert_non_mutating(&OperatorCommand::ReconcileBudgetDivergence([7; 32]))
            .unwrap_or_else(|error| panic!("budget route: {error:?}")),
        ActionPlan::ReconcileFromVerifiedCoreEvidence
    );
    assert_eq!(
        assert_non_mutating(&OperatorCommand::RetryVerification([8; 32]))
            .unwrap_or_else(|error| panic!("verification route: {error:?}")),
        ActionPlan::RetryEvidenceVerification
    );
    assert_eq!(
        assert_non_mutating(&OperatorCommand::SubmitActivity([9; 32]))
            .unwrap_or_else(|error| panic!("submission route: {error:?}")),
        ActionPlan::OrdinaryClientWrite(ORDINARY_CLIENT_WRITE)
    );
}

#[test]
fn inspections_and_verified_budget_reconciliation_are_audited_before_action() {
    let root = directory("operator-actions");
    let mut store = Store::open(&root).unwrap_or_else(|error| panic!("store: {error}"));
    let mut outbox = Outbox::default();
    enqueue_unknown(&mut store, &mut outbox, 1);
    let mut surface =
        Surface::open(&root, &tenant()).unwrap_or_else(|error| panic!("surface: {error:?}"));

    let status = surface
        .inspect_unknown(&context(1), &outbox, [1; 32])
        .unwrap_or_else(|error| panic!("inspect unknown: {error:?}"));
    assert_eq!(status.state, SubmissionState::Unknown);

    let mut local = LocalAccounting {
        consumed: 11,
        window_start_sequence: 80,
        last_receipt: None,
    };
    let protocol = ProtocolBudgetState {
        evidence: support::raw_budget_state(7, 93, 80, 120, 99),
    };
    let receipts = [SpendReceiptEvidence {
        window_start_sequence: 80,
        evidence: support::raw_receipt_at([0x44; 32], 0, 7, 90),
    }];
    let reconciled = surface
        .reconcile_budget_divergence(
            &context(2),
            [2; 32],
            &mut local,
            protocol,
            &receipts,
            &support::evidence_verifier(),
        )
        .unwrap_or_else(|error| panic!("reconcile: {error:?}"));
    let alert =
        divergence_alert(&reconciled, 100).unwrap_or_else(|| panic!("divergence alert absent"));
    assert_eq!(
        surface
            .inspect_budget_divergence(&context(3), [2; 32], alert)
            .unwrap_or_else(|error| panic!("inspect budget: {error:?}")),
        alert
    );

    let backlog = VerificationBacklog {
        idempotency_key: [3; 32],
        observed: VerificationLevel::STATE_PROVEN,
        requested: VerificationLevel::CHECKPOINT_FINALISED,
        queued_at_ms: 1_000,
        attempts: 2,
    };
    assert_eq!(
        surface
            .inspect_verification_backlog(&context(4), backlog)
            .unwrap_or_else(|error| panic!("inspect verification: {error:?}")),
        backlog
    );
    assert_eq!(surface.audit_entries(), 4);
    let verified = verify_chain(surface.audit_path())
        .unwrap_or_else(|error| panic!("verify operator audit: {error}"));
    assert_eq!(verified.entries, 4);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn adversarial_admin_edits_are_refused_audited_and_change_no_protected_value() {
    let root = directory("operator-adversarial");
    let mut store = Store::open(&root).unwrap_or_else(|error| panic!("store: {error}"));
    let mut outbox = Outbox::default();
    enqueue_unknown(&mut store, &mut outbox, 7);
    let receipt_key = TenantKey::new(tenant(), ObjectKind::Receipt, b"recorded-receipt".to_vec())
        .unwrap_or_else(|error| panic!("receipt key: {error}"));
    let level_key = TenantKey::new(
        tenant(),
        ObjectKind::Configuration,
        b"recorded-verification-level".to_vec(),
    )
    .unwrap_or_else(|error| panic!("level key: {error}"));
    store
        .put_core_cache(receipt_key.clone(), b"exact-core-receipt".to_vec())
        .unwrap_or_else(|error| panic!("receipt: {error}"));
    store
        .put_local(level_key.clone(), b"state-proven".to_vec())
        .unwrap_or_else(|error| panic!("level: {error}"));

    let mut surface =
        Surface::open(&root, &tenant()).unwrap_or_else(|error| panic!("surface: {error:?}"));
    let audit_before = fs::read(surface.audit_path())
        .unwrap_or_else(|error| panic!("read original audit: {error}"));
    for (index, mutation) in [
        ProtectedMutation::MarkUnknownExecuted,
        ProtectedMutation::ReplaceReceipt,
        ProtectedMutation::RaiseVerificationLevel,
        ProtectedMutation::RewriteAuditEntry,
        ProtectedMutation::ReplaceProtocolValue,
    ]
    .into_iter()
    .enumerate()
    {
        let result = surface.dispatch(
            &context(u8::try_from(index + 1).unwrap_or(1)),
            OperatorCommand::AttemptProtectedMutation {
                target: [7; 32],
                mutation,
            },
        );
        assert!(matches!(
            result,
            Err(layerx_agentd::admin::AdminError::ProtectedMutation(found)) if found == mutation
        ));
    }

    assert_eq!(
        outbox
            .status([7; 32])
            .unwrap_or_else(|| panic!("unknown missing"))
            .state,
        SubmissionState::Unknown
    );
    assert_eq!(
        store
            .get(&receipt_key)
            .unwrap_or_else(|| panic!("receipt missing"))
            .bytes(),
        b"exact-core-receipt"
    );
    assert_eq!(
        store
            .get(&level_key)
            .unwrap_or_else(|| panic!("level missing"))
            .bytes(),
        b"state-proven"
    );
    let audit_after = fs::read(surface.audit_path())
        .unwrap_or_else(|error| panic!("read operator audit: {error}"));
    assert!(audit_after.starts_with(&audit_before));
    assert_eq!(surface.audit_entries(), 5);
    assert_eq!(
        verify_chain(surface.audit_path())
            .unwrap_or_else(|error| panic!("verify audit: {error}"))
            .entries,
        5
    );
    let _ = fs::remove_dir_all(root);
}

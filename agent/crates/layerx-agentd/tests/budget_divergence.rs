mod support;

use layerx_agentd::budget::{
    divergence_alert, reconcile, LocalAccounting, ProtocolBudgetState, SpendReceiptEvidence,
};

fn protocol(consumed: u128, remaining: u128, head: u64) -> ProtocolBudgetState {
    ProtocolBudgetState {
        evidence: support::raw_budget_state(consumed, remaining, 100, 199, head),
    }
}

#[test]
fn divergence_is_audited_unhealthy_and_conservatively_enforced() {
    let mut local = LocalAccounting {
        consumed: 420,
        window_start_sequence: 100,
        last_receipt: Some([1; 32]),
    };
    let receipts = [SpendReceiptEvidence {
        window_start_sequence: 100,
        evidence: support::raw_receipt_at([7; 32], 0, 300, 120),
    }];
    let state = reconcile(&mut local, protocol(350, 650, 128), &receipts)
        .unwrap_or_else(|error| panic!("reconcile: {error:?}"));
    let alert =
        divergence_alert(&state, 1_000).unwrap_or_else(|| panic!("divergence alert is missing"));

    assert_eq!(alert.audit.local_consumed, 420);
    assert_eq!(alert.audit.protocol_consumed, 350);
    assert!(alert.audit.last_verified_receipt.is_some());
    assert_eq!(alert.audit.observed_head_sequence, 128);
    assert_eq!(alert.enforced_consumed, 420);
    assert_eq!(alert.enforced_remaining, 580);
    assert!(!alert.health.ready_for_writes);
    assert!(alert.health.divergence_open);
    assert_eq!(local.consumed, 350, "protocol remains authoritative");
}

#[test]
fn a_verified_missing_receipt_closes_the_alert() {
    let mut local = LocalAccounting {
        consumed: 200,
        window_start_sequence: 100,
        last_receipt: Some([2; 32]),
    };
    let missing = [SpendReceiptEvidence {
        window_start_sequence: 100,
        evidence: support::raw_receipt_at([3; 32], 0, 200, 120),
    }];
    let divergent = reconcile(&mut local, protocol(350, 650, 120), &missing)
        .unwrap_or_else(|error| panic!("reconcile: {error:?}"));
    assert!(divergence_alert(&divergent, 1_000).is_some());

    let resolved = reconcile(&mut local, protocol(350, 650, 121), &missing)
        .unwrap_or_else(|error| panic!("reconcile: {error:?}"));
    assert!(divergence_alert(&resolved, 1_000).is_none());
}

#[test]
fn protocol_overage_is_also_the_restrictive_figure() {
    let mut local = LocalAccounting {
        consumed: 200,
        window_start_sequence: 100,
        last_receipt: None,
    };
    let state = reconcile(&mut local, protocol(700, 300, 130), &[])
        .unwrap_or_else(|error| panic!("reconcile: {error:?}"));
    let alert =
        divergence_alert(&state, 1_000).unwrap_or_else(|| panic!("divergence alert is missing"));
    assert_eq!(alert.enforced_consumed, 700);
    assert_eq!(alert.enforced_remaining, 300);
}

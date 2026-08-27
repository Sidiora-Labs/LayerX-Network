mod support;

use layerx_agentd::budget::{
    reconcile, LocalAccounting, ProtocolBudgetState, ReconcileError, SpendReceiptEvidence,
};

fn protocol(consumed: u128, remaining: u128, head: u64) -> ProtocolBudgetState {
    let mut candidate = Vec::with_capacity(32);
    candidate.extend_from_slice(&consumed.to_be_bytes());
    candidate.extend_from_slice(&remaining.to_be_bytes());
    ProtocolBudgetState {
        evidence: support::raw_state_leaf(candidate, head),
    }
}

#[test]
fn divergence_cannot_be_derived_from_an_untyped_included_leaf() {
    let mut local = LocalAccounting {
        consumed: 420,
        window_start_sequence: 100,
        last_receipt: Some([1; 32]),
    };
    let receipts = [SpendReceiptEvidence {
        expected_activity_id: [7; 32],
        evidence: support::raw_receipt_at([7; 32], 0, 300, 120),
    }];
    assert_eq!(
        reconcile(
            &mut local,
            &protocol(350, 650, 128),
            &receipts,
            &support::evidence_verifier(),
        ),
        Err(ReconcileError::ProtocolStateSchemaUnavailable)
    );
    assert_eq!(local.consumed, 420);
}

#[test]
fn repeated_attempts_cannot_turn_an_untyped_leaf_into_authority() {
    let mut local = LocalAccounting {
        consumed: 200,
        window_start_sequence: 100,
        last_receipt: Some([2; 32]),
    };
    let missing = [SpendReceiptEvidence {
        expected_activity_id: [3; 32],
        evidence: support::raw_receipt_at([3; 32], 0, 200, 120),
    }];
    for head in [120, 121] {
        assert_eq!(
            reconcile(
                &mut local,
                &protocol(350, 650, head),
                &missing,
                &support::evidence_verifier(),
            ),
            Err(ReconcileError::ProtocolStateSchemaUnavailable)
        );
    }
    assert_eq!(local.consumed, 200);
}

#[test]
fn an_untyped_overage_candidate_never_replaces_local_accounting() {
    let mut local = LocalAccounting {
        consumed: 200,
        window_start_sequence: 100,
        last_receipt: None,
    };
    assert_eq!(
        reconcile(
            &mut local,
            &protocol(700, 300, 130),
            &[],
            &support::evidence_verifier(),
        ),
        Err(ReconcileError::ProtocolStateSchemaUnavailable)
    );
    assert_eq!(local.consumed, 200);
}

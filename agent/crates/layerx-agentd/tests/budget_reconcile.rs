mod support;

use layerx_agentd::budget::{
    reconcile, LocalAccounting, ProtocolBudgetState, ReconcileError, SpendReceiptEvidence,
};

fn protocol(consumed: u128, start: u64) -> ProtocolBudgetState {
    let mut candidate = Vec::with_capacity(24);
    candidate.extend_from_slice(&consumed.to_be_bytes());
    candidate.extend_from_slice(&start.to_be_bytes());
    ProtocolBudgetState {
        evidence: support::raw_state_leaf(candidate, start + 20),
    }
}

#[test]
fn merkle_inclusion_without_a_canonical_budget_schema_cannot_correct_the_cache() {
    let mut local = LocalAccounting {
        consumed: 900,
        window_start_sequence: 1,
        last_receipt: Some([1; 32]),
    };
    let protocol = protocol(0, 100);
    let verifier = support::evidence_verifier();
    let verified = verifier
        .verify_state(&protocol.evidence)
        .unwrap_or_else(|error| panic!("state verification: {error:?}"));
    assert_eq!(
        verified.level(),
        layerx_types::verify::VerificationLevel::STATE_PROVEN
    );
    assert_eq!(
        reconcile(&mut local, protocol, &[], &verifier),
        Err(ReconcileError::ProtocolStateSchemaUnavailable)
    );
    assert_eq!(local.consumed, 900);
    assert_eq!(local.window_start_sequence, 1);
    assert_eq!(local.last_receipt, Some([1; 32]));
}

#[test]
fn duplicate_receipt_and_activity_evidence_are_rejected_before_reconciliation() {
    let mut local = LocalAccounting {
        consumed: 200,
        window_start_sequence: 100,
        last_receipt: Some([1; 32]),
    };
    let receipt = support::raw_receipt_at([2; 32], 0, 200, 120);
    let duplicates = [
        SpendReceiptEvidence {
            expected_activity_id: [2; 32],
            evidence: receipt.clone(),
        },
        SpendReceiptEvidence {
            expected_activity_id: [2; 32],
            evidence: receipt,
        },
    ];
    assert_eq!(
        reconcile(
            &mut local,
            protocol(350, 100),
            &duplicates,
            &support::evidence_verifier(),
        ),
        Err(ReconcileError::DuplicateReceipt)
    );
    let same_activity = [
        SpendReceiptEvidence {
            expected_activity_id: [3; 32],
            evidence: support::raw_receipt_at([3; 32], 0, 100, 120),
        },
        SpendReceiptEvidence {
            expected_activity_id: [3; 32],
            evidence: support::raw_receipt_at([3; 32], 0, 100, 121),
        },
    ];
    assert_eq!(
        reconcile(
            &mut local,
            protocol(350, 100),
            &same_activity,
            &support::evidence_verifier(),
        ),
        Err(ReconcileError::DuplicateActivity)
    );
    assert_eq!(local.consumed, 200);
}

#[test]
fn receipts_are_bound_to_the_expected_activity_before_reconciliation() {
    let mut local = LocalAccounting {
        consumed: 200,
        window_start_sequence: 100,
        last_receipt: Some([1; 32]),
    };
    let receipts = [SpendReceiptEvidence {
        expected_activity_id: [9; 32],
        evidence: support::raw_receipt_at([2; 32], 0, 200, 120),
    }];
    assert_eq!(
        reconcile(
            &mut local,
            protocol(350, 100),
            &receipts,
            &support::evidence_verifier(),
        ),
        Err(ReconcileError::ReceiptActivityMismatch)
    );
    assert_eq!(local.consumed, 200);
}

#[test]
fn unverified_inputs_never_correct_the_cache() {
    let mut local = LocalAccounting {
        consumed: 200,
        window_start_sequence: 100,
        last_receipt: None,
    };
    let raw = support::raw_receipt_at([3; 32], 0, 1, 120);
    let mut corrupted = raw.canonical_receipt().to_vec();
    corrupted[0] ^= 1;
    let unverified_receipt = [SpendReceiptEvidence {
        expected_activity_id: [3; 32],
        evidence: support::corrupt_raw_receipt(&raw, corrupted),
    }];
    assert!(reconcile(
        &mut local,
        protocol(350, 100),
        &unverified_receipt,
        &support::evidence_verifier(),
    )
    .is_err());
    assert_eq!(local.consumed, 200);

    let state = protocol(350, 100);
    let mut corrupt_state = state.evidence.canonical_state().to_vec();
    corrupt_state[4] ^= 1;
    assert!(reconcile(
        &mut local,
        ProtocolBudgetState {
            evidence: support::corrupt_raw_state(&state.evidence, corrupt_state),
        },
        &[],
        &support::evidence_verifier(),
    )
    .is_err());
    assert_eq!(local.consumed, 200);
}

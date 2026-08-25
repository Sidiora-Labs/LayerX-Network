mod support;

use layerx_agentd::budget::{
    reconcile, LocalAccounting, ProtocolBudgetState, SpendReceiptEvidence,
};
use layerx_agentd::protocol_evidence::verify_state_evidence;

fn protocol(consumed: u128, start: u64) -> ProtocolBudgetState {
    ProtocolBudgetState {
        evidence: support::raw_budget_state(
            consumed,
            1_000 - consumed,
            start,
            start + 99,
            start + 20,
        ),
    }
}

#[test]
fn rollover_comes_only_from_protocol_state() {
    let mut local = LocalAccounting {
        consumed: 900,
        window_start_sequence: 1,
        last_receipt: Some([1; 32]),
    };
    let protocol = protocol(0, 100);
    let verified = verify_state_evidence(&protocol.evidence)
        .unwrap_or_else(|error| panic!("state verification: {error:?}"));
    assert_eq!(
        verified.level(),
        layerx_types::verify::VerificationLevel::STATE_PROVEN
    );
    let state = reconcile(&mut local, protocol, &[])
        .unwrap_or_else(|error| panic!("reconcile: {error:?}"));
    assert_eq!(state.window_start_sequence, 100);
    assert_eq!(state.window_end_sequence, 199);
    assert_eq!(state.remaining, 1_000);
    assert_eq!(local.consumed, 0);
    assert_eq!(state.divergence, Some(-900));
}

#[test]
fn missed_receipt_divergence_is_exposed_and_cache_is_corrected() {
    let mut local = LocalAccounting {
        consumed: 200,
        window_start_sequence: 100,
        last_receipt: Some([1; 32]),
    };
    let receipts = [SpendReceiptEvidence {
        window_start_sequence: 100,
        evidence: support::raw_receipt_at([2; 32], 0, 200, 120),
    }];
    let state = reconcile(&mut local, protocol(350, 100), &receipts)
        .unwrap_or_else(|error| panic!("reconcile: {error:?}"));
    assert_eq!(state.local_before, 200);
    assert_eq!(state.protocol_consumed, 350);
    assert_eq!(state.local_after, 350);
    assert_eq!(state.divergence, Some(150));
    assert!(state.last_verified_receipt.is_some());
}

#[test]
fn verified_failed_receipt_does_not_consume_budget() {
    let mut local = LocalAccounting {
        consumed: 0,
        window_start_sequence: 100,
        last_receipt: None,
    };
    let receipts = [
        SpendReceiptEvidence {
            window_start_sequence: 100,
            evidence: support::raw_receipt_at([2; 32], 0, 200, 120),
        },
        SpendReceiptEvidence {
            window_start_sequence: 100,
            evidence: support::raw_receipt_at([3; 32], 5, 900, 121),
        },
    ];
    let state = reconcile(&mut local, protocol(200, 100), &receipts)
        .unwrap_or_else(|error| panic!("reconcile: {error:?}"));
    assert_eq!(state.protocol_consumed, 200);
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
        window_start_sequence: 100,
        evidence: support::corrupt_raw_receipt(&raw, corrupted),
    }];
    assert!(reconcile(&mut local, protocol(350, 100), &unverified_receipt).is_err());
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
    )
    .is_err());
    assert_eq!(local.consumed, 200);
}

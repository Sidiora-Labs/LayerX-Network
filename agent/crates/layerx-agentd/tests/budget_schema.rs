mod support;

use std::fs;

use layerx_agentd::budget::{
    budget_state_key, hold_unknown, rebuild, reconcile, LocalAccounting, PersistedReceipt,
    ProtocolBudgetRecord, ProtocolBudgetState, ReconcileError, RestartError, SpendReceiptEvidence,
    UnknownReservation, BUDGET_MODULE_ID,
};
use layerx_agentd::outbox::{recover, RecoveryInputs};
use layerx_agentd::store::Store;

use support::{directory, tenant};

const BUDGET_ID: [u8; 32] = [0x51; 32];

fn core_budget_record(per_period_limit: u128, spent_this_period: u128) -> Vec<u8> {
    let mut bytes = vec![0_u8; 278];
    bytes[1] = 1;
    bytes[2..34].copy_from_slice(&BUDGET_ID);
    bytes[34..66].copy_from_slice(&[0x52; 32]);
    bytes[66..98].copy_from_slice(&[0x53; 32]);
    bytes[98..130].copy_from_slice(&[0x24; 32]);
    bytes[130..162].copy_from_slice(&[0x54; 32]);
    bytes[162..178].copy_from_slice(&per_period_limit.to_be_bytes());
    bytes[178..194].copy_from_slice(&per_period_limit.to_be_bytes());
    bytes[194..210].copy_from_slice(&0_u128.to_be_bytes());
    bytes[210..226].copy_from_slice(&spent_this_period.to_be_bytes());
    bytes[226..242].copy_from_slice(&0_u128.to_be_bytes());
    bytes[242..250].copy_from_slice(&1_000_u64.to_be_bytes());
    bytes[250..258].copy_from_slice(&80_u64.to_be_bytes());
    bytes[258..266].copy_from_slice(&5_000_u64.to_be_bytes());
    bytes[266..274].copy_from_slice(&0_u64.to_be_bytes());
    bytes
}

fn protocol(per_period_limit: u128, spent_this_period: u128) -> ProtocolBudgetState {
    ProtocolBudgetState {
        evidence: support::raw_state_leaf(
            core_budget_record(per_period_limit, spent_this_period),
            99,
        ),
    }
}

#[test]
fn verified_core_budget_record_reconciles_local_accounting() {
    assert_eq!(BUDGET_MODULE_ID, 3);
    assert_eq!(budget_state_key(BUDGET_ID).len(), 39);
    assert!(budget_state_key(BUDGET_ID).starts_with(b"budget:"));
    let record = ProtocolBudgetRecord::decode(&core_budget_record(500, 25))
        .unwrap_or_else(|error| panic!("record: {error:?}"));
    assert_eq!(record.budget_id, BUDGET_ID);
    assert_eq!(record.remaining(), 475);
    assert_eq!(record.window_end_sequence(), 1_080);
    assert_eq!(
        ProtocolBudgetRecord::decode(b"budget-state-schema-unavailable"),
        Err(ReconcileError::ProtocolStateSchemaUnavailable)
    );

    let verifier = support::evidence_verifier();
    let mut local = LocalAccounting {
        consumed: 11,
        window_start_sequence: 70,
        last_receipt: None,
    };
    let receipts = [SpendReceiptEvidence {
        expected_activity_id: [0x44; 32],
        evidence: support::raw_receipt_at([0x44; 32], 0, 25, 90),
    }];
    let state = reconcile(&mut local, &protocol(500, 25), &receipts, &verifier)
        .unwrap_or_else(|error| panic!("reconcile: {error:?}"));
    assert_eq!(state.remaining(), 475);
    assert_eq!(state.local_after(), 25);
    assert_eq!(state.observed_head_sequence(), 99);
    assert_eq!(local.consumed, 25);
    assert_eq!(local.window_start_sequence, 80);
    assert_eq!(local.last_receipt, Some([0x44; 32]));

    let mut untouched = LocalAccounting {
        consumed: 11,
        window_start_sequence: 70,
        last_receipt: None,
    };
    let opaque = ProtocolBudgetState {
        evidence: support::raw_state_leaf(b"budget-state-schema-unavailable".to_vec(), 99),
    };
    assert_eq!(
        reconcile(&mut untouched, &opaque, &receipts, &verifier),
        Err(ReconcileError::ProtocolStateSchemaUnavailable)
    );
    assert_eq!(untouched.consumed, 11);
}

#[test]
fn restart_recovery_reconciles_receipts_and_held_unknowns_against_core_state() {
    let root = directory("budget-schema-restart");
    let mut store = Store::open(&root).unwrap_or_else(|error| panic!("store: {error}"));
    let verifier = support::evidence_verifier();
    let receipts = [PersistedReceipt {
        expected_activity_id: [0x61; 32],
        evidence: support::raw_receipt([0x61; 32], 0, 25),
    }];
    hold_unknown(
        &mut store,
        &UnknownReservation {
            tenant: tenant(),
            id: [0x62; 32],
            amount: 15,
            expiry_sequence: 500,
            resolved: None,
        },
    )
    .unwrap_or_else(|error| panic!("hold: {error:?}"));
    let unknown_ids = [[0x62; 32]];

    let executed_unknown = rebuild(
        &store,
        &tenant(),
        &unknown_ids,
        &receipts,
        &protocol(500, 40),
        &verifier,
    )
    .unwrap_or_else(|error| panic!("rebuild: {error:?}"));
    assert_eq!(executed_unknown.protocol_consumed, Some(40));
    assert_eq!(executed_unknown.receipt_consumed, 25);
    assert_eq!(executed_unknown.held_unresolved, 15);
    assert_eq!(executed_unknown.unresolved_count, 1);
    assert!(executed_unknown.reconciled);
    executed_unknown
        .require_write_ready()
        .unwrap_or_else(|error| panic!("write ready: {error:?}"));

    let overspent = rebuild(
        &store,
        &tenant(),
        &unknown_ids,
        &receipts,
        &protocol(500, 41),
        &verifier,
    )
    .unwrap_or_else(|error| panic!("rebuild overspent: {error:?}"));
    assert_eq!(overspent.protocol_consumed, Some(41));
    assert!(!overspent.reconciled);
    assert!(matches!(
        overspent.require_write_ready(),
        Err(RestartError::Unreconciled)
    ));

    let recovered = recover(
        &mut store,
        &tenant(),
        &RecoveryInputs {
            verifier: verifier.clone(),
            unknown_budget_ids: &unknown_ids,
            budget_receipts: &receipts,
            protocol_budget: protocol(500, 25),
            ceiling_maximum: 1_000,
            ceiling_receipts: &[],
            unknown_ceiling_reservations: &[],
            current_sequence: 1,
        },
    )
    .unwrap_or_else(|error| panic!("recover: {error:?}"));
    assert_eq!(recovered.budget_accounting.protocol_consumed, Some(25));
    assert!(recovered.budget_accounting.reconciled);
    let _ = fs::remove_dir_all(root);
}

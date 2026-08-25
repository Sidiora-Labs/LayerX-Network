use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

use layerx_agentd::budget::{
    hold_unknown, rebuild, PersistedReceipt, ProtocolBudgetState, RestartError, UnknownReservation,
};
use layerx_agentd::store::{Store, TenantId};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

fn protocol(consumed: u128) -> ProtocolBudgetState {
    ProtocolBudgetState {
        evidence: support::raw_budget_state(consumed, 1_000 - consumed, 1, 100, 50),
    }
}

#[test]
fn process_loss_between_submission_and_receipt_preserves_unknown_hold() {
    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "layerx-budget-unknown-{}-{sequence}",
        std::process::id()
    ));
    let tenant = TenantId::new("tenant-a").unwrap_or_else(|error| panic!("tenant: {error}"));
    let mut before_kill = Store::open(&root).unwrap_or_else(|error| panic!("store: {error}"));
    hold_unknown(
        &mut before_kill,
        &UnknownReservation {
            tenant: tenant.clone(),
            id: [1; 32],
            amount: 400,
            expiry_sequence: 10,
            resolved: None,
        },
    )
    .unwrap_or_else(|error| panic!("hold: {error:?}"));
    drop(before_kill);

    let after_restart = Store::open(&root).unwrap_or_else(|error| panic!("restart: {error}"));
    let accounting = rebuild(&after_restart, &tenant, &[[1; 32]], &[], protocol(0))
        .unwrap_or_else(|error| panic!("rebuild: {error:?}"));
    assert_eq!(accounting.held_unresolved, 400);
    assert_eq!(accounting.unresolved_count, 1);
    assert!(accounting.require_write_ready().is_ok());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn writes_are_refused_until_receipts_and_protocol_state_reconcile() {
    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "layerx-budget-rebuild-{}-{sequence}",
        std::process::id()
    ));
    let tenant = TenantId::new("tenant-a").unwrap_or_else(|error| panic!("tenant: {error}"));
    let store = Store::open(&root).unwrap_or_else(|error| panic!("store: {error}"));
    let accounting = rebuild(
        &store,
        &tenant,
        &[],
        &[PersistedReceipt {
            evidence: support::raw_receipt([2; 32], 0, 100),
        }],
        protocol(200),
    )
    .unwrap_or_else(|error| panic!("rebuild: {error:?}"));
    assert_eq!(accounting.protocol_consumed, 200);
    assert_eq!(accounting.receipt_consumed, 100);
    assert!(matches!(
        accounting.require_write_ready(),
        Err(RestartError::Unreconciled)
    ));
    let _ = fs::remove_dir_all(root);
}
mod support;

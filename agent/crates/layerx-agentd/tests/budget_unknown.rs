use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

use layerx_agentd::budget::{
    hold_unknown, rebuild, PersistedReceipt, ProtocolBudgetState, RestartError, UnknownReservation,
};
use layerx_agentd::store::{Store, TenantId};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

fn protocol(consumed: u128) -> ProtocolBudgetState {
    ProtocolBudgetState {
        evidence: support::raw_state_leaf(consumed.to_be_bytes().to_vec(), 50),
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
    let accounting = rebuild(
        &after_restart,
        &tenant,
        &[[1; 32]],
        &[],
        &protocol(0),
        &support::evidence_verifier(),
    )
        .unwrap_or_else(|error| panic!("rebuild: {error:?}"));
    assert_eq!(accounting.held_unresolved, 400);
    assert_eq!(accounting.unresolved_count, 1);
    assert_eq!(accounting.protocol_consumed, None);
    assert!(matches!(
        accounting.require_write_ready(),
        Err(RestartError::ProtocolStateSchemaUnavailable)
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn writes_are_refused_when_no_canonical_protocol_budget_schema_exists() {
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
            expected_activity_id: [2; 32],
            evidence: support::raw_receipt([2; 32], 0, 100),
        }],
        &protocol(200),
        &support::evidence_verifier(),
    )
    .unwrap_or_else(|error| panic!("rebuild: {error:?}"));
    assert_eq!(accounting.protocol_consumed, None);
    assert_eq!(accounting.receipt_consumed, 100);
    assert!(matches!(
        accounting.require_write_ready(),
        Err(RestartError::ProtocolStateSchemaUnavailable)
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn restart_rejects_duplicate_receipt_and_activity_evidence() {
    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "layerx-budget-replay-{}-{sequence}",
        std::process::id()
    ));
    let tenant = TenantId::new("tenant-a").unwrap_or_else(|error| panic!("tenant: {error}"));
    let store = Store::open(&root).unwrap_or_else(|error| panic!("store: {error}"));
    let receipt = support::raw_receipt_at([3; 32], 0, 100, 9);
    assert!(matches!(
        rebuild(
            &store,
            &tenant,
            &[],
            &[
                PersistedReceipt {
                    expected_activity_id: [3; 32],
                    evidence: receipt.clone(),
                },
                PersistedReceipt {
                    expected_activity_id: [3; 32],
                    evidence: receipt,
                },
            ],
            &protocol(200),
            &support::evidence_verifier(),
        ),
        Err(RestartError::DuplicateReceipt)
    ));
    assert!(matches!(
        rebuild(
            &store,
            &tenant,
            &[],
            &[
                PersistedReceipt {
                    expected_activity_id: [4; 32],
                    evidence: support::raw_receipt_at([4; 32], 0, 100, 9),
                },
                PersistedReceipt {
                    expected_activity_id: [4; 32],
                    evidence: support::raw_receipt_at([4; 32], 0, 100, 10),
                },
            ],
            &protocol(200),
            &support::evidence_verifier(),
        ),
        Err(RestartError::DuplicateActivity)
    ));
    let _ = fs::remove_dir_all(root);
}
mod support;

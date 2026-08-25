use std::sync::Arc;
use std::thread;

use layerx_agentd::capability::{consume, Ceiling, CeilingError, ReceiptApplication};

#[test]
fn concurrent_reservations_never_exceed_the_ceiling() {
    let ceiling = Arc::new(Ceiling::new(1_000));
    let mut workers = Vec::new();
    for id in 0_u8..40 {
        let ceiling = Arc::clone(&ceiling);
        workers.push(thread::spawn(move || {
            consume(&ceiling, [id; 32], 100, 100, 1)
        }));
    }
    let accepted = workers
        .into_iter()
        .map(thread::JoinHandle::join)
        .filter(|result| result.as_ref().is_ok_and(Result::is_ok))
        .count();
    assert_eq!(accepted, 10);
    let snapshot = ceiling
        .snapshot()
        .unwrap_or_else(|error| panic!("snapshot: {error:?}"));
    assert_eq!(snapshot.held, 1_000);
    assert_eq!(snapshot.consumed, 0);
}

#[test]
fn only_verified_executed_receipts_consume_and_failures_release() {
    let ceiling = Ceiling::new(1_000);
    consume(&ceiling, [1; 32], 400, 100, 1).unwrap_or_else(|error| panic!("reserve: {error:?}"));
    let raw = support::raw_receipt([1; 32], 0, 400);
    let mut corrupted = raw.canonical_receipt().to_vec();
    corrupted[0] ^= 1;
    assert_eq!(
        ceiling.apply_receipt(&ReceiptApplication {
            reservation_id: [1; 32],
            evidence: support::corrupt_raw_receipt(&raw, corrupted),
        }),
        Err(CeilingError::UnverifiedReceipt)
    );
    ceiling
        .apply_receipt(&ReceiptApplication {
            reservation_id: [1; 32],
            evidence: support::raw_receipt([1; 32], 0, 400),
        })
        .unwrap_or_else(|error| panic!("receipt: {error:?}"));
    consume(&ceiling, [2; 32], 300, 100, 1).unwrap_or_else(|error| panic!("reserve: {error:?}"));
    ceiling
        .apply_receipt(&ReceiptApplication {
            reservation_id: [2; 32],
            evidence: support::raw_receipt([2; 32], 5, 300),
        })
        .unwrap_or_else(|error| panic!("failed receipt: {error:?}"));
    let snapshot = ceiling
        .snapshot()
        .unwrap_or_else(|error| panic!("snapshot: {error:?}"));
    assert_eq!(snapshot.consumed, 400);
    assert_eq!(snapshot.held, 0);
}

#[test]
fn unknown_is_held_past_expiry_and_rebuild_uses_verified_receipts() {
    let ceiling = Ceiling::new(1_000);
    consume(&ceiling, [1; 32], 400, 5, 1).unwrap_or_else(|error| panic!("reserve: {error:?}"));
    ceiling
        .mark_unknown([1; 32])
        .unwrap_or_else(|error| panic!("unknown: {error:?}"));
    assert_eq!(
        ceiling.cancel_unsubmitted([1; 32]),
        Err(CeilingError::Indeterminate)
    );
    assert_eq!(ceiling.release_expired(6), Ok(0));
    assert_eq!(ceiling.snapshot().map(|value| value.held), Ok(400));

    let rebuilt = Ceiling::rebuild(
        1_000,
        &[
            ReceiptApplication {
                reservation_id: [2; 32],
                evidence: support::raw_receipt([2; 32], 0, 250),
            },
            ReceiptApplication {
                reservation_id: [3; 32],
                evidence: support::raw_receipt([3; 32], 5, 100),
            },
        ],
    )
    .unwrap_or_else(|error| panic!("rebuild: {error:?}"));
    let snapshot = rebuilt
        .snapshot()
        .unwrap_or_else(|error| panic!("snapshot: {error:?}"));
    assert_eq!(snapshot.consumed, 250);
    assert!(snapshot.reconciled);
}
mod support;

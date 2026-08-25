use std::sync::Arc;
use std::thread;

use layerx_agentd::capability::{consume, Ceiling, CeilingError, ReceiptApplication};

#[test]
fn concurrent_reservations_never_exceed_the_ceiling() {
    let ceiling = Arc::new(Ceiling::new(1_000, support::evidence_verifier()));
    let mut workers = Vec::new();
    for id in 1_u8..=40 {
        let ceiling = Arc::clone(&ceiling);
        workers.push(thread::spawn(move || {
            consume(&ceiling, [id; 32], [id; 32], 100, 100, 1)
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
    let ceiling = Ceiling::new(1_000, support::evidence_verifier());
    consume(&ceiling, [1; 32], [1; 32], 400, 100, 1)
        .unwrap_or_else(|error| panic!("reserve: {error:?}"));
    let raw = support::raw_receipt([1; 32], 0, 400);
    let mut corrupted = raw.canonical_receipt().to_vec();
    corrupted[0] ^= 1;
    assert_eq!(
        ceiling.apply_receipt(&ReceiptApplication {
            reservation_id: [1; 32],
            expected_activity_id: [1; 32],
            evidence: support::corrupt_raw_receipt(&raw, corrupted),
        }),
        Err(CeilingError::UnverifiedReceipt)
    );
    ceiling
        .apply_receipt(&ReceiptApplication {
            reservation_id: [1; 32],
            expected_activity_id: [1; 32],
            evidence: support::raw_receipt([1; 32], 0, 400),
        })
        .unwrap_or_else(|error| panic!("receipt: {error:?}"));
    consume(&ceiling, [2; 32], [2; 32], 300, 100, 1)
        .unwrap_or_else(|error| panic!("reserve: {error:?}"));
    ceiling
        .apply_receipt(&ReceiptApplication {
            reservation_id: [2; 32],
            expected_activity_id: [2; 32],
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
    let ceiling = Ceiling::new(1_000, support::evidence_verifier());
    consume(&ceiling, [1; 32], [1; 32], 400, 5, 1)
        .unwrap_or_else(|error| panic!("reserve: {error:?}"));
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
        support::evidence_verifier(),
        &[
            ReceiptApplication {
                reservation_id: [2; 32],
                expected_activity_id: [2; 32],
                evidence: support::raw_receipt([2; 32], 0, 250),
            },
            ReceiptApplication {
                reservation_id: [3; 32],
                expected_activity_id: [3; 32],
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

#[test]
fn receipt_identity_is_bound_to_the_held_activity_and_cannot_be_replayed() {
    let ceiling = Ceiling::new(1_000, support::evidence_verifier());
    consume(&ceiling, [1; 32], [1; 32], 100, 100, 1)
        .unwrap_or_else(|error| panic!("reserve: {error:?}"));
    let receipt = support::raw_receipt([1; 32], 0, 100);
    assert_eq!(
        ceiling.apply_receipt(&ReceiptApplication {
            reservation_id: [1; 32],
            expected_activity_id: [2; 32],
            evidence: support::raw_receipt([2; 32], 0, 100),
        }),
        Err(CeilingError::ActivityMismatch)
    );
    ceiling
        .apply_receipt(&ReceiptApplication {
            reservation_id: [1; 32],
            expected_activity_id: [1; 32],
            evidence: receipt.clone(),
        })
        .unwrap_or_else(|error| panic!("matching receipt: {error:?}"));
    consume(&ceiling, [3; 32], [1; 32], 100, 100, 1)
        .unwrap_or_else(|error| panic!("second reservation: {error:?}"));
    assert_eq!(
        ceiling.apply_receipt(&ReceiptApplication {
            reservation_id: [3; 32],
            expected_activity_id: [1; 32],
            evidence: receipt,
        }),
        Err(CeilingError::DuplicateReceipt)
    );
}

#[test]
fn rebuild_rejects_duplicate_receipts_and_distinct_receipts_for_one_activity() {
    let receipt = support::raw_receipt_at([4; 32], 0, 100, 9);
    assert_eq!(
        Ceiling::rebuild(
            1_000,
            support::evidence_verifier(),
            &[
                ReceiptApplication {
                    reservation_id: [4; 32],
                    expected_activity_id: [4; 32],
                    evidence: receipt.clone(),
                },
                ReceiptApplication {
                    reservation_id: [5; 32],
                    expected_activity_id: [4; 32],
                    evidence: receipt,
                },
            ],
        )
        .map(|_| ()),
        Err(CeilingError::DuplicateReceipt)
    );
    assert_eq!(
        Ceiling::rebuild(
            1_000,
            support::evidence_verifier(),
            &[
                ReceiptApplication {
                    reservation_id: [6; 32],
                    expected_activity_id: [4; 32],
                    evidence: support::raw_receipt_at([4; 32], 0, 100, 9),
                },
                ReceiptApplication {
                    reservation_id: [7; 32],
                    expected_activity_id: [4; 32],
                    evidence: support::raw_receipt_at([4; 32], 0, 100, 10),
                },
            ],
        )
        .map(|_| ()),
        Err(CeilingError::DuplicateActivity)
    );
}

#[test]
fn one_activity_and_one_reservation_each_have_a_single_ceiling_identity() {
    let ceiling = Ceiling::new(1_000, support::evidence_verifier());
    consume(&ceiling, [1; 32], [8; 32], 100, 100, 1)
        .unwrap_or_else(|error| panic!("first reservation: {error:?}"));
    assert_eq!(
        consume(&ceiling, [2; 32], [8; 32], 100, 100, 1),
        Err(CeilingError::DuplicateActivity)
    );
    assert_eq!(
        Ceiling::rebuild(
            1_000,
            support::evidence_verifier(),
            &[
                ReceiptApplication {
                    reservation_id: [9; 32],
                    expected_activity_id: [9; 32],
                    evidence: support::raw_receipt_at([9; 32], 0, 100, 9),
                },
                ReceiptApplication {
                    reservation_id: [9; 32],
                    expected_activity_id: [10; 32],
                    evidence: support::raw_receipt_at([10; 32], 0, 100, 10),
                },
            ],
        )
        .map(|_| ()),
        Err(CeilingError::Duplicate)
    );
}
mod support;

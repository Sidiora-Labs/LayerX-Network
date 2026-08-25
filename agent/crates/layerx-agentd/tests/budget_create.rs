use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

use layerx_agentd::budget::{
    create_protocol_budget, BudgetCreationError, BudgetKind, BudgetPipeline, BudgetRequest,
    CoreBudgetReceipt, LocalLimit,
};
use layerx_agentd::store::{ObjectKind, Store, TenantId};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

struct SignedActivityPipeline {
    result: Result<CoreBudgetReceipt, BudgetCreationError>,
    submitted_bytes: Vec<u8>,
}

impl BudgetPipeline for SignedActivityPipeline {
    fn submit_budget(
        &mut self,
        request: &BudgetRequest,
    ) -> Result<CoreBudgetReceipt, BudgetCreationError> {
        self.submitted_bytes.clone_from(&request.canonical_activity);
        match &self.result {
            Ok(value) => Ok(value.clone()),
            Err(BudgetCreationError::Submission) => Err(BudgetCreationError::Submission),
            Err(other) => panic!("unexpected pipeline fixture: {other:?}"),
        }
    }
}

fn tenant() -> TenantId {
    TenantId::new("tenant-a").unwrap_or_else(|error| panic!("tenant: {error}"))
}

fn request() -> BudgetRequest {
    let submission = support::verified_submission(1);
    BudgetRequest {
        tenant: tenant(),
        request_id: [1; 32],
        kind: BudgetKind::ProtocolBudget,
        asset: [2; 32],
        ceiling: 5_000,
        expiry_sequence: 100,
        canonical_activity: submission.exact_bytes().to_vec(),
        verified_submission: Some(submission),
    }
}

fn root(label: &str) -> std::path::PathBuf {
    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "layerx-budget-{label}-{}-{sequence}",
        std::process::id()
    ))
}

#[test]
fn verified_creation_without_a_proven_object_effect_fails_closed() {
    let path = root("success");
    let mut store = Store::open(&path).unwrap_or_else(|error| panic!("store: {error}"));
    let request = request();
    let activity_id = request
        .verified_submission
        .as_ref()
        .map(layerx_agentd::sign::VerifiedSubmission::activity_id)
        .unwrap_or_else(|| panic!("verified submission missing"));
    let evidence = support::raw_receipt(activity_id, 0, 25);
    let mut pipeline = SignedActivityPipeline {
        result: Ok(CoreBudgetReceipt {
            evidence,
        }),
        submitted_bytes: Vec::new(),
    };
    assert_eq!(
        create_protocol_budget(
            &mut store,
            &request,
            &support::evidence_verifier(),
            &mut pipeline,
        ),
        Err(BudgetCreationError::ProtocolObjectEffectUnavailable)
    );
    assert_eq!(pipeline.submitted_bytes, request.canonical_activity);
    assert!(store.list_object_ids(&tenant(), ObjectKind::Budget).is_empty());
    let _ = fs::remove_dir_all(path);
}

#[test]
fn failed_creation_leaves_no_daemon_budget_record() {
    let path = root("failure");
    let mut store = Store::open(&path).unwrap_or_else(|error| panic!("store: {error}"));
    let mut pipeline = SignedActivityPipeline {
        result: Err(BudgetCreationError::Submission),
        submitted_bytes: Vec::new(),
    };
    assert!(matches!(
        create_protocol_budget(
            &mut store,
            &request(),
            &support::evidence_verifier(),
            &mut pipeline,
        ),
        Err(BudgetCreationError::Submission)
    ));
    assert!(store.list_object_ids(&tenant(), ObjectKind::Budget).is_empty());
    let _ = fs::remove_dir_all(path);
}

#[test]
fn unverified_or_substituted_canonical_activity_never_reaches_submission() {
    let path = root("activity-binding");
    let mut store = Store::open(&path).unwrap_or_else(|error| panic!("store: {error}"));
    let mut pipeline = SignedActivityPipeline {
        result: Err(BudgetCreationError::Submission),
        submitted_bytes: Vec::new(),
    };
    let mut unavailable = request();
    unavailable.verified_submission = None;
    assert_eq!(
        create_protocol_budget(
            &mut store,
            &unavailable,
            &support::evidence_verifier(),
            &mut pipeline,
        ),
        Err(BudgetCreationError::ActivityBindingUnavailable)
    );
    assert!(pipeline.submitted_bytes.is_empty());

    let mut substituted = request();
    substituted.canonical_activity = support::verified_submission(2).exact_bytes().to_vec();
    assert_eq!(
        create_protocol_budget(
            &mut store,
            &substituted,
            &support::evidence_verifier(),
            &mut pipeline,
        ),
        Err(BudgetCreationError::ActivityBindingMismatch)
    );
    assert!(pipeline.submitted_bytes.is_empty());

    let mut request_substitution = request();
    request_substitution.request_id = [2; 32];
    assert_eq!(
        create_protocol_budget(
            &mut store,
            &request_substitution,
            &support::evidence_verifier(),
            &mut pipeline,
        ),
        Err(BudgetCreationError::ActivityBindingMismatch)
    );
    assert!(pipeline.submitted_bytes.is_empty());
    assert!(store.list_object_ids(&tenant(), ObjectKind::Budget).is_empty());
    let _ = fs::remove_dir_all(path);
}

#[test]
fn unverifiable_creation_receipt_leaves_no_protocol_budget_cache() {
    let path = root("unverified");
    let mut store = Store::open(&path).unwrap_or_else(|error| panic!("store: {error}"));
    let raw = support::raw_receipt([1; 32], 0, 25);
    let mut corrupt = raw.canonical_receipt().to_vec();
    corrupt[0] ^= 1;
    let mut pipeline = SignedActivityPipeline {
        result: Ok(CoreBudgetReceipt {
            evidence: support::corrupt_raw_receipt(&raw, corrupt),
        }),
        submitted_bytes: Vec::new(),
    };
    assert!(matches!(
        create_protocol_budget(
            &mut store,
            &request(),
            &support::evidence_verifier(),
            &mut pipeline,
        ),
        Err(BudgetCreationError::UnverifiedReceipt)
    ));
    assert!(store.list_object_ids(&tenant(), ObjectKind::Budget).is_empty());
    let _ = fs::remove_dir_all(path);
}

#[test]
fn receipt_for_another_canonical_activity_cannot_create_or_cache_a_budget() {
    let path = root("activity-mismatch");
    let mut store = Store::open(&path).unwrap_or_else(|error| panic!("store: {error}"));
    let mut pipeline = SignedActivityPipeline {
        result: Ok(CoreBudgetReceipt {
            evidence: support::raw_receipt(
                support::verified_submission(2).activity_id(),
                0,
                25,
            ),
        }),
        submitted_bytes: Vec::new(),
    };
    assert_eq!(
        create_protocol_budget(
            &mut store,
            &request(),
            &support::evidence_verifier(),
            &mut pipeline,
        ),
        Err(BudgetCreationError::ReceiptActivityMismatch)
    );
    assert!(store.list_object_ids(&tenant(), ObjectKind::Budget).is_empty());
    let _ = fs::remove_dir_all(path);
}

#[test]
fn local_limit_is_never_described_as_protocol_equivalent() {
    let limit = LocalLimit::new(tenant(), [4; 32], [2; 32], 500);
    assert_eq!(limit.enforcement, "daemon-enforced");
    assert!(limit
        .bypass_statement
        .contains("bypassing layerx-agentd bypasses"));
    assert!(!limit.bypass_statement.contains("equivalent"));
}
mod support;

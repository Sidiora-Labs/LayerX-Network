use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

use layerx_agentd::budget::{
    create_protocol_budget, BudgetCreationError, BudgetKind, BudgetPipeline, BudgetRequest,
    CoreBudgetReceipt, LocalLimit,
};
use layerx_agentd::store::{ObjectKind, Store, TenantId, TenantKey};

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
    BudgetRequest {
        tenant: tenant(),
        request_id: [1; 32],
        kind: BudgetKind::ProtocolBudget,
        asset: [2; 32],
        ceiling: 5_000,
        expiry_sequence: 100,
        canonical_activity: b"canonical-signed-budget-activity".to_vec(),
        signature: [3; 64],
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
fn creation_returns_protocol_object_and_verified_receipt() {
    let path = root("success");
    let mut store = Store::open(&path).unwrap_or_else(|error| panic!("store: {error}"));
    let evidence = support::raw_receipt([1; 32], 0, 25);
    let expected_receipt = evidence.canonical_receipt().to_vec();
    let mut pipeline = SignedActivityPipeline {
        result: Ok(CoreBudgetReceipt {
            object_id: [9; 32],
            evidence,
        }),
        submitted_bytes: Vec::new(),
    };
    let budget = create_protocol_budget(
        &mut store,
        &request(),
        &support::evidence_verifier(),
        &mut pipeline,
    )
        .unwrap_or_else(|error| panic!("creation: {error:?}"));
    assert_eq!(pipeline.submitted_bytes, request().canonical_activity);
    assert_eq!(budget.object_id, [9; 32]);
    assert_eq!(budget.receipt_bytes, expected_receipt);
    assert_eq!(budget.enforcement, "protocol-enforced");
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
    let absent = TenantKey::new(tenant(), ObjectKind::Budget, [9; 32].to_vec())
        .unwrap_or_else(|error| panic!("key: {error}"));
    assert!(store.get(&absent).is_none());
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
            object_id: [9; 32],
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
    let absent = TenantKey::new(tenant(), ObjectKind::Budget, [9; 32].to_vec())
        .unwrap_or_else(|error| panic!("key: {error}"));
    assert!(store.get(&absent).is_none());
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

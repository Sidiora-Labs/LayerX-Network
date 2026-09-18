use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

use layerx_agentd::budget::{
    budget_create_identity, budget_state_key, create_protocol_budget, BudgetCreationError,
    BudgetKind, BudgetPipeline, BudgetRequest, CoreBudgetReceipt, LocalLimit, ProtocolBudgetRecord,
    ProtocolBudgetState,
};
use layerx_agentd::human_runtime::{recover_tenant_budget, BudgetRecoveryRequest};
use layerx_agentd::sign::VerifiedSubmission;
use layerx_agentd::store::{ObjectKind, Store, TenantId};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

const ASSET: [u8; 32] = [2; 32];
const CEILING: u128 = 5_000;
const EXPIRY: u64 = 100;
const OBSERVED_HEAD: u64 = 99;

struct SignedActivityPipeline {
    result: Result<CoreBudgetReceipt, BudgetCreationError>,
    state: Result<ProtocolBudgetState, BudgetCreationError>,
    submitted_bytes: Vec<u8>,
    state_reads: Vec<[u8; 32]>,
}

impl SignedActivityPipeline {
    fn new(
        result: Result<CoreBudgetReceipt, BudgetCreationError>,
        state: Result<ProtocolBudgetState, BudgetCreationError>,
    ) -> Self {
        Self {
            result,
            state,
            submitted_bytes: Vec::new(),
            state_reads: Vec::new(),
        }
    }
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

    fn budget_state(
        &mut self,
        budget_id: [u8; 32],
    ) -> Result<ProtocolBudgetState, BudgetCreationError> {
        self.state_reads.push(budget_id);
        match &self.state {
            Ok(value) => Ok(value.clone()),
            Err(BudgetCreationError::CreatedBudgetUnconfirmed) => {
                Err(BudgetCreationError::CreatedBudgetUnconfirmed)
            }
            Err(other) => panic!("unexpected state fixture: {other:?}"),
        }
    }
}

fn tenant() -> TenantId {
    TenantId::new("tenant-a").unwrap_or_else(|error| panic!("tenant: {error}"))
}

fn submission(id: u8) -> VerifiedSubmission {
    support::budget_create_submission(id, support::BUDGET_CREATE_ID, ASSET, CEILING, EXPIRY)
}

fn request_for(submission: VerifiedSubmission) -> BudgetRequest {
    BudgetRequest {
        tenant: tenant(),
        request_id: submission.idempotency_key(),
        kind: BudgetKind::ProtocolBudget,
        asset: ASSET,
        ceiling: CEILING,
        expiry_sequence: EXPIRY,
        canonical_activity: submission.exact_bytes().to_vec(),
        verified_submission: Some(submission),
    }
}

fn request() -> BudgetRequest {
    request_for(submission(1))
}

fn confirmed_state() -> ProtocolBudgetState {
    ProtocolBudgetState {
        evidence: support::raw_state_leaf(
            support::core_budget_record_for(support::BUDGET_CREATE_ID, ASSET, CEILING, EXPIRY),
            OBSERVED_HEAD,
        ),
    }
}

fn receipt_for(request: &BudgetRequest) -> CoreBudgetReceipt {
    let activity_id = request.verified_submission.as_ref().map_or_else(
        || panic!("verified submission missing"),
        VerifiedSubmission::activity_id,
    );
    CoreBudgetReceipt {
        evidence: support::raw_receipt(activity_id, 0, 25),
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
fn budget_identity_is_read_from_the_signed_payload_by_the_core_keying_rule() {
    let submission = submission(1);
    let identity = budget_create_identity(submission.exact_bytes(), &support::budget_registry())
        .unwrap_or_else(|error| panic!("identity: {error:?}"));
    assert_eq!(identity.budget_id, support::BUDGET_CREATE_ID);
    assert_eq!(identity.owner, support::BUDGET_CREATE_OWNER);
    assert_eq!(identity.budget_account, support::BUDGET_CREATE_ACCOUNT);
    assert_eq!(identity.asset, ASSET);
    assert_eq!(identity.per_period_limit, CEILING);
    assert_eq!(identity.period_length, support::BUDGET_CREATE_PERIOD_LENGTH);
    assert_eq!(identity.expiry, EXPIRY);
    assert_eq!(
        budget_state_key(identity.budget_id),
        budget_state_key(support::BUDGET_CREATE_ID)
    );
    assert_eq!(
        budget_create_identity(
            support::verified_submission(1).exact_bytes(),
            &support::budget_registry()
        ),
        Err(BudgetCreationError::NotBudgetCreation)
    );
}

#[test]
fn confirmed_creation_returns_the_core_keyed_budget_without_caching_on_trust() {
    let path = root("confirmed");
    let mut store = Store::open(&path).unwrap_or_else(|error| panic!("store: {error}"));
    let request = request();
    let mut pipeline =
        SignedActivityPipeline::new(Ok(receipt_for(&request)), Ok(confirmed_state()));
    let budget = create_protocol_budget(
        &mut store,
        &request,
        &support::budget_registry(),
        &support::evidence_verifier(),
        &mut pipeline,
    )
    .unwrap_or_else(|error| panic!("create: {error:?}"));
    assert_eq!(pipeline.submitted_bytes, request.canonical_activity);
    assert_eq!(pipeline.state_reads, vec![support::BUDGET_CREATE_ID]);
    assert_eq!(budget.object_id(), support::BUDGET_CREATE_ID);
    assert_eq!(budget.kind(), BudgetKind::ProtocolBudget);
    assert_eq!(budget.enforcement(), "protocol-enforced");
    assert_eq!(budget.observed_head_sequence(), OBSERVED_HEAD);
    assert_eq!(budget.record().per_period_limit, CEILING);
    assert_eq!(budget.record().expiry, EXPIRY);
    assert_eq!(budget.record().asset_id, ASSET);
    assert_eq!(
        budget.record(),
        &ProtocolBudgetRecord::decode(&support::core_budget_record_for(
            support::BUDGET_CREATE_ID,
            ASSET,
            CEILING,
            EXPIRY
        ))
        .unwrap_or_else(|error| panic!("record: {error:?}"))
    );
    assert!(!budget.receipt_bytes().is_empty());
    assert!(store
        .list_object_ids(&tenant(), ObjectKind::Budget)
        .is_empty());
    let _ = fs::remove_dir_all(path);
}

#[test]
fn created_budget_is_reconciled_and_writes_admitted_after_restart() {
    let path = root("restart");
    let budget_id = {
        let mut store = Store::open(&path).unwrap_or_else(|error| panic!("store: {error}"));
        let request = request();
        let mut pipeline =
            SignedActivityPipeline::new(Ok(receipt_for(&request)), Ok(confirmed_state()));
        create_protocol_budget(
            &mut store,
            &request,
            &support::budget_registry(),
            &support::evidence_verifier(),
            &mut pipeline,
        )
        .unwrap_or_else(|error| panic!("create: {error:?}"))
        .object_id()
    };
    let mut reopened = Store::open(&path).unwrap_or_else(|error| panic!("reopen: {error}"));
    let recovery = recover_tenant_budget(
        &mut reopened,
        &tenant(),
        &BudgetRecoveryRequest {
            budget_id,
            protocol_budget: confirmed_state(),
            verifier: support::evidence_verifier(),
            receipts_with_evidence: &[],
            receipts_without_evidence: &[],
            ceiling_maximum: 10_000,
            current_sequence: OBSERVED_HEAD,
        },
    )
    .unwrap_or_else(|error| panic!("recover: {error:?}"));
    let accounting = recovery.recovered.budget_accounting;
    assert_eq!(accounting.protocol_consumed, Some(0));
    assert_eq!(accounting.receipt_consumed, 0);
    assert_eq!(accounting.held_unresolved, 0);
    assert_eq!(accounting.unresolved_count, 0);
    assert!(accounting.reconciled);
    assert!(recovery
        .recovered
        .ceiling
        .snapshot()
        .is_ok_and(|snapshot| snapshot.reconciled));
    assert!(recovery.recovered.require_write_ready().is_ok());
    assert!(recovery.admission.is_ok());
    let _ = fs::remove_dir_all(path);
}

#[test]
fn executed_receipt_without_proven_budget_state_fails_closed() {
    let path = root("unconfirmed");
    let mut store = Store::open(&path).unwrap_or_else(|error| panic!("store: {error}"));
    let request = request();
    let mut pipeline = SignedActivityPipeline::new(
        Ok(receipt_for(&request)),
        Err(BudgetCreationError::CreatedBudgetUnconfirmed),
    );
    assert_eq!(
        create_protocol_budget(
            &mut store,
            &request,
            &support::budget_registry(),
            &support::evidence_verifier(),
            &mut pipeline,
        ),
        Err(BudgetCreationError::CreatedBudgetUnconfirmed)
    );
    assert_eq!(pipeline.submitted_bytes, request.canonical_activity);
    assert_eq!(pipeline.state_reads, vec![support::BUDGET_CREATE_ID]);
    assert!(store
        .list_object_ids(&tenant(), ObjectKind::Budget)
        .is_empty());
    let _ = fs::remove_dir_all(path);
}

#[test]
fn proven_state_that_does_not_match_the_signed_creation_fails_closed() {
    let path = root("mismatched-state");
    let mut store = Store::open(&path).unwrap_or_else(|error| panic!("store: {error}"));
    let request = request();
    let other_id = support::core_budget_record_for([0x0c; 32], ASSET, CEILING, EXPIRY);
    let other_limit =
        support::core_budget_record_for(support::BUDGET_CREATE_ID, ASSET, CEILING - 1, EXPIRY);
    let other_expiry =
        support::core_budget_record_for(support::BUDGET_CREATE_ID, ASSET, CEILING, EXPIRY + 1);
    let mut closed =
        support::core_budget_record_for(support::BUDGET_CREATE_ID, ASSET, CEILING, EXPIRY);
    closed[275] = 1;
    let authentic = support::raw_state_leaf(
        support::core_budget_record_for(support::BUDGET_CREATE_ID, ASSET, CEILING, EXPIRY),
        OBSERVED_HEAD,
    );
    let mut tampered = authentic.canonical_state().to_vec();
    tampered[162] ^= 1;
    let unverifiable = support::corrupt_raw_state(&authentic, tampered);
    for evidence in [
        support::raw_state_leaf(other_id, OBSERVED_HEAD),
        support::raw_state_leaf(other_limit, OBSERVED_HEAD),
        support::raw_state_leaf(other_expiry, OBSERVED_HEAD),
        support::raw_state_leaf(closed, OBSERVED_HEAD),
        unverifiable,
    ] {
        let mut pipeline = SignedActivityPipeline::new(
            Ok(receipt_for(&request)),
            Ok(ProtocolBudgetState { evidence }),
        );
        assert_eq!(
            create_protocol_budget(
                &mut store,
                &request,
                &support::budget_registry(),
                &support::evidence_verifier(),
                &mut pipeline,
            ),
            Err(BudgetCreationError::CreatedBudgetUnconfirmed)
        );
        assert_eq!(pipeline.state_reads, vec![support::BUDGET_CREATE_ID]);
    }
    assert!(store
        .list_object_ids(&tenant(), ObjectKind::Budget)
        .is_empty());
    let _ = fs::remove_dir_all(path);
}

#[test]
fn activity_that_is_not_a_matching_budget_creation_never_reaches_submission() {
    let path = root("not-creation");
    let mut store = Store::open(&path).unwrap_or_else(|error| panic!("store: {error}"));
    let mut pipeline = SignedActivityPipeline::new(
        Err(BudgetCreationError::Submission),
        Err(BudgetCreationError::CreatedBudgetUnconfirmed),
    );
    let asset_send = support::verified_submission(1);
    let mut not_budget = request();
    not_budget.request_id = asset_send.idempotency_key();
    not_budget.canonical_activity = asset_send.exact_bytes().to_vec();
    not_budget.verified_submission = Some(asset_send);
    assert_eq!(
        create_protocol_budget(
            &mut store,
            &not_budget,
            &support::budget_registry(),
            &support::evidence_verifier(),
            &mut pipeline,
        ),
        Err(BudgetCreationError::NotBudgetCreation)
    );
    assert!(pipeline.submitted_bytes.is_empty());

    let mut other_ceiling = request();
    other_ceiling.ceiling = CEILING - 1;
    assert_eq!(
        create_protocol_budget(
            &mut store,
            &other_ceiling,
            &support::budget_registry(),
            &support::evidence_verifier(),
            &mut pipeline,
        ),
        Err(BudgetCreationError::IdentityMismatch)
    );
    let mut other_expiry = request();
    other_expiry.expiry_sequence = EXPIRY + 1;
    assert_eq!(
        create_protocol_budget(
            &mut store,
            &other_expiry,
            &support::budget_registry(),
            &support::evidence_verifier(),
            &mut pipeline,
        ),
        Err(BudgetCreationError::IdentityMismatch)
    );
    let mut other_asset = request();
    other_asset.asset = [3; 32];
    assert_eq!(
        create_protocol_budget(
            &mut store,
            &other_asset,
            &support::budget_registry(),
            &support::evidence_verifier(),
            &mut pipeline,
        ),
        Err(BudgetCreationError::IdentityMismatch)
    );
    assert!(pipeline.submitted_bytes.is_empty());
    assert!(pipeline.state_reads.is_empty());
    assert!(store
        .list_object_ids(&tenant(), ObjectKind::Budget)
        .is_empty());
    let _ = fs::remove_dir_all(path);
}

#[test]
fn failed_creation_leaves_no_daemon_budget_record() {
    let path = root("failure");
    let mut store = Store::open(&path).unwrap_or_else(|error| panic!("store: {error}"));
    let mut pipeline = SignedActivityPipeline::new(
        Err(BudgetCreationError::Submission),
        Err(BudgetCreationError::CreatedBudgetUnconfirmed),
    );
    assert!(matches!(
        create_protocol_budget(
            &mut store,
            &request(),
            &support::budget_registry(),
            &support::evidence_verifier(),
            &mut pipeline,
        ),
        Err(BudgetCreationError::Submission)
    ));
    assert!(pipeline.state_reads.is_empty());
    assert!(store
        .list_object_ids(&tenant(), ObjectKind::Budget)
        .is_empty());
    let _ = fs::remove_dir_all(path);
}

#[test]
fn unverified_or_substituted_canonical_activity_never_reaches_submission() {
    let path = root("activity-binding");
    let mut store = Store::open(&path).unwrap_or_else(|error| panic!("store: {error}"));
    let mut pipeline = SignedActivityPipeline::new(
        Err(BudgetCreationError::Submission),
        Err(BudgetCreationError::CreatedBudgetUnconfirmed),
    );
    let mut unavailable = request();
    unavailable.verified_submission = None;
    assert_eq!(
        create_protocol_budget(
            &mut store,
            &unavailable,
            &support::budget_registry(),
            &support::evidence_verifier(),
            &mut pipeline,
        ),
        Err(BudgetCreationError::ActivityBindingUnavailable)
    );
    assert!(pipeline.submitted_bytes.is_empty());

    let mut substituted = request();
    substituted.canonical_activity = submission(2).exact_bytes().to_vec();
    assert_eq!(
        create_protocol_budget(
            &mut store,
            &substituted,
            &support::budget_registry(),
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
            &support::budget_registry(),
            &support::evidence_verifier(),
            &mut pipeline,
        ),
        Err(BudgetCreationError::ActivityBindingMismatch)
    );
    assert!(pipeline.submitted_bytes.is_empty());
    assert!(pipeline.state_reads.is_empty());
    assert!(store
        .list_object_ids(&tenant(), ObjectKind::Budget)
        .is_empty());
    let _ = fs::remove_dir_all(path);
}

#[test]
fn unverifiable_creation_receipt_leaves_no_protocol_budget_cache() {
    let path = root("unverified");
    let mut store = Store::open(&path).unwrap_or_else(|error| panic!("store: {error}"));
    let raw = support::raw_receipt(submission(1).activity_id(), 0, 25);
    let mut corrupt = raw.canonical_receipt().to_vec();
    corrupt[0] ^= 1;
    let mut pipeline = SignedActivityPipeline::new(
        Ok(CoreBudgetReceipt {
            evidence: support::corrupt_raw_receipt(&raw, corrupt),
        }),
        Ok(confirmed_state()),
    );
    assert!(matches!(
        create_protocol_budget(
            &mut store,
            &request(),
            &support::budget_registry(),
            &support::evidence_verifier(),
            &mut pipeline,
        ),
        Err(BudgetCreationError::UnverifiedReceipt)
    ));
    assert!(pipeline.state_reads.is_empty());
    assert!(store
        .list_object_ids(&tenant(), ObjectKind::Budget)
        .is_empty());
    let _ = fs::remove_dir_all(path);
}

#[test]
fn receipt_for_another_canonical_activity_cannot_create_or_cache_a_budget() {
    let path = root("activity-mismatch");
    let mut store = Store::open(&path).unwrap_or_else(|error| panic!("store: {error}"));
    let mut pipeline = SignedActivityPipeline::new(
        Ok(CoreBudgetReceipt {
            evidence: support::raw_receipt(submission(2).activity_id(), 0, 25),
        }),
        Ok(confirmed_state()),
    );
    assert_eq!(
        create_protocol_budget(
            &mut store,
            &request(),
            &support::budget_registry(),
            &support::evidence_verifier(),
            &mut pipeline,
        ),
        Err(BudgetCreationError::ReceiptActivityMismatch)
    );
    assert!(pipeline.state_reads.is_empty());
    assert!(store
        .list_object_ids(&tenant(), ObjectKind::Budget)
        .is_empty());
    let _ = fs::remove_dir_all(path);
}

#[test]
fn rejected_creation_receipt_is_never_confirmed_from_state() {
    let path = root("rejected");
    let mut store = Store::open(&path).unwrap_or_else(|error| panic!("store: {error}"));
    let request = request();
    let activity_id = request.verified_submission.as_ref().map_or_else(
        || panic!("verified submission missing"),
        VerifiedSubmission::activity_id,
    );
    let mut pipeline = SignedActivityPipeline::new(
        Ok(CoreBudgetReceipt {
            evidence: support::raw_receipt(activity_id, 7, 25),
        }),
        Ok(confirmed_state()),
    );
    assert_eq!(
        create_protocol_budget(
            &mut store,
            &request,
            &support::budget_registry(),
            &support::evidence_verifier(),
            &mut pipeline,
        ),
        Err(BudgetCreationError::CoreRejected)
    );
    assert!(pipeline.state_reads.is_empty());
    assert!(store
        .list_object_ids(&tenant(), ObjectKind::Budget)
        .is_empty());
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

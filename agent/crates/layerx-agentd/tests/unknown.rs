use std::collections::BTreeMap;
use std::future::Future;
use std::pin::pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

use layerx_agentd::budget::{
    reserve, BudgetLimiter, LimitConfig, LimitId, LimitScope, ReservationRequest,
};
use layerx_agentd::capability::{consume, Ceiling};
use layerx_agentd::outbox::{
    resolve_unknown, Outbox, ReceiptLookup, ResendObservation, ResolutionObservation,
    SubmissionState, UnknownBoundaryError,
};
use layerx_agentd::protocol_evidence::RawReceiptEvidence;
use layerx_agentd::prepare::{
    prepare_activity, CorePreparationBoundary, CorePreparationState, CoreStateError,
    PreparationDefaults, PrepareRequest,
};
use layerx_agentd::sign::{attach_external_signature, verify_before_submit, VerifiedSubmission};
use layerx_agentd::store::{Store, TenantId};
use layerx_crypto::local::LocalSigner;
use layerx_crypto::signer::{sign_disclosed, Signer};
use layerx_types::activity::{Authority, TimestampBound};
use layerx_types::amount::Amount;
use layerx_types::ids::{Did, IdempotencyKey};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_wire::encode::Encoder;

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

struct NoopWake;

impl Wake for NoopWake {
    fn wake(self: Arc<Self>) {}
}

fn ready<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let waker = Waker::from(Arc::new(NoopWake));
    let mut context = Context::from_waker(&waker);
    match future.as_mut().poll(&mut context) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("local signing unexpectedly blocked"),
    }
}

struct RecordedCore(CorePreparationState);

impl CorePreparationBoundary for RecordedCore {
    fn preparation_state(&mut self, _actor: &Did) -> Result<CorePreparationState, CoreStateError> {
        Ok(self.0.clone())
    }
}

#[derive(Default)]
struct FaultInjectedNode {
    receipts: BTreeMap<[u8; 32], RawReceiptEvidence>,
    transmitted: Vec<([u8; 32], Vec<u8>)>,
    lookup_unavailable: bool,
    lose_resend_response: bool,
    receipt_on_resend: Option<RawReceiptEvidence>,
}

impl ReceiptLookup for FaultInjectedNode {
    fn receipt_by_idempotency_key(
        &mut self,
        idempotency_key: [u8; 32],
    ) -> Result<Option<RawReceiptEvidence>, UnknownBoundaryError> {
        if self.lookup_unavailable {
            return Err(UnknownBoundaryError::Unavailable);
        }
        Ok(self.receipts.get(&idempotency_key).cloned())
    }

    fn resend_exact(
        &mut self,
        idempotency_key: [u8; 32],
        signed_canonical_bytes: &[u8],
    ) -> Result<(), UnknownBoundaryError> {
        self.transmitted
            .push((idempotency_key, signed_canonical_bytes.to_vec()));
        if let Some(receipt) = self.receipt_on_resend.take() {
            self.receipts.insert(idempotency_key, receipt);
        }
        if self.lose_resend_response {
            Err(UnknownBoundaryError::Unavailable)
        } else {
            Ok(())
        }
    }
}

fn activity_type() -> ActivityType {
    ActivityType::new(ModuleId::Asset, 5).unwrap_or_else(|error| panic!("activity: {error:?}"))
}

fn registry() -> ModuleRegistry {
    ModuleRegistry::new(
        &[ModuleRegistration::new(ModuleId::Asset, &[activity_type()])
            .unwrap_or_else(|error| panic!("registration: {error:?}"))],
    )
    .unwrap_or_else(|error| panic!("registry: {error:?}"))
}

fn send_payload(id: u8) -> Vec<u8> {
    let mut encoder = Encoder::new(512);
    encoder
        .u16(0x5301)
        .unwrap_or_else(|error| panic!("tag: {error:?}"));
    encoder
        .u16(10)
        .unwrap_or_else(|error| panic!("fields: {error:?}"));
    for fixed in [[0x11; 32], [0x22; 32], [0x33; 32]] {
        encoder
            .fixed(&fixed)
            .unwrap_or_else(|error| panic!("fixed: {error:?}"));
    }
    encoder
        .u128(25)
        .unwrap_or_else(|error| panic!("amount: {error:?}"));
    encoder
        .u64(5)
        .unwrap_or_else(|error| panic!("sequence: {error:?}"));
    encoder
        .fixed(&[id; 32])
        .unwrap_or_else(|error| panic!("idempotency: {error:?}"));
    encoder
        .u64(1_010)
        .unwrap_or_else(|error| panic!("expiry: {error:?}"));
    encoder
        .fixed(&[0x55; 32])
        .unwrap_or_else(|error| panic!("context: {error:?}"));
    encoder
        .u8(0)
        .unwrap_or_else(|error| panic!("conditions: {error:?}"));
    encoder
        .u8(1)
        .unwrap_or_else(|error| panic!("authority: {error:?}"));
    encoder
        .fixed(&[0x11; 32])
        .unwrap_or_else(|error| panic!("controller: {error:?}"));
    encoder
        .fixed(&[0x66; 32])
        .unwrap_or_else(|error| panic!("payload key: {error:?}"));
    encoder
        .fixed(&[0x77; 64])
        .unwrap_or_else(|error| panic!("payload signature: {error:?}"));
    encoder
        .fixed(&[0x55; 32])
        .unwrap_or_else(|error| panic!("signed context: {error:?}"));
    encoder
        .u32(17)
        .unwrap_or_else(|error| panic!("network: {error:?}"));
    encoder
        .u16(1)
        .unwrap_or_else(|error| panic!("version: {error:?}"));
    encoder.finish()
}

fn verified_submission(id: u8) -> VerifiedSubmission {
    let mut core = RecordedCore(CorePreparationState {
        network_id: 17,
        account_sequence: 5,
        protocol_timestamp: 1_000,
        observed_head_sequence: 88,
        module_registry: registry(),
    });
    let prepared = prepare_activity(
        &mut core,
        PreparationDefaults {
            timestamp_span: 30,
            fee_limit: Amount::from_u128(12),
            maximum_payload_bytes: 1_024,
        },
        PrepareRequest {
            actor: Did::new(b"did:layerx:unknown").unwrap_or_else(|error| panic!("DID: {error:?}")),
            authority: Authority::owner(b"external-authority")
                .unwrap_or_else(|error| panic!("authority: {error:?}")),
            activity_type: activity_type(),
            expected_account_sequence: Some(5),
            timestamp_bound: Some(
                TimestampBound::new(995, 1_010)
                    .unwrap_or_else(|error| panic!("timestamp: {error:?}")),
            ),
            fee_limit: Some(Amount::from_u128(7)),
            idempotency_key: IdempotencyKey::new([id; 32]),
            payload: send_payload(id),
            declared_payload_limit: 1_024,
        },
    )
    .unwrap_or_else(|error| panic!("prepare: {error:?}"));
    let signer = LocalSigner::new([0xa5; 32]);
    let signature = ready(sign_disclosed(
        &signer,
        &prepared.canonical_bytes,
        &prepared.disclosure,
        &registry(),
    ))
    .unwrap_or_else(|error| panic!("sign: {error:?}"));
    let signed_bytes = attach_external_signature(&prepared, *signature.as_bytes())
        .unwrap_or_else(|error| panic!("attach: {error:?}"));
    verify_before_submit(&signed_bytes, &prepared, &signer.public_key(), &registry())
        .unwrap_or_else(|error| panic!("verify: {error:?}"))
}

fn directory(label: &str) -> std::path::PathBuf {
    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "layerx-unknown-{label}-{}-{sequence}",
        std::process::id()
    ))
}

fn tenant() -> TenantId {
    TenantId::new("tenant-a").unwrap_or_else(|error| panic!("tenant: {error}"))
}

fn unknown_outbox(root: &std::path::Path, id: u8) -> (Store, Outbox, Vec<u8>) {
    let verified = verified_submission(id);
    let exact = verified.exact_bytes().to_vec();
    let mut store = Store::open(root).unwrap_or_else(|error| panic!("store: {error}"));
    let mut outbox = Outbox::default();
    outbox
        .enqueue(&mut store, tenant(), [id; 32], verified)
        .unwrap_or_else(|error| panic!("enqueue: {error:?}"));
    outbox
        .transition(
            &mut store,
            [id; 32],
            SubmissionState::Submitted,
            "transport started",
            None,
        )
        .unwrap_or_else(|error| panic!("submit: {error:?}"));
    outbox
        .transition(
            &mut store,
            [id; 32],
            SubmissionState::Unknown,
            "transport outcome indeterminate",
            None,
        )
        .unwrap_or_else(|error| panic!("unknown: {error:?}"));
    (store, outbox, exact)
}

fn receipt(activity_id: [u8; 32], result_code: i32) -> RawReceiptEvidence {
    support::raw_receipt(activity_id, result_code, 25)
}

fn activity_id(outbox: &Outbox, id: u8) -> [u8; 32] {
    outbox
        .status([id; 32])
        .map(|status| status.activity_id)
        .unwrap_or_else(|| panic!("outbox activity missing"))
}

#[test]
fn acknowledgement_loss_is_resolved_only_by_the_existing_receipt() {
    let root = directory("ack-loss");
    let (mut store, mut outbox, _) = unknown_outbox(&root, 1);
    let mut node = FaultInjectedNode::default();
    node.receipts.insert([1; 32], receipt(activity_id(&outbox, 1), 0));

    let result = resolve_unknown(&mut outbox, &mut store, [1; 32], 10_000, &mut node)
        .unwrap_or_else(|error| panic!("resolve: {error:?}"));
    assert_eq!(result.state, SubmissionState::Executed);
    assert_eq!(result.observation, ResolutionObservation::ExecutedReceipt);
    assert_eq!(result.resend, ResendObservation::NotWarranted);
    assert!(node.transmitted.is_empty());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn lost_resend_response_keeps_budget_and_ceiling_held_and_reuses_exact_bytes() {
    let root = directory("response-loss");
    let (mut store, mut outbox, exact) = unknown_outbox(&root, 2);
    let limit_id = LimitId([7; 16]);
    let limiter = BudgetLimiter::new(vec![LimitConfig {
        id: limit_id,
        name: "tenant spend".to_owned(),
        scope: LimitScope::Tenant([8; 32]),
        ceiling: 1_000,
        consumed: 0,
    }])
    .unwrap_or_else(|error| panic!("limiter: {error:?}"));
    reserve(
        &limiter,
        &ReservationRequest {
            id: [2; 32],
            amount: 400,
            expiry_sequence: 5,
            current_sequence: 1,
            applicable_limits: vec![limit_id],
        },
    )
    .unwrap_or_else(|error| panic!("budget reserve: {error:?}"));
    let ceiling = Ceiling::new(1_000);
    consume(&ceiling, [2; 32], 400, 5, 1)
        .unwrap_or_else(|error| panic!("ceiling reserve: {error:?}"));
    ceiling
        .mark_unknown([2; 32])
        .unwrap_or_else(|error| panic!("ceiling unknown: {error:?}"));

    let late_receipt = receipt(activity_id(&outbox, 2), 0);
    let mut node = FaultInjectedNode {
        lose_resend_response: true,
        receipt_on_resend: Some(late_receipt),
        ..FaultInjectedNode::default()
    };
    let first = resolve_unknown(&mut outbox, &mut store, [2; 32], 20_000, &mut node)
        .unwrap_or_else(|error| panic!("first resolve: {error:?}"));
    assert_eq!(first.state, SubmissionState::Unknown);
    assert_eq!(first.observation, ResolutionObservation::ReceiptMissing);
    assert_eq!(first.resend, ResendObservation::Indeterminate);
    assert_eq!(node.transmitted, vec![([2; 32], exact)]);
    assert_eq!(limiter.held_reservations(), Ok(1));
    assert_eq!(ceiling.release_expired(50), Ok(0));
    assert_eq!(ceiling.snapshot().map(|value| value.held), Ok(400));

    let second = resolve_unknown(
        &mut outbox,
        &mut store,
        [2; 32],
        first.age.next_attempt_at_ms,
        &mut node,
    )
    .unwrap_or_else(|error| panic!("second resolve: {error:?}"));
    assert_eq!(second.state, SubmissionState::Executed);
    assert_eq!(second.age.attempt_count, 2);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn restart_preserves_backoff_age_and_a_receipt_can_appear_minutes_later() {
    let root = directory("restart");
    let (mut store, mut outbox, exact) = unknown_outbox(&root, 3);
    let mut node = FaultInjectedNode::default();
    let first = resolve_unknown(&mut outbox, &mut store, [3; 32], 30_000, &mut node)
        .unwrap_or_else(|error| panic!("first resolve: {error:?}"));
    assert_eq!(first.age.attempt_count, 1);
    assert_eq!(node.transmitted, vec![([3; 32], exact)]);
    drop(outbox);
    drop(store);

    let mut reopened = Store::open(&root).unwrap_or_else(|error| panic!("reopen: {error}"));
    let mut restored = Outbox::default();
    restored
        .restore(&reopened, tenant(), [3; 32])
        .unwrap_or_else(|error| panic!("restore: {error:?}"));
    let deferred = resolve_unknown(
        &mut restored,
        &mut reopened,
        [3; 32],
        first.age.next_attempt_at_ms - 1,
        &mut node,
    )
    .unwrap_or_else(|error| panic!("deferred: {error:?}"));
    assert_eq!(deferred.observation, ResolutionObservation::Backoff);
    assert_eq!(deferred.age.attempt_count, 1);

    node.receipts
        .insert([3; 32], receipt(activity_id(&restored, 3), 5));
    let later = resolve_unknown(&mut restored, &mut reopened, [3; 32], 210_000, &mut node)
        .unwrap_or_else(|error| panic!("later: {error:?}"));
    assert_eq!(later.state, SubmissionState::Failed);
    assert_eq!(later.age.age_ms, 180_000);
    assert_eq!(later.age.attempt_count, 2);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn lookup_failure_and_unverified_receipt_never_infer_a_terminal_outcome() {
    let root = directory("lookup-loss");
    let (mut store, mut outbox, _) = unknown_outbox(&root, 4);
    let mut node = FaultInjectedNode {
        lookup_unavailable: true,
        ..FaultInjectedNode::default()
    };
    let first = resolve_unknown(&mut outbox, &mut store, [4; 32], 40_000, &mut node)
        .unwrap_or_else(|error| panic!("lookup loss: {error:?}"));
    assert_eq!(first.state, SubmissionState::Unknown);
    assert_eq!(first.observation, ResolutionObservation::LookupUnavailable);
    assert!(node.transmitted.is_empty());

    node.lookup_unavailable = false;
    let raw = support::raw_receipt(activity_id(&outbox, 4), 0, 25);
    let mut corrupt = raw.canonical_receipt().to_vec();
    corrupt[0] ^= 1;
    node.receipts
        .insert([4; 32], support::corrupt_raw_receipt(&raw, corrupt));
    let second = resolve_unknown(
        &mut outbox,
        &mut store,
        [4; 32],
        first.age.next_attempt_at_ms,
        &mut node,
    )
    .unwrap_or_else(|error| panic!("unverified: {error:?}"));
    assert_eq!(second.state, SubmissionState::Unknown);
    assert_eq!(second.observation, ResolutionObservation::UnverifiedReceipt);
    let _ = std::fs::remove_dir_all(root);
}
mod support;

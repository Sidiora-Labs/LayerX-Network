use std::future::Future;
use std::pin::pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

use layerx_agentd::outbox::{Outbox, OutboxError, SubmissionState};
use layerx_agentd::protocol_evidence::VerifiedReceiptEvidence;
use layerx_agentd::prepare::{
    prepare_activity, CorePreparationBoundary, CorePreparationState, CoreStateError,
    PreparationDefaults, PrepareRequest,
};
use layerx_agentd::sign::{attach_external_signature, verify_before_submit, VerifiedSubmission};
use layerx_agentd::store::{ObjectKind, Store, TenantId, TenantKey};
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
    encoder
        .fixed(&[0x11; 32])
        .unwrap_or_else(|error| panic!("from: {error:?}"));
    encoder
        .fixed(&[0x22; 32])
        .unwrap_or_else(|error| panic!("to: {error:?}"));
    encoder
        .fixed(&[0x33; 32])
        .unwrap_or_else(|error| panic!("asset: {error:?}"));
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
        .unwrap_or_else(|error| panic!("authority kind: {error:?}"));
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
            actor: Did::new(b"did:layerx:outbox").unwrap_or_else(|error| panic!("DID: {error:?}")),
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

fn directory() -> std::path::PathBuf {
    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("layerx-outbox-{}-{sequence}", std::process::id()))
}

fn tenant() -> TenantId {
    TenantId::new("tenant-a").unwrap_or_else(|error| panic!("tenant: {error}"))
}

fn enqueue(outbox: &mut Outbox, store: &mut Store, id: u8, verified: &VerifiedSubmission) {
    outbox
        .enqueue(store, tenant(), [id; 32], verified.clone())
        .unwrap_or_else(|error| panic!("enqueue {id}: {error:?}"));
}

fn transition(
    outbox: &mut Outbox,
    store: &mut Store,
    id: u8,
    state: SubmissionState,
    receipt: Option<VerifiedReceiptEvidence>,
) {
    outbox
        .transition(store, [id; 32], state, format!("to {state:?}"), receipt)
        .unwrap_or_else(|error| panic!("transition {id} to {state:?}: {error:?}"));
}

#[test]
fn outbox_is_durable_before_transmission_and_unknown_is_a_real_state() {
    let root = directory();
    let verified = verified_submission(1);
    let expected_bytes = verified.exact_bytes().to_vec();
    let mut store = Store::open(&root).unwrap_or_else(|error| panic!("store: {error}"));
    let mut outbox = Outbox::default();
    enqueue(&mut outbox, &mut store, 1, &verified);
    assert_eq!(
        outbox
            .bytes_for_transmission([1; 32])
            .unwrap_or_else(|error| panic!("transmission bytes: {error:?}")),
        expected_bytes
    );
    drop(outbox);
    drop(store);

    let mut reopened = Store::open(&root).unwrap_or_else(|error| panic!("reopen: {error}"));
    let mut restored = Outbox::default();
    restored
        .restore(&reopened, tenant(), [1; 32])
        .unwrap_or_else(|error| panic!("restore: {error:?}"));
    assert_eq!(
        restored
            .bytes_for_transmission([1; 32])
            .unwrap_or_else(|error| panic!("restored bytes: {error:?}")),
        expected_bytes
    );
    transition(
        &mut restored,
        &mut reopened,
        1,
        SubmissionState::Submitted,
        None,
    );
    transition(
        &mut restored,
        &mut reopened,
        1,
        SubmissionState::Unknown,
        None,
    );
    let status = restored
        .status([1; 32])
        .unwrap_or_else(|| panic!("status missing"));
    assert_eq!(status.state, SubmissionState::Unknown);
    assert_eq!(status.transitions.len(), 4);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn every_legal_terminal_path_is_recorded_and_illegal_transitions_fail() {
    let root = directory();
    let mut store = Store::open(&root).unwrap_or_else(|error| panic!("store: {error}"));
    let mut outbox = Outbox::default();
    for id in 1_u8..=6 {
        let verified = verified_submission(id);
        enqueue(&mut outbox, &mut store, id, &verified);
    }
    transition(&mut outbox, &mut store, 1, SubmissionState::Submitted, None);
    transition(
        &mut outbox,
        &mut store,
        1,
        SubmissionState::Acknowledged,
        None,
    );
    transition(&mut outbox, &mut store, 1, SubmissionState::Unknown, None);
    let activity_id = outbox
        .status([1; 32])
        .map(|status| status.activity_id)
        .unwrap_or_else(|| panic!("activity missing"));
    let receipt = support::evidence_verifier()
        .verify_receipt(&support::raw_receipt(activity_id, 0, 1))
        .unwrap_or_else(|error| panic!("receipt verification: {error:?}"));
    assert_eq!(
        receipt.level(),
        layerx_types::verify::VerificationLevel::BATCH_INCLUDED
    );
    transition(
        &mut outbox,
        &mut store,
        1,
        SubmissionState::Executed,
        Some(receipt),
    );
    transition(&mut outbox, &mut store, 2, SubmissionState::Expired, None);
    transition(
        &mut outbox,
        &mut store,
        3,
        SubmissionState::Superseded,
        None,
    );
    transition(&mut outbox, &mut store, 4, SubmissionState::Submitted, None);
    transition(&mut outbox, &mut store, 4, SubmissionState::Unknown, None);
    transition(
        &mut outbox,
        &mut store,
        4,
        SubmissionState::Failed,
        Some(receipt),
    );
    transition(&mut outbox, &mut store, 5, SubmissionState::Submitted, None);
    transition(
        &mut outbox,
        &mut store,
        5,
        SubmissionState::Acknowledged,
        None,
    );
    transition(
        &mut outbox,
        &mut store,
        5,
        SubmissionState::Executed,
        Some(receipt),
    );
    transition(&mut outbox, &mut store, 6, SubmissionState::Submitted, None);
    transition(
        &mut outbox,
        &mut store,
        6,
        SubmissionState::Acknowledged,
        None,
    );
    transition(
        &mut outbox,
        &mut store,
        6,
        SubmissionState::Failed,
        Some(receipt),
    );

    for id in 1_u8..=6 {
        assert!(outbox
            .status([id; 32])
            .is_some_and(|status| status.state.terminal()));
    }
    assert!(matches!(
        outbox.transition(
            &mut store,
            [1; 32],
            SubmissionState::Unknown,
            "illegal",
            None,
        ),
        Err(OutboxError::InvalidTransition { .. })
    ));
    assert!(matches!(
        outbox.transition(
            &mut store,
            [2; 32],
            SubmissionState::Submitted,
            "illegal",
            None,
        ),
        Err(OutboxError::InvalidTransition { .. })
    ));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn executed_is_impossible_without_a_verified_receipt_reference() {
    let root = directory();
    let verified = verified_submission(1);
    let mut store = Store::open(&root).unwrap_or_else(|error| panic!("store: {error}"));
    let mut outbox = Outbox::default();
    enqueue(&mut outbox, &mut store, 1, &verified);
    transition(&mut outbox, &mut store, 1, SubmissionState::Submitted, None);
    transition(
        &mut outbox,
        &mut store,
        1,
        SubmissionState::Acknowledged,
        None,
    );
    assert!(matches!(
        outbox.transition(
            &mut store,
            [1; 32],
            SubmissionState::Executed,
            "missing receipt",
            None,
        ),
        Err(OutboxError::SuccessWithoutVerifiedReceipt)
    ));
    let wrong_activity = support::evidence_verifier()
        .verify_receipt(&support::raw_receipt([8; 32], 0, 1))
        .unwrap_or_else(|error| panic!("receipt verification: {error:?}"));
    assert_eq!(
        outbox.transition(
            &mut store,
            [1; 32],
            SubmissionState::Executed,
            "receipt for another activity",
            Some(wrong_activity),
        ),
        Err(OutboxError::ReceiptMismatch)
    );
    let raw = support::raw_receipt([8; 32], 0, 1);
    let mut corrupt = raw.canonical_receipt().to_vec();
    corrupt[0] ^= 1;
    assert!(support::evidence_verifier()
        .verify_receipt(&support::corrupt_raw_receipt(&raw, corrupt))
        .is_err());
    assert_eq!(
        outbox.status([1; 32]).map(|status| status.state),
        Some(SubmissionState::Acknowledged)
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn legacy_boolean_receipt_records_fail_closed() {
    let root = directory();
    let verified = verified_submission(1);
    let mut store = Store::open(&root).unwrap_or_else(|error| panic!("store: {error}"));
    let mut outbox = Outbox::default();
    enqueue(&mut outbox, &mut store, 1, &verified);
    let key = TenantKey::new(tenant(), ObjectKind::Outbox, [1; 32].to_vec())
        .unwrap_or_else(|error| panic!("outbox key: {error}"));
    let mut legacy = store
        .get(&key)
        .map(|value| value.bytes().to_vec())
        .unwrap_or_else(|| panic!("outbox record missing"));
    legacy[4] = 1;
    store
        .put_local(key, legacy)
        .unwrap_or_else(|error| panic!("legacy record: {error}"));
    let mut restored = Outbox::default();
    assert_eq!(
        restored.restore(&store, tenant(), [1; 32]),
        Err(OutboxError::Corrupt)
    );
    let _ = std::fs::remove_dir_all(root);
}
mod support;

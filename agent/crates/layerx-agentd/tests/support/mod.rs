use std::future::Future;
use std::pin::pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

use layerx_agentd::prepare::{
    prepare_activity, CorePreparationBoundary, CorePreparationState, CoreStateError,
    PreparationDefaults, PrepareRequest,
};
use layerx_agentd::sign::{attach_external_signature, verify_before_submit, VerifiedSubmission};
use layerx_agentd::store::TenantId;
use layerx_crypto::local::LocalSigner;
use layerx_crypto::signer::{sign_disclosed, Signer};
use layerx_types::activity::{Authority, TimestampBound};
use layerx_types::amount::Amount;
use layerx_types::ids::{Did, IdempotencyKey};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_wire::encode::Encoder;
use layerx_agentd::protocol_evidence::{RawReceiptEvidence, RawStateEvidence};
use layerx_proof::inclusion::SequencerAuthorization;
use layerx_proof::merkle::build_proof;
use layerx_wire::hash::{batch_header_digest, receipt_digest};
use ed25519_dalek::{Signer as _, SigningKey};

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

pub fn verified_submission(id: u8) -> VerifiedSubmission {
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
            actor: Did::new(b"did:layerx:recovery")
                .unwrap_or_else(|error| panic!("DID: {error:?}")),
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

pub fn tenant() -> TenantId {
    TenantId::new("tenant-a").unwrap_or_else(|error| panic!("tenant: {error}"))
}

pub fn directory(label: &str) -> std::path::PathBuf {
    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "layerx-recovery-{label}-{}-{sequence}",
        std::process::id()
    ))
}

pub fn raw_receipt(activity_id: [u8; 32], result_code: i32, amount: u128) -> RawReceiptEvidence {
    raw_receipt_at(activity_id, result_code, amount, 9)
}

pub fn raw_receipt_at(
    activity_id: [u8; 32],
    result_code: i32,
    amount: u128,
    global_sequence: u64,
) -> RawReceiptEvidence {
    let key = SigningKey::from_bytes(&[0x3a; 32]);
    let previous_state_root = [0x21; 32];
    let resulting_state_root = [0x22; 32];
    let batch_id = [0x23; 32];
    let asset = [0x24; 32];
    let encode = |signature: Option<[u8; 64]>| {
        let mut encoder = Encoder::new(4096);
        assert_eq!(encoder.structure_header(0x5201), Ok(()));
        assert_eq!(encoder.u16(1), Ok(()));
        assert_eq!(encoder.bytes(&activity_id, 32), Ok(()));
        assert_eq!(encoder.u64(global_sequence), Ok(()));
        assert_eq!(encoder.bytes(&previous_state_root, 32), Ok(()));
        assert_eq!(encoder.bytes(&resulting_state_root, 32), Ok(()));
        assert_eq!(encoder.bytes(&[0x25; 32], 32), Ok(()));
        assert_eq!(encoder.i32(result_code), Ok(()));
        assert_eq!(encoder.sequence_length(0, 512), Ok(()));
        assert_eq!(encoder.u128(1), Ok(()));
        assert_eq!(encoder.bytes(&batch_id, 32), Ok(()));
        assert_eq!(encoder.u16(1), Ok(()));
        assert_eq!(encoder.u32(1), Ok(()));
        assert_eq!(encoder.u32(1), Ok(()));
        assert_eq!(encoder.u8(1), Ok(()));
        assert_eq!(encoder.bytes(&asset, 32), Ok(()));
        assert_eq!(encoder.u128(amount), Ok(()));
        assert_eq!(encoder.bytes(&[0x26; 32], 32), Ok(()));
        assert_eq!(encoder.u128(1_000 + amount), Ok(()));
        assert_eq!(encoder.u128(if result_code == 0 { 1_000 } else { 1_000 + amount }), Ok(()));
        assert_eq!(encoder.u64(1), Ok(()));
        assert_eq!(encoder.bytes(&[0x27; 32], 32), Ok(()));
        assert_eq!(encoder.u128(10), Ok(()));
        assert_eq!(encoder.u128(if result_code == 0 { 10 + amount } else { 10 }), Ok(()));
        for value in [[0x28; 32], [0x29; 32], [0x2a; 32]] {
            assert_eq!(encoder.bytes(&value, 32), Ok(()));
        }
        assert_eq!(encoder.u64(1_000), Ok(()));
        assert_eq!(encoder.u8(u8::from(signature.is_some())), Ok(()));
        if let Some(signature) = signature {
            assert_eq!(encoder.bytes(&signature, 64), Ok(()));
        }
        encoder.finish()
    };
    let unsigned = encode(None);
    let digest = receipt_digest(&unsigned)
        .unwrap_or_else(|error| panic!("receipt digest: {error:?}"));
    let canonical_receipt = encode(Some(key.sign(&digest).to_bytes()));
    let leaves = [canonical_receipt.as_slice()];
    let (proof, receipt_root) = build_proof(&leaves, 0)
        .unwrap_or_else(|error| panic!("receipt proof: {error:?}"));
    let sequencer_id = key.verifying_key().to_bytes();
    let mut encoder = Encoder::new(354);
    assert_eq!(encoder.structure_header(0x1701), Ok(()));
    assert_eq!(encoder.u8(15), Ok(()));
    let fields: [(u8, Vec<u8>); 15] = [
        (1, 1_u16.to_be_bytes().to_vec()),
        (2, 42_u32.to_be_bytes().to_vec()),
        (3, 2_u64.to_be_bytes().to_vec()),
        (4, 7_u64.to_be_bytes().to_vec()),
        (5, global_sequence.to_be_bytes().to_vec()),
        (6, global_sequence.to_be_bytes().to_vec()),
        (7, previous_state_root.to_vec()),
        (8, resulting_state_root.to_vec()),
        (9, [0x25; 32].to_vec()),
        (10, receipt_root.to_vec()),
        (11, [0x34; 32].to_vec()),
        (12, [0x35; 32].to_vec()),
        (13, [0x36; 32].to_vec()),
        (14, 1_000_u64.to_be_bytes().to_vec()),
        (15, sequencer_id.to_vec()),
    ];
    for (field, value) in fields {
        assert_eq!(encoder.tag(field, 15), Ok(()));
        match field {
            1 => assert_eq!(
                encoder.u16(u16::from_be_bytes([value[0], value[1]])),
                Ok(())
            ),
            2 => assert_eq!(
                encoder.u32(u32::from_be_bytes(
                    value
                        .as_slice()
                        .try_into()
                        .unwrap_or_else(|_| panic!("u32 field"))
                )),
                Ok(())
            ),
            3..=6 | 14 => assert_eq!(
                encoder.u64(u64::from_be_bytes(
                    value
                        .as_slice()
                        .try_into()
                        .unwrap_or_else(|_| panic!("u64 field"))
                )),
                Ok(())
            ),
            _ => assert_eq!(encoder.bytes(&value, 32), Ok(())),
        }
    }
    let header = encoder.finish();
    let header_digest = batch_header_digest(&header)
        .unwrap_or_else(|error| panic!("header digest: {error:?}"));
    RawReceiptEvidence::new(
        canonical_receipt,
        proof,
        header,
        key.sign(&header_digest).to_bytes(),
        SequencerAuthorization::new(sequencer_id, sequencer_id, 7, 7),
    )
}

pub fn corrupt_raw_receipt(
    raw: &RawReceiptEvidence,
    canonical_receipt: Vec<u8>,
) -> RawReceiptEvidence {
    RawReceiptEvidence::new(
        canonical_receipt,
        raw.proof().clone(),
        raw.canonical_header().to_vec(),
        raw.header_signature(),
        raw.authorization(),
    )
}

pub fn raw_budget_state(
    consumed: u128,
    remaining: u128,
    window_start: u64,
    window_end: u64,
    observed_head: u64,
) -> RawStateEvidence {
    let mut state = Vec::with_capacity(52);
    state.extend_from_slice(b"LXBS");
    state.extend_from_slice(&consumed.to_be_bytes());
    state.extend_from_slice(&remaining.to_be_bytes());
    state.extend_from_slice(&window_start.to_be_bytes());
    state.extend_from_slice(&window_end.to_be_bytes());
    let leaves = [state.as_slice()];
    let (proof, root) = build_proof(&leaves, 0)
        .unwrap_or_else(|error| panic!("state proof: {error:?}"));
    let key = SigningKey::from_bytes(&[0x4a; 32]);
    let sequencer_id = key.verifying_key().to_bytes();
    let mut encoder = Encoder::new(354);
    assert_eq!(encoder.structure_header(0x1701), Ok(()));
    assert_eq!(encoder.u8(15), Ok(()));
    let fields: [(u8, Vec<u8>); 15] = [
        (1, 1_u16.to_be_bytes().to_vec()),
        (2, 42_u32.to_be_bytes().to_vec()),
        (3, 2_u64.to_be_bytes().to_vec()),
        (4, 7_u64.to_be_bytes().to_vec()),
        (5, 1_u64.to_be_bytes().to_vec()),
        (6, observed_head.to_be_bytes().to_vec()),
        (7, [0x31; 32].to_vec()),
        (8, root.to_vec()),
        (9, [0x32; 32].to_vec()),
        (10, [0x33; 32].to_vec()),
        (11, [0x34; 32].to_vec()),
        (12, [0x35; 32].to_vec()),
        (13, [0x36; 32].to_vec()),
        (14, 1_000_u64.to_be_bytes().to_vec()),
        (15, sequencer_id.to_vec()),
    ];
    for (field, value) in fields {
        assert_eq!(encoder.tag(field, 15), Ok(()));
        match field {
            1 => assert_eq!(encoder.u16(u16::from_be_bytes([value[0], value[1]])), Ok(())),
            2 => assert_eq!(encoder.u32(u32::from_be_bytes(value.as_slice().try_into().unwrap_or_else(|_| panic!("u32 field")))), Ok(())),
            3..=6 | 14 => assert_eq!(encoder.u64(u64::from_be_bytes(value.as_slice().try_into().unwrap_or_else(|_| panic!("u64 field")))), Ok(())),
            _ => assert_eq!(encoder.bytes(&value, 32), Ok(())),
        }
    }
    let header = encoder.finish();
    let digest = batch_header_digest(&header)
        .unwrap_or_else(|error| panic!("header digest: {error:?}"));
    RawStateEvidence::new(
        state,
        proof,
        root,
        header,
        key.sign(&digest).to_bytes(),
        SequencerAuthorization::new(sequencer_id, sequencer_id, 7, 7),
    )
}

pub fn corrupt_raw_state(
    raw: &RawStateEvidence,
    canonical_state: Vec<u8>,
) -> RawStateEvidence {
    RawStateEvidence::new(
        canonical_state,
        raw.proof().clone(),
        raw.resulting_state_root(),
        raw.canonical_header().to_vec(),
        raw.header_signature(),
        raw.authorization(),
    )
}

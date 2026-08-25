use std::sync::atomic::{AtomicU64, Ordering};

use ed25519_dalek::{Signer as _, SigningKey as EdSigningKey};
use k256::ecdsa::{signature::hazmat::PrehashSigner as _, Signature, SigningKey};
use layerx_agentd::finality::{
    augment, wait_for_level, CheckpointBundle, FinalityError, InclusionBundle, VerificationProgress,
};
use layerx_agentd::receipt::{serve, store, ReceiptLookupKey};
use layerx_agentd::store::{Store, TenantId};
use layerx_proof::checkpoint::{
    checkpoint_id, Attestation, Certificate, Checkpoint, CheckpointError, GuarantorKey,
};
use layerx_proof::inclusion::SequencerAuthorization;
use layerx_proof::merkle::{build_proof, Proof};
use layerx_proof::receipt::AuthorizedBatch;
use layerx_types::verify::VerificationLevel;
use layerx_wire::encode::Encoder;
use layerx_wire::hash::{batch_header_digest, checkpoint_attestation_digest, receipt_digest};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

fn tenant() -> TenantId {
    TenantId::new("tenant-a").unwrap_or_else(|error| panic!("tenant: {error}"))
}

fn directory(label: &str) -> std::path::PathBuf {
    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "layerx-finality-{label}-{}-{sequence}",
        std::process::id()
    ))
}

fn receipt_bytes(signature: Option<[u8; 64]>) -> Vec<u8> {
    let mut encoder = Encoder::new(4096);
    assert_eq!(encoder.structure_header(0x5201), Ok(()));
    assert_eq!(encoder.u16(1), Ok(()));
    assert_eq!(encoder.bytes(&[1; 32], 32), Ok(()));
    assert_eq!(encoder.u64(9), Ok(()));
    assert_eq!(encoder.bytes(&[2; 32], 32), Ok(()));
    assert_eq!(encoder.bytes(&[3; 32], 32), Ok(()));
    assert_eq!(encoder.bytes(&[8; 32], 32), Ok(()));
    assert_eq!(encoder.i32(0), Ok(()));
    assert_eq!(encoder.sequence_length(0, 512), Ok(()));
    assert_eq!(encoder.u128(1), Ok(()));
    assert_eq!(encoder.bytes(&[4; 32], 32), Ok(()));
    assert_eq!(encoder.u16(1), Ok(()));
    assert_eq!(encoder.u32(1), Ok(()));
    assert_eq!(encoder.u32(1), Ok(()));
    assert_eq!(encoder.u8(1), Ok(()));
    assert_eq!(encoder.bytes(&[5; 32], 32), Ok(()));
    assert_eq!(encoder.u128(25), Ok(()));
    assert_eq!(encoder.bytes(&[6; 32], 32), Ok(()));
    assert_eq!(encoder.u128(100), Ok(()));
    assert_eq!(encoder.u128(75), Ok(()));
    assert_eq!(encoder.u64(1), Ok(()));
    assert_eq!(encoder.bytes(&[7; 32], 32), Ok(()));
    assert_eq!(encoder.u128(10), Ok(()));
    assert_eq!(encoder.u128(35), Ok(()));
    assert_eq!(encoder.bytes(&[9; 32], 32), Ok(()));
    assert_eq!(encoder.bytes(&[10; 32], 32), Ok(()));
    assert_eq!(encoder.bytes(&[11; 32], 32), Ok(()));
    assert_eq!(encoder.u64(1_000), Ok(()));
    assert_eq!(encoder.u8(u8::from(signature.is_some())), Ok(()));
    if let Some(value) = signature {
        assert_eq!(encoder.bytes(&value, 64), Ok(()));
    }
    encoder.finish()
}

fn persist_receipt(durable: &mut Store, idempotency_key: [u8; 32]) -> Vec<u8> {
    let key = EdSigningKey::from_bytes(&[3; 32]);
    let unsigned = receipt_bytes(None);
    let digest =
        receipt_digest(&unsigned).unwrap_or_else(|error| panic!("receipt digest: {error:?}"));
    let exact = receipt_bytes(Some(key.sign(&digest).to_bytes()));
    store(
        durable,
        tenant(),
        idempotency_key,
        &exact,
        &AuthorizedBatch::new(
            [4; 32],
            [5; 32],
            [2; 32],
            [3; 32],
            key.verifying_key().to_bytes(),
        ),
    )
    .unwrap_or_else(|error| panic!("store receipt: {error:?}"));
    exact
}

fn header_bytes(state_root: [u8; 32], activity_root: [u8; 32], sequencer_id: [u8; 32]) -> Vec<u8> {
    let mut encoder = Encoder::new(354);
    assert_eq!(encoder.structure_header(0x1701), Ok(()));
    assert_eq!(encoder.u8(15), Ok(()));
    for field in 1..=15 {
        assert_eq!(encoder.tag(field, 15), Ok(()));
        match field {
            1 => assert_eq!(encoder.u16(1), Ok(())),
            2 => assert_eq!(encoder.u32(42), Ok(())),
            3 => assert_eq!(encoder.u64(2), Ok(())),
            4 => assert_eq!(encoder.u64(8), Ok(())),
            5 => assert_eq!(encoder.u64(9), Ok(())),
            6 => assert_eq!(encoder.u64(10), Ok(())),
            7 => assert_eq!(encoder.bytes(&[1; 32], 32), Ok(())),
            8 => assert_eq!(encoder.bytes(&state_root, 32), Ok(())),
            9 => assert_eq!(encoder.bytes(&activity_root, 32), Ok(())),
            10 => assert_eq!(encoder.bytes(&[2; 32], 32), Ok(())),
            11 => assert_eq!(encoder.bytes(&[3; 32], 32), Ok(())),
            12 => assert_eq!(encoder.bytes(&[12; 32], 32), Ok(())),
            13 => assert_eq!(encoder.bytes(&[5; 32], 32), Ok(())),
            14 => assert_eq!(encoder.u64(1_000), Ok(())),
            15 => assert_eq!(encoder.bytes(&sequencer_id, 32), Ok(())),
            _ => panic!("unreachable header field"),
        }
    }
    encoder.finish()
}

struct InclusionFixture {
    activity: Vec<u8>,
    activity_proof: Proof,
    state: Vec<u8>,
    state_proof: Proof,
    state_root: [u8; 32],
    header: Vec<u8>,
    header_signature: [u8; 64],
    authorization: SequencerAuthorization,
}

fn inclusion_fixture() -> InclusionFixture {
    let activity = b"canonical-activity".to_vec();
    let state = b"canonical-state-leaf".to_vec();
    let (activity_proof, activity_root) = build_proof(&[activity.as_slice()], 0)
        .unwrap_or_else(|error| panic!("activity proof: {error:?}"));
    let (state_proof, state_root) = build_proof(&[state.as_slice()], 0)
        .unwrap_or_else(|error| panic!("state proof: {error:?}"));
    let key = EdSigningKey::from_bytes(&[7; 32]);
    let sequencer_id = key.verifying_key().to_bytes();
    let header = header_bytes(state_root, activity_root, sequencer_id);
    let digest =
        batch_header_digest(&header).unwrap_or_else(|error| panic!("header digest: {error:?}"));
    InclusionFixture {
        activity,
        activity_proof,
        state,
        state_proof,
        state_root,
        header,
        header_signature: key.sign(&digest).to_bytes(),
        authorization: SequencerAuthorization::new(sequencer_id, sequencer_id, 8, 8),
    }
}

fn guarantor_key(value: u8) -> (SigningKey, [u8; 33], [u8; 32]) {
    let mut scalar = [0_u8; 32];
    scalar[31] = value;
    let signing = SigningKey::from_bytes((&scalar).into())
        .unwrap_or_else(|error| panic!("guarantor key: {error}"));
    let encoded = signing.verifying_key().to_encoded_point(true);
    let public: [u8; 33] = encoded
        .as_bytes()
        .try_into()
        .unwrap_or_else(|_| panic!("public key width"));
    let mut identifier = [0_u8; 32];
    identifier[0] = value;
    (signing, public, identifier)
}

fn attestation(checkpoint: [u8; 32], guarantor_id: [u8; 32], key: &SigningKey) -> Attestation {
    let settlement_contract = [0x55; 20];
    let mut message = [0_u8; 189];
    message[..2].copy_from_slice(&1_u16.to_be_bytes());
    message[2..6].copy_from_slice(&42_u32.to_be_bytes());
    message[6..14].copy_from_slice(&31_337_u64.to_be_bytes());
    message[14..34].copy_from_slice(&settlement_contract);
    message[34..42].copy_from_slice(&7_u64.to_be_bytes());
    message[42..74].copy_from_slice(&checkpoint);
    message[74..106].copy_from_slice(&checkpoint);
    message[106..138].copy_from_slice(&guarantor_id);
    message[138..146].copy_from_slice(&8_u64.to_be_bytes());
    message[146..178].copy_from_slice(&[12; 32]);
    message[178] = 1;
    message[179] = 1;
    message[180] = 0x1f;
    message[181..].copy_from_slice(&(1_000 + u64::from(guarantor_id[0])).to_be_bytes());
    let digest = checkpoint_attestation_digest(&message)
        .unwrap_or_else(|error| panic!("attestation digest: {error:?}"));
    let signature: Signature = key
        .sign_prehash(&digest)
        .unwrap_or_else(|error| panic!("attestation signature: {error}"));
    Attestation::new(
        1,
        42,
        31_337,
        settlement_contract,
        7,
        checkpoint,
        checkpoint,
        guarantor_id,
        8,
        [12; 32],
        true,
        true,
        0x1f,
        1_000 + u64::from(guarantor_id[0]),
        signature.to_bytes().into(),
    )
}

fn certificate_fixture(
    header: &[u8],
    signer_count: u8,
    threshold: usize,
    settlement: Option<Vec<u8>>,
) -> (Certificate, Vec<GuarantorKey>, [u8; 32]) {
    let checkpoint = Checkpoint::new(header.to_vec(), b"PROOF".to_vec());
    let identifier = checkpoint_id(&checkpoint)
        .unwrap_or_else(|error| panic!("checkpoint identifier: {error:?}"));
    let mut attestations = Vec::new();
    let mut keys = Vec::new();
    for value in 1..=signer_count {
        let (signing, public, guarantor_id) = guarantor_key(value);
        attestations.push(attestation(identifier, guarantor_id, &signing));
        keys.push(GuarantorKey::new(guarantor_id, public, true));
    }
    (
        Certificate::new(checkpoint, attestations, threshold, settlement),
        keys,
        identifier,
    )
}

fn inclusion_bundle(fixture: &InclusionFixture) -> InclusionBundle<'_> {
    InclusionBundle {
        activity_bytes: &fixture.activity,
        activity_proof: &fixture.activity_proof,
        state_leaf_bytes: &fixture.state,
        state_proof: &fixture.state_proof,
        named_resulting_state_root: fixture.state_root,
        header_bytes: &fixture.header,
        header_signature: fixture.header_signature,
        authorization: &fixture.authorization,
    }
}

#[test]
fn verified_checkpoint_and_settlement_raise_level_without_altering_receipt() {
    let root = directory("anchored");
    let idempotency_key = [0x41; 32];
    let mut durable = Store::open(&root).unwrap_or_else(|error| panic!("store: {error}"));
    let exact_receipt = persist_receipt(&mut durable, idempotency_key);
    let inclusion = inclusion_fixture();
    let (certificate, bonded, checkpoint_id) = certificate_fixture(
        &inclusion.header,
        3,
        2,
        Some(b"paxeer-settlement-42".to_vec()),
    );
    let checkpoint = CheckpointBundle {
        certificate: &certificate,
        bonded_set: &bonded,
        registered_checkpoint_id: checkpoint_id,
        registered_settlement_reference: Some(b"paxeer-settlement-42"),
    };
    let record = augment(
        &mut durable,
        tenant(),
        idempotency_key,
        &inclusion_bundle(&inclusion),
        Some(&checkpoint),
    )
    .unwrap_or_else(|error| panic!("augment: {error:?}"));
    assert_eq!(
        record.verification_level,
        VerificationLevel::SETTLEMENT_ANCHORED
    );
    assert_eq!(record.checkpoint_id, Some(checkpoint_id));
    assert_eq!(record.guarantor_signatures_achieved, Some(3));
    assert_eq!(record.guarantor_threshold, Some(2));
    assert!(!record.activity_proof.is_empty());
    assert!(!record.state_proof.is_empty());
    let served = serve(
        &durable,
        tenant(),
        ReceiptLookupKey::Idempotency(idempotency_key),
    )
    .unwrap_or_else(|error| panic!("serve: {error:?}"));
    assert_eq!(served.canonical_bytes, exact_receipt);
    assert_eq!(
        served.metadata.verification_level,
        VerificationLevel::SETTLEMENT_ANCHORED
    );
    let _ = std::fs::remove_dir_all(root);
}

struct NeverFinalises;

impl VerificationProgress for NeverFinalises {
    fn level_at(
        &mut self,
        _idempotency_key: [u8; 32],
        _observed_at_ms: u64,
    ) -> Result<VerificationLevel, FinalityError> {
        Ok(VerificationLevel::STATE_PROVEN)
    }
}

#[test]
fn missing_checkpoint_returns_actual_level_at_the_explicit_deadline() {
    let root = directory("pending");
    let idempotency_key = [0x42; 32];
    let mut durable = Store::open(&root).unwrap_or_else(|error| panic!("store: {error}"));
    let _ = persist_receipt(&mut durable, idempotency_key);
    let inclusion = inclusion_fixture();
    let record = augment(
        &mut durable,
        tenant(),
        idempotency_key,
        &inclusion_bundle(&inclusion),
        None,
    )
    .unwrap_or_else(|error| panic!("inclusion augment: {error:?}"));
    assert_eq!(record.verification_level, VerificationLevel::STATE_PROVEN);
    let waited = wait_for_level(
        &mut NeverFinalises,
        idempotency_key,
        VerificationLevel::CHECKPOINT_FINALISED,
        10_000,
        70_000,
        5_000,
    )
    .unwrap_or_else(|error| panic!("wait: {error:?}"));
    assert!(waited.deadline_elapsed);
    assert_eq!(waited.observed_at_ms, 70_000);
    assert_eq!(waited.reached, VerificationLevel::STATE_PROVEN);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn subthreshold_certificate_records_no_unearned_checkpoint_level() {
    let root = directory("threshold");
    let idempotency_key = [0x43; 32];
    let mut durable = Store::open(&root).unwrap_or_else(|error| panic!("store: {error}"));
    let exact = persist_receipt(&mut durable, idempotency_key);
    let inclusion = inclusion_fixture();
    let (certificate, bonded, checkpoint_id) = certificate_fixture(&inclusion.header, 1, 2, None);
    let checkpoint = CheckpointBundle {
        certificate: &certificate,
        bonded_set: &bonded,
        registered_checkpoint_id: checkpoint_id,
        registered_settlement_reference: None,
    };
    assert!(matches!(
        augment(
            &mut durable,
            tenant(),
            idempotency_key,
            &inclusion_bundle(&inclusion),
            Some(&checkpoint),
        ),
        Err(FinalityError::Checkpoint(CheckpointError::Threshold {
            achieved: 1,
            required: 2
        }))
    ));
    let served = serve(
        &durable,
        tenant(),
        ReceiptLookupKey::Idempotency(idempotency_key),
    )
    .unwrap_or_else(|error| panic!("serve: {error:?}"));
    assert_eq!(served.canonical_bytes, exact);
    assert_eq!(
        served.metadata.verification_level,
        VerificationLevel::SEQUENCER_SIGNED
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn mismatched_settlement_reference_is_rejected_without_level_change() {
    let root = directory("settlement");
    let idempotency_key = [0x44; 32];
    let mut durable = Store::open(&root).unwrap_or_else(|error| panic!("store: {error}"));
    let _ = persist_receipt(&mut durable, idempotency_key);
    let inclusion = inclusion_fixture();
    let (certificate, bonded, checkpoint_id) =
        certificate_fixture(&inclusion.header, 2, 2, Some(b"registered-one".to_vec()));
    let checkpoint = CheckpointBundle {
        certificate: &certificate,
        bonded_set: &bonded,
        registered_checkpoint_id: checkpoint_id,
        registered_settlement_reference: Some(b"different-reference"),
    };
    assert!(matches!(
        augment(
            &mut durable,
            tenant(),
            idempotency_key,
            &inclusion_bundle(&inclusion),
            Some(&checkpoint),
        ),
        Err(FinalityError::Checkpoint(CheckpointError::Settlement))
    ));
    let served = serve(
        &durable,
        tenant(),
        ReceiptLookupKey::Idempotency(idempotency_key),
    )
    .unwrap_or_else(|error| panic!("serve: {error:?}"));
    assert_eq!(
        served.metadata.verification_level,
        VerificationLevel::SEQUENCER_SIGNED
    );
    let _ = std::fs::remove_dir_all(root);
}

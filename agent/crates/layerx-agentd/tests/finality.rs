use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};

use ed25519_dalek::{Signer as _, SigningKey as EdSigningKey};
use k256::ecdsa::{Signature, SigningKey};
use layerx_agentd::finality::{
    augment, augment_verified, wait_for_level, FinalityError, InclusionBundle, VerificationProgress,
};
use layerx_agentd::receipt::{serve, store, store_verified_if_absent, ReceiptLookupKey};
use layerx_agentd::store::{Store, TenantId};
use layerx_client::evidence::{
    checkpoint, proof_bundle, CheckpointSelector, EvidenceContext, EvidenceError,
    FinalityEvidenceCandidate, ProofBundleSelector, VerifiedCheckpoint, VerifiedProofBundle,
};
use layerx_client::lni::schema::{encode_envelope, Envelope, Version};
use layerx_client::lni::transport::{FrameTransport, TransportError};
use layerx_crypto::{secp256k1, SignatureMessage};
use layerx_proof::checkpoint::{checkpoint_id, Checkpoint};
use layerx_proof::inclusion::SequencerAuthorization;
use layerx_proof::merkle::{build_proof, Proof};
use layerx_proof::receipt::AuthorizedBatch;
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_types::verify::VerificationLevel;
use layerx_wire::activity::decode_signed;
use layerx_wire::encode::Encoder;
use layerx_wire::hash::{
    activity_id, batch_header_digest, checkpoint_attestation_digest, receipt_digest, Domain,
};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);
const PROTOCOL_VERSION: u16 = 1;
const NETWORK_ID: u32 = 42;
const BATCH_NUMBER: u64 = 8;
const GLOBAL_SEQUENCE: u64 = 9;
const EPOCH: u64 = 2;
const PAXEER_CHAIN_ID: u64 = 31_337;
const SETTLEMENT_CONTRACT: [u8; 20] = [0x55; 20];

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

fn registry() -> ModuleRegistry {
    let activity = ActivityType::new(ModuleId::Asset, 1)
        .unwrap_or_else(|error| panic!("activity type: {error:?}"));
    let registration = ModuleRegistration::new(ModuleId::Asset, &[activity])
        .unwrap_or_else(|error| panic!("module registration: {error:?}"));
    ModuleRegistry::new(&[registration])
        .unwrap_or_else(|error| panic!("module registry: {error:?}"))
}

fn activity_fields(encoder: &mut Encoder, public_key: &[u8; 32]) {
    assert_eq!(encoder.tag(1, 12), Ok(()));
    assert_eq!(encoder.u16(PROTOCOL_VERSION), Ok(()));
    assert_eq!(encoder.tag(2, 12), Ok(()));
    assert_eq!(encoder.u32(NETWORK_ID), Ok(()));
    assert_eq!(encoder.tag(3, 12), Ok(()));
    assert_eq!(encoder.u32(0x0001_0001), Ok(()));
    assert_eq!(encoder.tag(4, 12), Ok(()));
    assert_eq!(encoder.bytes(b"did:layerx:finality", 255), Ok(()));
    assert_eq!(encoder.tag(5, 12), Ok(()));
    assert_eq!(encoder.bytes(public_key, 524_288), Ok(()));
    assert_eq!(encoder.tag(6, 12), Ok(()));
    assert_eq!(encoder.u64(GLOBAL_SEQUENCE), Ok(()));
    assert_eq!(encoder.tag(7, 12), Ok(()));
    assert_eq!(encoder.u64(10), Ok(()));
    assert_eq!(encoder.u64(100), Ok(()));
    assert_eq!(encoder.tag(8, 12), Ok(()));
    assert_eq!(encoder.bytes(&[0x81; 32], 32), Ok(()));
    assert_eq!(encoder.tag(9, 12), Ok(()));
    assert_eq!(encoder.u128(1_000), Ok(()));
    assert_eq!(encoder.tag(10, 12), Ok(()));
    assert_eq!(encoder.bytes(&[0x91; 32], 32), Ok(()));
    assert_eq!(encoder.tag(11, 12), Ok(()));
    assert_eq!(encoder.bytes(&[0x42, 0x43], 524_288), Ok(()));
}

fn signed_activity(registry: &ModuleRegistry) -> (Vec<u8>, [u8; 32]) {
    let key = EdSigningKey::from_bytes(&[0x31; 32]);
    let public_key = key.verifying_key().to_bytes();
    let mut unsigned = Encoder::new(4_096);
    assert_eq!(unsigned.structure_header(0x1001), Ok(()));
    assert_eq!(unsigned.u8(11), Ok(()));
    activity_fields(&mut unsigned, &public_key);
    let unsigned = unsigned.finish();
    let message = SignatureMessage::new(
        Domain::SignaturePreimage,
        PROTOCOL_VERSION,
        NETWORK_ID,
        &unsigned,
    )
    .unwrap_or_else(|error| panic!("activity signature message: {error:?}"));
    let signature = key.sign(&message.digest()).to_bytes();

    let mut signed = Encoder::new(4_096);
    assert_eq!(signed.structure_header(0x1001), Ok(()));
    assert_eq!(signed.u8(12), Ok(()));
    activity_fields(&mut signed, &public_key);
    assert_eq!(signed.tag(12, 12), Ok(()));
    assert_eq!(signed.bytes(&signature, 128), Ok(()));
    let bytes = signed.finish();
    let decoded = decode_signed(&bytes, registry)
        .unwrap_or_else(|error| panic!("signed activity: {error:?}"));
    let identifier =
        activity_id(&decoded).unwrap_or_else(|error| panic!("activity identifier: {error:?}"));
    (bytes, identifier)
}

fn receipt_bytes(activity_identifier: [u8; 32], signature: Option<[u8; 64]>) -> Vec<u8> {
    let mut encoder = Encoder::new(4_096);
    assert_eq!(encoder.structure_header(0x5201), Ok(()));
    assert_eq!(encoder.u16(PROTOCOL_VERSION), Ok(()));
    assert_eq!(encoder.bytes(&activity_identifier, 32), Ok(()));
    assert_eq!(encoder.u64(GLOBAL_SEQUENCE), Ok(()));
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

fn signed_receipt(activity_identifier: [u8; 32], key: &EdSigningKey) -> Vec<u8> {
    let unsigned = receipt_bytes(activity_identifier, None);
    let digest =
        receipt_digest(&unsigned).unwrap_or_else(|error| panic!("receipt digest: {error:?}"));
    receipt_bytes(activity_identifier, Some(key.sign(&digest).to_bytes()))
}

fn header_bytes(
    activity_root: [u8; 32],
    receipt_root: [u8; 32],
    sequencer_id: [u8; 32],
) -> Vec<u8> {
    let mut encoder = Encoder::new(354);
    assert_eq!(encoder.structure_header(0x1701), Ok(()));
    assert_eq!(encoder.u8(15), Ok(()));
    for field in 1..=15 {
        assert_eq!(encoder.tag(field, 15), Ok(()));
        match field {
            1 => assert_eq!(encoder.u16(PROTOCOL_VERSION), Ok(())),
            2 => assert_eq!(encoder.u32(NETWORK_ID), Ok(())),
            3 => assert_eq!(encoder.u64(EPOCH), Ok(())),
            4 => assert_eq!(encoder.u64(BATCH_NUMBER), Ok(())),
            5 | 6 => assert_eq!(encoder.u64(GLOBAL_SEQUENCE), Ok(())),
            7 => assert_eq!(encoder.bytes(&[2; 32], 32), Ok(())),
            8 => assert_eq!(encoder.bytes(&[3; 32], 32), Ok(())),
            9 => assert_eq!(encoder.bytes(&activity_root, 32), Ok(())),
            10 => assert_eq!(encoder.bytes(&receipt_root, 32), Ok(())),
            11 => assert_eq!(encoder.bytes(&[13; 32], 32), Ok(())),
            12 => assert_eq!(encoder.bytes(&[12; 32], 32), Ok(())),
            13 => assert_eq!(encoder.bytes(&[14; 32], 32), Ok(())),
            14 => assert_eq!(encoder.u64(1_000), Ok(())),
            15 => assert_eq!(encoder.bytes(&sequencer_id, 32), Ok(())),
            _ => panic!("unreachable header field"),
        }
    }
    encoder.finish()
}

struct InclusionFixture {
    registry: ModuleRegistry,
    activity: Vec<u8>,
    activity_id: [u8; 32],
    activity_proof: Proof,
    receipt: Vec<u8>,
    receipt_proof: Proof,
    header: Vec<u8>,
    header_signature: [u8; 64],
    authorization: SequencerAuthorization,
}

fn inclusion_fixture() -> InclusionFixture {
    let registry = registry();
    let (activity, activity_identifier) = signed_activity(&registry);
    let (activity_proof, activity_root) = build_proof(&[activity.as_slice()], 0)
        .unwrap_or_else(|error| panic!("activity proof: {error:?}"));
    let sequencer = EdSigningKey::from_bytes(&[0x63; 32]);
    let sequencer_id = sequencer.verifying_key().to_bytes();
    let receipt = signed_receipt(activity_identifier, &sequencer);
    let (receipt_proof, receipt_root) = build_proof(&[receipt.as_slice()], 0)
        .unwrap_or_else(|error| panic!("receipt proof: {error:?}"));
    let header = header_bytes(activity_root, receipt_root, sequencer_id);
    let digest =
        batch_header_digest(&header).unwrap_or_else(|error| panic!("header digest: {error:?}"));
    InclusionFixture {
        registry,
        activity,
        activity_id: activity_identifier,
        activity_proof,
        receipt,
        receipt_proof,
        header,
        header_signature: sequencer.sign(&digest).to_bytes(),
        authorization: SequencerAuthorization::new(
            sequencer_id,
            sequencer_id,
            BATCH_NUMBER,
            BATCH_NUMBER,
        ),
    }
}

fn authorised_batch(fixture: &InclusionFixture) -> AuthorizedBatch {
    AuthorizedBatch::new(
        [4; 32],
        [5; 32],
        [2; 32],
        [3; 32],
        fixture.authorization.public_key(),
    )
}

fn persist_receipt(durable: &mut Store, idempotency_key: [u8; 32], fixture: &InclusionFixture) {
    store(
        durable,
        tenant(),
        idempotency_key,
        &fixture.receipt,
        &authorised_batch(fixture),
    )
    .unwrap_or_else(|error| panic!("store receipt: {error:?}"));
}

fn inclusion_bundle(fixture: &InclusionFixture) -> InclusionBundle<'_> {
    InclusionBundle {
        registry: &fixture.registry,
        activity_bytes: &fixture.activity,
        activity_proof: &fixture.activity_proof,
        receipt_bytes: &fixture.receipt,
        receipt_proof: &fixture.receipt_proof,
        header_bytes: &fixture.header,
        header_signature: fixture.header_signature,
        authorization: &fixture.authorization,
    }
}

struct Scripted {
    responses: VecDeque<Vec<u8>>,
}

impl Scripted {
    fn one(response: Vec<u8>) -> Self {
        Self {
            responses: VecDeque::from([response]),
        }
    }
}

impl FrameTransport for Scripted {
    fn send(&mut self, _frame: &[u8]) -> Result<(), TransportError> {
        Ok(())
    }

    fn receive(&mut self) -> Result<Vec<u8>, TransportError> {
        self.responses
            .pop_front()
            .ok_or(TransportError::PeerShutdown)
    }
}

fn envelope(tag: u16, correlation_id: u64, payload: &[u8], proof: &[u8]) -> Vec<u8> {
    encode_envelope(Envelope {
        version: Version::V1_2,
        message_tag: tag,
        correlation_id,
        canonical_payload: payload,
        proof_material: proof,
    })
    .unwrap_or_else(|error| panic!("response envelope: {error:?}"))
}

fn encode_proof_bundle(kind: u8, fixture: &InclusionFixture, proof: &Proof) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&1_u16.to_be_bytes());
    bytes.push(kind);
    bytes.extend_from_slice(&fixture.activity_id);
    bytes.extend_from_slice(&proof.leaf_index().to_be_bytes());
    bytes.extend_from_slice(&proof.leaf_count().to_be_bytes());
    bytes.push(
        u8::try_from(proof.siblings().len())
            .unwrap_or_else(|_| panic!("proof depth does not fit wire")),
    );
    for sibling in proof.siblings() {
        bytes.extend_from_slice(sibling);
    }
    bytes.extend_from_slice(&1_u16.to_be_bytes());
    let public = fixture.authorization.public_key();
    bytes.extend_from_slice(&public);
    bytes.extend_from_slice(&public);
    bytes.extend_from_slice(&BATCH_NUMBER.to_be_bytes());
    bytes.extend_from_slice(&BATCH_NUMBER.to_be_bytes());
    bytes.extend_from_slice(
        &u32::try_from(fixture.header.len())
            .unwrap_or_else(|_| panic!("header length does not fit wire"))
            .to_be_bytes(),
    );
    bytes.extend_from_slice(&fixture.header);
    bytes.extend_from_slice(&fixture.header_signature);
    bytes
}

fn verified_proof_bundles(
    fixture: &InclusionFixture,
) -> (VerifiedProofBundle, VerifiedProofBundle) {
    let context = |correlation_id| EvidenceContext {
        interface_version: Version::V1_2,
        correlation_id,
        expected_protocol_version: PROTOCOL_VERSION,
        expected_network_id: NETWORK_ID,
        handshake_sequencer_key: fixture.authorization.public_key(),
    };
    let mut activity_transport = Scripted::one(envelope(
        17,
        71,
        &fixture.activity,
        &encode_proof_bundle(1, fixture, &fixture.activity_proof),
    ));
    let activity = proof_bundle(
        &mut activity_transport,
        ProofBundleSelector::Activity(fixture.activity_id),
        context(71),
        &fixture.registry,
    )
    .unwrap_or_else(|error| panic!("verified activity bundle: {error:?}"));
    let mut receipt_transport = Scripted::one(envelope(
        17,
        72,
        &fixture.receipt,
        &encode_proof_bundle(3, fixture, &fixture.receipt_proof),
    ));
    let receipt = proof_bundle(
        &mut receipt_transport,
        ProofBundleSelector::Receipt(fixture.activity_id),
        context(72),
        &fixture.registry,
    )
    .unwrap_or_else(|error| panic!("verified receipt bundle: {error:?}"));
    (activity, receipt)
}

struct GuarantorFixture {
    identifier: [u8; 32],
    public_key: [u8; 33],
    attestation: Vec<u8>,
}

fn guarantor(value: u8, checkpoint: [u8; 32]) -> GuarantorFixture {
    let mut scalar = [0_u8; 32];
    scalar[31] = value;
    let signing = SigningKey::from_bytes((&scalar).into())
        .unwrap_or_else(|error| panic!("guarantor key: {error}"));
    let encoded = signing.verifying_key().to_encoded_point(true);
    let public_key: [u8; 33] = encoded
        .as_bytes()
        .try_into()
        .unwrap_or_else(|_| panic!("public key width"));
    let mut identifier = [0_u8; 32];
    identifier[0] = value;
    let mut message = Vec::with_capacity(189);
    message.extend_from_slice(&PROTOCOL_VERSION.to_be_bytes());
    message.extend_from_slice(&NETWORK_ID.to_be_bytes());
    message.extend_from_slice(&PAXEER_CHAIN_ID.to_be_bytes());
    message.extend_from_slice(&SETTLEMENT_CONTRACT);
    message.extend_from_slice(&EPOCH.to_be_bytes());
    message.extend_from_slice(&checkpoint);
    message.extend_from_slice(&checkpoint);
    message.extend_from_slice(&identifier);
    message.extend_from_slice(&BATCH_NUMBER.to_be_bytes());
    message.extend_from_slice(&[12; 32]);
    message.extend_from_slice(&[1, 1, 0x1f]);
    message.extend_from_slice(&(1_000 + u64::from(value)).to_be_bytes());
    assert_eq!(message.len(), 189);
    let digest = checkpoint_attestation_digest(&message)
        .unwrap_or_else(|error| panic!("attestation digest: {error:?}"));
    let (signature, recovery_id): (Signature, _) = signing
        .sign_prehash_recoverable(&digest)
        .unwrap_or_else(|error| panic!("attestation signature: {error}"));
    let signer = secp256k1::evm_address(&public_key)
        .unwrap_or_else(|error| panic!("attestation signer: {error:?}"));
    let mut attestation = message;
    attestation.extend_from_slice(&signer);
    attestation.extend_from_slice(&signature.to_bytes());
    attestation.push(27 + u8::from(recovery_id));
    assert_eq!(attestation.len(), 274);
    GuarantorFixture {
        identifier,
        public_key,
        attestation,
    }
}

fn registration_reference(checkpoint: [u8; 32], transaction_byte: u8) -> Vec<u8> {
    let mut reference = Vec::with_capacity(110);
    reference.extend_from_slice(&1_u16.to_be_bytes());
    reference.extend_from_slice(&PAXEER_CHAIN_ID.to_be_bytes());
    reference.extend_from_slice(&SETTLEMENT_CONTRACT);
    reference.extend_from_slice(&checkpoint);
    reference.extend_from_slice(&[transaction_byte; 32]);
    reference.extend_from_slice(&7_u64.to_be_bytes());
    reference.extend_from_slice(&1_100_u64.to_be_bytes());
    assert_eq!(reference.len(), 110);
    reference
}

struct CheckpointFixture {
    checkpoint_bytes: Vec<u8>,
    context_bytes: Vec<u8>,
    identifier: [u8; 32],
    registration_reference: Vec<u8>,
}

fn checkpoint_fixture(
    header: &[u8],
    signer_count: u8,
    threshold: u8,
    ineligible_signer: Option<u8>,
    mismatched_registration: bool,
) -> CheckpointFixture {
    let validity = b"PROOF";
    let checkpoint = Checkpoint::new(header.to_vec(), validity.to_vec());
    let identifier = checkpoint_id(&checkpoint)
        .unwrap_or_else(|error| panic!("checkpoint identifier: {error:?}"));
    let guarantors = (1..=signer_count)
        .map(|value| guarantor(value, identifier))
        .collect::<Vec<_>>();
    let stored_reference = registration_reference(identifier, 0x91);
    let context_reference = if mismatched_registration {
        registration_reference(identifier, 0x92)
    } else {
        stored_reference.clone()
    };

    let mut checkpoint_bytes = Vec::new();
    checkpoint_bytes.extend_from_slice(&1_u16.to_be_bytes());
    checkpoint_bytes.extend_from_slice(
        &u32::try_from(header.len())
            .unwrap_or_else(|_| panic!("header length does not fit wire"))
            .to_be_bytes(),
    );
    checkpoint_bytes.extend_from_slice(header);
    checkpoint_bytes.extend_from_slice(
        &u32::try_from(validity.len())
            .unwrap_or_else(|_| panic!("validity length does not fit wire"))
            .to_be_bytes(),
    );
    checkpoint_bytes.extend_from_slice(validity);
    checkpoint_bytes.push(signer_count);
    for guarantor in &guarantors {
        checkpoint_bytes.extend_from_slice(&guarantor.attestation);
    }
    checkpoint_bytes.push(threshold);
    checkpoint_bytes.extend_from_slice(
        &u16::try_from(stored_reference.len())
            .unwrap_or_else(|_| panic!("registration reference length does not fit wire"))
            .to_be_bytes(),
    );
    checkpoint_bytes.extend_from_slice(&stored_reference);

    let mut context_bytes = Vec::new();
    context_bytes.extend_from_slice(&1_u16.to_be_bytes());
    context_bytes.extend_from_slice(&0_u64.to_be_bytes());
    context_bytes.extend_from_slice(&1_u64.to_be_bytes());
    context_bytes.extend_from_slice(&1_u64.to_be_bytes());
    context_bytes.push(signer_count);
    for (index, guarantor) in guarantors.iter().enumerate() {
        context_bytes.extend_from_slice(&guarantor.identifier);
        context_bytes.extend_from_slice(&guarantor.public_key);
        context_bytes.extend_from_slice(&100_u128.to_be_bytes());
        context_bytes.extend_from_slice(&1_u64.to_be_bytes());
        context_bytes.extend_from_slice(&0_u64.to_be_bytes());
        context_bytes.extend_from_slice(&0_u64.to_be_bytes());
        context_bytes.push(1);
        context_bytes.extend_from_slice(&guarantor.public_key);
        context_bytes.extend_from_slice(&1_u64.to_be_bytes());
        context_bytes.extend_from_slice(&0_u64.to_be_bytes());
        context_bytes.extend_from_slice(&1_u64.to_be_bytes());
        let value = u8::try_from(index + 1).unwrap_or_else(|_| panic!("guarantor index"));
        context_bytes.push(u8::from(ineligible_signer != Some(value)) << 2);
    }
    context_bytes.extend_from_slice(&EPOCH.to_be_bytes());
    context_bytes.extend_from_slice(&900_u64.to_be_bytes());
    context_bytes.extend_from_slice(&2_000_u64.to_be_bytes());
    context_bytes.extend_from_slice(&1_000_u64.to_be_bytes());
    context_bytes.push(threshold);
    context_bytes.extend_from_slice(&1_u128.to_be_bytes());
    context_bytes.push(1);
    context_bytes.extend_from_slice(&identifier);
    context_bytes.extend_from_slice(&[3; 32]);
    context_bytes.extend_from_slice(&BATCH_NUMBER.to_be_bytes());
    context_bytes.extend_from_slice(&PAXEER_CHAIN_ID.to_be_bytes());
    context_bytes.extend_from_slice(&SETTLEMENT_CONTRACT);
    context_bytes.extend_from_slice(
        &u16::try_from(context_reference.len())
            .unwrap_or_else(|_| panic!("context reference length does not fit wire"))
            .to_be_bytes(),
    );
    context_bytes.extend_from_slice(&context_reference);

    CheckpointFixture {
        checkpoint_bytes,
        context_bytes,
        identifier,
        registration_reference: stored_reference,
    }
}

fn authenticated_checkpoint(
    fixture: &CheckpointFixture,
) -> Result<VerifiedCheckpoint, EvidenceError> {
    let correlation_id = 81;
    let mut transport = Scripted::one(envelope(
        15,
        correlation_id,
        &fixture.checkpoint_bytes,
        &fixture.context_bytes,
    ));
    checkpoint(
        &mut transport,
        CheckpointSelector::Batch(BATCH_NUMBER),
        EvidenceContext {
            interface_version: Version::V1_2,
            correlation_id,
            expected_protocol_version: PROTOCOL_VERSION,
            expected_network_id: NETWORK_ID,
            handshake_sequencer_key: [0x63; 32],
        },
    )
}

#[test]
fn authenticated_checkpoint_and_registration_raise_only_finalised_without_altering_receipt() {
    let root = directory("finalised");
    let idempotency_key = [0x41; 32];
    let fixture = inclusion_fixture();
    let mut durable = Store::open(&root).unwrap_or_else(|error| panic!("store: {error}"));
    persist_receipt(&mut durable, idempotency_key, &fixture);
    let (activity, receipt) = verified_proof_bundles(&fixture);
    let checkpoint_fixture = checkpoint_fixture(&fixture.header, 3, 2, None, false);
    let checkpoint = authenticated_checkpoint(&checkpoint_fixture)
        .unwrap_or_else(|error| panic!("authenticated checkpoint: {error:?}"));

    let record = augment_verified(
        &mut durable,
        tenant(),
        idempotency_key,
        &activity,
        &receipt,
        Some(&checkpoint),
    )
    .unwrap_or_else(|error| panic!("augment verified: {error:?}"));
    assert_eq!(
        record.verification_level,
        VerificationLevel::CHECKPOINT_FINALISED
    );
    assert_ne!(
        record.verification_level,
        VerificationLevel::SETTLEMENT_ANCHORED
    );
    assert_eq!(record.checkpoint_id, Some(checkpoint_fixture.identifier));
    assert_eq!(record.guarantor_signatures_achieved, Some(3));
    assert_eq!(record.guarantor_threshold, Some(2));
    assert_eq!(
        record.settlement_reference.as_deref(),
        Some(checkpoint_fixture.registration_reference.as_slice())
    );
    assert!(!record.activity_proof.is_empty());
    assert!(!record.receipt_proof.is_empty());
    let served = serve(
        &durable,
        tenant(),
        ReceiptLookupKey::Idempotency(idempotency_key),
    )
    .unwrap_or_else(|error| panic!("serve: {error:?}"));
    assert_eq!(served.canonical_bytes, fixture.receipt);
    assert_eq!(
        served.metadata.verification_level,
        VerificationLevel::CHECKPOINT_FINALISED
    );
    let repeated = store_verified_if_absent(
        &mut durable,
        tenant(),
        idempotency_key,
        &fixture.receipt,
        &authorised_batch(&fixture),
    )
    .unwrap_or_else(|error| panic!("repeat verified ingress: {error:?}"));
    assert_eq!(
        repeated.metadata.verification_level,
        VerificationLevel::CHECKPOINT_FINALISED
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
fn raw_inclusion_cannot_escalate_a_candidate_checkpoint_and_wait_reports_observed_level() {
    let root = directory("pending");
    let idempotency_key = [0x42; 32];
    let fixture = inclusion_fixture();
    let mut durable = Store::open(&root).unwrap_or_else(|error| panic!("store: {error}"));
    persist_receipt(&mut durable, idempotency_key, &fixture);
    let checkpoint_fixture = checkpoint_fixture(&fixture.header, 2, 2, None, false);
    let _untrusted_candidate = FinalityEvidenceCandidate::from_exact_bytes(
        checkpoint_fixture.checkpoint_bytes,
        checkpoint_fixture.context_bytes,
        PROTOCOL_VERSION,
        NETWORK_ID,
    )
    .unwrap_or_else(|error| panic!("locally checked candidate: {error:?}"));
    let record = augment(
        &mut durable,
        tenant(),
        idempotency_key,
        &inclusion_bundle(&fixture),
    )
    .unwrap_or_else(|error| panic!("raw inclusion augment: {error:?}"));
    assert_eq!(record.verification_level, VerificationLevel::BATCH_INCLUDED);
    assert_eq!(record.checkpoint_id, None);
    assert_eq!(record.guarantor_signatures_achieved, None);
    assert_eq!(record.guarantor_threshold, None);
    assert_eq!(record.settlement_reference, None);

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
fn ineligible_bonded_snapshot_records_no_unearned_checkpoint_level() {
    let root = directory("threshold");
    let idempotency_key = [0x43; 32];
    let fixture = inclusion_fixture();
    let mut durable = Store::open(&root).unwrap_or_else(|error| panic!("store: {error}"));
    persist_receipt(&mut durable, idempotency_key, &fixture);
    let checkpoint_fixture = checkpoint_fixture(&fixture.header, 2, 2, Some(2), false);
    assert!(matches!(
        authenticated_checkpoint(&checkpoint_fixture),
        Err(EvidenceError::Requirements)
    ));
    let served = serve(
        &durable,
        tenant(),
        ReceiptLookupKey::Idempotency(idempotency_key),
    )
    .unwrap_or_else(|error| panic!("serve: {error:?}"));
    assert_eq!(served.canonical_bytes, fixture.receipt);
    assert_eq!(
        served.metadata.verification_level,
        VerificationLevel::SEQUENCER_SIGNED
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn mismatched_registration_reference_is_rejected_without_level_change() {
    let root = directory("registration");
    let idempotency_key = [0x44; 32];
    let fixture = inclusion_fixture();
    let mut durable = Store::open(&root).unwrap_or_else(|error| panic!("store: {error}"));
    persist_receipt(&mut durable, idempotency_key, &fixture);
    let checkpoint_fixture = checkpoint_fixture(&fixture.header, 2, 2, None, true);
    assert!(matches!(
        authenticated_checkpoint(&checkpoint_fixture),
        Err(EvidenceError::Registration)
    ));
    let served = serve(
        &durable,
        tenant(),
        ReceiptLookupKey::Idempotency(idempotency_key),
    )
    .unwrap_or_else(|error| panic!("serve: {error:?}"));
    assert_eq!(served.canonical_bytes, fixture.receipt);
    assert_eq!(
        served.metadata.verification_level,
        VerificationLevel::SEQUENCER_SIGNED
    );
    let _ = std::fs::remove_dir_all(root);
}

use ed25519_dalek::{Signer as _, SigningKey};
use layerx_interop_gateway::adapter::{
    AdapterDescriptor, AdapterId, ConformanceSuite, PinnedSpec, SpecVersion,
};
use layerx_interop_gateway::audit::{verify_export, AuditEventKind};
use layerx_interop_gateway::error::{error_emission, GatewayError, Retriability};
use layerx_interop_gateway::gateway::{TranslationKind, TranslationRequest, TranslationStatus};
use layerx_interop_gateway::principal::PrincipalId;
use layerx_interop_gateway::trace::TraceId;
use layerx_interop_gateway::{interop_gateway_core, GatewayCore};
use layerx_proof::merkle::leaf_hash;
use layerx_proof::receipt::AuthorizedBatch;
use layerx_types::payload::ModuleId;
use sha2::{Digest as _, Sha256};

const AGENT_ACCOUNT: [u8; 32] = [0xa1; 32];
const ASSET: [u8; 32] = [0xc1; 32];
const COUNTERPARTY: [u8; 32] = [0xd1; 32];
const PERIOD_START: u64 = 1_700_000_000;
const WINDOW_START: u64 = 100;

struct ReceiptFields {
    activity_id: [u8; 32],
    sequence: u64,
    previous_state_root: [u8; 32],
    resulting_state_root: [u8; 32],
    batch_id: [u8; 32],
    asset: [u8; 32],
    amount: u128,
    counterparty: [u8; 32],
}

struct ReceiptMaterial {
    canonical_receipt: Vec<u8>,
    authorised_batch: AuthorizedBatch,
    sequencer_seed: [u8; 32],
}

fn signed_receipt(sequence: u64, idempotency_key: [u8; 32], amount: u128) -> ReceiptMaterial {
    let activity_id: [u8; 32] = Sha256::digest(
        [
            b"layerx-interop-activity/v1".as_slice(),
            &sequence.to_be_bytes(),
            &idempotency_key,
        ]
        .concat(),
    )
    .into();
    let fields = ReceiptFields {
        activity_id,
        sequence,
        previous_state_root: Sha256::digest([b"before".as_slice(), &activity_id].concat()).into(),
        resulting_state_root: Sha256::digest([b"after".as_slice(), &activity_id].concat()).into(),
        batch_id: Sha256::digest([b"batch".as_slice(), &activity_id].concat()).into(),
        asset: ASSET,
        amount,
        counterparty: COUNTERPARTY,
    };
    let sequencer_seed: [u8; 32] =
        Sha256::digest([b"interop-sequencer".as_slice(), &activity_id].concat()).into();
    let signer = SigningKey::from_bytes(&sequencer_seed);
    let unsigned = encode_receipt(&fields, None);
    let mut digest = Sha256::new();
    digest.update(b"LXP/v1/receipt\0");
    digest.update(&unsigned);
    let signature = signer.sign(&<[u8; 32]>::from(digest.finalize()));
    ReceiptMaterial {
        canonical_receipt: encode_receipt(&fields, Some(signature.to_bytes())),
        authorised_batch: AuthorizedBatch::new(
            fields.batch_id,
            fields.asset,
            fields.previous_state_root,
            fields.resulting_state_root,
            signer.verifying_key().to_bytes(),
        ),
        sequencer_seed,
    }
}

fn encode_receipt(fields: &ReceiptFields, signature: Option<[u8; 64]>) -> Vec<u8> {
    let debit_before = 10_000_u128;
    let credit_before = 20_000_u128;
    let mut bytes = Vec::new();
    push_u16(&mut bytes, 2);
    push_u16(&mut bytes, 0x5201);
    push_u16(&mut bytes, 2);
    push_bytes(&mut bytes, &fields.activity_id);
    push_u64(&mut bytes, fields.sequence);
    push_bytes(&mut bytes, &fields.previous_state_root);
    push_bytes(&mut bytes, &fields.resulting_state_root);
    push_bytes(&mut bytes, &[0x81; 32]);
    bytes.extend_from_slice(&0_i32.to_be_bytes());
    bytes.extend_from_slice(&0_u32.to_be_bytes());
    bytes.extend_from_slice(&1_u128.to_be_bytes());
    push_bytes(&mut bytes, &fields.batch_id);
    push_u16(&mut bytes, ModuleId::Asset as u16);
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.push(1);
    push_bytes(&mut bytes, &fields.asset);
    bytes.extend_from_slice(&fields.amount.to_be_bytes());
    push_bytes(&mut bytes, &AGENT_ACCOUNT);
    bytes.extend_from_slice(&debit_before.to_be_bytes());
    bytes.extend_from_slice(&(debit_before - fields.amount).to_be_bytes());
    push_u64(&mut bytes, fields.sequence - WINDOW_START + 1);
    push_bytes(&mut bytes, &fields.counterparty);
    bytes.extend_from_slice(&credit_before.to_be_bytes());
    bytes.extend_from_slice(&(credit_before + fields.amount).to_be_bytes());
    push_bytes(&mut bytes, &[0x91; 32]);
    push_bytes(&mut bytes, &[0x92; 32]);
    push_bytes(&mut bytes, &[0x93; 32]);
    push_u64(&mut bytes, PERIOD_START + fields.sequence);
    bytes.push(u8::from(signature.is_some()));
    if let Some(signature) = signature {
        push_bytes(&mut bytes, &signature);
    }
    bytes
}

fn push_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

fn push_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

fn push_bytes(output: &mut Vec<u8>, value: &[u8]) {
    let length = u32::try_from(value.len()).unwrap_or_else(|_| panic!("receipt field overflow"));
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(value);
}

fn principal(name: &str) -> PrincipalId {
    PrincipalId::new(name).unwrap_or_else(|error| panic!("principal {name}: {error}"))
}

fn adapter_id(name: &str) -> AdapterId {
    AdapterId::new(name).unwrap_or_else(|error| panic!("adapter {name}: {error}"))
}

fn x402_descriptor() -> AdapterDescriptor {
    let version = SpecVersion::parse("2.0.0").unwrap_or_else(|error| panic!("version: {error}"));
    let spec = PinnedSpec::new(adapter_id("x402"), version, [0x51; 32])
        .unwrap_or_else(|error| panic!("spec: {error}"));
    let conformance = ConformanceSuite::new(adapter_id("x402-v2-vectors"), 96, [0x52; 32])
        .unwrap_or_else(|error| panic!("conformance: {error}"));
    AdapterDescriptor::new(adapter_id("x402"), spec, conformance)
}

fn registered_core() -> GatewayCore {
    let mut core = interop_gateway_core();
    core.register_adapter(x402_descriptor(), &TraceId::mint([0xee; 16]), 1)
        .unwrap_or_else(|error| panic!("register: {error}"));
    core
}

fn request(kind: TranslationKind, key_byte: u8) -> TranslationRequest {
    TranslationRequest::new(adapter_id("x402"), kind, [key_byte; 32], [0x42; 32])
        .unwrap_or_else(|error| panic!("request: {error}"))
}

#[test]
fn registration_pins_the_spec_and_conformance_suite_and_refuses_duplicates() {
    let mut core = registered_core();
    let declared = core
        .adapter(&adapter_id("x402"))
        .unwrap_or_else(|| panic!("registered adapter missing"));
    assert_eq!(declared.spec().version().as_str(), "2.0.0");
    assert_eq!(declared.spec().document_digest(), [0x51; 32]);
    assert_eq!(declared.conformance().vector_count(), 96);
    assert_eq!(declared.conformance().suite_digest(), [0x52; 32]);

    let trace = TraceId::mint([0xef; 16]);
    let refused = core
        .register_adapter(x402_descriptor(), &trace, 2)
        .map_err(|error| *error.error());
    assert_eq!(refused, Err(GatewayError::DuplicateAdapter));

    let entries = core.registry_audit().entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].kind(), AuditEventKind::AdapterRegistered);
    assert_eq!(entries[0].subject(), [0x51; 32]);
    assert_eq!(entries[0].adapter(), "x402");
}

#[test]
fn translations_against_unregistered_adapters_are_refused() {
    let mut core = interop_gateway_core();
    let trace = TraceId::mint([1; 16]);
    let refused = core
        .begin_translation(
            &principal("alice"),
            &request(TranslationKind::ReadOnly, 0x11),
            &trace,
            10,
        )
        .map_err(|error| *error.error());
    assert_eq!(refused, Err(GatewayError::UnknownAdapter));
}

#[test]
fn a_state_changing_translation_refuses_every_unverified_termination() {
    let mut core = registered_core();
    let alice = principal("alice");
    let trace = TraceId::mint([2; 16]);
    let key = [0x21; 32];
    let opened = core
        .begin_translation(
            &alice,
            &request(TranslationKind::StateChanging, 0x21),
            &trace,
            10,
        )
        .unwrap_or_else(|error| panic!("begin: {error}"));
    assert_eq!(opened, TranslationStatus::Pending);

    let no_receipt = core
        .complete_read_only(&alice, key, &trace, 11)
        .map_err(|error| *error.error());
    assert_eq!(no_receipt, Err(GatewayError::ReceiptRequired));
    assert_eq!(
        core.translation(&alice, key),
        Some(TranslationStatus::Pending),
        "a refused completion must not advance the translation"
    );

    let material = signed_receipt(150, key, 250);
    let mut tampered = material.canonical_receipt.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 0x01;
    let rejected = core
        .settle_with_receipt(
            &alice,
            key,
            &tampered,
            &material.authorised_batch,
            &trace,
            12,
        )
        .map_err(|error| *error.error());
    assert!(
        matches!(rejected, Err(GatewayError::ReceiptRejected(_))),
        "tampered receipt bytes must be rejected, got {rejected:?}"
    );
    assert_eq!(
        core.translation(&alice, key),
        Some(TranslationStatus::Pending)
    );

    let foreign_signer = SigningKey::from_bytes(&[0x99; 32]);
    let foreign_authority = AuthorizedBatch::new(
        material.authorised_batch.batch_id(),
        material.authorised_batch.asset(),
        material.authorised_batch.previous_state_root(),
        material.authorised_batch.resulting_state_root(),
        foreign_signer.verifying_key().to_bytes(),
    );
    assert_ne!(foreign_signer.to_bytes(), material.sequencer_seed);
    let unauthorised = core
        .settle_with_receipt(
            &alice,
            key,
            &material.canonical_receipt,
            &foreign_authority,
            &trace,
            13,
        )
        .map_err(|error| *error.error());
    assert!(
        matches!(unauthorised, Err(GatewayError::ReceiptRejected(_))),
        "a receipt outside the authorised batch must be rejected, got {unauthorised:?}"
    );
    assert_eq!(
        core.translation(&alice, key),
        Some(TranslationStatus::Pending)
    );
}

#[test]
fn a_verified_receipt_terminates_the_translation_and_binds_the_audit_chain() {
    let mut core = registered_core();
    let alice = principal("alice");
    let trace = TraceId::mint([2; 16]);
    let key = [0x21; 32];
    core.begin_translation(
        &alice,
        &request(TranslationKind::StateChanging, 0x21),
        &trace,
        10,
    )
    .unwrap_or_else(|error| panic!("begin: {error}"));

    let material = signed_receipt(150, key, 250);
    let settled = core
        .settle_with_receipt(
            &alice,
            key,
            &material.canonical_receipt,
            &material.authorised_batch,
            &trace,
            14,
        )
        .unwrap_or_else(|error| panic!("settle: {error}"));
    let receipt_digest =
        leaf_hash(&material.canonical_receipt).unwrap_or_else(|error| panic!("digest: {error:?}"));
    assert_eq!(
        settled,
        TranslationStatus::ReceiptVerified { receipt_digest }
    );

    let replay = core
        .settle_with_receipt(
            &alice,
            key,
            &material.canonical_receipt,
            &material.authorised_batch,
            &trace,
            15,
        )
        .unwrap_or_else(|error| panic!("replayed settle: {error}"));
    assert_eq!(
        replay,
        TranslationStatus::ReceiptVerified { receipt_digest }
    );

    let other = signed_receipt(151, key, 251);
    let conflict = core
        .settle_with_receipt(
            &alice,
            key,
            &other.canonical_receipt,
            &other.authorised_batch,
            &trace,
            16,
        )
        .map_err(|error| *error.error());
    assert_eq!(conflict, Err(GatewayError::IdempotencyConflict));

    let chain = core
        .principal_audit(&alice)
        .unwrap_or_else(|| panic!("principal audit chain missing"));
    let completed = chain
        .entries()
        .iter()
        .find(|entry| entry.kind() == AuditEventKind::TranslationCompleted)
        .unwrap_or_else(|| panic!("completion was not audited"));
    assert_eq!(completed.subject(), receipt_digest);
    assert_eq!(completed.trace(), &trace);
    let head = chain.head().unwrap_or_else(|error| panic!("head: {error}"));
    verify_export("alice", &chain.export(), &head)
        .unwrap_or_else(|error| panic!("audit chain broken: {error}"));
}

#[test]
fn idempotency_keys_bind_content_and_replay_the_original() {
    let mut core = registered_core();
    let alice = principal("alice");
    let trace = TraceId::mint([3; 16]);
    let first = request(TranslationKind::StateChanging, 0x31);
    core.begin_translation(&alice, &first, &trace, 10)
        .unwrap_or_else(|error| panic!("begin: {error}"));
    let replay = core
        .begin_translation(&alice, &first, &trace, 11)
        .unwrap_or_else(|error| panic!("replay: {error}"));
    assert_eq!(replay, TranslationStatus::Pending);

    let conflicting = TranslationRequest::new(
        adapter_id("x402"),
        TranslationKind::StateChanging,
        [0x31; 32],
        [0x43; 32],
    )
    .unwrap_or_else(|error| panic!("conflicting request: {error}"));
    let conflict = core
        .begin_translation(&alice, &conflicting, &trace, 12)
        .map_err(|error| *error.error());
    assert_eq!(conflict, Err(GatewayError::IdempotencyConflict));

    let rekind = TranslationRequest::new(
        adapter_id("x402"),
        TranslationKind::ReadOnly,
        [0x31; 32],
        [0x42; 32],
    )
    .unwrap_or_else(|error| panic!("rekind request: {error}"));
    let kind_conflict = core
        .begin_translation(&alice, &rekind, &trace, 13)
        .map_err(|error| *error.error());
    assert_eq!(kind_conflict, Err(GatewayError::IdempotencyConflict));

    assert_eq!(
        TranslationRequest::new(
            adapter_id("x402"),
            TranslationKind::ReadOnly,
            [0; 32],
            [0x42; 32]
        ),
        Err(GatewayError::InvalidTranslation),
        "the reserved zero idempotency key must be refused"
    );
}

#[test]
fn principals_are_isolated_across_every_gateway_surface() {
    let mut core = registered_core();
    let alice = principal("alice");
    let mallory = principal("mallory");
    let trace = TraceId::mint([4; 16]);
    let key = [0x41; 32];
    core.begin_translation(
        &alice,
        &request(TranslationKind::StateChanging, 0x41),
        &trace,
        10,
    )
    .unwrap_or_else(|error| panic!("begin: {error}"));

    assert_eq!(core.translation(&mallory, key), None);
    let material = signed_receipt(160, key, 77);
    let stolen = core
        .settle_with_receipt(
            &mallory,
            key,
            &material.canonical_receipt,
            &material.authorised_batch,
            &trace,
            11,
        )
        .map_err(|error| *error.error());
    assert_eq!(
        stolen,
        Err(GatewayError::UnknownTranslation),
        "a principal must not reach another principal's translation"
    );
    assert_eq!(
        core.translation(&alice, key),
        Some(TranslationStatus::Pending)
    );

    let same_key = core
        .begin_translation(
            &mallory,
            &request(TranslationKind::StateChanging, 0x41),
            &trace,
            12,
        )
        .unwrap_or_else(|error| panic!("independent begin: {error}"));
    assert_eq!(
        same_key,
        TranslationStatus::Pending,
        "the same key under another principal is an independent translation, not a replay"
    );

    let alice_chain = core
        .principal_audit(&alice)
        .unwrap_or_else(|| panic!("alice audit missing"));
    let mallory_chain = core
        .principal_audit(&mallory)
        .unwrap_or_else(|| panic!("mallory audit missing"));
    assert_eq!(alice_chain.entries().len(), 1);
    assert_eq!(mallory_chain.entries().len(), 1);
    let alice_head = alice_chain
        .head()
        .unwrap_or_else(|error| panic!("head: {error}"));
    assert!(
        verify_export("mallory", &alice_chain.export(), &alice_head).is_err(),
        "one principal's chain must not verify under another principal's genesis"
    );
}

#[test]
fn read_only_translations_complete_without_receipts_but_cannot_claim_settlement() {
    let mut core = registered_core();
    let alice = principal("alice");
    let trace = TraceId::mint([5; 16]);
    let key = [0x51; 32];
    core.begin_translation(
        &alice,
        &request(TranslationKind::ReadOnly, 0x51),
        &trace,
        10,
    )
    .unwrap_or_else(|error| panic!("begin: {error}"));
    let material = signed_receipt(170, key, 5);
    let misclaimed = core
        .settle_with_receipt(
            &alice,
            key,
            &material.canonical_receipt,
            &material.authorised_batch,
            &trace,
            11,
        )
        .map_err(|error| *error.error());
    assert_eq!(misclaimed, Err(GatewayError::NotStateChanging));

    let completed = core
        .complete_read_only(&alice, key, &trace, 12)
        .unwrap_or_else(|error| panic!("complete: {error}"));
    assert_eq!(completed, TranslationStatus::Translated);
    let replay = core
        .complete_read_only(&alice, key, &trace, 13)
        .unwrap_or_else(|error| panic!("replay: {error}"));
    assert_eq!(replay, TranslationStatus::Translated);
}

#[test]
fn refusals_are_honest_terminal_states() {
    let mut core = registered_core();
    let alice = principal("alice");
    let trace = TraceId::mint([6; 16]);
    let key = [0x61; 32];
    core.begin_translation(
        &alice,
        &request(TranslationKind::StateChanging, 0x61),
        &trace,
        10,
    )
    .unwrap_or_else(|error| panic!("begin: {error}"));
    let refused = core
        .refuse_translation(&alice, key, &trace, 11)
        .unwrap_or_else(|error| panic!("refuse: {error}"));
    assert_eq!(refused, TranslationStatus::Refused);

    let material = signed_receipt(180, key, 9);
    let closed = core
        .settle_with_receipt(
            &alice,
            key,
            &material.canonical_receipt,
            &material.authorised_batch,
            &trace,
            12,
        )
        .map_err(|error| *error.error());
    assert_eq!(closed, Err(GatewayError::TranslationClosed));
    assert_eq!(
        core.translation(&alice, key),
        Some(TranslationStatus::Refused)
    );
}

#[test]
fn traces_propagate_onto_audits_errors_and_boundary_headers() {
    let mut core = registered_core();
    let alice = principal("alice");
    let begin_trace = TraceId::from_inbound(Some("trc_00112233445566778899aabbccddeeff"), [7; 16]);
    assert_eq!(
        begin_trace.as_str(),
        "trc_00112233445566778899aabbccddeeff",
        "a well-formed inbound trace must cross the boundary unchanged"
    );
    let key = [0x71; 32];
    core.begin_translation(
        &alice,
        &request(TranslationKind::StateChanging, 0x71),
        &begin_trace,
        10,
    )
    .unwrap_or_else(|error| panic!("begin: {error}"));

    let settle_trace = TraceId::mint([8; 16]);
    let material = signed_receipt(190, key, 12);
    core.settle_with_receipt(
        &alice,
        key,
        &material.canonical_receipt,
        &material.authorised_batch,
        &settle_trace,
        11,
    )
    .unwrap_or_else(|error| panic!("settle: {error}"));

    let chain = core
        .principal_audit(&alice)
        .unwrap_or_else(|| panic!("audit missing"));
    assert_eq!(chain.entries()[0].trace(), &begin_trace);
    assert_eq!(chain.entries()[1].trace(), &settle_trace);

    let error_trace = TraceId::mint([9; 16]);
    let Err(failure) = core.settle_with_receipt(
        &alice,
        [0x72; 32],
        &material.canonical_receipt,
        &material.authorised_batch,
        &error_trace,
        12,
    ) else {
        panic!("an unknown translation must not settle");
    };
    assert_eq!(failure.trace(), &error_trace);
    assert_eq!(failure.error(), &GatewayError::UnknownTranslation);
    assert_eq!(failure.error().retriability(), Retriability::Terminal);
    let rendered =
        error_emission(&failure).unwrap_or_else(|error| panic!("error emission: {error}"));
    assert!(!rendered.is_empty());
}

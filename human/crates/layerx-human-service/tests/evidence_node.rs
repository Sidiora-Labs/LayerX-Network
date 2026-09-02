mod support;

use ed25519_dalek::{Signer as _, SigningKey};
use layerx_agentd::boot::handshake_gate;
use layerx_client::lni::schema::{Capability, Version};
use layerx_client::submit::{submit_signed, Submission, SubmissionContext, SubmitError};
use layerx_crypto::SignatureMessage;
use layerx_types::activity::{Authority, EnvelopeBuilder, Signature, TimestampBound};
use layerx_types::amount::Amount;
use layerx_types::ids::{Did, IdempotencyKey};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistry, Payload};
use layerx_types::result::{KnownResult, ResultCode};
use layerx_wire::activity::{decode_signed, encode_signed_envelope, encode_unsigned_envelope};
use layerx_wire::hash::{activity_id, payload_hash_for, Domain};
use layerx_wire::limits::PROTOCOL_VERSION;

use support::evidence_node::AdmissionJournal;
use support::EVIDENCE_NETWORK_ID;

fn signed_activity(
    registry: &ModuleRegistry,
    actor_did: &[u8],
    authority_key: [u8; 32],
    signer: &SigningKey,
    sequence: u64,
    idempotency: u8,
) -> Vec<u8> {
    let activity_type = ActivityType::new(ModuleId::Asset, 1)
        .unwrap_or_else(|error| panic!("activity type: {error:?}"));
    let sequence_byte =
        u8::try_from(sequence).unwrap_or_else(|error| panic!("sequence byte: {error}"));
    let payload = Payload::new(registry, activity_type, &[sequence_byte, idempotency, 3])
        .unwrap_or_else(|error| panic!("payload: {error:?}"));
    let digest =
        payload_hash_for(&payload).unwrap_or_else(|error| panic!("payload hash: {error:?}"));
    let mut builder = EnvelopeBuilder::new();
    builder
        .protocol_version(PROTOCOL_VERSION)
        .and_then(|builder| builder.network_id(EVIDENCE_NETWORK_ID))
        .and_then(|builder| builder.activity_type(activity_type))
        .and_then(|builder| {
            builder.actor_did(Did::new(actor_did).unwrap_or_else(|error| panic!("did: {error:?}")))
        })
        .and_then(|builder| {
            builder.authority(
                Authority::owner(&authority_key)
                    .unwrap_or_else(|error| panic!("authority: {error:?}")),
            )
        })
        .and_then(|builder| builder.account_sequence(sequence))
        .and_then(|builder| {
            builder.timestamp_bound(
                TimestampBound::new(1, 1_000)
                    .unwrap_or_else(|error| panic!("timestamp bound: {error:?}")),
            )
        })
        .and_then(|builder| builder.idempotency_key(IdempotencyKey::new([idempotency; 32])))
        .and_then(|builder| builder.fee_limit(Amount::from_u128(99)))
        .and_then(|builder| builder.payload_hash(digest))
        .and_then(|builder| builder.payload(payload))
        .unwrap_or_else(|error| panic!("envelope builder: {error:?}"));
    let unsigned = builder
        .build()
        .unwrap_or_else(|error| panic!("unsigned envelope: {error:?}"));
    let unsigned_bytes = encode_unsigned_envelope(&unsigned)
        .unwrap_or_else(|error| panic!("unsigned encoding: {error:?}"));
    let message = SignatureMessage::new(
        Domain::SignaturePreimage,
        PROTOCOL_VERSION,
        EVIDENCE_NETWORK_ID,
        &unsigned_bytes,
    )
    .unwrap_or_else(|error| panic!("signature message: {error:?}"));
    let signature = signer.sign(&message.digest()).to_bytes();
    let envelope = unsigned.attach_signature(
        Signature::new(&signature).unwrap_or_else(|error| panic!("signature: {error:?}")),
    );
    encode_signed_envelope(&envelope).unwrap_or_else(|error| panic!("signed encoding: {error:?}"))
}

fn context(signer_public_key: [u8; 32], correlation_id: u64) -> SubmissionContext {
    SubmissionContext {
        interface_version: Version::V1_3,
        protocol_version: PROTOCOL_VERSION,
        network_id: EVIDENCE_NETWORK_ID,
        correlation_id,
        signer_public_key,
        attempt: 1,
    }
}

fn known(result: KnownResult) -> ResultCode {
    ResultCode::from_raw(result.raw())
}

#[test]
fn advertised_durable_submit_is_backed_by_authenticated_fdatasync_admission() {
    let receipt_signer = SigningKey::from_bytes(&[0x51; 32]);
    let actor = SigningKey::from_bytes(&[0x52; 32]);
    let actor_key = actor.verifying_key().to_bytes();
    let registry = support::evidence_registry();
    let mut gate = support::evidence_gate(&receipt_signer);
    let mut node = support::evidence_node(&receipt_signer, "durable-admission", 1);
    node.register_identity(b"did:layerx:actor", actor_key);
    let admission_directory = node.journal().directory().to_path_buf();
    let (socket_path, server) = support::serve_evidence_node(node, "durable-handshake");
    let mut transport = support::connect_evidence_node(&socket_path);

    let status = handshake_gate(&mut gate, &mut transport)
        .unwrap_or_else(|error| panic!("handshake gate: {error:?}"));
    assert!(status.writes_ready);
    assert!(status
        .available_capabilities
        .contains(&Capability::AuthenticatedDurableSubmit));
    assert!(gate.evidence_authority().is_ok());

    let first = signed_activity(&registry, b"did:layerx:actor", actor_key, &actor, 1, 0x11);
    let first_activity = decode_signed(&first, &registry)
        .unwrap_or_else(|error| panic!("decode first activity: {error:?}"));
    let first_id =
        activity_id(&first_activity).unwrap_or_else(|error| panic!("first activity id: {error:?}"));
    let admitted = submit_signed(&mut transport, &registry, context(actor_key, 7), &first)
        .unwrap_or_else(|error| panic!("first submission: {error:?}"));
    let Submission::Acknowledged(acknowledgement) = admitted else {
        panic!("first submission was not acknowledged: {admitted:?}");
    };
    assert_eq!(acknowledgement.correlation_id(), 7);
    assert_eq!(acknowledgement.activity_id(), first_id);
    assert_eq!(acknowledgement.admission_bytes(), first.as_slice());
    assert_eq!(acknowledgement.core_evidence(), first_id.as_slice());

    let retried = submit_signed(&mut transport, &registry, context(actor_key, 8), &first)
        .unwrap_or_else(|error| panic!("retried submission: {error:?}"));
    let Submission::Acknowledged(retry) = retried else {
        panic!("retry was not acknowledged: {retried:?}");
    };
    assert_eq!(retry.activity_id(), first_id);

    let second = signed_activity(&registry, b"did:layerx:actor", actor_key, &actor, 2, 0x22);
    assert_eq!(
        submit_signed(&mut transport, &registry, context(actor_key, 9), &second),
        Err(SubmitError::CoreRefusal {
            class: 4,
            result: known(KnownResult::LengthLimit),
        })
    );

    drop(transport);
    let node = server
        .join()
        .unwrap_or_else(|_| panic!("evidence node panicked"));
    assert!(!node.fail_stopped());
    assert_eq!(node.authentication_refusals(), 0);
    assert_eq!(node.queued(), vec![first_id]);
    let records = node.journal().records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].activity_id, first_id);
    assert_eq!(records[0].activity, first);
    drop(node);

    let recovered = AdmissionJournal::recover(&admission_directory, EVIDENCE_NETWORK_ID);
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].activity_id, first_id);
    assert_eq!(recovered[0].activity, first);
    let reopened = AdmissionJournal::open(&admission_directory, EVIDENCE_NETWORK_ID);
    assert!(reopened.contains(&first_id));
    let _ = std::fs::remove_file(socket_path);
}

#[test]
fn authentication_refusals_are_typed_and_consume_no_admission_capacity() {
    let receipt_signer = SigningKey::from_bytes(&[0x61; 32]);
    let actor = SigningKey::from_bytes(&[0x62; 32]);
    let impostor = SigningKey::from_bytes(&[0x63; 32]);
    let actor_key = actor.verifying_key().to_bytes();
    let impostor_key = impostor.verifying_key().to_bytes();
    let registry = support::evidence_registry();
    let mut gate = support::evidence_gate(&receipt_signer);
    let mut node = support::evidence_node(&receipt_signer, "refused-admission", 1);
    node.register_identity(b"did:layerx:actor", actor_key);
    let admission_directory = node.journal().directory().to_path_buf();
    let (socket_path, server) = support::serve_evidence_node(node, "refused-handshake");
    let mut transport = support::connect_evidence_node(&socket_path);
    handshake_gate(&mut gate, &mut transport)
        .unwrap_or_else(|error| panic!("handshake gate: {error:?}"));

    let forged = signed_activity(
        &registry,
        b"did:layerx:actor",
        impostor_key,
        &impostor,
        1,
        0x31,
    );
    assert_eq!(
        submit_signed(&mut transport, &registry, context(impostor_key, 1), &forged),
        Err(SubmitError::CoreRefusal {
            class: 6,
            result: known(KnownResult::BadSignature),
        })
    );
    let unknown = signed_activity(
        &registry,
        b"did:layerx:stranger",
        actor_key,
        &actor,
        1,
        0x32,
    );
    assert_eq!(
        submit_signed(&mut transport, &registry, context(actor_key, 2), &unknown),
        Err(SubmitError::CoreRefusal {
            class: 6,
            result: known(KnownResult::UnknownDid),
        })
    );
    let genuine = signed_activity(&registry, b"did:layerx:actor", actor_key, &actor, 1, 0x33);
    let admitted = submit_signed(&mut transport, &registry, context(actor_key, 3), &genuine)
        .unwrap_or_else(|error| panic!("genuine submission: {error:?}"));
    assert!(matches!(admitted, Submission::Acknowledged(_)));

    drop(transport);
    let node = server
        .join()
        .unwrap_or_else(|_| panic!("evidence node panicked"));
    assert!(!node.fail_stopped());
    assert_eq!(node.authentication_refusals(), 2);
    assert_eq!(node.queued().len(), 1);
    assert_eq!(node.journal().records().len(), 1);
    drop(node);
    assert_eq!(
        AdmissionJournal::recover(&admission_directory, EVIDENCE_NETWORK_ID).len(),
        1
    );
    let _ = std::fs::remove_file(socket_path);
}

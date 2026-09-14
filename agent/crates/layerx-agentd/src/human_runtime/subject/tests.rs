use super::*;
use crate::outbox::{Outbox, SubmissionState};
use layerx_types::activity::{Authority, EnvelopeBuilder, TimestampBound};
use layerx_types::amount::Amount;
use layerx_types::ids::IdempotencyKey;
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, Payload};
use std::fmt::Debug;

fn must<T, E: Debug>(value: Result<T, E>) -> T {
    value.unwrap_or_else(|error| panic!("subject invariant: {error:?}"))
}
fn scoped(principal: &str) -> HumanPeer {
    HumanPeer {
        uid: 7,
        tenant: must(layerx_identity_binding::subject_namespace(
            "service-tenant",
            principal,
        )),
        principal: principal.to_owned(),
        subject: Some(HumanSubject {
            transport_tenant: "service-tenant".to_owned(),
            transport_principal: "human-service".to_owned(),
            owner: format!("did:layerx:{principal}"),
            account: format!("agent:did:layerx:{principal}:main"),
            asset: [3; 32],
            registration: None,
        }),
    }
}

#[test]
fn durable_subject_bindings_preserve_service_identity_and_refuse_remap_after_restart() {
    let root = std::env::temp_dir().join(format!("lxp-subject-bindings-{}", std::process::id()));
    let mut store = must(Store::open(&root));
    let configured =
        BTreeMap::from([(7, ("human-service".to_owned(), "service-tenant".to_owned()))]);
    let alice = scoped("alice");
    let bob = scoped("bob");
    must(retain(&mut store, &alice));
    must(retain(&mut store, &bob));
    must(retain(&mut store, &alice));
    let mut changed = alice.clone();
    changed
        .subject
        .as_mut()
        .unwrap_or_else(|| panic!("scope"))
        .owner = "did:layerx:bob".to_owned();
    assert!(retain(&mut store, &changed).is_err());
    changed = alice.clone();
    changed.tenant = bob.tenant.clone();
    assert!(retain(&mut store, &changed).is_err());
    drop(store);
    let mut store = must(Store::open(&root));
    let restored = must(restore_peers(&store, &configured));
    assert_eq!(restored.len(), 3);
    assert!(restored.contains(&alice));
    assert!(restored.contains(&bob));
    assert!(restore_peers(
        &store,
        &BTreeMap::from([(
            7,
            ("different-service".to_owned(), "service-tenant".to_owned())
        )])
    )
    .is_err());
    let key = must(TenantKey::new(
        must(TenantId::new(alice.tenant.clone())),
        ObjectKind::Configuration,
        KEY,
    ));
    let raw = store
        .get(&key)
        .unwrap_or_else(|| panic!("durable scope"))
        .bytes()
        .to_vec();
    let mut value: serde_json::Value = must(serde_json::from_slice(&raw));
    value["principal"] = serde_json::json!("bob");
    must(store.put_local(key, must(serde_json::to_vec(&value))));
    assert!(restore_peers(&store, &configured).is_err());
}

fn signed_submission() -> crate::sign::VerifiedSubmission {
    let bytes = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../tests/fixtures/authority/provider-subject/onboarding.activity"
    ));
    let kind = must(ActivityType::new(ModuleId::Governance, 1));
    let registry = must(ModuleRegistry::new(&[must(ModuleRegistration::new(
        ModuleId::Governance,
        &[kind],
    ))]));
    let activity = must(layerx_wire::activity::decode_signed(bytes, &registry));
    let payload = must(Payload::new(&registry, kind, activity.payload()));
    let mut builder = EnvelopeBuilder::new();
    must(builder.protocol_version(activity.protocol_version()));
    must(builder.network_id(activity.network_id()));
    must(builder.activity_type(kind));
    must(builder.actor_did(must(Did::new(activity.actor_did()))));
    must(builder.authority(must(Authority::owner(activity.authority()))));
    must(builder.account_sequence(activity.account_sequence()));
    must(builder.timestamp_bound(must(TimestampBound::new(
        activity.timestamp_bound().not_before,
        activity.timestamp_bound().not_after,
    ))));
    must(builder.idempotency_key(IdempotencyKey::new(activity.idempotency_key())));
    must(builder.fee_limit(Amount::from_u128(activity.fee_limit())));
    must(builder.payload_hash(activity.payload_hash()));
    must(builder.payload(payload));
    let envelope = must(builder.build());
    let canonical = must(layerx_wire::activity::encode_unsigned_envelope(&envelope));
    let disclosure = must(crate::prepare::disclose(&canonical, &registry));
    let prepared = crate::prepare::Prepared {
        signing_preimage: *must(layerx_wire::sign::preimage_unsigned(&envelope)).as_bytes(),
        envelope,
        canonical_bytes: canonical,
        observed_head_sequence: 0,
        disclosure: disclosure.disclosure,
        disclosure_digest: disclosure.digest,
        audit: crate::prepare::PreparationAuditEntry {
            idempotency_key: activity.idempotency_key(),
            observed_head_sequence: 0,
            disclosure_digest: disclosure.digest,
        },
    };
    let key: [u8; 32] = must(activity.authority().try_into());
    must(crate::sign::verify_before_submit(
        bytes, &prepared, &key, &registry,
    ))
}

#[test]
fn exact_signed_queue_is_partitioned_before_transmission_and_restores_independently() {
    let root = std::env::temp_dir().join(format!("lxp-subject-outbox-{}", std::process::id()));
    let mut store = must(Store::open(&root));
    let alice = scoped("alice");
    let bob = scoped("bob");
    must(retain(&mut store, &alice));
    must(retain(&mut store, &bob));
    let submission = signed_submission();
    let id = submission.idempotency_key();
    let original = submission.exact_bytes().to_vec();
    let mut outboxes = BTreeMap::<String, Outbox>::new();
    for peer in [&alice, &bob] {
        must(outboxes.entry(peer.tenant.clone()).or_default().enqueue(
            &mut store,
            must(TenantId::new(peer.tenant.clone())),
            id,
            submission.clone(),
        ));
    }
    let sent = must(begin_transmission(
        outboxes
            .get_mut(&alice.tenant)
            .unwrap_or_else(|| panic!("alice queue")),
        &mut store,
        id,
    ));
    assert_eq!(sent, original);
    assert_eq!(
        outboxes[&alice.tenant]
            .status(id)
            .unwrap_or_else(|| panic!("alice status"))
            .state,
        SubmissionState::Submitted
    );
    assert_eq!(
        outboxes[&bob.tenant]
            .status(id)
            .unwrap_or_else(|| panic!("bob status"))
            .state,
        SubmissionState::Queued
    );
    assert!(begin_transmission(
        outboxes
            .get_mut(&alice.tenant)
            .unwrap_or_else(|| panic!("alice queue")),
        &mut store,
        id
    )
    .is_err());
    drop(store);
    let mut store = must(Store::open(&root));
    let mut alice_queue = Outbox::default();
    let mut bob_queue = Outbox::default();
    must(alice_queue.restore(&store, must(TenantId::new(alice.tenant)), id));
    must(bob_queue.restore(&store, must(TenantId::new(bob.tenant)), id));
    assert_eq!(must(alice_queue.exact_signed_bytes(id)), original);
    assert_eq!(must(bob_queue.bytes_for_transmission(id)), original);
    assert_eq!(
        alice_queue
            .status(id)
            .unwrap_or_else(|| panic!("restored alice"))
            .state,
        SubmissionState::Submitted
    );
    assert_eq!(
        bob_queue
            .status(id)
            .unwrap_or_else(|| panic!("restored bob"))
            .state,
        SubmissionState::Queued
    );
    assert_eq!(
        must(begin_transmission(&mut bob_queue, &mut store, id)),
        original
    );
    assert_eq!(
        bob_queue
            .status(id)
            .unwrap_or_else(|| panic!("resumed Bob queue"))
            .state,
        SubmissionState::Submitted
    );
}

use super::{
    checked,
    fixture::{result_code, Fixture},
    scenarios::{now_ms, Session},
    Result,
};
use ed25519_dalek::{Signer as _, SigningKey};
use layerx_crypto::rotation::{
    OwnerRotation, OwnerRotationCommit, OwnerRotationConsent, OwnerRotationState,
};
use layerx_types::activity::{Authority, EnvelopeBuilder, Signature, TimestampBound};
use layerx_types::amount::Amount;
use layerx_types::ids::IdempotencyKey;
use layerx_types::payload::{ActivityType, ModuleRegistry, Payload};
use layerx_types::verify::VerificationLevel;

fn consent(
    registry: &ModuleRegistry,
    owner: OwnerRotationConsent,
    pending: &SigningKey,
    begin: u64,
) -> Result<Vec<u8>> {
    let kind = checked(ActivityType::from_u32(0x0007_0002))?;
    let payload = checked(Payload::new(registry, kind, &checked(owner.payload())?))?;
    let hash = checked(layerx_wire::hash::payload_hash_for(&payload))?;
    let mut builder = EnvelopeBuilder::new();
    checked(
        builder
            .protocol_version(3)
            .and_then(|value| value.network_id(77))
            .and_then(|value| value.activity_type(kind))
            .and_then(|value| value.actor_did(owner.owner))
            .and_then(|value| {
                value.authority(Authority::owner(&pending.verifying_key().to_bytes())?)
            })
            .and_then(|value| value.account_sequence(0))
            .and_then(|value| value.timestamp_bound(TimestampBound::new(begin, owner.expires_at)?))
            .and_then(|value| value.idempotency_key(IdempotencyKey::new(owner.action_key)))
            .and_then(|value| value.fee_limit(Amount::from_u128(0)))
            .and_then(|value| value.payload_hash(hash))
            .and_then(|value| value.payload(payload)),
    )?;
    let unsigned = checked(builder.build())?;
    let preimage = checked(layerx_wire::sign::preimage_unsigned(&unsigned))?;
    let signature = pending.sign(preimage.as_bytes()).to_bytes();
    checked(layerx_wire::activity::encode_signed_envelope(
        &unsigned.attach_signature(checked(Signature::new(&signature))?),
    ))
}

fn rotate(fixture: &mut Fixture, id: u8, next: SigningKey) -> Result<()> {
    checked(fixture.client.reconnect())?;
    let current = checked(fixture.client.preparation_state(&fixture.did, 8300))?;
    let begin = now_ms()?
        .checked_add(10_000)
        .ok_or("rotation begin overflow")?;
    let end = begin
        .checked_add(180_000)
        .ok_or("rotation expiry overflow")?;
    let pending_public_key = next.verifying_key().to_bytes();
    let payload = checked(
        OwnerRotation::Announce {
            owner: fixture.did.clone(),
            pending_public_key,
            begin,
            end,
            effective_sequence: current
                .observed_head_sequence
                .checked_add(3)
                .ok_or("rotation sequence overflow")?,
        }
        .payload(),
    )?;
    let signed = fixture.signed(0x0007_0002, id, payload)?;
    let announced = fixture.submit(signed.exact_bytes())?;
    assert_eq!(result_code(&announced.0)?, 0);
    fixture.finalize(&announced.1)?;
    let header = &announced.1;
    let authorization = layerx_proof::inclusion::SequencerAuthorization::new(
        header.sequencer_id,
        header.public_key,
        header.first_batch_number,
        header.last_batch_number,
    );
    let did = checked(layerx_wire::hash::did_id_for_protocol(&fixture.did, 3))?;
    let identity = checked(fixture.client.module_state(
        7,
        &did,
        VerificationLevel::CHECKPOINT_FINALISED,
        8301,
        authorization,
    ))?;
    let state = checked(OwnerRotationState::decode(
        identity.canonical_bytes(),
        &fixture.did,
    ))?;
    assert_eq!(state.primary_public_key, fixture.public);
    assert_eq!(state.pending_public_key, Some(pending_public_key));
    let consent = consent(
        &fixture.registry,
        OwnerRotationConsent {
            owner: fixture.did.clone(),
            current_public_key: fixture.public,
            pending_public_key,
            announcement: state.announcement,
            action_key: [id + 1; 32],
            expires_at: end,
        },
        &next,
        begin,
    )?;
    let commit = checked(OwnerRotationCommit::from_signed_consent(&consent))?;
    while now_ms()? < begin {
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    let signed = fixture.signed(0x0007_0002, id + 1, checked(commit.payload())?)?;
    let committed = fixture.submit(signed.exact_bytes())?;
    assert_eq!(result_code(&committed.0)?, 0);
    fixture.finalize(&committed.1)?;
    fixture.replace_signer(next);
    Ok(())
}

pub fn run(fixture: &mut Fixture) -> Result<()> {
    let did = checked(layerx_wire::hash::did_id_for_protocol(&fixture.did, 3))?;
    let mut identity = vec![0x71, 1, 0, 2];
    identity.extend_from_slice(&did);
    identity.extend_from_slice(&fixture.public);
    let signed = fixture.signed(0x0007_0001, 0xf0, identity)?;
    let registered = fixture.submit(signed.exact_bytes())?;
    assert_eq!(result_code(&registered.0)?, 0);
    fixture.finalize(&registered.1)?;
    let mut session = Session::open(fixture, 0xf1)?;
    let previous = session.reconcile(fixture)?;
    let raw = checked(layerx_agentd::budget::retrieve_native_budget_evidence(
        &mut fixture.client,
        &fixture.authority,
        &session.scope.binding,
        None,
        &fixture.registry,
    ))?;
    let id = 0x81;
    let exact = session.enqueue(fixture, id, [0xee; 32])?;
    session.transition(id, layerx_agentd::outbox::SubmissionState::Submitted)?;
    fixture.drop_response(&exact)?;
    session.transition(id, layerx_agentd::outbox::SubmissionState::Unknown)?;
    let activity = session
        .outbox
        .status([id; 32])
        .ok_or("unknown missing")?
        .activity_id;
    assert_ne!(result_code(&fixture.receipt(activity)?.0)?, 0);
    rotate(fixture, 0x82, SigningKey::from_bytes(&[0x22; 32]))?;
    rotate(fixture, 0x84, SigningKey::from_bytes(&[0x33; 32]))?;
    session.scope.binding.owner_public_key = fixture.public;
    let raw = checked(layerx_agentd::budget::retrieve_native_budget_evidence(
        &mut fixture.client,
        &fixture.authority,
        &session.scope.binding,
        Some(raw.current),
        &fixture.registry,
    ))?;
    checked(
        fixture
            .authority
            .advance_native_budget(&session.scope.binding, &raw, &previous),
    )?;
    let mut altered = raw.clone();
    altered
        .current
        .owner_history
        .as_mut()
        .ok_or("native owner history missing")?
        .canonical_record[69] ^= 1;
    assert!(fixture
        .authority
        .advance_native_budget(&session.scope.binding, &altered, &previous)
        .is_err());
    let mut missing = raw.clone();
    missing.current.owner_history = None;
    assert!(fixture
        .authority
        .advance_native_budget(&session.scope.binding, &missing, &previous)
        .is_err());
    let mut missing = raw.clone();
    let rotation = missing
        .history
        .iter()
        .position(|entry| {
            layerx_wire::receipt::decode(entry.receipt().canonical_receipt())
                .ok()
                .and_then(|receipt| {
                    receipt.protocol().map(|value| {
                        value
                            .effects()
                            .iter()
                            .any(|effect| effect.event_type() == 0x7142)
                    })
                })
                == Some(true)
        })
        .ok_or("actual rotation history missing")?;
    missing.history.remove(rotation);
    assert!(fixture
        .authority
        .advance_native_budget(&session.scope.binding, &missing, &previous)
        .is_err());
    session = session.restart(fixture)?;
    let current = session.reconcile(fixture)?;
    assert_eq!(current.binding().owner_public_key, fixture.public);
    assert_eq!(
        session
            .outbox
            .status([id; 32])
            .ok_or("terminal missing")?
            .state,
        layerx_agentd::outbox::SubmissionState::Failed
    );
    session.assert_accounting(0, 0)?;
    session = session.restart(fixture)?;
    session.reconcile(fixture)?;
    session.assert_accounting(0, 0)?;
    assert_eq!(checked(session.outbox.exact_signed_bytes([id; 32]))?, exact);
    Ok(())
}

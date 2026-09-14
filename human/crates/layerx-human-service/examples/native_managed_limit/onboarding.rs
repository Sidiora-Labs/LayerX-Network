use super::{
    checked,
    creation::{self, Receipt},
    fixture::Fixture,
    identity::Identity,
    Result,
};
use ed25519_dalek::{Signer as _, SigningKey};
use layerx_crypto::onboarding::{OnboardingConsent, SponsoredRegistration};
use layerx_types::activity::{Authority, EnvelopeBuilder, Signature, TimestampBound};
use layerx_types::{
    amount::Amount,
    ids::IdempotencyKey,
    payload::{ActivityType, Payload},
};

fn consent(fixture: &mut Fixture, identity: &Identity) -> Result<Vec<u8>> {
    checked(fixture.client.reconnect())?;
    let state = checked(fixture.client.preparation_state(&fixture.did, 8590))?;
    let key = SigningKey::from_bytes(&[0x22; 32]);
    let consent = OnboardingConsent {
        sponsor: checked(layerx_wire::hash::did_id_for_protocol(&fixture.did, 3))?,
        target: identity.did.clone(),
        target_public_key: key.verifying_key().to_bytes(),
        native_asset: fixture.asset,
        action_key: [0xb6; 32],
        expires_at: state
            .protocol_timestamp
            .checked_add(300_000)
            .ok_or("consent expiry")?,
    };
    let kind = checked(ActivityType::from_u32(0x0007_0001))?;
    let payload = checked(Payload::new(
        &fixture.registry,
        kind,
        &checked(consent.payload())?,
    ))?;
    let payload_hash = checked(layerx_wire::hash::payload_hash_for(&payload))?;
    let mut builder = EnvelopeBuilder::new();
    checked(
        builder
            .protocol_version(3)
            .and_then(|v| v.network_id(77))
            .and_then(|v| v.activity_type(kind))
            .and_then(|v| v.actor_did(identity.did.clone()))
            .and_then(|v| v.authority(Authority::owner(key.verifying_key().as_bytes())?))
            .and_then(|v| v.account_sequence(0))
            .and_then(|v| {
                v.timestamp_bound(TimestampBound::new(
                    state.protocol_timestamp,
                    consent.expires_at,
                )?)
            })
            .and_then(|v| v.idempotency_key(IdempotencyKey::new([0xb6; 32])))
            .and_then(|v| v.fee_limit(Amount::from_u128(0)))
            .and_then(|v| v.payload_hash(payload_hash))
            .and_then(|v| v.payload(payload)),
    )?;
    let unsigned = checked(builder.build())?;
    let preimage = checked(layerx_wire::sign::preimage_unsigned(&unsigned))?;
    let signature = checked(Signature::new(&key.sign(preimage.as_bytes()).to_bytes()))?;
    checked(layerx_wire::activity::encode_signed_envelope(
        &unsigned.attach_signature(signature),
    ))
}
fn fund(fixture: &mut Fixture, identity: &Identity, registration: &Receipt) -> Result<Receipt> {
    checked(fixture.client.reconnect())?;
    let state = checked(fixture.client.preparation_state(&fixture.did, 8591))?;
    let from = super::fixture::account(&format!(
        "agent:{}:main",
        std::str::from_utf8(fixture.did.as_bytes())?
    ))?;
    let to = super::fixture::account(&format!(
        "agent:{}:main",
        std::str::from_utf8(identity.did.as_bytes())?
    ))?;
    let account = checked(fixture.client.account(
        from,
        layerx_types::verify::VerificationLevel::CHECKPOINT_FINALISED,
        8592,
        creation::authorization(&registration.header),
    ))?;
    let source = checked(layerx_proof::state::decode_account_value(
        from,
        account.canonical_bytes(),
    ))?;
    let amount = source
        .balance()
        .checked_div(2)
        .filter(|v| *v > 100)
        .ok_or("owner fixture funding unavailable")?;
    let debit = layerx_crypto::send::SendDebit {
        from,
        to,
        asset: fixture.asset,
        amount,
        source_sequence: source.next_sequence,
        idempotency_key: [0xb7; 32],
        expires_at: state
            .protocol_timestamp
            .checked_add(30_000)
            .ok_or("funding expiry")?,
        context_hash: layerx_crypto::send::send_context_hash(
            &from,
            &to,
            &fixture.asset,
            amount,
            &[0xb7; 32],
        ),
        conditions: vec![],
        authorization_kind: 1,
        network_id: 77,
        protocol_version: 3,
    };
    let canonical = checked(debit.authorization_message())?;
    let message = checked(layerx_crypto::SignatureMessage::new(
        layerx_wire::hash::Domain::SignaturePreimage,
        3,
        77,
        &canonical,
    ))?;
    let key = SigningKey::from_bytes(&[0x11; 32]);
    let signature = key.sign(&message.digest()).to_bytes();
    let payload = checked(debit.encode_signed(fixture.public, signature))?;
    let envelope = checked(layerx_crypto::send::encode_send_envelope(
        &payload,
        &layerx_crypto::send::EnvelopeOptions {
            actor: std::str::from_utf8(fixture.did.as_bytes())?,
            public_key: fixture.public,
            protocol_version: 3,
            network_id: 77,
            identity_sequence: state.account_sequence,
            idempotency_key: [0xb7; 32],
            fee_limit: source
                .balance()
                .checked_sub(amount)
                .ok_or("funding fee reservation")?,
            not_before: state.protocol_timestamp,
            not_after: debit.expires_at,
        },
    ))?;
    let preimage = checked(layerx_wire::sign::preimage_unsigned(&envelope.envelope))?;
    let kind = envelope.envelope.activity_type().value();
    let signature = checked(Signature::new(&key.sign(preimage.as_bytes()).to_bytes()))?;
    let exact = checked(layerx_wire::activity::encode_signed_envelope(
        &envelope.envelope.attach_signature(signature),
    ))?;
    let (receipt, header) = fixture.submit(&exact)?;
    assert_eq!(super::fixture::result_code(&receipt)?, 0);
    fixture.finalize(&header)?;
    Ok(Receipt {
        signed: exact,
        kind,
        bytes: receipt,
        header,
    })
}
pub struct Bound {
    pub registration: Receipt,
    pub funding: Receipt,
}
pub fn bind(fixture: &mut Fixture, identity: &Identity) -> Result<Bound> {
    let consent = consent(fixture, identity)?;
    let registration = checked(SponsoredRegistration::from_signed_consent(&consent))?;
    let receipt = creation::submit(fixture, 0x0007_0001, 0xb6, checked(registration.payload())?)?;
    let funding = fund(fixture, identity, &receipt)?;
    fixture.did = identity.did.clone();
    fixture.replace_signer(SigningKey::from_bytes(&[0x22; 32]));
    Ok(Bound {
        registration: receipt,
        funding,
    })
}

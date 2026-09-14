use super::{
    checked, encoded_disclosure, facts, registry, request, setup_envelope, signing_request, Host,
    Result,
};
use layerx_crypto::disclosure::{
    bind, DisclosedNativeBudgetCreate, DisclosedNativeOperation, Disclosure, DisclosureError,
};
use layerx_crypto::onboarding::{OnboardingConsent, SponsoredRegistration};
use layerx_types::account::AccountId;
use layerx_types::activity::{Signature, UnsignedEnvelope};
use layerx_types::ids::Did;
use layerx_types::payload::{ActivityType, ModuleId, Payload};
use layerx_wire::activity::{encode_signed_envelope, encode_unsigned_envelope};
use layerx_wire::encode::Encoder;
use layerx_wire::hash;

fn account(name: &str) -> Result<[u8; 32]> {
    checked(hash::account_id_for_protocol(
        &checked(AccountId::parse(name))?,
        3,
    ))
}

fn payload(module: ModuleId, ordinal: u16, bytes: &[u8]) -> Result<Payload> {
    checked(Payload::new(
        &registry()?,
        checked(ActivityType::new(module, ordinal))?,
        bytes,
    ))
}

fn native_budget(version: u16) -> Result<Vec<u8>> {
    let mut encoded = Encoder::new(251);
    checked(encoded.u16(version))?;
    checked(encoded.fixed(&[8; 32]))?;
    checked(encoded.fixed(&account(&format!(
        "agent:did:layerx:alice:budget:{}",
        "08".repeat(32)
    ))?))?;
    checked(encoded.fixed(&[3; 32]))?;
    checked(encoded.fixed(&[9; 32]))?;
    for amount in [100, 0, 25] {
        checked(encoded.u128(amount))?;
    }
    for number in [100, 1000, 2000, 3] {
        checked(encoded.u64(number))?;
    }
    checked(encoded.u8(1))?;
    if version == 2 {
        checked(encoded.fixed(&account(&format!(
            "agent:did:layerx:alice:asset:{}",
            "03".repeat(32)
        ))?))?;
        checked(encoded.u64(2))?;
    }
    Ok(encoded.finish())
}

fn recovery(extended: bool) -> Result<Vec<u8>> {
    let mut encoded = Encoder::new(86);
    checked(encoded.fixed(&[0x71, 3, 0, if extended { 5 } else { 3 }]))?;
    checked(encoded.fixed(&checked(hash::did_id_for_protocol(
        &checked(Did::new(b"did:layerx:alice"))?,
        3,
    ))?))?;
    checked(encoded.fixed(&[7; 32]))?;
    checked(encoded.u16(2))?;
    if extended {
        checked(encoded.u64(100))?;
        checked(encoded.u64(500))?;
    }
    Ok(encoded.finish())
}

fn mutated_native(disclosure: &Disclosure) -> Vec<Disclosure> {
    let mutations: &[fn(&mut DisclosedNativeBudgetCreate)] = &[
        |v| v.encoding_version ^= 1,
        |v| v.budget_id[0] ^= 1,
        |v| v.budget_account[0] ^= 1,
        |v| v.asset[0] ^= 1,
        |v| v.purpose[0] ^= 1,
        |v| v.per_period_limit += 1,
        |v| v.carry_cap += 1,
        |v| v.initial_amount += 1,
        |v| v.period_length_ms += 1,
        |v| v.period_start_ms += 1,
        |v| v.expiry_ms += 1,
        |v| v.revocation_sequence += 1,
        |v| v.rollover ^= 1,
        |v| v.source_account[0] ^= 1,
        |v| v.source_sequence += 1,
    ];
    let count = match &disclosure.native_operation {
        Some(DisclosedNativeOperation::BudgetCreate(_)) => mutations.len(),
        Some(DisclosedNativeOperation::RecoveryPolicy(_)) => 4,
        Some(DisclosedNativeOperation::OwnerRotation(_)) => 0,
        None => 0,
    };
    (0..count)
        .map(|field| {
            let mut changed = disclosure.clone();
            match changed.native_operation.as_mut() {
                Some(DisclosedNativeOperation::BudgetCreate(value)) => mutations[field](value),
                Some(DisclosedNativeOperation::RecoveryPolicy(value)) => match field {
                    0 => value.did_id[0] ^= 1,
                    1 => value.recovery_root[0] ^= 1,
                    2 => value.threshold += 1,
                    _ => value.delay_bounds = Some((1, 2)),
                },
                Some(DisclosedNativeOperation::OwnerRotation(_)) => {
                    panic!("unexpected rotation disclosure")
                }
                None => panic!("missing native disclosure"),
            }
            changed
        })
        .collect()
}

fn sign_setup(
    host: &Host,
    (binding, handle, public): ([u8; 32], &[u8], [u8; 32]),
    envelope: &UnsignedEnvelope,
) -> Result<[u8; 64]> {
    let canonical = checked(encode_unsigned_envelope(envelope))?;
    let disclosure = checked(bind(&canonical, &registry()?))?;
    let encoded = encoded_disclosure(&disclosure)?;
    let (request, digest) = signing_request(binding, handle, &canonical, &encoded)?;
    let response = host.call(&request)?;
    assert_eq!(response[7], 0);
    assert_eq!(response.len(), 72);
    checked(
        ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, public)
            .verify(&digest, &response[8..]),
    )?;
    for changed in mutated_native(&disclosure) {
        assert_eq!(
            changed.reencode(),
            Err(DisclosureError::FieldMismatch("native_operation"))
        );
        assert_eq!(
            host.call(
                &signing_request(binding, handle, &canonical, &encoded_disclosure(&changed)?)?.0
            )?[7],
            1
        );
    }
    if disclosure.onboarding.is_some() {
        assert_eq!(encoded[0], 4);
        let mut changed = encoded.clone();
        let last = changed.len() - 1;
        changed[last] ^= 1;
        assert_eq!(
            host.call(&signing_request(binding, handle, &canonical, &changed)?.0)?[7],
            1
        );
    }
    Ok(response[8..].try_into()?)
}

#[test]
fn native_creation_disclosures_sign_through_real_kms_before_and_after_restart() -> Result<()> {
    let mut host = Host::new()?;
    let target_binding = [75; 32];
    let sponsor_binding = [76; 32];
    let (target_handle, target_public) =
        facts(&host.call(&request(1, target_binding, &[], None)?)?)?;
    let (sponsor_handle, sponsor_public) =
        facts(&host.call(&request(1, sponsor_binding, &[], None)?)?)?;
    let target = (target_binding, target_handle.as_slice(), target_public);
    let sponsor = (sponsor_binding, sponsor_handle.as_slice(), sponsor_public);
    let consent = OnboardingConsent {
        sponsor: checked(hash::did_id_for_protocol(
            &checked(Did::new(b"did:layerx:sponsor"))?,
            3,
        ))?,
        target: checked(Did::new(b"did:layerx:alice"))?,
        target_public_key: target_public,
        native_asset: [3; 32],
        action_key: [4; 32],
        expires_at: 1010,
    };
    let target_envelope = setup_envelope(
        target_public,
        77,
        payload(ModuleId::Governance, 1, &checked(consent.payload())?)?,
        (b"did:layerx:alice", 0, 0),
    )?;
    let signature = sign_setup(&host, target, &target_envelope)?;
    let signed = checked(encode_signed_envelope(
        &target_envelope
            .clone()
            .attach_signature(checked(Signature::new(&signature))?),
    ))?;
    let registration = checked(SponsoredRegistration::from_signed_consent(&signed))?;
    let sponsor_envelope = setup_envelope(
        sponsor_public,
        77,
        payload(ModuleId::Governance, 1, &checked(registration.payload())?)?,
        (b"did:layerx:sponsor", 7, 2),
    )?;
    let mut target_operations = vec![target_envelope];
    for extended in [false, true] {
        target_operations.push(setup_envelope(
            target_public,
            77,
            payload(ModuleId::Governance, 3, &recovery(extended)?)?,
            (b"did:layerx:alice", 7, 1),
        )?);
    }
    for version in [1, 2] {
        target_operations.push(setup_envelope(
            target_public,
            77,
            payload(ModuleId::Budget, 1, &native_budget(version)?)?,
            (b"did:layerx:alice", 7, 1),
        )?);
    }
    let opening = checked(
        layerx_crypto::payments::Payment::OpenAccount { asset: [3; 32] }
            .encode(b"did:layerx:alice"),
    )?;
    target_operations.push(setup_envelope(
        target_public,
        77,
        payload(ModuleId::Asset, 4, &opening)?,
        (b"did:layerx:alice", 7, 1),
    )?);
    let mut first = Vec::new();
    for pass in 0..2 {
        if pass == 1 {
            host.stop();
            host.start()?;
        }
        let mut signatures = vec![sign_setup(&host, sponsor, &sponsor_envelope)?];
        for envelope in &target_operations {
            signatures.push(sign_setup(&host, target, envelope)?);
        }
        if pass == 0 {
            first = signatures;
        } else {
            assert_eq!(signatures, first);
        }
    }
    Ok(())
}

#[test]
fn native_setup_refuses_wrong_accounts_limits_and_payload_shapes() -> Result<()> {
    use layerx_crypto::signer::Signer as _;
    let public = layerx_crypto::local::LocalSigner::new([0x62; 32]).public_key();
    for version in [1, 2] {
        let original = native_budget(version)?;
        let mut corruptions = Vec::new();
        for offset in [2, 34] {
            let mut changed = original.clone();
            changed[offset] ^= 1;
            corruptions.push(changed);
        }
        for (start, length) in [(130, 16), (162, 16), (178, 8), (194, 8), (210, 1)] {
            let mut changed = original.clone();
            changed[start..start + length].fill(0);
            corruptions.push(changed);
        }
        if version == 2 {
            let mut changed = original.clone();
            changed[211] ^= 1;
            corruptions.push(changed);
            let mut changed = original.clone();
            changed[243..].fill(255);
            corruptions.push(changed);
            let mut changed = original.clone();
            changed[66] ^= 1;
            corruptions.push(changed);
        }
        let mut trailing = original;
        trailing.push(0);
        corruptions.push(trailing);
        for bytes in corruptions {
            let envelope = setup_envelope(
                public,
                77,
                payload(ModuleId::Budget, 1, &bytes)?,
                (b"did:layerx:alice", 7, 1),
            )?;
            assert!(bind(&checked(encode_unsigned_envelope(&envelope))?, &registry()?).is_err());
        }
    }
    for (start, length) in [(4, 32), (36, 32), (68, 2)] {
        let mut bytes = recovery(false)?;
        bytes[start..start + length].fill(0);
        let envelope = setup_envelope(
            public,
            77,
            payload(ModuleId::Governance, 3, &bytes)?,
            (b"did:layerx:alice", 7, 1),
        )?;
        assert!(bind(&checked(encode_unsigned_envelope(&envelope))?, &registry()?).is_err());
    }
    let consent = OnboardingConsent {
        sponsor: checked(hash::did_id_for_protocol(
            &checked(Did::new(b"did:layerx:sponsor"))?,
            3,
        ))?,
        target: checked(Did::new(b"did:layerx:alice"))?,
        target_public_key: public,
        native_asset: [3; 32],
        action_key: [4; 32],
        expires_at: 1010,
    };
    for (sequence, fee) in [(1, 0), (0, 1)] {
        let envelope = setup_envelope(
            public,
            77,
            payload(ModuleId::Governance, 1, &checked(consent.payload())?)?,
            (b"did:layerx:alice", sequence, fee),
        )?;
        assert!(bind(&checked(encode_unsigned_envelope(&envelope))?, &registry()?).is_err());
    }
    Ok(())
}

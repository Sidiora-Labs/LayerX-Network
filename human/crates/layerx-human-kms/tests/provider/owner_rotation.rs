use super::{checked, encoded_disclosure, facts, registry, request, signing_request, Host, Result};
use layerx_crypto::disclosure::{bind, DisclosedNativeOperation};
use layerx_crypto::onboarding::OnboardingConsent;
use layerx_crypto::rotation::{
    OwnerRotation, OwnerRotationCommit, OwnerRotationConsent, OwnerRotationState,
};
use layerx_crypto::signer::Signer as _;
use layerx_types::activity::{Authority, EnvelopeBuilder, Signature, TimestampBound};
use layerx_types::amount::Amount;
use layerx_types::ids::{Did, IdempotencyKey};
use layerx_types::payload::{ActivityType, ModuleId, Payload};
use layerx_wire::activity::{encode_signed_envelope, encode_unsigned_envelope};
use layerx_wire::hash;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

struct Key {
    binding: [u8; 32],
    handle: [u8; 32],
    public: [u8; 32],
}

impl Key {
    fn create(host: &Host, binding: u8) -> Result<Self> {
        let binding = [binding; 32];
        let (handle, public) = facts(&host.call(&request(1, binding, &[], None)?)?)?;
        Ok(Self {
            binding,
            handle,
            public,
        })
    }
}

#[derive(Clone, Copy)]
struct Envelope {
    sequence: u64,
    action: [u8; 32],
    begin: u64,
    end: u64,
    fee: u128,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn bytes(value: &str) -> Result<Vec<u8>> {
    if !value.len().is_multiple_of(2) {
        return Err("odd public encoding".into());
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| Ok(u8::from_str_radix(std::str::from_utf8(pair)?, 16)?))
        .collect()
}

fn did(key: &Key) -> Result<Did> {
    checked(Did::new(
        format!("did:layerx:{}", hex(&key.public)).as_bytes(),
    ))
}

fn payload(ordinal: u16, value: &[u8]) -> Result<Payload> {
    checked(Payload::new(
        &registry()?,
        checked(ActivityType::new(ModuleId::Governance, ordinal))?,
        value,
    ))
}

fn sign(
    host: &Host,
    key: &Key,
    owner: &Did,
    payload: Payload,
    values: Envelope,
) -> Result<Vec<u8>> {
    let mut builder = EnvelopeBuilder::new();
    checked(builder.protocol_version(3))?;
    checked(builder.network_id(77))?;
    checked(builder.activity_type(payload.activity_type()))?;
    checked(builder.actor_did(owner.clone()))?;
    checked(builder.authority(checked(Authority::owner(&key.public))?))?;
    checked(builder.account_sequence(values.sequence))?;
    checked(builder.timestamp_bound(checked(TimestampBound::new(values.begin, values.end))?))?;
    checked(builder.idempotency_key(IdempotencyKey::new(values.action)))?;
    checked(builder.fee_limit(Amount::from_u128(values.fee)))?;
    checked(builder.payload_hash(checked(hash::payload_hash_for(&payload))?))?;
    checked(builder.payload(payload))?;
    let envelope = checked(builder.build())?;
    let canonical = checked(encode_unsigned_envelope(&envelope))?;
    let disclosure = checked(bind(&canonical, &registry()?))?;
    let encoded = encoded_disclosure(&disclosure)?;
    let (request, digest) = signing_request(key.binding, &key.handle, &canonical, &encoded)?;
    let response = host.call(&request)?;
    assert_eq!(response.len(), 72);
    assert_eq!(response[7], 0);
    checked(
        ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, key.public)
            .verify(&digest, &response[8..]),
    )?;
    let mut corrupted = encoded;
    let last = corrupted.len() - 1;
    corrupted[last] ^= 1;
    assert_eq!(
        host.call(&signing_request(key.binding, &key.handle, &canonical, &corrupted)?.0)?[7],
        1
    );
    checked(encode_signed_envelope(
        &envelope.attach_signature(checked(Signature::new(&response[8..]))?),
    ))
}

fn rotation(host: &Host, key: &Key, value: &OwnerRotation, envelope: Envelope) -> Result<Vec<u8>> {
    let intent = layerx_intents::Intent::v3(layerx_intents::IntentKind::NativeOwnerRotation(
        value.clone(),
    ));
    let compiled = checked(layerx_intents::compile(&intent, &registry()?))?;
    assert_eq!(compiled.payload().as_bytes(), checked(value.payload())?);
    sign(
        host,
        key,
        value.owner(),
        compiled.payload().clone(),
        envelope,
    )
}

fn provider_cases(host: &Host, old: &Key, next: &Key) -> Result<Vec<Vec<u8>>> {
    let owner = did(old)?;
    let envelope = Envelope {
        sequence: 7,
        action: [61; 32],
        begin: 1000,
        end: 1010,
        fee: 4,
    };
    let consent = OwnerRotationConsent {
        owner: owner.clone(),
        current_public_key: old.public,
        pending_public_key: next.public,
        announcement: [62; 32],
        action_key: envelope.action,
        expires_at: envelope.end,
    };
    let signed = rotation(
        host,
        next,
        &OwnerRotation::Consent(consent.clone()),
        Envelope {
            sequence: 0,
            fee: 0,
            ..envelope
        },
    )?;
    let commit = checked(OwnerRotationCommit::from_signed_consent(&signed))?;
    assert_eq!(commit.consent, consent);
    let mut result = vec![signed];
    for value in [
        OwnerRotation::Announce {
            owner: owner.clone(),
            pending_public_key: next.public,
            begin: 1005,
            end: 1009,
            effective_sequence: 20,
        },
        OwnerRotation::Commit(commit),
        OwnerRotation::Cancel {
            owner,
            announcement: [62; 32],
        },
    ] {
        let signed = rotation(host, old, &value, envelope)?;
        let decoded = checked(layerx_wire::activity::decode_signed(&signed, &registry()?))?;
        assert_eq!(checked(OwnerRotation::from_activity(&decoded))?, value);
        let unsigned = checked(layerx_wire::activity::encode_unsigned(&decoded))?;
        let disclosed = checked(bind(&unsigned, &registry()?))?;
        assert_eq!(
            disclosed.native_operation,
            Some(DisclosedNativeOperation::OwnerRotation(Box::new(value)))
        );
        result.push(signed);
    }
    let signed_consent = &result[0];
    for index in [0, signed_consent.len() / 2, signed_consent.len() - 1] {
        let mut changed = signed_consent.clone();
        changed[index] ^= 1;
        assert!(OwnerRotationCommit::from_signed_consent(&changed).is_err());
    }
    let mut trailing = signed_consent.clone();
    trailing.push(0);
    assert!(OwnerRotationCommit::from_signed_consent(&trailing).is_err());
    Ok(result)
}

struct Native {
    socket: PathBuf,
    driver: PathBuf,
    prefix: PathBuf,
}

impl Native {
    fn environment() -> Result<Self> {
        let get = |name| -> Result<PathBuf> {
            Ok(PathBuf::from(
                std::env::var_os(format!("LAYERX_TEST_OWNER_ROTATION_{name}"))
                    .ok_or("native rotation requires socket, driver and state")?,
            ))
        };
        Ok(Self {
            socket: get("SOCKET")?,
            driver: get("DRIVER")?,
            prefix: get("STATE")?,
        })
    }
    fn path(&self, suffix: &str) -> PathBuf {
        let mut path = self.prefix.as_os_str().to_os_string();
        path.push(suffix);
        PathBuf::from(path)
    }
    fn write(&self, suffix: &str, data: &[u8]) -> Result<PathBuf> {
        let path = self.path(suffix);
        fs::write(&path, data)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644))?;
        Ok(path)
    }
    fn invoke(&self, arguments: &[&std::ffi::OsStr]) -> Result<()> {
        let status = Command::new(&self.driver)
            .arg(&self.socket)
            .arg(&self.prefix)
            .args(arguments)
            .status()?;
        assert!(
            status.success(),
            "native owner rotation driver failed: {status}"
        );
        Ok(())
    }
    fn state(&self, owner: &Did) -> Result<(serde_json::Value, OwnerRotationState)> {
        let value: serde_json::Value = serde_json::from_slice(&fs::read(self.path(".json"))?)?;
        assert_eq!(value["network_id"], 77);
        assert_eq!(value["protocol_version"], 3);
        assert_eq!(
            bytes(value["did_hex"].as_str().ok_or("missing DID")?)?,
            owner.as_bytes()
        );
        let identity = bytes(value["identity"].as_str().ok_or("missing identity")?)?;
        let state = checked(OwnerRotationState::decode(&identity, owner))?;
        assert_eq!(
            bytes(value["announcement"].as_str().ok_or("missing commitment")?)?,
            state.announcement
        );
        Ok((value, state))
    }
    fn apply(&self, name: &str, signed: &[u8], expected: i32) -> Result<()> {
        let path = self.write(name, signed)?;
        self.invoke(&[
            "--apply".as_ref(),
            path.as_os_str(),
            expected.to_string().as_ref(),
        ])
    }
    fn envelope(&self, owner: &Did, action: u8) -> Result<Envelope> {
        let (value, _) = self.state(owner)?;
        let begin = now_ms()?;
        Ok(Envelope {
            sequence: value["activity_sequence"]
                .as_u64()
                .ok_or("missing activity sequence")?,
            action: [action; 32],
            begin,
            end: begin.checked_add(300_000).ok_or("clock overflow")?,
            fee: 4,
        })
    }
}

fn now_ms() -> Result<u64> {
    Ok(u64::try_from(
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
    )?)
}

fn native_onboard(host: &Host, old: &Key, native: &Native) -> Result<Did> {
    let owner = did(old)?;
    let sponsor_public = layerx_crypto::local::LocalSigner::new([0x11; 32]).public_key();
    let sponsor = checked(Did::new(
        format!("did:layerx:{}", hex(&sponsor_public)).as_bytes(),
    ))?;
    let begin = now_ms()?;
    let end = begin.checked_add(300_000).ok_or("clock overflow")?;
    let consent = OnboardingConsent {
        sponsor: checked(hash::did_id_for_protocol(&sponsor, 3))?,
        target: owner.clone(),
        target_public_key: old.public,
        native_asset: bytes("b5a32b12029f8ddfb905f90f280f664b46390de0fc62770fc197dd87b18cd898")?
            .try_into()
            .map_err(|_| "asset encoding")?,
        action_key: [71; 32],
        expires_at: end,
    };
    let signed = sign(
        host,
        old,
        &owner,
        payload(1, &checked(consent.payload())?)?,
        Envelope {
            sequence: 0,
            action: consent.action_key,
            begin,
            end,
            fee: 0,
        },
    )?;
    let path = native.write(".onboard.activity", &signed)?;
    native.invoke(&["--prepare".as_ref(), path.as_os_str()])?;
    let (value, state) = native.state(&owner)?;
    assert_eq!(state.primary_public_key, old.public);
    assert_eq!(state.pending_public_key, None);
    assert_eq!(value["balance_hi"], 0);
    assert_eq!(value["balance_lo"], 200_000_000);
    Ok(owner)
}

fn native_commit(host: &Host, old: &Key, next: &Key, native: &Native, owner: &Did) -> Result<()> {
    let (value, _) = native.state(owner)?;
    let begin = now_ms()?.checked_add(15_000).ok_or("clock overflow")?;
    let end = begin.checked_add(60_000).ok_or("clock overflow")?;
    let effective_sequence = value["global_sequence"]
        .as_u64()
        .ok_or("missing sequence")?
        .checked_add(2)
        .ok_or("sequence overflow")?;
    let announce = OwnerRotation::Announce {
        owner: owner.clone(),
        pending_public_key: next.public,
        begin,
        end,
        effective_sequence,
    };
    native.apply(
        ".announce.activity",
        &rotation(host, old, &announce, native.envelope(owner, 72)?)?,
        0,
    )?;
    let (_, state) = native.state(owner)?;
    assert_eq!(state.primary_public_key, old.public);
    assert_eq!(state.pending_public_key, Some(next.public));
    let envelope = native.envelope(owner, 73)?;
    let commit = consent_commit(host, old, next, owner, state.announcement, envelope)?;
    native.apply(
        ".early.activity",
        &rotation(host, old, &commit, envelope)?,
        -304,
    )?;
    let (_, refused) = native.state(owner)?;
    assert_eq!(refused, state);
    while now_ms()? < begin {
        std::thread::sleep(Duration::from_millis(20));
    }
    let envelope = native.envelope(owner, 74)?;
    let mut wrong_announcement = state.announcement;
    wrong_announcement[0] ^= 1;
    let wrong = consent_commit(host, old, next, owner, wrong_announcement, envelope)?;
    native.apply(
        ".wrong-announcement.activity",
        &rotation(host, old, &wrong, envelope)?,
        -204,
    )?;
    assert_eq!(native.state(owner)?.1, state);
    let envelope = native.envelope(owner, 75)?;
    let commit = consent_commit(host, old, next, owner, state.announcement, envelope)?;
    let signed = rotation(host, old, &commit, envelope)?;
    native.apply(".commit.activity", &signed, 0)?;
    let (value, committed) = native.state(owner)?;
    assert_eq!(committed.primary_public_key, next.public);
    assert_eq!(committed.pending_public_key, None);
    assert_eq!(
        committed.revocation_sequence,
        value["global_sequence"]
            .as_u64()
            .ok_or("missing sequence")?
    );
    assert!(committed.revocation_sequence > state.revocation_sequence);
    native.invoke(&[
        "--replay".as_ref(),
        native.path(".commit.activity").as_os_str(),
    ])?;
    assert_eq!(native.state(owner)?.1, committed);
    Ok(())
}

fn consent_commit(
    host: &Host,
    old: &Key,
    next: &Key,
    owner: &Did,
    announcement: [u8; 32],
    envelope: Envelope,
) -> Result<OwnerRotation> {
    let consent = OwnerRotationConsent {
        owner: owner.clone(),
        current_public_key: old.public,
        pending_public_key: next.public,
        announcement,
        action_key: envelope.action,
        expires_at: envelope.end,
    };
    let signed = rotation(
        host,
        next,
        &OwnerRotation::Consent(consent),
        Envelope {
            sequence: 0,
            fee: 0,
            ..envelope
        },
    )?;
    Ok(OwnerRotation::Commit(checked(
        OwnerRotationCommit::from_signed_consent(&signed),
    )?))
}

fn recovery_payload(owner: &Did) -> Result<Payload> {
    let mut value = vec![0x71, 3, 0, 3];
    value.extend(checked(hash::did_id_for_protocol(owner, 3))?);
    value.extend([91; 32]);
    value.extend(2_u16.to_be_bytes());
    payload(3, &value)
}

fn native_recovery(host: &Host, old: &Key, next: &Key, native: &Native, owner: &Did) -> Result<()> {
    let envelope = native.envelope(owner, 76)?;
    let retired = native.write(
        ".retired-live.activity",
        &sign(host, old, owner, recovery_payload(owner)?, envelope)?,
    )?;
    native.invoke(&[
        "--refuse".as_ref(),
        retired.as_os_str(),
        "6".as_ref(),
        "-201".as_ref(),
    ])?;
    let signed = sign(host, next, owner, recovery_payload(owner)?, envelope)?;
    native.apply(".replacement.activity", &signed, 0)?;
    let (_, state) = native.state(owner)?;
    assert_eq!(state.primary_public_key, next.public);
    native_cancel(host, old, next, native, owner)?;
    let envelope = native.envelope(owner, 80)?;
    native.write(
        ".recovered.activity",
        &sign(host, next, owner, recovery_payload(owner)?, envelope)?,
    )?;
    native.write(
        ".retired.activity",
        &sign(host, old, owner, recovery_payload(owner)?, envelope)?,
    )?;
    native.write(".retired-error.json", b"{\"class\":6,\"code\":-201}\n")?;
    Ok(())
}

fn native_cancel(host: &Host, old: &Key, next: &Key, native: &Native, owner: &Did) -> Result<()> {
    let (value, _) = native.state(owner)?;
    let begin = now_ms()?.checked_add(3000).ok_or("clock overflow")?;
    let end = begin.checked_add(3000).ok_or("clock overflow")?;
    let effective_sequence = value["global_sequence"]
        .as_u64()
        .ok_or("missing sequence")?
        .checked_add(2)
        .ok_or("sequence overflow")?;
    let announce = OwnerRotation::Announce {
        owner: owner.clone(),
        pending_public_key: old.public,
        begin,
        end,
        effective_sequence,
    };
    native.apply(
        ".cancel-announce.activity",
        &rotation(host, next, &announce, native.envelope(owner, 77)?)?,
        0,
    )?;
    let (_, state) = native.state(owner)?;
    assert_eq!(state.pending_public_key, Some(old.public));
    let cancel = OwnerRotation::Cancel {
        owner: owner.clone(),
        announcement: state.announcement,
    };
    native.apply(
        ".premature-cancel.activity",
        &rotation(host, next, &cancel, native.envelope(owner, 78)?)?,
        -204,
    )?;
    assert_eq!(native.state(owner)?.1, state);
    while now_ms()? <= end {
        std::thread::sleep(Duration::from_millis(20));
    }
    native.apply(
        ".cancel.activity",
        &rotation(host, next, &cancel, native.envelope(owner, 79)?)?,
        0,
    )?;
    let (_, cancelled) = native.state(owner)?;
    assert_eq!(cancelled.primary_public_key, next.public);
    assert_eq!(cancelled.pending_public_key, None);
    assert_eq!(cancelled.revocation_sequence, state.revocation_sequence);
    assert_eq!(
        (cancelled.begin, cancelled.end, cancelled.effective_sequence),
        (0, 0, 0)
    );
    Ok(())
}

#[test]
fn kms_owner_rotation_disclosure() -> Result<()> {
    let mut host = Host::new()?;
    let old = Key::create(&host, 81)?;
    let next = Key::create(&host, 82)?;
    assert_ne!(old.public, next.public);
    let original = provider_cases(&host, &old, &next)?;
    host.stop();
    host.start()?;
    assert_eq!(provider_cases(&host, &old, &next)?, original);
    match std::env::var("LAYERX_TEST_OWNER_ROTATION_MODE") {
        Ok(mode) if mode == "native" => {
            let native = Native::environment()?;
            let owner = native_onboard(&host, &old, &native)?;
            native_commit(&host, &old, &next, &native, &owner)?;
            host.stop();
            host.start()?;
            native_recovery(&host, &old, &next, &native, &owner)?;
            println!("real KMS disclosure, native cutover and replacement execution passed; signed native restart probes retained");
        }
        Err(std::env::VarError::NotPresent) => {
            for name in ["SOCKET", "DRIVER", "STATE"] {
                assert!(
                    std::env::var_os(format!("LAYERX_TEST_OWNER_ROTATION_{name}")).is_none(),
                    "native inputs require explicit native mode"
                );
            }
            println!(
                "real KMS owner rotation disclosure, signature binding and KMS restart passed"
            );
        }
        _ => return Err("unsupported owner rotation qualification mode".into()),
    }
    Ok(())
}

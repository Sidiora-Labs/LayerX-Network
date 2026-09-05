use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistry};
use layerx_types::program_lifecycle::{
    NativeProgramDeploy, NativeProgramUpgrade, NativeProgramWindDown,
};
use sha2::{Digest, Sha256};

pub(crate) fn ordinal(path: &str) -> Option<u16> {
    match path {
        "/v1/programs/deploy" => Some(1),
        "/v1/programs/upgrade" => Some(2),
        "/v1/programs/wind-down" => Some(7),
        _ => None,
    }
}

pub(crate) fn validate_payload(ordinal: u16, payload: &[u8]) -> Result<[u8; 32], String> {
    let invalid = |_| "invalid canonical program lifecycle payload".to_owned();
    let (program_id, encoded) = match ordinal {
        1 => {
            let value = NativeProgramDeploy::decode(payload).map_err(invalid)?;
            if <[u8; 32]>::from(Sha256::digest(value.wasm)) != value.new_hash {
                return Err("program Wasm hash mismatch".to_owned());
            }
            (value.program_id.bytes(), value.encode().map_err(invalid)?)
        }
        2 => {
            let value = NativeProgramUpgrade::decode(payload).map_err(invalid)?;
            if <[u8; 32]>::from(Sha256::digest(value.wasm)) != value.new_hash {
                return Err("program Wasm hash mismatch".to_owned());
            }
            (value.program_id.bytes(), value.encode().map_err(invalid)?)
        }
        7 => {
            let value = NativeProgramWindDown::decode(payload).map_err(invalid)?;
            (value.program_id.bytes(), value.encode().map_err(invalid)?)
        }
        _ => return Err("unsupported program lifecycle ordinal".to_owned()),
    };
    if encoded != payload {
        return Err("noncanonical program lifecycle payload".to_owned());
    }
    Ok(program_id)
}

pub(crate) fn validate(
    canonical: &[u8],
    registry: &ModuleRegistry,
    expected_ordinal: u16,
) -> Result<[u8; 32], String> {
    if canonical.is_empty() || canonical.len() > 1_048_576 {
        return Err("signed program activity exceeds its bound".to_owned());
    }
    let activity = layerx_wire::activity::decode_signed(canonical, registry)
        .map_err(|_| "invalid signed program activity".to_owned())?;
    if layerx_wire::activity::encode_signed(&activity)
        .map_err(|_| "invalid canonical program activity".to_owned())?
        != canonical
    {
        return Err("noncanonical signed program activity".to_owned());
    }
    let expected = ActivityType::new(ModuleId::Programs, expected_ordinal)
        .map_err(|_| "invalid program ordinal".to_owned())?;
    if activity.protocol_version() != 3 || activity.activity_type() != expected {
        return Err("program activity does not match route and protocol".to_owned());
    }
    validate_payload(expected_ordinal, activity.payload())
}

pub(crate) fn verify_receipt(
    bytes: &[u8],
    authority: &layerx_proof::receipt::AuthorizedBatch,
    expected_activity: [u8; 32],
) -> Result<(), String> {
    let receipt =
        layerx_proof::receipt::verify_sequencer_signature(bytes, authority.sequencer_public_key())
            .map_err(|error| format!("lifecycle receipt signature: {error:?}"))?;
    let protocol = receipt.protocol().ok_or("missing protocol receipt")?;
    if protocol.protocol_version() != 3
        || protocol.module_id() != 9
        || protocol.module_version() != 4
        || protocol.operation() != 0
        || protocol.program_outcome().is_some()
        || protocol.activity_id() != expected_activity
        || protocol.batch_id() != authority.batch_id()
        || protocol.previous_state_root() != authority.previous_state_root()
        || protocol.resulting_state_root() != authority.resulting_state_root()
    {
        return Err("lifecycle receipt binding mismatch".to_owned());
    }
    if protocol.result_code() == 0 {
        layerx_proof::receipt::verify_program_state(bytes, authority)
            .map_err(|error| format!("lifecycle state receipt: {error:?}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use layerx_types::intent::ProgramId;
    use layerx_types::program_lifecycle::{ProgramUpgradePolicy, ProgramWindDownOperation};

    fn required<T, E: std::fmt::Debug>(value: Result<T, E>) -> T {
        value.unwrap_or_else(|error| panic!("{error:?}"))
    }

    fn signed(ordinal: u16, payload_bytes: &[u8], protocol: u16) -> (Vec<u8>, ModuleRegistry) {
        use ed25519_dalek::{Signer, SigningKey};
        use layerx_types::activity::{Authority, EnvelopeBuilder, Signature, TimestampBound};
        use layerx_types::amount::Amount;
        use layerx_types::ids::{Did, IdempotencyKey};
        use layerx_types::payload::{ModuleRegistration, Payload};
        let key = SigningKey::from_bytes(&[42; 32]);
        let kinds =
            [1, 2, 3, 7].map(|ordinal| required(ActivityType::new(ModuleId::Programs, ordinal)));
        let registry = required(ModuleRegistry::new(&[required(ModuleRegistration::new(
            ModuleId::Programs,
            &kinds,
        ))]));
        let kind = required(ActivityType::new(ModuleId::Programs, ordinal));
        let payload = required(Payload::new(&registry, kind, payload_bytes));
        let hash = required(layerx_wire::hash::payload_hash_for(&payload));
        let mut builder = EnvelopeBuilder::new();
        required(
            builder
                .protocol_version(protocol)
                .and_then(|value| value.network_id(402))
                .and_then(|value| value.activity_type(kind))
                .and_then(|value| value.actor_did(required(Did::new(b"did:layerx:lifecycle"))))
                .and_then(|value| {
                    value.authority(required(Authority::owner(&key.verifying_key().to_bytes())))
                })
                .and_then(|value| value.account_sequence(0))
                .and_then(|value| value.timestamp_bound(required(TimestampBound::new(1, u64::MAX))))
                .and_then(|value| value.idempotency_key(IdempotencyKey::new([7; 32])))
                .and_then(|value| value.fee_limit(Amount::from_u128(0)))
                .and_then(|value| value.payload_hash(hash))
                .and_then(|value| value.payload(payload)),
        );
        let unsigned = required(builder.build());
        let preimage = required(layerx_wire::sign::preimage_unsigned(&unsigned));
        let signature = key.sign(preimage.as_bytes()).to_bytes();
        let canonical = required(layerx_wire::activity::encode_signed_envelope(
            &unsigned.attach_signature(required(Signature::new(&signature))),
        ));
        (canonical, registry)
    }

    #[test]
    fn lifecycle_signed_activity_binds_exact_route_protocol_and_payload() {
        let wasm = b"\0asm\x01\0\0\0";
        let payload = required(
            NativeProgramDeploy {
                program_id: ProgramId::new([1; 32]),
                guest_abi: 2,
                policy: ProgramUpgradePolicy::Immutable,
                new_hash: Sha256::digest(wasm).into(),
                interface: None,
                wasm,
            }
            .encode(),
        );
        let (canonical, registry) = signed(1, &payload, 3);
        let key = ed25519_dalek::SigningKey::from_bytes(&[42; 32])
            .verifying_key()
            .to_bytes();
        assert!(
            layerx_platform_gateway::verify_submission(&canonical, &registry, 3, 402, &key).is_ok()
        );
        let other = ed25519_dalek::SigningKey::from_bytes(&[43; 32])
            .verifying_key()
            .to_bytes();
        assert!(
            layerx_platform_gateway::verify_submission(&canonical, &registry, 3, 402, &other)
                .is_err()
        );
        assert!(
            layerx_platform_gateway::verify_submission(&canonical, &registry, 3, 403, &key)
                .is_err()
        );
        let mut altered_signature = canonical.clone();
        let last = altered_signature.len() - 1;
        altered_signature[last] ^= 1;
        assert!(layerx_platform_gateway::verify_submission(
            &altered_signature,
            &registry,
            3,
            402,
            &key
        )
        .is_err());
        assert_eq!(required(validate(&canonical, &registry, 1)), [1; 32]);
        assert!(validate(&canonical, &registry, 2).is_err());
        assert!(validate(&canonical, &registry, 7).is_err());
        let (legacy, _) = signed(1, &payload, 2);
        assert!(validate(&legacy, &registry, 1).is_err());
        let mut bad_hash = payload;
        bad_hash[68] ^= 1;
        let (bad_hash, _) = signed(1, &bad_hash, 3);
        assert!(validate(&bad_hash, &registry, 1).is_err());
        let mut trailing = canonical;
        trailing.push(0);
        assert!(validate(&trailing, &registry, 1).is_err());
    }

    #[test]
    fn lifecycle_payloads_bind_hash_framing_and_ordinal() -> Result<(), String> {
        let wasm = b"\0asm\x01\0\0\0";
        let hash = Sha256::digest(wasm).into();
        let deploy = NativeProgramDeploy {
            program_id: ProgramId::new([1; 32]),
            guest_abi: 2,
            policy: ProgramUpgradePolicy::Authority([2; 32]),
            new_hash: hash,
            interface: None,
            wasm,
        }
        .encode()
        .map_err(|error| format!("{error:?}"))?;
        let upgrade = NativeProgramUpgrade {
            program_id: ProgramId::new([1; 32]),
            guest_abi: 2,
            old_hash: [3; 32],
            new_hash: hash,
            migration_hook: &[],
            clear_interface: false,
            interface: None,
            wasm,
        }
        .encode()
        .map_err(|error| format!("{error:?}"))?;
        let wind_down = NativeProgramWindDown {
            program_id: ProgramId::new([1; 32]),
            operation: ProgramWindDownOperation::Tombstone,
        }
        .encode()
        .map_err(|error| format!("{error:?}"))?;
        for (ordinal, payload, hash_offset) in [
            (1, deploy, Some(68)),
            (2, upgrade, Some(68)),
            (7, wind_down, None),
        ] {
            assert_eq!(validate_payload(ordinal, &payload)?, [1; 32]);
            for length in 0..payload.len() {
                assert!(validate_payload(ordinal, &payload[..length]).is_err());
            }
            let mut trailing = payload.clone();
            trailing.push(0);
            assert!(validate_payload(ordinal, &trailing).is_err());
            if let Some(offset) = hash_offset {
                let mut corrupted = payload;
                corrupted[offset] ^= 1;
                assert!(validate_payload(ordinal, &corrupted).is_err());
            }
        }
        assert!(validate_payload(3, &[]).is_err());
        Ok(())
    }
}

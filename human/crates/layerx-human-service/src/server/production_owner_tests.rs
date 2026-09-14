use super::*;
use layerx_types::payload::{ActivityType, ModuleRegistration, ModuleRegistry};

fn native_identity() -> (
    Did,
    [u8; 32],
    super::super::super::agent_runtime::AgentCoreIdentity,
) {
    const ACTIVITY: &[u8] =
        include_bytes!("../../../../../tests/fixtures/governance/owner-rotation/activity");
    const RECEIPT: &[u8] =
        include_bytes!("../../../../../tests/fixtures/governance/owner-rotation/receipt");
    const SEQUENCER: &[u8; 32] =
        include_bytes!("../../../../../tests/fixtures/governance/owner-rotation/sequencer-public");
    let activity_type = ActivityType::new(ModuleId::Governance, 2).expect("native rotation type");
    let registry =
        ModuleRegistry::new(&[
            ModuleRegistration::new(ModuleId::Governance, &[activity_type]).expect("registry row"),
        ])
        .expect("registry");
    let activity =
        layerx_wire::activity::decode_signed(ACTIVITY, &registry).expect("actual native rotation");
    let receipt = layerx_proof::receipt::verify_sequencer_signature(RECEIPT, *SEQUENCER)
        .expect("actual signed native receipt");
    let receipt = receipt.protocol().expect("native receipt");
    assert_eq!(
        receipt.activity_id(),
        layerx_wire::hash::activity_id(&activity).expect("activity id")
    );
    assert_eq!(receipt.result_code(), 0);
    let state = receipt
        .effects()
        .iter()
        .find(|effect| effect.module_id() == 7 && effect.event_type() == 0x7110)
        .expect("executed native identity transition")
        .body()
        .to_vec();
    let did = Did::new(activity.actor_did()).expect("actual owner DID");
    let decoded = OwnerRotationState::decode(&state, &did).expect("canonical native identity");
    let key = decoded.primary_public_key;
    (
        did,
        key,
        super::super::super::agent_runtime::AgentCoreIdentity {
            head_sequence: decoded.observed_sequence,
            revocation_sequence: decoded.revocation_sequence,
            verification: 1,
            frozen: false,
            authorities: vec![(1, key)],
            canonical_bytes: state,
        },
    )
}

#[test]
fn principal_owner_requires_finalised_current_primary_membership() {
    let (did, key, mut identity) = native_identity();
    assert!(validate_owner_identity(&did, key, &identity).is_err());
    for level in [0, 1, 2, 3, 6, u8::MAX] {
        identity.verification = level;
        assert!(validate_owner_identity(&did, key, &identity).is_err());
    }
    identity.verification = 4;
    assert!(validate_owner_identity(&did, key, &identity).is_ok());
    identity.verification = 5;
    assert!(validate_owner_identity(&did, key, &identity).is_ok());
    identity.frozen = true;
    assert!(validate_owner_identity(&did, key, &identity).is_err());
    identity.frozen = false;
    identity.authorities = vec![(2, key)];
    assert!(validate_owner_identity(&did, key, &identity).is_err());
    identity.authorities = vec![(1, key)];
    let foreign = Did::new(b"did:layerx:another-owner").expect("foreign DID");
    assert!(validate_owner_identity(&foreign, key, &identity).is_err());
    let mut foreign_key = key;
    foreign_key[0] ^= 1;
    assert!(validate_owner_identity(&did, foreign_key, &identity).is_err());
    identity.revocation_sequence -= 1;
    assert!(validate_owner_identity(&did, key, &identity).is_err());
    identity.revocation_sequence += 1;
    identity.head_sequence -= 1;
    assert!(validate_owner_identity(&did, key, &identity).is_err());
}

#[test]
fn principal_owner_rejects_every_truncated_or_foreign_identity_record() {
    let (did, key, mut identity) = native_identity();
    identity.verification = 4;
    let original = identity.canonical_bytes.clone();
    for length in 0..original.len() {
        identity.canonical_bytes = original[..length].to_vec();
        assert!(validate_owner_identity(&did, key, &identity).is_err());
    }
    identity.canonical_bytes = original;
    identity.canonical_bytes.push(0);
    assert!(validate_owner_identity(&did, key, &identity).is_err());
    identity.canonical_bytes.pop();
    identity.canonical_bytes[5] ^= 1;
    assert!(validate_owner_identity(&did, key, &identity).is_err());
}

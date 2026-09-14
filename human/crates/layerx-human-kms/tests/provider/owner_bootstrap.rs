use super::{checked, encoded_disclosure, facts, registry, request, signing_request, Host, Result};
use layerx_intents::owner_activity::{
    attach_signature, unsigned_native, verify, OwnerEnvelopeContext,
};
use layerx_intents::NativeOwnerBootstrap;
use layerx_types::activity::TimestampBound;
use layerx_types::ids::Did;
use layerx_types::intent::{ApprovalThreshold, PublicKey, RecoveryRoot};

#[test]
fn typed_owner_bootstrap_signs_through_real_kms_and_replays_after_restart() -> Result<()> {
    let mut host = Host::new()?;
    let binding = [81; 32];
    let (handle, public) = facts(&host.call(&request(1, binding, &[], None)?)?)?;
    let (_, pending) = facts(&host.call(&request(1, [82; 32], &[], None)?)?)?;
    let did = checked(Did::new(b"did:layerx:bootstrap-owner"))?;
    let registry = registry()?;
    let operations = [
        NativeOwnerBootstrap::Identity {
            did: did.clone(),
            primary_key: PublicKey::new(public),
        },
        NativeOwnerBootstrap::RotationPolicy {
            did: did.clone(),
            pending_key: PublicKey::new(pending),
            challenge: checked(TimestampBound::new(1_000, 61_000))?,
            effective_sequence: 11,
        },
        NativeOwnerBootstrap::RecoveryPolicy {
            did: did.clone(),
            root: RecoveryRoot::new([19; 32]),
            threshold: checked(ApprovalThreshold::new(2))?,
            minimum_delay: 30,
            maximum_delay: 60,
        },
    ];
    let mut original = Vec::new();
    for pass in 0..2 {
        if pass == 1 {
            host.stop();
            host.start()?;
        }
        let mut signed_operations = Vec::new();
        for operation in &operations {
            let compiled = checked(operation.compile(&registry))?;
            let context = OwnerEnvelopeContext {
                actor: did.clone(),
                owner_public_key: public,
                network_id: 77,
                account_sequence: 10,
                not_before_ms: 1_000,
                not_after_ms: 61_000,
                action_key: [u8::try_from(compiled.activity_type().ordinal())?; 32],
                fee_limit: 100,
            };
            let (unsigned, disclosure) = checked(unsigned_native(&compiled, &context, &registry))?;
            let encoded = encoded_disclosure(&disclosure)?;
            let packet = signing_request(binding, &handle, &unsigned, &encoded)?.0;
            let response = host.call(&packet)?;
            assert_eq!(response[7], 0);
            assert_eq!(response.len(), 72);
            let signature = response[8..].try_into()?;
            let signed = checked(attach_signature(&unsigned, signature, public, &registry))?;
            let activity = checked(verify(&signed, &registry))?;
            assert_eq!(activity.actor_did(), did.as_bytes());
            assert_eq!(activity.authority(), public);
            assert_eq!(activity.payload(), compiled.payload().as_bytes());
            assert_eq!(activity.activity_type(), compiled.activity_type());
            assert!(attach_signature(&unsigned, signature, pending, &registry).is_err());
            for changed in super::native_setup::mutated_native(&disclosure) {
                let packet =
                    signing_request(binding, &handle, &unsigned, &encoded_disclosure(&changed)?)?.0;
                assert_eq!(host.call(&packet)?[7], 1);
            }
            let mut changed = disclosure.clone();
            changed.actor[0] ^= 1;
            let refused =
                signing_request(binding, &handle, &unsigned, &encoded_disclosure(&changed)?)?.0;
            assert_eq!(host.call(&refused)?[7], 1);
            assert_eq!(
                host.call(&signing_request([83; 32], &handle, &unsigned, &encoded)?.0)?[7],
                2
            );
            signed_operations.push(signed);
        }
        if pass == 0 {
            original = signed_operations;
        } else {
            assert_eq!(signed_operations, original);
        }
    }
    Ok(())
}

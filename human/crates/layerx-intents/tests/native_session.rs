use std::fmt::Debug;

use layerx_crypto::disclosure::bind;
use layerx_crypto::{ed25519, SignatureMessage};
use layerx_intents::{compile, DisclosureCheck, Intent, IntentKind, SessionGrant};
use layerx_proof::receipt::verify_sequencer_signature;
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_wire::{activity, hash};

fn checked<T, E: Debug>(value: Result<T, E>) -> T {
    value.unwrap_or_else(|error| panic!("original native session evidence: {error:?}"))
}

struct Fixture {
    activity: &'static [u8],
    activity_id: &'static [u8; 32],
    receipt: &'static [u8],
    sequencer: &'static [u8; 32],
}

macro_rules! fixture {
    ($name:literal) => {
        Fixture {
            activity: include_bytes!(concat!(
                "../../../../tests/fixtures/authority/native-sessions/",
                $name,
                "/grant.activity"
            )),
            activity_id: include_bytes!(concat!(
                "../../../../tests/fixtures/authority/native-sessions/",
                $name,
                "/grant.activity-id"
            )),
            receipt: include_bytes!(concat!(
                "../../../../tests/fixtures/authority/native-sessions/",
                $name,
                "/grant.receipt"
            )),
            sequencer: include_bytes!(concat!(
                "../../../../tests/fixtures/authority/native-sessions/",
                $name,
                "/grant.sequencer-public"
            )),
        }
    };
}

#[test]
fn executable_session_intents_reproduce_original_owner_signed_native_registrations() {
    let kind = checked(ActivityType::new(ModuleId::Governance, 5));
    let registry = checked(ModuleRegistry::new(&[checked(ModuleRegistration::new(
        ModuleId::Governance,
        &[kind],
    ))]));
    for fixture in [
        fixture!("legacy"),
        fixture!("fee"),
        fixture!("authentication"),
        fixture!("replacement"),
    ] {
        let submitted = checked(activity::decode_signed(fixture.activity, &registry));
        assert_eq!(checked(hash::activity_id(&submitted)), *fixture.activity_id);
        assert_eq!(
            (submitted.network_id(), submitted.protocol_version()),
            (77, 3)
        );
        let unsigned = checked(activity::encode_unsigned(&submitted));
        let original_signature: [u8; 64] = checked(
            submitted
                .signature()
                .unwrap_or_else(|| panic!("missing original signature"))
                .try_into(),
        );
        let original_owner: [u8; 32] = checked(submitted.authority().try_into());
        assert_eq!(
            checked(layerx_intents::owner_activity::attach_signature(
                &unsigned,
                original_signature,
                original_owner,
                &registry
            )),
            fixture.activity
        );
        assert!(layerx_intents::owner_activity::attach_signature(
            &unsigned,
            original_signature,
            [0; 32],
            &registry
        )
        .is_err());
        let mut changed_signature = original_signature;
        changed_signature[0] ^= 1;
        assert!(layerx_intents::owner_activity::attach_signature(
            &unsigned,
            changed_signature,
            original_owner,
            &registry
        )
        .is_err());

        let message = checked(SignatureMessage::new(
            hash::Domain::SignaturePreimage,
            submitted.protocol_version(),
            submitted.network_id(),
            &unsigned,
        ));
        let signature = submitted
            .signature()
            .unwrap_or_else(|| panic!("missing original owner signature"));
        assert_eq!(
            ed25519::verify(
                checked(submitted.authority().try_into()),
                checked(signature.try_into()),
                message
            ),
            Ok(())
        );
        let receipt = checked(verify_sequencer_signature(
            fixture.receipt,
            *fixture.sequencer,
        ));
        let protocol = receipt
            .protocol()
            .unwrap_or_else(|| panic!("missing native receipt"));
        assert_eq!(protocol.activity_id(), *fixture.activity_id);
        assert_eq!(protocol.protocol_version(), submitted.protocol_version());
        assert_eq!(
            (
                protocol.module_id(),
                protocol.operation(),
                protocol.result_code()
            ),
            (7, 0, 0)
        );
        let disclosed = checked(bind(&unsigned, &registry));
        let session = disclosed
            .session_grant
            .unwrap_or_else(|| panic!("missing canonical session disclosure"));
        let mut grant = checked(SessionGrant::new(
            session.grant.registration_payload.clone(),
            session.expiry_sequence,
            session.action_key,
        ));
        if let Some(replacement) = session.replacement {
            grant = checked(grant.replacing(
                replacement.predecessor_grant_id,
                replacement.expected_charge_state,
            ));
        }
        let intent = Intent::v3(IntentKind::SessionGrant(grant.clone()));
        let compiled = checked(compile(&intent, &registry));
        assert_eq!(compiled.payload().as_bytes(), submitted.payload());
        assert_eq!(
            checked(DisclosureCheck::verify(&intent, &compiled)).canonical_payload(),
            submitted.payload()
        );
        if session.grant.registration_payload[4] != 1 || session.replacement.is_some() {
            assert!(compile(
                &Intent::v1(IntentKind::SessionGrant(grant.clone())),
                &registry
            )
            .is_err());
            assert!(compile(&Intent::v2(IntentKind::SessionGrant(grant)), &registry).is_err());
        }
    }
}

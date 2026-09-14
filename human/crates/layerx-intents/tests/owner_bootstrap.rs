use layerx_crypto::signer::{LocalSigner, Signer as _, SigningRequest};
use layerx_crypto::SignatureMessage;
use layerx_intents::canonical::Domain;
use layerx_intents::owner_activity::{
    attach_signature, unsigned_native, verify, OwnerEnvelopeContext,
};
use layerx_intents::NativeOwnerBootstrap;
use layerx_types::activity::TimestampBound;
use layerx_types::ids::Did;
use layerx_types::intent::{ApprovalThreshold, PublicKey, RecoveryRoot};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use std::task::{Context, Poll, Waker};

fn registry(ordinals: &[u16]) -> ModuleRegistry {
    let activities: Vec<_> = ordinals
        .iter()
        .map(|ordinal| {
            ActivityType::new(ModuleId::Governance, *ordinal)
                .unwrap_or_else(|error| panic!("native Governance ordinal: {error:?}"))
        })
        .collect();
    ModuleRegistry::new(&[ModuleRegistration::new(ModuleId::Governance, &activities)
        .unwrap_or_else(|error| panic!("native Governance registration: {error:?}"))])
    .unwrap_or_else(|error| panic!("native module registry: {error:?}"))
}

#[test]
fn native_owner_bootstrap_is_disclosed_signed_and_bound_to_every_envelope_field() {
    let signer = LocalSigner::new([71; 32]);
    let pending = LocalSigner::new([72; 32]).public_key();
    let did =
        Did::new(b"did:layerx:owner-bootstrap").unwrap_or_else(|error| panic!("DID: {error:?}"));
    let registry = registry(&[1, 2, 3]);
    let operations = [
        NativeOwnerBootstrap::Identity {
            did: did.clone(),
            primary_key: PublicKey::new(signer.public_key()),
        },
        NativeOwnerBootstrap::RotationPolicy {
            did: did.clone(),
            pending_key: PublicKey::new(pending),
            challenge: TimestampBound::new(1_000, 61_000)
                .unwrap_or_else(|error| panic!("challenge: {error:?}")),
            effective_sequence: 11,
        },
        NativeOwnerBootstrap::RecoveryPolicy {
            did: did.clone(),
            root: RecoveryRoot::new([19; 32]),
            threshold: ApprovalThreshold::new(2)
                .unwrap_or_else(|error| panic!("threshold: {error:?}")),
            minimum_delay: 30,
            maximum_delay: 60,
        },
    ];
    for (index, operation) in operations.iter().enumerate() {
        let ordinal = u16::try_from(index + 1).unwrap_or_else(|error| panic!("ordinal: {error:?}"));
        let compiled = operation
            .compile(&registry)
            .unwrap_or_else(|error| panic!("native bootstrap compile: {error:?}"));
        let context = OwnerEnvelopeContext {
            actor: did.clone(),
            owner_public_key: signer.public_key(),
            network_id: 77,
            account_sequence: 10,
            not_before_ms: 1_000,
            not_after_ms: 61_000,
            action_key: [u8::try_from(ordinal).unwrap_or_else(|error| panic!("ordinal: {error:?}"));
                32],
            fee_limit: 100,
        };
        let (unsigned, disclosure) = unsigned_native(&compiled, &context, &registry)
            .unwrap_or_else(|error| panic!("canonical disclosure: {error:?}"));
        assert_eq!(
            disclosure
                .reencode()
                .unwrap_or_else(|error| panic!("disclosure round trip: {error:?}")),
            unsigned
        );
        let message = SignatureMessage::new(Domain::SignaturePreimage, 3, 77, &unsigned)
            .unwrap_or_else(|error| panic!("domain: {error:?}"));
        let request = SigningRequest::new(message, &disclosure)
            .unwrap_or_else(|error| panic!("bound disclosure: {error:?}"));
        let mut future = signer.sign(request);
        let signature = match future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
        {
            Poll::Ready(result) => {
                result.unwrap_or_else(|error| panic!("actual Ed25519 signer: {error:?}"))
            }
            Poll::Pending => panic!("local signer did not finish"),
        };
        let signed = attach_signature(
            &unsigned,
            *signature.as_bytes(),
            signer.public_key(),
            &registry,
        )
        .unwrap_or_else(|error| panic!("authenticated owner envelope: {error:?}"));
        let decoded = verify(&signed, &registry)
            .unwrap_or_else(|error| panic!("independent signature verification: {error:?}"));
        assert_eq!(
            decoded.activity_type(),
            ActivityType::new(ModuleId::Governance, ordinal)
                .unwrap_or_else(|error| panic!("type: {error:?}"))
        );
        assert_eq!(decoded.payload(), compiled.payload().as_bytes());
        assert_eq!(decoded.actor_did(), did.as_bytes());
        assert_eq!(decoded.authority(), signer.public_key());
        assert_eq!(decoded.network_id(), 77);
        assert_eq!(decoded.account_sequence(), 10);
        assert_eq!(decoded.idempotency_key(), context.action_key);
        for offset in 0..signed.len() {
            let mut changed = signed.clone();
            changed[offset] ^= 1;
            assert!(
                verify(&changed, &registry).is_err(),
                "changed canonical byte {offset}"
            );
        }
        assert!(attach_signature(&unsigned, *signature.as_bytes(), pending, &registry).is_err());
    }
}

#[test]
fn native_bootstrap_refuses_missing_module_and_invalid_authority_policy() {
    let did =
        Did::new(b"did:layerx:owner-bootstrap").unwrap_or_else(|error| panic!("DID: {error:?}"));
    let key = PublicKey::new(LocalSigner::new([71; 32]).public_key());
    let identity = NativeOwnerBootstrap::Identity {
        did: did.clone(),
        primary_key: key,
    };
    assert!(identity.compile(&registry(&[2, 3])).is_err());
    assert!(NativeOwnerBootstrap::Identity {
        did: did.clone(),
        primary_key: PublicKey::new([0; 32])
    }
    .compile(&registry(&[1, 2, 3]))
    .is_err());
    assert!(NativeOwnerBootstrap::RotationPolicy {
        did: did.clone(),
        pending_key: key,
        challenge: TimestampBound::new(1, 2).unwrap_or_else(|error| panic!("challenge: {error:?}")),
        effective_sequence: 0
    }
    .compile(&registry(&[1, 2, 3]))
    .is_err());
    for (root, minimum_delay, maximum_delay) in
        [([0; 32], 30, 60), ([19; 32], 0, 60), ([19; 32], 60, 30)]
    {
        assert!(NativeOwnerBootstrap::RecoveryPolicy {
            did: did.clone(),
            root: RecoveryRoot::new(root),
            threshold: ApprovalThreshold::new(2)
                .unwrap_or_else(|error| panic!("threshold: {error:?}")),
            minimum_delay,
            maximum_delay
        }
        .compile(&registry(&[1, 2, 3]))
        .is_err());
    }
}

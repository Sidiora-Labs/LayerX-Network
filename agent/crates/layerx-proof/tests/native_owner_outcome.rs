use layerx_proof::receipt::{
    verify_native_owner_outcome, verify_outcome, verify_sequencer_signature, AuthorizedBatch,
    NativeOwnerOutcomeContext, NativeOwnerOutcomeFailure, ReceiptCheck,
};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_types::verify::VerificationLevel;
use layerx_wire::activity::decode_signed;

const ROOT: &str = "../../../platform/hosted/authority/tests/fixtures/native-sessions/lifecycle";

fn fixture(index: usize) -> (Vec<u8>, Vec<u8>, [u8; 32]) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(ROOT);
    let activity = std::fs::read(path.join(format!("grant-{index}.activity")))
        .unwrap_or_else(|error| panic!("native activity: {error}"));
    let receipt = std::fs::read(path.join(format!("grant-{index}.receipt")))
        .unwrap_or_else(|error| panic!("native receipt: {error}"));
    let key = std::fs::read(path.join("sequencer-public"))
        .unwrap_or_else(|error| panic!("native key: {error}"))
        .try_into()
        .unwrap_or_else(|_| panic!("native key length"));
    (activity, receipt, key)
}

fn registry() -> ModuleRegistry {
    let kinds = [5, 6].map(|ordinal| {
        ActivityType::new(ModuleId::Governance, ordinal)
            .unwrap_or_else(|_| panic!("Governance type"))
    });
    let registration = ModuleRegistration::new(ModuleId::Governance, &kinds)
        .unwrap_or_else(|_| panic!("Governance registration"));
    ModuleRegistry::new(&[registration]).unwrap_or_else(|_| panic!("registry"))
}

#[test]
fn native_owner_receipts_bind_exact_activity_and_preserve_refusals() {
    for index in 1..=10 {
        let (bytes, raw_receipt, key) = fixture(index);
        let activity = decode_signed(&bytes, &registry())
            .unwrap_or_else(|error| panic!("native activity: {error:?}"));
        let receipt = verify_sequencer_signature(&raw_receipt, key)
            .unwrap_or_else(|error| panic!("native receipt: {error:?}"));
        let protocol = receipt.protocol().unwrap_or_else(|| panic!("protocol"));
        let authorised = AuthorizedBatch::new(
            protocol.batch_id(),
            protocol.asset(),
            protocol.previous_state_root(),
            protocol.resulting_state_root(),
            key,
        );
        let expected = NativeOwnerOutcomeContext {
            canonical_activity: &bytes,
            actor: activity.actor_did(),
            action_key: activity.idempotency_key(),
            activity_type: activity.activity_type(),
            owner_public_key: activity
                .authority()
                .try_into()
                .unwrap_or_else(|_| panic!("owner authority")),
            network_id: activity.network_id(),
        };
        assert_eq!(protocol.operation(), 0);
        assert_ne!(expected.activity_type.ordinal(), 0);
        assert_ne!(protocol.activity_id(), expected.action_key);
        assert_eq!(
            verify_outcome(&raw_receipt, &authorised)
                .err()
                .map(|failure| failure.check),
            Some(ReceiptCheck::Operation)
        );
        let verified = verify_native_owner_outcome(&raw_receipt, &authorised, &expected)
            .unwrap_or_else(|error| panic!("native owner outcome {index}: {error:?}"));
        assert_eq!(verified.canonical_bytes(), raw_receipt);
        assert_eq!(verified.level(), VerificationLevel::SEQUENCER_SIGNED);
        let result = verified
            .receipt()
            .protocol()
            .unwrap_or_else(|| panic!("verified protocol"))
            .result_code();
        assert_eq!(result, protocol.result_code());
        if matches!(index, 7 | 8 | 10) {
            assert_ne!(result, 0);
        } else {
            assert_eq!(result, 0);
        }
    }
}

#[test]
fn independently_retained_context_and_batch_mismatches_are_refused() {
    let (bytes, raw_receipt, key) = fixture(2);
    let activity = decode_signed(&bytes, &registry()).unwrap_or_else(|_| panic!("native activity"));
    let receipt =
        verify_sequencer_signature(&raw_receipt, key).unwrap_or_else(|_| panic!("native receipt"));
    let protocol = receipt.protocol().unwrap_or_else(|| panic!("protocol"));
    let authorised = AuthorizedBatch::new(
        protocol.batch_id(),
        protocol.asset(),
        protocol.previous_state_root(),
        protocol.resulting_state_root(),
        key,
    );
    let expected = NativeOwnerOutcomeContext {
        canonical_activity: &bytes,
        actor: activity.actor_did(),
        action_key: activity.idempotency_key(),
        activity_type: activity.activity_type(),
        owner_public_key: activity
            .authority()
            .try_into()
            .unwrap_or_else(|_| panic!("owner")),
        network_id: activity.network_id(),
    };
    let mut changed = expected;
    changed.action_key[0] ^= 1;
    assert_eq!(
        verify_native_owner_outcome(&raw_receipt, &authorised, &changed),
        Err(NativeOwnerOutcomeFailure::ActivityBinding)
    );
    changed = expected;
    changed.actor = b"did:layerx:another-owner";
    assert_eq!(
        verify_native_owner_outcome(&raw_receipt, &authorised, &changed),
        Err(NativeOwnerOutcomeFailure::ActivityBinding)
    );
    changed = expected;
    changed.network_id += 1;
    assert_eq!(
        verify_native_owner_outcome(&raw_receipt, &authorised, &changed),
        Err(NativeOwnerOutcomeFailure::ActivityBinding)
    );
    changed = expected;
    changed.owner_public_key[0] ^= 1;
    assert_eq!(
        verify_native_owner_outcome(&raw_receipt, &authorised, &changed),
        Err(NativeOwnerOutcomeFailure::ActivityBinding)
    );
    changed = expected;
    changed.activity_type =
        ActivityType::new(ModuleId::Governance, 6).unwrap_or_else(|_| panic!("type"));
    assert!(verify_native_owner_outcome(&raw_receipt, &authorised, &changed).is_err());
    let other = AuthorizedBatch::new(
        authorised.batch_id(),
        authorised.asset(),
        [91; 32],
        authorised.resulting_state_root(),
        key,
    );
    assert!(
        matches!(verify_native_owner_outcome(&raw_receipt, &other, &expected), Err(NativeOwnerOutcomeFailure::Receipt(failure)) if failure.check == ReceiptCheck::PreviousStateRoot)
    );
    let other = AuthorizedBatch::new(
        authorised.batch_id(),
        authorised.asset(),
        authorised.previous_state_root(),
        [92; 32],
        key,
    );
    assert!(
        matches!(verify_native_owner_outcome(&raw_receipt, &other, &expected), Err(NativeOwnerOutcomeFailure::Receipt(failure)) if failure.check == ReceiptCheck::ResultingStateRoot)
    );
    let mut changed_bytes = bytes.clone();
    let last = changed_bytes.len() - 1;
    changed_bytes[last] ^= 1;
    changed = expected;
    changed.canonical_activity = &changed_bytes;
    assert_eq!(
        verify_native_owner_outcome(&raw_receipt, &authorised, &changed),
        Err(NativeOwnerOutcomeFailure::ActivitySignature)
    );
    let (another, _, _) = fixture(9);
    changed = expected;
    changed.canonical_activity = &another;
    assert!(verify_native_owner_outcome(&raw_receipt, &authorised, &changed).is_err());
    let mut changed_receipt = raw_receipt.clone();
    let last = changed_receipt.len() - 1;
    changed_receipt[last] ^= 1;
    assert!(
        matches!(verify_native_owner_outcome(&changed_receipt, &authorised, &expected), Err(NativeOwnerOutcomeFailure::Receipt(failure)) if failure.check == ReceiptCheck::SequencerSignature)
    );
}

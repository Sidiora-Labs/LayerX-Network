use layerx_agentd::receipt::{
    serve, store_native_owner_if_absent, store_verified_if_absent, ReceiptLookupKey,
};
use layerx_agentd::store::{Store, TenantId};
use layerx_proof::receipt::{
    verify_sequencer_signature, AuthorizedBatch, NativeOwnerOutcomeContext,
};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_types::verify::VerificationLevel;
use layerx_wire::activity::decode_signed;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(1);

fn checked<T, E: std::fmt::Debug>(value: Result<T, E>) -> T {
    value.unwrap_or_else(|error| panic!("native owner receipt: {error:?}"))
}

fn fixture(index: usize) -> (Vec<u8>, Vec<u8>, [u8; 32]) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../platform/hosted/authority/tests/fixtures/native-sessions/lifecycle");
    (
        checked(std::fs::read(root.join(format!("grant-{index}.activity")))),
        checked(std::fs::read(root.join(format!("grant-{index}.receipt")))),
        checked(checked(std::fs::read(root.join("sequencer-public"))).try_into()),
    )
}

fn registry() -> ModuleRegistry {
    let kinds = [5, 6].map(|ordinal| checked(ActivityType::new(ModuleId::Governance, ordinal)));
    checked(ModuleRegistry::new(&[checked(ModuleRegistration::new(
        ModuleId::Governance,
        &kinds,
    ))]))
}

#[test]
fn real_native_owner_success_and_refusal_survive_durable_indexes_and_restart() {
    for index in [2, 7, 8, 10] {
        let (original, receipt, key) = fixture(index);
        let activity = checked(decode_signed(&original, &registry()));
        let decoded = checked(verify_sequencer_signature(&receipt, key));
        let protocol = decoded
            .protocol()
            .unwrap_or_else(|| panic!("native protocol"));
        let batch = AuthorizedBatch::new(
            protocol.batch_id(),
            protocol.asset(),
            protocol.previous_state_root(),
            protocol.resulting_state_root(),
            key,
        );
        let expected = NativeOwnerOutcomeContext {
            canonical_activity: &original,
            actor: activity.actor_did(),
            action_key: activity.idempotency_key(),
            activity_type: activity.activity_type(),
            owner_public_key: checked(activity.authority().try_into()),
            network_id: activity.network_id(),
        };
        let root = std::env::temp_dir().join(format!(
            "layerx-native-owner-ingress-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let tenant = checked(TenantId::new("native-owner"));
        let mut durable = checked(Store::open(&root));
        assert!(store_verified_if_absent(
            &mut durable,
            tenant.clone(),
            expected.action_key,
            &receipt,
            &batch
        )
        .is_err());
        let mut wrong = expected;
        wrong.owner_public_key[0] ^= 1;
        assert!(store_native_owner_if_absent(
            &mut durable,
            tenant.clone(),
            &receipt,
            &batch,
            &wrong
        )
        .is_err());
        assert!(serve(
            &durable,
            tenant.clone(),
            ReceiptLookupKey::Activity(protocol.activity_id())
        )
        .is_err());
        let saved = checked(store_native_owner_if_absent(
            &mut durable,
            tenant.clone(),
            &receipt,
            &batch,
            &expected,
        ));
        assert_eq!(saved.metadata.result.code.raw(), protocol.result_code());
        assert_eq!(
            saved.metadata.verification_level,
            VerificationLevel::SEQUENCER_SIGNED
        );
        assert_eq!(saved.canonical_bytes, receipt);
        drop(durable);
        let mut durable = checked(Store::open(&root));
        for lookup in [
            ReceiptLookupKey::Activity(protocol.activity_id()),
            ReceiptLookupKey::Idempotency(expected.action_key),
            ReceiptLookupKey::GlobalSequence(protocol.global_sequence()),
        ] {
            assert_eq!(checked(serve(&durable, tenant.clone(), lookup)), saved);
        }
        assert_eq!(
            checked(store_native_owner_if_absent(
                &mut durable,
                tenant.clone(),
                &receipt,
                &batch,
                &expected
            )),
            saved
        );
        assert_refusal_binding(&mut durable, tenant, &receipt, &batch, &expected, &saved);
    }
}

fn assert_refusal_binding(
    durable: &mut Store,
    tenant: TenantId,
    receipt: &[u8],
    batch: &AuthorizedBatch,
    expected: &NativeOwnerOutcomeContext<'_>,
    saved: &layerx_agentd::receipt::ServedReceipt,
) {
    for mutate in [
        |v: &mut NativeOwnerOutcomeContext<'_>| v.action_key[0] ^= 1,
        |v: &mut NativeOwnerOutcomeContext<'_>| v.network_id += 1,
    ] {
        let mut changed = *expected;
        mutate(&mut changed);
        assert!(
            store_native_owner_if_absent(durable, tenant.clone(), receipt, batch, &changed)
                .is_err()
        );
    }
    assert_eq!(
        checked(serve(
            durable,
            tenant.clone(),
            ReceiptLookupKey::Idempotency(expected.action_key)
        )),
        *saved
    );
    assert!(serve(
        durable,
        checked(TenantId::new("different-tenant")),
        ReceiptLookupKey::Activity(saved.metadata.activity_id)
    )
    .is_err());
    let mut changed = receipt.to_vec();
    let last = changed.len() - 1;
    changed[last] ^= 1;
    assert!(store_native_owner_if_absent(durable, tenant, &changed, batch, expected).is_err());
}

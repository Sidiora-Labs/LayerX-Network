use ed25519_dalek::{Signer as _, SigningKey};
use layerx_agentd::cache::{revalidate, CacheError, CacheValue, EvidenceCache, EvidenceKind};
use layerx_agentd::store::TenantId;
use layerx_proof::receipt::{verify, AuthorizedBatch, VerifiedReceipt};
use layerx_types::verify::VerificationLevel;
use layerx_wire::encode::Encoder;
use layerx_wire::hash::receipt_digest;

fn receipt_bytes(signature: Option<[u8; 64]>) -> Vec<u8> {
    let mut encoder = Encoder::new(4096);
    assert_eq!(
        encoder.structure_header_version(0x5201, layerx_wire::limits::PROTOCOL_VERSION),
        Ok(())
    );
    assert_eq!(encoder.u16(layerx_wire::limits::PROTOCOL_VERSION), Ok(()));
    assert_eq!(encoder.bytes(&[1; 32], 32), Ok(()));
    assert_eq!(encoder.u64(9), Ok(()));
    assert_eq!(encoder.bytes(&[2; 32], 32), Ok(()));
    assert_eq!(encoder.bytes(&[3; 32], 32), Ok(()));
    assert_eq!(encoder.bytes(&[8; 32], 32), Ok(()));
    assert_eq!(encoder.i32(0), Ok(()));
    assert_eq!(encoder.sequence_length(0, 512), Ok(()));
    assert_eq!(encoder.u128(1), Ok(()));
    assert_eq!(encoder.bytes(&[4; 32], 32), Ok(()));
    assert_eq!(encoder.u16(1), Ok(()));
    assert_eq!(encoder.u32(1), Ok(()));
    assert_eq!(encoder.u32(1), Ok(()));
    assert_eq!(encoder.u8(1), Ok(()));
    assert_eq!(encoder.bytes(&[5; 32], 32), Ok(()));
    assert_eq!(encoder.u128(25), Ok(()));
    assert_eq!(encoder.bytes(&[6; 32], 32), Ok(()));
    assert_eq!(encoder.u128(100), Ok(()));
    assert_eq!(encoder.u128(75), Ok(()));
    assert_eq!(encoder.u64(1), Ok(()));
    assert_eq!(encoder.bytes(&[7; 32], 32), Ok(()));
    assert_eq!(encoder.u128(10), Ok(()));
    assert_eq!(encoder.u128(35), Ok(()));
    assert_eq!(encoder.bytes(&[9; 32], 32), Ok(()));
    assert_eq!(encoder.bytes(&[10; 32], 32), Ok(()));
    assert_eq!(encoder.bytes(&[11; 32], 32), Ok(()));
    assert_eq!(encoder.u64(1_000), Ok(()));
    assert_eq!(encoder.u8(u8::from(signature.is_some())), Ok(()));
    if let Some(value) = signature {
        assert_eq!(encoder.bytes(&value, 64), Ok(()));
    }
    encoder.finish()
}

fn verified_receipt() -> (Vec<u8>, VerifiedReceipt) {
    let key = SigningKey::from_bytes(&[3; 32]);
    let unsigned = receipt_bytes(None);
    let digest =
        receipt_digest(&unsigned).unwrap_or_else(|error| panic!("receipt digest: {error:?}"));
    let bytes = receipt_bytes(Some(key.sign(&digest).to_bytes()));
    let verified = verify(
        &bytes,
        &AuthorizedBatch::new(
            [4; 32],
            [5; 32],
            [2; 32],
            [3; 32],
            key.verifying_key().to_bytes(),
        ),
    )
    .unwrap_or_else(|error| panic!("receipt verification: {error:?}"));
    (bytes, verified)
}

fn tenant(name: &str) -> TenantId {
    TenantId::new(name).unwrap_or_else(|error| panic!("tenant: {error}"))
}

#[test]
fn cached_value_never_serves_above_the_proof_level_it_holds() {
    let (bytes, proof) = verified_receipt();
    let value = CacheValue::from_receipt(bytes.clone(), &proof, 10, [1; 32]);
    assert_eq!(value.evidence_kind(), EvidenceKind::Receipt);
    let mut cache = EvidenceCache::new(4, 4096).unwrap_or_else(|error| panic!("cache: {error:?}"));
    cache
        .insert(tenant("tenant-a"), b"balance".to_vec(), value)
        .unwrap_or_else(|error| panic!("insert: {error:?}"));
    assert!(matches!(
        revalidate(
            &mut cache,
            tenant("tenant-a"),
            b"balance",
            VerificationLevel::STATE_PROVEN,
            10,
            [1; 32],
            || panic!("fresh entry revalidated")
        ),
        Err(CacheError::InsufficientEvidence {
            requested: VerificationLevel::STATE_PROVEN,
            held: VerificationLevel::SEQUENCER_SIGNED
        })
    ));
    let served = revalidate(
        &mut cache,
        tenant("tenant-a"),
        b"balance",
        VerificationLevel::SEQUENCER_SIGNED,
        10,
        [1; 32],
        || panic!("fresh entry revalidated"),
    )
    .unwrap_or_else(|error| panic!("serve: {error:?}"));
    assert_eq!(served.core_bytes(), bytes);
    assert_eq!(served.level(), VerificationLevel::SEQUENCER_SIGNED);
    assert_eq!(cache.metrics(&tenant("tenant-a")).hits, 1);
}

#[test]
fn head_or_checkpoint_movement_forces_real_revalidation_and_tracks_staleness() {
    let (bytes, proof) = verified_receipt();
    let mut cache = EvidenceCache::new(4, 4096).unwrap_or_else(|error| panic!("cache: {error:?}"));
    cache
        .insert(
            tenant("tenant-a"),
            b"balance".to_vec(),
            CacheValue::from_receipt(bytes.clone(), &proof, 10, [1; 32]),
        )
        .unwrap_or_else(|error| panic!("insert: {error:?}"));
    let mut calls = 0;
    let refreshed = revalidate(
        &mut cache,
        tenant("tenant-a"),
        b"balance",
        VerificationLevel::SEQUENCER_SIGNED,
        11,
        [2; 32],
        || {
            calls += 1;
            Ok(CacheValue::from_receipt(bytes.clone(), &proof, 11, [2; 32]))
        },
    )
    .unwrap_or_else(|error| panic!("revalidate: {error:?}"));
    assert_eq!(calls, 1);
    assert_eq!(refreshed.observed_head_sequence(), 11);
    let metrics = cache.metrics(&tenant("tenant-a"));
    assert_eq!(metrics.stale, 1);
    assert_eq!(metrics.revalidations, 1);

    assert!(matches!(
        revalidate(
            &mut cache,
            tenant("tenant-a"),
            b"balance",
            VerificationLevel::SEQUENCER_SIGNED,
            12,
            [2; 32],
            || Err(CacheError::CoreUnavailable {
                cached_level: VerificationLevel::UNVERIFIED,
                stale_by_sequences: 0
            })
        ),
        Err(CacheError::CoreUnavailable {
            cached_level: VerificationLevel::SEQUENCER_SIGNED,
            stale_by_sequences: 1
        })
    ));
}

#[test]
fn tenant_quotas_refuse_new_entries_without_degrading_existing_values() {
    let (bytes, proof) = verified_receipt();
    let mut cache =
        EvidenceCache::new(1, bytes.len() + 8).unwrap_or_else(|error| panic!("cache: {error:?}"));
    cache
        .insert(
            tenant("tenant-a"),
            b"first".to_vec(),
            CacheValue::from_receipt(bytes.clone(), &proof, 10, [1; 32]),
        )
        .unwrap_or_else(|error| panic!("first: {error:?}"));
    assert_eq!(
        cache.insert(
            tenant("tenant-a"),
            b"second".to_vec(),
            CacheValue::from_receipt(bytes.clone(), &proof, 10, [1; 32]),
        ),
        Err(CacheError::QuotaExceeded)
    );
    cache
        .insert(
            tenant("tenant-b"),
            b"first".to_vec(),
            CacheValue::from_receipt(bytes, &proof, 10, [1; 32]),
        )
        .unwrap_or_else(|error| panic!("other tenant: {error:?}"));
    assert_eq!(cache.metrics(&tenant("tenant-a")).entries, 1);
    assert_eq!(cache.metrics(&tenant("tenant-a")).quota_refusals, 1);
    assert_eq!(cache.metrics(&tenant("tenant-b")).entries, 1);
}

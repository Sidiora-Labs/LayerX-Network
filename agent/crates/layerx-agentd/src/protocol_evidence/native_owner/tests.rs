use super::*;
use layerx_proof::merkle::Proof;
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use serde_json::Value;

fn checked<T, E: std::fmt::Debug>(value: Result<T, E>) -> T {
    value.unwrap_or_else(|error| panic!("native maintained owner evidence: {error:?}"))
}

fn decode_hex(value: &str) -> Vec<u8> {
    assert!(value.len().is_multiple_of(2));
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| checked(u8::from_str_radix(checked(std::str::from_utf8(pair)), 16)))
        .collect()
}

fn proof(value: &Value) -> Proof {
    let siblings = value["siblings"]
        .as_array()
        .unwrap_or_else(|| panic!("siblings"))
        .iter()
        .map(|sibling| {
            checked(decode_hex(sibling.as_str().unwrap_or_else(|| panic!("sibling"))).try_into())
        })
        .collect();
    checked(Proof::new(
        checked(u32::try_from(
            value["leaf_index"]
                .as_u64()
                .unwrap_or_else(|| panic!("index")),
        )),
        checked(u32::try_from(
            value["leaf_count"]
                .as_u64()
                .unwrap_or_else(|| panic!("count")),
        )),
        siblings,
    ))
}

struct Fixture {
    original: Vec<u8>,
    raw: RawReceiptEvidence,
    maintenance: Vec<u8>,
    proof: Proof,
    batch: AuthorizedBatch,
    authorization: SequencerAuthorization,
}

fn fixture() -> Fixture {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../tests/fixtures/governance/handover-maintenance");
    let json: Value = checked(serde_json::from_slice(&checked(std::fs::read(
        root.join("evidence.json"),
    ))));
    let read = |name| {
        decode_hex(
            json[name]
                .as_str()
                .unwrap_or_else(|| panic!("missing {name}")),
        )
    };
    let header_bytes = read("signed_header_hex");
    let header = checked(decode_batch_header(&header_bytes));
    let receipt = read("activity_receipt_hex");
    let decoded = checked(layerx_wire::receipt::decode(&receipt));
    let protocol = decoded.protocol().unwrap_or_else(|| panic!("protocol"));
    let key = checked(read("sequencer_public_key_hex").try_into());
    Fixture {
        original: checked(std::fs::read(root.join("activity"))),
        raw: RawReceiptEvidence::new(
            receipt.clone(),
            proof(&json["activity_proof"]),
            header_bytes,
            checked(read("sequencer_signature_hex").try_into()),
        ),
        maintenance: read("maintenance_hex"),
        proof: proof(&json["maintenance_proof"]),
        batch: AuthorizedBatch::new(
            protocol.batch_id(),
            protocol.asset(),
            protocol.previous_state_root(),
            protocol.resulting_state_root(),
            key,
        ),
        authorization: SequencerAuthorization::new(
            header.sequencer_id(),
            key,
            header.batch_number(),
            header.batch_number(),
        ),
    }
}

#[test]
fn actual_owner_handover_requires_original_activity_and_complete_maintenance() {
    let fixture = fixture();
    let kind = checked(ActivityType::new(ModuleId::Governance, 9));
    let registry = checked(ModuleRegistry::new(&[checked(ModuleRegistration::new(
        ModuleId::Governance,
        &[kind],
    ))]));
    let activity = checked(layerx_wire::activity::decode_signed(
        &fixture.original,
        &registry,
    ));
    let expected = NativeOwnerOutcomeContext {
        canonical_activity: &fixture.original,
        actor: activity.actor_did(),
        action_key: activity.idempotency_key(),
        activity_type: kind,
        owner_public_key: checked(activity.authority().try_into()),
        network_id: activity.network_id(),
    };
    let signature = fixture.raw.header_signature();
    let evidence = MaintainedOutcomeEvidence {
        header: fixture.raw.canonical_header(),
        header_signature: &signature,
        activity_proof: fixture.raw.proof(),
        maintenance: &fixture.maintenance,
        maintenance_proof: &fixture.proof,
        authorization: &fixture.authorization,
    };
    let receipts = [fixture.raw.canonical_receipt().to_vec()];
    let verify = |context: &NativeOwnerOutcomeContext<'_>, evidence, receipts: &[Vec<u8>]| {
        VerifiedReceiptEvidence::verify_authorized_native_owner(
            &fixture.raw,
            &fixture.batch,
            context,
            Some((evidence, receipts)),
        )
    };
    let verified = checked(verify(&expected, &evidence, &receipts));
    assert_eq!(
        verified.activity_id(),
        checked(layerx_wire::hash::activity_id(&activity))
    );
    assert_eq!(
        verified.canonical_receipt(),
        fixture.raw.canonical_receipt()
    );
    assert_eq!(verified.result_code(), 0);
    assert_eq!(
        verified.level(),
        layerx_types::verify::VerificationLevel::BATCH_INCLUDED
    );
    assert!(VerifiedReceiptEvidence::verify_authorized_native_owner(
        &fixture.raw,
        &fixture.batch,
        &expected,
        None
    )
    .is_err());
    assert!(verify(&expected, &evidence, &[]).is_err());
    let mut changed = expected;
    changed.action_key[0] ^= 1;
    assert!(verify(&changed, &evidence, &receipts).is_err());
    changed = expected;
    changed.network_id += 1;
    assert!(verify(&changed, &evidence, &receipts).is_err());
    changed = expected;
    changed.owner_public_key[0] ^= 1;
    assert!(verify(&changed, &evidence, &receipts).is_err());
    let mut altered = fixture.maintenance.clone();
    altered[70] ^= 1;
    let corrupted = MaintainedOutcomeEvidence {
        maintenance: &altered,
        ..evidence
    };
    assert!(verify(&expected, &corrupted, &receipts).is_err());
    let mut omitted = receipts;
    omitted[0][30] ^= 1;
    assert!(verify(&expected, &evidence, &omitted).is_err());
}

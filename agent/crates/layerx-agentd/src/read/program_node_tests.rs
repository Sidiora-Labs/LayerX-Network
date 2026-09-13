use super::*;
use layerx_proof::merkle::{build_proof, encode_proof, verify_path};
use layerx_wire::encode::Encoder;
use layerx_wire::receipt::decode_batch_header;

const RECEIPT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../programs/crates/layerx-programs-registry/tests/fixtures/maintained-head/receipt"
));
const MAINTENANCE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../programs/crates/layerx-programs-registry/tests/fixtures/maintained-head/maintenance.receipt"
));
const HEADER: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../programs/crates/layerx-programs-registry/tests/fixtures/maintained-head/header"
));
const SIGNATURE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../programs/crates/layerx-programs-registry/tests/fixtures/maintained-head/header.signature"
));

fn must<T, E: std::fmt::Debug>(value: Result<T, E>) -> T {
    value.unwrap_or_else(|error| panic!("native proof fixture: {error:?}"))
}

fn native_encoding(proof: &Proof) -> Vec<u8> {
    let mut encoder = Encoder::new(1_041);
    must(encoder.structure_header(0x4d50));
    must(encoder.u32(proof.leaf_index()));
    must(encoder.u32(proof.leaf_count()));
    must(encoder.u8(must(u8::try_from(proof.siblings().len()))));
    must(encoder.bytes(&proof.siblings().concat(), 1_024));
    encoder.finish()
}

fn native_document() -> (Value, Proof, Proof) {
    let leaves = [RECEIPT, MAINTENANCE];
    let header = must(decode_batch_header(HEADER));
    let (receipt_proof, root) = must(build_proof(&leaves, 0));
    let (maintenance_proof, maintenance_root) = must(build_proof(&leaves, 1));
    assert_eq!(root, header.receipt_merkle_root());
    assert_eq!(maintenance_root, root);
    let document = serde_json::json!({
        "header_hex": hex::encode(HEADER),
        "header_signature": hex::encode(SIGNATURE),
        "receipt_proof_hex": hex::encode(&native_encoding(&receipt_proof)),
        "batch_identity": {
            "kind": "occupancy_maintenance_v2",
            "receipt_hex": hex::encode(MAINTENANCE),
            "receipt_proof_hex": hex::encode(&native_encoding(&maintenance_proof)),
            "activity_receipts_hex": [hex::encode(RECEIPT)]
        }
    });
    (document, receipt_proof, maintenance_proof)
}

#[test]
fn native_maintained_heads_preserve_authenticated_proof_coordinates() {
    let (mut document, receipt_proof, maintenance_proof) = native_document();
    let evidence = must(batch_evidence(&document));
    let maintenance = evidence
        .maintenance
        .as_ref()
        .unwrap_or_else(|| panic!("native maintenance evidence"));
    assert_eq!(evidence.receipt_proof, receipt_proof);
    assert_eq!(maintenance.receipt_proof, maintenance_proof);
    assert_eq!(maintenance.activity_receipts, [RECEIPT.to_vec()]);
    let root = must(decode_batch_header(HEADER)).receipt_merkle_root();
    must(verify_path(RECEIPT, &evidence.receipt_proof, &root));
    must(verify_path(MAINTENANCE, &maintenance.receipt_proof, &root));
    let (activity_batch, activity_digest) = must(head_identity(RECEIPT, &evidence));
    let receipt = must(decode_receipt(RECEIPT));
    assert_eq!(
        activity_digest,
        must(receipt_digest(&must(encode_unsigned(&receipt))))
    );

    document["receipt_proof_hex"] =
        serde_json::json!(hex::encode(&native_encoding(&maintenance_proof)));
    let maintained_head = must(batch_evidence(&document));
    let (maintenance_batch, maintenance_digest) =
        must(head_identity(MAINTENANCE, &maintained_head));
    assert_eq!(maintenance_batch, activity_batch);
    assert_eq!(maintenance_digest, digest(MAINTENANCE));
    assert!(head_identity(MAINTENANCE, &evidence).is_err());
}

#[test]
fn native_maintained_heads_refuse_public_or_trailing_proof_encodings() {
    let (document, receipt_proof, maintenance_proof) = native_document();
    for identity in [false, true] {
        let proof = if identity {
            &maintenance_proof
        } else {
            &receipt_proof
        };
        let mut trailing = native_encoding(proof);
        trailing.push(0);
        let mut truncated = native_encoding(proof);
        truncated.pop();
        for bytes in [encode_proof(proof), trailing, truncated] {
            let mut changed = document.clone();
            let target = if identity {
                &mut changed["batch_identity"]
            } else {
                &mut changed
            };
            target["receipt_proof_hex"] = serde_json::json!(hex::encode(&bytes));
            assert!(batch_evidence(&changed).is_err());
        }
    }
}

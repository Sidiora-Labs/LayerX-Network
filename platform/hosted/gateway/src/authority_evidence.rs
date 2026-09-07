use crate::SequencerAuthorization;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaintainedBatchDocument {
    header_hex: String,
    header_signature: String,
    receipt_proof_hex: String,
    batch_identity: MaintainedIdentityDocument,
}

#[derive(serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum MaintainedIdentityDocument {
    OccupancyMaintenanceV2 {
        receipt_hex: String,
        receipt_proof_hex: String,
    },
}

impl MaintainedBatchDocument {
    /// Authenticates this selected maintained attachment under configured pins.
    ///
    /// # Errors
    /// Refuses malformed encodings, signatures, inclusion, identity or roots.
    pub fn authorize(
        &self,
        receipt: &[u8],
        facts: &layerx_proof::receipt::AuthorizedBatch,
        authorization: &SequencerAuthorization,
    ) -> Result<layerx_proof::receipt::AuthorizedBatch, &'static str> {
        fn bytes(text: &str) -> Result<Vec<u8>, &'static str> {
            if text.len() > 2_097_152
                || !text.len().is_multiple_of(2)
                || !text.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Err("maintained evidence encoding");
            }
            text.as_bytes()
                .chunks_exact(2)
                .map(|pair| {
                    let digits =
                        std::str::from_utf8(pair).map_err(|_| "maintained evidence encoding")?;
                    u8::from_str_radix(digits, 16).map_err(|_| "maintained evidence encoding")
                })
                .collect()
        }
        fn proof(text: &str) -> Result<layerx_proof::merkle::Proof, &'static str> {
            let encoded = bytes(text)?;
            let path = layerx_wire::receipt::decode_merkle_proof(&encoded)
                .map_err(|_| "maintained proof encoding")?;
            layerx_proof::merkle::Proof::new(
                path.leaf_index(),
                path.leaf_count(),
                path.siblings().to_vec(),
            )
            .map_err(|_| "maintained proof encoding")
        }
        let header = bytes(&self.header_hex)?;
        let signature: [u8; 64] = bytes(&self.header_signature)?
            .try_into()
            .map_err(|_| "maintained signature encoding")?;
        let activity_proof = proof(&self.receipt_proof_hex)?;
        let MaintainedIdentityDocument::OccupancyMaintenanceV2 {
            receipt_hex,
            receipt_proof_hex,
        } = &self.batch_identity;
        let maintenance = bytes(receipt_hex)?;
        let maintenance_proof = proof(receipt_proof_hex)?;
        layerx_proof::receipt::authorized_maintained_activity_batch(
            receipt,
            facts,
            &layerx_proof::receipt::MaintainedOutcomeEvidence {
                header: &header,
                header_signature: &signature,
                activity_proof: &activity_proof,
                maintenance: &maintenance,
                maintenance_proof: &maintenance_proof,
                authorization,
            },
        )
        .map_err(|_| "maintained evidence verification")
    }
}

/// Decodes a present attachment without treating null as historical.
///
/// # Errors
/// Refuses invalid maintained document shapes.
pub fn present_maintained<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<MaintainedBatchDocument>, D::Error> {
    <MaintainedBatchDocument as serde::Deserialize>::deserialize(deserializer).map(Some)
}

#[cfg(test)]
mod maintained_consumer_tests {
    use super::*;
    use layerx_proof::receipt::{verify_outcome, verify_program_state, AuthorizedBatch};
    use std::path::PathBuf;

    fn required<T, E: std::fmt::Debug>(value: Result<T, E>) -> T {
        value.unwrap_or_else(|error| panic!("{error:?}"))
    }
    fn bytes(value: &str) -> Vec<u8> {
        value
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| required(u8::from_str_radix(required(std::str::from_utf8(pair)), 16)))
            .collect()
    }
    fn field(value: &serde_json::Value, name: &str) -> String {
        value[name]
            .as_str()
            .unwrap_or_else(|| panic!("missing {name}"))
            .to_owned()
    }
    fn pins(value: &serde_json::Value) -> SequencerAuthorization {
        required(SequencerAuthorization::from_config(
            &field(value, "sequencer_id"),
            &field(value, "sequencer_public_key"),
            &field(value, "first_batch"),
            &field(value, "last_batch"),
        ))
    }
    fn facts(value: &serde_json::Value) -> AuthorizedBatch {
        let fixed = |name| required(bytes(&field(value, name)).try_into());
        AuthorizedBatch::new(
            fixed("batch_id"),
            fixed("asset"),
            fixed("previous_state_root"),
            fixed("resulting_state_root"),
            fixed("sequencer_public_key"),
        )
    }
    fn captured() -> serde_json::Value {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let path = root.join("tests/fixtures/maintained-authority.json");
        required(serde_json::from_slice(&required(std::fs::read(path))))
    }
    #[test]
    fn real_maintained_response_requires_independent_pins_and_exact_variant() {
        let capture = captured();
        let authority = &capture["authority"];
        let receipt = bytes(&field(&capture, "receipt_hex"));
        let document: MaintainedBatchDocument =
            required(serde_json::from_value(authority["batch_evidence"].clone()));
        let original = facts(authority);
        let authorization = pins(&capture);
        let selected = required(document.authorize(&receipt, &original, &authorization));
        assert!(verify_outcome(&receipt, &selected).is_ok());
        assert!(
            verify_outcome(&receipt, &original).is_err(),
            "maintained response must fail historical verification"
        );
        for name in [
            "sequencer_id",
            "sequencer_public_key",
            "first_batch",
            "last_batch",
        ] {
            let mut changed = capture.clone();
            changed[name] = match name {
                "first_batch" => serde_json::json!(u64::MAX.to_string()),
                "last_batch" => serde_json::json!("0"),
                _ => serde_json::json!("aa".repeat(32)),
            };
            let key = required(bytes(&field(&changed, "sequencer_public_key")).try_into());
            let id = required(bytes(&field(&changed, "sequencer_id")).try_into());
            let first = required(field(&changed, "first_batch").parse());
            let last = required(field(&changed, "last_batch").parse());
            assert!(
                document
                    .authorize(
                        &receipt,
                        &original,
                        &SequencerAuthorization::new(id, key, first, last)
                    )
                    .is_err(),
                "{name}"
            );
        }
        for name in [
            "batch_id",
            "asset",
            "previous_state_root",
            "resulting_state_root",
            "sequencer_public_key",
        ] {
            let mut changed = authority.clone();
            changed[name] = serde_json::json!("aa".repeat(32));
            let result = document.authorize(&receipt, &facts(&changed), &authorization);
            assert!(
                result.is_err()
                    || result.is_ok_and(|selected| verify_outcome(&receipt, &selected).is_err()),
                "{name}"
            );
        }
        for change in ["kind", "unknown", "null"] {
            let mut changed = authority["batch_evidence"].clone();
            match change {
                "kind" => changed["batch_identity"] = serde_json::json!({"kind": "historical"}),
                "unknown" => changed["unexpected"] = serde_json::json!(true),
                _ => changed = serde_json::Value::Null,
            }
            assert!(serde_json::from_value::<MaintainedBatchDocument>(changed).is_err());
        }
    }
    #[test]
    fn historical_document_cannot_enter_maintained_verification() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let historical: serde_json::Value =
            required(serde_json::from_slice(&required(std::fs::read(root.join(
                "../authority/tests/fixtures/real-program-deploy-receipt.json",
            )))));
        let receipt = bytes(&field(&historical, "receipt_hex"));
        let header = required(layerx_wire::receipt::decode_batch_header(&bytes(&field(
            &historical,
            "header_hex",
        ))));
        let decoded = required(layerx_wire::receipt::decode(&receipt));
        let protocol = decoded.protocol().unwrap_or_else(|| panic!("protocol"));
        let historical_facts = AuthorizedBatch::new(
            protocol.batch_id(),
            protocol.asset(),
            header.previous_state_root(),
            header.resulting_state_root(),
            required(bytes(&field(&historical, "sequencer_public_key_hex")).try_into()),
        );
        assert!(verify_program_state(&receipt, &historical_facts).is_ok());
        let capture = captured();
        let document: MaintainedBatchDocument = required(serde_json::from_value(
            capture["authority"]["batch_evidence"].clone(),
        ));
        assert!(document
            .authorize(&receipt, &historical_facts, &pins(&capture))
            .is_err());
    }
}

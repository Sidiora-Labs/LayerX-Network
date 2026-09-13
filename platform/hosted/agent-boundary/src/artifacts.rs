use layerx_proof::inclusion::{verify_receipt, SequencerAuthorization};
use layerx_proof::merkle::Proof;
use layerx_proof::program::{
    verify_authorized_program_execution, AuthorizedProgramExecutionExpectation,
};
use layerx_proof::receipt::{
    authorized_maintained_activity_batch_chain, verify_program_outcome, verify_sequencer_signature,
    AuthorizedBatch, MaintainedOutcomeEvidence,
};
use layerx_wire::hash::{receipt_digest, receipt_execution_batch_id, Domain};
use layerx_wire::receipt::{decode, decode_batch_header, decode_merkle_proof, encode_unsigned};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{decode_hex, hex, parse_hex32, MAX_ACTIVITY_BYTES, PROTOCOL_VERSION};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ArtifactDocument {
    pub activity_id: String,
    pub receipt_digest: String,
    pub terminal_payload: String,
    pub call_graph: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BatchEvidence {
    pub header_hex: String,
    pub header_signature: String,
    pub receipt_proof_hex: String,
    #[serde(default)]
    pub batch_identity: BatchIdentity,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum BatchIdentity {
    Historical {},
    OccupancyMaintenanceV2 {
        receipt_hex: String,
        receipt_proof_hex: String,
        #[serde(default)]
        activity_receipts_hex: Vec<String>,
    },
}

impl Default for BatchIdentity {
    fn default() -> Self {
        Self::Historical {}
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AuthorityDocument {
    pub sequencer_public_key: String,
    pub batch_evidence: BatchEvidence,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredExecution {
    pub version: u8,
    pub sequencer_public_key: String,
    pub evidence: BatchEvidence,
    pub terminal_payload: String,
    pub call_graph: String,
}

fn error(detail: impl std::fmt::Debug) -> String {
    format!("program evidence invalid: {detail:?}")
}

pub(super) fn canonical_hex(text: &str, maximum: usize) -> Result<Vec<u8>, String> {
    let bytes = decode_hex(text, maximum)?;
    if hex(&bytes) != text {
        return Err("program evidence hex is not canonical".into());
    }
    Ok(bytes)
}

pub(super) fn locator(receipt: &[u8]) -> Result<([u8; 32], [u8; 32]), String> {
    let receipt = decode(receipt).map_err(error)?;
    let protocol = receipt.protocol().ok_or_else(|| error("receipt shape"))?;
    let digest = receipt_digest(&encode_unsigned(&receipt).map_err(error)?).map_err(error)?;
    Ok((protocol.batch_id(), digest))
}

pub(super) fn document(
    body: &str,
    activity: [u8; 32],
    digest: [u8; 32],
) -> Result<ArtifactDocument, String> {
    let document: ArtifactDocument = serde_json::from_str(body).map_err(error)?;
    if document.activity_id != hex(&activity) || document.receipt_digest != hex(&digest) {
        return Err(error("artifact identity"));
    }
    let terminal = canonical_hex(&document.terminal_payload, MAX_ACTIVITY_BYTES)?;
    let graph = canonical_hex(&document.call_graph, MAX_ACTIVITY_BYTES)?;
    if terminal.is_empty() != graph.is_empty() {
        return Err(error("partial artifact pair"));
    }
    Ok(document)
}

pub(super) fn verify(
    stored: &StoredExecution,
    receipt_bytes: &[u8],
    activity_id: [u8; 32],
    program_id: [u8; 32],
    network_id: u32,
) -> Result<(), String> {
    if stored.version != 1 {
        return Err(error("artifact journal version"));
    }
    let key = parse_hex32(&stored.sequencer_public_key).ok_or_else(|| error("sequencer key"))?;
    let receipt = verify_sequencer_signature(receipt_bytes, key).map_err(error)?;
    let protocol = receipt.protocol().ok_or_else(|| error("receipt shape"))?;
    if protocol.activity_id() != activity_id
        || protocol.protocol_version() != PROTOCOL_VERSION
        || protocol.module_id() != 9
        || protocol.operation() != 3
        || protocol.module_version() != 4
    {
        return Err(error("receipt identity"));
    }
    let authority = authorized_activity_batch(&stored.evidence, receipt_bytes, key, network_id)?;
    if protocol.batch_id() != authority.batch_id()
        || protocol.previous_state_root() != authority.previous_state_root()
        || protocol.resulting_state_root() != authority.resulting_state_root()
    {
        return Err(error("receipt state or batch identity"));
    }
    let terminal = canonical_hex(&stored.terminal_payload, MAX_ACTIVITY_BYTES)?;
    let graph = canonical_hex(&stored.call_graph, MAX_ACTIVITY_BYTES)?;
    if terminal.is_empty() && graph.is_empty() && protocol.result_code() < 0 {
        if let Some(outcome) = protocol.program_outcome() {
            verify_program_outcome(receipt_bytes, &authority).map_err(error)?;
            let empty_root = empty_call_graph_root();
            if outcome.terminal_kind() != 2
                || outcome.result_code() == 0
                || outcome.call_graph_root() != empty_root
            {
                return Err(error("missing execution artifacts"));
            }
        }
        return Ok(());
    }
    if terminal.is_empty() || graph.is_empty() {
        return Err(error("missing execution artifacts"));
    }
    let outcome = protocol
        .program_outcome()
        .ok_or_else(|| error("program outcome"))?;
    verify_authorized_program_execution(
        receipt_bytes,
        &terminal,
        &graph,
        AuthorizedProgramExecutionExpectation {
            authority,
            activity_id,
            program_id,
            guest_abi_version: outcome.abi_version(),
        },
    )
    .map_err(error)?;
    Ok(())
}

fn authorized_activity_batch(
    evidence: &BatchEvidence,
    receipt_bytes: &[u8],
    key: [u8; 32],
    network_id: u32,
) -> Result<AuthorizedBatch, String> {
    let receipt = decode(receipt_bytes).map_err(error)?;
    let protocol = receipt.protocol().ok_or_else(|| error("receipt shape"))?;
    let header_bytes = canonical_hex(&evidence.header_hex, 4096)?;
    let header = decode_batch_header(&header_bytes).map_err(error)?;
    if header.network_id() != network_id
        || header.protocol_version() != PROTOCOL_VERSION
        || protocol.global_sequence() < header.first_sequence()
        || protocol.global_sequence() > header.last_sequence()
    {
        return Err(error("batch domain or sequence"));
    }
    let signature: [u8; 64] = canonical_hex(&evidence.header_signature, 64)?
        .try_into()
        .map_err(error)?;
    let wire_proof =
        decode_merkle_proof(&canonical_hex(&evidence.receipt_proof_hex, 4096)?).map_err(error)?;
    let proof = Proof::new(
        wire_proof.leaf_index(),
        wire_proof.leaf_count(),
        wire_proof.siblings().to_vec(),
    )
    .map_err(error)?;
    let authorization = SequencerAuthorization::new(header.sequencer_id(), key, 1, u64::MAX);
    let included = verify_receipt(
        receipt_bytes,
        &proof,
        &header_bytes,
        &signature,
        &authorization,
    )
    .map_err(error)?;
    let header = included.header().header();
    let authority = AuthorizedBatch::new(
        protocol.batch_id(),
        protocol.asset(),
        header.previous_state_root(),
        header.resulting_state_root(),
        key,
    );
    match &evidence.batch_identity {
        BatchIdentity::Historical {} => {
            let batch_id = receipt_execution_batch_id(protocol, header).map_err(error)?;
            if protocol.batch_id() != batch_id {
                return Err(error("receipt state or batch identity"));
            }
            Ok(authority)
        }
        BatchIdentity::OccupancyMaintenanceV2 {
            receipt_hex,
            receipt_proof_hex,
            activity_receipts_hex,
        } => {
            let maintenance = canonical_hex(receipt_hex, MAX_ACTIVITY_BYTES)?;
            let wire_proof =
                decode_merkle_proof(&canonical_hex(receipt_proof_hex, 4096)?).map_err(error)?;
            let maintenance_proof = Proof::new(
                wire_proof.leaf_index(),
                wire_proof.leaf_count(),
                wire_proof.siblings().to_vec(),
            )
            .map_err(error)?;
            let receipts = if activity_receipts_hex.is_empty() {
                vec![receipt_bytes.to_vec()]
            } else {
                activity_receipts_hex
                    .iter()
                    .map(|value| canonical_hex(value, MAX_ACTIVITY_BYTES))
                    .collect::<Result<Vec<_>, _>>()?
            };
            authorized_maintained_activity_batch_chain(
                receipt_bytes,
                &authority,
                &MaintainedOutcomeEvidence {
                    header: &header_bytes,
                    header_signature: &signature,
                    activity_proof: &proof,
                    maintenance: &maintenance,
                    maintenance_proof: &maintenance_proof,
                    authorization: &authorization,
                },
                &receipts,
            )
            .map_err(error)
        }
    }
}

fn empty_call_graph_root() -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(Domain::ContextHash.tag());
    digest.update(b"LXP/programs/empty-call-graph/v1\0");
    digest.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn must<T, E: std::fmt::Debug>(result: Result<T, E>) -> T {
        result.unwrap_or_else(|failure| panic!("{failure:?}"))
    }

    #[test]
    fn historical_native_evidence_retains_identity_and_root_checks() {
        let fixture: serde_json::Value = must(serde_json::from_str(include_str!(
            "../../authority/tests/fixtures/real-program-deploy-receipt.json"
        )));
        let field = |name: &str| {
            fixture[name]
                .as_str()
                .unwrap_or_else(|| panic!("fixture field"))
        };
        assert_eq!(fixture["proof_index"], 0);
        assert_eq!(fixture["proof_count"], 1);
        assert_eq!(fixture["proof_siblings"], serde_json::json!([]));
        let mut proof = layerx_wire::encode::Encoder::new(17);
        must(proof.structure_header(0x4d50));
        must(proof.u32(0));
        must(proof.u32(1));
        must(proof.u8(0));
        must(proof.bytes(&[], 0));
        let mut document = serde_json::json!({
            "header_hex": field("header_hex"),
            "header_signature": field("header_signature_hex"),
            "receipt_proof_hex": hex(&proof.finish()),
        });
        let receipt_bytes = must(canonical_hex(field("receipt_hex"), MAX_ACTIVITY_BYTES));
        let receipt = must(decode(&receipt_bytes));
        let protocol = receipt
            .protocol()
            .unwrap_or_else(|| panic!("protocol receipt"));
        let header = must(decode_batch_header(&must(canonical_hex(
            field("header_hex"),
            4096,
        ))));
        let key = parse_hex32(field("sequencer_public_key_hex")).unwrap_or_else(|| panic!("key"));
        for explicit in [false, true] {
            if explicit {
                document["batch_identity"] = serde_json::json!({"kind": "historical"});
            }
            let evidence = must(serde_json::from_value(document.clone()));
            let activity = must(authorized_activity_batch(
                &evidence,
                &receipt_bytes,
                key,
                header.network_id(),
            ));
            assert_eq!(activity.batch_id(), protocol.batch_id());
            assert_eq!(
                activity.previous_state_root(),
                protocol.previous_state_root()
            );
            assert_eq!(
                activity.resulting_state_root(),
                protocol.resulting_state_root()
            );
            assert!(authorized_activity_batch(
                &evidence,
                &receipt_bytes,
                [0; 32],
                header.network_id()
            )
            .is_err());
            assert!(authorized_activity_batch(
                &evidence,
                &receipt_bytes,
                key,
                header.network_id() + 1
            )
            .is_err());
        }
    }

    #[test]
    fn maintained_signed_evidence_authenticates_activity_identity_and_refuses_substitution() {
        let fixture: serde_json::Value = must(serde_json::from_str(include_str!(
            "../../gateway/tests/fixtures/maintained-authority.json"
        )));
        let receipt_bytes = must(canonical_hex(
            fixture["receipt_hex"]
                .as_str()
                .unwrap_or_else(|| panic!("receipt")),
            MAX_ACTIVITY_BYTES,
        ));
        let key = parse_hex32(
            fixture["sequencer_public_key"]
                .as_str()
                .unwrap_or_else(|| panic!("key")),
        )
        .unwrap_or_else(|| panic!("key encoding"));
        let document = &fixture["authority"]["batch_evidence"];
        let evidence: BatchEvidence = must(serde_json::from_value(document.clone()));
        let header = must(decode_batch_header(&must(canonical_hex(
            &evidence.header_hex,
            4096,
        ))));
        let activity = must(authorized_activity_batch(
            &evidence,
            &receipt_bytes,
            key,
            header.network_id(),
        ));
        let receipt = must(decode(&receipt_bytes));
        let protocol = receipt.protocol().unwrap_or_else(|| panic!("protocol"));
        assert_eq!(activity.batch_id(), protocol.batch_id());
        assert_eq!(
            activity.previous_state_root(),
            protocol.previous_state_root()
        );
        assert_eq!(
            activity.resulting_state_root(),
            protocol.resulting_state_root()
        );
        for case in 0..4 {
            let mut changed = document.clone();
            match case {
                0 => {
                    changed
                        .as_object_mut()
                        .unwrap_or_else(|| panic!("evidence"))
                        .remove("batch_identity");
                }
                1 => {
                    changed["batch_identity"] = serde_json::json!({"kind": "historical"});
                }
                2 => {
                    changed["batch_identity"]["receipt_proof_hex"] =
                        changed["receipt_proof_hex"].clone();
                }
                3 => {
                    let text = changed["batch_identity"]["receipt_hex"]
                        .as_str()
                        .unwrap_or_else(|| panic!("maintenance"));
                    let mut bytes = must(canonical_hex(text, MAX_ACTIVITY_BYTES));
                    let last = bytes.len() - 1;
                    bytes[last] ^= 1;
                    changed["batch_identity"]["receipt_hex"] = serde_json::json!(hex(&bytes));
                }
                _ => unreachable!(),
            }
            let evidence = must(serde_json::from_value(changed));
            assert!(
                authorized_activity_batch(&evidence, &receipt_bytes, key, header.network_id())
                    .is_err()
            );
        }
    }

    #[test]
    fn artifact_documents_reject_identity_substitution_partial_pairs_and_noncanonical_hex() {
        let activity = [1; 32];
        let digest = [2; 32];
        let mut value = serde_json::json!({"activity_id": hex(&activity), "receipt_digest": hex(&digest), "terminal_payload": "aa", "call_graph": "bb"});
        assert!(document(&value.to_string(), activity, digest).is_ok());
        assert!(document(&value.to_string(), [3; 32], digest).is_err());
        assert!(document(&value.to_string(), activity, [3; 32]).is_err());
        value["call_graph"] = serde_json::json!("");
        assert!(document(&value.to_string(), activity, digest).is_err());
        value["call_graph"] = serde_json::json!("BB");
        assert!(document(&value.to_string(), activity, digest).is_err());
        assert!(canonical_hex(&"00".repeat(MAX_ACTIVITY_BYTES + 1), MAX_ACTIVITY_BYTES).is_err());
    }
}

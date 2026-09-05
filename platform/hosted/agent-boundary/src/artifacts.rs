use layerx_proof::inclusion::{verify_receipt, SequencerAuthorization};
use layerx_proof::merkle::Proof;
use layerx_proof::program::{
    verify_authorized_program_execution, AuthorizedProgramExecutionExpectation,
};
use layerx_proof::receipt::{verify_program_outcome, verify_sequencer_signature, AuthorizedBatch};
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
    let header_bytes = canonical_hex(&stored.evidence.header_hex, 4096)?;
    let header = decode_batch_header(&header_bytes).map_err(error)?;
    if header.network_id() != network_id
        || header.protocol_version() != PROTOCOL_VERSION
        || protocol.global_sequence() < header.first_sequence()
        || protocol.global_sequence() > header.last_sequence()
    {
        return Err(error("batch domain or sequence"));
    }
    let signature: [u8; 64] = canonical_hex(&stored.evidence.header_signature, 64)?
        .try_into()
        .map_err(error)?;
    let wire_proof = decode_merkle_proof(&canonical_hex(&stored.evidence.receipt_proof_hex, 4096)?)
        .map_err(error)?;
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
    let batch_id = receipt_execution_batch_id(protocol, header).map_err(error)?;
    if protocol.batch_id() != batch_id
        || protocol.previous_state_root() != header.previous_state_root()
        || protocol.resulting_state_root() != header.resulting_state_root()
    {
        return Err(error("receipt state or batch identity"));
    }
    let authority = AuthorizedBatch::new(
        batch_id,
        protocol.asset(),
        header.previous_state_root(),
        header.resulting_state_root(),
        key,
    );
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

fn empty_call_graph_root() -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(Domain::ContextHash.tag());
    digest.update(b"LXP/programs/empty-call-graph/v1\0");
    digest.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;

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

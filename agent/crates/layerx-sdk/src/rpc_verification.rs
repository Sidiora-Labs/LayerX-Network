use std::time::{Duration, Instant};

use layerx_proof::inclusion::{verify_receipt, SequencerAuthorization};
use layerx_proof::merkle::Proof;
use layerx_wire::receipt::{decode_batch_header, Receipt};
use serde_json::Value;
use sha2::{Digest as _, Sha256};

use crate::rpc::{Commitment, RpcClient, RpcError};

pub struct ReceiptPolicy {
    pub protocol_version: u16,
    pub network_id: u32,
    pub sequencer: SequencerAuthorization,
    pub trusted_checkpoint_context_digest: Option<[u8; 32]>,
}

pub struct VerifiedRpcReceipt {
    receipt: Receipt,
    commitment: Commitment,
    canonical: Vec<u8>,
    batch_evidence: Option<VerifiedBatchEvidence>,
    canonical_activity: Option<Vec<u8>>,
}

/// Exact inclusion material retained after SDK verification for daemon evidence ingress.
pub struct VerifiedBatchEvidence {
    proof: Proof,
    canonical_header: Vec<u8>,
    header_signature: [u8; 64],
}

impl VerifiedRpcReceipt {
    #[must_use]
    pub const fn receipt(&self) -> &Receipt {
        &self.receipt
    }
    #[must_use]
    pub const fn commitment(&self) -> Commitment {
        self.commitment
    }
    #[must_use]
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical
    }

    #[must_use]
    pub const fn batch_evidence(&self) -> Option<&VerifiedBatchEvidence> {
        self.batch_evidence.as_ref()
    }

    /// Returns the exact submitted signed activity when this receipt came from
    /// a wallet execution rather than an independent receipt lookup.
    #[must_use]
    pub fn canonical_activity(&self) -> Option<&[u8]> {
        self.canonical_activity.as_deref()
    }

    pub(crate) fn bind_canonical_activity(&mut self, canonical: Vec<u8>) {
        self.canonical_activity = Some(canonical);
    }
}

impl VerifiedBatchEvidence {
    #[must_use]
    pub const fn proof(&self) -> &Proof {
        &self.proof
    }

    #[must_use]
    pub fn canonical_header(&self) -> &[u8] {
        &self.canonical_header
    }

    #[must_use]
    pub const fn header_signature(&self) -> [u8; 64] {
        self.header_signature
    }
}

impl RpcClient {
    /// # Errors
    /// Returns pending on deadline, RPC errors, or an exact verification refusal.
    pub fn wait_for(
        &self,
        activity: [u8; 32],
        commitment: Commitment,
        policy: &ReceiptPolicy,
        timeout: Duration,
    ) -> Result<VerifiedRpcReceipt, RpcError> {
        if activity == [0; 32] || timeout > Duration::from_secs(300) {
            return Err(RpcError::InvalidRequest);
        }
        if commitment == Commitment::Finalised && policy.trusted_checkpoint_context_digest.is_none()
        {
            return Err(RpcError::MissingFinalityTrust);
        }
        let id = super::rpc::encode_hex(&activity);
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or(RpcError::InvalidRequest)?;
        loop {
            if Instant::now() >= deadline {
                return Err(RpcError::Pending {
                    activity_id: activity,
                });
            }
            match self.receipt_at_commitment(&id, activity, commitment, policy, deadline) {
                Ok(Some(receipt)) => return Ok(receipt),
                Ok(None)
                | Err(RpcError::Remote {
                    code: -32001 | -32005,
                    ..
                }) => {}
                Err(error) => return Err(error),
            }
            if Instant::now() >= deadline {
                return Err(RpcError::Pending {
                    activity_id: activity,
                });
            }
            std::thread::sleep(
                Duration::from_millis(50).min(deadline.saturating_duration_since(Instant::now())),
            );
        }
    }

    fn receipt_at_commitment(
        &self,
        id: &str,
        activity: [u8; 32],
        commitment: Commitment,
        policy: &ReceiptPolicy,
        deadline: Instant,
    ) -> Result<Option<VerifiedRpcReceipt>, RpcError> {
        let result = self.get_receipt(id)?;
        if matches!(
            result.get("state").and_then(Value::as_str),
            Some("pending" | "unknown")
        ) {
            return Ok(None);
        }
        if result.get("activity_id").and_then(Value::as_str) != Some(id) {
            return Err(RpcError::InvalidResponse);
        }
        let canonical = hex_field(&result, "receipt", 1_048_576)?;
        let receipt = layerx_proof::receipt::verify_sequencer_signature(
            &canonical,
            policy.sequencer.public_key(),
        )
        .map_err(|_| RpcError::Verification)?;
        let protocol = receipt.protocol().ok_or(RpcError::Verification)?;
        if protocol.activity_id() != activity
            || protocol.protocol_version() != policy.protocol_version
        {
            return Err(RpcError::Verification);
        }
        let batch_evidence = if commitment == Commitment::Executed {
            None
        } else {
            if Instant::now() >= deadline {
                return Ok(None);
            }
            let proof = self.get_proof("receipt", id, None)?;
            let evidence = verify_rpc_inclusion(&proof, id, &canonical, policy)?;
            if commitment == Commitment::Finalised {
                if Instant::now() >= deadline {
                    return Ok(None);
                }
                let node = self.get_node_info()?;
                let checkpoint_id = node
                    .get("latest_finalised_checkpoint")
                    .and_then(Value::as_str)
                    .ok_or(RpcError::InvalidResponse)?;
                if Instant::now() >= deadline {
                    return Ok(None);
                }
                let checkpoint = self.get_checkpoint(checkpoint_id)?;
                verify_rpc_checkpoint(&checkpoint, evidence.canonical_header(), policy)?;
            }
            Some(evidence)
        };
        Ok(Some(VerifiedRpcReceipt {
            receipt,
            commitment,
            canonical,
            batch_evidence,
            canonical_activity: None,
        }))
    }
}

fn verify_rpc_inclusion(
    value: &Value,
    id: &str,
    receipt: &[u8],
    policy: &ReceiptPolicy,
) -> Result<VerifiedBatchEvidence, RpcError> {
    if value["kind"] != "receipt"
        || value["activity_id"] != id
        || hex_field(value, "canonical_value", 1_048_576)? != receipt
    {
        return Err(RpcError::Verification);
    }
    let signed = &value["signed_header"];
    let header = hex_field(signed, "canonical_header", 65536)?;
    let signature: [u8; 64] = hex_field(signed, "signature", 64)?
        .try_into()
        .map_err(|_| RpcError::InvalidResponse)?;
    let decoded = decode_batch_header(&header).map_err(|_| RpcError::Verification)?;
    if decoded.protocol_version() != policy.protocol_version
        || decoded.network_id() != policy.network_id
    {
        return Err(RpcError::Verification);
    }
    let path = &value["proof"];
    let index = path["leaf_index"]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .ok_or(RpcError::InvalidResponse)?;
    let count = path["leaf_count"]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .ok_or(RpcError::InvalidResponse)?;
    let siblings = path["siblings"]
        .as_array()
        .filter(|v| v.len() <= 32)
        .ok_or(RpcError::InvalidResponse)?;
    let siblings = siblings
        .iter()
        .map(|s| {
            decode_hex(s.as_str().ok_or(RpcError::InvalidResponse)?, 32)?
                .try_into()
                .map_err(|_| RpcError::InvalidResponse)
        })
        .collect::<Result<Vec<[u8; 32]>, RpcError>>()?;
    let proof = Proof::new(index, count, siblings).map_err(|_| RpcError::Verification)?;
    verify_receipt(receipt, &proof, &header, &signature, &policy.sequencer)
        .map_err(|_| RpcError::Verification)?;
    Ok(VerifiedBatchEvidence {
        proof,
        canonical_header: header,
        header_signature: signature,
    })
}

fn verify_rpc_checkpoint(
    value: &Value,
    header: &[u8],
    policy: &ReceiptPolicy,
) -> Result<(), RpcError> {
    let context = hex_field(value, "context", 128 * 1024)?;
    let digest: [u8; 32] = Sha256::digest(&context).into();
    if Some(digest) != policy.trusted_checkpoint_context_digest {
        return Err(RpcError::Verification);
    }
    let checkpoint = hex_field(value, "checkpoint", 1_048_576)?;
    let candidate = layerx_client::evidence::FinalityEvidenceCandidate::from_exact_bytes(
        checkpoint,
        context,
        policy.protocol_version,
        policy.network_id,
    )
    .map_err(|_| RpcError::Verification)?;
    if candidate.canonical_header() != header
        || hex_field(value, "canonical_header", 65536)? != header
        || hex_field(value, "checkpoint_id", 32)? != candidate.checkpoint_id()
    {
        return Err(RpcError::Verification);
    }
    Ok(())
}

fn hex_field(value: &Value, name: &str, maximum: usize) -> Result<Vec<u8>, RpcError> {
    decode_hex(
        value
            .get(name)
            .and_then(Value::as_str)
            .ok_or(RpcError::InvalidResponse)?,
        maximum,
    )
}
fn decode_hex(value: &str, maximum: usize) -> Result<Vec<u8>, RpcError> {
    if value.is_empty()
        || value.len() > maximum * 2
        || !value.len().is_multiple_of(2)
        || !value.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(RpcError::InvalidResponse);
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|b| {
            std::str::from_utf8(b)
                .ok()
                .and_then(|s| u8::from_str_radix(s, 16).ok())
                .ok_or(RpcError::InvalidResponse)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn finality_requires_the_exact_trusted_context_and_header() -> Result<(), RpcError> {
        let fields: std::collections::BTreeMap<_, _> =
            include_str!("../../../../tests/vectors/finality_evidence_v1.vec")
                .lines()
                .filter_map(|l| l.split_once('='))
                .collect();
        let get = |name| fields.get(name).copied().ok_or(RpcError::InvalidResponse);
        let checkpoint = decode_hex(get("checkpoint_payload")?, 1_048_576)?;
        let context = decode_hex(get("finality_proof")?, 131_072)?;
        let protocol = get("protocol_version")?
            .parse()
            .map_err(|_| RpcError::InvalidResponse)?;
        let network = get("network_id")?
            .parse()
            .map_err(|_| RpcError::InvalidResponse)?;
        let candidate = layerx_client::evidence::FinalityEvidenceCandidate::from_exact_bytes(
            checkpoint.clone(),
            context.clone(),
            protocol,
            network,
        )
        .map_err(|_| RpcError::Verification)?;
        let policy = ReceiptPolicy {
            protocol_version: protocol,
            network_id: network,
            sequencer: SequencerAuthorization::new([1; 32], [2; 32], 1, 100),
            trusted_checkpoint_context_digest: Some(Sha256::digest(&context).into()),
        };
        let mut response = json!({"checkpoint":crate::rpc::encode_hex(&checkpoint),"context":crate::rpc::encode_hex(&context),"checkpoint_id":crate::rpc::encode_hex(&candidate.checkpoint_id()),"canonical_header":crate::rpc::encode_hex(candidate.canonical_header())});
        verify_rpc_checkpoint(&response, candidate.canonical_header(), &policy)?;
        response["checkpoint_id"] = json!("00".repeat(32));
        assert!(verify_rpc_checkpoint(&response, candidate.canonical_header(), &policy).is_err());
        response["checkpoint_id"] = json!(crate::rpc::encode_hex(&candidate.checkpoint_id()));
        response["context"] = json!("00");
        assert!(verify_rpc_checkpoint(&response, candidate.canonical_header(), &policy).is_err());
        response["context"] = json!(crate::rpc::encode_hex(&context));
        assert!(verify_rpc_checkpoint(&response, &[0; 32], &policy).is_err());
        Ok(())
    }
    #[test]
    fn acknowledgement_is_not_inclusion() {
        let policy = ReceiptPolicy {
            protocol_version: 3,
            network_id: 17,
            sequencer: SequencerAuthorization::new([1; 32], [2; 32], 1, 10),
            trusted_checkpoint_context_digest: None,
        };
        assert!(verify_rpc_inclusion(&json!({"state":"accepted"}), "01", &[1], &policy).is_err());
    }
}

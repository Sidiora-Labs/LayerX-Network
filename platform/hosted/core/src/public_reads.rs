use super::{
    connect_client, decode_account_value, fixed_hex, hex_encode, refusal, success, Config, Request,
    Response, SequencerAuthorization, VerificationLevel,
};

pub(super) fn account(config: &Config, id: &str) -> Response {
    let Ok(account_id) = fixed_hex::<32>("account_id", id) else {
        return refusal(400, "invalid_account_id", None);
    };
    let Ok(mut client) = connect_client(config) else {
        return refusal(503, "node_unavailable", Some(5));
    };
    let authorization = SequencerAuthorization::new(
        config.sequencer_id,
        client.handshake().node().authorised_sequencer_key,
        1,
        u64::MAX,
    );
    let Ok(value) = client.account(account_id, VerificationLevel::UNVERIFIED, 1, authorization)
    else {
        return refusal(503, "account_evidence_unavailable", Some(5));
    };
    let Ok(account) = decode_account_value(account_id, value.canonical_bytes()) else {
        return refusal(502, "invalid_account_evidence", None);
    };
    let Ok(name) = std::str::from_utf8(&account.name) else {
        return refusal(502, "invalid_account_name", None);
    };
    success(&serde_json::json!({
        "account_id": hex_encode(&account.account_id),
        "name": name,
        "asset_id": hex_encode(&account.asset_id()),
        "balance": account.balance().to_string(),
        "next_sequence": account.next_sequence.to_string(),
        "frozen": account.frozen,
        "canonical_value": hex_encode(value.canonical_bytes()),
        "proof_material": hex_encode(value.proof_material()),
        "observed_head_sequence": value.freshness().observed_head_sequence.to_string(),
        "batch_number": value.freshness().batch_number.to_string()
    }))
}

pub(super) fn route(config: &Config, request: &Request) -> Option<Response> {
    if request.path == "/v1/node-info" {
        return Some(if request.method != "GET" {
            refusal(405, "method_not_allowed", None)
        } else if request.query.is_some() {
            refusal(400, "invalid_request", None)
        } else {
            node_info(config)
        });
    }
    let parts: Vec<_> = request.path.split('/').collect();
    let target = match parts.as_slice() {
        ["", "v1", "accounts", id, "balance"] | ["", "v1", "accounts", id] => Some(*id),
        ["", "v1", "batches" | "checkpoints", id] => {
            return Some(if request.method != "GET" {
                refusal(405, "method_not_allowed", None)
            } else if request.query.is_some() {
                refusal(400, "invalid_request", None)
            } else {
                evidence(config, parts[2], id)
            });
        }
        ["", "v1", "proofs", kind, id] => {
            return Some(if request.method != "GET" {
                refusal(405, "method_not_allowed", None)
            } else if request.query.is_some() {
                refusal(400, "invalid_request", None)
            } else {
                proof(config, kind, id, None)
            });
        }
        ["", "v1", "proofs", "account", activity, account] => {
            return Some(if request.method != "GET" {
                refusal(405, "method_not_allowed", None)
            } else if request.query.is_some() {
                refusal(400, "invalid_request", None)
            } else {
                proof(config, "account", activity, Some(account))
            });
        }
        ["", "v1", "dids", did, "sequence"] => {
            return Some(if request.method != "GET" {
                refusal(405, "method_not_allowed", None)
            } else if request.query.is_some() {
                refusal(400, "invalid_request", None)
            } else {
                sequence(config, did)
            });
        }
        ["", "v1", "dids", did, "accounts"] => {
            return Some(if request.method != "GET" {
                refusal(405, "method_not_allowed", None)
            } else if request.query.is_some()
                || layerx_types::ids::Did::new(did.as_bytes()).is_err()
            {
                refusal(400, "invalid_did", None)
            } else {
                refusal(503, "did_account_listing_unavailable", Some(30))
            });
        }
        _ => return None,
    };
    Some(if request.method != "GET" {
        refusal(405, "method_not_allowed", None)
    } else if request.query.is_some() {
        refusal(400, "invalid_request", None)
    } else {
        account(config, target?)
    })
}

fn node_info(config: &Config) -> Response {
    let Ok(client) = connect_client(config) else {
        return refusal(503, "node_unavailable", Some(5));
    };
    let node = client.handshake().node();
    success(&serde_json::json!({
        "protocol_version": node.protocol_version,
        "network_id": node.network_id,
        "chain_head_sequence": node.chain_head_sequence.to_string(),
        "latest_sealed_batch": node.latest_sealed_batch.to_string(),
        "latest_finalised_checkpoint": hex_encode(&node.latest_finalised_checkpoint),
        "authorised_sequencer_key": hex_encode(&node.authorised_sequencer_key),
        "capabilities": node.advertised_capabilities
    }))
}

fn evidence(config: &Config, kind: &str, id: &str) -> Response {
    use layerx_client::evidence::CheckpointSelector;
    if kind == "batches" {
        let Ok(number) = id.parse::<u64>() else {
            return refusal(400, "invalid_batch", None);
        };
        if number == 0 || number.to_string() != id {
            return refusal(400, "invalid_batch", None);
        }
        let Ok(mut client) = connect_client(config) else {
            return refusal(503, "node_unavailable", Some(5));
        };
        return match client.batch_header(number, 1) {
            Ok(header) => success(&serde_json::json!({
                "batch_number": number.to_string(), "canonical_header": hex_encode(header.canonical_bytes()),
                "signature": hex_encode(&header.signature), "sequencer_id": hex_encode(&header.sequencer_id),
                "sequencer_public_key": hex_encode(&header.sequencer_public_key),
                "first_batch_number": header.first_batch_number.to_string(),
                "last_batch_number": header.last_batch_number.to_string()
            })),
            Err(_) => refusal(503, "batch_evidence_unavailable", Some(5)),
        };
    }
    let Ok(id) = fixed_hex::<32>("checkpoint_id", id) else {
        return refusal(400, "invalid_checkpoint", None);
    };
    if id == [0; 32] {
        return refusal(400, "invalid_checkpoint", None);
    }
    let Ok(mut client) = connect_client(config) else {
        return refusal(503, "node_unavailable", Some(5));
    };
    match client.checkpoint_evidence(CheckpointSelector::Identifier(id), 1) {
        Ok(checkpoint) => success(&serde_json::json!({
            "checkpoint_id": hex_encode(&id), "checkpoint": hex_encode(checkpoint.checkpoint_bytes()),
            "context": hex_encode(checkpoint.context_bytes())
        })),
        Err(_) => refusal(503, "checkpoint_evidence_unavailable", Some(5)),
    }
}

fn sequence(config: &Config, did: &str) -> Response {
    let Ok(actor) = layerx_types::ids::Did::new(did.as_bytes()) else {
        return refusal(400, "invalid_did", None);
    };
    let Ok(mut client) = connect_client(config) else {
        return refusal(503, "node_unavailable", Some(5));
    };
    match client.preparation_state(&actor, 1) {
        Ok(snapshot) => success(&serde_json::json!({
            "did": did, "next_sequence": snapshot.account_sequence.to_string(),
            "observed_head_sequence": snapshot.observed_head_sequence.to_string(),
            "state_root": hex_encode(&snapshot.observed_state_root),
            "verification": "authenticated_node_snapshot"
        })),
        Err(_) => refusal(503, "sequence_unavailable", Some(5)),
    }
}

fn proof(config: &Config, kind: &str, id: &str, account: Option<&str>) -> Response {
    use layerx_client::evidence::{ProofBundleSelector, VerifiedProofBundle};
    let Ok(identifier) = fixed_hex::<32>("activity_id", id) else {
        return refusal(400, "invalid_proof_selector", None);
    };
    if identifier == [0; 32] {
        return refusal(400, "invalid_proof_selector", None);
    }
    let selector = match kind {
        "activity" => ProofBundleSelector::Activity(identifier),
        "receipt" => ProofBundleSelector::Receipt(identifier),
        "account" => {
            let Some(account) = account else {
                return refusal(400, "invalid_proof_selector", None);
            };
            let Ok(account_id) = fixed_hex::<32>("account_id", account) else {
                return refusal(400, "invalid_proof_selector", None);
            };
            if account_id == [0; 32] {
                return refusal(400, "invalid_proof_selector", None);
            }
            ProofBundleSelector::AccountState {
                activity_id: identifier,
                account_id,
            }
        }
        _ => return refusal(400, "invalid_proof_selector", None),
    };
    let Ok(registry) = super::submission_registry() else {
        return refusal(503, "registry_unavailable", Some(5));
    };
    let Ok(mut client) = connect_client(config) else {
        return refusal(503, "node_unavailable", Some(5));
    };
    let Ok(bundle) = client.proof_bundle(selector, 1, &registry) else {
        return refusal(503, "proof_evidence_unavailable", Some(5));
    };
    let proof = match &bundle {
        VerifiedProofBundle::Activity { proof, .. }
        | VerifiedProofBundle::Receipt { proof, .. } => {
            serde_json::json!({"leaf_index": proof.leaf_index(), "leaf_count": proof.leaf_count(),
                "siblings": proof.siblings().iter().map(|v| hex_encode(v)).collect::<Vec<_>>()})
        }
        VerifiedProofBundle::Account { proof_material, .. }
        | VerifiedProofBundle::MaintainedAccount { proof_material, .. } => {
            serde_json::json!({"canonical_bytes": hex_encode(proof_material)})
        }
    };
    let header = bundle.signed_header();
    success(&serde_json::json!({
        "kind": kind, "activity_id": id, "canonical_value": hex_encode(bundle.canonical_bytes()),
        "proof": proof, "account_id": account,
        "signed_header": {"canonical_header": hex_encode(&header.canonical_bytes),
            "signature": hex_encode(&header.signature), "sequencer_id": hex_encode(&header.sequencer_id),
            "public_key": hex_encode(&header.public_key),
            "first_batch_number": header.first_batch_number.to_string(),
            "last_batch_number": header.last_batch_number.to_string()}
    }))
}

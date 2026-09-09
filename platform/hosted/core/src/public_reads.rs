use sha2::{Digest, Sha256};

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

fn snapshot_context(handshake: &super::Handshake) -> layerx_client::payments::SnapshotContext {
    layerx_client::payments::SnapshotContext {
        interface_version: handshake.node().interface_version,
        correlation_id: 1,
        minimum_sequence: handshake.node().chain_head_sequence,
    }
}

fn asset_json(asset: &layerx_client::payments::AssetMetadata) -> serde_json::Value {
    serde_json::json!({
        "asset_id": hex_encode(&asset.asset_id), "symbol": String::from_utf8_lossy(&asset.symbol),
        "name": asset.name, "decimals": asset.decimals, "custody_kind": asset.custody_kind,
        "custody_reference": hex_encode(&asset.custody_reference), "paused": asset.paused,
        "supply_cap": asset.supply_cap.to_string(), "issuer_did": hex_encode(&asset.issuer_did),
        "issuer_kind": asset.issuer_kind, "total_units": asset.total_units.to_string(),
        "salt": hex_encode(&asset.salt)
    })
}

fn assets(config: &Config, id: Option<[u8; 32]>) -> Response {
    let Ok((mut transport, handshake)) = super::connect_raw(config) else {
        return refusal(503, "node_unavailable", Some(5));
    };
    let context = snapshot_context(&handshake);
    if let Some(id) = id {
        return match layerx_client::payments::get_asset(&mut transport, id, context) {
            Ok(snapshot) => success(&serde_json::json!({
                "asset": asset_json(&snapshot.value),
                "observed_head_sequence": snapshot.observed_sequence.to_string(),
                "state_root": hex_encode(&snapshot.state_root),
                "verification": "authenticated_committed_snapshot"
            })),
            Err(_) => refusal(503, "asset_evidence_unavailable", Some(5)),
        };
    }
    match layerx_client::payments::list_assets(&mut transport, None, context) {
        Ok(snapshot) => success(&serde_json::json!({
            "assets": snapshot.value.iter().map(asset_json).collect::<Vec<_>>(),
            "observed_head_sequence": snapshot.observed_sequence.to_string(),
            "state_root": hex_encode(&snapshot.state_root),
            "verification": "authenticated_committed_snapshot"
        })),
        Err(_) => refusal(503, "asset_evidence_unavailable", Some(5)),
    }
}

fn did_accounts(config: &Config, did: &str) -> Response {
    use layerx_client::{
        evidence::RootSelector,
        head::HeadTracker,
        read::{ReadContext, Requested},
    };
    let Ok(did_value) = layerx_types::ids::Did::new(did.as_bytes()) else {
        return refusal(400, "invalid_did", None);
    };
    let Ok((mut transport, handshake)) = super::connect_raw(config) else {
        return refusal(503, "node_unavailable", Some(5));
    };
    let node = handshake.node();
    if node.interface_version.minor < 5 {
        return refusal(503, "did_account_listing_unavailable", Some(5));
    }
    let context = ReadContext {
        interface_version: node.interface_version,
        correlation_id: 1,
        expected_protocol_version: node.protocol_version,
        expected_network_id: config.network_id,
        requested: Requested::new(VerificationLevel::UNVERIFIED),
        head: HeadTracker::new(node).current(),
        sequencer_authorization: SequencerAuthorization::new(
            config.sequencer_id,
            node.authorised_sequencer_key,
            1,
            u64::MAX,
        ),
        handshake_sequencer_key: node.authorised_sequencer_key,
        root_selector: RootSelector::Latest,
    };
    let Ok(values) = layerx_client::read::did_accounts(&mut transport, &did_value, context) else {
        return refusal(503, "did_account_listing_unavailable", Some(5));
    };
    let mut accounts = Vec::with_capacity(values.len());
    for value in values {
        let bytes = value.canonical_bytes();
        let Some(length) = bytes
            .get(..2)
            .map(|v| usize::from(u16::from_be_bytes([v[0], v[1]])))
        else {
            return refusal(502, "invalid_account_evidence", None);
        };
        let Some(name) = bytes
            .get(2..2 + length)
            .and_then(|v| std::str::from_utf8(v).ok())
        else {
            return refusal(502, "invalid_account_name", None);
        };
        let Ok(length) = u32::try_from(name.len()) else {
            return refusal(502, "invalid_account_name", None);
        };
        let mut hash = Sha256::new();
        hash.update(b"LX:ACCOUNT:v1");
        hash.update(length.to_be_bytes());
        hash.update(name.as_bytes());
        let id = hash.finalize().into();
        let Ok(account) = decode_account_value(id, bytes) else {
            return refusal(502, "invalid_account_evidence", None);
        };
        accounts.push(serde_json::json!({
            "account_id": hex_encode(&id), "name": name,
            "asset_id": hex_encode(&account.asset_id()), "balance": account.balance().to_string(),
            "next_sequence": account.next_sequence.to_string(), "frozen": account.frozen,
            "canonical_value": hex_encode(bytes), "proof_material": hex_encode(value.proof_material()),
            "observed_head_sequence": value.freshness().observed_head_sequence.to_string(),
            "batch_number": value.freshness().batch_number.to_string()
        }));
    }
    success(&serde_json::json!({"did": did, "accounts": accounts,
        "verification": "authenticated_node_snapshot"}))
}

fn estimate_fee(config: &Config, body: &[u8]) -> Response {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return refusal(400, "invalid_fee_request", None);
    };
    let Some(hex) = value
        .get("canonical_hex")
        .and_then(serde_json::Value::as_str)
    else {
        return refusal(400, "invalid_fee_request", None);
    };
    let Ok(canonical) = layerx_platform_core::hex_decode(hex) else {
        return refusal(400, "invalid_fee_request", None);
    };
    let operations = [1, 4, 5, 6, 7, 8, 10, 11]
        .into_iter()
        .map(|ordinal| super::ActivityType::new(super::ModuleId::Asset, ordinal))
        .collect::<Result<Vec<_>, _>>();
    let Some(registry) = operations
        .ok()
        .and_then(|operations| {
            super::ModuleRegistration::new(super::ModuleId::Asset, &operations).ok()
        })
        .and_then(|asset| {
            let registry = super::submission_registry().ok()?;
            let mut registrations = registry.registrations().to_vec();
            registrations.retain(|entry| entry.module() != super::ModuleId::Asset);
            registrations.push(asset);
            super::ModuleRegistry::new(&registrations).ok()
        })
    else {
        return refusal(503, "registry_unavailable", Some(5));
    };
    let Ok(activity) = layerx_wire::activity::decode_signed(&canonical, &registry) else {
        return refusal(400, "invalid_activity", None);
    };
    if activity.protocol_version() != layerx_wire::limits::STATE_COMMITMENT_PROTOCOL_VERSION
        || activity.network_id() != config.network_id
    {
        return refusal(400, "invalid_activity_domain", None);
    }
    if activity.activity_type().module() != super::ModuleId::Asset {
        return refusal(503, "fee_execution_meter_unavailable", Some(5));
    }
    let Ok((mut transport, handshake)) = super::connect_raw(config) else {
        return refusal(503, "node_unavailable", Some(5));
    };
    let kind = activity.activity_type().value();
    match layerx_client::payments::estimate_fee(
        &mut transport,
        kind,
        canonical.len() as u64,
        0,
        0,
        snapshot_context(&handshake),
    ) {
        Ok(snapshot) => {
            if snapshot.value.canonical_schedule[50..82]
                .iter()
                .any(|byte| *byte != 0)
            {
                return refusal(503, "fee_execution_meter_unavailable", Some(5));
            }
            success(&serde_json::json!({
                "fee": snapshot.value.fee.to_string(),
                "parameter_version": snapshot.value.parameter_version,
                "canonical_schedule": hex_encode(&snapshot.value.canonical_schedule),
                "canonical_bytes": canonical.len(),
                "observed_head_sequence": snapshot.observed_sequence.to_string(),
                "state_root": hex_encode(&snapshot.state_root),
                "verification": "authenticated_committed_snapshot"
            }))
        }
        Err(_) => refusal(503, "fee_evidence_unavailable", Some(5)),
    }
}

fn asset_route(config: &Config, request: &Request) -> Response {
    if request.method != "GET" {
        refusal(405, "method_not_allowed", None)
    } else if request.query.is_some() || !request.body.is_empty() {
        refusal(400, "invalid_request", None)
    } else if request.path != "/v1/assets"
        && request
            .path
            .strip_prefix("/v1/assets/")
            .and_then(|id| fixed_hex::<32>("asset_id", id).ok())
            .is_none_or(|id| id == [0; 32])
    {
        refusal(400, "invalid_asset_id", None)
    } else {
        assets(
            config,
            request
                .path
                .strip_prefix("/v1/assets/")
                .and_then(|id| fixed_hex::<32>("asset_id", id).ok()),
        )
    }
}

pub(super) fn route(config: &Config, request: &Request) -> Option<Response> {
    if request.path == "/v1/assets" || request.path.starts_with("/v1/assets/") {
        return Some(asset_route(config, request));
    }
    if request.path == "/v1/fees/estimate" {
        return Some(if request.method != "POST" {
            refusal(405, "method_not_allowed", None)
        } else if request.query.is_some() || !valid_fee_request(&request.body) {
            refusal(400, "invalid_fee_request", None)
        } else {
            estimate_fee(config, &request.body)
        });
    }
    if request.path == "/v1/node-info" {
        return Some(if request.method != "GET" {
            refusal(405, "method_not_allowed", None)
        } else if request.query.is_some() {
            refusal(400, "invalid_request", None)
        } else {
            node_info(config)
        });
    }
    if let Some(sequence) = request.path.strip_prefix("/internal/v1/receipt-events/") {
        return Some(receipt_event(config, request, sequence));
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
                did_accounts(config, did)
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
            "context": hex_encode(checkpoint.context_bytes()),
            "canonical_header": hex_encode(checkpoint.canonical_header())
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

fn receipt_event(config: &Config, request: &Request, sequence: &str) -> Response {
    use subtle::ConstantTimeEq;
    let Some(token) = &config.receipt_events_token else {
        return refusal(503, "receipt_events_not_configured", None);
    };
    let supplied = request
        .headers
        .get("authorization")
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or("");
    if !bool::from(supplied.as_bytes().ct_eq(token.as_bytes())) {
        return refusal(401, "unauthorized", None);
    }
    if request.method != "GET" {
        return refusal(405, "method_not_allowed", None);
    }
    let Ok(number) = sequence.parse::<u64>() else {
        return refusal(400, "invalid_sequence", None);
    };
    if number == 0
        || number.to_string() != sequence
        || request.query.is_some()
        || !request.body.is_empty()
    {
        return refusal(400, "invalid_sequence", None);
    }
    let Ok((mut transport, handshake)) = super::connect_raw(config) else {
        return refusal(503, "node_unavailable", Some(5));
    };
    let mut selector = vec![3];
    selector.extend_from_slice(&number.to_be_bytes());
    match super::lookup_receipt_selector(&mut transport, &handshake, selector, 1, true) {
        Ok(Some(bytes)) => {
            match super::receipt_facts(&bytes, handshake.node().authorised_sequencer_key) {
                Ok(facts) if facts.global_sequence == number => {
                    success(&super::receipt_result(&facts))
                }
                _ => refusal(502, "invalid_receipt_event", None),
            }
        }
        Ok(None) => super::json_response(202, &serde_json::json!({"result":{"state":"pending"}})),
        Err(_) => refusal(503, "receipt_events_unavailable", Some(5)),
    }
}

fn valid_fee_request(body: &[u8]) -> bool {
    let Ok(serde_json::Value::Object(value)) = serde_json::from_slice(body) else {
        return false;
    };
    value.len() == 1
        && value
            .get("canonical_hex")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|hex| {
                !hex.is_empty()
                    && hex.len() <= 1024 * 1024
                    && hex.len().is_multiple_of(2)
                    && hex.bytes().all(|b| b.is_ascii_hexdigit())
            })
}

#[cfg(test)]
mod tests {
    #[test]
    fn fee_request_requires_bounded_canonical_bytes() {
        assert!(super::valid_fee_request(br#"{"canonical_hex":"abcd"}"#));
        for body in [
            br"{}".as_slice(),
            br#"{"canonical_hex":""}"#,
            br#"{"canonical_hex":"abc"}"#,
            br#"{"canonical_hex":"zz"}"#,
            br#"{"canonical_hex":"ab","extra":1}"#,
        ] {
            assert!(!super::valid_fee_request(body));
        }
    }
}

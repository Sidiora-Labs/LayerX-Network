use super::*;

pub(super) fn account(config: &Config, id: &str) -> Response {
    let Ok(account_id) = fixed_hex::<32>(id) else {
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
    let value = match client.account(account_id, VerificationLevel::UNVERIFIED, 1, authorization) {
        Ok(value) => value,
        Err(_) => return refusal(503, "account_evidence_unavailable", Some(5)),
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
    let parts: Vec<_> = request.path.split('/').collect();
    let target = match parts.as_slice() {
        ["", "v1", "accounts", id, "balance"] => Some(*id),
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

use super::*;
use std::os::unix::fs::PermissionsExt;

fn principal() -> PrincipalPolicy {
    serde_json::from_value(value!({
        "tenant": "tenant", "principal": "principal", "account_id": hex::encode(&[1;32]), "asset_id": hex::encode(&[2;32]),
        "activities": [hex::encode(&[3;32])], "budgets": [hex::encode(&[4;32])],
        "maximum_age_seconds": 60, "maximum_age_sequences": 100,
        "identities": [{"did": "did:layerx:test", "authorities": [{"kind": "primary_key", "id": hex::encode(&[5;32])}],
            "revocation_sequence": 1, "frozen": false,
            "evidence": {"activity_id": hex::encode(&[3;32]), "receipt_digest": hex::encode(&[6;32])},
            "capabilities": [],
            "rotation": {"policy_revision": 1, "required_delay_seconds": 10, "maximum_delay_seconds": 20, "effective_sequence": 1,
                "evidence": {"activity_id": hex::encode(&[3;32]), "receipt_digest": hex::encode(&[6;32])}},
            "recovery": {"policy_revision": 2, "required_delay_seconds": 20, "maximum_delay_seconds": 40, "effective_sequence": 1,
                "evidence": {"activity_id": hex::encode(&[3;32]), "receipt_digest": hex::encode(&[6;32])}}
        }]
    })).unwrap_or_else(|e| panic!("policy: {e}"))
}

fn result_status(result: Result<Response, Response>) -> u16 {
    result.unwrap_or_else(|r| r).status
}

#[test]
fn registry_uses_exact_file_bytes_and_refuses_unprotected_invalid_missing_sources() {
    let root = std::env::temp_dir().join(format!("human-registry-{}", std::process::id()));
    fs::create_dir(&root).unwrap_or_else(|e| panic!("directory: {e}"));
    let path = root.join("registry.json");
    assert_eq!(result_status(registry(&path)), 503);
    let bytes = br#"{"modules":[{"module":9,"ordinals":[1,7]}]}"#;
    fs::write(&path, bytes).unwrap_or_else(|e| panic!("registry: {e}"));
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
        .unwrap_or_else(|e| panic!("mode: {e}"));
    let response = registry(&path).unwrap_or_else(|_| panic!("valid registry"));
    let body: Value =
        serde_json::from_slice(&response.body).unwrap_or_else(|e| panic!("JSON: {e}"));
    assert_eq!(body["revision"], hex::encode(&digest(bytes)));
    assert_eq!(
        body["modules"][0]["activity_types"],
        value!([589_825, 589_831])
    );
    fs::write(&path, br#"{"modules":[{"module":9,"ordinals":[7,1]}]}"#)
        .unwrap_or_else(|e| panic!("registry: {e}"));
    assert_eq!(result_status(registry(&path)), 503);
    fs::write(&path, bytes).unwrap_or_else(|e| panic!("registry: {e}"));
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644))
        .unwrap_or_else(|e| panic!("mode: {e}"));
    assert_eq!(result_status(registry(&path)), 503);
    fs::remove_dir_all(root).unwrap_or_else(|e| panic!("cleanup: {e}"));
}

#[test]
fn authorized_batch_requires_explicit_activity_binding() {
    let p = principal();
    assert!(authorize_activity(&p, &hex::encode(&[3; 32])).is_ok());
    assert_eq!(
        authorize_activity(&p, &hex::encode(&[8; 32]))
            .err()
            .map(|r| r.status),
        Some(403)
    );
    assert_eq!(
        authorize_activity(&p, "bad").err().map(|r| r.status),
        Some(400)
    );
}

#[test]
fn balance_context_zero_account_does_not_invent_receipts_or_head() {
    let summary = account_summary(&principal(), &[]).unwrap_or_else(|_| panic!("zero account"));
    assert_eq!(summary["remaining"], "0");
    assert_eq!(summary["receipt_digests"], value!([]));
    assert_eq!(summary["checkpoint_digests"], value!([]));
    assert!(summary["observed_head_sequence"].is_null());
}

#[test]
fn core_clock_requires_two_anchors_and_checked_measured_rate() {
    assert_eq!(result_status(clock(&[], 100)), 503);
    assert_eq!(extrapolate(10, 1000, 20, 3000, 5), Some((4000, 25)));
    assert_eq!(extrapolate(10, 1000, 10, 3000, 5), None);
    assert_eq!(extrapolate(10, 1000, 20, 1000, 5), None);
    assert_eq!(extrapolate(10, 1000, 20, 3000, u64::MAX), None);
    assert_eq!(extrapolate(0, 1000, 20, 3000, 5), None);
    assert_eq!(extrapolate(10, 1000, 20, 1001, 1), None);
}

#[test]
fn identity_requires_bound_did_and_actual_receipt_evidence() {
    let params = BTreeMap::from([("did".to_owned(), "did:layerx:test".to_owned())]);
    assert_eq!(
        result_status(policy_route("identity", &params, &principal(), &[])),
        503
    );
    let params = BTreeMap::from([("did".to_owned(), "did:layerx:other".to_owned())]);
    assert_eq!(
        result_status(policy_route("identity", &params, &principal(), &[])),
        404
    );
}

#[test]
fn capability_scope_refuses_unlisted_capability() {
    let params = BTreeMap::from([
        ("did".to_owned(), "did:layerx:test".to_owned()),
        ("authority".to_owned(), hex::encode(&[5; 32])),
        ("action_key".to_owned(), hex::encode(&[6; 32])),
        ("capability_id".to_owned(), hex::encode(&[7; 32])),
    ]);
    assert_eq!(
        result_status(policy_route("capability-scope", &params, &principal(), &[])),
        403
    );
}

#[test]
fn budget_state_refuses_missing_revocation_and_checkpoint_with_zero_evidence_summary() {
    assert_eq!(
        result_status(budget(&principal(), &hex::encode(&[8; 32]), &[])),
        404
    );
    let response = budget(&principal(), &hex::encode(&[4; 32]), &[])
        .unwrap_or_else(|_| panic!("bound budget"));
    assert_eq!(response.status, 503);
    let body: Value =
        serde_json::from_slice(&response.body).unwrap_or_else(|e| panic!("JSON: {e}"));
    assert_eq!(body["evidence"]["remaining"], "0");
}

#[test]
fn key_policy_requires_exact_recovery_selector_and_evidence() {
    for (selector, status) in [("false", 503), ("true", 503), ("TRUE", 400)] {
        let params = BTreeMap::from([
            ("did".to_owned(), "did:layerx:test".to_owned()),
            ("recovery".to_owned(), selector.to_owned()),
        ]);
        assert_eq!(
            result_status(policy_route("key-policy", &params, &principal(), &[])),
            status
        );
    }
    assert!(policy_valid(&Policy {
        principals: vec![principal()]
    }));
}

#[test]
fn query_decoding_rejects_duplicates_and_preserves_utf8() {
    assert!(query(Some("tenant=a&tenant=b")).is_err());
    assert!(query(Some("did=%FF")).is_err());
    assert!(query(Some("did=%2")).is_err());
    let result =
        query(Some("did=did%3Alayerx%3A%C3%A9%20x")).unwrap_or_else(|_| panic!("valid query"));
    assert_eq!(result["did"], "did:layerx:é x");
}

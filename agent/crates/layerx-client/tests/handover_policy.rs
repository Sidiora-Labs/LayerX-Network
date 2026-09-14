use layerx_client::handover::decode_finality_policy;
use layerx_paxeer_verifier::EndpointTransport;

fn configuration() -> String {
    format!(
        "version=1\nurl=http://127.0.0.1:18546\ntransport=local-emulator\ntrust_anchor_der=\nchain_id=125\nrequest_timeout_ms=8000\nregistry={}\nguarantor_bond={}\nprotocol_version=3\nnetwork_id=77\ncanonical_genesis_root={}\nconfirmations=2\n",
        "12".repeat(20), "34".repeat(20), "56".repeat(32)
    )
}

#[test]
fn explicit_policy_preserves_all_trust_inputs() -> Result<(), String> {
    let policy = decode_finality_policy(configuration().as_bytes())
        .map_err(|error| format!("valid explicit policy: {error:?}"))?;
    assert_eq!(policy.endpoint.expected_chain_id, 125);
    assert_eq!(policy.endpoint.url, "http://127.0.0.1:18546");
    assert_eq!(policy.endpoint.request_timeout.as_millis(), 8000);
    assert_eq!(policy.endpoint.transport, EndpointTransport::LocalEmulator);
    assert_eq!(policy.registry.bytes(), [0x12; 20]);
    assert_eq!(policy.guarantor_bond.bytes(), [0x34; 20]);
    assert_eq!(policy.protocol_version, 3);
    assert_eq!(policy.network_id, 77);
    assert_eq!(policy.canonical_genesis_root, [0x56; 32]);
    assert_eq!(policy.confirmations, 2);
    Ok(())
}

#[test]
fn refuses_ambiguous_incomplete_and_insecure_policy() {
    let original = configuration();
    for (before, after) in [
        ("version=1", "version=2"),
        ("chain_id=125", "chain_id=0"),
        ("chain_id=125", "chain_id=0125"),
        ("chain_id=125", "chain_id=+125"),
        ("chain_id=125", "chain_id=18446744073709551616"),
        ("network_id=77", "network_id=4294967296"),
        ("request_timeout_ms=8000", "request_timeout_ms=60001"),
        ("confirmations=2", "confirmations=0"),
        ("url=http://127.0.0.1:18546", "url=http://example.com"),
        ("url=http://127.0.0.1:18546", "url=https://127.0.0.1:18546"),
        ("transport=local-emulator", "transport=pinned-tls"),
        ("trust_anchor_der=", "trust_anchor_der=01"),
    ] {
        assert!(
            decode_finality_policy(original.replace(before, after).as_bytes()).is_err(),
            "{before}"
        );
    }
    for suffix in ["network_id=77\n", "unknown=1\n", "\n", "\0"] {
        assert!(decode_finality_policy(format!("{original}{suffix}").as_bytes()).is_err());
    }
    for line in original.lines() {
        assert!(
            decode_finality_policy(original.replace(&format!("{line}\n"), "").as_bytes()).is_err()
        );
    }
    assert!(decode_finality_policy(&vec![b'a'; 1_048_577]).is_err());
    assert!(decode_finality_policy(b"version=1\xff").is_err());
}

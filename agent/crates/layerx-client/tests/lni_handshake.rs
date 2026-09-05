use layerx_client::lni::handshake::{
    decode_node_info, encode_node_info, validate, HandshakeConfig, HandshakeError, NodeInfo,
    NodeRole, SequencerKeyChange,
};
use layerx_client::lni::schema::{Capability, Version};
use layerx_types::error::LayerError;
use layerx_wire::limits::PROTOCOL_VERSION;

fn config(version: Version) -> HandshakeConfig {
    HandshakeConfig {
        built_interface_version: version,
        expected_protocol_version: PROTOCOL_VERSION,
        expected_network_id: 42,
    }
}

fn node(version: Version) -> NodeInfo {
    NodeInfo {
        interface_version: version,
        protocol_version: PROTOCOL_VERSION,
        network_id: 42,
        role: NodeRole::Sequencer,
        chain_head_sequence: 900,
        latest_sealed_batch: 44,
        latest_finalised_checkpoint: [7; 32],
        authorised_sequencer_key: [8; 32],
        advertised_capabilities: vec![
            "account_read".to_owned(),
            "node_info".to_owned(),
            "submit".to_owned(),
        ],
    }
}

#[test]
fn accepts_newer_and_older_minor_versions_but_names_major_mismatch() {
    let newer = validate(
        node(Version { major: 1, minor: 9 }),
        &config(Version { major: 1, minor: 2 }),
        None,
    );
    assert!(newer.is_ok());
    let older = validate(
        node(Version { major: 1, minor: 0 }),
        &config(Version { major: 1, minor: 2 }),
        None,
    );
    assert!(older.is_ok());
    assert_eq!(
        validate(
            node(Version { major: 2, minor: 0 }),
            &config(Version { major: 1, minor: 2 }),
            None,
        ),
        Err(HandshakeError::InterfaceIncompatible {
            built: Version { major: 1, minor: 2 },
            peer: Version { major: 2, minor: 0 },
        })
    );
}

#[test]
fn refuses_wrong_network_and_protocol_as_startup_failures() {
    let mut wrong_network = node(Version::V1_0);
    wrong_network.network_id = 99;
    assert_eq!(
        validate(wrong_network, &config(Version::V1_0), None),
        Err(HandshakeError::Network {
            expected: 42,
            peer: 99,
        })
    );
    let mut wrong_protocol = node(Version::V1_0);
    wrong_protocol.protocol_version = 7;
    assert_eq!(
        validate(wrong_protocol, &config(Version::V1_0), None),
        Err(HandshakeError::ProtocolVersion {
            expected: PROTOCOL_VERSION,
            peer: 7,
        })
    );
}

#[test]
fn reports_missing_and_unknown_capabilities_without_emulation() {
    let mut info = node(Version::V1_0);
    info.advertised_capabilities
        .insert(1, "future_capability".to_owned());
    let accepted = validate(info, &config(Version::V1_0), None)
        .unwrap_or_else(|error| panic!("compatible node refused: {error:?}"));
    assert!(accepted.capabilities().contains(Capability::Submit));
    assert_eq!(
        accepted
            .capabilities()
            .require(Capability::AvailabilityFetch),
        Err(LayerError::UnavailableCapability {
            capability: "availability_fetch".to_owned(),
        })
    );
    assert_eq!(
        accepted.capabilities().unknown_advertised(),
        &["future_capability".to_owned()]
    );
    assert!(accepted
        .capabilities()
        .unavailable()
        .contains(&Capability::HistoricalProofs));
}

#[test]
fn records_sequencer_key_changes_across_reconnects() {
    let first = validate(node(Version::V1_0), &config(Version::V1_0), None)
        .unwrap_or_else(|error| panic!("first handshake failed: {error:?}"));
    let mut changed = node(Version::V1_0);
    changed.authorised_sequencer_key = [9; 32];
    let second = validate(changed, &config(Version::V1_0), Some(&first))
        .unwrap_or_else(|error| panic!("reconnect failed: {error:?}"));
    assert_eq!(
        second.sequencer_key_change(),
        Some(SequencerKeyChange {
            previous: [8; 32],
            current: [9; 32],
        })
    );
}

#[test]
fn node_info_encoding_is_canonical_and_complete() {
    let info = node(Version::V1_0);
    let encoded = encode_node_info(&info)
        .unwrap_or_else(|error| panic!("NodeInfo encoding failed: {error:?}"));
    assert_eq!(decode_node_info(&encoded), Ok(info));

    let mut noncanonical = node(Version::V1_0);
    noncanonical.advertised_capabilities.swap(0, 2);
    assert_eq!(
        encode_node_info(&noncanonical),
        Err(HandshakeError::MalformedNodeInfo)
    );
}

#[test]
fn explicit_state_commitment_handshake_keeps_exact_version_pin() {
    let mut peer = node(Version::V1_3);
    peer.protocol_version = layerx_wire::limits::STATE_COMMITMENT_PROTOCOL_VERSION;
    let mut expected = config(Version::V1_3);
    expected.expected_protocol_version = peer.protocol_version;
    let bytes = encode_node_info(&peer).expect("encode explicit version three");
    let decoded = decode_node_info(&bytes).expect("decode explicit version three");
    assert!(validate(decoded.clone(), &expected, None).is_ok());
    assert_eq!(
        validate(decoded, &config(Version::V1_3), None),
        Err(HandshakeError::ProtocolVersion {
            expected: 2,
            peer: 3
        })
    );
}

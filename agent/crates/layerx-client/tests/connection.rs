use std::io::ErrorKind;
use std::time::Duration;

use layerx_client::client::{ConnectionError, ConnectionState, ReconnectPolicy};
use layerx_client::head::{HeadError, HeadTracker};
use layerx_client::lni::handshake::{
    validate, HandshakeConfig, HandshakeError, NodeInfo, NodeRole,
};
use layerx_client::lni::schema::Version;
use layerx_client::lni::transport::TransportError;
use layerx_wire::limits::PROTOCOL_VERSION;

fn node(sequence: u64, batch: u64, key: u8) -> NodeInfo {
    NodeInfo {
        interface_version: Version::V1_0,
        protocol_version: PROTOCOL_VERSION,
        network_id: 77,
        role: NodeRole::Sequencer,
        chain_head_sequence: sequence,
        latest_sealed_batch: batch,
        latest_finalised_checkpoint: [5; 32],
        authorised_sequencer_key: [key; 32],
        advertised_capabilities: vec!["node_info".to_owned()],
    }
}

fn config() -> HandshakeConfig {
    HandshakeConfig {
        built_interface_version: Version::V1_0,
        expected_protocol_version: PROTOCOL_VERSION,
        expected_network_id: 77,
    }
}

#[test]
fn reconnect_during_request_preserves_explicit_state_and_bounded_backoff() {
    let policy = ReconnectPolicy {
        maximum_attempts: 4,
        base_delay: Duration::from_millis(10),
        maximum_delay: Duration::from_millis(80),
        jitter_percent: 20,
    };
    let delays: Vec<_> = (0..policy.maximum_attempts)
        .map(|attempt| policy.delay(attempt, "/run/layerx/lni.sock"))
        .collect();
    assert!(delays.iter().all(|delay| *delay <= policy.maximum_delay));
    assert!(delays.windows(2).all(|pair| pair[0] <= pair[1]));
    assert_eq!(
        ConnectionError::AttemptsExhausted.state(),
        ConnectionState::Unreachable
    );
}

#[test]
fn rejects_head_regression_and_tracks_key_ranges() {
    let mut tracker = HeadTracker::new(&node(10, 4, 1));
    assert_eq!(
        tracker.update(&node(9, 4, 1)),
        Err(HeadError::SequenceRegression {
            current: 10,
            peer: 9,
        })
    );
    assert_eq!(
        tracker
            .update(&node(11, 5, 2))
            .map(|head| head.sealed_batch),
        Ok(5)
    );
    assert_eq!(tracker.require_sequencer_key(4, [1; 32]), Ok(()));
    assert_eq!(tracker.require_sequencer_key(5, [2; 32]), Ok(()));
    assert_eq!(
        tracker.require_sequencer_key(5, [1; 32]),
        Err(HeadError::UnadvertisedSequencerKey {
            batch: 5,
            key: [1; 32],
        })
    );
}

#[test]
fn unexpected_network_mid_session_is_incompatible() {
    let first = validate(node(10, 4, 1), &config(), None)
        .unwrap_or_else(|error| panic!("initial handshake failed: {error:?}"));
    let mut changed = node(11, 5, 1);
    changed.network_id = 99;
    assert_eq!(
        validate(changed, &config(), Some(&first)),
        Err(HandshakeError::Network {
            expected: 77,
            peer: 99,
        })
    );
    assert_eq!(
        ConnectionError::Handshake(HandshakeError::Network {
            expected: 77,
            peer: 99,
        })
        .state(),
        ConnectionState::Incompatible
    );
}

#[test]
fn every_reconnect_failure_maps_to_a_non_connected_state() {
    let failures = [
        ConnectionError::Transport(TransportError::ConnectionFailure(
            ErrorKind::ConnectionRefused,
        )),
        ConnectionError::Handshake(HandshakeError::Network {
            expected: 77,
            peer: 99,
        }),
        ConnectionError::Head(HeadError::SequenceRegression {
            current: 10,
            peer: 9,
        }),
        ConnectionError::AttemptsExhausted,
    ];
    assert!(failures.into_iter().all(|error| matches!(
        error.state(),
        ConnectionState::Incompatible | ConnectionState::Unreachable
    )));
}

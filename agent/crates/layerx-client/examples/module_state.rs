use std::error::Error;
use std::path::PathBuf;
use std::time::Duration;

use layerx_client::client::{Client, ClientConfig, ReconnectPolicy};
use layerx_client::evidence::MINIMUM_FINALITY_FRAME_BYTES;
use layerx_client::lni::handshake::HandshakeConfig;
use layerx_client::lni::schema::Version;
use layerx_client::lni::transport::Limits;
use layerx_proof::inclusion::SequencerAuthorization;
use layerx_types::json::decode_hex;
use layerx_types::verify::VerificationLevel;

fn config(endpoint: PathBuf, network: u32) -> ClientConfig {
    ClientConfig {
        endpoint,
        handshake: HandshakeConfig {
            built_interface_version: Version::V1_5,
            expected_protocol_version: 3,
            expected_network_id: network,
        },
        limits: Limits {
            maximum_frame_bytes: MINIMUM_FINALITY_FRAME_BYTES,
            maximum_connections: 1,
            maximum_streams: 1,
            maximum_queued_bytes: MINIMUM_FINALITY_FRAME_BYTES,
            deadline: Duration::from_secs(8),
        },
        reconnect: ReconnectPolicy {
            maximum_attempts: 1,
            base_delay: Duration::from_millis(1),
            maximum_delay: Duration::from_millis(1),
            jitter_percent: 0,
        },
    }
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(2 + bytes.len() * 2);
    text.push_str("0x");
    for byte in bytes {
        text.push(char::from(DIGITS[usize::from(byte >> 4)]));
        text.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    text
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 7 {
        return Err("usage: module_state SOCKET NETWORK SEQUENCER_HEX MODULE KEY_HEX LEVEL".into());
    }
    let mut client = Client::connect(config(PathBuf::from(&args[1]), args[2].parse()?))
        .map_err(|error| format!("{error:?}"))?;
    let pinned: [u8; 32] = decode_hex(&args[3])?
        .try_into()
        .map_err(|_| "sequencer width")?;
    if client.handshake().node().authorised_sequencer_key != pinned {
        return Err("sequencer binding mismatch".into());
    }
    let requested = match args[6].as_str() {
        "3" => VerificationLevel::STATE_PROVEN,
        "4" => VerificationLevel::CHECKPOINT_FINALISED,
        _ => return Err("expected verification level 3 or 4".into()),
    };
    let header = client
        .batch_header(client.head().sealed_batch, 1)
        .map_err(|error| format!("{error:?}"))?;
    let authorization = SequencerAuthorization::new(
        header.sequencer_id,
        pinned,
        header.first_batch_number,
        header.last_batch_number,
    );
    let value = client
        .module_state(
            args[4].parse()?,
            &decode_hex(&args[5])?,
            requested,
            2,
            authorization,
        )
        .map_err(|error| format!("{error:?}"))?;
    println!(
        "{{\"value\":\"{}\",\"proof\":\"{}\",\"level\":{},\"sequence\":{},\"batch\":{}}}",
        hex(value.canonical_bytes()),
        hex(value.proof_material()),
        value.achieved().wire_rank(),
        value.freshness().global_sequence,
        value.freshness().batch_number
    );
    Ok(())
}

use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::thread;
use std::time::Duration;

use layerx_client::lni::handshake::{perform, HandshakeConfig};
use layerx_client::lni::preparation::{preparation_state, PreparationStateContext};
use layerx_client::lni::schema::{decode_envelope, encode_envelope, Envelope, Version};
use layerx_client::lni::transport::FrameTransport;
use layerx_types::ids::Did;
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_wire::activity::decode_signed;
use layerx_wire::hash::activity_id;
use layerx_wire::receipt::decode as decode_receipt;

use crate::cases::connect;

fn registry() -> Result<ModuleRegistry, String> {
    let activity = ActivityType::new(ModuleId::Asset, 1)
        .map_err(|error| format!("activity type failed: {error:?}"))?;
    let registration = ModuleRegistration::new(ModuleId::Asset, &[activity])
        .map_err(|error| format!("module registration failed: {error:?}"))?;
    ModuleRegistry::new(&[registration])
        .map_err(|error| format!("module registry failed: {error:?}"))
}

fn exchange(
    transport: &mut dyn FrameTransport,
    tag: u16,
    correlation_id: u64,
    payload: &[u8],
) -> Result<(u16, Vec<u8>, Vec<u8>), String> {
    let request = encode_envelope(Envelope {
        version: Version::V1_1,
        message_tag: tag,
        correlation_id,
        canonical_payload: payload,
        proof_material: &[],
    })
    .map_err(|error| format!("production request encoding failed: {error:?}"))?;
    transport
        .send(&request)
        .map_err(|error| format!("production request send failed: {error:?}"))?;
    let response = transport
        .receive()
        .map_err(|error| format!("production response receive failed: {error:?}"))?;
    let response = decode_envelope(&response)
        .map_err(|error| format!("production response malformed: {error:?}"))?;
    if response.version.major != Version::V1_1.major || response.correlation_id != correlation_id {
        return Err("production response changed version or correlation".to_owned());
    }
    Ok((
        response.message_tag,
        response.canonical_payload.to_vec(),
        response.proof_material.to_vec(),
    ))
}

pub fn run_if_configured() -> Result<Option<String>, String> {
    let Some(socket) = std::env::var_os("LAYERX_QUALIFY_LNI_SOCKET") else {
        return Ok(None);
    };
    let activity_path = std::env::var_os("LAYERX_QUALIFY_SIGNED_ACTIVITY")
        .ok_or_else(|| "LAYERX_QUALIFY_SIGNED_ACTIVITY is required".to_owned())?;
    let activity_bytes = fs::read(&activity_path)
        .map_err(|error| format!("could not read signed activity: {error}"))?;
    let socket_metadata = fs::symlink_metadata(&socket)
        .map_err(|error| format!("could not inspect production LNI socket: {error}"))?;
    let daemon_uid: u32 = std::env::var("LAYERX_QUALIFY_LNI_DAEMON_UID")
        .map_err(|_| "LAYERX_QUALIFY_LNI_DAEMON_UID is required".to_owned())?
        .parse()
        .map_err(|_| "invalid production LNI daemon uid".to_owned())?;
    let client_gid: u32 = std::env::var("LAYERX_QUALIFY_LNI_CLIENT_GID")
        .map_err(|_| "LAYERX_QUALIFY_LNI_CLIENT_GID is required".to_owned())?
        .parse()
        .map_err(|_| "invalid production LNI client gid".to_owned())?;
    let parent = Path::new(&socket)
        .parent()
        .ok_or_else(|| "production LNI socket has no parent".to_owned())?;
    let parent_metadata = fs::symlink_metadata(parent)
        .map_err(|error| format!("could not inspect production LNI parent: {error}"))?;
    if socket_metadata.mode() & 0o777 != 0o660
        || socket_metadata.uid() != daemon_uid
        || socket_metadata.gid() != client_gid
        || parent_metadata.mode() & 0o777 != 0o750
        || parent_metadata.uid() != daemon_uid
        || parent_metadata.gid() != client_gid
    {
        return Err("production LNI parent/socket ownership or mode is not pinned".to_owned());
    }
    let decoded = decode_signed(&activity_bytes, &registry()?)
        .map_err(|error| format!("qualification activity is not canonical: {error:?}"))?;
    let actor = Did::new(decoded.actor_did())
        .map_err(|error| format!("qualification actor DID is not canonical: {error:?}"))?;
    let expected_activity_id = activity_id(&decoded)
        .map_err(|error| format!("qualification activity id failed: {error:?}"))?;
    let mut transport = connect(Path::new(&socket))?;
    let expected = HandshakeConfig {
        built_interface_version: Version::V1_1,
        expected_protocol_version: std::env::var("LAYERX_QUALIFY_PROTOCOL_VERSION")
            .map_err(|_| "LAYERX_QUALIFY_PROTOCOL_VERSION is required".to_owned())?
            .parse()
            .map_err(|_| "invalid qualification protocol version".to_owned())?,
        expected_network_id: std::env::var("LAYERX_QUALIFY_NETWORK_ID")
            .map_err(|_| "LAYERX_QUALIFY_NETWORK_ID is required".to_owned())?
            .parse()
            .map_err(|_| "invalid qualification network id".to_owned())?,
    };
    let handshake = perform(&mut transport, &expected, None)
        .map_err(|error| format!("production handshake refused: {error:?}"))?;
    handshake
        .capabilities()
        .require(layerx_client::lni::schema::Capability::Submit)
        .map_err(|error| format!("production submit unavailable: {error:?}"))?;
    handshake
        .capabilities()
        .require(layerx_client::lni::schema::Capability::ReceiptLookup)
        .map_err(|error| format!("production receipt lookup unavailable: {error:?}"))?;
    handshake
        .capabilities()
        .require(layerx_client::lni::schema::Capability::PreparationState)
        .map_err(|error| format!("production preparation state unavailable: {error:?}"))?;
    let preparation = preparation_state(
        &mut transport,
        &actor,
        PreparationStateContext {
            interface_version: handshake.node().interface_version,
            expected_network_id: expected.expected_network_id,
            minimum_observed_head: handshake.node().chain_head_sequence,
            correlation_id: 38_199,
        },
    )
    .map_err(|error| format!("production preparation snapshot refused: {error:?}"))?;
    if preparation.account_sequence != decoded.account_sequence()
        || preparation.network_id != decoded.network_id()
        || preparation.protocol_timestamp < decoded.timestamp_bound().not_before
        || preparation.protocol_timestamp > decoded.timestamp_bound().not_after
        || !preparation
            .module_registry
            .declares(decoded.activity_type())
    {
        return Err(
            "production preparation snapshot did not bind the submitted activity".to_owned(),
        );
    }

    let (tag, retained, evidence) = exchange(&mut transport, 3, 38_200, &activity_bytes)?;
    if tag != 4 || retained != activity_bytes || evidence.as_slice() != expected_activity_id {
        return Err("production submit did not retain the exact accepted bytes".to_owned());
    }
    let mut selector = Vec::with_capacity(33);
    selector.push(1);
    selector.extend_from_slice(&expected_activity_id);
    let mut receipt = Vec::new();
    for attempt in 0_u64..100 {
        let (tag, candidate, proof) = exchange(&mut transport, 5, 38_201 + attempt, &selector)?;
        if tag != 6 || !proof.is_empty() {
            return Err("production receipt lookup response shape changed".to_owned());
        }
        if !candidate.is_empty() {
            receipt = candidate;
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    if receipt.is_empty() {
        return Err("production receipt did not become durable within the lookup bound".to_owned());
    }
    let decoded_receipt = decode_receipt(&receipt)
        .map_err(|error| format!("production receipt is not canonical: {error:?}"))?;
    let decoded_receipt = decoded_receipt
        .protocol()
        .ok_or_else(|| "production node returned a replay-only receipt".to_owned())?;
    let mut idempotency_selector = Vec::with_capacity(33);
    idempotency_selector.push(2);
    idempotency_selector.extend_from_slice(&decoded.idempotency_key());
    let (tag, by_idempotency, proof) = exchange(&mut transport, 5, 38_400, &idempotency_selector)?;
    if tag != 6 || by_idempotency != receipt || !proof.is_empty() {
        return Err("idempotency lookup did not return byte-identical receipt".to_owned());
    }
    let mut sequence_selector = Vec::with_capacity(9);
    sequence_selector.push(3);
    sequence_selector.extend_from_slice(&decoded_receipt.global_sequence().to_be_bytes());
    let (tag, by_sequence, proof) = exchange(&mut transport, 5, 38_401, &sequence_selector)?;
    if tag != 6 || by_sequence != receipt || !proof.is_empty() {
        return Err("sequence lookup did not return byte-identical receipt".to_owned());
    }
    let mut absent_selector = vec![1];
    absent_selector.extend_from_slice(&[0xff; 32]);
    let (tag, absent, proof) = exchange(&mut transport, 5, 38_402, &absent_selector)?;
    if tag != 6 || !absent.is_empty() || !proof.is_empty() {
        return Err("production absent receipt was not canonical empty evidence".to_owned());
    }
    let maximum_activity_path = std::env::var_os("LAYERX_QUALIFY_MAX_SIGNED_ACTIVITY")
        .ok_or_else(|| "LAYERX_QUALIFY_MAX_SIGNED_ACTIVITY is required".to_owned())?;
    let maximum_activity = fs::read(maximum_activity_path)
        .map_err(|error| format!("could not read maximum signed activity: {error}"))?;
    if maximum_activity.len() != 1_048_576 {
        return Err("maximum signed activity is not the canonical 1,048,576 bytes".to_owned());
    }
    let maximum_decoded = decode_signed(&maximum_activity, &registry()?)
        .map_err(|error| format!("maximum activity is not canonical: {error:?}"))?;
    let maximum_id = activity_id(&maximum_decoded)
        .map_err(|error| format!("maximum activity id failed: {error:?}"))?;
    let (tag, retained, evidence) = exchange(&mut transport, 3, 38_500, &maximum_activity)?;
    if tag != 4 || retained != maximum_activity || evidence.as_slice() != maximum_id {
        return Err("maximum canonical activity was not retained byte-exactly".to_owned());
    }
    drop(transport);

    let deadline_ms: u64 = std::env::var("LAYERX_QUALIFY_LNI_DEADLINE_MS")
        .map_err(|_| "LAYERX_QUALIFY_LNI_DEADLINE_MS is required".to_owned())?
        .parse()
        .map_err(|_| "invalid production LNI deadline".to_owned())?;
    let mut drip = UnixStream::connect(Path::new(&socket))
        .map_err(|error| format!("could not open deadline probe: {error}"))?;
    drip.write_all(&[0])
        .map_err(|error| format!("could not write deadline probe: {error}"))?;
    thread::sleep(Duration::from_millis(deadline_ms.saturating_add(100)));
    drip.set_read_timeout(Some(Duration::from_millis(100)))
        .map_err(|error| format!("could not bound deadline probe read: {error}"))?;
    let mut byte = [0_u8; 1];
    if matches!(drip.read(&mut byte), Ok(1)) {
        return Err("drip connection survived the absolute frame deadline".to_owned());
    }
    drop(drip);
    let mut after_drip = connect(Path::new(&socket))?;
    perform(&mut after_drip, &expected, None)
        .map_err(|error| format!("LNI did not remain live after deadline: {error:?}"))?;
    Ok(Some(format!(
        "production LNI boundary exercised at {} for network {}",
        Path::new(&socket).display(),
        expected.expected_network_id
    )))
}

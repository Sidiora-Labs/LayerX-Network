use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use layerx_client::lni::handshake::{perform, HandshakeConfig};
use layerx_client::lni::refusal::decode_core_refusal;
use layerx_client::lni::schema::{decode_envelope, encode_envelope, Envelope, Version};
use layerx_client::lni::transport::{ConnectionGate, FrameTransport, Limits, Uds};
use layerx_client::submit::{submit_signed, SubmissionContext};
use layerx_programs::DeploymentProof;
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};

const FRAME_BYTES: usize = 1_212_416;

/// # Errors
/// Refuses malformed activities, native admission refusals, unavailable evidence,
/// and evidence that does not contain the exact submitted signed activity.
pub fn deploy(
    socket: &Path,
    canonical: &[u8],
    deadline: Instant,
) -> Result<DeploymentProof, String> {
    let types = [1, 2].map(|ordinal| ActivityType::new(ModuleId::Programs, ordinal));
    let types = types
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("deployment types: {error:?}"))?;
    let registration = ModuleRegistration::new(ModuleId::Programs, &types)
        .map_err(|error| format!("deployment registration: {error:?}"))?;
    let registry = ModuleRegistry::new(&[registration])
        .map_err(|error| format!("deployment registry: {error:?}"))?;
    let activity = layerx_wire::activity::decode_signed(canonical, &registry)
        .map_err(|error| format!("deployment activity: {error:?}"))?;
    let signer = activity
        .authority()
        .try_into()
        .map_err(|_| "deployment requires an owner signing authority".to_owned())?;
    let id = layerx_wire::hash::activity_id(&activity)
        .map_err(|error| format!("deployment identity: {error:?}"))?;
    let config = HandshakeConfig {
        built_interface_version: Version::V1_4,
        expected_protocol_version: activity.protocol_version(),
        expected_network_id: activity.network_id(),
    };
    let (mut transport, version) = connect(socket, &config, deadline)?;
    submit_signed(
        &mut transport,
        &registry,
        SubmissionContext {
            interface_version: version,
            protocol_version: activity.protocol_version(),
            network_id: activity.network_id(),
            correlation_id: 1,
            signer_public_key: signer,
            attempt: 1,
        },
        canonical,
    )
    .map_err(|error| format!("deployment admission refused: {error:?}"))?;
    drop(transport);
    loop {
        let (mut transport, version) = connect(socket, &config, deadline)?;
        let mut payload = vec![0, 1, 4];
        payload.extend_from_slice(&id);
        let request = encode_envelope(Envelope {
            version,
            message_tag: 16,
            correlation_id: 2,
            canonical_payload: &payload,
            proof_material: &[],
        })
        .map_err(|error| format!("deployment request: {error:?}"))?;
        transport
            .send(&request)
            .map_err(|error| format!("deployment send: {error:?}"))?;
        let bytes = transport
            .receive()
            .map_err(|error| format!("deployment receive: {error:?}"))?;
        let response =
            decode_envelope(&bytes).map_err(|error| format!("deployment response: {error:?}"))?;
        if response.version != version
            || response.correlation_id != 2
            || !response.proof_material.is_empty()
        {
            return Err("deployment response binding mismatch".to_owned());
        }
        if response.message_tag == 17 {
            let proof = DeploymentProof::decode(response.canonical_payload)
                .map_err(|error| format!("deployment proof: {error}"))?;
            if proof.activity != canonical {
                return Err("deployment proof names different activity bytes".to_owned());
            }
            return Ok(proof);
        }
        if response.message_tag != 25 {
            return Err("unexpected deployment response tag".to_owned());
        }
        let refusal = decode_core_refusal(response.canonical_payload)
            .ok_or_else(|| "malformed deployment refusal".to_owned())?;
        if refusal.result.raw() != -106 {
            return Err(format!(
                "deployment evidence refused: {}",
                refusal.result.raw()
            ));
        }
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| "deployment outcome unavailable at request deadline".to_owned())?;
        thread::sleep(remaining.min(Duration::from_millis(20)));
    }
}

fn connect(
    socket: &Path,
    config: &HandshakeConfig,
    deadline: Instant,
) -> Result<(Uds, Version), String> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .filter(|value| !value.is_zero())
        .ok_or_else(|| "deployment outcome unavailable at request deadline".to_owned())?;
    let mut transport = Uds::connect(
        socket,
        &ConnectionGate::new(1),
        Limits {
            maximum_frame_bytes: FRAME_BYTES,
            maximum_connections: 1,
            maximum_streams: 1,
            maximum_queued_bytes: FRAME_BYTES,
            deadline: remaining,
        },
    )
    .map_err(|error| format!("deployment LNI unavailable: {error:?}"))?;
    let handshake = perform(&mut transport, config, None)
        .map_err(|error| format!("deployment handshake refused: {error:?}"))?;
    Ok((transport, handshake.node().interface_version))
}

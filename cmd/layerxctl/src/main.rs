use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use layerx_client::client::{Client, ClientConfig, ReconnectPolicy};
use layerx_client::lni::handshake::HandshakeConfig;
use layerx_client::lni::schema::Version;
use layerx_client::lni::transport::Limits;
use layerx_client::submit::Submission;
use layerx_types::ids::Did;
use layerx_wire::limits::{MAX_MESSAGE_BYTES, PROTOCOL_VERSION};

const FRAME_BYTES: usize = 1_212_416;
const USAGE: &str = "usage: layerxctl read-state --socket PATH --network-id N --actor DID [--protocol-version N]\n       layerxctl submit --socket PATH --network-id N --actor DID --public-key HEX64 --activity FILE [--protocol-version N]";

fn options(arguments: &[String], submit: bool) -> Result<BTreeMap<String, String>, String> {
    let mut parsed = BTreeMap::new();
    for pair in arguments.chunks(2) {
        let [key, value] = pair else {
            return Err("option requires a value".to_owned());
        };
        let common = matches!(
            key.as_str(),
            "--socket" | "--network-id" | "--actor" | "--protocol-version"
        );
        if !(common || submit && matches!(key.as_str(), "--public-key" | "--activity")) {
            return Err(format!("unknown option {key}"));
        }
        if value.is_empty() || parsed.insert(key.clone(), value.clone()).is_some() {
            return Err(format!("empty or duplicate option {key}"));
        }
    }
    Ok(parsed)
}

fn required<'a>(options: &'a BTreeMap<String, String>, key: &str) -> Result<&'a str, String> {
    options
        .get(key)
        .map(String::as_str)
        .ok_or_else(|| format!("{key} is required"))
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn public_key(text: &str) -> Result<[u8; 32], String> {
    if text.len() != 64 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("--public-key requires 64 hex characters".to_owned());
    }
    let mut key = [0; 32];
    for (index, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16)
            .map_err(|error| error.to_string())?;
    }
    Ok(key)
}

fn activity_file(path: &str) -> Result<Vec<u8>, String> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
        .open(path)
        .map_err(|error| format!("activity file: {error}"))?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_MESSAGE_BYTES as u64 {
        return Err(
            "activity must be a nonempty regular file within the canonical message bound"
                .to_owned(),
        );
    }
    let mut bytes = Vec::new();
    file.take(MAX_MESSAGE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.is_empty() || bytes.len() > MAX_MESSAGE_BYTES {
        return Err("activity changed outside the canonical message bound".to_owned());
    }
    Ok(bytes)
}

fn run(arguments: &[String]) -> Result<(String, ExitCode), String> {
    let Some(command) = arguments.first() else {
        return Err(USAGE.to_owned());
    };
    if command == "--help" && arguments.len() == 1 {
        return Ok((USAGE.to_owned(), ExitCode::SUCCESS));
    }
    let submit = match command.as_str() {
        "read-state" => false,
        "submit" => true,
        _ => return Err(USAGE.to_owned()),
    };
    let parsed = options(&arguments[1..], submit)?;
    let network_id = required(&parsed, "--network-id")?
        .parse::<u32>()
        .map_err(|error| format!("network id: {error}"))?;
    if network_id == 0 {
        return Err("network id must be nonzero".to_owned());
    }
    let protocol_version = parsed
        .get("--protocol-version")
        .map_or(Ok(PROTOCOL_VERSION), |value| value.parse::<u16>())
        .map_err(|error| format!("protocol version: {error}"))?;
    if !layerx_wire::limits::protocol_version_supported(protocol_version) {
        return Err("unsupported protocol version".to_owned());
    }
    let actor = Did::new(required(&parsed, "--actor")?.as_bytes())
        .map_err(|error| format!("actor: {error:?}"))?;
    let signed = if submit {
        Some((
            public_key(required(&parsed, "--public-key")?)?,
            activity_file(required(&parsed, "--activity")?)?,
        ))
    } else {
        None
    };
    let mut client = Client::connect(ClientConfig {
        endpoint: PathBuf::from(required(&parsed, "--socket")?),
        handshake: HandshakeConfig {
            built_interface_version: Version::V1_3,
            expected_protocol_version: protocol_version,
            expected_network_id: network_id,
        },
        limits: Limits {
            maximum_frame_bytes: FRAME_BYTES,
            maximum_connections: 1,
            maximum_streams: 4,
            maximum_queued_bytes: FRAME_BYTES * 4,
            deadline: Duration::from_secs(10),
        },
        reconnect: ReconnectPolicy {
            maximum_attempts: 1,
            base_delay: Duration::from_millis(50),
            maximum_delay: Duration::from_millis(50),
            jitter_percent: 0,
        },
    })
    .map_err(|error| format!("LNI connection: {error:?}"))?;
    let state = client
        .preparation_state(&actor, 1)
        .map_err(|error| format!("state read: {error:?}"))?;
    let Some((key, bytes)) = signed else {
        return Ok((format!("{{\"network_id\":{network_id},\"protocol_version\":{protocol_version},\"global_sequence\":{},\"account_sequence\":{},\"state_root\":\"{}\",\"kernel_epoch\":{},\"evidence\":\"authenticated_node_snapshot\"}}",
            state.observed_head_sequence, state.account_sequence, hex(&state.observed_state_root), state.kernel_epoch), ExitCode::SUCCESS));
    };
    let activity = layerx_wire::activity::decode_signed(&bytes, &state.module_registry)
        .map_err(|error| format!("canonical activity: {error:?}"))?;
    if activity.actor_did() != actor.as_bytes() {
        return Err("signed activity actor differs from --actor".to_owned());
    }
    match client
        .submit_signed(&state.module_registry, key, 2, 1, &bytes)
        .map_err(|error| format!("submission refused: {error:?}"))?
    {
        Submission::Acknowledged(ack) => Ok((
            format!(
                "{{\"state\":\"acknowledged\",\"activity_id\":\"{}\",\"idempotency_key\":\"{}\"}}",
                hex(&ack.activity_id()),
                hex(&ack.idempotency_key())
            ),
            ExitCode::SUCCESS,
        )),
        Submission::Unknown(unknown) => Ok((
            format!(
                "{{\"state\":\"unknown\",\"activity_id\":\"{}\",\"idempotency_key\":\"{}\"}}",
                hex(&unknown.activity_id()),
                hex(&unknown.idempotency_key())
            ),
            ExitCode::from(3),
        )),
    }
}

fn main() -> ExitCode {
    match run(&std::env::args().skip(1).collect::<Vec<_>>()) {
        Ok((output, status)) => {
            println!("{output}");
            status
        }
        Err(error) => {
            eprintln!("layerxctl: {error}");
            ExitCode::FAILURE
        }
    }
}

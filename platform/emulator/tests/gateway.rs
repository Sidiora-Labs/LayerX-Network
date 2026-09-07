//! In-process integration coverage for the local emulator gateway.
//!
//! These tests boot the real `layerx_platform_emulator::run` listener, which
//! links the production `LayerX` core transition and receipt machinery through
//! the C bridge, and drive it over its HTTP surface exactly as an SDK or the
//! middleware would. They assert the production gateway surface
//! (`/v1/activities`, `/v1/state`, `/v1/receipts/<id>`) and the emulator-only
//! control hooks (`/__emulator/*`) that live clearly outside the deterministic
//! transition path.

use std::fmt::Write as _;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

const EMULATOR_SEED: [u8; 32] = [0x42; 32];

struct Reply {
    status: u16,
    content_type: String,
    body: Vec<u8>,
}

impl Reply {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

fn free_port() -> Result<u16, String> {
    let probe = TcpListener::bind("127.0.0.1:0").map_err(|error| error.to_string())?;
    let port = probe
        .local_addr()
        .map_err(|error| error.to_string())?
        .port();
    drop(probe);
    Ok(port)
}

fn request(
    address: &str,
    method: &str,
    path: &str,
    content_type: &str,
    body: &[u8],
) -> Result<Reply, String> {
    request_with_idempotency(address, method, path, content_type, body, None)
}

fn request_with_idempotency(
    address: &str,
    method: &str,
    path: &str,
    content_type: &str,
    body: &[u8],
    idempotency_key: Option<&str>,
) -> Result<Reply, String> {
    let mut stream = TcpStream::connect(address).map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|error| error.to_string())?;
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    if !content_type.is_empty() {
        write!(head, "Content-Type: {content_type}\r\n").map_err(|error| error.to_string())?;
    }
    if let Some(value) = idempotency_key {
        write!(head, "Idempotency-Key: {value}\r\n").map_err(|error| error.to_string())?;
    }
    head.push_str("\r\n");
    stream
        .write_all(head.as_bytes())
        .map_err(|error| error.to_string())?;
    stream.write_all(body).map_err(|error| error.to_string())?;
    stream.flush().map_err(|error| error.to_string())?;
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .map_err(|error| error.to_string())?;
    parse_reply(&raw)
}

fn parse_reply(raw: &[u8]) -> Result<Reply, String> {
    let split = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| "response is missing a header terminator".to_string())?;
    let header_text =
        std::str::from_utf8(&raw[..split]).map_err(|_| "response headers are not UTF-8")?;
    let mut lines = header_text.split("\r\n");
    let status_line = lines.next().ok_or("response is missing a status line")?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .ok_or("status line is missing a code")?
        .parse::<u16>()
        .map_err(|_| "status code is not numeric".to_string())?;
    let mut content_type = String::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-type") {
                content_type = value.trim().to_string();
            }
        }
    }
    Ok(Reply {
        status,
        content_type,
        body: raw[split + 4..].to_vec(),
    })
}

/// Boots the emulator on a private port and returns its loopback address once
/// the real core reports readiness on `/healthz`.
fn boot() -> Result<String, String> {
    boot_protocol(2)
}

fn boot_protocol(protocol_version: u16) -> Result<String, String> {
    let port = free_port()?;
    let address = format!("127.0.0.1:{port}");
    let listen = address.clone();
    let seed_path = PathBuf::from(format!(
        "/tmp/layerx-emulator-gateway-seed-{}-{port}",
        std::process::id()
    ));
    std::fs::write(&seed_path, EMULATOR_SEED).map_err(|error| error.to_string())?;
    let seed_argument = seed_path.to_string_lossy().into_owned();
    thread::spawn(move || {
        let _ = layerx_platform_emulator::run(vec![
            "up".to_string(),
            "--listen".to_string(),
            listen,
            "--sequencer-seed-file".to_string(),
            seed_argument,
            "--protocol-version".to_string(),
            protocol_version.to_string(),
        ]);
    });
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Ok(reply) = request(&address, "GET", "/healthz", "", &[]) {
            if reply.status == 200 {
                let _ = std::fs::remove_file(&seed_path);
                return Ok(address);
            }
        }
        if Instant::now() >= deadline {
            return Err("emulator did not become ready".to_string());
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn response_result(reply: &Reply) -> Result<serde_json::Value, String> {
    serde_json::from_slice::<serde_json::Value>(&reply.body)
        .map_err(|error| error.to_string())?
        .get("result")
        .cloned()
        .ok_or_else(|| format!("response omitted result: {}", reply.text()))
}

fn post_json(address: &str, path: &str, body: &str) -> Result<Reply, String> {
    request(address, "POST", path, "application/json", body.as_bytes())
}

fn error_code(reply: &Reply) -> Option<String> {
    let text = reply.text();
    let marker = "\"code\":\"";
    let start = text.find(marker)? + marker.len();
    let rest = &text[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn hex_encode(bytes: &[u8]) -> Result<String, String> {
    let mut encoded = String::new();
    for byte in bytes {
        write!(encoded, "{byte:02x}").map_err(|error| error.to_string())?;
    }
    Ok(encoded)
}

fn hex_decode(value: &str) -> Result<Vec<u8>, String> {
    if !value.len().is_multiple_of(2) {
        return Err("hex value has odd length".to_string());
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).map_err(|_| "hex value is not UTF-8")?;
            u8::from_str_radix(text, 16).map_err(|_| "hex value has a non-hex byte".to_string())
        })
        .collect()
}

#[test]
fn healthz_reports_the_layerx_core() -> Result<(), String> {
    let address = boot()?;
    let reply = request(&address, "GET", "/healthz", "", &[])?;
    assert_eq!(reply.status, 200);
    let text = reply.text();
    assert!(
        text.contains("\"status\":\"ready\""),
        "unexpected body: {text}"
    );
    assert!(
        text.contains("\"core\":\"layerx\""),
        "unexpected body: {text}"
    );
    Ok(())
}

#[test]
fn state_advertises_the_emulator_with_instant_batching() -> Result<(), String> {
    let address = boot()?;
    let reply = request(&address, "GET", "/v1/state", "", &[])?;
    assert_eq!(reply.status, 200);
    let text = reply.text();
    assert!(
        text.contains("\"network_mode\":\"emulator\""),
        "unexpected body: {text}"
    );
    assert!(
        text.contains("\"batch_cadence\":\"instant\""),
        "unexpected body: {text}"
    );
    assert!(text.contains("\"accounts\":[]"), "unexpected body: {text}");
    Ok(())
}

#[test]
fn prefunded_accounts_appear_in_state() -> Result<(), String> {
    let address = boot()?;
    let public_key = "11".repeat(32);
    let body = format!("{{\"did\":\"did:layerx:agent-alpha\",\"public_key\":\"{public_key}\",\"amount_lo\":123456}}");
    let reply = post_json(&address, "/__emulator/accounts/prefund", &body)?;
    assert_eq!(reply.status, 200, "prefund failed: {}", reply.text());
    assert!(reply.text().contains("\"prefunded\":true"));

    let state = request(&address, "GET", "/v1/state", "", &[])?;
    assert_eq!(state.status, 200);
    let text = state.text();
    assert!(
        text.contains("agent:did:layerx:agent-alpha:main"),
        "prefunded account missing from state: {text}"
    );
    assert!(
        text.contains("\"balance_lo\":123456"),
        "prefunded balance missing from state: {text}"
    );
    Ok(())
}

#[test]
fn invalid_activity_is_refused_by_the_real_transition() -> Result<(), String> {
    let address = boot()?;
    let reply = post_json(&address, "/v1/activities", "{\"activity\":\"00\"}")?;
    assert_eq!(reply.status, 400, "unexpected status: {}", reply.text());
    assert!(reply.text().contains("\"ok\":false"));
    assert!(error_code(&reply).is_some(), "missing typed error code");

    let empty = post_json(&address, "/v1/activities", "{\"activity\":\"\"}")?;
    assert_eq!(empty.status, 400);
    assert_eq!(error_code(&empty).as_deref(), Some("invalid_argument"));
    Ok(())
}

#[test]
fn fault_injection_changes_transition_behaviour() -> Result<(), String> {
    let address = boot()?;
    let baseline = post_json(&address, "/v1/activities", "{\"activity\":\"00\"}")?;
    assert_eq!(baseline.status, 400);
    let baseline_code = error_code(&baseline).ok_or("baseline had no error code")?;

    let configured = post_json(
        &address,
        "/__emulator/faults",
        "{\"kind\":\"reject\",\"count\":1}",
    )?;
    assert_eq!(
        configured.status,
        200,
        "fault refused: {}",
        configured.text()
    );
    assert!(configured.text().contains("\"configured\":true"));

    let injected = post_json(&address, "/v1/activities", "{\"activity\":\"00\"}")?;
    assert_eq!(
        injected.status, 503,
        "reject fault surfaces as service unavailable"
    );
    let injected_code = error_code(&injected).ok_or("injected response had no error code")?;
    assert_ne!(
        baseline_code, injected_code,
        "reject fault did not alter the observed transition outcome"
    );

    let unknown = post_json(&address, "/__emulator/faults", "{\"kind\":\"nope\"}")?;
    assert_eq!(unknown.status, 400);
    assert_eq!(error_code(&unknown).as_deref(), Some("invalid_argument"));
    Ok(())
}

#[test]
fn time_control_is_monotonic() -> Result<(), String> {
    let address = boot()?;
    let target = 1_800_000_000_000_u64;
    let set = post_json(
        &address,
        "/__emulator/time/set",
        &format!("{{\"timestamp_ms\":{target}}}"),
    )?;
    assert_eq!(set.status, 200, "time set refused: {}", set.text());

    let state = request(&address, "GET", "/v1/state", "", &[])?;
    assert!(
        state.text().contains(&format!("\"timestamp_ms\":{target}")),
        "state did not adopt the controlled time: {}",
        state.text()
    );

    let advanced = post_json(&address, "/__emulator/time/advance", "{\"delta_ms\":1000}")?;
    assert_eq!(advanced.status, 200);
    let after = request(&address, "GET", "/v1/state", "", &[])?;
    assert!(
        after
            .text()
            .contains(&format!("\"timestamp_ms\":{}", target + 1000)),
        "advance did not move the clock: {}",
        after.text()
    );

    let backward = post_json(&address, "/__emulator/time/set", "{\"timestamp_ms\":1}")?;
    assert_eq!(backward.status, 400, "non-monotonic set was accepted");
    Ok(())
}

#[test]
fn snapshots_round_trip_through_the_core() -> Result<(), String> {
    let address = boot()?;
    let public_key = "22".repeat(32);
    let body = format!(
        "{{\"did\":\"did:layerx:agent-beta\",\"public_key\":\"{public_key}\",\"amount_lo\":777}}"
    );
    assert_eq!(
        post_json(&address, "/__emulator/accounts/prefund", &body)?.status,
        200
    );

    let exported = request(&address, "GET", "/__emulator/snapshot", "", &[])?;
    assert_eq!(exported.status, 200, "export refused: {}", exported.text());
    assert_eq!(
        exported.content_type,
        "application/vnd.layerx.emulator-snapshot"
    );
    assert!(!exported.body.is_empty(), "snapshot body was empty");

    let imported = request(
        &address,
        "PUT",
        "/__emulator/snapshot",
        "application/octet-stream",
        &exported.body,
    )?;
    assert_eq!(imported.status, 200, "import refused: {}", imported.text());
    assert!(imported.text().contains("\"imported\":true"));

    let state = request(&address, "GET", "/v1/state", "", &[])?;
    assert!(
        state.text().contains("agent:did:layerx:agent-beta:main"),
        "restored snapshot lost the prefunded account: {}",
        state.text()
    );
    Ok(())
}

#[test]
fn gateway_surface_matches_production_verbs() -> Result<(), String> {
    let address = boot()?;

    let wrong_verb = post_json(&address, "/v1/state", "{}")?;
    assert_eq!(wrong_verb.status, 405, "state accepted a write verb");
    assert_eq!(
        error_code(&wrong_verb).as_deref(),
        Some("method_not_allowed")
    );

    let activities_get = request(&address, "GET", "/v1/activities", "", &[])?;
    assert_eq!(
        activities_get.status, 405,
        "activities accepted a read verb"
    );

    let missing_receipt = request(&address, "GET", "/v1/receipts/unknown-id", "", &[])?;
    assert_eq!(missing_receipt.status, 404);
    assert_eq!(error_code(&missing_receipt).as_deref(), Some("not_found"));

    let unknown_route = request(&address, "GET", "/v1/does-not-exist", "", &[])?;
    assert_eq!(unknown_route.status, 404);
    assert_eq!(error_code(&unknown_route).as_deref(), Some("not_found"));
    Ok(())
}

fn lifecycle_echo_guest() -> Vec<u8> {
    use layerx_programs_runtime::test_support::{
        code_section, func_body, function_section, import_section, module, raw_section,
        type_section, unsigned_leb, TYPE_I32,
    };
    let mut exports = unsigned_leb(3);
    for (name, kind, index) in [
        ("layerx_reserve", 0_u8, 1_u8),
        ("layerx_call", 0, 2),
        ("memory", 2, 0),
    ] {
        exports.extend(unsigned_leb(
            u64::try_from(name.len()).unwrap_or_else(|error| panic!("{error}")),
        ));
        exports.extend_from_slice(name.as_bytes());
        exports.extend_from_slice(&[kind, index]);
    }
    module(&[
        type_section(&[
            (&[TYPE_I32, TYPE_I32, TYPE_I32], &[TYPE_I32]),
            (&[TYPE_I32], &[TYPE_I32]),
            (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
        ]),
        import_section(&[("layerx_v2", "response_write", 0)]),
        function_section(&[1, 2]),
        raw_section(5, &[1, 1, 1, 1]),
        raw_section(7, &exports),
        code_section(&[
            func_body(&[], &[0x41, 0, 0x0b]),
            func_body(
                &[],
                &[0x41, 7, 0x20, 0, 0x20, 1, 0x10, 0, 0x1a, 0x41, 7, 0x0b],
            ),
        ]),
    ])
}

fn lifecycle_signed_activity(
    ordinal: u16,
    payload_bytes: &[u8],
    sequence: u64,
) -> Result<(Vec<u8>, String, [u8; 32]), String> {
    lifecycle_signed_activity_for_protocol(ordinal, payload_bytes, sequence, 3)
}

fn lifecycle_signed_activity_for_protocol(
    ordinal: u16,
    payload_bytes: &[u8],
    sequence: u64,
    protocol: u16,
) -> Result<(Vec<u8>, String, [u8; 32]), String> {
    use ed25519_dalek::{Signer, SigningKey};
    use layerx_types::activity::{Authority, EnvelopeBuilder, Signature, TimestampBound};
    use layerx_types::amount::Amount;
    use layerx_types::ids::{Did, IdempotencyKey};
    use layerx_types::payload::{
        ActivityType, ModuleId, ModuleRegistration, ModuleRegistry, Payload,
    };
    fn checked<T, E: std::fmt::Debug>(result: Result<T, E>) -> Result<T, String> {
        result.map_err(|error| format!("{error:?}"))
    }
    let key = SigningKey::from_bytes(&EMULATOR_SEED);
    let kind = checked(ActivityType::new(ModuleId::Programs, ordinal))?;
    let registry = checked(ModuleRegistry::new(&[checked(ModuleRegistration::new(
        ModuleId::Programs,
        &[kind],
    ))?]))?;
    let payload = checked(Payload::new(&registry, kind, payload_bytes))?;
    let hash = checked(layerx_wire::hash::payload_hash_for(&payload))?;
    let mut idempotency = [7; 32];
    idempotency[24..].copy_from_slice(&sequence.to_be_bytes());
    let mut builder = EnvelopeBuilder::new();
    checked(builder.protocol_version(protocol))?;
    checked(builder.network_id(402))?;
    checked(builder.activity_type(kind))?;
    checked(builder.actor_did(checked(Did::new(b"did:layerx:lifecycle"))?))?;
    checked(builder.authority(checked(Authority::owner(&key.verifying_key().to_bytes()))?))?;
    checked(builder.account_sequence(sequence))?;
    checked(builder.timestamp_bound(checked(TimestampBound::new(
        1_699_999_970_000,
        1_700_000_120_000,
    ))?))?;
    checked(builder.idempotency_key(IdempotencyKey::new(idempotency)))?;
    checked(builder.fee_limit(Amount::from_u128(1_000_000)))?;
    checked(builder.payload_hash(hash))?;
    checked(builder.payload(payload))?;
    let unsigned = checked(builder.build())?;
    let preimage = checked(layerx_wire::sign::preimage_unsigned(&unsigned))?;
    let signature = key.sign(preimage.as_bytes()).to_bytes();
    let bytes = checked(layerx_wire::activity::encode_signed_envelope(
        &unsigned.attach_signature(checked(Signature::new(&signature))?),
    ))?;
    let activity = checked(layerx_wire::activity::decode_signed(&bytes, &registry))?;
    let id = checked(layerx_wire::hash::activity_id(&activity))?;
    let key = hex_encode(&idempotency)?;
    Ok((bytes, key, id))
}

type LifecycleOperation = (u16, &'static str, Vec<u8>);

fn lifecycle_operations(
) -> Result<(layerx_types::intent::ProgramId, [LifecycleOperation; 5]), String> {
    use layerx_types::intent::ProgramId;
    use layerx_types::program_lifecycle::{
        NativeProgramDeploy, NativeProgramUpgrade, NativeProgramWindDown, ProgramUpgradePolicy,
        ProgramWindDownOperation,
    };
    use sha2::{Digest, Sha256};
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../sdk/conformance/fixtures/native-program-deploy-v3.json");
    let fixture: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture_path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let encoded = hex_decode(fixture["payload_hex"].as_str().ok_or("missing C payload")?)?;
    let original = NativeProgramDeploy::decode(&encoded).map_err(|error| format!("{error:?}"))?;
    let account = layerx_types::account::AccountId::parse("agent:did:layerx:lifecycle:main")
        .map_err(|error| format!("{error:?}"))?;
    let principal = layerx_wire::hash::account_id_for_protocol(&account, 3)
        .map_err(|error| format!("{error:?}"))?;
    let program = ProgramId::new([0x71; 32]);
    let wasm = lifecycle_echo_guest();
    assert_eq!(
        original.encode().map_err(|error| format!("{error:?}"))?,
        encoded
    );
    let deploy = NativeProgramDeploy {
        program_id: program,
        guest_abi: 2,
        policy: ProgramUpgradePolicy::Authority(principal),
        wasm: &wasm,
        new_hash: Sha256::digest(&wasm).into(),
        interface: None,
    };
    let call = layerx_types::program_call::NativeProgramCall {
        program_id: program,
        guest_abi: 2,
        entrypoint: b"layerx_call",
        calldata: b"echo",
        capabilities: &[0, 0],
        access_declaration: b"LayerX/programs/access-declaration/v1\0\0",
        response_capacity: 4096,
        resources: layerx_types::program_call::Resources([100_000, 65_536, 0, 0, 2, 4096, 0]),
    };
    let mut upgraded_wasm = deploy.wasm.to_vec();
    upgraded_wasm.extend_from_slice(b"\0\x08\x07upgrade");
    let upgrade = NativeProgramUpgrade {
        program_id: program,
        guest_abi: 2,
        old_hash: deploy.new_hash,
        new_hash: Sha256::digest(&upgraded_wasm).into(),
        migration_hook: &[],
        clear_interface: false,
        interface: deploy.interface,
        wasm: &upgraded_wasm,
    };
    let deprecate = NativeProgramWindDown {
        program_id: program,
        operation: ProgramWindDownOperation::Deprecate {
            exit_program: program.bytes(),
            deadline_batch: 1000,
        },
    };
    Ok((
        program,
        [
            (
                1,
                "/v1/programs/deploy",
                deploy.encode().map_err(|error| format!("{error:?}"))?,
            ),
            (
                3,
                "/v1/programs/call",
                call.encode().map_err(|error| format!("{error:?}"))?,
            ),
            (
                2,
                "/v1/programs/upgrade",
                upgrade.encode().map_err(|error| format!("{error:?}"))?,
            ),
            (
                3,
                "/v1/programs/call",
                call.encode().map_err(|error| format!("{error:?}"))?,
            ),
            (
                7,
                "/v1/programs/wind-down",
                deprecate.encode().map_err(|error| format!("{error:?}"))?,
            ),
        ],
    ))
}

struct LifecycleRequest<'a> {
    address: &'a String,
    ordinal: u16,
    path: &'a str,
    payload: &'a [u8],
    bytes: &'a Vec<u8>,
    key: &'a String,
    expected_id: [u8; 32],
    public: [u8; 32],
    program: layerx_types::intent::ProgramId,
    sequence: usize,
}
mod lifecycle_checks {
    use super::*;

    pub(super) fn lifecycle_refusals(input: &LifecycleRequest<'_>) -> Result<(), String> {
        let address = input.address.clone();
        let ordinal = input.ordinal;
        let path = input.path;
        let payload = input.payload;
        let bytes = input.bytes.clone();
        let key = input.key.clone();
        assert_eq!(
            request(&address, "POST", path, "application/octet-stream", &bytes)?.status,
            400
        );
        if ordinal != 3 {
            assert_eq!(
                request_with_idempotency(
                    &address,
                    "POST",
                    path,
                    "application/json",
                    &bytes,
                    Some(&key)
                )?
                .status,
                415
            );
        }
        if ordinal == 1 {
            assert_eq!(
                request_with_idempotency(
                    &address,
                    "POST",
                    "/v1/programs/upgrade",
                    "application/octet-stream",
                    &bytes,
                    Some(&key)
                )?
                .status,
                400
            );
            let mut corrupt = payload.to_vec();
            corrupt[68] ^= 1;
            let (corrupt, corrupt_key, _) = lifecycle_signed_activity(ordinal, &corrupt, 0)?;
            assert_eq!(
                request_with_idempotency(
                    &address,
                    "POST",
                    path,
                    "application/octet-stream",
                    &corrupt,
                    Some(&corrupt_key)
                )?
                .status,
                400
            );
        }
        Ok(())
    }

    pub(super) fn lifecycle_submit(
        input: &LifecycleRequest<'_>,
    ) -> Result<serde_json::Value, String> {
        let address = input.address;
        let ordinal = input.ordinal;
        let path = input.path;
        let bytes = input.bytes;
        let key = input.key.as_str();
        let reply = request_with_idempotency(
            address,
            "POST",
            path,
            "application/octet-stream",
            bytes,
            Some(key),
        )?;
        assert_eq!(reply.status, 200, "{}", reply.text());
        let document: serde_json::Value =
            serde_json::from_slice(&reply.body).map_err(|error| error.to_string())?;
        let result = &document["result"];
        if ordinal == 3 {
            assert_eq!(result["idempotency_key"], key);
        } else {
            assert!(result.get("program_id").is_none());
            assert!(result.get("idempotency_key").is_none());
        }
        Ok(document)
    }

    pub(super) fn lifecycle_receipt(
        input: &LifecycleRequest<'_>,
        result: &serde_json::Value,
    ) -> Result<(), String> {
        let ordinal = input.ordinal;
        let public = input.public;
        let expected_id = input.expected_id;
        let program = input.program;
        let sequence = input.sequence;
        let receipt = hex_decode(result["receipt"].as_str().ok_or("receipt missing")?)?;
        let verified = layerx_proof::receipt::verify_sequencer_signature(&receipt, public)
            .map_err(|error| format!("{error:?}"))?;
        let protocol = verified.protocol().ok_or("protocol receipt missing")?;
        assert_eq!(protocol.activity_id(), expected_id);
        assert_eq!(
            protocol.global_sequence(),
            2 * u64::try_from(sequence).map_err(|error| error.to_string())? + 1
        );
        assert_eq!(
            (
                protocol.module_id(),
                protocol.module_version(),
                protocol.operation()
            ),
            (9, 4, if ordinal == 3 { 3 } else { 0 })
        );
        let execution_detail = if ordinal == 3 {
            let terminal = hex_decode(
                result["terminal_payload"]
                    .as_str()
                    .ok_or("terminal missing")?,
            )?;
            let graph = hex_decode(result["call_graph"].as_str().ok_or("graph missing")?)?;
            let execution = layerx_proof::program::verify_program_execution(
                &receipt,
                &terminal,
                &graph,
                layerx_proof::program::ProgramExecutionExpectation {
                    sequencer_public_key: public,
                    previous_state_root: protocol.previous_state_root(),
                    activity_id: expected_id,
                    program_id: program.bytes(),
                    guest_abi_version: 2,
                },
            )
            .map_err(|error| format!("{error:?}"))?;
            format!(
                "resource={:?} failure={:?} terminal={:?}",
                execution.authenticated_resource(),
                execution.authenticated_failure(),
                execution.terminal()
            )
        } else {
            String::new()
        };
        assert_eq!(
            protocol.result_code(),
            0,
            "ordinal={ordinal} sequence={sequence} activity={expected_id:02x?} {execution_detail}"
        );
        lifecycle_authority(input, result, &receipt, protocol)?;
        Ok(())
    }

    fn lifecycle_authority(
        input: &LifecycleRequest<'_>,
        result: &serde_json::Value,
        receipt: &[u8],
        protocol: &layerx_wire::receipt::ProtocolReceipt,
    ) -> Result<(), String> {
        let ordinal = input.ordinal;
        let public = input.public;
        let program = input.program;
        let expected_id = input.expected_id;
        let authority = layerx_proof::receipt::AuthorizedBatch::new(
            protocol.batch_id(),
            protocol.asset(),
            protocol.previous_state_root(),
            protocol.resulting_state_root(),
            public,
        );
        if ordinal == 3 {
            let terminal = hex_decode(
                result["terminal_payload"]
                    .as_str()
                    .ok_or("terminal missing")?,
            )?;
            let graph = hex_decode(result["call_graph"].as_str().ok_or("graph missing")?)?;
            let execution = layerx_proof::program::verify_program_execution(
                receipt,
                &terminal,
                &graph,
                layerx_proof::program::ProgramExecutionExpectation {
                    sequencer_public_key: public,
                    previous_state_root: protocol.previous_state_root(),
                    activity_id: expected_id,
                    program_id: program.bytes(),
                    guest_abi_version: 2,
                },
            )
            .map_err(|error| format!("{error:?}"))?;
            assert!(execution.fee_units() > 0);
            assert!(execution.cpu_fuel() > 0);
            assert!(execution.memory_bytes() >= 65_536);
            assert_eq!(execution.output_values(), 2);
        } else {
            assert!(protocol.program_outcome().is_none());
            layerx_proof::receipt::verify_program_state(receipt, &authority)
                .map_err(|error| format!("{error:?}"))?;
        }
        let mut corrupt = receipt.to_vec();
        let last = corrupt.len() - 1;
        corrupt[last] ^= 1;
        assert!(layerx_proof::receipt::verify_sequencer_signature(&corrupt, public).is_err());
        if ordinal != 3 {
            assert!(layerx_proof::receipt::verify_program_state(&corrupt, &authority).is_err());
        }
        Ok(())
    }

    pub(super) fn lifecycle_replay(
        input: &LifecycleRequest<'_>,
        result: &serde_json::Value,
    ) -> Result<(), String> {
        let address = input.address.clone();
        let ordinal = input.ordinal;
        let path = input.path;
        let payload = input.payload;
        let bytes = input.bytes;
        let key = input.key.as_str();
        let sequence = input.sequence;
        let replay = request_with_idempotency(
            &address,
            "POST",
            path,
            "application/octet-stream",
            bytes,
            Some(key),
        )?;
        assert_eq!(replay.status, 200, "{}", replay.text());
        let replay: serde_json::Value =
            serde_json::from_slice(&replay.body).map_err(|error| error.to_string())?;
        assert_eq!(replay["result"], *result);
        if ordinal == 3 {
            let snapshot = request(&address, "GET", "/__emulator/snapshot", "", &[])?;
            assert_eq!(
                snapshot.status,
                200,
                "snapshot export after sequence={sequence}: {}",
                snapshot.text()
            );
            let imported = request(
                &address,
                "PUT",
                "/__emulator/snapshot",
                "application/octet-stream",
                &snapshot.body,
            )?;
            assert_eq!(
                imported.status,
                200,
                "snapshot import after sequence={sequence}: {}",
                imported.text()
            );
        }
        if ordinal == 1 {
            let mut conflict = payload.to_vec();
            conflict[0] ^= 1;
            let (conflict, conflict_key, _) = lifecycle_signed_activity(ordinal, &conflict, 0)?;
            assert_eq!(
                request_with_idempotency(
                    &address,
                    "POST",
                    path,
                    "application/octet-stream",
                    &conflict,
                    Some(&conflict_key)
                )?
                .status,
                409
            );
        }
        Ok(())
    }
}

#[test]
fn lifecycle_routes_execute_and_replay_real_native_state_receipts() -> Result<(), String> {
    let address = boot_protocol(3)?;
    let signing_key = ed25519_dalek::SigningKey::from_bytes(&EMULATOR_SEED);
    let public = signing_key.verifying_key().to_bytes();
    let public_hex = hex_encode(&public)?;
    let prefund = format!("{{\"did\":\"did:layerx:lifecycle\",\"public_key\":\"{public_hex}\",\"amount_lo\":100000000}}");
    assert_eq!(
        post_json(&address, "/__emulator/accounts/prefund", &prefund)?.status,
        200
    );
    let (program, operations) = lifecycle_operations()?;
    for (sequence, (ordinal, path, payload)) in operations.into_iter().enumerate() {
        let (bytes, key, expected_id) = lifecycle_signed_activity(
            ordinal,
            &payload,
            u64::try_from(sequence).map_err(|error| error.to_string())?,
        )?;
        let input = LifecycleRequest {
            address: &address,
            ordinal,
            path,
            payload: &payload,
            bytes: &bytes,
            key: &key,
            expected_id,
            public,
            program,
            sequence,
        };
        lifecycle_checks::lifecycle_refusals(&input)?;
        let document = lifecycle_checks::lifecycle_submit(&input)?;
        let result = &document["result"];
        lifecycle_checks::lifecycle_receipt(&input, result)?;
        lifecycle_checks::lifecycle_replay(&input, result)?;
        let state = request(&address, "GET", "/v1/state", "", &[])?;
        assert_eq!(state.status, 200);
        let state: serde_json::Value =
            serde_json::from_slice(&state.body).map_err(|error| error.to_string())?;
        assert_eq!(state["result"]["batch_number"], sequence + 1);
        assert_eq!(state["result"]["next_sequence"], 2 * (sequence + 1) + 1);
    }
    Ok(())
}

fn prefund_profiles(legacy: &String, native: &String) -> Result<(), String> {
    for address in [&legacy, &native] {
        let public = ed25519_dalek::SigningKey::from_bytes(&EMULATOR_SEED)
            .verifying_key()
            .to_bytes();
        let public = hex_encode(&public)?;
        let body = format!("{{\"did\":\"did:layerx:lifecycle\",\"public_key\":\"{public}\",\"amount_lo\":1000000}}");
        assert_eq!(
            post_json(address, "/__emulator/accounts/prefund", &body)?.status,
            200
        );
    }
    Ok(())
}

#[test]
fn native_and_legacy_profiles_keep_snapshot_and_module_versions_separate() -> Result<(), String> {
    use layerx_types::program_lifecycle::{NativeProgramDeploy, ProgramUpgradePolicy};
    let legacy = boot_protocol(2)?;
    let native = boot_protocol(3)?;
    prefund_profiles(&legacy, &native)?;
    let legacy_snapshot = request(&legacy, "GET", "/__emulator/snapshot", "", &[])?;
    let native_snapshot = request(&native, "GET", "/__emulator/snapshot", "", &[])?;
    assert_eq!(legacy_snapshot.status, 200);
    assert_eq!(native_snapshot.status, 200);
    for (target, snapshot) in [(&legacy, &native_snapshot), (&native, &legacy_snapshot)] {
        assert_eq!(
            request(
                target,
                "PUT",
                "/__emulator/snapshot",
                "application/octet-stream",
                &snapshot.body
            )?
            .status,
            400
        );
    }
    for (target, snapshot) in [(&legacy, &legacy_snapshot), (&native, &native_snapshot)] {
        assert_eq!(
            request(
                target,
                "PUT",
                "/__emulator/snapshot",
                "application/octet-stream",
                &snapshot.body
            )?
            .status,
            200
        );
    }
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../sdk/conformance/fixtures/native-program-deploy-v3.json");
    let fixture: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let encoded = hex_decode(fixture["payload_hex"].as_str().ok_or("missing C payload")?)?;
    let original = NativeProgramDeploy::decode(&encoded).map_err(|error| format!("{error:?}"))?;
    let deploy = NativeProgramDeploy {
        guest_abi: 1,
        policy: ProgramUpgradePolicy::Immutable,
        interface: None,
        ..original
    };
    let payload = deploy.encode().map_err(|error| format!("{error:?}"))?;
    for (protocol, own, other, expected_module) in
        [(2, &legacy, &native, 1), (3, &native, &legacy, 4)]
    {
        let (signed, _, _) = lifecycle_signed_activity_for_protocol(1, &payload, 0, protocol)?;
        assert_eq!(
            request(
                other,
                "POST",
                "/v1/activities",
                "application/octet-stream",
                &signed
            )?
            .status,
            400
        );
        let reply = request(
            own,
            "POST",
            "/v1/activities",
            "application/octet-stream",
            &signed,
        )?;
        assert_eq!(reply.status, 200, "{}", reply.text());
        let body: serde_json::Value =
            serde_json::from_slice(&reply.body).map_err(|error| error.to_string())?;
        let bytes = hex_decode(
            body["result"]["receipt"]
                .as_str()
                .ok_or("missing receipt")?,
        )?;
        let public = ed25519_dalek::SigningKey::from_bytes(&EMULATOR_SEED)
            .verifying_key()
            .to_bytes();
        let receipt = layerx_proof::receipt::verify_sequencer_signature(&bytes, public)
            .map_err(|error| format!("{error:?}"))?;
        let protocol_receipt = receipt.protocol().ok_or("missing protocol receipt")?;
        assert_eq!(protocol_receipt.protocol_version(), protocol);
        assert_eq!(protocol_receipt.module_version(), expected_module);
        assert_eq!(protocol_receipt.result_code(), 0);
    }
    Ok(())
}

struct MoveSetup {
    address: String,
    source: &'static str,
    destination: &'static str,
    before_state: serde_json::Value,
    before_root: String,
    before_receipt_root: String,
}
struct MoveCommit {
    quote_body: String,
    commit_body: String,
    idempotency: &'static str,
    committed: Reply,
    committed_result: serde_json::Value,
}
struct MoveRoots {
    committed_root: String,
    committed_receipt_root: String,
}
struct MoveRace {
    competing_b_id: String,
    winner_root: String,
    winner_receipt_root: String,
}
fn move_setup() -> Result<MoveSetup, String> {
    let address = boot()?;
    let source_public = ed25519_dalek::SigningKey::from_bytes(&EMULATOR_SEED)
        .verifying_key()
        .to_bytes();
    let source_public = hex_encode(&source_public)?;
    let destination_public = "24".repeat(32);
    let source = "agent:did:layerx:move-source:main";
    let destination = "agent:did:layerx:move-destination:main";
    let source_prefund = format!(
        "{{\"did\":\"did:layerx:move-source\",\"public_key\":\"{source_public}\",\"amount_lo\":1000}}"
    );
    let destination_prefund = format!(
        "{{\"did\":\"did:layerx:move-destination\",\"public_key\":\"{destination_public}\",\"amount_lo\":0}}"
    );
    assert_eq!(
        post_json(&address, "/__emulator/accounts/prefund", &source_prefund)?.status,
        200
    );
    assert_eq!(
        post_json(
            &address,
            "/__emulator/accounts/prefund",
            &destination_prefund
        )?
        .status,
        200
    );
    let before_reply = request(&address, "GET", "/v1/state", "", &[])?;
    let before_state = response_result(&before_reply)?;
    let before_root = before_state
        .get("state_root")
        .and_then(serde_json::Value::as_str)
        .ok_or("pre-move state omitted state_root")?
        .to_owned();
    assert_eq!(
        before_state
            .get("canonical_state_root")
            .and_then(serde_json::Value::as_str),
        Some(before_root.as_str())
    );
    let before_receipt_root = before_state
        .get("receipt_state_root")
        .and_then(serde_json::Value::as_str)
        .ok_or("pre-move state omitted receipt_state_root")?
        .to_owned();
    assert_ne!(before_receipt_root, before_root);

    Ok(MoveSetup {
        address,
        source,
        destination,
        before_state,
        before_root,
        before_receipt_root,
    })
}

fn move_commit(setup: &MoveSetup) -> Result<MoveCommit, String> {
    let address = &setup.address;
    let source = setup.source;
    let destination = setup.destination;
    let quote_body = format!(
        "{{\"source\":\"{source}\",\"destination\":\"{destination}\",\"money\":{{\"currency\":\"LXP\",\"amount\":\"250\"}}}}"
    );
    let quote_reply = post_json(address, "/v1/moves/quote", &quote_body)?;
    assert_eq!(
        quote_reply.status,
        200,
        "quote failed: {}",
        quote_reply.text()
    );
    let quote = response_result(&quote_reply)?;
    let quote_id = quote
        .get("quote_id")
        .and_then(serde_json::Value::as_str)
        .ok_or("quote omitted quote_id")?;
    assert_eq!(
        quote
            .pointer("/money/amount")
            .and_then(serde_json::Value::as_str),
        Some("250")
    );
    let commit_body = format!("{{\"quote_id\":\"{quote_id}\"}}");
    let idempotency = "move-payment-test-0001";
    let committed = request_with_idempotency(
        address,
        "POST",
        "/v1/moves",
        "application/json",
        commit_body.as_bytes(),
        Some(idempotency),
    )?;
    assert_eq!(committed.status, 200, "commit failed: {}", committed.text());
    let committed_result = response_result(&committed)?;
    assert_eq!(
        committed_result
            .get("state")
            .and_then(serde_json::Value::as_str),
        Some("done")
    );

    Ok(MoveCommit {
        quote_body,
        commit_body,
        idempotency,
        committed,
        committed_result,
    })
}

fn move_receipt(
    setup: &MoveSetup,
    payment: &MoveCommit,
) -> Result<layerx_wire::receipt::Receipt, String> {
    let address = &setup.address;
    let source = setup.source;
    let destination = setup.destination;
    let before_state = &setup.before_state;
    let before_receipt_root = setup.before_receipt_root.clone();
    let committed_result = &payment.committed_result;
    let receipt_path = committed_result
        .pointer("/evidence/0/source_ref")
        .and_then(serde_json::Value::as_str)
        .ok_or("move journey omitted receipt source_ref")?;
    let receipt_reply = request(address, "GET", receipt_path, "", &[])?;
    assert_eq!(receipt_reply.status, 200);
    let receipt_result = response_result(&receipt_reply)?;
    let receipt_hex = receipt_result
        .get("receipt")
        .and_then(serde_json::Value::as_str)
        .ok_or("receipt lookup omitted canonical bytes")?;
    let receipt_bytes = hex_decode(receipt_hex)?;
    let decoded =
        layerx_wire::receipt::decode(&receipt_bytes).map_err(|error| format!("{error:?}"))?;
    let protocol = decoded
        .protocol()
        .ok_or("move receipt was not protocol receipt")?;
    let before_accounts = before_state
        .get("accounts")
        .and_then(serde_json::Value::as_array)
        .ok_or("pre-move state omitted accounts")?;
    let source_id = before_accounts
        .iter()
        .find(|account| account.get("name").and_then(serde_json::Value::as_str) == Some(source))
        .and_then(|account| account.get("id"))
        .and_then(serde_json::Value::as_str)
        .ok_or("pre-move state omitted source id")?;
    let destination_id = before_accounts
        .iter()
        .find(|account| {
            account.get("name").and_then(serde_json::Value::as_str) == Some(destination)
        })
        .and_then(|account| account.get("id"))
        .and_then(serde_json::Value::as_str)
        .ok_or("pre-move state omitted destination id")?;
    assert_eq!(
        protocol.protocol_version(),
        layerx_wire::limits::PROTOCOL_VERSION
    );
    assert_eq!(protocol.module_id(), 1);
    assert_eq!(protocol.operation(), 5);
    assert_eq!(protocol.amount(), 250);
    assert_eq!(
        protocol.from().as_slice(),
        hex_decode(source_id)?.as_slice()
    );
    assert_eq!(
        protocol.to().as_slice(),
        hex_decode(destination_id)?.as_slice()
    );
    assert_eq!(protocol.debit_balance_before(), 1000);
    assert_eq!(protocol.debit_balance_after(), 750);
    assert_eq!(protocol.credit_balance_before(), 0);
    assert_eq!(protocol.credit_balance_after(), 250);
    assert_eq!(protocol.debit_sequence(), 0);
    assert_eq!(
        protocol.previous_state_root().as_slice(),
        hex_decode(&before_receipt_root)?.as_slice()
    );
    assert_ne!(
        protocol.previous_state_root(),
        protocol.resulting_state_root()
    );
    assert_ne!(protocol.transfer_set_root(), [0; 32]);
    assert_eq!(protocol.effects().len(), 1);
    assert_eq!(protocol.effects()[0].kind(), 2);
    assert!(protocol.effects()[0].monetary());
    assert_eq!(
        protocol.effects()[0].transfer_set_root(),
        protocol.transfer_set_root()
    );

    Ok(decoded)
}

fn move_replay(
    setup: &MoveSetup,
    payment: &MoveCommit,
    protocol: &layerx_wire::receipt::ProtocolReceipt,
) -> Result<MoveRoots, String> {
    let address = &setup.address;
    let source = setup.source;
    let destination = setup.destination;
    let before_root = setup.before_root.as_str();
    let commit_body = &payment.commit_body;
    let idempotency = payment.idempotency;
    let committed = &payment.committed;
    let replayed = request_with_idempotency(
        address,
        "POST",
        "/v1/moves",
        "application/json",
        commit_body.as_bytes(),
        Some(idempotency),
    )?;
    assert_eq!(replayed.status, 200);
    assert_eq!(replayed.body, committed.body);
    let state_reply = request(address, "GET", "/v1/state", "", &[])?;
    let state = response_result(&state_reply)?;
    let committed_root = state
        .get("state_root")
        .and_then(serde_json::Value::as_str)
        .ok_or("committed state omitted state_root")?
        .to_owned();
    let committed_receipt_root = state
        .get("receipt_state_root")
        .and_then(serde_json::Value::as_str)
        .ok_or("committed state omitted receipt_state_root")?
        .to_owned();
    assert_eq!(
        hex_decode(&committed_receipt_root)?.as_slice(),
        protocol.resulting_state_root().as_slice()
    );
    assert_ne!(
        committed_root, before_root,
        "move did not change account root"
    );
    let accounts = state
        .get("accounts")
        .and_then(serde_json::Value::as_array)
        .ok_or("committed state omitted accounts")?;
    let source_state = accounts
        .iter()
        .find(|account| account.get("name").and_then(serde_json::Value::as_str) == Some(source))
        .ok_or("committed state omitted source")?;
    let destination_state = accounts
        .iter()
        .find(|account| {
            account.get("name").and_then(serde_json::Value::as_str) == Some(destination)
        })
        .ok_or("committed state omitted destination")?;
    assert_eq!(
        source_state
            .get("balance_lo")
            .and_then(serde_json::Value::as_u64),
        Some(750)
    );
    assert_eq!(
        source_state
            .get("next_sequence")
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
    assert_eq!(
        destination_state
            .get("balance_lo")
            .and_then(serde_json::Value::as_u64),
        Some(250)
    );
    assert_eq!(
        destination_state
            .get("next_sequence")
            .and_then(serde_json::Value::as_u64),
        Some(0)
    );

    Ok(MoveRoots {
        committed_root,
        committed_receipt_root,
    })
}

fn move_recovery(setup: &MoveSetup, payment: &MoveCommit, roots: &MoveRoots) -> Result<(), String> {
    let address = &setup.address;
    let commit_body = &payment.commit_body;
    let idempotency = payment.idempotency;
    let committed = &payment.committed;
    let committed_root = &roots.committed_root;
    let committed_receipt_root = &roots.committed_receipt_root;
    let snapshot = request(address, "GET", "/__emulator/snapshot", "", &[])?;
    assert_eq!(snapshot.status, 200);
    let imported = request(
        address,
        "PUT",
        "/__emulator/snapshot",
        "application/octet-stream",
        &snapshot.body,
    )?;
    assert_eq!(
        imported.status,
        200,
        "snapshot import failed: {}",
        imported.text()
    );
    let recovered_replay = request_with_idempotency(
        address,
        "POST",
        "/v1/moves",
        "application/json",
        commit_body.as_bytes(),
        Some(idempotency),
    )?;
    assert_eq!(recovered_replay.status, 200);
    assert_eq!(recovered_replay.body, committed.body);
    let recovered_state_reply = request(address, "GET", "/v1/state", "", &[])?;
    let recovered_state = response_result(&recovered_state_reply)?;
    assert_eq!(
        recovered_state
            .get("state_root")
            .and_then(serde_json::Value::as_str),
        Some(committed_root.as_str()),
        "snapshot recovery changed the committed account root"
    );
    assert_eq!(
        recovered_state
            .get("receipt_state_root")
            .and_then(serde_json::Value::as_str),
        Some(committed_receipt_root.as_str()),
        "snapshot recovery changed the committed receipt root"
    );

    Ok(())
}

fn move_refusal(setup: &MoveSetup, payment: &MoveCommit, roots: &MoveRoots) -> Result<(), String> {
    let address = &setup.address;
    let quote_body = &payment.quote_body;
    let committed_root = &roots.committed_root;
    let committed_receipt_root = &roots.committed_receipt_root;
    let insufficient = quote_body.replace("\"250\"", "\"9999\"");
    let insufficient_reply = post_json(address, "/v1/moves/quote", &insufficient)?;
    assert_eq!(insufficient_reply.status, 409);
    assert_eq!(
        error_code(&insufficient_reply).as_deref(),
        Some("move_balance_unavailable")
    );
    let after_refusal_reply = request(address, "GET", "/v1/state", "", &[])?;
    let after_refusal = response_result(&after_refusal_reply)?;
    assert_eq!(
        after_refusal
            .get("state_root")
            .and_then(serde_json::Value::as_str),
        Some(committed_root.as_str()),
        "refused quote changed canonical state"
    );
    assert_eq!(
        after_refusal
            .get("receipt_state_root")
            .and_then(serde_json::Value::as_str),
        Some(committed_receipt_root.as_str()),
        "refused quote changed receipt state"
    );

    Ok(())
}

fn move_conflict(
    setup: &MoveSetup,
    payment: &MoveCommit,
    roots: &MoveRoots,
) -> Result<String, String> {
    let address = &setup.address;
    let quote_body = &payment.quote_body;
    let idempotency = payment.idempotency;
    let committed_root = &roots.committed_root;
    let committed_receipt_root = &roots.committed_receipt_root;
    let second_quote_body = quote_body.replace("\"250\"", "\"100\"");
    let second_quote_reply = post_json(address, "/v1/moves/quote", &second_quote_body)?;
    assert_eq!(second_quote_reply.status, 200);
    let second_quote = response_result(&second_quote_reply)?;
    let second_quote_id = second_quote
        .get("quote_id")
        .and_then(serde_json::Value::as_str)
        .ok_or("second quote omitted quote_id")?;
    let second_commit = format!("{{\"quote_id\":\"{second_quote_id}\"}}");
    let conflicting = request_with_idempotency(
        address,
        "POST",
        "/v1/moves",
        "application/json",
        second_commit.as_bytes(),
        Some(idempotency),
    )?;
    assert_eq!(conflicting.status, 409);
    assert_eq!(
        error_code(&conflicting).as_deref(),
        Some("idempotency_conflict")
    );
    let after_conflict_reply = request(address, "GET", "/v1/state", "", &[])?;
    let after_conflict = response_result(&after_conflict_reply)?;
    assert_eq!(
        after_conflict
            .get("state_root")
            .and_then(serde_json::Value::as_str),
        Some(committed_root.as_str()),
        "idempotency conflict caused a second debit"
    );
    assert_eq!(
        after_conflict
            .get("receipt_state_root")
            .and_then(serde_json::Value::as_str),
        Some(committed_receipt_root.as_str()),
        "idempotency conflict changed receipt state"
    );

    Ok(second_commit)
}

fn move_lost_ack(setup: &MoveSetup, second_commit: &str) -> Result<(), String> {
    let address = setup.address.clone();
    assert_eq!(
        post_json(
            &address,
            "/__emulator/faults",
            "{\"kind\":\"drop_receipt\",\"count\":1}"
        )?
        .status,
        200
    );
    let second_key = "move-payment-test-0002";
    let lost_ack = request_with_idempotency(
        &address,
        "POST",
        "/v1/moves",
        "application/json",
        second_commit.as_bytes(),
        Some(second_key),
    )?;
    assert_eq!(lost_ack.status, 503);
    assert_eq!(
        error_code(&lost_ack).as_deref(),
        Some("move_acknowledgement_lost")
    );
    let resolved = request_with_idempotency(
        &address,
        "POST",
        "/v1/moves",
        "application/json",
        second_commit.as_bytes(),
        Some(second_key),
    )?;
    assert_eq!(
        resolved.status,
        200,
        "lost acknowledgement did not resolve: {}",
        resolved.text()
    );

    Ok(())
}

fn move_final_state(setup: &MoveSetup, roots: &MoveRoots) -> Result<(), String> {
    let address = &setup.address;
    let source = setup.source;
    let destination = setup.destination;
    let committed_root = &roots.committed_root;
    let final_state_reply = request(address, "GET", "/v1/state", "", &[])?;
    let final_state = response_result(&final_state_reply)?;
    let final_root = final_state
        .get("state_root")
        .and_then(serde_json::Value::as_str)
        .ok_or("final state omitted state_root")?;
    assert_ne!(final_root, committed_root);
    let final_accounts = final_state
        .get("accounts")
        .and_then(serde_json::Value::as_array)
        .ok_or("final state omitted accounts")?;
    let final_source = final_accounts
        .iter()
        .find(|account| account.get("name").and_then(serde_json::Value::as_str) == Some(source))
        .ok_or("final state omitted source")?;
    let final_destination = final_accounts
        .iter()
        .find(|account| {
            account.get("name").and_then(serde_json::Value::as_str) == Some(destination)
        })
        .ok_or("final state omitted destination")?;
    assert_eq!(
        final_source
            .get("balance_lo")
            .and_then(serde_json::Value::as_u64),
        Some(650)
    );
    assert_eq!(
        final_source
            .get("next_sequence")
            .and_then(serde_json::Value::as_u64),
        Some(2)
    );
    assert_eq!(
        final_destination
            .get("balance_lo")
            .and_then(serde_json::Value::as_u64),
        Some(350)
    );
    assert_eq!(
        final_destination
            .get("next_sequence")
            .and_then(serde_json::Value::as_u64),
        Some(0)
    );

    Ok(())
}

fn move_competing_quotes(setup: &MoveSetup, payment: &MoveCommit) -> Result<MoveRace, String> {
    let address = &setup.address;
    let quote_body = &payment.quote_body;
    let replies = CompetingReplies {
        competing_a_reply: post_json(
            address,
            "/v1/moves/quote",
            &quote_body.replace("\"250\"", "\"50\""),
        )?,
        competing_b_reply: post_json(
            address,
            "/v1/moves/quote",
            &quote_body.replace("\"250\"", "\"60\""),
        )?,
    };
    let ids = competing_quote_ids(&replies)?;
    finish_competing_quotes(setup, &ids)
}

struct CompetingReplies {
    competing_a_reply: Reply,
    competing_b_reply: Reply,
}
struct CompetingIds {
    competing_a_id: String,
    competing_b_id: String,
}
fn competing_quote_ids(
    CompetingReplies {
        competing_a_reply,
        competing_b_reply,
    }: &CompetingReplies,
) -> Result<CompetingIds, String> {
    assert_eq!(competing_a_reply.status, 200);
    assert_eq!(competing_b_reply.status, 200);
    let competing_a = response_result(competing_a_reply)?;
    let competing_b = response_result(competing_b_reply)?;
    Ok(CompetingIds {
        competing_a_id: competing_a
            .get("quote_id")
            .and_then(serde_json::Value::as_str)
            .ok_or("first competing quote omitted quote_id")?
            .to_owned(),
        competing_b_id: competing_b
            .get("quote_id")
            .and_then(serde_json::Value::as_str)
            .ok_or("second competing quote omitted quote_id")?
            .to_owned(),
    })
}
fn finish_competing_quotes(
    setup: &MoveSetup,
    CompetingIds {
        competing_a_id,
        competing_b_id,
    }: &CompetingIds,
) -> Result<MoveRace, String> {
    let address = &setup.address;
    assert_ne!(competing_a_id, competing_b_id);
    let winner = request_with_idempotency(
        address,
        "POST",
        "/v1/moves",
        "application/json",
        format!("{{\"quote_id\":\"{competing_a_id}\"}}").as_bytes(),
        Some("move-payment-race-0003"),
    )?;
    assert_eq!(
        winner.status,
        200,
        "winning quote failed: {}",
        winner.text()
    );
    let winner_state_reply = request(address, "GET", "/v1/state", "", &[])?;
    let winner_state = response_result(&winner_state_reply)?;
    let winner_root = winner_state
        .get("state_root")
        .and_then(serde_json::Value::as_str)
        .ok_or("winner state omitted state_root")?
        .to_owned();
    let winner_receipt_root = winner_state
        .get("receipt_state_root")
        .and_then(serde_json::Value::as_str)
        .ok_or("winner state omitted receipt_state_root")?
        .to_owned();

    Ok(MoveRace {
        competing_b_id: competing_b_id.to_owned(),
        winner_root,
        winner_receipt_root,
    })
}

fn move_stale_quote(setup: &MoveSetup, race: &MoveRace) -> Result<(), String> {
    let address = &setup.address;
    let source = setup.source;
    let destination = setup.destination;
    let competing_b_id = &race.competing_b_id;
    let winner_root = &race.winner_root;
    let winner_receipt_root = &race.winner_receipt_root;
    let loser = request_with_idempotency(
        address,
        "POST",
        "/v1/moves",
        "application/json",
        format!("{{\"quote_id\":\"{competing_b_id}\"}}").as_bytes(),
        Some("move-payment-race-0004"),
    )?;
    assert_eq!(loser.status, 409);
    assert_eq!(error_code(&loser).as_deref(), Some("move_quote_stale"));
    let race_state_reply = request(address, "GET", "/v1/state", "", &[])?;
    let race_state = response_result(&race_state_reply)?;
    assert_eq!(
        race_state
            .get("state_root")
            .and_then(serde_json::Value::as_str),
        Some(winner_root.as_str()),
        "losing same-sequence quote changed post-winner state"
    );
    assert_eq!(
        race_state
            .get("receipt_state_root")
            .and_then(serde_json::Value::as_str),
        Some(winner_receipt_root.as_str()),
        "losing same-sequence quote changed post-winner receipt root"
    );
    let race_accounts = race_state
        .get("accounts")
        .and_then(serde_json::Value::as_array)
        .ok_or("race state omitted accounts")?;
    let race_source = race_accounts
        .iter()
        .find(|account| account.get("name").and_then(serde_json::Value::as_str) == Some(source))
        .ok_or("race state omitted source")?;
    let race_destination = race_accounts
        .iter()
        .find(|account| {
            account.get("name").and_then(serde_json::Value::as_str) == Some(destination)
        })
        .ok_or("race state omitted destination")?;
    assert_eq!(
        race_source
            .get("balance_lo")
            .and_then(serde_json::Value::as_u64),
        Some(600)
    );
    assert_eq!(
        race_source
            .get("next_sequence")
            .and_then(serde_json::Value::as_u64),
        Some(3)
    );
    assert_eq!(
        race_destination
            .get("balance_lo")
            .and_then(serde_json::Value::as_u64),
        Some(400)
    );
    Ok(())
}

#[test]
fn move_quote_commit_replay_recovery_and_lost_ack_use_the_real_transition() -> Result<(), String> {
    let setup = move_setup()?;
    let payment = move_commit(&setup)?;
    let decoded = move_receipt(&setup, &payment)?;
    let protocol = decoded
        .protocol()
        .ok_or("move receipt was not protocol receipt")?;
    let roots = move_replay(&setup, &payment, protocol)?;
    move_recovery(&setup, &payment, &roots)?;
    move_refusal(&setup, &payment, &roots)?;
    let second_commit = move_conflict(&setup, &payment, &roots)?;
    move_lost_ack(&setup, &second_commit)?;
    move_final_state(&setup, &roots)?;
    let race = move_competing_quotes(&setup, &payment)?;
    move_stale_quote(&setup, &race)?;
    Ok(())
}

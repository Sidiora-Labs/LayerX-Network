//! In-process integration coverage for the local emulator gateway.
//!
//! These tests boot the real `layerx_platform_emulator::run` listener, which
//! links the production LayerX core transition and receipt machinery through
//! the C bridge, and drive it over its HTTP surface exactly as an SDK or the
//! middleware would. They assert the production gateway surface
//! (`/v1/activities`, `/v1/state`, `/v1/receipts/<id>`) and the emulator-only
//! control hooks (`/__emulator/*`) that live clearly outside the deterministic
//! transition path.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

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
    let mut stream = TcpStream::connect(address).map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|error| error.to_string())?;
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    if !content_type.is_empty() {
        head.push_str(&format!("Content-Type: {content_type}\r\n"));
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
    let port = free_port()?;
    let address = format!("127.0.0.1:{port}");
    let listen = address.clone();
    thread::spawn(move || {
        let _ = layerx_platform_emulator::run(vec!["up".to_string(), "--listen".to_string(), listen]);
    });
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Ok(reply) = request(&address, "GET", "/healthz", "", &[]) {
            if reply.status == 200 {
                return Ok(address);
            }
        }
        if Instant::now() >= deadline {
            return Err("emulator did not become ready".to_string());
        }
        thread::sleep(Duration::from_millis(20));
    }
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

#[test]
fn healthz_reports_the_layerx_core() -> Result<(), String> {
    let address = boot()?;
    let reply = request(&address, "GET", "/healthz", "", &[])?;
    assert_eq!(reply.status, 200);
    let text = reply.text();
    assert!(text.contains("\"status\":\"ready\""), "unexpected body: {text}");
    assert!(text.contains("\"core\":\"layerx\""), "unexpected body: {text}");
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
    let body = format!(
        "{{\"did\":\"agent-alpha\",\"public_key\":\"{public_key}\",\"amount_lo\":123456}}"
    );
    let reply = post_json(&address, "/__emulator/accounts/prefund", &body)?;
    assert_eq!(reply.status, 200, "prefund failed: {}", reply.text());
    assert!(reply.text().contains("\"prefunded\":true"));

    let state = request(&address, "GET", "/v1/state", "", &[])?;
    assert_eq!(state.status, 200);
    let text = state.text();
    assert!(
        text.contains("agent:agent-alpha:main"),
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

    let configured = post_json(&address, "/__emulator/faults", "{\"kind\":\"reject\",\"count\":1}")?;
    assert_eq!(configured.status, 200, "fault refused: {}", configured.text());
    assert!(configured.text().contains("\"configured\":true"));

    let injected = post_json(&address, "/v1/activities", "{\"activity\":\"00\"}")?;
    assert_eq!(injected.status, 503, "reject fault surfaces as service unavailable");
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
    let set = post_json(&address, "/__emulator/time/set", &format!("{{\"timestamp_ms\":{target}}}"))?;
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
        after.text().contains(&format!("\"timestamp_ms\":{}", target + 1000)),
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
    let body = format!("{{\"did\":\"agent-beta\",\"public_key\":\"{public_key}\",\"amount_lo\":777}}");
    assert_eq!(post_json(&address, "/__emulator/accounts/prefund", &body)?.status, 200);

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
        state.text().contains("agent:agent-beta:main"),
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
    assert_eq!(error_code(&wrong_verb).as_deref(), Some("method_not_allowed"));

    let activities_get = request(&address, "GET", "/v1/activities", "", &[])?;
    assert_eq!(activities_get.status, 405, "activities accepted a read verb");

    let missing_receipt = request(&address, "GET", "/v1/receipts/unknown-id", "", &[])?;
    assert_eq!(missing_receipt.status, 404);
    assert_eq!(error_code(&missing_receipt).as_deref(), Some("not_found"));

    let unknown_route = request(&address, "GET", "/v1/does-not-exist", "", &[])?;
    assert_eq!(unknown_route.status, 404);
    assert_eq!(error_code(&unknown_route).as_deref(), Some("not_found"));
    Ok(())
}

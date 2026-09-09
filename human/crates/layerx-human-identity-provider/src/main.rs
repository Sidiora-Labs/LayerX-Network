#![forbid(unsafe_code)]

use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use layerx_human_identity_provider::{Policy, Server, State};
use layerx_human_service::auth::Device;
use layerx_human_service::store::PrincipalId;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Enrollment {
    principal: String,
    assertion_id: String,
    device: Device,
}

fn required(name: &str) -> io::Result<String> {
    std::env::var(name)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "required environment missing"))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AccountRequest {
    did: String,
}

fn provision_account() -> io::Result<()> {
    let mut bytes = Vec::new();
    io::stdin().take(16_385).read_to_end(&mut bytes)?;
    if bytes.len() > 16_384 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "account request too large",
        ));
    }
    let request: AccountRequest = serde_json::from_slice(&bytes)?;
    let key = request
        .did
        .strip_prefix("did:layerx:")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid exported DID"))?;
    if key.len() != 64
        || !key
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || key.bytes().all(|b| b == b'0')
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid exported DID",
        ));
    }
    let account = layerx_types::account::AccountId::parse(&format!("agent:{}:main", request.did))
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid account"))?;
    let id = layerx_wire::hash::account_id_for_protocol(&account, 3)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid protocol account"))?;
    let account_hex: String = id
        .iter()
        .flat_map(|byte| {
            let digits = b"0123456789abcdef";
            [
                char::from(digits[usize::from(byte >> 4)]),
                char::from(digits[usize::from(byte & 15)]),
            ]
        })
        .collect();
    let mut output = io::stdout().lock();
    serde_json::to_writer(&mut output, &serde_json::json!({"account": account_hex}))?;
    output.write_all(b"\n")
}

fn run() -> io::Result<()> {
    let mut args = std::env::args().skip(1);
    let command = args.next();
    if args.next().is_some()
        || !matches!(
            command.as_deref(),
            None | Some("serve" | "bind-device" | "provision-owner" | "provision-account")
        )
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected serve, bind-device, provision-owner or provision-account",
        ));
    }
    if command.as_deref() == Some("provision-account") {
        return provision_account();
    }
    let policy = Policy::read(&PathBuf::from(required(
        "LAYERX_HUMAN_IDENTITY_PROVIDER_RECOVERY_POLICY_FILE",
    )?))?;
    let mut state = State::open(
        &PathBuf::from(required("LAYERX_HUMAN_IDENTITY_PROVIDER_STATE_ROOT")?),
        policy,
    )?;
    if command.as_deref() == Some("provision-owner") {
        return state.provision_owner(io::stdin().lock(), io::stdout().lock());
    }
    if command.as_deref() == Some("bind-device") {
        let mut bytes = Vec::new();
        io::stdin().take(16_385).read_to_end(&mut bytes)?;
        if bytes.len() > 16_384 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "enrollment too large",
            ));
        }
        let enrollment: Enrollment = serde_json::from_slice(&bytes)?;
        let principal = PrincipalId::new(enrollment.principal)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid principal"))?;
        return state.bind_device(&principal, &enrollment.assertion_id, enrollment.device);
    }
    let socket = PathBuf::from(required("LAYERX_HUMAN_IDENTITY_PROVIDER_SOCKET")?);
    let uid = required("LAYERX_HUMAN_IDENTITY_PROVIDER_ALLOWED_UID")?
        .parse::<u32>()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid allowed uid"))?;
    let deadline = match std::env::var("LAYERX_HUMAN_IDENTITY_PROVIDER_DEADLINE_SECONDS") {
        Ok(value) => value
            .parse::<u64>()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid deadline"))?,
        Err(std::env::VarError::NotPresent) => 5,
        Err(error) => return Err(io::Error::new(io::ErrorKind::InvalidInput, error)),
    };
    let shutdown = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&shutdown))?;
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&shutdown))?;
    Server::bind(&socket, state, uid, Duration::from_secs(deadline))?.run(&shutdown)
}

fn main() -> std::process::ExitCode {
    if run().is_ok() {
        std::process::ExitCode::SUCCESS
    } else {
        eprintln!("identity provider refused configuration, transport, or durable state");
        std::process::ExitCode::FAILURE
    }
}

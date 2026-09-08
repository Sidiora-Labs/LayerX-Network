#![forbid(unsafe_code)]

use std::io::{self, Read};
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

fn run() -> io::Result<()> {
    let mut args = std::env::args().skip(1);
    let command = args.next();
    if args.next().is_some()
        || !matches!(
            command.as_deref(),
            None | Some("serve" | "bind-device" | "provision-owner")
        )
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected serve, bind-device or provision-owner",
        ));
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

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use layerx_human_service::server::{
    ComponentServerConfig, HumanComponentServer, ProductionComponents, ProductionComponentsConfig,
};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("layerx-human-components refused startup: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let allowed_uid = required_number::<u32>("LAYERX_HUMAN_COMPONENT_ALLOWED_UID")?;
    if rustix::process::getuid().as_raw() != allowed_uid {
        return Err("the configured component UID does not own this process".to_owned());
    }
    let clock = layerx_client::runtime_clock::RuntimeClock::from_environment()
        .map_err(|error| error.to_string())?;
    let backend = Arc::new(ProductionComponents::open(
        ProductionComponentsConfig::from_environment()?,
        clock.clone(),
    )?);
    let recipient = layerx_human_service::server::production_components::RecipientServer::bind(
        Arc::clone(&backend), layerx_human_service::server::production_components::RecipientServerConfig {
            socket: PathBuf::from(required("LAYERX_HUMAN_RECIPIENT_SOCKET")?),
            caller_uid: required_number("LAYERX_HUMAN_RECIPIENT_CALLER_UID")?,
            caller_gid: required_number("LAYERX_HUMAN_RECIPIENT_CALLER_GID")?,
            deadline: Duration::from_secs(required_number("LAYERX_HUMAN_RECIPIENT_DEADLINE_SECONDS")?),
            clock: layerx_client::runtime_clock::RuntimeClock::from_environment()
                .map_err(|_| "the recipient clock authority is unavailable".to_owned())?,
        }).map_err(|_| "the recipient listener cannot bind".to_owned())?;
    let server = HumanComponentServer::new_maintained(
        backend,
        Duration::from_secs(required_number(
            "LAYERX_HUMAN_MAINTENANCE_INTERVAL_SECONDS",
        )?),
        required_number("LAYERX_HUMAN_MAINTENANCE_MAXIMUM_ITEMS")?,
        clock,
    )
    .map_err(|_| "the component maintenance policy is invalid".to_owned())?
    .bind(ComponentServerConfig {
        socket_path: PathBuf::from(required("LAYERX_HUMAN_COMPONENT_SOCKET")?),
        allowed_uid,
        worker_count: required_number("LAYERX_HUMAN_COMPONENT_WORKERS")?,
        queue_capacity: required_number("LAYERX_HUMAN_COMPONENT_QUEUE_CAPACITY")?,
        limits: layerx_human_service::server::default_component_limits(),
    })
    .map_err(|_| "the privileged component listener cannot bind".to_owned())?;
    let shutdown = server.shutdown();
    let recipient_shutdown = shutdown.clone();
    let worker = std::thread::spawn(move || {
        let result = recipient.run(&recipient_shutdown);
        recipient_shutdown.request();
        result
    });
    let result = server.run();
    shutdown.request();
    worker.join().map_err(|_| "the recipient listener panicked".to_owned())?
        .map_err(|_| "the recipient listener failed".to_owned())?;
    result.map_err(|_| "the privileged component listener failed".to_owned())
}

fn required(name: &str) -> Result<String, String> {
    env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{name} is required"))
}

fn required_number<T>(name: &str) -> Result<T, String>
where
    T: std::str::FromStr,
{
    required(name)?
        .parse::<T>()
        .map_err(|_| format!("{name} is invalid"))
}

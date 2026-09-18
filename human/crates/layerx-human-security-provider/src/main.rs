mod probe;

use layerx_human_security_provider::{ingest_recovery_receipt, serve, Config, Error, Result};
use std::path::PathBuf;
use std::sync::{atomic::AtomicBool, Arc};

fn run() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() == 2 && args[0] == "ingest-recovery-receipt" {
        let root = std::env::var_os("LAYERX_HUMAN_SECURITY_PROVIDER_STATE_ROOT")
            .map(PathBuf::from)
            .ok_or(Error::Configuration)?;
        let trust = std::env::var_os("LAYERX_HUMAN_SECURITY_PROVIDER_TRUST_HISTORY")
            .map(PathBuf::from)
            .ok_or(Error::Configuration)?;
        return ingest_recovery_receipt(&root, &trust, &PathBuf::from(&args[1]));
    }
    if args.len() == 1 && args[0] == "probe" {
        return probe::run();
    }
    if !args.is_empty() {
        return Err(Error::Configuration);
    }
    let stop = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, stop.clone())?;
    signal_hook::flag::register(signal_hook::consts::SIGINT, stop.clone())?;
    serve(
        Config::from_env()?,
        stop,
        layerx_client::runtime_clock::RuntimeClock::from_environment()
            .map_err(|_| Error::Configuration)?,
    )
}
fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}

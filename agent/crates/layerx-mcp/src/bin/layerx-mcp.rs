//! Daemon-bound model context protocol server for the LayerX agent plane.

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use layerx_mcp::binding::Binding;
use layerx_mcp::listener::Listener;

const USAGE: &str = "usage: layerx-mcp <absolute path to the daemon binding document>";

fn binding_path() -> Result<PathBuf, String> {
    let mut arguments = env::args_os().skip(1);
    let path = arguments.next().ok_or_else(|| USAGE.to_owned())?;
    if arguments.next().is_some() {
        return Err(USAGE.to_owned());
    }
    Ok(PathBuf::from(path))
}

fn run() -> Result<(), String> {
    let path = binding_path()?;
    let binding = Binding::open(&path).map_err(|error| error.detail())?;
    let configuration = binding
        .listener()
        .cloned()
        .ok_or_else(|| "the binding document declares no protocol socket".to_owned())?;
    let mut session = binding.open_session().map_err(|error| error.detail())?;
    let listener = Listener::bind(configuration)
        .map_err(|error| format!("the protocol socket was refused: {}", error.detail()))?;
    listener
        .serve(&mut session)
        .map_err(|error| format!("the protocol socket stopped: {}", error.detail()))
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("layerx-mcp: {error}");
            ExitCode::FAILURE
        }
    }
}

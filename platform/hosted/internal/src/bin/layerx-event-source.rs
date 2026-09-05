use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::process::ExitCode;

use layerx_platform_internal::{events, http, secret, tls};

fn run() -> Result<(), String> {
    let prefix = "LAYERX_EVENTS";
    let listen = secret::required_env("LAYERX_EVENTS_LISTEN")?
        .parse()
        .map_err(|_| "invalid LAYERX_EVENTS_LISTEN".to_owned())?;
    let kind = events::Kind::parse(&secret::required_env("LAYERX_EVENTS_KIND")?)?;
    if !tls::client_ca_configured(prefix) {
        return Err("LAYERX_EVENTS_CLIENT_CA_DER is required".to_owned());
    }
    let mut bytes = Vec::new();
    File::open(secret::required_env("LAYERX_EVENTS_CREDENTIALS_FILE")?)
        .and_then(|file| file.take(1_048_577).read_to_end(&mut bytes))
        .map_err(|error| error.to_string())?;
    if bytes.len() > 1_048_576 {
        return Err("credential map exceeds bound".to_owned());
    }
    let paths: BTreeMap<String, String> =
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    let credentials = paths
        .into_iter()
        .map(|(principal, path)| {
            secret::read_secret_file(Path::new(&path)).map(|credential| (principal, credential))
        })
        .collect::<Result<_, _>>()?;
    let state = secret::required_env("LAYERX_EVENTS_STATE_DIR")?;
    let service = events::Service::open(
        kind,
        tls::Upstream::from_environment(prefix)?,
        credentials,
        secret::read_token("LAYERX_EVENTS_TOKEN_FILE")?,
        Path::new(&state),
    )?;
    http::serve(
        "layerx-event-source",
        listen,
        &tls::server_config(prefix)?,
        move |request| service.route(request),
    )
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("layerx-event-source: {error}");
            ExitCode::FAILURE
        }
    }
}

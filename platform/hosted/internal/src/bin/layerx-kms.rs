use std::path::Path;
use std::process::ExitCode;

use layerx_platform_internal::{http, kms, seal::SealKey, secret, tls};

fn run() -> Result<(), String> {
    let prefix = "LAYERX_KMS";
    let listen = secret::required_env("LAYERX_KMS_LISTEN")?
        .parse()
        .map_err(|_| "invalid LAYERX_KMS_LISTEN".to_owned())?;
    let token = secret::read_token("LAYERX_KMS_TOKEN_FILE")?;
    let seal_secret = secret::read_token("LAYERX_KMS_SEAL_SECRET_FILE")?;
    let state = secret::required_env("LAYERX_KMS_STATE_DIR")?;
    if !tls::client_ca_configured(prefix) {
        return Err("LAYERX_KMS_CLIENT_CA_DER is required".to_owned());
    }
    let store = kms::KeyStore::open(
        Path::new(&state),
        SealKey::derive(kms::SEAL_LABEL, seal_secret.as_bytes()),
    )?;
    let service = kms::Service::new(store, token, true);
    http::serve(
        "layerx-kms",
        listen,
        &tls::server_config(prefix)?,
        move |request| service.route(request),
    )
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("layerx-kms: {error}");
            ExitCode::FAILURE
        }
    }
}

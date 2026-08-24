//! Executable entry point of the hosted program registry.

use std::env;
use std::net::TcpListener;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use layerx_platform_registry::{parse_request, refusal, write_response, Config, Registrar};
use layerx_programs::hex;

const DEFAULT_LISTEN: &str = "127.0.0.1:9420";
const DEFAULT_ROOT: &str = "/var/lib/layerx-program-registry";
const DEFAULT_PATH: &str = "/usr/local/bin:/usr/bin:/bin";

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|value| u64::try_from(value.as_millis()).ok())
        .unwrap_or(0)
}

fn parse_u64(name: &str, default: u64) -> Result<u64, String> {
    env::var(name).map_or(Ok(default), |value| {
        value
            .parse()
            .map_err(|_| format!("{name} must be an integer"))
    })
}

fn parse_u32(name: &str, default: u32) -> Result<u32, String> {
    env::var(name).map_or(Ok(default), |value| {
        value
            .parse()
            .map_err(|_| format!("{name} must be an integer"))
    })
}

fn parse_path(name: &str, default: PathBuf) -> PathBuf {
    env::var(name).map_or(default, PathBuf::from)
}

fn config() -> Result<Config, String> {
    let root = parse_path("LAYERX_REGISTRY_STATE", PathBuf::from(DEFAULT_ROOT));
    let digest = env::var("LAYERX_REGISTRY_BUILDER_IMAGE_DIGEST")
        .map_err(|_| "LAYERX_REGISTRY_BUILDER_IMAGE_DIGEST is required".to_owned())?;
    Ok(Config {
        listen: env::var("LAYERX_REGISTRY_LISTEN").unwrap_or_else(|_| DEFAULT_LISTEN.to_owned()),
        journal: parse_path("LAYERX_REGISTRY_JOURNAL", root.join("journal")),
        mirror: parse_path("LAYERX_REGISTRY_SOURCE_MIRROR", root.join("sources")),
        verified: parse_path("LAYERX_REGISTRY_VERIFIED", root.join("verified")),
        workspace: parse_path("LAYERX_REGISTRY_BUILD_ROOT", root.join("builds")),
        builder_image_digest: hex::decode_digest(&digest)
            .map_err(|error| format!("LAYERX_REGISTRY_BUILDER_IMAGE_DIGEST is invalid: {error}"))?,
        builder_path: env::var("LAYERX_REGISTRY_BUILDER_PATH")
            .unwrap_or_else(|_| DEFAULT_PATH.to_owned()),
        build_timeout_seconds: parse_u64("LAYERX_REGISTRY_BUILD_TIMEOUT_SECONDS", 1_800)?,
        attempts: parse_u32("LAYERX_REGISTRY_ATTEMPTS", 2)?,
        staleness_seconds: parse_u64("LAYERX_REGISTRY_MAX_STALENESS_SECONDS", 300)?
            .checked_mul(1_000)
            .ok_or_else(|| "LAYERX_REGISTRY_MAX_STALENESS_SECONDS is too large".to_owned())?,
        node_endpoint: env::var("LAYERX_REGISTRY_NODE_ENDPOINT")
            .map_err(|_| "LAYERX_REGISTRY_NODE_ENDPOINT is required".to_owned())?,
        node_authorization: env::var("LAYERX_REGISTRY_NODE_AUTHORIZATION")
            .map_err(|_| "LAYERX_REGISTRY_NODE_AUTHORIZATION is required".to_owned())?,
        receipt_authority_endpoint: env::var("LAYERX_REGISTRY_RECEIPT_AUTHORITY_ENDPOINT")
            .map_err(|_| "LAYERX_REGISTRY_RECEIPT_AUTHORITY_ENDPOINT is required".to_owned())?,
        receipt_authority_authorization: env::var(
            "LAYERX_REGISTRY_RECEIPT_AUTHORITY_AUTHORIZATION",
        )
        .map_err(|_| "LAYERX_REGISTRY_RECEIPT_AUTHORITY_AUTHORIZATION is required".to_owned())?,
        receipt_authority_replica_id: hex::decode_digest(
            &env::var("LAYERX_REGISTRY_RECEIPT_AUTHORITY_REPLICA_ID").map_err(|_| {
                "LAYERX_REGISTRY_RECEIPT_AUTHORITY_REPLICA_ID is required".to_owned()
            })?,
        )
        .map_err(|error| {
            format!("LAYERX_REGISTRY_RECEIPT_AUTHORITY_REPLICA_ID is invalid: {error}")
        })?,
        sequencer_id: hex::decode_digest(
            &env::var("LAYERX_REGISTRY_SEQUENCER_ID")
                .map_err(|_| "LAYERX_REGISTRY_SEQUENCER_ID is required".to_owned())?,
        )
        .map_err(|error| format!("LAYERX_REGISTRY_SEQUENCER_ID is invalid: {error}"))?,
        sequencer_public_key: hex::decode_digest(
            &env::var("LAYERX_REGISTRY_SEQUENCER_PUBLIC_KEY")
                .map_err(|_| "LAYERX_REGISTRY_SEQUENCER_PUBLIC_KEY is required".to_owned())?,
        )
        .map_err(|error| format!("LAYERX_REGISTRY_SEQUENCER_PUBLIC_KEY is invalid: {error}"))?,
        sequencer_first_batch: parse_u64("LAYERX_REGISTRY_SEQUENCER_FIRST_BATCH", 1)?,
        sequencer_last_batch: parse_u64("LAYERX_REGISTRY_SEQUENCER_LAST_BATCH", u64::MAX)?,
    })
}

fn serve(config: &Config) -> Result<(), String> {
    let mut registrar = Registrar::open(config, now())?;
    let listener = TcpListener::bind(&config.listen).map_err(|error| error.to_string())?;
    eprintln!(
        "LayerX program registry ready on {} with journal {} and source mirror {}",
        config.listen,
        config.journal.display(),
        config.mirror.display()
    );
    for connection in listener.incoming() {
        match connection {
            Ok(mut stream) => {
                let response = parse_request(&mut stream).map_or_else(
                    |_| refusal(400, "invalid_request", "request could not be parsed"),
                    |request| registrar.route(&request, now()),
                );
                let _ = write_response(&mut stream, &response);
            }
            Err(error) => eprintln!("program registry accept error: {error}"),
        }
    }
    Ok(())
}

fn main() {
    if let Err(error) = config().and_then(|config| serve(&config)) {
        eprintln!("layerx-program-registry: {error}");
        std::process::exit(2);
    }
}

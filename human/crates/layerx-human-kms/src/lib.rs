#![forbid(unsafe_code)]
mod config;
mod server;
mod store;
mod wire;

/// Runs the protected Human LXKP provider using its explicit environment configuration.
///
/// # Errors
/// Refuses invalid policy, TLS identity, protected storage or listener failure.
pub fn run_from_environment() -> Result<(), String> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| "TLS cryptographic provider already configured".to_owned())?;
    server::run(config::Config::load()?)
}

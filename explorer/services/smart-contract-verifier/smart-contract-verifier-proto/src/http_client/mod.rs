mod client;
mod config;

#[cfg(feature = "mock")]
pub mod mock;

pub use client::{
    health_client, solidity_verifier_client, sourcify_verifier_client, vyper_verifier_client,
    Client,
};
pub use config::Config;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// There was an error running some middleware
    #[error("Middleware error: {0}")]
    Middleware(#[from] anyhow::Error),
    /// Error from the underlying reqwest client
    #[error("Request error: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("Non-success status code: {}", .0.status())]
    StatusCode(reqwest::Response),
}

impl From<reqwest_middleware::Error> for Error {
    fn from(value: reqwest_middleware::Error) -> Error {
        match value {
            reqwest_middleware::Error::Middleware(err) => err.into(),
            reqwest_middleware::Error::Reqwest(err) => err.into(),
        }
    }
}

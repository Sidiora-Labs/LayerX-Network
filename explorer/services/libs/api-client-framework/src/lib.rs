//! Adapted from https://github.com/cloudflare/cloudflare-rs

mod async_client;
mod endpoint;

/// Blockscout services related structs.
/// Contains config and client definitions to be used by blockscout-rs services.
#[cfg(feature = "blockscout")]
pub mod blockscout;

pub use async_client::{HttpApiClient, HttpApiClientConfig};
pub use endpoint::{serialize_query, Endpoint};

pub use reqwest;
pub use reqwest_middleware;
pub use url;

/******************** Config definition ********************/

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("middleware error: {0}")]
    Middleware(anyhow::Error),
    #[error("request error: {0}")]
    Request(#[from] reqwest::Error),
    #[error("response deserialization failed: {0}")]
    Deserialization(#[from] serde_path_to_error::Error<serde_json::Error>),
    #[error("request returned with invalid status code: 404 - Not Found")]
    NotFound,
    #[error("request returned with invalid status code: {status_code} - {message}")]
    InvalidStatusCode {
        status_code: reqwest::StatusCode,
        message: String,
    },
    #[error("{0:#?}")]
    CustomError(anyhow::Error),
}

impl From<reqwest_middleware::Error> for Error {
    fn from(value: reqwest_middleware::Error) -> Self {
        match value {
            reqwest_middleware::Error::Middleware(error) => Error::Middleware(error),
            reqwest_middleware::Error::Reqwest(error) => Error::Request(error),
        }
    }
}

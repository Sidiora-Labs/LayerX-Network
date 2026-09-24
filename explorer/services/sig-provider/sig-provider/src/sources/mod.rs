pub mod eth_bytecode_db;
pub mod fourbyte;
pub mod sigeth;

use async_trait::async_trait;
use mockall::automock;
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{policies::ExponentialBackoff, RetryTransientMiddleware};
use std::time::Duration;

fn hash(hex: &str) -> String {
    if hex.starts_with("0x") {
        hex.to_owned()
    } else {
        "0x".to_owned() + hex
    }
}

#[automock]
#[async_trait]
pub trait SignatureSource {
    async fn create_signatures(&self, abi: &str) -> Result<(), anyhow::Error>;
    // Resulting signatures should be sorted in priority descending order (first - max priority)
    async fn get_function_signatures(&self, hex: &str) -> Result<Vec<String>, anyhow::Error>;
    // Resulting signatures should be sorted in priority descending order (first - max priority)
    async fn get_event_signatures(&self, hex: &str) -> Result<Vec<String>, anyhow::Error>;

    async fn batch_get_event_signatures(
        &self,
        hex: &[String],
    ) -> Result<Vec<Vec<String>>, anyhow::Error> {
        Ok(vec![vec![]; hex.len()])
    }

    // for errors
    fn source(&self) -> String;
}

#[automock]
#[async_trait]
pub trait CompleteSignatureSource {
    async fn get_event_signatures(
        &self,
        hex: &str,
    ) -> Result<Vec<alloy_json_abi::Event>, anyhow::Error>;

    async fn batch_get_event_signatures(
        &self,
        hex: &[String],
    ) -> Result<Vec<Vec<alloy_json_abi::Event>>, anyhow::Error> {
        Ok(vec![vec![]; hex.len()])
    }

    // for errors
    fn source(&self) -> String;
}

pub fn new_client() -> ClientWithMiddleware {
    let retry_policy = ExponentialBackoff::builder().build_with_max_retries(1);
    ClientBuilder::new(
        reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap(),
    )
    .with(RetryTransientMiddleware::new_with_policy(retry_policy))
    .build()
}

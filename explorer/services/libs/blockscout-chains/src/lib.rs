use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{policies::ExponentialBackoff, RetryTransientMiddleware};
use serde::Deserialize;
use std::collections::HashMap;

const CHAINS_URL: &str = "https://chains.blockscout.com/api/chains";

pub struct BlockscoutChainsClient {
    client: ClientWithMiddleware,
    url: String,
}

impl BlockscoutChainsClient {
    pub fn builder() -> BlockscoutChainsClientBuilder {
        Default::default()
    }

    pub async fn fetch_all(&self) -> Result<BlockscoutChains, reqwest_middleware::Error> {
        let res = self.client.get(&self.url).send().await?;
        let chains: BlockscoutChains = res.json().await?;
        Ok(chains)
    }

    /// Fetch a single chain by its ID from the Blockscout Chains API.
    pub async fn fetch_one_by_id(
        &self,
        chain_id: &str,
    ) -> Result<BlockscoutChainData, reqwest_middleware::Error> {
        let url = format!("{}/{}", self.url.trim_end_matches('/'), chain_id);
        let res = self.client.get(&url).send().await?;
        let chain: BlockscoutChainData = res.json().await?;
        Ok(chain)
    }
}

impl Default for BlockscoutChainsClient {
    fn default() -> Self {
        Self::builder().build()
    }
}

pub struct BlockscoutChainsClientBuilder {
    max_retries: u32,
    url: String,
}

impl BlockscoutChainsClientBuilder {
    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    pub fn with_url(mut self, url: String) -> Self {
        self.url = url;
        self
    }

    pub fn build(self) -> BlockscoutChainsClient {
        let retry_policy = ExponentialBackoff::builder().build_with_max_retries(self.max_retries);
        let client = ClientBuilder::new(reqwest::Client::new())
            .with(RetryTransientMiddleware::new_with_policy(retry_policy))
            .build();
        BlockscoutChainsClient {
            client,
            url: self.url,
        }
    }
}

impl Default for BlockscoutChainsClientBuilder {
    fn default() -> Self {
        Self {
            url: CHAINS_URL.to_string(),
            max_retries: 3,
        }
    }
}

pub type BlockscoutChains = HashMap<String, BlockscoutChainData>;

#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BlockscoutChainData {
    pub name: String,
    pub description: String,
    pub ecosystem: Ecosystem,
    pub is_testnet: Option<bool>,
    pub layer: Option<u8>,
    pub rollup_type: Option<String>,
    pub website: String,
    pub explorers: Vec<ExplorerConfig>,
    pub logo: String,
}

#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(untagged)]
pub enum Ecosystem {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExplorerConfig {
    pub url: String,
    pub hosted_by: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_blockscout_chains() {
        let chains = BlockscoutChainsClient::builder()
            .with_max_retries(0)
            .build()
            .fetch_all()
            .await
            .unwrap();
        assert!(!chains.is_empty());
    }

    #[tokio::test]
    async fn test_get_specific_blockscout_chain() {
        let client = BlockscoutChainsClient::builder()
            .with_max_retries(0)
            .build();
        // Chain ID "1" is Ethereum Mainnet and should always exist
        let chain = client.fetch_one_by_id("1").await.unwrap();
        assert_eq!(chain.name, "Ethereum");
        assert!(chain.website.contains("ethereum.org"));
    }
}

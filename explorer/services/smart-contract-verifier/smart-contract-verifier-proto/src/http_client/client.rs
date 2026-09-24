use super::{config, Error, Result};
use crate::blockscout::smart_contract_verifier::v2 as proto;

#[derive(Clone, Debug)]
pub struct Client {
    base_url: url::Url,
    request_client: reqwest_middleware::ClientWithMiddleware,
}

impl Client {
    pub async fn new(config: config::Config) -> Self {
        let config = match config::ValidatedConfig::try_from(config) {
            Ok(config) => config,
            Err(err) => panic!("Invalid client configuration: {err}"),
        };

        let request_client = {
            let client = reqwest::Client::new();
            let mut client_with_middleware = reqwest_middleware::ClientBuilder::new(client);
            for middleware in config.middleware_stack {
                client_with_middleware = client_with_middleware.with_arc(middleware);
            }
            client_with_middleware.build()
        };

        let client = Self {
            base_url: config.url,
            request_client,
        };

        if config.probe_url {
            if let Err(err) = health_client::health(&client, Default::default()).await {
                panic!("Cannot establish a connection with smart-contract-verifier client: {err}")
            }
        }

        client
    }

    fn build_url(&self, path: &str) -> url::Url {
        self.build_url_with_query(path, None)
    }

    fn build_url_with_query(&self, path: &str, query: Option<&str>) -> url::Url {
        let mut url = self.base_url.clone();
        url.set_path(path);
        url.set_query(query);
        url
    }

    async fn post_request<U, Request, Response>(
        &self,
        url: U,
        request: &Request,
    ) -> Result<Response>
    where
        U: reqwest::IntoUrl,
        Request: serde::Serialize + ?Sized,
        Response: serde::de::DeserializeOwned,
    {
        let response = self.request_client.post(url).json(&request).send().await;
        Self::process_response(response).await
    }

    async fn get_request<U, Response>(&self, url: U) -> Result<Response>
    where
        U: reqwest::IntoUrl,
        Response: serde::de::DeserializeOwned,
    {
        let response = self.request_client.get(url).send().await;
        Self::process_response(response).await
    }

    async fn process_response<Response>(
        response: reqwest_middleware::Result<reqwest::Response>,
    ) -> Result<Response>
    where
        Response: serde::de::DeserializeOwned,
    {
        match response {
            Ok(response) if response.status().is_success() => Ok(response.json().await?),
            Ok(response) => Err(Error::StatusCode(response)),
            Err(err) => Err(err.into()),
        }
    }
}

pub mod solidity_verifier_client {
    use super::{proto, Client, Result};

    pub async fn verify_multi_part(
        client: &Client,
        request: proto::VerifySolidityMultiPartRequest,
    ) -> Result<proto::VerifyResponse> {
        let path = "/api/v2/verifier/solidity/sources:verify-multi-part";
        client.post_request(client.build_url(path), &request).await
    }

    pub async fn verify_standard_json(
        client: &Client,
        request: proto::VerifySolidityStandardJsonRequest,
    ) -> Result<proto::VerifyResponse> {
        let path = "/api/v2/verifier/solidity/sources:verify-standard-json";
        client.post_request(client.build_url(path), &request).await
    }

    pub async fn batch_verify_multi_part(
        client: &Client,
        request: proto::BatchVerifySolidityMultiPartRequest,
    ) -> Result<proto::BatchVerifyResponse> {
        let path = "/api/v2/verifier/solidity/sources:batch-verify-multi-part";
        client.post_request(client.build_url(path), &request).await
    }

    pub async fn batch_verify_standard_json(
        client: &Client,
        request: proto::BatchVerifySolidityStandardJsonRequest,
    ) -> Result<proto::BatchVerifyResponse> {
        let path = "/api/v2/verifier/solidity/sources:batch-verify-standard-json";
        client.post_request(client.build_url(path), &request).await
    }

    pub async fn list_compiler_versions(
        client: &Client,
        _request: proto::ListCompilerVersionsRequest,
    ) -> Result<proto::ListCompilerVersionsResponse> {
        let path = "/api/v2/verifier/solidity/versions";
        client.get_request(client.build_url(path)).await
    }

    pub async fn lookup_methods(
        client: &Client,
        request: proto::LookupMethodsRequest,
    ) -> Result<proto::LookupMethodsResponse> {
        let path = "/api/v2/verifier/solidity/methods:lookup";
        client.post_request(client.build_url(path), &request).await
    }
}

pub mod vyper_verifier_client {
    use super::{proto, Client, Result};

    pub async fn verify_multi_part(
        client: &Client,
        request: proto::VerifyVyperMultiPartRequest,
    ) -> Result<proto::VerifyResponse> {
        let path = "/api/v2/verifier/vyper/sources:verify-multi-part";
        client.post_request(client.build_url(path), &request).await
    }

    pub async fn verify_standard_json(
        client: &Client,
        request: proto::VerifyVyperStandardJsonRequest,
    ) -> Result<proto::VerifyResponse> {
        let path = "/api/v2/verifier/vyper/sources:verify-standard-json";
        client.post_request(client.build_url(path), &request).await
    }

    pub async fn list_compiler_versions(
        client: &Client,
        _request: proto::ListCompilerVersionsRequest,
    ) -> Result<proto::ListCompilerVersionsResponse> {
        let path = "/api/v2/verifier/vyper/versions";
        client.get_request(client.build_url(path)).await
    }
}

pub mod sourcify_verifier_client {
    use super::{proto, Client, Result};

    pub async fn verify(
        client: &Client,
        request: proto::VerifySourcifyRequest,
    ) -> Result<proto::VerifyResponse> {
        let path = "/api/v2/verifier/sourcify/sources:verify";
        client.post_request(client.build_url(path), &request).await
    }

    pub async fn verify_from_etherscan(
        client: &Client,
        request: proto::VerifyFromEtherscanSourcifyRequest,
    ) -> Result<proto::VerifyResponse> {
        let path = "/api/v2/verifier/sourcify/sources:verify-from-etherscan";
        client.post_request(client.build_url(path), &request).await
    }
}

pub mod health_client {
    use super::{proto, Client, Result};

    pub async fn health(
        client: &Client,
        request: proto::HealthCheckRequest,
    ) -> Result<proto::HealthCheckResponse> {
        let path = "/health";
        let query = Some(format!("service={}", request.service));
        client
            .get_request(client.build_url_with_query(path, query.as_deref()))
            .await
    }
}

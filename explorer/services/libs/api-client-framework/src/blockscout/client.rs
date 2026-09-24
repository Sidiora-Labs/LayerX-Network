use super::config;
use crate::{Endpoint, Error, HttpApiClient, HttpApiClientConfig};
use reqwest::header::HeaderValue;

pub struct Client {
    http_client: HttpApiClient,
    api_key: Option<HeaderValue>,
}

impl Client {
    pub async fn new(config: config::Config) -> Self {
        let http_client_config = HttpApiClientConfig {
            http_timeout: config.http_timeout,
            default_headers: Default::default(),
            middlewares: config.middlewares,
        };

        let http_client = HttpApiClient::new(config.url, http_client_config)
            .unwrap_or_else(|err| panic!("cannot build an http client: {err:#?}"));

        let client = Self {
            http_client,
            api_key: config.api_key,
        };

        if config.probe_url {
            let endpoint = health::HealthCheck::new(Default::default());
            if let Err(err) = client.request(&endpoint).await {
                panic!("Cannot establish a connection with contracts-info client: {err}")
            }
        }

        client
    }

    pub async fn request<EndpointType: Endpoint>(
        &self,
        endpoint: &EndpointType,
    ) -> Result<<EndpointType as Endpoint>::Response, Error> {
        self.http_client.request(endpoint).await
    }

    pub fn api_key(&self) -> Option<&HeaderValue> {
        self.api_key.as_ref()
    }
}

/// As we don't have protobuf generated structures here (they are only available inside a service),
/// we have to imitate the service health endpoint.
mod health {
    use crate::{serialize_query, Endpoint};
    use reqwest::Method;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Default, Serialize)]
    pub struct HealthCheckRequest {
        pub service: String,
    }

    #[derive(Debug, Deserialize)]
    pub struct HealthCheckResponse {
        #[serde(rename = "status")]
        pub _status: i32,
    }

    pub struct HealthCheck {
        request: HealthCheckRequest,
    }

    impl HealthCheck {
        pub fn new(request: HealthCheckRequest) -> Self {
            Self { request }
        }
    }

    impl Endpoint for HealthCheck {
        type Response = HealthCheckResponse;

        fn method(&self) -> Method {
            Method::GET
        }

        fn path(&self) -> String {
            "/health".to_string()
        }

        fn query(&self) -> Option<String> {
            serialize_query(&self.request)
        }
    }
}

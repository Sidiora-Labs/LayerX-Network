use super::endpoint::Endpoint;
use crate::Error;
use reqwest::{header::HeaderMap, Response, StatusCode};
use reqwest_middleware::{ClientBuilder, Middleware};
use serde::Deserialize;
use std::{sync::Arc, time::Duration};

#[derive(Clone)]
pub struct HttpApiClientConfig {
    /// The maximum time limit for an API request. If a request takes longer than this, it will be
    /// cancelled.
    pub http_timeout: Duration,
    /// A default set of HTTP headers which will be sent with each API request.
    pub default_headers: HeaderMap,
    /// Middlewares that will process each API request before the request is actually sent.
    pub middlewares: Vec<Arc<dyn Middleware>>,
}

impl Default for HttpApiClientConfig {
    fn default() -> Self {
        Self {
            http_timeout: Duration::from_secs(30),
            default_headers: HeaderMap::default(),
            middlewares: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct HttpApiClient {
    base_url: url::Url,
    http_client: reqwest_middleware::ClientWithMiddleware,
}

impl HttpApiClient {
    pub fn new(base_url: url::Url, config: HttpApiClientConfig) -> Result<Self, Error> {
        let reqwest_client = reqwest::Client::builder()
            .default_headers(config.default_headers)
            .timeout(config.http_timeout)
            .build()?;
        let mut client_builder = ClientBuilder::new(reqwest_client);
        for middleware in config.middlewares {
            client_builder = client_builder.with_arc(middleware);
        }
        let client = client_builder.build();

        Ok(Self {
            base_url,
            http_client: client,
        })
    }

    /// Issue an API request of the given type.
    pub async fn request<EndpointType: Endpoint>(
        &self,
        endpoint: &EndpointType,
    ) -> Result<<EndpointType as Endpoint>::Response, Error> {
        // Build the request
        let mut request = self
            .http_client
            .request(endpoint.method(), endpoint.url(&self.base_url));

        if let Some(body) = endpoint.body() {
            request = request.body(body);
            request = request.header(
                reqwest::header::CONTENT_TYPE,
                endpoint.content_type().as_ref(),
            );
        }

        if let Some(headers) = endpoint.headers() {
            request = request.headers(headers);
        }

        let response = request.send().await?;
        process_api_response(response).await
    }
}

async fn process_api_response<T: for<'a> Deserialize<'a>>(response: Response) -> Result<T, Error> {
    let status = response.status();
    match status {
        status if status.is_success() => (),
        StatusCode::NOT_FOUND => return Err(Error::NotFound),
        status => {
            return Err(Error::InvalidStatusCode {
                status_code: status,
                message: response.text().await?,
            })
        }
    }

    let raw_value = response.bytes().await?;
    let deserializer = &mut serde_json::Deserializer::from_slice(raw_value.as_ref());
    let value: T = serde_path_to_error::deserialize(deserializer)?;
    Ok(value)
}

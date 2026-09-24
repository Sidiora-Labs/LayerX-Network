use crate::{
    types::{
        CustomError, EmptyCustomError, ErrorResponse, GetSourceFilesResponse,
        VerifyFromEtherscanResponse,
    },
    Error, SourcifyError, VerifyFromEtherscanError,
};
use blockscout_display_bytes::ToHex;
use bytes::Bytes;
use reqwest::{Response, StatusCode};
use reqwest_middleware::{ClientWithMiddleware, Middleware};
use reqwest_retry::{policies::ExponentialBackoff, RetryTransientMiddleware};
use serde::{Deserialize, Serialize};
use std::{str::FromStr, sync::Arc};
use url::Url;

mod retryable_strategy {
    use reqwest::StatusCode;
    use reqwest_middleware::Error;
    use reqwest_retry::{Retryable, RetryableStrategy};

    pub struct SourcifyRetryableStrategy;

    impl RetryableStrategy for SourcifyRetryableStrategy {
        fn handle(&self, res: &Result<reqwest::Response, Error>) -> Option<Retryable> {
            match res {
                Ok(success) => default_on_request_success(success),
                Err(error) => reqwest_retry::default_on_request_failure(error),
            }
        }
    }

    // The strategy differs from `reqwest_retry::default_on_request_success`
    // by considering 500 errors as Fatal instead of Transient.
    // The reason is that Sourcify uses 500 code to propagate fatal internal errors,
    // which will not be resolved on retry and which we would like to get early to process.
    fn default_on_request_success(success: &reqwest::Response) -> Option<Retryable> {
        let status = success.status();
        if status.is_server_error() && status != StatusCode::INTERNAL_SERVER_ERROR {
            Some(Retryable::Transient)
        } else if status.is_success() {
            None
        } else if status == StatusCode::REQUEST_TIMEOUT || status == StatusCode::TOO_MANY_REQUESTS {
            Some(Retryable::Transient)
        } else {
            Some(Retryable::Fatal)
        }
    }
}

#[derive(Clone)]
pub struct ClientBuilder {
    base_url: Url,
    max_retries: u32,
    middleware_stack: Vec<Arc<dyn Middleware>>,
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self {
            base_url: Url::from_str("https://sourcify.dev/server/").unwrap(),
            max_retries: 3,
            middleware_stack: vec![],
        }
    }
}

impl ClientBuilder {
    pub fn try_base_url(mut self, base_url: &str) -> Result<Self, String> {
        let base_url = Url::from_str(base_url).map_err(|err| err.to_string())?;
        self.base_url = base_url;

        Ok(self)
    }

    pub fn max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    pub fn with_middleware<M: Middleware>(self, middleware: M) -> Self {
        self.with_arc_middleware(Arc::new(middleware))
    }

    pub fn with_arc_middleware<M: Middleware>(mut self, middleware: Arc<M>) -> Self {
        self.middleware_stack.push(middleware);
        self
    }

    pub fn build(self) -> Client {
        let retry_policy = ExponentialBackoff::builder().build_with_max_retries(self.max_retries);
        let mut client_builder = reqwest_middleware::ClientBuilder::new(reqwest::Client::new())
            .with(RetryTransientMiddleware::new_with_policy_and_strategy(
                retry_policy,
                retryable_strategy::SourcifyRetryableStrategy,
            ));
        for middleware in self.middleware_stack {
            client_builder = client_builder.with_arc(middleware);
        }
        let client = client_builder.build();

        Client {
            base_url: self.base_url,
            reqwest_client: client,
        }
    }
}

#[derive(Clone)]
pub struct Client {
    base_url: Url,
    reqwest_client: ClientWithMiddleware,
}

impl Default for Client {
    /// Initializes [`Client`] with base url set to "https://sourcify.dev/server/",
    /// and total duration to 60 seconds.
    fn default() -> Self {
        ClientBuilder::default().build()
    }
}

impl Client {
    pub async fn get_source_files_any(
        &self,
        chain_id: &str,
        contract_address: Bytes,
    ) -> Result<GetSourceFilesResponse, Error<EmptyCustomError>> {
        let url = self.generate_url(
            format!(
                "files/any/{}/{}",
                chain_id,
                ToHex::to_hex(&contract_address)
            )
            .as_str(),
        );

        let response = self
            .reqwest_client
            .get(url)
            .send()
            .await
            .map_err(|error| match error {
                reqwest_middleware::Error::Middleware(err) => Error::ReqwestMiddleware(err),
                reqwest_middleware::Error::Reqwest(err) => Error::Reqwest(err),
            })?;

        Self::process_sourcify_response(response).await
    }

    pub async fn verify_from_etherscan(
        &self,
        chain_id: &str,
        contract_address: Bytes,
    ) -> Result<VerifyFromEtherscanResponse, Error<VerifyFromEtherscanError>> {
        let url = self.generate_url("verify/etherscan");

        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Request<'a> {
            chain_id: &'a str,
            address: String,
        }

        let request = Request {
            chain_id,
            address: ToHex::to_hex(&contract_address),
        };

        let response = self
            .reqwest_client
            .post(url)
            .json(&request)
            .send()
            .await
            .map_err(|error| match error {
                reqwest_middleware::Error::Middleware(err) => Error::ReqwestMiddleware(err),
                reqwest_middleware::Error::Reqwest(err) => Error::Reqwest(err),
            })?;

        Self::process_sourcify_response(response).await
    }
}

impl Client {
    fn generate_url(&self, route: &str) -> Url {
        self.base_url.join(route).unwrap()
    }

    async fn process_sourcify_response<T: for<'de> Deserialize<'de>, E: CustomError>(
        response: Response,
    ) -> Result<T, Error<E>> {
        let error_message = |response: Response| async {
            response
                .json::<ErrorResponse>()
                .await
                .map(|value| value.error)
        };

        match response.status() {
            StatusCode::OK => Ok(response.json::<T>().await?),
            StatusCode::NOT_FOUND => {
                let message = error_message(response).await?;
                if let Some(err) = E::handle_not_found(&message) {
                    Err(Error::Sourcify(SourcifyError::Custom(err)))
                } else {
                    Err(Error::Sourcify(SourcifyError::NotFound(message)))
                }
            }
            StatusCode::BAD_REQUEST => {
                let message = error_message(response).await?;
                if let Some(err) = E::handle_bad_request(&message) {
                    Err(Error::Sourcify(SourcifyError::Custom(err)))
                } else {
                    Err(Error::Sourcify(SourcifyError::BadRequest(message)))
                }
            }
            StatusCode::BAD_GATEWAY => {
                let message = error_message(response).await?;
                if let Some(err) = E::handle_bad_gateway(&message) {
                    Err(Error::Sourcify(SourcifyError::Custom(err)))
                } else {
                    Err(Error::Sourcify(SourcifyError::BadGateway(message)))
                }
            }
            StatusCode::INTERNAL_SERVER_ERROR => {
                let message = error_message(response).await?;
                if let Some(err) = E::handle_internal_server_error(&message) {
                    Err(Error::Sourcify(SourcifyError::Custom(err)))
                } else {
                    // For now the only way to recognize that the chain is not supported by Sourcify.
                    // Message example: "Chain 134135 is not a Sourcify chain!"
                    if message.contains("is not a Sourcify chain") {
                        Err(Error::Sourcify(SourcifyError::ChainNotSupported(message)))
                    } else {
                        Err(Error::Sourcify(SourcifyError::InternalServerError(message)))
                    }
                }
            }
            status_code => {
                let text = response.text().await?;
                if let Some(err) = E::handle_status_code(status_code, &text) {
                    Err(Error::Sourcify(SourcifyError::Custom(err)))
                } else {
                    Err(Error::Sourcify(SourcifyError::UnexpectedStatusCode {
                        status_code,
                        msg: text,
                    }))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blockscout_display_bytes::decode_hex;
    use governor::{
        clock::DefaultClock,
        middleware::NoOpMiddleware,
        state::{InMemoryState, NotKeyed},
        Quota, RateLimiter,
    };
    use once_cell::sync::OnceCell;
    use reqwest_rate_limiter::RateLimiterMiddleware;
    use serde_json::json;
    use std::num::NonZeroU32;

    fn parse_contract_address(contract_address: &str) -> Bytes {
        decode_hex(contract_address).unwrap().into()
    }

    static RATE_LIMITER_MIDDLEWARE: OnceCell<
        Arc<RateLimiterMiddleware<NotKeyed, InMemoryState, DefaultClock, NoOpMiddleware>>,
    > = OnceCell::new();
    fn rate_limiter_middleware(
    ) -> &'static Arc<RateLimiterMiddleware<NotKeyed, InMemoryState, DefaultClock, NoOpMiddleware>>
    {
        RATE_LIMITER_MIDDLEWARE.get_or_init(|| {
            let max_burst = NonZeroU32::new(1).unwrap();
            Arc::new(RateLimiterMiddleware::new(RateLimiter::direct(
                Quota::per_second(max_burst),
            )))
        })
    }

    fn client() -> Client {
        let rate_limiter = rate_limiter_middleware().clone();
        ClientBuilder::default()
            .try_base_url("https://staging.sourcify.dev/server/")
            .unwrap()
            .max_retries(3)
            .with_arc_middleware(rate_limiter)
            .build()
    }

    mod success {
        use super::*;
        use pretty_assertions::assert_eq;

        #[tokio::test]
        async fn get_source_files_any() {
            let expected: GetSourceFilesResponse = serde_json::from_value(json!({
            "status": "full",
            "files": [
                {
                    "name": "Example.sol",
                    "path": "contracts/full_match/11155111/0x4E7095a3519A33dF3D25774c2F9D7a89eB99745D/sources/contracts/Example.sol",
                    "content": "library Lib {\n    function sum(uint256 a, uint256 b) external returns (uint256) {\n        return a + b;\n    }\n}\n\ncontract A {\n    function sum(uint256 a, uint256 b) external returns (uint256) {\n        return Lib.sum(a, b);\n    }\n}\n"
                },
                {
                    "name": "metadata.json",
                    "path": "contracts/full_match/11155111/0x4E7095a3519A33dF3D25774c2F9D7a89eB99745D/metadata.json",
                    "content": "{\"compiler\":{\"version\":\"0.8.20+commit.a1b79de6\"},\"language\":\"Solidity\",\"output\":{\"abi\":[{\"inputs\":[{\"internalType\":\"uint256\",\"name\":\"a\",\"type\":\"uint256\"},{\"internalType\":\"uint256\",\"name\":\"b\",\"type\":\"uint256\"}],\"name\":\"sum\",\"outputs\":[{\"internalType\":\"uint256\",\"name\":\"\",\"type\":\"uint256\"}],\"stateMutability\":\"nonpayable\",\"type\":\"function\"}],\"devdoc\":{\"kind\":\"dev\",\"methods\":{},\"version\":1},\"userdoc\":{\"kind\":\"user\",\"methods\":{},\"version\":1}},\"settings\":{\"compilationTarget\":{\"contracts/Example.sol\":\"A\"},\"evmVersion\":\"istanbul\",\"libraries\":{},\"metadata\":{\"bytecodeHash\":\"ipfs\"},\"optimizer\":{\"enabled\":false,\"runs\":200},\"remappings\":[]},\"sources\":{\"contracts/Example.sol\":{\"keccak256\":\"0x74f0b08e915377a73ed19a56ae3bfce73f4a75c2b9c76ce3c450c2e3f35ad730\",\"urls\":[\"bzz-raw://57df7d0de4fd2c829d638021ea6ed9845fe04c6ad68ed9bd516ca82295ffbfea\",\"dweb:/ipfs/QmR8QjtWYAW2RuyvZVKJMNS3Wp38rRQP33FxviuRdf1Vek\"]}},\"version\":1}"
                },
                {
                    "name": "creator-tx-hash.txt",
                    "path": "contracts/full_match/11155111/0x4E7095a3519A33dF3D25774c2F9D7a89eB99745D/creator-tx-hash.txt",
                    "content": "0x4b511e8d9bcd56407bc348631d04a673b39c859a036e5cd49df7526a8de29b93"
                },
                {
                    "name": "library-map.json",
                    "path": "contracts/full_match/11155111/0x4E7095a3519A33dF3D25774c2F9D7a89eB99745D/library-map.json",
                    "content": "{\"__$50698f9fab9190debff1c0247749d3c3d0$__\":\"0xf145e3a26c6706f64d95dc8d9d45022d8b3d676b\"}"
                }
            ]
        })).unwrap();

            let chain_id = "11155111";
            let contract_address =
                parse_contract_address("0x4E7095a3519A33dF3D25774c2F9D7a89eB99745D");

            let result = client()
                .get_source_files_any(chain_id, contract_address)
                .await
                .expect("success expected");
            assert_eq!(expected, result);
        }

        // TODO: returns "directory already has entry by that name" internal error
        // #[tokio::test]
        // async fn verify_from_etherscan() {
        //     let expected: VerifyFromEtherscanResponse = serde_json::from_value(json!({
        //     "result": [
        //         {
        //             "address": "0x831b003398106153eD89a758bEC9734667D18AeC",
        //             "chainId": "10",
        //             "status": "partial",
        //             "libraryMap": {
        //                 "__$5762d9689e001ee319dd424b89cc702f5c$__": "9224ee604e9b62f8e0a0e5824fee2e0df2ca902f"
        //             },
        //             "immutableReferences": {"2155":[{"length":32,"start":4157},{"length":32,"start":4712}],"2157":[{"length":32,"start":1172},{"length":32,"start":1221},{"length":32,"start":1289},{"length":32,"start":2077},{"length":32,"start":4218},{"length":32,"start":5837}],"2159":[{"length":32,"start":742},{"length":32,"start":4943}],"2161":[{"length":32,"start":402},{"length":32,"start":3247},{"length":32,"start":5564}]}
        //         }
        //     ]
        // })).unwrap();
        //
        //     let chain_id = "10";
        //     let contract_address =
        //         parse_contract_address("0x831b003398106153eD89a758bEC9734667D18AeC");
        //
        //     let result = client()
        //         .verify_from_etherscan(chain_id, contract_address)
        //         .await
        //         .expect("success expected");
        //     assert_eq!(expected, result);
        // }
    }

    mod not_found {
        use super::*;

        #[tokio::test]
        async fn get_source_files_any() {
            let chain_id = "11155111";
            let contract_address =
                parse_contract_address("0x847F2d0c193E90963aAD7B2791aAE8d7310dFF6A");

            let result = client()
                .get_source_files_any(chain_id, contract_address)
                .await
                .expect_err("error expected");
            assert!(
                matches!(result, Error::Sourcify(SourcifyError::NotFound(_))),
                "expected: 'SourcifyError::NotFound', got: {result:?}"
            );
        }

        /*
        * Not implemented, as custom error that 'contract is not verified on Etherscan' is returned instead.

        * async fn verify_from_etherscan() {}
        */
    }

    mod bad_request {
        use super::*;

        #[tokio::test]
        async fn get_source_files_any_invalid_argument() {
            let chain_id = "11155111";
            let contract_address = parse_contract_address("0xcafecafecafecafe");

            let result = client()
                .get_source_files_any(chain_id, contract_address)
                .await
                .expect_err("error expected");
            assert!(
                matches!(result, Error::Sourcify(SourcifyError::BadRequest(_))),
                "expected: 'SourcifyError::BadRequest', got: {result:?}"
            );
        }

        #[tokio::test]
        async fn verify_from_etherscan_invalid_argument() {
            let chain_id = "11155111";
            let contract_address = parse_contract_address("0xcafecafecafecafe");

            let result = client()
                .verify_from_etherscan(chain_id, contract_address)
                .await
                .expect_err("error expected");
            assert!(
                matches!(result, Error::Sourcify(SourcifyError::BadRequest(_))),
                "expected: 'SourcifyError::BadRequest', got: {result:?}"
            );
        }
    }

    mod chain_not_supported {
        use super::*;

        #[tokio::test]
        async fn get_source_files_any() {
            let chain_id = "12345";
            let contract_address =
                parse_contract_address("0x027f1fe8BbC2a7E9fE97868E82c6Ec6939086c52");

            let result = client()
                .get_source_files_any(chain_id, contract_address)
                .await
                .expect_err("error expected");
            assert!(
                matches!(result, Error::Sourcify(SourcifyError::ChainNotSupported(_))),
                "expected: 'SourcifyError::ChainNotSupported', got: {result:?}"
            );
        }

        /*
        * Not implemented, as custom error that 'chain is not supported for verification on Etherscan' is returned instead.

        * async fn verify_from_etherscan() {}
        */
    }

    mod custom_errors {
        use super::*;

        #[tokio::test]
        async fn verify_from_etherscan_chain_is_not_supported() {
            let chain_id = "2221";
            let contract_address =
                parse_contract_address("0xcb566e3B6934Fa77258d68ea18E931fa75e1aaAa");

            let result = client()
                .verify_from_etherscan(chain_id, contract_address)
                .await
                .expect_err("error expected");
            assert!(
                matches!(
                    result,
                    Error::Sourcify(SourcifyError::Custom(
                        VerifyFromEtherscanError::ChainNotSupported(_)
                    ))
                ),
                "expected: 'SourcifyError::ChainNotSupported', got: {result:?}"
            );
        }

        #[tokio::test]
        async fn verify_from_etherscan_contract_not_verified() {
            let chain_id = "11155111";
            let contract_address =
                parse_contract_address("0xa4E5DF47af3Cf0746DF6337E3F45286887e86680");

            let result = client()
                .verify_from_etherscan(chain_id, contract_address)
                .await
                .expect_err("error expected");
            assert!(
                matches!(
                    &result,
                    Error::Sourcify(SourcifyError::NotFound(message))
                    if message.contains("not verified on Etherscan")
                ),
                "expected: 'SourcifyError::NotFound with not verified on etherscan message', got: {result:?}"
            );
        }

        /*
        * Not implemented to avoid unnecessary burden on the Sourcify server.

        * async fn verify_from_etherscan_too_many_request() {}
        */

        /*
        * Not implemented as could not find any contract for which the error is returned.
        * We need to add the implementation when such contract is found.

        * async fn verify_from_etherscan_api_response_error() {}
        */

        /*
        * Not implemented as could not find any contract for which the error is returned.
        * We need to add the implementation when such contract is found.

        * async fn verify_from_etherscan_cannot_generate_solc_json_input() {}
        */

        /*
        * Not implemented as could not find any contract for which the error is returned.
        * We need to add the implementation when such contract is found.

        * async fn verify_from_etherscan_verified_with_errors() {}
        */
    }
}

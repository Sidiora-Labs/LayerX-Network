use crate::SignatureSource;
use reqwest_middleware::ClientWithMiddleware;

pub struct Source {
    host: url::Url,
    client: ClientWithMiddleware,
    n_retries: usize,
}

impl Source {
    pub fn new(host: url::Url) -> Source {
        Source {
            host,
            client: super::new_client(),
            n_retries: 3,
        }
    }

    #[cfg(test)]
    pub fn n_retries(mut self, n_retries: usize) -> Source {
        self.n_retries = n_retries;
        self
    }

    async fn fetch(&self, mut path: String) -> Result<Vec<String>, anyhow::Error> {
        let mut signatures = Vec::default();

        loop {
            let resp: json::GetResponse = self.try_make_request(&path, self.n_retries).await?;
            signatures.extend(resp.results.into_iter().map(|sig| sig.text_signature));
            if let Some(next) = resp.next {
                path = next;
            } else {
                break;
            }
        }
        // TODO: sort using "id" field
        Ok(signatures)
    }

    #[async_recursion::async_recursion]
    async fn try_make_request(&self, path: &str, n: usize) -> anyhow::Result<json::GetResponse> {
        let response = self
            .client
            .get(self.host.join(path).unwrap())
            .send()
            .await
            .map_err(anyhow::Error::msg)?;
        match response.status() {
            reqwest::StatusCode::OK => Ok(response.json::<json::GetResponse>().await?),
            reqwest::StatusCode::BAD_GATEWAY if n > 0 => self.try_make_request(path, n - 1).await,
            status => Err(anyhow::anyhow!(
                "invalid status code got as a result: {}",
                status
            )),
        }
    }
}

#[async_trait::async_trait]
impl SignatureSource for Source {
    async fn create_signatures(&self, abi: &str) -> Result<(), anyhow::Error> {
        self.client
            .post(self.host.join("/api/v1/import-solidity/").unwrap())
            .json(&json::CreateRequest { contract_abi: abi })
            .send()
            .await
            .map(|_| ())
            .map_err(anyhow::Error::msg)
    }

    async fn get_function_signatures(&self, hex: &str) -> Result<Vec<String>, anyhow::Error> {
        self.fetch(format!("/api/v1/signatures/?hex_signature={hex}"))
            .await
    }

    async fn get_event_signatures(&self, hex: &str) -> Result<Vec<String>, anyhow::Error> {
        self.fetch(format!("/api/v1/event-signatures/?hex_signature={hex}"))
            .await
    }

    fn source(&self) -> String {
        self.host.to_string()
    }
}

mod json {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize)]
    pub struct CreateRequest<'a> {
        pub contract_abi: &'a str,
    }

    #[derive(Debug, Deserialize)]
    pub struct Signature {
        pub text_signature: String,
    }

    #[derive(Debug, Deserialize)]
    pub struct GetResponse {
        pub next: Option<String>,
        pub results: Vec<Signature>,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    const DEFAULT_HOST: &str = "https://www.4byte.directory/";

    #[rstest::fixture]
    fn source() -> Source {
        let host = url::Url::from_str(DEFAULT_HOST).expect("default host is not an url");
        Source::new(host).n_retries(6) // We increase n_retries to avoid most blinking tests
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn create(source: Source) {
        let abi = r#"[{"constant":false,"inputs":[],"name":"f","outputs":[],"type":"function"},{"inputs":[],"type":"constructor"},{"anonymous":false,"inputs":[{"name":"","type":"string","indexed":true}],"name":"E","type":"event"}]"#;
        source
            .create_signatures(abi)
            .await
            .expect("error while submitting a new signature");
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn get_function_signatures(source: Source) {
        let (signature, hex) = ("f()", "0x26121ff0");
        let result = source
            .get_function_signatures(hex)
            .await
            .expect("error while getting function signature");
        assert!(result.contains(&signature.into()))
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn get_event_signatures(source: Source) {
        let (signature, hex) = (
            "E(string)",
            "0x3e9992c940c54ea252d3a34557cc3d3014281525c43d694f89d5f3dfd820b07d",
        );
        let result = source
            .get_event_signatures(hex)
            .await
            .expect("error while getting event signature");
        assert!(result.contains(&signature.into()))
    }
}

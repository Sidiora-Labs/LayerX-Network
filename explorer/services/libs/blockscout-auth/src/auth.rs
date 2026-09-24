use crate::{
    consts,
    jwt_headers::{build_http_headers, extract_csrf_token, extract_jwt},
};
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use thiserror::Error;
use tonic::metadata::MetadataMap;
use url::Url;

#[derive(Debug, Clone, Deserialize)]
pub struct AuthSuccess {
    pub avatar: Option<String>,
    pub email: String,
    pub id: i64,
    pub name: Option<String>,
    pub nickname: Option<String>,
    pub uid: String,
    pub watchlist_id: i64,
    pub email_verified: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct AuthFailed {
    pub message: String,
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("blockscout API error: {0}")]
    BlockscoutApi(String),

    #[error("unauthorized: {0}")]
    Unauthorized(String),

    #[error("forbidden: {0}")]
    Forbidden(String),

    #[error("invalid jwt token: {0}")]
    InvalidJwt(String),

    #[error("invalid csrf token: {0}")]
    InvalidCsrfToken(String),

    #[error("cannot build headers: {0}")]
    HeaderError(String),
}

pub async fn auth_from_metadata(
    metadata: &MetadataMap,
    is_safe_http_method: bool,
    blockscout_host: &Url,
    blockscout_api_key: Option<&str>,
) -> Result<AuthSuccess, Error> {
    let jwt = extract_jwt(metadata)?;
    let csrf_opt = if is_safe_http_method {
        None
    } else {
        Some(extract_csrf_token(metadata)?)
    };
    auth_from_tokens(
        &jwt,
        csrf_opt.as_deref(),
        blockscout_host,
        blockscout_api_key,
    )
    .await
}

pub async fn auth_from_tokens(
    jwt: &str,
    csrf_token: Option<&str>,
    blockscout_host: &Url,
    blockscout_api_key: Option<&str>,
) -> Result<AuthSuccess, Error> {
    let mut url = blockscout_host
        .join("/api/account/v2/authenticate")
        .expect("invalid base url");
    url.set_query(
        blockscout_api_key
            .map(|api_key| format!("{}={api_key}", consts::API_KEY_NAME))
            .as_deref(),
    );
    let headers = build_http_headers(jwt, csrf_token, blockscout_api_key)?;
    let client = Client::new();
    let response = if csrf_token.is_some() {
        let body = build_request_body(blockscout_api_key);
        client.post(url).json(&body)
    } else {
        client.get(url)
    }
    .headers(headers)
    .send()
    .await
    .map_err(|e| Error::BlockscoutApi(e.to_string()))?;

    let status = response.status();
    let response_raw = response
        .text()
        .await
        .map_err(|e| Error::BlockscoutApi(e.to_string()))?;

    match status {
        StatusCode::OK => {
            let success: AuthSuccess = serde_json::from_str(&response_raw).map_err(|error| {
                tracing::warn!(
                    error = ?error,
                    body = ?response_raw,
                    "failed to parse blockscout response body"
                );
                Error::BlockscoutApi(format!("invalid body: {error}"))
            })?;
            Ok(success)
        }
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            let failed: AuthFailed = serde_json::from_str(&response_raw).map_err(|error| {
                tracing::warn!(
                    error = ?error,
                    body = ?response_raw,
                    "failed to parse blockscout response body"
                );
                Error::BlockscoutApi(format!("invalid body: {error}"))
            })?;
            if status == StatusCode::UNAUTHORIZED {
                Err(Error::Unauthorized(failed.message))
            } else {
                Err(Error::Forbidden(failed.message))
            }
        }
        _ => Err(Error::BlockscoutApi(format!("unexpected status {status}"))),
    }
}

pub fn build_request_body(blockscout_api_key: Option<&str>) -> serde_json::Value {
    serde_json::json!({
        consts::API_KEY_NAME: blockscout_api_key,
    })
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use crate::{consts, init_mocked_blockscout_auth_service, MockUser};
    use reqwest::header::{HeaderMap, HeaderName};
    use serde::Serialize;
    use tonic::{codegen::http::header::CONTENT_TYPE, Extensions, Request};

    fn build_headers(jwt: &str, csrf_token: Option<&str>, in_cookie: bool) -> HeaderMap {
        use tonic::codegen::http::header::COOKIE;
        let mut headers = HeaderMap::new();
        if in_cookie {
            let cookies = format!(
                "intercom-id-gsgyurk3=2380c963-677d-4899-b130-01b29609f8ca; \
                intercom-session-gsgyurk3=; intercom-device-id-gsgyurk3=2fa296b4-a133-4922-b754-e3a5e446bb8e; \
                chakra-ui-color-mode=light; __cuid=0a2ad6cf04a343c0812f65aff55f0f56; \
                amp_fef1e8=3f4a1e5a-ca9c-4092-9b66-0705e0e44a21R...1gqhd6tvp.1gqhd6tvq.4.1.5; \
                adblock_detected=true; indexing_alert=false; _explorer_key={jwt}"
            );
            headers.insert(COOKIE, cookies.parse().unwrap());
        } else {
            headers.insert(consts::HEADER_JWT_TOKEN_NAME, jwt.parse().unwrap());
        };
        headers.insert(CONTENT_TYPE, "application/json".parse().unwrap());
        if let Some(csrf_token) = csrf_token {
            headers.insert(
                HeaderName::from_lowercase(b"x-csrf-token").unwrap(),
                csrf_token.parse().unwrap(),
            );
        };
        headers
    }

    fn build_request<T: Serialize>(jwt: &str, csrf_token: Option<&str>, data: T) -> Request<T> {
        let meta =
            tonic::metadata::MetadataMap::from_headers(build_headers(jwt, csrf_token, false));
        Request::from_parts(meta, Extensions::default(), data)
    }

    #[test]
    fn extract_jwt_works() {
        let jwt = "VALID_JWT_TOKEN";
        let meta = tonic::metadata::MetadataMap::from_headers(build_headers(jwt, None, true));

        let cookie_token = extract_jwt(&meta).expect("failed to extract token from cookie");
        assert_eq!(cookie_token, jwt);

        let meta = tonic::metadata::MetadataMap::from_headers(build_headers(jwt, None, false));

        let header_token = extract_jwt(&meta).expect("failed to extract token from header");
        assert_eq!(header_token, jwt);
    }

    #[test]
    fn extract_csrf_token_works() {
        let csrf_token = "VALID_CSRF_TOKEN";
        let meta =
            tonic::metadata::MetadataMap::from_headers(build_headers("", Some(csrf_token), true));

        let token = extract_csrf_token(&meta).expect("failed to extract metadata");
        assert_eq!(token, csrf_token);
    }

    #[derive(Debug, Serialize)]
    struct GetBody {}

    #[derive(Debug, Serialize)]
    struct PostBody {
        name: String,
    }

    #[tokio::test]
    async fn auth_works() {
        let users = [MockUser {
            id: 1,
            email: "user@gmail.com".into(),
            chain_id: 1,
            jwt: "jwt1".into(),
            csrf_token: "csrf1".into(),
        }];
        let blockscout_api_key = Some("somekey");
        let blockscout = init_mocked_blockscout_auth_service(blockscout_api_key, &users).await;
        let blockscout_host = Url::from_str(&blockscout.uri()).unwrap();

        let request = build_request("jwt1", None, GetBody {});
        let success = auth_from_metadata(
            request.metadata(),
            true,
            &blockscout_host,
            blockscout_api_key,
        )
        .await
        .expect("failed to auth get request");
        assert_eq!(success.id, 1);

        let request = build_request(
            "jwt1",
            Some("csrf1"),
            PostBody {
                name: "lev".to_string(),
            },
        );
        let success = auth_from_metadata(
            request.metadata(),
            false,
            &blockscout_host,
            blockscout_api_key,
        )
        .await
        .expect("failed to auth post request");
        assert_eq!(success.id, 1);

        let request = build_request(
            "jwt1",
            None,
            PostBody {
                name: "lev".to_string(),
            },
        );
        auth_from_metadata(
            request.metadata(),
            false,
            &blockscout_host,
            blockscout_api_key,
        )
        .await
        .expect_err("success response from request without csrf_token");

        let request = Request::new(GetBody {});
        auth_from_metadata(
            request.metadata(),
            true,
            &blockscout_host,
            blockscout_api_key,
        )
        .await
        .expect_err("success response for empty request");
    }

    #[tokio::test]
    async fn auth_works_no_api_key() {
        let users = [MockUser {
            id: 1,
            email: "user@gmail.com".into(),
            chain_id: 1,
            jwt: "jwt1".into(),
            csrf_token: "csrf1".into(),
        }];
        let blockscout_api_key = None;
        let blockscout = init_mocked_blockscout_auth_service(blockscout_api_key, &users).await;
        let blockscout_host = Url::from_str(&blockscout.uri()).unwrap();

        let request = build_request("jwt1", None, GetBody {});
        let success = auth_from_metadata(
            request.metadata(),
            true,
            &blockscout_host,
            blockscout_api_key,
        )
        .await
        .expect("failed to auth get request without api_key");
        assert_eq!(success.id, 1);
    }
}

use crate::{
    error::Error,
    types::{GoogleVerifyRequest, VerifyRequest, VerifyResponse},
};
use reqwest::Client;

const GOOGLE_RECAPTCHA_VERIFY_URL: &str = "https://www.google.com/recaptcha/api/siteverify";
/// Default minimum score threshold for v3
const DEFAULT_MIN_SCORE: f32 = 0.5;

#[derive(Debug, Clone)]
pub struct RecaptchaClient {
    secret_key: String,
    http_client: Client,
    verify_url: String,
}

impl RecaptchaClient {
    pub fn new(secret_key: impl Into<String>) -> Self {
        Self::with_http_client(secret_key, Client::new())
    }

    pub fn with_http_client(secret_key: impl Into<String>, http_client: Client) -> Self {
        Self {
            secret_key: secret_key.into(),
            http_client,
            verify_url: GOOGLE_RECAPTCHA_VERIFY_URL.to_string(),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_verify_url(mut self, url: impl Into<String>) -> Self {
        self.verify_url = url.into();
        self
    }

    pub async fn verify(&self, request: VerifyRequest<'_>) -> Result<VerifyResponse, Error> {
        let google_request = GoogleVerifyRequest {
            secret: &self.secret_key,
            response: request.response_token,
        };

        let response = self
            .http_client
            .post(&self.verify_url)
            .form(&google_request)
            .send()
            .await?;

        let verify_response: VerifyResponse = response.json().await?;

        // Check for API errors
        if !verify_response.success {
            return Err(Error::VerificationFailed(verify_response.error_codes));
        }

        // Check hostname if expected
        if let Some(expected_hostname) = request.expected_hostname
            && let Some(ref actual_hostname) = verify_response.hostname
            && actual_hostname != expected_hostname
        {
            return Err(Error::HostnameMismatch {
                expected: expected_hostname.to_string(),
                actual: actual_hostname.clone(),
            });
        }

        // For v3 responses, check score and action
        if let Some(score) = verify_response.score {
            let min_score = request.min_score.unwrap_or(DEFAULT_MIN_SCORE);
            if score < min_score {
                return Err(Error::ScoreTooLow {
                    score,
                    threshold: min_score,
                });
            }

            // Check action if expected
            if let Some(expected_action) = request.expected_action
                && let Some(ref actual_action) = verify_response.action
                && actual_action != expected_action
            {
                return Err(Error::ActionMismatch {
                    expected: expected_action.to_string(),
                    actual: actual_action.clone(),
                });
            }
        }

        Ok(verify_response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    async fn mock_client(response: serde_json::Value) -> (MockServer, RecaptchaClient) {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/recaptcha/api/siteverify"))
            .respond_with(ResponseTemplate::new(200).set_body_json(response))
            .mount(&server)
            .await;
        let client = RecaptchaClient::new("test-secret")
            .with_verify_url(format!("{}/recaptcha/api/siteverify", server.uri()));

        (server, client)
    }

    fn v2_response(hostname: &str) -> serde_json::Value {
        serde_json::json!({
            "success": true,
            "challenge_ts": "2026-01-21T10:00:00Z",
            "hostname": hostname
        })
    }

    fn v3_response(hostname: &str, score: f32, action: &str) -> serde_json::Value {
        serde_json::json!({
            "success": true,
            "challenge_ts": "2026-01-21T10:00:00Z",
            "hostname": hostname,
            "score": score,
            "action": action
        })
    }

    #[tokio::test]
    async fn verify_v2_success() {
        let (_, client) = mock_client(v2_response("example.com")).await;
        let response = client.verify(VerifyRequest::new("token")).await.unwrap();
        assert!(response.success);
        assert!(!response.is_v3());
    }

    #[tokio::test]
    async fn verify_v3_success() {
        let (_, client) = mock_client(v3_response("example.com", 0.9, "login")).await;
        let response = client
            .verify(
                VerifyRequest::new("token")
                    .with_expected_action("login")
                    .with_min_score(0.7),
            )
            .await
            .unwrap();
        assert!(response.is_v3());
        assert_eq!(response.score(), Some(0.9));
    }

    #[tokio::test]
    async fn verify_v3_score_too_low() {
        let (_, client) = mock_client(v3_response("example.com", 0.3, "login")).await;
        let err = client
            .verify(VerifyRequest::new("token").with_min_score(0.5))
            .await
            .unwrap_err();
        assert!(
            matches!(err, Error::ScoreTooLow { score, threshold } if score == 0.3 && threshold == 0.5)
        );
    }

    #[tokio::test]
    async fn verify_action_mismatch() {
        let (_, client) = mock_client(v3_response("example.com", 0.9, "signup")).await;
        let err = client
            .verify(VerifyRequest::new("token").with_expected_action("login"))
            .await
            .unwrap_err();
        assert!(
            matches!(err, Error::ActionMismatch { expected, actual } if expected == "login" && actual == "signup")
        );
    }

    #[tokio::test]
    async fn verify_hostname_mismatch() {
        let (_, client) = mock_client(v2_response("malicious.com")).await;
        let err = client
            .verify(VerifyRequest::new("token").with_expected_hostname("example.com"))
            .await
            .unwrap_err();
        assert!(
            matches!(err, Error::HostnameMismatch { expected, actual } if expected == "example.com" && actual == "malicious.com")
        );
    }

    #[tokio::test]
    async fn verify_failed_with_error_codes() {
        let (_, client) = mock_client(serde_json::json!({
            "success": false,
            "error-codes": ["invalid-input-response", "timeout-or-duplicate"]
        }))
        .await;
        let err = client
            .verify(VerifyRequest::new("token"))
            .await
            .unwrap_err();
        assert!(matches!(err, Error::VerificationFailed(codes) if codes.len() == 2));
    }
}

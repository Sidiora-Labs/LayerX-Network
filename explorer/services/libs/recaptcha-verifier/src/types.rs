use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct VerifyRequest<'a> {
    pub response_token: &'a str,
    pub expected_hostname: Option<&'a str>,
    /// Expected action name (v3 only, optional)
    pub expected_action: Option<&'a str>,
    /// Minimum acceptable score (v3 only, defaults to 0.5)
    pub min_score: Option<f32>,
}

impl<'a> VerifyRequest<'a> {
    pub fn new(response_token: &'a str) -> Self {
        Self {
            response_token,
            expected_hostname: None,
            expected_action: None,
            min_score: None,
        }
    }

    pub fn with_expected_hostname(mut self, hostname: &'a str) -> Self {
        self.expected_hostname = Some(hostname);
        self
    }

    /// Set the expected action (for v3)
    pub fn with_expected_action(mut self, action: &'a str) -> Self {
        self.expected_action = Some(action);
        self
    }

    /// Set the minimum acceptable score (for v3)
    pub fn with_min_score(mut self, min_score: f32) -> Self {
        self.min_score = Some(min_score);
        self
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct GoogleVerifyRequest<'a> {
    pub secret: &'a str,
    pub response: &'a str,
}

/// Response from Google's reCAPTCHA verification API
#[derive(Debug, Clone, Deserialize)]
pub struct VerifyResponse {
    pub success: bool,
    /// Timestamp of the challenge load (ISO format)
    #[serde(default)]
    pub challenge_ts: Option<String>,
    #[serde(default)]
    pub hostname: Option<String>,
    /// The score for this request (v3 only, 0.0 - 1.0)
    #[serde(default)]
    pub score: Option<f32>,
    /// The action name for this request (v3 only)
    #[serde(default)]
    pub action: Option<String>,
    /// Error codes if verification failed
    #[serde(default, rename = "error-codes")]
    pub error_codes: Vec<String>,
}

impl VerifyResponse {
    pub fn is_v3(&self) -> bool {
        self.score.is_some()
    }

    pub fn score(&self) -> Option<f32> {
        self.score
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_v2_response() {
        let json = r#"{
            "success": true,
            "challenge_ts": "2024-01-15T10:00:00Z",
            "hostname": "example.com"
        }"#;

        let response: VerifyResponse = serde_json::from_str(json).unwrap();
        assert!(response.success);
        assert!(!response.is_v3());
        assert_eq!(response.hostname, Some("example.com".to_string()));
    }

    #[test]
    fn deserialize_v3_response() {
        let json = r#"{
            "success": true,
            "challenge_ts": "2024-01-15T10:00:00Z",
            "hostname": "example.com",
            "score": 0.9,
            "action": "login"
        }"#;

        let response: VerifyResponse = serde_json::from_str(json).unwrap();
        assert!(response.success);
        assert!(response.is_v3());
        assert_eq!(response.score(), Some(0.9));
        assert_eq!(response.action, Some("login".to_string()));
    }

    #[test]
    fn deserialize_error_response() {
        let json = r#"{
            "success": false,
            "error-codes": ["invalid-input-response", "timeout-or-duplicate"]
        }"#;

        let response: VerifyResponse = serde_json::from_str(json).unwrap();
        assert!(!response.success);
        assert_eq!(response.error_codes.len(), 2);
    }
}

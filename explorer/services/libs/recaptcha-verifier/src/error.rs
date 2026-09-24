use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("HTTP request failed: {0}")]
    HttpRequest(#[from] reqwest::Error),

    #[error("reCAPTCHA verification failed: {}", .0.join(", "))]
    VerificationFailed(Vec<String>),

    #[error("reCAPTCHA score {score} is below threshold {threshold}")]
    ScoreTooLow { score: f32, threshold: f32 },

    #[error("reCAPTCHA action mismatch: expected '{expected}', got '{actual}'")]
    ActionMismatch { expected: String, actual: String },

    #[error("hostname mismatch: expected '{expected}', got '{actual}'")]
    HostnameMismatch { expected: String, actual: String },

    #[error("reCAPTCHA token not found in header '{header}'")]
    MissingToken { header: String },
}

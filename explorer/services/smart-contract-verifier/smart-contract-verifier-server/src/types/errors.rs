use thiserror::Error;
#[derive(Error, Debug)]
pub enum StandardJsonParseError {
    #[error("content is not a valid standard json: {0}")]
    InvalidContent(#[from] serde_json::Error),
    #[error("{0}")]
    BadRequest(#[from] anyhow::Error),
}

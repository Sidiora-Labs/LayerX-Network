//!
//! The `solc --standard-json` output error source location.
//!

use std::str::FromStr;

use serde::{Deserialize, Serialize};

///
/// The `solc --standard-json` output error source location.
///
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SourceLocation {
    /// The source file path.
    pub file: String,
    /// The start location.
    pub start: isize,
    /// The end location.
    pub end: isize,
}

impl FromStr for SourceLocation {
    type Err = anyhow::Error;

    fn from_str(string: &str) -> Result<Self, Self::Err> {
        let mut parts = string.split(':');
        let start = parts
            .next()
            .map(|string| string.parse::<isize>())
            .and_then(Result::ok)
            .unwrap_or_default();
        let length = parts
            .next()
            .map(|string| string.parse::<isize>())
            .and_then(Result::ok)
            .unwrap_or_default();
        let file = parts.next().unwrap_or_default().to_owned();

        Ok(Self {
            file,
            start,
            end: start + length,
        })
    }
}

//!
//! The `solc --standard-json` output source.
//!

use serde::{Deserialize, Serialize};

///
/// The `solc --standard-json` output source.
///
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    /// The source code ID.
    pub id: usize,
    /// The source code AST.
    pub ast: Option<serde_json::Value>,
}

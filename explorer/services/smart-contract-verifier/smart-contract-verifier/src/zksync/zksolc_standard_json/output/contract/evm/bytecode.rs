//!
//! The `solc --standard-json` output contract EVM bytecode.
//!

use serde::{Deserialize, Serialize};

///
/// The `solc --standard-json` output contract EVM bytecode.
///
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Bytecode {
    /// The bytecode object.
    pub object: String,
}

impl Bytecode {
    ///
    /// A shortcut constructor.
    ///
    pub fn new(object: String) -> Self {
        Self { object }
    }
}

//!
//! The `solc --standard-json` input language.
//!

use serde::{Deserialize, Serialize};

///
/// The `solc --standard-json` input language.
///
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    /// The Solidity language.
    Solidity,
    /// The Yul IR.
    Yul,
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Solidity => write!(f, "Solidity"),
            Self::Yul => write!(f, "Yul"),
        }
    }
}

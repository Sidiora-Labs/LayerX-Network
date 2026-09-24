//!
//! The `solc --standard-json` output selection.
//!

pub mod file;

use serde::{Deserialize, Serialize};

use self::file::File as FileSelection;

///
/// The `solc --standard-json` output selection.
///
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Selection {
    /// Only the 'all' wildcard is available for robustness reasons.
    #[serde(rename = "*", skip_serializing_if = "Option::is_none")]
    pub all: Option<FileSelection>,
}

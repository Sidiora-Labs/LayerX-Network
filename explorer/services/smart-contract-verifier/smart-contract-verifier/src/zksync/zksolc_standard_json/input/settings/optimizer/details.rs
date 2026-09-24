//!
//! The `solc --standard-json` input settings optimizer details.
//!

use serde::{Deserialize, Serialize};

///
/// The `solc --standard-json` input settings optimizer details.
///
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Details {
    /// Whether the pass is enabled.
    pub peephole: bool,
    /// Whether the pass is enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inliner: Option<bool>,
    /// Whether the pass is enabled.
    pub jumpdest_remover: bool,
    /// Whether the pass is enabled.
    pub order_literals: bool,
    /// Whether the pass is enabled.
    pub deduplicate: bool,
    /// Whether the pass is enabled.
    pub cse: bool,
    /// Whether the pass is enabled.
    pub constant_optimizer: bool,
}

impl Details {
    ///
    /// A shortcut constructor.
    ///
    pub fn new(
        peephole: bool,
        inliner: Option<bool>,
        jumpdest_remover: bool,
        order_literals: bool,
        deduplicate: bool,
        cse: bool,
        constant_optimizer: bool,
    ) -> Self {
        Self {
            peephole,
            inliner,
            jumpdest_remover,
            order_literals,
            deduplicate,
            cse,
            constant_optimizer,
        }
    }

    ///
    /// Creates a set of disabled optimizations.
    ///
    pub fn disabled(version: &semver::Version) -> Self {
        let inliner = if version >= &semver::Version::new(0, 8, 5) {
            Some(false)
        } else {
            None
        };

        Self::new(false, inliner, false, false, false, false, false)
    }
}

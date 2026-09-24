//!
//! The `solc --standard-json` input settings optimizer.
//!

pub mod details;

use serde::{Deserialize, Serialize};

use self::details::Details;

///
/// The `solc --standard-json` input settings optimizer.
///
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Optimizer {
    /// Whether the optimizer is enabled.
    pub enabled: bool,
    /// The optimization mode string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<char>,
    /// The `solc` optimizer details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Details>,
    /// Whether to try to recompile with -Oz if the bytecode is too large.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_to_optimizing_for_size: Option<bool>,
    /// Whether to disable the system request memoization.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disable_system_request_memoization: Option<bool>,
    /// Set the jump table density threshold.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jump_table_density_threshold: Option<u32>,
    // The original structure contained `camelCase` modifier for all optimizer fields.
    // But sometimes those parameters are supplied in a snake case.
    // To support such cases, we also add explicit snake case variations.
    /// Whether to try to recompile with -Oz if the bytecode is too large.
    #[serde(
        rename = "fallback_to_optimizing_for_size",
        skip_serializing_if = "Option::is_none"
    )]
    pub fallback_to_optimizing_for_size_snake: Option<bool>,
    /// Whether to disable the system request memoization.
    #[serde(
        rename = "disable_system_request_memoization",
        skip_serializing_if = "Option::is_none"
    )]
    pub disable_system_request_memoization_snake: Option<bool>,
    /// Set the jump table density threshold.
    #[serde(
        rename = "jump_table_density_threshold",
        skip_serializing_if = "Option::is_none"
    )]
    pub jump_table_density_threshold_snake: Option<u32>,
}

impl Optimizer {
    ///
    /// A shortcut constructor.
    ///
    pub fn new(
        enabled: bool,
        mode: Option<char>,
        version: &semver::Version,
        fallback_to_optimizing_for_size: bool,
        disable_system_request_memoization: bool,
        jump_table_density_threshold: Option<u32>,
    ) -> Self {
        Self {
            enabled,
            mode,
            details: Some(Details::disabled(version)),
            fallback_to_optimizing_for_size: Some(fallback_to_optimizing_for_size),
            disable_system_request_memoization: Some(disable_system_request_memoization),
            jump_table_density_threshold,
            fallback_to_optimizing_for_size_snake: Some(fallback_to_optimizing_for_size),
            disable_system_request_memoization_snake: Some(disable_system_request_memoization),
            jump_table_density_threshold_snake: jump_table_density_threshold,
        }
    }

    ///
    /// Sets the necessary defaults.
    ///
    pub fn normalize(&mut self, version: &semver::Version) {
        self.details = if version >= &semver::Version::new(0, 5, 5) {
            Some(Details::disabled(version))
        } else {
            None
        };
    }
}

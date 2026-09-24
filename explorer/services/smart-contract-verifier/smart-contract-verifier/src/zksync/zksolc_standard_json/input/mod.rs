//!
//! The `solc --standard-json` input.
//!

pub mod language;
pub mod settings;
pub mod source;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use self::{language::Language, settings::Settings, source::Source};

///
/// The `solc --standard-json` input.
///
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input {
    /// The input language.
    pub language: Language,
    /// The input source code files hashmap.
    pub sources: BTreeMap<String, Source>,
    /// The compiler settings.
    pub settings: Settings,
}

impl Input {
    ///
    /// Sets the necessary defaults.
    ///
    pub fn normalize(&mut self, version: &semver::Version) {
        self.settings.normalize(version);
    }
}

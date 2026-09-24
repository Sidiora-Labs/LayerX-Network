//!
//! The `solc --standard-json` input settings.
//!

pub mod metadata;
pub mod optimizer;
pub mod selection;

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use self::{metadata::Metadata, optimizer::Optimizer, selection::Selection};

///
/// The `solc --standard-json` input settings.
///
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// The target EVM version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evm_version: Option<era_compiler_common::EVMVersion>,
    /// The linker library addresses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub libraries: Option<BTreeMap<String, BTreeMap<String, String>>>,
    /// The sorted list of remappings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remappings: Option<BTreeSet<String>>,
    /// The output selection filters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_selection: Option<Selection>,
    /// Whether to compile via EVM assembly.
    #[serde(rename = "forceEVMLA", skip_serializing_if = "Option::is_none")]
    pub force_evmla: Option<bool>,
    /// Whether to add the Yul step to compilation via EVM assembly.
    #[serde(rename = "viaIR", skip_serializing_if = "Option::is_none")]
    pub via_ir: Option<bool>,
    /// Whether to enable EraVM extensions.
    #[serde(
        rename = "enableEraVMExtensions",
        skip_serializing_if = "Option::is_none"
    )]
    pub enable_eravm_extensions: Option<bool>,
    /// Whether to enable the missing libraries detection mode.
    #[serde(
        rename = "detectMissingLibraries",
        skip_serializing_if = "Option::is_none"
    )]
    pub detect_missing_libraries: Option<bool>,
    /// The optimizer settings.
    pub optimizer: Optimizer,
    /// The extra LLVM options.
    #[serde(rename = "LLVMOptions", skip_serializing_if = "Option::is_none")]
    pub llvm_options: Option<Vec<String>>,
    /// The metadata settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Metadata>,
}

impl Settings {
    ///
    /// Sets the necessary defaults.
    ///
    pub fn normalize(&mut self, version: &semver::Version) {
        self.optimizer.normalize(version);
    }

    ///
    /// Parses the library list and returns their double hashmap with path and name as keys.
    ///
    pub fn parse_libraries(
        input: Vec<String>,
    ) -> anyhow::Result<BTreeMap<String, BTreeMap<String, String>>> {
        let mut libraries = BTreeMap::new();
        for (index, library) in input.into_iter().enumerate() {
            let mut path_and_address = library.split('=');
            let path = path_and_address
                .next()
                .ok_or_else(|| anyhow::anyhow!("The library #{} path is missing", index))?;
            let mut file_and_contract = path.split(':');
            let file = file_and_contract
                .next()
                .ok_or_else(|| anyhow::anyhow!("The library `{}` file name is missing", path))?;
            let contract = file_and_contract.next().ok_or_else(|| {
                anyhow::anyhow!("The library `{}` contract name is missing", path)
            })?;
            let address = path_and_address
                .next()
                .ok_or_else(|| anyhow::anyhow!("The library `{}` address is missing", path))?;
            libraries
                .entry(file.to_owned())
                .or_insert_with(BTreeMap::new)
                .insert(contract.to_owned(), address.to_owned());
        }
        Ok(libraries)
    }
}

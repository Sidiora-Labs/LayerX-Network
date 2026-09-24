use super::{evm_compilers, solc_compiler_cli, Error};
use crate::{DetailedVersion, Language, Version};
use anyhow::Context;
use async_trait::async_trait;
use foundry_compilers::{
    artifacts, artifacts::output_selection::OutputSelection, solc::SolcLanguage,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, path::Path, sync::Arc};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(transparent)]
pub struct SolcInput(pub artifacts::SolcInput);

impl evm_compilers::CompilerInput for SolcInput {
    fn normalize_output_selection(&mut self, _version: &semver::Version) {
        self.0.settings.output_selection = OutputSelection::complete_output_selection();
    }

    fn modified_copy(&self) -> Self {
        let mut copy = self.clone();
        copy.0.sources.iter_mut().for_each(|(_file, source)| {
            let mut modified_content = source.content.as_ref().clone();
            modified_content.push(' ');
            source.content = Arc::new(modified_content);
        });
        copy
    }

    fn language(&self) -> Language {
        match self.0.language {
            SolcLanguage::Solidity => Language::Solidity,
            SolcLanguage::Yul => Language::Yul,
            // default value required because SolcLanguage enum is non_exhaustive
            _ => Language::Solidity,
        }
    }

    fn settings(&self) -> Value {
        serde_json::to_value(&self.0.settings).expect("failed to serialize settings")
    }

    fn sources(&self) -> BTreeMap<String, String> {
        let mut sources = BTreeMap::new();
        for (file_path, source) in self.0.sources.clone() {
            sources.insert(
                file_path.to_string_lossy().to_string(),
                source.content.as_ref().clone(),
            );
        }
        sources
    }
}

impl evm_compilers::CompilationError for artifacts::solc::Error {
    fn formatted_message(&self) -> String {
        self.formatted_message
            .clone()
            .unwrap_or(self.message.clone())
    }
}

#[derive(Debug, Default)]
pub struct SolcCompiler {}

#[async_trait]
impl evm_compilers::EvmCompiler for SolcCompiler {
    type CompilerInput = SolcInput;
    type CompilationError = artifacts::solc::Error;

    async fn compile(
        compiler_path: &Path,
        compiler_version: &DetailedVersion,
        input: &Self::CompilerInput,
    ) -> Result<Value, Error> {
        if compiler_version.to_semver() < &semver::Version::new(0, 4, 11) {
            let output = solc_compiler_cli::compile_using_cli(compiler_path, input)
                .await
                .context("error compiling using cli")?;
            return Ok(
                serde_json::to_value(output).context("serializing compiler output into value")?
            );
        }
        let solc = foundry_compilers::solc::Solc::new_with_version(
            compiler_path,
            compiler_version.to_semver().to_owned(),
        );
        let output = solc
            .async_compile_output(input)
            .await
            .context("compilation")?;
        let output_value =
            serde_json::from_slice(&output).context("deserializing compiler output into value")?;

        Ok(output_value)
    }
}

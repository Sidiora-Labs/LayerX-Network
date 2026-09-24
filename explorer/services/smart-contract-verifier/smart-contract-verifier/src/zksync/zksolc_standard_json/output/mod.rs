//!
//! The `solc --standard-json` output.
//!

pub mod contract;
pub mod error;
pub mod source;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
// use sha3::Digest;
//
// use crate::evmla::assembly::instruction::Instruction;
// use crate::evmla::assembly::Assembly;
// use crate::project::contract::ir::IR as ProjectContractIR;
// use crate::project::contract::Contract as ProjectContract;
// use crate::project::Project;
// use crate::solc::pipeline::Pipeline as SolcPipeline;
// use crate::solc::version::Version as SolcVersion;
// use crate::warning::Warning;
// use crate::yul::lexer::Lexer;
// use crate::yul::parser::statement::object::Object;
//
use self::{contract::Contract, error::Error as SolcStandardJsonOutputError, source::Source};

///
/// The `solc --standard-json` output.
///
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Output {
    /// The file-contract hashmap.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contracts: Option<BTreeMap<String, BTreeMap<String, Contract>>>,
    /// The source code mapping data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sources: Option<BTreeMap<String, Source>>,
    /// The compilation errors and warnings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub errors: Option<Vec<SolcStandardJsonOutputError>>,
    /// The `solc` compiler version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// The `solc` compiler long version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub long_version: Option<String>,
    /// The `zksolc` compiler version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zk_version: Option<String>,
}
//
// impl Output {
//     ///
//     /// Converts the `solc` JSON output into a convenient project.
//     ///
//     pub fn try_to_project(
//         &mut self,
//         source_code_files: BTreeMap<String, String>,
//         libraries: BTreeMap<String, BTreeMap<String, String>>,
//         pipeline: SolcPipeline,
//         solc_version: &SolcVersion,
//         debug_config: Option<&era_compiler_llvm_context::DebugConfig>,
//     ) -> anyhow::Result<Project> {
//         if let SolcPipeline::EVMLA = pipeline {
//             self.preprocess_dependencies()?;
//         }
//
//         let files = match self.contracts.as_ref() {
//             Some(files) => files,
//             None => {
//                 anyhow::bail!(
//                     "{}",
//                     self.errors
//                         .as_ref()
//                         .map(|errors| serde_json::to_string_pretty(errors).expect("Always valid"))
//                         .unwrap_or_else(|| "Unknown project assembling error".to_owned())
//                 );
//             }
//         };
//         let mut project_contracts = BTreeMap::new();
//
//         for (path, contracts) in files.iter() {
//             for (name, contract) in contracts.iter() {
//                 let full_path = format!("{path}:{name}");
//
//                 let source = match pipeline {
//                     SolcPipeline::Yul => {
//                         let ir_optimized = match contract.ir_optimized.to_owned() {
//                             Some(ir_optimized) => ir_optimized,
//                             None => continue,
//                         };
//                         if ir_optimized.is_empty() {
//                             continue;
//                         }
//
//                         if let Some(debug_config) = debug_config {
//                             debug_config.dump_yul(
//                                 full_path.as_str(),
//                                 None,
//                                 ir_optimized.as_str(),
//                             )?;
//                         }
//
//                         let mut lexer = Lexer::new(ir_optimized.to_owned());
//                         let object = Object::parse(&mut lexer, None).map_err(|error| {
//                             anyhow::anyhow!("Contract `{}` parsing error: {:?}", full_path, error)
//                         })?;
//
//                         ProjectContractIR::new_yul(ir_optimized.to_owned(), object)
//                     }
//                     SolcPipeline::EVMLA => {
//                         let evm = contract.evm.as_ref();
//                         let assembly = match evm.and_then(|evm| evm.assembly.to_owned()) {
//                             Some(assembly) => assembly.to_owned(),
//                             None => continue,
//                         };
//                         let extra_metadata = evm
//                             .and_then(|evm| evm.extra_metadata.to_owned())
//                             .unwrap_or_default();
//
//                         ProjectContractIR::new_evmla(assembly, extra_metadata)
//                     }
//                 };
//
//                 let source_code = source_code_files
//                     .get(path.as_str())
//                     .ok_or_else(|| anyhow::anyhow!("Source code for path `{}` not found", path))?;
//                 let source_hash = sha3::Keccak256::digest(source_code.as_bytes()).into();
//
//                 let project_contract = ProjectContract::new(
//                     full_path.clone(),
//                     source_hash,
//                     solc_version.to_owned(),
//                     source,
//                     contract.metadata.to_owned(),
//                 );
//                 project_contracts.insert(full_path, project_contract);
//             }
//         }
//
//         Ok(Project::new(
//             solc_version.to_owned(),
//             project_contracts,
//             libraries,
//         ))
//     }
//
//     ///
//     /// Removes EVM artifacts to prevent their accidental usage.
//     ///
//     pub fn remove_evm(&mut self) {
//         if let Some(files) = self.contracts.as_mut() {
//             for (_, file) in files.iter_mut() {
//                 for (_, contract) in file.iter_mut() {
//                     if let Some(evm) = contract.evm.as_mut() {
//                         evm.bytecode = None;
//                     }
//                 }
//             }
//         }
//     }
//
//     ///
//     /// Traverses the AST and returns the list of additional errors and warnings.
//     ///
//     pub fn preprocess_ast(
//         &mut self,
//         version: &SolcVersion,
//         pipeline: SolcPipeline,
//         suppressed_warnings: &[Warning],
//     ) -> anyhow::Result<()> {
//         let sources = match self.sources.as_ref() {
//             Some(sources) => sources,
//             None => return Ok(()),
//         };
//
//         let mut messages = Vec::new();
//         for (path, source) in sources.iter() {
//             if let Some(ast) = source.ast.as_ref() {
//                 let mut eravm_messages =
//                     Source::get_messages(ast, version, pipeline, suppressed_warnings);
//                 for message in eravm_messages.iter_mut() {
//                     message.push_contract_path(path.as_str());
//                 }
//                 messages.extend(eravm_messages);
//             }
//         }
//
//         self.errors = match self.errors.take() {
//             Some(mut errors) => {
//                 errors.extend(messages);
//                 Some(errors)
//             }
//             None => Some(messages),
//         };
//
//         Ok(())
//     }
//
//     ///
//     /// The pass, which replaces with dependency indexes with actual data.
//     ///
//     fn preprocess_dependencies(&mut self) -> anyhow::Result<()> {
//         let files = match self.contracts.as_mut() {
//             Some(files) => files,
//             None => return Ok(()),
//         };
//         let mut hash_path_mapping = BTreeMap::new();
//
//         for (path, contracts) in files.iter() {
//             for (name, contract) in contracts.iter() {
//                 let full_path = format!("{path}:{name}");
//                 let hash = match contract
//                     .evm
//                     .as_ref()
//                     .and_then(|evm| evm.assembly.as_ref())
//                     .map(|assembly| assembly.keccak256())
//                 {
//                     Some(hash) => hash,
//                     None => continue,
//                 };
//
//                 hash_path_mapping.insert(hash, full_path);
//             }
//         }
//
//         for (path, contracts) in files.iter_mut() {
//             for (name, contract) in contracts.iter_mut() {
//                 let assembly = match contract.evm.as_mut().and_then(|evm| evm.assembly.as_mut()) {
//                     Some(assembly) => assembly,
//                     None => continue,
//                 };
//
//                 let full_path = format!("{path}:{name}");
//                 Self::preprocess_dependency_level(
//                     full_path.as_str(),
//                     assembly,
//                     &hash_path_mapping,
//                 )?;
//             }
//         }
//
//         Ok(())
//     }
//
//     ///
//     /// Preprocesses an assembly JSON structure dependency data map.
//     ///
//     fn preprocess_dependency_level(
//         full_path: &str,
//         assembly: &mut Assembly,
//         hash_path_mapping: &BTreeMap<String, String>,
//     ) -> anyhow::Result<()> {
//         assembly.set_full_path(full_path.to_owned());
//
//         let deploy_code_index_path_mapping =
//             assembly.deploy_dependencies_pass(full_path, hash_path_mapping)?;
//         if let Some(deploy_code_instructions) = assembly.code.as_deref_mut() {
//             Instruction::replace_data_aliases(
//                 deploy_code_instructions,
//                 &deploy_code_index_path_mapping,
//             )?;
//         };
//
//         let runtime_code_index_path_mapping =
//             assembly.runtime_dependencies_pass(full_path, hash_path_mapping)?;
//         if let Some(runtime_code_instructions) = assembly
//             .data
//             .as_mut()
//             .and_then(|data_map| data_map.get_mut("0"))
//             .and_then(|data| data.get_assembly_mut())
//             .and_then(|assembly| assembly.code.as_deref_mut())
//         {
//             Instruction::replace_data_aliases(
//                 runtime_code_instructions,
//                 &runtime_code_index_path_mapping,
//             )?;
//         }
//
//         Ok(())
//     }
// }

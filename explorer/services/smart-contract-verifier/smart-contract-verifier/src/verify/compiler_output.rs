use foundry_compilers::artifacts;
use serde::Deserialize;
use std::collections::BTreeMap;
use verification_common::verifier_alliance::{ImmutableReferences, LinkReferences};

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct SharedCompilerOutput {
    #[serde(default)]
    pub contracts: BTreeMap<String, BTreeMap<String, Contract>>,
    #[serde(default)]
    pub sources: SourceFiles,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Contract {
    /// The Ethereum Contract ABI.
    /// See https://docs.soliditylang.org/en/develop/abi-spec.html
    pub abi: Option<serde_json::Value>,
    pub userdoc: Option<serde_json::Value>,
    pub devdoc: Option<serde_json::Value>,
    pub storage_layout: Option<serde_json::Value>,
    /// EVM-related outputs
    pub evm: Evm,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Evm {
    pub bytecode: Bytecode,
    pub deployed_bytecode: DeployedBytecode,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Bytecode {
    pub object: BytecodeObject,
    /// The source mapping as a string. See the source mapping definition.
    pub source_map: Option<serde_json::Value>,
    /// If given, this is an unlinked object.
    pub link_references: Option<LinkReferences>,
}

pub type BytecodeObject = artifacts::BytecodeObject;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DeployedBytecode {
    #[serde(flatten)]
    pub bytecode: Bytecode,
    pub immutable_references: Option<ImmutableReferences>,
}

pub type SourceFiles = BTreeMap<String, SourceFile>;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct SourceFile {
    pub id: u32,
    pub ast: Option<serde_json::Value>,
}

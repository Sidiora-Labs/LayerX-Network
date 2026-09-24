use blockscout_display_bytes::Bytes as DisplayBytes;
use serde::{de::DeserializeOwned, Deserialize};
use smart_contract_verifier_proto::blockscout::smart_contract_verifier::v2::source::MatchType;
pub use smart_contract_verifier_proto::blockscout::smart_contract_verifier::v2::BytecodeType;
use std::{borrow::Cow, collections::BTreeMap};
use verification_common::solidity_libraries;

const TEST_CASES_DIR: &str = "tests/test_cases_solidity";

pub trait TestCase {
    fn route() -> &'static str;

    fn to_request(&self, bytecode_type: BytecodeType) -> serde_json::Value;

    fn is_yul(&self) -> bool;

    fn file_name(&self) -> Cow<'_, str>;

    fn contract_name(&self) -> &str;

    fn compiler_version(&self) -> &str;

    fn evm_version(&self) -> Option<String>;

    fn libraries(&self) -> BTreeMap<String, String>;

    fn optimizer_enabled(&self) -> Option<bool>;

    fn optimizer_runs(&self) -> Option<i32>;

    fn source_files(&self) -> BTreeMap<String, String>;

    fn constructor_args(&self) -> Option<DisplayBytes>;

    fn match_type(&self) -> MatchType;

    fn abi(&self) -> Option<serde_json::Value> {
        None
    }

    fn compiler_settings(&self) -> Option<serde_json::Value> {
        None
    }

    fn compiler_artifacts(&self) -> Option<serde_json::Value> {
        None
    }

    fn creation_input_artifacts(&self) -> Option<serde_json::Value> {
        None
    }

    fn deployed_bytecode_artifacts(&self) -> Option<serde_json::Value> {
        None
    }
}

pub fn from_file<T: TestCase + DeserializeOwned>(test_case: &str) -> T {
    let test_case_path = format!("{TEST_CASES_DIR}/{test_case}.json");
    let content = std::fs::read_to_string(test_case_path).expect("failed to read file");
    serde_json::from_str(&content).expect("invalid test case format")
}

#[derive(Debug, Clone, Deserialize)]
pub struct Flattened {
    pub contract_name: String,
    pub deployed_bytecode: String,
    pub creation_bytecode: String,
    pub compiler_version: String,
    pub evm_version: Option<String>,
    pub optimization_runs: Option<i32>,
    #[serde(default)]
    pub libraries: BTreeMap<String, String>,
    pub source_code: String,
    pub expected_constructor_argument: Option<DisplayBytes>,
    pub is_yul: Option<bool>,
    pub is_full_match: Option<bool>,

    pub expected_compiler_artifacts: Option<serde_json::Value>,
    pub expected_creation_input_artifacts: Option<serde_json::Value>,
    pub expected_deployed_bytecode_artifacts: Option<serde_json::Value>,

    // Verification metadata related values
    pub chain_id: Option<String>,
    pub contract_address: Option<String>,
}

impl TestCase for Flattened {
    fn route() -> &'static str {
        "/api/v2/verifier/solidity/sources:verify-multi-part"
    }

    fn to_request(&self, bytecode_type: BytecodeType) -> serde_json::Value {
        let extension = if self.is_yul() { "yul" } else { "sol" };
        let bytecode = match bytecode_type {
            BytecodeType::Unspecified | BytecodeType::CreationInput => {
                self.creation_bytecode.as_str()
            }
            BytecodeType::DeployedBytecode => self.deployed_bytecode.as_str(),
        };
        serde_json::json!({
            "bytecode": bytecode,
            "bytecodeType": bytecode_type.as_str_name(),
            "compilerVersion": self.compiler_version,
            "evmVersion": self.evm_version,
            "optimization_runs": self.optimization_runs,
            "sourceFiles": {
                format!("source.{extension}"): self.source_code
            },
            "libraries": self.libraries,
            "metadata": {
                "chainId": self.chain_id,
                "contractAddress": self.contract_address
            }
        })
    }

    fn is_yul(&self) -> bool {
        self.is_yul.unwrap_or_default()
    }

    fn file_name(&self) -> Cow<'_, str> {
        "source.sol".into()
    }

    fn contract_name(&self) -> &str {
        self.contract_name.as_str()
    }

    fn compiler_version(&self) -> &str {
        self.compiler_version.as_str()
    }

    fn evm_version(&self) -> Option<String> {
        self.evm_version.clone()
    }

    fn libraries(&self) -> BTreeMap<String, String> {
        self.libraries.clone()
    }

    fn optimizer_enabled(&self) -> Option<bool> {
        Some(self.optimization_runs.is_some())
    }

    fn optimizer_runs(&self) -> Option<i32> {
        self.optimization_runs.or(Some(200))
    }

    fn source_files(&self) -> BTreeMap<String, String> {
        BTreeMap::from([(self.file_name().to_string(), self.source_code.clone())])
    }

    fn constructor_args(&self) -> Option<DisplayBytes> {
        self.expected_constructor_argument.clone()
    }

    fn match_type(&self) -> MatchType {
        if let Some(true) = self.is_full_match {
            MatchType::Full
        } else {
            MatchType::Partial
        }
    }

    fn abi(&self) -> Option<serde_json::Value> {
        None
    }

    fn compiler_artifacts(&self) -> Option<serde_json::Value> {
        self.expected_compiler_artifacts.clone()
    }

    fn creation_input_artifacts(&self) -> Option<serde_json::Value> {
        self.expected_creation_input_artifacts.clone()
    }

    fn deployed_bytecode_artifacts(&self) -> Option<serde_json::Value> {
        self.expected_deployed_bytecode_artifacts.clone()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct StandardJson {
    pub file_name: String,
    pub contract_name: String,
    pub deployed_bytecode: String,
    pub creation_bytecode: String,
    pub compiler_version: String,

    #[serde(deserialize_with = "StandardJson::deserialize_input")]
    pub input: String,
    #[serde(default)]
    pub manually_linked_libraries: BTreeMap<String, String>,

    pub is_full_match: Option<bool>,
    pub expected_constructor_argument: Option<DisplayBytes>,
    pub expected_compiler_artifacts: Option<serde_json::Value>,
    pub expected_creation_input_artifacts: Option<serde_json::Value>,
    pub expected_deployed_bytecode_artifacts: Option<serde_json::Value>,

    // Verification metadata related values
    pub chain_id: Option<String>,
    pub contract_address: Option<String>,
}

impl StandardJson {
    fn deserialize_input<'de, D>(deserializer: D) -> Result<String, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let val = serde_json::Value::deserialize(deserializer)?;
        Ok(val.to_string())
    }
}

// The helper function for the tests. Allows to easier create the structs required for the json fields deserialization.
macro_rules! single_field_struct {
    ($struct_name:ident, $field:ident, $field_type:ty) => {
        #[derive(Default, Deserialize)]
        #[serde(rename_all = "camelCase")]
        #[serde(default)]
        struct $struct_name {
            $field: $field_type,
        }
    };
}

impl TestCase for StandardJson {
    fn route() -> &'static str {
        "/api/v2/verifier/solidity/sources:verify-standard-json"
    }

    fn to_request(&self, bytecode_type: BytecodeType) -> serde_json::Value {
        let bytecode = match bytecode_type {
            BytecodeType::Unspecified | BytecodeType::CreationInput => {
                self.creation_bytecode.as_str()
            }
            BytecodeType::DeployedBytecode => self.deployed_bytecode.as_str(),
        };
        serde_json::json!({
            "bytecode": bytecode,
            "bytecodeType": bytecode_type.as_str_name(),
            "compilerVersion": self.compiler_version,
            "input": self.input,
            "metadata": {
                "chainId": self.chain_id,
                "contractAddress": self.contract_address
            }
        })
    }

    fn is_yul(&self) -> bool {
        single_field_struct!(Input, language, String);

        let input: Input = serde_json::from_str(&self.input).expect("language parsing failed");
        input.language == "Yul"
    }

    fn file_name(&self) -> Cow<'_, str> {
        Cow::from(&self.file_name)
    }

    fn contract_name(&self) -> &str {
        self.contract_name.as_str()
    }

    fn compiler_version(&self) -> &str {
        self.compiler_version.as_str()
    }

    fn evm_version(&self) -> Option<String> {
        single_field_struct!(Settings, evm_version, Option<String>);
        single_field_struct!(Input, settings, Option<Settings>);

        let input: Input = serde_json::from_str(&self.input).expect("evm version parsing failed");
        input.settings.and_then(|v| v.evm_version)
    }

    fn libraries(&self) -> BTreeMap<String, String> {
        let mut linked_libraries = BTreeMap::new();
        if let Some(compiler_settings) = self.compiler_settings() {
            linked_libraries.extend(
                solidity_libraries::try_parse_compiler_linked_libraries(&compiler_settings)
                    .unwrap(),
            )
        }
        linked_libraries.extend(self.manually_linked_libraries.clone());

        linked_libraries
    }

    fn optimizer_enabled(&self) -> Option<bool> {
        single_field_struct!(Optimizer, enabled, Option<bool>);
        single_field_struct!(Settings, optimizer, Option<Optimizer>);
        single_field_struct!(Input, settings, Option<Settings>);

        let input: Input =
            serde_json::from_str(&self.input).expect("optimizer_enabled parsing failed");
        input
            .settings
            .and_then(|v| v.optimizer.and_then(|v| v.enabled))
    }

    fn optimizer_runs(&self) -> Option<i32> {
        single_field_struct!(Optimizer, runs, Option<i32>);
        single_field_struct!(Settings, optimizer, Option<Optimizer>);
        single_field_struct!(Input, settings, Option<Settings>);

        let input: Input =
            serde_json::from_str(&self.input).expect("optimizer_runs parsing failed");
        input
            .settings
            .and_then(|v| v.optimizer.and_then(|v| v.runs))
    }

    fn source_files(&self) -> BTreeMap<String, String> {
        single_field_struct!(Source, content, String);
        single_field_struct!(Input, sources, BTreeMap<String, Source>);

        let input: Input = serde_json::from_str(&self.input).expect("source files parsing failed");
        input
            .sources
            .into_iter()
            .map(|(name, source)| (name, source.content))
            .collect()
    }

    fn constructor_args(&self) -> Option<DisplayBytes> {
        self.expected_constructor_argument.clone()
    }

    fn match_type(&self) -> MatchType {
        if let Some(true) = self.is_full_match {
            MatchType::Full
        } else {
            MatchType::Partial
        }
    }

    fn abi(&self) -> Option<serde_json::Value> {
        None
    }

    fn compiler_settings(&self) -> Option<serde_json::Value> {
        single_field_struct!(Input, settings, Option<serde_json::Value>);

        let input: Input = serde_json::from_str(&self.input).expect("settings parsing failed");
        input.settings
    }

    fn compiler_artifacts(&self) -> Option<serde_json::Value> {
        self.expected_compiler_artifacts.clone()
    }

    fn creation_input_artifacts(&self) -> Option<serde_json::Value> {
        self.expected_creation_input_artifacts.clone()
    }

    fn deployed_bytecode_artifacts(&self) -> Option<serde_json::Value> {
        self.expected_deployed_bytecode_artifacts.clone()
    }
}

use blockscout_display_bytes::Bytes as DisplayBytes;
use serde::{de::DeserializeOwned, Deserialize};
use smart_contract_verifier_proto::blockscout::smart_contract_verifier::v2::{
    verify_response::extra_data::BytecodePart, BytecodeType,
};
use std::{borrow::Cow, collections::BTreeMap, path::PathBuf, str::FromStr};

const TEST_CASES_DIR: &str = "tests/test_cases_vyper";

pub trait TestCase {
    fn route() -> &'static str;

    fn to_request(&self) -> serde_json::Value;

    fn file_name(&self) -> Cow<'_, str>;

    fn contract_name(&self) -> &str;

    fn constructor_args(&self) -> Option<DisplayBytes>;

    fn compiler_version(&self) -> &str;

    fn source_files(&self) -> BTreeMap<String, String>;

    fn evm_version(&self) -> Option<String>;

    fn optimize(&self) -> Option<bool> {
        None
    }

    fn bytecode_metadata(&self) -> Option<bool> {
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

    fn is_blueprint(&self) -> bool {
        false
    }

    fn expected_local_creation_code(&self) -> Option<Vec<BytecodePart>> {
        None
    }

    fn expected_local_runtime_code(&self) -> Option<Vec<BytecodePart>> {
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
    #[serde(default = "default_flattened_contract_name")]
    pub contract_name: String,
    pub deployed_bytecode: String,
    pub creation_bytecode: String,
    pub compiler_version: String,
    pub evm_version: Option<String>,
    pub source_code: String,
    pub expected_constructor_argument: Option<DisplayBytes>,
    pub expected_compiler_artifacts: Option<serde_json::Value>,
    pub expected_creation_input_artifacts: Option<serde_json::Value>,
    pub expected_deployed_bytecode_artifacts: Option<serde_json::Value>,
    #[serde(default)]
    pub is_blueprint: bool,
    pub expected_local_creation_code: Option<Vec<BytecodePart>>,
    pub expected_local_runtime_code: Option<Vec<BytecodePart>>,

    // Verification metadata related values
    pub chain_id: Option<String>,
    pub contract_address: Option<String>,

    #[serde(default)]
    pub use_deployed_bytecode: bool,
}

fn default_flattened_contract_name() -> String {
    "VyperContract".to_string()
}

impl TestCase for Flattened {
    fn route() -> &'static str {
        "/api/v2/verifier/vyper/sources:verify-multi-part"
    }

    fn to_request(&self) -> serde_json::Value {
        let (bytecode, bytecode_type) = if self.use_deployed_bytecode {
            (
                &self.deployed_bytecode,
                BytecodeType::DeployedBytecode.as_str_name(),
            )
        } else {
            (
                &self.creation_bytecode,
                BytecodeType::CreationInput.as_str_name(),
            )
        };
        serde_json::json!({
            "bytecode": bytecode,
            "bytecodeType": bytecode_type,
            "compilerVersion": self.compiler_version,
            "evmVersion": self.evm_version,
            "sourceFiles": {
                format!("{}.vy", self.contract_name): self.source_code
            },
            "metadata": {
                "chainId": self.chain_id,
                "contractAddress": self.contract_address
            }
        })
    }

    fn file_name(&self) -> Cow<'_, str> {
        format!("{}.vy", self.contract_name).into()
    }

    fn contract_name(&self) -> &str {
        self.contract_name.as_str()
    }

    fn constructor_args(&self) -> Option<DisplayBytes> {
        self.expected_constructor_argument.clone()
    }

    fn compiler_version(&self) -> &str {
        self.compiler_version.as_str()
    }

    fn evm_version(&self) -> Option<String> {
        self.evm_version.clone()
    }

    fn source_files(&self) -> BTreeMap<String, String> {
        BTreeMap::from([(self.file_name().to_string(), self.source_code.clone())])
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

    fn is_blueprint(&self) -> bool {
        self.is_blueprint
    }

    fn expected_local_creation_code(&self) -> Option<Vec<BytecodePart>> {
        self.expected_local_creation_code.clone()
    }

    fn expected_local_runtime_code(&self) -> Option<Vec<BytecodePart>> {
        self.expected_local_runtime_code.clone()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct MultiPart {
    #[allow(unused)]
    pub deployed_bytecode: String,
    pub creation_bytecode: String,
    pub compiler_version: String,
    pub file_name: String,
    pub contract_name: String,
    pub evm_version: Option<String>,
    pub source_files: BTreeMap<String, String>,
    pub interfaces: BTreeMap<String, String>,
    pub expected_constructor_argument: Option<DisplayBytes>,
    #[serde(default)]
    pub is_blueprint: bool,

    // Verification metadata related values
    pub chain_id: Option<String>,
    pub contract_address: Option<String>,
}

impl TestCase for MultiPart {
    fn route() -> &'static str {
        "/api/v2/verifier/vyper/sources:verify-multi-part"
    }

    fn to_request(&self) -> serde_json::Value {
        serde_json::json!({
            "bytecode": self.creation_bytecode,
            "bytecodeType": BytecodeType::CreationInput.as_str_name(),
            "compilerVersion": self.compiler_version,
            "evmVersion": self.evm_version,
            "sourceFiles": self.source_files,
            "interfaces": self.interfaces,
            "metadata": {
                "chainId": self.chain_id,
                "contractAddress": self.contract_address
            }
        })
    }

    fn file_name(&self) -> Cow<'_, str> {
        Cow::from(&self.file_name)
    }

    fn contract_name(&self) -> &str {
        self.contract_name.as_str()
    }

    fn constructor_args(&self) -> Option<DisplayBytes> {
        self.expected_constructor_argument.clone()
    }

    fn compiler_version(&self) -> &str {
        self.compiler_version.as_str()
    }

    fn evm_version(&self) -> Option<String> {
        self.evm_version.clone()
    }

    fn source_files(&self) -> BTreeMap<String, String> {
        let sources = self.source_files.clone().into_iter();
        let interfaces = self.interfaces.iter().map(|(path, content)| {
            let content = if PathBuf::from(path).extension() == Some(std::ffi::OsStr::new("json")) {
                let value = serde_json::Value::from_str(content).unwrap();
                value.to_string()
            } else {
                content.clone()
            };
            (path.clone(), content)
        });
        sources.chain(interfaces).collect()
    }

    fn is_blueprint(&self) -> bool {
        self.is_blueprint
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct StandardJson {
    #[allow(unused)]
    pub deployed_bytecode: String,
    pub creation_bytecode: String,
    pub compiler_version: String,
    pub file_name: String,
    pub contract_name: String,
    #[serde(deserialize_with = "StandardJson::deserialize_input")]
    pub input: String,
    pub expected_constructor_argument: Option<DisplayBytes>,
    #[serde(default)]
    pub is_blueprint: bool,

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

impl TestCase for StandardJson {
    fn route() -> &'static str {
        "/api/v2/verifier/vyper/sources:verify-standard-json"
    }

    fn to_request(&self) -> serde_json::Value {
        serde_json::json!({
            "bytecode": self.creation_bytecode,
            "bytecodeType": BytecodeType::CreationInput.as_str_name(),
            "compilerVersion": self.compiler_version,
            "input": self.input,
            "metadata": {
                "chainId": self.chain_id,
                "contractAddress": self.contract_address
            }
        })
    }

    fn file_name(&self) -> Cow<'_, str> {
        Cow::from(&self.file_name)
    }

    fn contract_name(&self) -> &str {
        self.contract_name.as_str()
    }

    fn constructor_args(&self) -> Option<DisplayBytes> {
        self.expected_constructor_argument.clone()
    }

    fn compiler_version(&self) -> &str {
        self.compiler_version.as_str()
    }

    fn source_files(&self) -> BTreeMap<String, String> {
        #[derive(Deserialize)]
        struct VyperSource {
            pub content: String,
        }
        #[derive(Deserialize)]
        struct AbiSource {
            pub abi: serde_json::Value,
        }
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Interface {
            Vyper(VyperSource),
            Abi(AbiSource),
            ContractTypes(serde_json::Value),
        }

        let input = serde_json::Value::from_str(&self.input).unwrap();
        let mut source_files = if let serde_json::Value::Object(map) =
            input.get("sources").expect("sources are missing")
        {
            map.into_iter()
                .map(|(path, value)| {
                    let source: VyperSource =
                        serde_json::from_value(value.clone()).expect("invalid source");
                    (path.clone(), source.content)
                })
                .collect()
        } else {
            BTreeMap::default()
        };
        if let Some(serde_json::Value::Object(map)) = input.get("interfaces") {
            source_files.extend(map.into_iter().map(|(path, value)| {
                let interface: Interface =
                    serde_json::from_value(value.clone()).expect("invalid interface");
                let content = match interface {
                    Interface::Vyper(source) => source.content,
                    Interface::Abi(source) => source.abi.to_string(),
                    Interface::ContractTypes(source) => source.to_string(),
                };
                (path.clone(), content)
            }))
        };
        source_files
    }

    fn evm_version(&self) -> Option<String> {
        let input = serde_json::Value::from_str(&self.input).unwrap();
        input.get("settings")?.get("evmVersion").map(|value| {
            if let serde_json::Value::String(val) = value {
                val.clone()
            } else {
                panic!("evm version is not a string")
            }
        })
    }

    fn optimize(&self) -> Option<bool> {
        let input = serde_json::Value::from_str(&self.input).unwrap();
        input.get("settings")?.get("optimize").map(|value| {
            if let serde_json::Value::Bool(val) = value {
                *val
            } else {
                panic!("optimize is not a bool")
            }
        })
    }

    fn bytecode_metadata(&self) -> Option<bool> {
        let input = serde_json::Value::from_str(&self.input).unwrap();
        input.get("settings")?.get("bytecodeMetadata").map(|value| {
            if let serde_json::Value::Bool(val) = value {
                *val
            } else {
                panic!("bytecode metadata is not a bool")
            }
        })
    }

    fn is_blueprint(&self) -> bool {
        self.is_blueprint
    }
}

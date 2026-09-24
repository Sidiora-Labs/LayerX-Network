use bytes::Bytes;
use pretty_assertions::assert_eq;
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::Value;
use smart_contract_verifier_proto::blockscout::smart_contract_verifier::v2::zksync::solidity::{
    verify_response, Match, VerificationSuccess, VerifyResponse, VerifyStandardJsonRequest,
};
use std::collections::BTreeMap;

pub trait TestCase {
    fn to_request(&self) -> VerifyStandardJsonRequest;
    fn check_verification_success(&self, success: VerificationSuccess);

    fn check_verify_response(&self, response: VerifyResponse) {
        match response {
            VerifyResponse {
                verify_response: Some(verify_response::VerifyResponse::VerificationSuccess(success)),
            } => self.check_verification_success(success),
            _ => {
                panic!("invalid response: {response:#?}")
            }
        }
    }
}

pub fn from_file<T: TestCase + DeserializeOwned>(test_case: &str) -> T {
    let current_dir = std::env::current_dir().unwrap();
    let current_dir = current_dir.to_string_lossy();
    let test_case_path = format!("{current_dir}/tests/test_cases_zksync_solidity/{test_case}.json");
    let content = std::fs::read_to_string(test_case_path).expect("failed to read file");
    serde_json::from_str(&content).expect("invalid test case format")
}

#[serde_with::serde_as]
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct StandardJson {
    #[serde_as(as = "blockscout_display_bytes::serde_as::Hex")]
    pub deployed_code: Bytes,
    #[serde_as(as = "Option<blockscout_display_bytes::serde_as::Hex>")]
    pub constructor_arguments: Option<Bytes>,
    pub zk_compiler_version: String,
    pub evm_compiler_version: String,
    pub input: Value,
    pub file_name: String,
    pub contract_name: String,
    pub expected_sources: Option<BTreeMap<String, String>>,
    pub expected_compilation_artifacts: Option<Value>,
    pub expected_creation_code_artifacts: Option<Value>,
    pub expected_runtime_code_artifacts: Option<Value>,
    pub expected_creation_match_type: Option<String>,
    pub expected_creation_transformations: Option<Value>,
    pub expected_creation_values: Option<Value>,
    pub expected_runtime_match_type: Option<String>,
    pub expected_runtime_transformations: Option<Value>,
    pub expected_runtime_values: Option<Value>,
}

impl TestCase for StandardJson {
    fn to_request(&self) -> VerifyStandardJsonRequest {
        VerifyStandardJsonRequest {
            code: hex::encode(&self.deployed_code),
            constructor_arguments: self.constructor_arguments.as_ref().map(hex::encode),
            zk_compiler: self.zk_compiler_version.clone(),
            solc_compiler: self.evm_compiler_version.clone(),
            input: self.input.to_string(),
        }
    }

    fn check_verification_success(&self, success: VerificationSuccess) {
        assert_eq!(self.file_name, success.file_name, "invalid file name");
        assert_eq!(
            self.contract_name, success.contract_name,
            "invalid contract name"
        );
        assert_eq!(
            "zksolc",
            success.zk_compiler.as_ref().unwrap().compiler,
            "invalid zk-compiler"
        );
        assert_eq!(
            self.zk_compiler_version,
            success.zk_compiler.as_ref().unwrap().version,
            "invalid zk-compiler version"
        );
        assert_eq!(
            "solc",
            success.evm_compiler.as_ref().unwrap().compiler,
            "invalid evm-compiler"
        );
        assert_eq!(
            self.evm_compiler_version,
            success.evm_compiler.as_ref().unwrap().version,
            "invalid evm-compiler version"
        );

        {
            #[derive(Deserialize)]
            struct Input {
                language: String,
            }
            let input =
                Input::deserialize(&self.input).expect("expected language field deserialization");
            assert_eq!(
                input.language.to_lowercase(),
                success.language().as_str_name().to_lowercase(),
                "invalid language"
            );
        }

        {
            #[derive(Deserialize)]
            struct Input {
                settings: Value,
            }
            let mut input = Input::deserialize(&self.input)
                .expect("expected compiler settings field deserialization");
            let mut compiler_settings: Value = serde_json::from_str(&success.compiler_settings)
                .expect("compiler settings deserialization");
            remove_output_selection(&mut input.settings);
            remove_output_selection(&mut compiler_settings);
            assert_eq!(
                input.settings, compiler_settings,
                "invalid compiler settings"
            );
        }

        if let Some(expected_sources) = &self.expected_sources {
            assert_eq!(expected_sources, &success.sources, "invalid sources");
        }

        if let Some(expected_compilation_artifacts) = &self.expected_compilation_artifacts {
            let compilation_artifacts: Value = serde_json::from_str(&success.compilation_artifacts)
                .expect("compilation artifacts deserialization");
            assert_eq!(
                expected_compilation_artifacts, &compilation_artifacts,
                "invalid compilation artifacts"
            );
        }

        if let Some(expected_creation_code_artifacts) = &self.expected_creation_code_artifacts {
            let creation_code_artifacts: Value =
                serde_json::from_str(&success.creation_code_artifacts)
                    .expect("creation code artifacts deserialization");
            assert_eq!(
                expected_creation_code_artifacts, &creation_code_artifacts,
                "invalid creation code artifacts"
            );
        }

        if let Some(expected_runtime_code_artifacts) = &self.expected_runtime_code_artifacts {
            let runtime_code_artifacts: Value =
                serde_json::from_str(&success.runtime_code_artifacts)
                    .expect("runtime code artifacts deserialization");
            assert_eq!(
                expected_runtime_code_artifacts, &runtime_code_artifacts,
                "invalid runtime code artifacts"
            );
        }

        check_match(
            "creation",
            self.expected_creation_match_type.as_ref(),
            self.expected_creation_values.as_ref(),
            self.expected_creation_transformations.as_ref(),
            success.creation_match,
        );
        check_match(
            "runtime",
            self.expected_runtime_match_type.as_ref(),
            self.expected_runtime_values.as_ref(),
            self.expected_runtime_transformations.as_ref(),
            success.runtime_match,
        );
    }
}

fn remove_output_selection(compiler_settings: &mut Value) {
    compiler_settings
        .as_object_mut()
        .expect("Compiler settings is not an object")
        .remove("outputSelection");
}

fn check_match(
    prefix: &'static str,
    expected_match_type: Option<&String>,
    expected_values: Option<&Value>,
    expected_transformations: Option<&Value>,
    actual_match: Option<Match>,
) {
    match (expected_match_type, actual_match) {
        (None, None) => {}
        (Some(expected_creation_match_type), Some(actual_match)) => {
            assert_eq!(
                expected_creation_match_type.to_lowercase(),
                actual_match.r#type().as_str_name().to_lowercase(),
                "invalid {} match type",
                prefix,
            );
            if let Some(expected_values) = expected_values {
                let values: Value = serde_json::from_str(&actual_match.values)
                    .unwrap_or_else(|_| panic!("{prefix} values deserialization"));
                assert_eq!(expected_values, &values, "invalid {} values", prefix,);
            }
            if let Some(expected_transformations) = expected_transformations {
                let transformations: Value = serde_json::from_str(&actual_match.transformations)
                    .unwrap_or_else(|_| panic!("{prefix} transformations deserialization"));
                assert_eq!(
                    expected_transformations, &transformations,
                    "invalid {} transformations",
                    prefix,
                );
            }
        }
        (expected, actual) => {
            panic!(
                "invalid {prefix} match type; expected={:?}; actual={:?}",
                expected.as_ref().map(|v| v.to_lowercase()),
                actual.map(|v| v.r#type().as_str_name().to_lowercase())
            )
        }
    }
}

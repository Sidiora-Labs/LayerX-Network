use super::{TestCaseRequest, TestCaseResponse};
use blockscout_display_bytes::Bytes as DisplayBytes;
use pretty_assertions::assert_eq;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use smart_contract_verifier_proto::blockscout::smart_contract_verifier::v2::{
    BatchVerifyResponse, BatchVerifySolidityMultiPartRequest,
    BatchVerifySolidityStandardJsonRequest, Contract,
};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

#[derive(Debug, Clone, Deserialize)]
pub struct TestCase {
    pub deployed_creation_code: DisplayBytes,
    pub deployed_runtime_code: DisplayBytes,

    pub compiled_creation_code: DisplayBytes,
    pub compiled_runtime_code: DisplayBytes,
    pub compiler: String,
    pub version: String,
    pub language: String,
    pub name: String,
    pub fully_qualified_name: String,
    pub sources: BTreeMap<String, String>,
    pub compiler_settings: Value,
    pub compilation_artifacts: Value,
    pub creation_code_artifacts: Value,
    pub runtime_code_artifacts: Value,

    pub creation_match: bool,
    pub creation_values: Value,
    pub creation_transformations: Value,
    pub creation_match_type: String,

    pub runtime_match: bool,
    pub runtime_values: Value,
    pub runtime_transformations: Value,
    pub runtime_match_type: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TestCaseMultiPart(pub TestCase);

impl TestCaseRequest for TestCaseMultiPart {
    fn route() -> &'static str {
        "/api/v2/verifier/solidity/sources:batch-verify-multi-part"
    }

    fn to_request(&self) -> Value {
        let test_case = &self.0;

        let compiler_settings: foundry_compilers::artifacts::Settings =
            serde_json::from_value(test_case.compiler_settings.clone())
                .expect("cannot deserialize compiler settings");

        let optimization_runs = compiler_settings
            .optimizer
            .enabled
            .unwrap_or_default()
            .then_some(compiler_settings.optimizer.runs.map(|value| value as u32))
            .flatten();

        let request = BatchVerifySolidityMultiPartRequest {
            contracts: vec![Contract {
                creation_code: Some(test_case.deployed_creation_code.to_string()),
                runtime_code: Some(test_case.deployed_runtime_code.to_string()),
                metadata: None,
            }],
            compiler_version: test_case.version.clone(),
            sources: test_case.sources.clone(),
            evm_version: compiler_settings
                .evm_version
                .as_ref()
                .map(|value| value.to_string()),
            optimization_runs,
        };

        serde_json::to_value(request).expect("cannot serialize request into value")
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct TestCaseStandardJson(pub TestCase);

impl TestCaseRequest for TestCaseStandardJson {
    fn route() -> &'static str {
        "/api/v2/verifier/solidity/sources:batch-verify-standard-json"
    }

    fn to_request(&self) -> Value {
        #[derive(Clone, Debug, Serialize)]
        struct CompilerInput {
            language: String,
            sources: foundry_compilers::artifacts::Sources,
            settings: Value,
        }

        let sources = self
            .0
            .sources
            .clone()
            .into_iter()
            .map(|(file, content)| {
                (
                    PathBuf::from(file),
                    foundry_compilers::artifacts::Source {
                        content: Arc::new(content),
                        kind: Default::default(),
                    },
                )
            })
            .collect();

        let request = BatchVerifySolidityStandardJsonRequest {
            contracts: vec![Contract {
                creation_code: Some(self.0.deployed_creation_code.to_string()),
                runtime_code: Some(self.0.deployed_runtime_code.to_string()),
                metadata: None,
            }],
            compiler_version: self.0.version.clone(),
            input: serde_json::to_string(&CompilerInput {
                language: "Solidity".to_string(),
                sources,
                settings: self.0.compiler_settings.clone(),
            })
            .expect("cannot serialize compiler input to string"),
        };

        serde_json::to_value(request).expect("cannot serialize request into value")
    }
}

impl TestCaseResponse for TestCase {
    type Response = BatchVerifyResponse;

    fn check(&self, actual_response: Self::Response) {
        let super::batch_solidity::ParsedSuccessItem {
            creation_code,
            runtime_code,
            compiler,
            compiler_version,
            language,
            file_name,
            contract_name,
            sources,
            compiler_settings,
            compilation_artifacts,
            creation_code_artifacts,
            runtime_code_artifacts,
            creation_match_details,
            runtime_match_details,
        } = super::batch_solidity::retrieve_success_item(actual_response);

        let expected_file_name = {
            let names = self.fully_qualified_name.split(':').collect::<Vec<_>>();
            names[..names.len() - 1].join(":")
        };

        assert_eq!(
            self.compiled_creation_code, creation_code,
            "invalid creation_code"
        );
        assert_eq!(
            self.compiled_runtime_code, runtime_code,
            "invalid runtime_code"
        );
        assert_eq!(self.compiler.to_uppercase(), compiler, "invalid compiler");
        assert_eq!(self.version, compiler_version, "invalid compiler_version");
        assert_eq!(self.language.to_uppercase(), language, "invalid language");
        assert_eq!(expected_file_name, file_name, "invalid file_name");
        assert_eq!(self.name, contract_name, "invalid contract_name");
        assert_eq!(self.sources, sources, "invalid sources");
        assert_eq!(
            self.compiler_settings, compiler_settings,
            "invalid compiler_settings"
        );
        assert_eq!(
            self.compilation_artifacts, compilation_artifacts,
            "invalid compilation_artifacts"
        );
        assert_eq!(
            self.creation_code_artifacts, creation_code_artifacts,
            "invalid creation_code_artifacts"
        );
        assert_eq!(
            self.runtime_code_artifacts, runtime_code_artifacts,
            "invalid runtime_code_artifacts"
        );

        assert_eq!(
            self.creation_match,
            creation_match_details.is_some(),
            "invalid creation_match"
        );
        if self.creation_match {
            let creation_match_details = creation_match_details.unwrap();
            assert_eq!(
                self.creation_values.clone(),
                creation_match_details.values,
                "invalid creation_values"
            );
            assert_eq!(
                self.creation_transformations.clone(),
                creation_match_details.transformations,
                "invalid creation_transformations"
            );
            assert_eq!(
                self.creation_match_type.to_uppercase(),
                creation_match_details.match_type,
                "invalid creation_match_type"
            );
        }

        assert_eq!(
            self.runtime_match,
            runtime_match_details.is_some(),
            "invalid runtime_match"
        );
        if self.runtime_match {
            let runtime_match_details = runtime_match_details.unwrap();
            assert_eq!(
                self.runtime_values.clone(),
                runtime_match_details.values,
                "invalid runtime_values"
            );
            assert_eq!(
                self.runtime_transformations.clone(),
                runtime_match_details.transformations,
                "invalid runtime_transformations"
            );
            assert_eq!(
                self.runtime_match_type.to_uppercase(),
                runtime_match_details.match_type,
                "invalid runtime_match_type"
            );
        }
    }
}

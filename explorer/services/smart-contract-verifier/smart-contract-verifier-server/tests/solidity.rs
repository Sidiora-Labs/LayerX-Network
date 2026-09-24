mod solidity_types;

use actix_web::{
    dev::ServiceResponse,
    http::StatusCode,
    test,
    test::{read_body, read_body_json, TestRequest},
    App,
};
use blockscout_display_bytes::Bytes as DisplayBytes;
use pretty_assertions::assert_eq;
use smart_contract_verifier_proto::blockscout::smart_contract_verifier::v2::{
    solidity_verifier_actix::route_solidity_verifier, source::SourceType, verify_response::Status,
    VerifyResponse,
};
use smart_contract_verifier_server::{Settings, SolidityVerifierService};
use solidity_types::{BytecodeType, TestCase};
use std::{
    collections::BTreeMap,
    str::{from_utf8, FromStr},
    sync::Arc,
};
use tokio::sync::{OnceCell, Semaphore};

async fn global_service() -> &'static Arc<SolidityVerifierService> {
    static SERVICE: OnceCell<Arc<SolidityVerifierService>> = OnceCell::const_new();
    SERVICE
        .get_or_init(|| async {
            let settings = Settings::default();
            let compilers_lock = Semaphore::new(settings.compilers.max_threads.get());
            let service = SolidityVerifierService::new(settings.solidity, Arc::new(compilers_lock))
                .await
                .expect("couldn't initialize the service");
            Arc::new(service)
        })
        .await
}

async fn test_setup<T: TestCase>(test_case: &T, bytecode_type: BytecodeType) -> ServiceResponse {
    let service = global_service().await;
    let app = test::init_service(
        App::new().configure(|config| route_solidity_verifier(config, service.clone())),
    )
    .await;

    let request = test_case.to_request(bytecode_type);

    TestRequest::post()
        .uri(T::route())
        .set_json(&request)
        .send_request(&app)
        .await
}

async fn get_verification_response<T: TestCase>(
    test_case: &T,
    bytecode_type: BytecodeType,
) -> VerifyResponse {
    let response = test_setup(test_case, bytecode_type).await;
    if !response.status().is_success() {
        let status = response.status();
        let body = read_body(response).await;
        let message = from_utf8(&body).expect("Read body as UTF-8");
        panic!("Invalid status code (success expected). Status: {status}. Messsage: {message}")
    }

    let verification_response: VerifyResponse = read_body_json(response).await;
    assert_eq!(
        verification_response.status(),
        Status::Success,
        "Invalid verification status. Response: {verification_response:?}"
    );

    assert!(
        verification_response.extra_data.is_some(),
        "Verification extra_data is absent"
    );

    assert!(
        verification_response.source.is_some(),
        "Verification source is absent"
    );

    verification_response
}

fn validate_verification_response<T: TestCase>(
    test_case: &T,
    verification_response: VerifyResponse,
) {
    let source = verification_response
        .source
        .expect("Verification source is absent");

    /********** Check the source **********/

    assert_eq!(source.file_name, test_case.file_name(), "Invalid file name");

    assert_eq!(
        source.contract_name,
        test_case.contract_name(),
        "Invalid contract name"
    );

    assert_eq!(
        source.compiler_version,
        test_case.compiler_version(),
        "Invalid compiler version"
    );

    if let Some(expected_compiler_settings) = test_case.compiler_settings() {
        let compiler_settings = serde_json::Value::from_str(&source.compiler_settings)
            .expect("Compiler settings deserialization failed");
        assert_eq!(
            compiler_settings, expected_compiler_settings,
            "Invalid compiler settings"
        );
    } else {
        mod compiler_settings {
            use serde::Deserialize;
            use std::collections::BTreeMap;

            #[derive(Default, Deserialize)]
            #[serde(rename_all = "camelCase")]
            pub struct Optimizer {
                pub enabled: Option<bool>,
                pub runs: Option<i32>,
            }

            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            pub struct CompilerSettings {
                pub evm_version: Option<String>,
                pub libraries: BTreeMap<String, BTreeMap<String, String>>,
                #[serde(default)]
                pub optimizer: Optimizer,
            }
        }

        let compiler_settings: compiler_settings::CompilerSettings =
            serde_json::from_str(&source.compiler_settings).unwrap_or_else(|_| {
                panic!(
                    "Compiler Settings deserialization failed: {}",
                    &source.compiler_settings
                )
            });
        assert_eq!(
            compiler_settings.evm_version,
            test_case.evm_version(),
            "Invalid evm version"
        );
        assert_eq!(
            compiler_settings.libraries,
            BTreeMap::default(),
            "Invalid compiler settings libraries"
        );
        assert_eq!(
            compiler_settings.optimizer.enabled,
            test_case.optimizer_enabled(),
            "Invalid optimizer enabled value"
        );
        assert_eq!(
            compiler_settings.optimizer.runs,
            test_case.optimizer_runs(),
            "Invalid optimizer runs"
        );
    }

    let expected_source_type = if test_case.is_yul() {
        SourceType::Yul
    } else {
        SourceType::Solidity
    };
    assert_eq!(
        source.source_type(),
        expected_source_type,
        "Invalid source type"
    );

    assert_eq!(
        source.match_type(),
        test_case.match_type(),
        "Invalid match type",
    );

    assert_eq!(
        source.source_files,
        test_case.source_files(),
        "Invalid source files"
    );

    let abi: Option<serde_json::Value> = source
        .abi
        .map(|value| serde_json::from_str(&value).expect("Abi deserialization failed"));
    match (test_case.abi(), abi) {
        (Some(expected_abi), Some(abi)) => {
            assert_eq!(abi, expected_abi, "Invalid abi")
        }
        (Some(_expected_abi), None) => {
            panic!("Abi was expected but was not found")
        }
        (_, None) if !test_case.is_yul() => {
            panic!("Abi is expected for non Yul contracts but was not found")
        }
        _ => (),
    }

    assert_eq!(
        source.libraries,
        test_case.libraries(),
        "Invalid contract libraries"
    );

    let constructor_arguments = source
        .constructor_arguments
        .map(|args| DisplayBytes::from_str(&args).unwrap());
    assert_eq!(
        constructor_arguments,
        test_case.constructor_args(),
        "Invalid constructor arguments"
    );

    if let Some(expected_compilation_artifacts) = test_case.compiler_artifacts() {
        let compilation_artifacts = source
            .compilation_artifacts
            .map(|value| {
                serde_json::from_str::<serde_json::Value>(&value).unwrap_or_else(|err| {
                    panic!("Compilation artifacts deserialization failed: {value}; err: {err}")
                })
            })
            .expect("Compilation artifacts are missing");
        assert_eq!(
            compilation_artifacts, expected_compilation_artifacts,
            "Invalid compilation artifacts"
        )
    }
    if let Some(expected_creation_input_artifacts) = test_case.creation_input_artifacts() {
        let creation_input_artifacts = source
            .creation_input_artifacts
            .map(|value| {
                serde_json::from_str::<serde_json::Value>(&value).unwrap_or_else(|err| {
                    panic!("Creation input artifacts deserialization failed: {value}; err: {err}")
                })
            })
            .expect("Creation input artifacts are missing");
        assert_eq!(
            creation_input_artifacts, expected_creation_input_artifacts,
            "Invalid creation input artifacts"
        )
    }
    if let Some(expected_deployed_bytecode_artifacts) = test_case.deployed_bytecode_artifacts() {
        let deployed_bytecode_artifacts = source
            .deployed_bytecode_artifacts
            .map(|value| {
                serde_json::from_str::<serde_json::Value>(&value).unwrap_or_else(|err| {
                    panic!(
                        "Deployed bytecode artifacts deserialization failed: {value}; err: {err}"
                    )
                })
            })
            .expect("Deployed bytecode artifacts are missing");
        assert_eq!(
            deployed_bytecode_artifacts, expected_deployed_bytecode_artifacts,
            "Invalid deployed bytecode artifacts"
        )
    }
}

async fn test_success<T: TestCase>(test_case: &T, bytecode_type: BytecodeType) {
    let verification_response = get_verification_response(test_case, bytecode_type).await;
    validate_verification_response(test_case, verification_response);
}

async fn _test_failure<T: TestCase>(
    test_case: &T,
    bytecode_type: BytecodeType,
    expected_message: &str,
) {
    let response = test_setup(test_case, bytecode_type).await;

    assert!(
        response.status().is_success(),
        "Invalid status code (success expected): {}",
        response.status()
    );

    let verification_response: VerifyResponse = read_body_json(response).await;

    assert_eq!(
        verification_response.status(),
        Status::Failure,
        "Invalid verification status. Response: {verification_response:?}"
    );

    assert!(
        verification_response.source.is_none(),
        "Failure verification source should be None"
    );
    assert!(
        verification_response.extra_data.is_none(),
        "Failure verification extra data should be None"
    );

    assert!(
        verification_response.message.contains(expected_message),
        "Invalid message: {}",
        verification_response.message
    );
}

async fn _test_error<T: TestCase>(
    test_case: &T,
    bytecode_type: BytecodeType,
    expected_status: StatusCode,
    expected_message: &str,
) {
    let response = test_setup(test_case, bytecode_type).await;
    let status = response.status();
    let body = read_body(response).await;
    let message = from_utf8(&body).expect("Read body as UTF-8");

    assert_eq!(status, expected_status, "Invalid status code: {status}",);

    assert!(
        message.contains(expected_message),
        "Invalid message: {message}"
    );
}

mod success_tests {
    use super::*;
    use solidity_types::{Flattened, StandardJson};

    #[tokio::test]
    async fn returns_compilation_related_artifacts() {
        let test_case = solidity_types::from_file::<Flattened>("simple_storage");
        test_success(&test_case, BytecodeType::CreationInput).await;
        test_success(&test_case, BytecodeType::DeployedBytecode).await;
    }

    #[tokio::test]
    async fn returns_compilation_related_artifacts_with_two_cbor_auxdata() {
        let test_case = solidity_types::from_file::<Flattened>("two_cbor_auxdata");
        test_success(&test_case, BytecodeType::CreationInput).await;
        test_success(&test_case, BytecodeType::DeployedBytecode).await;
    }

    #[tokio::test]
    async fn returns_compilation_related_artifacts_with_no_metadata_hash() {
        // Now auxdata is not retrieved for contracts compiled without metadata hash.
        // TODO: should be removed, when that is fixed
        let remove_cbor_auxdata_from_artifacts = |artifacts: &mut serde_json::Value| {
            artifacts.as_object_mut().map(|artifacts| {
                if let Some(value) = artifacts.get_mut("cborAuxdata") {
                    *value = serde_json::Value::Object(Default::default());
                }
            })
        };

        let mut test_case = solidity_types::from_file::<StandardJson>("no_metadata_hash");
        test_case
            .expected_creation_input_artifacts
            .as_mut()
            .map(remove_cbor_auxdata_from_artifacts);
        test_case
            .expected_deployed_bytecode_artifacts
            .as_mut()
            .map(remove_cbor_auxdata_from_artifacts);

        test_success(&test_case, BytecodeType::CreationInput).await;
        test_success(&test_case, BytecodeType::DeployedBytecode).await;
    }

    #[tokio::test]
    async fn verifies_runtime_code_with_immutables() {
        let test_case = solidity_types::from_file::<Flattened>("immutables");
        test_success(&test_case, BytecodeType::CreationInput).await;
        test_success(&test_case, BytecodeType::DeployedBytecode).await;
    }

    #[tokio::test]
    async fn supports_cancun_evm_version() {
        let test_case = solidity_types::from_file::<StandardJson>("cancun");
        test_success(&test_case, BytecodeType::CreationInput).await;
        test_success(&test_case, BytecodeType::DeployedBytecode).await;
    }

    #[tokio::test]
    async fn flattened_accepts_partially_matching_compiler_version_commit_hashes() {
        // provided commit hash is a prefix of the one used in the list
        let initial_test_case = solidity_types::from_file::<Flattened>("simple_storage");
        let test_case = {
            let mut test_case = initial_test_case.clone();
            test_case.compiler_version.pop();
            test_case
        };
        let verification_response =
            get_verification_response(&test_case, BytecodeType::CreationInput).await;
        validate_verification_response(&initial_test_case, verification_response);

        // the commit hash from the list is a prefix of the provided one
        let test_case = {
            let mut test_case = initial_test_case.clone();
            test_case.compiler_version.push_str("1234");
            test_case
        };
        let verification_response =
            get_verification_response(&test_case, BytecodeType::CreationInput).await;
        validate_verification_response(&initial_test_case, verification_response);
    }

    #[tokio::test]
    async fn standard_json_accepts_partially_matching_compiler_version_commit_hashes() {
        // provided commit hash is a prefix of the one used in the list
        let initial_test_case = solidity_types::from_file::<StandardJson>("cancun");
        let test_case = {
            let mut test_case = initial_test_case.clone();
            test_case.compiler_version.pop();
            test_case
        };
        let verification_response =
            get_verification_response(&test_case, BytecodeType::CreationInput).await;
        validate_verification_response(&initial_test_case, verification_response);

        // the commit hash from the list is a prefix of the provided one
        let test_case = {
            let mut test_case = initial_test_case.clone();
            test_case.compiler_version.push_str("1234");
            test_case
        };
        let verification_response =
            get_verification_response(&test_case, BytecodeType::CreationInput).await;
        validate_verification_response(&initial_test_case, verification_response);
    }

    #[tokio::test]
    async fn standard_json_supports_manually_linked_libraries() {
        let test_case = solidity_types::from_file::<StandardJson>("libraries_manually_linked");
        test_success(&test_case, BytecodeType::CreationInput).await;
        test_success(&test_case, BytecodeType::DeployedBytecode).await;
    }

    #[tokio::test]
    async fn standard_json_supports_libraries_linked_by_compiler() {
        let test_case = solidity_types::from_file::<StandardJson>("libraries_linked_by_compiler");
        test_success(&test_case, BytecodeType::CreationInput).await;
        test_success(&test_case, BytecodeType::DeployedBytecode).await;
    }
}

mod regression_tests {
    use super::*;
    use solidity_types::StandardJson;

    #[tokio::test]
    async fn constructor_arguments_and_abstract_contract() {
        let test_case = solidity_types::from_file::<StandardJson>(
            "constructor_arguments_and_abstract_contract",
        );
        test_success(&test_case, BytecodeType::CreationInput).await;
    }

    #[tokio::test]
    async fn both_partial_and_full_verification_sources() {
        let test_case =
            solidity_types::from_file::<StandardJson>("both_partial_and_full_verification_sources");
        test_success(&test_case, BytecodeType::DeployedBytecode).await
    }
}

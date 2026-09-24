mod vyper_types;

use actix_web::{
    dev::ServiceResponse,
    http::StatusCode,
    test,
    test::{read_body, read_body_json, TestRequest},
    App,
};
use blockscout_display_bytes::Bytes as DisplayBytes;
use pretty_assertions::assert_eq;
use serde::Deserialize;
use smart_contract_verifier_proto::blockscout::smart_contract_verifier::v2::{
    source::{MatchType, SourceType},
    verify_response::Status,
    vyper_verifier_actix::route_vyper_verifier,
    VerifyResponse,
};
use smart_contract_verifier_server::{Settings, VyperVerifierService};
use std::{
    str::{from_utf8, FromStr},
    sync::Arc,
};
use tokio::sync::{OnceCell, Semaphore};
use vyper_types::TestCase;

async fn global_service() -> &'static Arc<VyperVerifierService> {
    static SERVICE: OnceCell<Arc<VyperVerifierService>> = OnceCell::const_new();
    SERVICE
        .get_or_init(|| async {
            let settings = Settings::default();
            let compilers_lock = Semaphore::new(settings.compilers.max_threads.get());
            let service = VyperVerifierService::new(settings.vyper, Arc::new(compilers_lock))
                .await
                .expect("couldn't initialize the service");
            Arc::new(service)
        })
        .await
}

async fn test_setup<T: TestCase>(test_case: &T) -> ServiceResponse {
    let service = global_service().await;
    let app = test::init_service(
        App::new().configure(|config| route_vyper_verifier(config, service.clone())),
    )
    .await;

    let request = test_case.to_request();

    TestRequest::post()
        .uri(T::route())
        .set_json(&request)
        .send_request(&app)
        .await
}

async fn get_verification_response<T: TestCase>(test_case: &T) -> VerifyResponse {
    let response = test_setup(test_case).await;
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
    let verification_result = verification_response.source.unwrap();

    // Vyper always results in partial matches, as currently there is no way to
    // check if the source code is exact.
    assert_eq!(
        verification_result.match_type(),
        MatchType::Partial,
        "Invalid match type"
    );

    let abi: Option<Result<alloy_json_abi::JsonAbi, _>> = verification_result
        .abi
        .as_ref()
        .map(|abi| serde_json::from_str(abi));
    assert_eq!(
        verification_result.file_name,
        test_case.file_name(),
        "Invalid file name"
    );
    assert_eq!(
        verification_result.contract_name,
        test_case.contract_name(),
        "Invalid contract name"
    );
    assert!(abi.is_some(), "Vyper contracts must have abi");
    assert!(
        abi.as_ref().unwrap().is_ok(),
        "Abi deserialization failed: {}",
        abi.unwrap().as_ref().unwrap_err()
    );
    assert_eq!(
        verification_result.source_type(),
        SourceType::Vyper,
        "Invalid source type"
    );
    let verification_result_constructor_arguments = verification_result
        .constructor_arguments
        .map(|args| DisplayBytes::from_str(&args).unwrap());
    assert_eq!(
        verification_result_constructor_arguments,
        test_case.constructor_args(),
        "Invalid constructor args"
    );
    assert_eq!(
        verification_result.compiler_version,
        test_case.compiler_version(),
        "Invalid compiler version"
    );

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct CompilerSettings {
        pub evm_version: Option<String>,
        pub optimize: Option<bool>,
        pub bytecode_metadata: Option<bool>,
    }
    let compiler_settings: CompilerSettings =
        serde_json::from_str(&verification_result.compiler_settings).unwrap_or_else(|_| {
            panic!(
                "Compiler Settings deserialization failed: {}",
                &verification_result.compiler_settings
            )
        });
    assert_eq!(
        compiler_settings.evm_version,
        test_case.evm_version(),
        "Invalid evm version"
    );
    assert_eq!(
        compiler_settings.optimize,
        test_case.optimize(),
        "Invalid optimize setting"
    );
    assert_eq!(
        compiler_settings.bytecode_metadata,
        test_case.bytecode_metadata(),
        "Invalid bytecode metadata settings"
    );

    assert_eq!(
        verification_result.source_files,
        test_case.source_files(),
        "Invalid source files"
    );

    if let Some(expected_compilation_artifacts) = test_case.compiler_artifacts() {
        let compilation_artifacts = verification_result
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
        let creation_input_artifacts = verification_result
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
        let deployed_bytecode_artifacts = verification_result
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

    assert_eq!(
        test_case.is_blueprint(),
        verification_result.is_blueprint,
        "Invalid is_blueprint value"
    );

    let extra_data = verification_response
        .extra_data
        .expect("Missing extra data");
    if let Some(expected_local_creation_code) = test_case.expected_local_creation_code() {
        assert_eq!(
            expected_local_creation_code, extra_data.local_creation_input_parts,
            "Invalid local creation code"
        );
    }
    if let Some(expected_local_runtime_code) = test_case.expected_local_runtime_code() {
        assert_eq!(
            expected_local_runtime_code, extra_data.local_deployed_bytecode_parts,
            "Invalid local runtime code"
        );
    }
}

async fn test_success(test_case: impl TestCase) {
    let verification_response = get_verification_response(&test_case).await;
    validate_verification_response(&test_case, verification_response);
}

async fn test_failure(test_case: impl TestCase, expected_message: &str) {
    let response = test_setup(&test_case).await;

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

async fn test_error(test_case: impl TestCase, expected_status: StatusCode, expected_message: &str) {
    let response = test_setup(&test_case).await;
    let status = response.status();
    let body = read_body(response).await;
    let message = from_utf8(&body).expect("Read body as UTF-8");

    assert_eq!(status, expected_status, "Invalid status code: {status}",);

    assert!(
        message.contains(expected_message),
        "Invalid message: {message}"
    );
}

mod flattened {
    use super::{
        get_verification_response, test_error, test_failure, test_success,
        validate_verification_response, vyper_types,
    };
    use actix_web::http::StatusCode;
    use vyper_types::Flattened;

    #[tokio::test]
    async fn verify_success() {
        for test_case_name in &["simple", "arguments", "erc20", "erc667"] {
            let test_case = vyper_types::from_file::<Flattened>(test_case_name);
            test_success(test_case).await;
        }
    }

    #[tokio::test]
    async fn verify_fail() {
        let mut test_case = vyper_types::from_file::<Flattened>("arguments");
        test_case.source_code =
            "count: public(uint256)\n@external\ndef __init__():\n    self.count = 345678765"
                .to_string();
        test_failure(
            test_case,
            "No contract could be verified with provided data",
        )
        .await;

        let mut test_case = vyper_types::from_file::<Flattened>("erc20");
        test_case.creation_bytecode = "0x60".to_string();
        test_failure(
            test_case,
            "No contract could be verified with provided data",
        )
        .await;
    }

    #[tokio::test]
    async fn verify_error() {
        let mut test_case = vyper_types::from_file::<Flattened>("simple");
        test_case.compiler_version = "v0.1.400+commit.e67f0147".to_string();
        test_error(
            test_case.clone(),
            StatusCode::BAD_REQUEST,
            "Compiler version not found: ",
        )
        .await;

        let mut test_case = vyper_types::from_file::<Flattened>("simple");
        test_case.creation_bytecode = "0xkeklol".to_string();
        test_error(
            test_case,
            StatusCode::BAD_REQUEST,
            "bytecode is not valid hex",
        )
        .await;
    }

    #[tokio::test]
    async fn accepts_partial_verification_metadata_in_input() {
        // `chain_id` is provided, but `contract_address` is missed from the verification metadata
        let test_case = {
            let mut test_case = vyper_types::from_file::<Flattened>("simple");
            test_case.chain_id = Some("5".to_string());
            test_case
        };
        test_success(test_case).await;

        // `contract_address` is provided, but `chain_id` is missed from the verification metadata
        let test_case = {
            let mut test_case = vyper_types::from_file::<Flattened>("simple");
            test_case.contract_address =
                Some("0x0123456789012345678901234567890123456789".to_string());
            test_case
        };
        test_success(test_case).await;
    }

    #[tokio::test]
    async fn blueprint_contract() {
        let mut test_case = vyper_types::from_file::<Flattened>("blueprint");
        test_success(test_case.clone()).await;

        test_case.use_deployed_bytecode = true;
        test_success(test_case).await;
    }

    #[tokio::test]
    async fn accepts_partially_matching_compiler_version_commit_hashes() {
        let initial_test_case = vyper_types::from_file::<Flattened>("simple");
        // provided commit hash is a prefix of the one used in the list
        let test_case = {
            let mut test_case = initial_test_case.clone();
            test_case.compiler_version.pop();
            test_case
        };
        let verification_response = get_verification_response(&test_case).await;
        validate_verification_response(&initial_test_case, verification_response);

        // the commit hash from the list is a prefix of the provided one
        let test_case = {
            let mut test_case = initial_test_case.clone();
            test_case.compiler_version.push_str("1234");
            test_case
        };
        let verification_response = get_verification_response(&test_case).await;
        validate_verification_response(&initial_test_case, verification_response);
    }
}

mod multi_part {
    use super::{test_success, vyper_types};
    use vyper_types::MultiPart;

    #[tokio::test]
    async fn verify_success() {
        for test_case_name in &["with_interfaces"] {
            let test_case = vyper_types::from_file::<MultiPart>(test_case_name);
            test_success(test_case).await;
        }
    }

    #[tokio::test]
    async fn accepts_partial_verification_metadata_in_input() {
        // `chain_id` is provided, but `contract_address` is missed from the verification metadata
        let test_case = {
            let mut test_case = vyper_types::from_file::<MultiPart>("with_interfaces");
            test_case.chain_id = Some("5".to_string());
            test_case
        };
        test_success(test_case).await;

        // `contract_address` is provided, but `chain_id` is missed from the verification metadata
        let test_case = {
            let mut test_case = vyper_types::from_file::<MultiPart>("with_interfaces");
            test_case.contract_address =
                Some("0x0123456789012345678901234567890123456789".to_string());
            test_case
        };
        test_success(test_case).await;
    }
}

mod standard_json {
    use super::{
        get_verification_response, test_success, validate_verification_response, vyper_types,
    };
    use crate::{test_error, test_failure};
    use actix_web::http::StatusCode;
    use vyper_types::StandardJson;

    #[tokio::test]
    async fn verify_success() {
        for test_case_name in &["standard_json_with_interfaces"] {
            let test_case = vyper_types::from_file::<StandardJson>(test_case_name);
            test_success(test_case).await;
        }
    }

    #[tokio::test]
    async fn verify_invalid_input() {
        let test_case = {
            let mut test_case =
                vyper_types::from_file::<StandardJson>("standard_json_with_interfaces");
            // Remove the latest "}" bracket to make the input json invalid
            test_case.input.remove(test_case.input.len() - 1);
            test_case
        };
        test_failure(test_case, "content is not a valid standard json").await;
    }

    #[tokio::test]
    async fn verify_bad_request() {
        let test_case = {
            let mut test_case =
                vyper_types::from_file::<StandardJson>("standard_json_with_interfaces");
            // Remove the latest "}" bracket to make the input json invalid
            test_case.compiler_version = "invalid_compiler_version".to_string();
            test_case
        };
        test_error(
            test_case,
            StatusCode::BAD_REQUEST,
            "invalid compiler version",
        )
        .await;
    }

    #[tokio::test]
    async fn accepts_partial_verification_metadata_in_input() {
        // `chain_id` is provided, but `contract_address` is missed from the verification metadata
        let test_case = {
            let mut test_case =
                vyper_types::from_file::<StandardJson>("standard_json_with_interfaces");
            test_case.chain_id = Some("5".to_string());
            test_case
        };
        test_success(test_case).await;

        // `contract_address` is provided, but `chain_id` is missed from the verification metadata
        let test_case = {
            let mut test_case =
                vyper_types::from_file::<StandardJson>("standard_json_with_interfaces");
            test_case.contract_address =
                Some("0x0123456789012345678901234567890123456789".to_string());
            test_case
        };
        test_success(test_case).await;
    }

    #[tokio::test]
    async fn interfaces_in_sources_from_0_4_0() {
        let test_case =
            vyper_types::from_file::<StandardJson>("standard_json_interfaces_in_sources");
        test_success(test_case.clone()).await;
    }

    #[tokio::test]
    async fn accepts_partially_matching_compiler_version_commit_hashes() {
        let initial_test_case =
            vyper_types::from_file::<StandardJson>("standard_json_interfaces_in_sources");
        // provided commit hash is a prefix of the one used in the list
        let test_case = {
            let mut test_case = initial_test_case.clone();
            test_case.compiler_version.pop();
            test_case
        };
        let verification_response = get_verification_response(&test_case).await;
        validate_verification_response(&initial_test_case, verification_response);

        // the commit hash from the list is a prefix of the provided one
        let test_case = {
            let mut test_case = initial_test_case.clone();
            test_case.compiler_version.push_str("1234");
            test_case
        };
        let verification_response = get_verification_response(&test_case).await;
        validate_verification_response(&initial_test_case, verification_response);
    }

    #[tokio::test]
    async fn verify_contracts_with_integrity_hashes_inside_cbor_auxdata() {
        let test_case =
            vyper_types::from_file::<StandardJson>("standard_json_contracts_with_integrity_hashes");
        test_success(test_case).await;
    }
}

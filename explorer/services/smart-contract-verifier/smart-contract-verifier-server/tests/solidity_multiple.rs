mod solidity_multiple_types;

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
use serde_json::json;
use smart_contract_verifier_proto::blockscout::smart_contract_verifier::v2::{
    solidity_verifier_actix::route_solidity_verifier, VerifyResponse,
};
use smart_contract_verifier_server::{Settings, SolidityVerifierService};
use solidity_multiple_types::TestInput;
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    str::{from_utf8, FromStr},
    sync::Arc,
};
use tokio::sync::{OnceCell, Semaphore};

const CONTRACTS_DIR: &str = "tests/contracts";
const ROUTE: &str = "/api/v2/verifier/solidity/sources:verify-multi-part";

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

async fn test_setup(
    dir: &str,
    input: &mut TestInput,
) -> (ServiceResponse, Option<String>, Option<serde_json::Value>) {
    let service = global_service().await;
    let app = test::init_service(
        App::new().configure(|config| route_solidity_verifier(config, service.clone())),
    )
    .await;

    let prefix = format!("{CONTRACTS_DIR}/{dir}");
    let suffix = if input.is_yul { "yul" } else { "sol" };
    let file_path = format!("{prefix}/source.{suffix}");

    input.file_path = Some(file_path.clone());
    input.source_code =
        Some(input.source_code.clone().unwrap_or_else(|| {
            fs::read_to_string(&file_path).expect("Error while reading source")
        }));
    input.creation_tx_input = if !input.ignore_creation_tx_input {
        Some(input.creation_tx_input.clone().unwrap_or_else(|| {
            fs::read_to_string(format!("{prefix}/creation_tx_input"))
                .expect("Error while reading creation_tx_input")
        }))
    } else {
        None
    };
    input.deployed_bytecode = Some(input.deployed_bytecode.clone().unwrap_or_else(|| {
        fs::read_to_string(format!("{prefix}/deployed_bytecode"))
            .expect("Error while reading deployed_bytecode")
    }));
    let expected_constructor_argument = input.has_constructor_args.then(|| {
        fs::read_to_string(format!("{prefix}/constructor_arguments"))
            .expect("Error while reading constructor_arguments")
    });

    let abi = {
        let path = PathBuf::from(format!("{prefix}/abi.json"));
        path.is_file().then(|| {
            let content = fs::read_to_string(path).expect("Error while reading abi");
            serde_json::Value::from_str(&content).expect("Error while deserializing abi")
        })
    };

    let (bytecode, bytecode_type) = if !input.ignore_creation_tx_input {
        (input.creation_tx_input.as_ref().unwrap(), "CREATION_INPUT")
    } else {
        (
            input.deployed_bytecode.as_ref().unwrap(),
            "DEPLOYED_BYTECODE",
        )
    };
    let request = json!({
        "bytecode": bytecode,
        "bytecodeType": bytecode_type,
        "compilerVersion": input.compiler_version,
        "sourceFiles": BTreeMap::from([(file_path, input.source_code.as_ref().unwrap())]),
        "evmVersion": input.evm_version,
        "libraries": input.contract_libraries,
        "optimizationRuns": input.optimization_runs
    });

    let response = TestRequest::post()
        .uri(ROUTE)
        .set_json(&request)
        .send_request(&app)
        .await;

    (response, expected_constructor_argument, abi)
}

async fn test_success(dir: &'static str, mut input: TestInput) -> VerifyResponse {
    let (response, expected_constructor_argument, expected_abi) = test_setup(dir, &mut input).await;

    // Assert that status code is success
    if !response.status().is_success() {
        let status = response.status();
        let body = read_body(response).await;
        let message = from_utf8(&body).expect("Read body as UTF-8");
        panic!("Invalid status code (success expected). Status: {status}. Messsage: {message}")
    }

    let verification_response: VerifyResponse = read_body_json(response).await;
    let verification_response_clone = verification_response.clone();

    assert_eq!(
        verification_response.status().as_str_name(),
        "SUCCESS", // success
        "Invalid verification status. Response: {:?}",
        verification_response
    );

    assert!(
        verification_response.source.is_some(),
        "Verification source is not Some"
    );
    assert!(
        verification_response.extra_data.is_some(),
        "Verification extra data is not Some"
    );

    let result_source = verification_response.source.expect("Checked above");
    let abi: Option<Result<serde_json::Value, _>> = result_source
        .abi
        .as_ref()
        .map(|abi| serde_json::from_str(abi));
    let file_path = input.file_path.expect("Set `Some` on test_setup");
    assert_eq!(result_source.file_name, file_path, "Invalid file path");
    assert_eq!(
        result_source.contract_name, input.contract_name,
        "Invalid contract name"
    );
    if !input.is_yul {
        assert!(abi.is_some(), "Solidity contracts must have abi");
        assert!(
            abi.as_ref().unwrap().is_ok(),
            "Abi deserialization failed: {}",
            abi.unwrap().as_ref().unwrap_err()
        );
        if let Some(expected_abi) = expected_abi {
            assert_eq!(
                &expected_abi,
                abi.as_ref().unwrap().as_ref().unwrap(),
                "Invalid abi"
            )
        }
        assert_eq!(
            result_source.source_type().as_str_name(),
            "SOLIDITY",
            "Invalid source type"
        );
    } else {
        assert_eq!(
            result_source.source_type().as_str_name(),
            "YUL",
            "Invalid source type"
        );
    }
    let result_constructor_arguments = result_source
        .constructor_arguments
        .map(|args| DisplayBytes::from_str(&args).unwrap());
    let expected_constructor_argument =
        expected_constructor_argument.map(|args| DisplayBytes::from_str(&args).unwrap());
    assert_eq!(
        result_constructor_arguments, expected_constructor_argument,
        "Invalid constructor args"
    );
    assert_eq!(
        result_source.compiler_version, input.compiler_version,
        "Invalid compiler version"
    );

    assert_eq!(
        result_source.libraries, input.contract_libraries,
        "Invalid contract libraries"
    );

    mod compiler_settings {
        use serde::Deserialize;

        #[derive(Default, Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub struct Optimizer {
            pub enabled: Option<bool>,
            pub runs: Option<i32>,
        }
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct CompilerSettings<'a> {
        pub evm_version: Option<&'a str>,
        pub libraries: BTreeMap<String, BTreeMap<String, String>>,
        #[serde(default)]
        pub optimizer: compiler_settings::Optimizer,
    }

    let result_compiler_settings: CompilerSettings =
        serde_json::from_str(&result_source.compiler_settings).unwrap_or_else(|_| {
            panic!(
                "Compiler Settings deserialization failed: {}",
                &result_source.compiler_settings
            )
        });
    assert_eq!(
        result_compiler_settings.evm_version.unwrap_or("default"),
        input.evm_version,
        "Invalid evm version"
    );
    assert_eq!(
        result_compiler_settings.libraries,
        BTreeMap::new(),
        "Invalid compiler settings libraries"
    );
    assert_eq!(
        result_compiler_settings.optimizer.enabled,
        Some(input.optimization_runs.is_some()),
        "Invalid optimization"
    );
    assert_eq!(
        result_compiler_settings.optimizer.runs,
        input.optimization_runs.or(Some(200)),
        "Invalid optimization runs"
    );
    assert_eq!(
        result_source.source_files.len(),
        1,
        "Invalid number of sources"
    );
    assert_eq!(
        result_source.source_files,
        BTreeMap::from([(
            file_path,
            input.source_code.expect("Set `Some` on test_setup")
        )]),
        "Invalid source files"
    );

    verification_response_clone
}

/// Test verification failures (note: do not handle 400 BadRequest responses)
async fn test_failure(dir: &str, mut input: TestInput, expected_message: &str) {
    let (response, _expected_constructor_argument, _abi) = test_setup(dir, &mut input).await;

    assert!(
        response.status().is_success(),
        "Invalid status code (success expected): {}",
        response.status()
    );

    let verification_response: VerifyResponse = read_body_json(response).await;

    assert_eq!(
        verification_response.status().as_str_name(),
        "FAILURE", // failure
        "Invalid verification status. Response: {:?}",
        verification_response
    );

    assert!(
        verification_response.source.is_none(),
        "In case of failure, source should be None"
    );
    assert!(
        verification_response.extra_data.is_none(),
        "In case of failure, extra data should be None"
    );

    assert!(
        verification_response.message.contains(expected_message),
        "Invalid message: {}",
        verification_response.message
    );
}

/// Test errors codes (handle 400 BadRequest, 500 InternalServerError and similar responses)
async fn test_error(
    dir: &str,
    mut input: TestInput,
    expected_status: StatusCode,
    expected_message: Option<&str>,
) {
    let (response, _expected_constructor_argument, _abi) = test_setup(dir, &mut input).await;

    let status = response.status();

    let body = read_body(response).await;
    let message = from_utf8(&body).expect("Read body as UTF-8");

    assert_eq!(
        status, expected_status,
        "Invalid status code. Message: {}",
        message
    );

    if let Some(expected_message) = expected_message {
        assert!(
            message.contains(expected_message),
            "Invalid message: {message}"
        );
    }
}

mod success_tests {
    use super::*;

    #[tokio::test]
    async fn verifies_the_generated_bytecode_against_bytecode_retrieved_from_the_blockchain() {
        let contract_dir = "simple_storage";
        let test_input = TestInput::new("SimpleStorage", "v0.4.24+commit.e67f0147");
        test_success(contract_dir, test_input).await;
    }

    #[tokio::test]
    async fn verifies_the_generated_bytecode_with_external_libraries() {
        let contract_dir = "contract_with_lib";
        let mut libraries = BTreeMap::new();
        libraries.insert(
            "tests/contracts/contract_with_lib/source.sol:BadSafeMath".to_string(),
            "0x9bca1bf2810c9b68f25c82e8ebb9dc0a5301e310".to_string(),
        );
        // let test_input = TestInput::new("SimpleStorage", "v0.5.11+commit.c082d0b4")
        let test_input = TestInput::new("SimpleStorage", "v0.5.11+commit.22be8592")
            .with_optimization_runs(200)
            .with_contract_libraries(libraries);
        test_success(contract_dir, test_input).await;
    }

    #[tokio::test]
    // `whisper` metadata - (bzz0 => bzz1) in solidity 0.5.11()
    async fn verifies_smart_contract_with_new_whisper_metadata() {
        let contract_dir = "solidity_5.11_new_whisper_metadata";
        let test_input = TestInput::new("FixedSupplyToken", "v0.5.11+commit.22be8592")
            .with_evm_version("byzantium");
        test_success(contract_dir, test_input).await;
    }

    #[tokio::test]
    async fn verifies_library() {
        let contract_dir = "library";
        // let test_input = TestInput::new("Foo", ""v0.5.11+commit.c082d0b4"")
        let test_input =
            TestInput::new("Foo", "v0.5.11+commit.22be8592").with_optimization_runs(200);
        test_success(contract_dir, test_input).await;
    }

    #[tokio::test]
    // (includes new metadata in bytecode)
    async fn verifies_smart_contract_compiled_with_solidity_0_5_9() {
        let contract_dir = "solidity_0.5.9_smart_contract";
        // let test_input = TestInput::new("TestToken", "v0.5.9+commit.e560f70d")
        let test_input = TestInput::new("TestToken", "v0.5.9+commit.c68bc34e")
            .with_evm_version("petersburg")
            .has_constructor_args();
        test_success(contract_dir, test_input).await;
    }

    #[tokio::test]
    // https://solidity.readthedocs.io/en/v0.6.6/contracts.html?highlight=immutable#constant-and-immutable-state-variables
    async fn verifies_smart_contract_with_immutable_assignment() {
        let contract_dir = "with_immutable_assignment";
        let test_input = TestInput::new("C", "v0.6.7+commit.b8d736ae")
            .with_optimization_runs(200)
            .has_constructor_args();
        test_success(contract_dir, test_input).await;
    }

    #[tokio::test]
    // verifies smart-contract created from another contract
    async fn contract_from_factory() {
        let contract_dir = "contract_from_factory";
        let test_input = TestInput::new("ContractFromFactory", "v0.4.26+commit.4563c3fc")
            .with_optimization_runs(200)
            .has_constructor_args();
        test_success(contract_dir, test_input).await;
    }

    #[tokio::test]
    async fn yul_contract() {
        let contract_dir = "yul";
        let test_input = TestInput::new("Proxy", "v0.8.7+commit.e28d00a7").set_is_yul();
        test_success(contract_dir, test_input).await;
    }

    #[tokio::test]
    async fn yul_erc20() {
        let contract_dir = "yul_erc20";
        let test_input = TestInput::new("Token", "v0.8.7+commit.e28d00a7").set_is_yul();
        test_success(contract_dir, test_input).await;
    }

    #[tokio::test]
    async fn solidity_0_4_10() {
        let contract_dir = "solidity_0.4.10";

        // For some reason v0.4.10 in default solc list for linux
        // has different commit hash from macos and js versions
        #[cfg(target_os = "linux")]
        let compiler_version = "v0.4.10+commit.9e8cc01b";
        #[cfg(not(target_os = "linux"))]
        let compiler_version = "v0.4.10+commit.f0d539ae";

        let test_input = TestInput::new("Main", compiler_version).has_constructor_args();
        test_success(contract_dir, test_input).await;
    }
}

mod failure_tests {
    use super::*;

    #[tokio::test]
    async fn returns_failure_when_bytecode_does_not_match() {
        let contract_dir = "simple_storage";
        let test_input = TestInput::new("SimpleStorage", "v0.4.24+commit.e67f0147")
            .with_source_code("pragma solidity ^0.4.24; contract SimpleStorage {}".to_string());
        test_failure(
            contract_dir,
            test_input,
            "No contract could be verified with provided data",
        )
        .await;
    }

    #[tokio::test]
    async fn returns_failure_with_compilation_problems() {
        let contract_dir = "simple_storage";
        let test_input = TestInput::new("SimpleStorage", "v0.4.24+commit.e67f0147")
            .with_source_code("pragma solidity ^0.4.24; contract SimpleStorage { ".to_string());
        test_failure(contract_dir, test_input, "ParserError").await;
    }

    // #[tokio::test]
    // async fn returns_compiler_version_mismatch() { TODO: uncomment when support for compiler version mismatch is returned
    // let contract_dir = "solidity_0.5.14";
    //
    // // Another version
    // let test_input = TestInput::new("A", "v0.5.15+commit.6a57276f");
    // test_failure(
    //     contract_dir,
    //     test_input,
    //     "Invalid compiler version: Expected 0.5.14, found 0.5.15",
    // )
    // .await;

    // Currently due to the nature of bytecodes comparing (see `base_verifier::compare_creation_tx_inputs`)
    // if on chain creation transaction input length is less than the local creation transaction input,
    // the verifier returns `NoMatchingContracts` error. Thus, the test case below would not work.
    //
    // TODO: see how difficult it would be to fix that

    // // Another nightly version
    // let settings_json = "{ \"solidity\": { \"fetcher\": { \"list\": { \"list_url\": \"https://raw.githubusercontent.com/blockscout/solc-bin/main/list.json\" } } } }";
    // let settings = serde_json::from_str(settings_json).expect("Settings is valid json");
    // let local_app_router = local_app_router(settings).await;
    // let test_input = TestInput::new("A", "v0.5.14-nightly.2019.12.10+commit.45aa7a88").with_app_router(local_app_router);
    // test_failure(contract_dir, test_input, "Invalid compiler version").await;
    // }
}

mod bad_request_error_tests {
    use super::*;

    #[tokio::test]
    async fn returns_failure_with_version_not_found() {
        let contract_dir = "simple_storage";
        let test_input = TestInput::new("SimpleStorage", "v0.4.40+commit.e67f0147");
        test_error(
            contract_dir,
            test_input,
            StatusCode::BAD_REQUEST,
            Some("Compiler version not found: v0.4.40+commit.e67f0147"),
        )
        .await;
    }
}

mod regression_tests {
    use super::*;

    // https://github.com/blockscout/blockscout/issues/5114
    #[tokio::test]
    async fn issue_5114() {
        let contract_dir = "issue_5114";
        let test_input = TestInput::new("TransparentUpgradeableProxy", "v0.8.2+commit.661d1103")
            .with_optimization_runs(200)
            .has_constructor_args();
        test_success(contract_dir, test_input).await;
    }

    // https://github.com/blockscout/blockscout/issues/5127
    #[tokio::test]
    async fn issue_5127() {
        let contract_dir = "issue_5127";
        let test_input = TestInput::new("YESToken", "v0.8.11+commit.d7f03943")
            .with_optimization_runs(200)
            .has_constructor_args();
        test_success(contract_dir, test_input).await;
    }

    // https://github.com/blockscout/blockscout/issues/3082
    #[tokio::test]
    async fn issue_3082() {
        let contract_dir = "issue_3082";
        let test_input = TestInput::new("Distribution", "v0.5.10+commit.5a6ea5b1")
            .with_optimization_runs(200)
            .has_constructor_args();
        test_success(contract_dir, test_input).await;
    }

    #[tokio::test]
    async fn another_failed_constructor_args_verification() {
        let contract_dir = "issue_with_constructor_args";
        let test_input = TestInput::new("ERC1967Proxy", "v0.8.2+commit.661d1103")
            .with_optimization_runs(200)
            .has_constructor_args();
        test_success(contract_dir, test_input).await;
    }

    // https://github.com/blockscout/blockscout/issues/4758
    #[tokio::test]
    async fn issue_4758() {
        let contract_dir = "issue_4758";
        let test_input = TestInput::new("CS3_1OnChainShop", "v0.8.4+commit.c7e474f2")
            .with_optimization_runs(200)
            .has_constructor_args();
        test_success(contract_dir, test_input).await;
    }

    // runs only for linux, as other compiler lists do not have nightly builds
    #[cfg(target_os = "linux")]
    // https://github.com/blockscout/blockscout/issues/5430
    // https://github.com/blockscout/blockscout/issues/5434
    #[tokio::test]
    #[ignore] // remove when list with nightly builds would be ready for linux
    async fn issue_5430_5434() {
        let contract_dir = "issue_5430_5434";
        let test_input = TestInput::new("C", "v0.8.14-nightly.2022.4.13+commit.25923c1f");
        test_success(contract_dir, test_input).await;
    }

    // https://github.com/blockscout/blockscout/issues/5431
    #[tokio::test]
    // (smart contract was compiled with bytecodeHash=none; constructor with arguments)
    async fn issue_5431() {
        let contract_dir = "issue_5431";
        let test_input = TestInput::new("Owner", "v0.8.8+commit.dddeac2f").has_constructor_args();
        test_success(contract_dir, test_input).await;
    }

    // https://github.com/blockscout/blockscout/issues/5636
    #[tokio::test]
    // type(A).creationCode in the constructor
    async fn issue_5636() {
        let contract_dir = "issue_5636";
        let test_input = TestInput::new("B", "v0.8.14+commit.80d49f37").with_optimization_runs(200);
        test_success(contract_dir, test_input).await;
    }
}

mod tests_from_constructor_arguments_test_exs {
    use super::*;

    #[tokio::test]
    async fn verifies_with_require_messages() {
        let contract_dir = "home_bridge";
        let test_input = TestInput::new("HomeBridge", "v0.5.8+commit.23d335f2")
            .with_optimization_runs(200)
            .has_constructor_args();
        test_success(contract_dir, test_input).await;
    }

    #[tokio::test]
    async fn verifies_with_string_in_keccak256() {
        let contract_dir = "ERC677";
        let test_input = TestInput::new("ERC677MultiBridgeToken", "v0.5.10+commit.5a6ea5b1")
            .with_optimization_runs(200)
            .has_constructor_args();
        test_success(contract_dir, test_input).await;
    }
}

mod tests_without_creation_tx_input {
    use super::*;

    #[tokio::test]
    async fn verifies_contract_via_deployed_bytecode() {
        let contract_dir = "solidity_0.5.14";
        let test_input = TestInput::new("A", "v0.5.14+commit.01f1aaa4").ignore_creation_tx_input();
        test_success(contract_dir, test_input).await;
    }

    #[tokio::test]
    async fn verifies_contract_with_constructor_args_in_abi() {
        let contract_dir = "solidity_0.5.9_smart_contract";
        let test_input = TestInput::new("TestToken", "v0.5.9+commit.c68bc34e")
            .with_evm_version("petersburg")
            .ignore_creation_tx_input();
        test_success(contract_dir, test_input).await;
    }

    // // Fails as deployed bytecode for both "A" and "B" contracts is the same (
    // // the only difference is constructor which does not make sense for deployed bytecode)
    // #[tokio::test]
    // async fn verifies_contract_with_several_metadata_hashes() {
    //     let contract_dir = "issue_5636";
    //     let test_input = TestInput::new("B", "v0.8.14+commit.80d49f37").with_optimization_runs(200).ignore_creation_tx_input();
    //     test_success(contract_dir, test_input).await;
    // }

    // Libraries have the address they are deployed at in the beginning of deployed bytecode,
    // while compiler fills those bytes with zeros. Thus, we cannot verify libraries via deployed bytecode only.
    #[tokio::test]
    async fn library_verification_fails() {
        let contract_dir = "library";
        let test_input = TestInput::new("Foo", "v0.5.11+commit.22be8592")
            .with_optimization_runs(200)
            .ignore_creation_tx_input();
        test_failure(
            contract_dir,
            test_input,
            "No contract could be verified with provided data",
        )
        .await;
    }
}

mod bytecode_parts_tests {
    use super::*;
    use smart_contract_verifier_proto::blockscout::smart_contract_verifier::v2::verify_response::extra_data::BytecodePart;

    #[tokio::test]
    async fn one_main_one_meta() {
        let contract_dir = "simple_storage";
        let test_input = TestInput::new("SimpleStorage", "v0.4.24+commit.e67f0147");
        let extra_data = test_success(contract_dir, test_input)
            .await
            .extra_data
            .expect("Was unpacked successfully inside test_success");

        let expected_creation_tx_input_parts = vec![
            BytecodePart { r#type: "main".into(), data: "0x608060405234801561001057600080fd5b5060df8061001f6000396000f3006080604052600436106049576000357c0100000000000000000000000000000000000000000000000000000000900463ffffffff16806360fe47b114604e5780636d4ce63c146078575b600080fd5b348015605957600080fd5b5060766004803603810190808035906020019092919050505060a0565b005b348015608357600080fd5b50608a60aa565b6040518082815260200191505060405180910390f35b8060008190555050565b600080549050905600".into() },
            BytecodePart { r#type: "meta".into(), data: "0xa165627a7a72305820b127de36a4e02cfe83fe4ccce7cfdbe00e4a2da70d71c3b2d0be5097bcfb94c80029".into() }
        ];
        super::assert_eq!(
            expected_creation_tx_input_parts,
            extra_data.local_creation_input_parts,
            "Invalid creation tx input parts"
        );

        let expected_deployed_bytecode_parts = vec![
            BytecodePart { r#type: "main".into(), data: "0x6080604052600436106049576000357c0100000000000000000000000000000000000000000000000000000000900463ffffffff16806360fe47b114604e5780636d4ce63c146078575b600080fd5b348015605957600080fd5b5060766004803603810190808035906020019092919050505060a0565b005b348015608357600080fd5b50608a60aa565b6040518082815260200191505060405180910390f35b8060008190555050565b600080549050905600".into() },
            BytecodePart { r#type: "meta".into(), data: "0xa165627a7a72305820b127de36a4e02cfe83fe4ccce7cfdbe00e4a2da70d71c3b2d0be5097bcfb94c80029".into() }
        ];
        super::assert_eq!(
            expected_deployed_bytecode_parts,
            extra_data.local_deployed_bytecode_parts,
            "Invalid deployed bytecode parts"
        );
    }

    #[tokio::test]
    async fn two_main_two_meta() {
        let contract_dir = "issue_5636";
        let test_input = TestInput::new("B", "v0.8.14+commit.80d49f37").with_optimization_runs(200);
        let extra_data = test_success(contract_dir, test_input)
            .await
            .extra_data
            .expect("Was unpacked successfully inside test_success");

        let expected_creation_tx_input_parts = vec![
            BytecodePart { r#type: "main".into(), data: "0x608060405234801561001057600080fd5b506040516100206020820161004e565b601f1982820381018352601f909101166040528051610048916000916020919091019061005a565b5061012d565b605c8061017a83390190565b828054610066906100f3565b90600052602060002090601f01602090048101928261008857600085556100ce565b82601f106100a157805160ff19168380011785556100ce565b828001600101855582156100ce579182015b828111156100ce5782518255916020019190600101906100b3565b506100da9291506100de565b5090565b5b808211156100da57600081556001016100df565b600181811c9082168061010757607f821691505b60208210810361012757634e487b7160e01b600052602260045260246000fd5b50919050565b603f8061013b6000396000f3fe6080604052600080fdfe".into() },
            BytecodePart { r#type: "meta".into(), data: "0xa26469706673582212205c9c5bb56fb32b38e31f567bf368712fd0bd017cf3b36663c99b9fa32ddf41ae64736f6c634300080e0033".into() },
            BytecodePart { r#type: "main".into(), data: "0x6080604052348015600f57600080fd5b50603f80601d6000396000f3fe6080604052600080fdfe".into() },
            BytecodePart { r#type: "meta".into(), data: "0xa2646970667358221220708123f84ee8016bdaaab1461b231024c52e14bd1f9c02b522c3c057528434dd64736f6c634300080e0033".into() }
        ];
        super::assert_eq!(
            expected_creation_tx_input_parts,
            extra_data.local_creation_input_parts,
            "Invalid creation tx input parts"
        );

        let expected_deployed_bytecode_parts = vec![
            BytecodePart { r#type: "main".into(), data: "0x6080604052600080fdfe".into() },
            BytecodePart { r#type: "meta".into(), data: "0xa26469706673582212205c9c5bb56fb32b38e31f567bf368712fd0bd017cf3b36663c99b9fa32ddf41ae64736f6c634300080e0033".into() }
        ];
        super::assert_eq!(
            expected_deployed_bytecode_parts,
            extra_data.local_deployed_bytecode_parts,
            "Invalid deployed bytecode parts"
        );
    }
}

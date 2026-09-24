use blockscout_service_launcher::{
    launcher::ConfigSettings,
    test_server::{get_test_server_settings, init_server, send_post_request},
};
use foundry_compilers::{
    artifacts::{CompilerOutput, EvmVersion, Severity, SolcInput, Source},
    solc::Solc,
};
use lazy_static::lazy_static;
use rstest::rstest;
use serde::Deserialize;
use smart_contract_verifier_proto::blockscout::smart_contract_verifier::v2::{
    LookupMethodsRequest, LookupMethodsResponse,
};
use smart_contract_verifier_server::Settings;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};
use tokio::sync::Semaphore;

const ROUTE: &str = "/api/v2/verifier/solidity/methods:lookup";

lazy_static! {
    static ref COMPILER_LOCK: Semaphore = Semaphore::new(1);
}

fn process_compiler_output(
    output: &CompilerOutput,
    contract_name: &str,
) -> anyhow::Result<(LookupMethodsRequest, BTreeMap<String, String>)> {
    let (_, contract) = output
        .contracts_iter()
        .find(|(name, _)| *name == contract_name)
        .ok_or_else(|| anyhow::anyhow!("contract not found"))?;
    let evm = contract.evm.as_ref().expect("evm included");
    let deployed_bytecode = evm
        .deployed_bytecode
        .as_ref()
        .expect("deployed bytecode included")
        .bytecode
        .as_ref()
        .expect("bytecode included");
    let methods = evm.method_identifiers.clone();

    let bytecode = deployed_bytecode
        .object
        .clone()
        .into_bytes()
        .unwrap()
        .to_string();
    let abi = serde_json::to_string(&contract.abi.clone().expect("abi included"))?;
    let source_map = deployed_bytecode
        .source_map
        .as_ref()
        .expect("srcmap included")
        .clone();
    let file_ids = output
        .sources
        .iter()
        .map(|(name, file)| (file.id, name.to_string_lossy().into_owned()))
        .collect();

    let request = LookupMethodsRequest {
        abi,
        bytecode,
        file_ids,
        source_map,
    };
    Ok((request, methods))
}

#[derive(Deserialize)]
struct TestCase {
    version: String,
    contract_name: String,
}

#[rstest]
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_lookup_methods(#[files("tests/test_cases_lookup_methods/*")] test_dir: PathBuf) {
    let mut settings = Settings::build().expect("Failed to build settings");
    let (server_settings, base) = get_test_server_settings();
    settings.server = server_settings;
    settings.vyper.enabled = false;
    settings.solidity.enabled = true;
    settings.sourcify.enabled = false;
    settings.jaeger.enabled = false;
    settings.tracing.enabled = false;

    init_server(|| smart_contract_verifier_server::run(settings), &base).await;

    let test_case: TestCase = serde_json::from_str(
        std::fs::read_to_string(test_dir.join("config.json"))
            .unwrap()
            .as_str(),
    )
    .expect("Failed to parse test case");

    let solc = {
        let _permit = COMPILER_LOCK.acquire().await.unwrap();
        let version =
            semver::Version::parse(&test_case.version).expect("Cannot parse solidity version");
        Solc::find_or_install(&version).expect("failed to install version")
    };

    let sources = Source::read_sol_yul_from(&test_dir).expect("failed to read dir");
    let inputs = SolcInput::resolve_and_build(sources, Default::default());
    let input = inputs[0].clone().evm_version(EvmVersion::London);

    let output = solc.compile(&input).expect("failed to compile");
    let errors = output
        .errors
        .iter()
        .filter(|e| e.severity == Severity::Error)
        .collect::<Vec<_>>();
    if !errors.is_empty() {
        panic!("errors during compilation: {errors:?}");
    }

    let (request, methods) = process_compiler_output(&output, &test_case.contract_name).unwrap();
    let res: LookupMethodsResponse = send_post_request(&base, ROUTE, &request).await;

    // Make sure we extracted all methods
    assert_eq!(
        methods.values().collect::<BTreeSet<&String>>(),
        res.methods.keys().collect::<BTreeSet<&String>>()
    );
}

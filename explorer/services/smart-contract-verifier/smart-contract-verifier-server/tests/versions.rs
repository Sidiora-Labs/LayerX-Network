use actix_web::{
    test,
    test::{read_body, read_body_json, TestRequest},
    App,
};
use smart_contract_verifier_proto::blockscout::smart_contract_verifier::v2::{
    solidity_verifier_actix::route_solidity_verifier, vyper_verifier_actix::route_vyper_verifier,
    ListCompilerVersionsResponse,
};
use smart_contract_verifier_server::{Settings, SolidityVerifierService, VyperVerifierService};
use std::{str::from_utf8, sync::Arc};
use tokio::sync::Semaphore;

async fn test_versions(uri: &str) {
    let settings = Settings::default();
    let compilers_lock = Arc::new(Semaphore::new(settings.compilers.max_threads.get()));

    let solidity_service = SolidityVerifierService::new(settings.solidity, compilers_lock.clone())
        .await
        .expect("couldn't initialize solidity service");
    let vyper_service = VyperVerifierService::new(settings.vyper, compilers_lock.clone())
        .await
        .expect("couldn't initialize vyper service");

    let app = test::init_service(
        App::new()
            .configure(|config| route_solidity_verifier(config, Arc::new(solidity_service)))
            .configure(|config| route_vyper_verifier(config, Arc::new(vyper_service))),
    )
    .await;

    let response = TestRequest::get().uri(uri).send_request(&app).await;

    // Assert that status code is success
    if !response.status().is_success() {
        let status = response.status();
        let body = read_body(response).await;
        let message = from_utf8(&body).expect("Read body as UTF-8");
        panic!("Invalid status code (success expected). Status: {status}. Message: {message}")
    }

    let versions_response: ListCompilerVersionsResponse = read_body_json(response).await;
    assert!(
        !versions_response.compiler_versions.is_empty(),
        "List of versions is empty"
    )
}

#[tokio::test]
async fn solidity() {
    test_versions("/api/v2/verifier/solidity/versions").await;
}

#[tokio::test]
async fn vyper() {
    test_versions("/api/v2/verifier/vyper/versions").await;
}

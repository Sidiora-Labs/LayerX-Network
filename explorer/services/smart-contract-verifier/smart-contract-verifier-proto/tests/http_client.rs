use smart_contract_verifier_proto::{
    blockscout::smart_contract_verifier::v2 as proto,
    http_client::{
        mock, solidity_verifier_client, sourcify_verifier_client, vyper_verifier_client, Client,
        Config,
    },
};
use tonic::Response;

macro_rules! set_expectation {
    ($mock_service:expr, $expect:ident, $request:ty, $response:ty) => {
        $mock_service
            .$expect()
            .withf(|request| request.get_ref() == &<$request>::default())
            .returning(move |_| Ok(Response::new(<$response>::default())));
        // .once();
    };
}

async fn build_client(server: mock::SmartContractVerifierServer) -> Client {
    let server_addr = server.start().await;
    let config = Config::new(format!("http://{server_addr}"));
    Client::new(config).await
}

#[tokio::test]
async fn solidity_service() {
    let mock_service = {
        let mut mock_service = mock::MockSolidityVerifierService::default();
        set_expectation!(
            mock_service,
            expect_verify_multi_part,
            proto::VerifySolidityMultiPartRequest,
            proto::VerifyResponse
        );
        set_expectation!(
            mock_service,
            expect_verify_standard_json,
            proto::VerifySolidityStandardJsonRequest,
            proto::VerifyResponse
        );
        set_expectation!(
            mock_service,
            expect_batch_verify_multi_part,
            proto::BatchVerifySolidityMultiPartRequest,
            proto::BatchVerifyResponse
        );
        set_expectation!(
            mock_service,
            expect_batch_verify_standard_json,
            proto::BatchVerifySolidityStandardJsonRequest,
            proto::BatchVerifyResponse
        );
        set_expectation!(
            mock_service,
            expect_list_compiler_versions,
            proto::ListCompilerVersionsRequest,
            proto::ListCompilerVersionsResponse
        );
        set_expectation!(
            mock_service,
            expect_lookup_methods,
            proto::LookupMethodsRequest,
            proto::LookupMethodsResponse
        );

        mock_service
    };

    let client =
        build_client(mock::SmartContractVerifierServer::new().solidity_service(mock_service)).await;

    assert!(solidity_verifier_client::verify_multi_part(
        &client,
        proto::VerifySolidityMultiPartRequest::default()
    )
    .await
    .is_ok());
    assert!(solidity_verifier_client::verify_standard_json(
        &client,
        proto::VerifySolidityStandardJsonRequest::default()
    )
    .await
    .is_ok());
    assert!(solidity_verifier_client::batch_verify_multi_part(
        &client,
        proto::BatchVerifySolidityMultiPartRequest::default()
    )
    .await
    .is_ok());
    assert!(solidity_verifier_client::batch_verify_standard_json(
        &client,
        proto::BatchVerifySolidityStandardJsonRequest::default()
    )
    .await
    .is_ok());
    assert!(solidity_verifier_client::list_compiler_versions(
        &client,
        proto::ListCompilerVersionsRequest::default()
    )
    .await
    .is_ok());
    assert!(solidity_verifier_client::lookup_methods(
        &client,
        proto::LookupMethodsRequest::default()
    )
    .await
    .is_ok());
}

#[tokio::test]
async fn vyper_service() {
    let mock_service = {
        let mut mock_service = mock::MockVyperVerifierService::default();
        set_expectation!(
            mock_service,
            expect_verify_multi_part,
            proto::VerifyVyperMultiPartRequest,
            proto::VerifyResponse
        );
        set_expectation!(
            mock_service,
            expect_verify_standard_json,
            proto::VerifyVyperStandardJsonRequest,
            proto::VerifyResponse
        );
        set_expectation!(
            mock_service,
            expect_list_compiler_versions,
            proto::ListCompilerVersionsRequest,
            proto::ListCompilerVersionsResponse
        );

        mock_service
    };

    let client =
        build_client(mock::SmartContractVerifierServer::new().vyper_service(mock_service)).await;

    assert!(vyper_verifier_client::verify_multi_part(
        &client,
        proto::VerifyVyperMultiPartRequest::default()
    )
    .await
    .is_ok());
    assert!(vyper_verifier_client::verify_standard_json(
        &client,
        proto::VerifyVyperStandardJsonRequest::default()
    )
    .await
    .is_ok());
    assert!(vyper_verifier_client::list_compiler_versions(
        &client,
        proto::ListCompilerVersionsRequest::default()
    )
    .await
    .is_ok());
}

#[tokio::test]
async fn sourcify_service() {
    let mock_service = {
        let mut mock_service = mock::MockSourcifyVerifierService::default();
        set_expectation!(
            mock_service,
            expect_verify,
            proto::VerifySourcifyRequest,
            proto::VerifyResponse
        );
        set_expectation!(
            mock_service,
            expect_verify_from_etherscan,
            proto::VerifyFromEtherscanSourcifyRequest,
            proto::VerifyResponse
        );

        mock_service
    };

    let client =
        build_client(mock::SmartContractVerifierServer::new().sourcify_service(mock_service)).await;

    assert!(
        sourcify_verifier_client::verify(&client, proto::VerifySourcifyRequest::default())
            .await
            .is_ok()
    );
    assert!(sourcify_verifier_client::verify_from_etherscan(
        &client,
        proto::VerifyFromEtherscanSourcifyRequest::default()
    )
    .await
    .is_ok());
}

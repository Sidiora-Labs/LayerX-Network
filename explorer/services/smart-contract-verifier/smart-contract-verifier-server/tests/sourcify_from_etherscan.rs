use actix_web::{test, test::TestRequest, App};
use pretty_assertions::assert_eq;
use serde_json::json;
use smart_contract_verifier_proto::blockscout::smart_contract_verifier::v2::{
    sourcify_verifier_actix::route_sourcify_verifier, VerifyResponse,
};
use smart_contract_verifier_server::{Settings, SourcifyVerifierService};
use std::sync::Arc;

const ROUTE: &str = "/api/v2/verifier/sourcify/sources:verify-from-etherscan";

async fn init_service() -> Arc<SourcifyVerifierService> {
    let mut settings = Settings::default();
    settings.sourcify.verification_attempts = std::num::NonZeroU32::new(5).unwrap();
    let service = SourcifyVerifierService::new(settings.sourcify)
        .await
        .expect("couldn't initialize the service");
    Arc::new(service)
}

// #[tokio::test]
// async fn should_return_200() {
//     let address = "0x20f6a0edCE30681CDE6debAa58ed9768E42d1899";
//     let chain_id = "5";
//
//     let service = init_service().await;
//     let app = test::init_service(
//         App::new().configure(|config| route_sourcify_verifier(config, service.clone())),
//     )
//     .await;
//
//     let request_body = json!({
//         // relies on the fact that the Ethereum Testnet Goerli has this contract
//             // https://goerli.etherscan.io/address/0x20f6a0edCE30681CDE6debAa58ed9768E42d1899#code
//         "address": address,
//         "chain": chain_id,
//     });
//
//     let resp = TestRequest::post()
//         .uri(ROUTE)
//         .set_json(&request_body)
//         .send_request(&app)
//         .await;
//
//     assert!(
//         resp.status().is_success(),
//         "failed to verify contract, status is {}",
//         resp.status()
//     );
//
//     let body: serde_json::Value = test::read_body_json(resp).await;
//     assert_eq!(
//         body,
//         json!({
//             "message": "OK",
//             "status": "SUCCESS",
//             "source": {
//                 "fileName": "C1.sol",
//                 "contractName": "C1",
//                 "compilerVersion": "0.8.7+commit.e28d00a7",
//                 "constructorArguments": null,
//                 "abi": "[{\"inputs\":[],\"name\":\"a\",\"outputs\":[{\"internalType\":\"uint256\",\"name\":\"\",\"type\":\"uint256\"}],\"stateMutability\":\"view\",\"type\":\"function\"},{\"inputs\":[],\"name\":\"b\",\"outputs\":[{\"internalType\":\"uint256\",\"name\":\"\",\"type\":\"uint256\"}],\"stateMutability\":\"view\",\"type\":\"function\"},{\"inputs\":[],\"name\":\"c\",\"outputs\":[{\"internalType\":\"address\",\"name\":\"\",\"type\":\"address\"}],\"stateMutability\":\"view\",\"type\":\"function\"},{\"inputs\":[],\"name\":\"d\",\"outputs\":[{\"internalType\":\"address\",\"name\":\"\",\"type\":\"address\"}],\"stateMutability\":\"view\",\"type\":\"function\"}]",
//                 "sourceFiles": {
//                     "C1.sol": "// SPDX-License-Identifier: MIT\r\npragma solidity 0.8.7;\r\n\r\ncontract C1 {\r\n    uint256 immutable public a = 0;\r\n    uint256 public b = 1202;\r\n    address public c = 0x9563Fdb01BFbF3D6c548C2C64E446cb5900ACA88;\r\n    address public d = 0x9563Fdb01BFbF3D6c548C2C64E446cb5900ACA88;\r\n}"
//                 },
//                 "compilerSettings": "{\"evmVersion\":\"london\",\"libraries\":{},\"metadata\":{\"bytecodeHash\":\"ipfs\"},\"optimizer\":{\"enabled\":false,\"runs\":200},\"remappings\":[]}",
//                 "matchType": "PARTIAL",
//                 "sourceType": "SOLIDITY",
//                 "compilationArtifacts": null,
//                 "creationInputArtifacts": null,
//                 "deployedBytecodeArtifacts": null
//             },
//             "extraData": {
//                 "localCreationInputParts": [],
//                 "localDeployedBytecodeParts": [],
//             }
//         }),
//     );
// }

#[tokio::test]
async fn chain_not_supported_fail() {
    let address = "0xcb566e3B6934Fa77258d68ea18E931fa75e1aaAa";
    let chain_id = "2221";

    let service = init_service().await;
    let app = test::init_service(
        App::new().configure(|config| route_sourcify_verifier(config, service.clone())),
    )
    .await;

    let request_body = json!({
        "address": address,
        "chain": chain_id,
    });

    let resp = TestRequest::post()
        .uri(ROUTE)
        .set_json(&request_body)
        .send_request(&app)
        .await;

    assert!(
        resp.status().is_success(),
        "failed to verify contract, status is {}",
        resp.status()
    );

    let body: VerifyResponse = test::read_body_json(resp).await;

    assert_eq!(body.status().as_str_name(), "FAILURE");

    let error_message = "is not supported for importing from Etherscan";
    assert!(
        body.message.contains(error_message),
        "body message: {}, expected message: {}",
        body.message,
        error_message
    );
}

// #[tokio::test]
// async fn contract_not_verified_fail() {
//     let address = "0x847F2d0c193E90963aAD7B2791aAE8d7310dFF6A";
//     let chain_id = "5";
//
//     let service = init_service().await;
//     let app = test::init_service(
//         App::new().configure(|config| route_sourcify_verifier(config, service.clone())),
//     )
//     .await;
//
//     let request_body = json!({
//         "address": address,
//         "chain": chain_id,
//     });
//
//     let resp = TestRequest::post()
//         .uri(ROUTE)
//         .set_json(&request_body)
//         .send_request(&app)
//         .await;
//
//     if !resp.status().is_success() {
//         let status = resp.status();
//         let body = test::read_body(resp).await;
//         let message = std::str::from_utf8(&body).unwrap();
//         panic!("failed to verify contract, status={status}, message={message}");
//     }
//
//     let body: VerifyResponse = test::read_body_json(resp).await;
//
//     assert_eq!(body.status().as_str_name(), "FAILURE");
//
//     let error_message = "contract is not verified on Etherscan";
//     assert!(
//         body.message.contains(error_message),
//         "body message: {}, expected message: {}",
//         body.message,
//         error_message
//     );
// }

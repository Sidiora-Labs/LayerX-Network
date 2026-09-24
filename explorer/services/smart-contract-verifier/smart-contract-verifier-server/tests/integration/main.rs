mod batch_solidity;
mod transformations;
mod types;

/************************************************/

use std::str::from_utf8;
use std::sync::Arc;
use actix_web::dev::ServiceResponse;
use actix_web::{App, test};
use actix_web::test::{read_body, read_body_json, TestRequest};
use tokio::sync::{OnceCell, Semaphore};
use smart_contract_verifier_proto::blockscout::smart_contract_verifier::v2::solidity_verifier_actix::route_solidity_verifier;
use smart_contract_verifier_server::{Settings, SolidityVerifierService};
use types::{TestCaseRequest, TestCaseResponse};

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

async fn test_setup<Request: TestCaseRequest>(test_case: &Request) -> ServiceResponse {
    let service = global_service().await;
    let app = test::init_service(
        App::new().configure(|config| route_solidity_verifier(config, service.clone())),
    )
    .await;

    TestRequest::post()
        .uri(Request::route())
        .set_json(test_case.to_request())
        .send_request(&app)
        .await
}

async fn test_success<Request, Response>(test_case_request: &Request, test_case_response: &Response)
where
    Request: TestCaseRequest,
    Response: TestCaseResponse,
{
    let response = test_setup(test_case_request).await;
    if !response.status().is_success() {
        let status = response.status();
        let body = read_body(response).await;
        let message = from_utf8(&body).expect("Read body as UTF-8");
        panic!("Invalid status code (success expected). Status: {status}. Messsage: {message}")
    }

    test_case_response.check(read_body_json(response).await);
}

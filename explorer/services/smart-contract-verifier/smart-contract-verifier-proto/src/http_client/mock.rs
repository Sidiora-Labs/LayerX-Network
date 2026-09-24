use crate::blockscout::smart_contract_verifier::v2::{
    solidity_verifier_actix::route_solidity_verifier, solidity_verifier_server::SolidityVerifier,
    sourcify_verifier_actix::route_sourcify_verifier, sourcify_verifier_server::SourcifyVerifier,
    vyper_verifier_actix::route_vyper_verifier, vyper_verifier_server::VyperVerifier,
    BatchVerifyResponse, BatchVerifySolidityMultiPartRequest,
    BatchVerifySolidityStandardJsonRequest, ListCompilerVersionsRequest,
    ListCompilerVersionsResponse, LookupMethodsRequest, LookupMethodsResponse,
    VerifyFromEtherscanSourcifyRequest, VerifyResponse, VerifySolidityMultiPartRequest,
    VerifySolidityStandardJsonRequest, VerifySourcifyRequest, VerifyVyperMultiPartRequest,
    VerifyVyperStandardJsonRequest,
};
use mockall::mock;
use std::{net::SocketAddr, sync::Arc};

mock! {
    pub SolidityVerifierService {}

    #[async_trait::async_trait]
    impl SolidityVerifier for SolidityVerifierService {
        async fn verify_multi_part(&self, request: tonic::Request<VerifySolidityMultiPartRequest>) -> Result<tonic::Response<VerifyResponse>, tonic::Status>;

        async fn verify_standard_json(&self, request: tonic::Request<VerifySolidityStandardJsonRequest>) -> Result<tonic::Response<VerifyResponse>, tonic::Status>;

        async fn batch_verify_multi_part(&self, request: tonic::Request<BatchVerifySolidityMultiPartRequest>) -> Result<tonic::Response<BatchVerifyResponse>, tonic::Status>;

        async fn batch_verify_standard_json(&self, request: tonic::Request<BatchVerifySolidityStandardJsonRequest>) -> Result<tonic::Response<BatchVerifyResponse>, tonic::Status>;

        async fn list_compiler_versions(&self, request: tonic::Request<ListCompilerVersionsRequest>) -> Result<tonic::Response<ListCompilerVersionsResponse>, tonic::Status>;

        async fn lookup_methods(&self,request: tonic::Request<LookupMethodsRequest>) -> Result<tonic::Response<LookupMethodsResponse>, tonic::Status>;
    }
}

mock! {
    #[derive(Clone)]
    pub VyperVerifierService {}

    #[async_trait::async_trait]
    impl VyperVerifier for VyperVerifierService {
        async fn verify_multi_part(&self, request: tonic::Request<VerifyVyperMultiPartRequest>) -> Result<tonic::Response<VerifyResponse>, tonic::Status>;

        async fn verify_standard_json(&self, request: tonic::Request<VerifyVyperStandardJsonRequest>) -> Result<tonic::Response<VerifyResponse>, tonic::Status>;

        async fn list_compiler_versions(&self, request: tonic::Request<ListCompilerVersionsRequest>) -> Result<tonic::Response<ListCompilerVersionsResponse>, tonic::Status>;
    }
}

mock! {
    #[derive(Clone)]
    pub SourcifyVerifierService {}

    #[async_trait::async_trait]
    impl SourcifyVerifier for SourcifyVerifierService {
        async fn verify(&self, request: tonic::Request<VerifySourcifyRequest>) -> Result<tonic::Response<VerifyResponse>, tonic::Status>;

        async fn verify_from_etherscan(&self, request: tonic::Request<VerifyFromEtherscanSourcifyRequest>) -> Result<tonic::Response<VerifyResponse>, tonic::Status>;
    }
}

#[derive(Default, Clone)]
pub struct SmartContractVerifierServer {
    solidity_service: Option<Arc<MockSolidityVerifierService>>,
    vyper_service: Option<Arc<MockVyperVerifierService>>,
    sourcify_service: Option<Arc<MockSourcifyVerifierService>>,
}

impl SmartContractVerifierServer {
    pub fn new() -> Self {
        Self {
            solidity_service: None,
            vyper_service: None,
            sourcify_service: None,
        }
    }

    pub fn solidity_service(mut self, solidity_service: MockSolidityVerifierService) -> Self {
        self.solidity_service = Some(Arc::new(solidity_service));
        self
    }

    pub fn vyper_service(mut self, vyper_service: MockVyperVerifierService) -> Self {
        self.vyper_service = Some(Arc::new(vyper_service));
        self
    }

    pub fn sourcify_service(mut self, sourcify_service: MockSourcifyVerifierService) -> Self {
        self.sourcify_service = Some(Arc::new(sourcify_service));
        self
    }

    pub async fn start(&self) -> SocketAddr {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let solidity_service = self.solidity_service.clone();
        let vyper_service = self.vyper_service.clone();
        let sourcify_service = self.sourcify_service.clone();

        let configure_router = move |service_config: &mut actix_web::web::ServiceConfig| {
            if let Some(solidity) = solidity_service.clone() {
                service_config.configure(|config| route_solidity_verifier(config, solidity));
            }
            if let Some(vyper) = vyper_service.clone() {
                service_config.configure(|config| route_vyper_verifier(config, vyper));
            }
            if let Some(sourcify) = sourcify_service.clone() {
                service_config.configure(|config| route_sourcify_verifier(config, sourcify));
            }
        };

        let server =
            actix_web::HttpServer::new(move || actix_web::App::new().configure(&configure_router))
                .listen(listener)
                .expect("failed to bind server")
                .run();
        tokio::spawn(server);

        addr
    }
}

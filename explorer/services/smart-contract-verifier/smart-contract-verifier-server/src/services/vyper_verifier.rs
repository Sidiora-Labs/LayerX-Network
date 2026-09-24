use crate::{
    metrics,
    proto::{
        vyper_verifier_server::VyperVerifier, ListCompilerVersionsRequest,
        ListCompilerVersionsResponse, VerifyResponse, VerifyVyperMultiPartRequest,
        VerifyVyperStandardJsonRequest,
    },
    services::common,
    settings::VyperSettings,
    types,
};
use anyhow::Context;
use smart_contract_verifier::{vyper, EvmCompilersPool, VyperCompiler};
use std::sync::Arc;
use tokio::sync::Semaphore;
use tonic::{Request, Response, Status};

pub struct VyperVerifierService {
    compilers: Arc<EvmCompilersPool<VyperCompiler>>,
}

impl VyperVerifierService {
    pub async fn new(
        settings: VyperSettings,
        compilers_threads_semaphore: Arc<Semaphore>,
    ) -> anyhow::Result<Self> {
        let fetcher = common::initialize_fetcher(
            settings.fetcher,
            settings.compilers_dir.clone(),
            settings.refresh_versions_schedule,
            None,
        )
        .await
        .context("vyper fetcher initialization")?;
        let compilers: EvmCompilersPool<VyperCompiler> =
            EvmCompilersPool::new(fetcher, compilers_threads_semaphore);
        compilers.load_from_dir(&settings.compilers_dir).await;

        Ok(Self {
            compilers: Arc::new(compilers),
        })
    }
}

#[async_trait::async_trait]
impl VyperVerifier for VyperVerifierService {
    async fn verify_multi_part(
        &self,
        request: Request<VerifyVyperMultiPartRequest>,
    ) -> Result<Response<VerifyResponse>, Status> {
        let request = request.into_inner();

        let chain_id = request
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.chain_id.clone());
        let contract_address = request
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.contract_address.clone());
        tracing::info!(
            chain_id =? chain_id,
            contract_address =? contract_address,
            "vyper multi-part verification request received"
        );

        let maybe_verification_request = vyper::multi_part::VerificationRequest::try_from(request);
        let verification_request =
            common::process_solo_verification_request_conversion!(maybe_verification_request);

        let result = vyper::multi_part::verify(&self.compilers, verification_request).await;

        let verify_response = match result {
            Ok(value) => types::verification_result::process_verification_result(value)?,
            Err(error) => types::verification_result::process_error(error)?,
        };

        metrics::count_verify_contract(
            &chain_id.unwrap_or_default(),
            "vyper",
            verify_response.status().as_str_name(),
            "multi-part",
        );
        Ok(Response::new(verify_response))
    }

    async fn verify_standard_json(
        &self,
        request: Request<VerifyVyperStandardJsonRequest>,
    ) -> Result<Response<VerifyResponse>, Status> {
        let request = request.into_inner();

        let chain_id = request
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.chain_id.clone());
        let contract_address = request
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.contract_address.clone());
        tracing::info!(
            chain_id =? chain_id,
            contract_address =? contract_address,
            "vyper standard-json verification request received"
        );

        let maybe_verification_request =
            vyper::standard_json::VerificationRequest::try_from(request);
        let verification_request =
            common::process_solo_verification_request_conversion!(maybe_verification_request);

        let result = vyper::standard_json::verify(&self.compilers, verification_request).await;

        let verify_response = match result {
            Ok(value) => types::verification_result::process_verification_result(value)?,
            Err(error) => types::verification_result::process_error(error)?,
        };

        metrics::count_verify_contract(
            &chain_id.unwrap_or_default(),
            "vyper",
            verify_response.status().as_str_name(),
            "standard-json",
        );
        Ok(Response::new(verify_response))
    }

    async fn list_compiler_versions(
        &self,
        _request: Request<ListCompilerVersionsRequest>,
    ) -> Result<Response<ListCompilerVersionsResponse>, Status> {
        let compiler_versions = self.compilers.all_versions();
        Ok(Response::new(ListCompilerVersionsResponse {
            compiler_versions: common::versions_to_sorted_string(compiler_versions),
        }))
    }
}

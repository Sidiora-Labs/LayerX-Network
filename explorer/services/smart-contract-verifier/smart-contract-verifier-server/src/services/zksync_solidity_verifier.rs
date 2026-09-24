use crate::{
    proto::zksync::solidity::{
        verification_success, verify_response,
        zk_sync_solidity_verifier_server::ZkSyncSolidityVerifier, CompilationFailure,
        ListCompilersRequest, ListCompilersResponse, VerificationFailure, VerificationSuccess,
        VerifyResponse, VerifyStandardJsonRequest,
    },
    services::common,
    settings::ZksyncSoliditySettings,
    types::{zksolc_standard_json::VerifyStandardJsonRequestWrapper, StandardJsonParseError},
};
use anyhow::Context;
use smart_contract_verifier::{
    zksync,
    zksync::{ZkSolcCompiler, ZkSyncCompilers},
    SolcValidator,
};
use smart_contract_verifier_proto::blockscout::smart_contract_verifier::v2::zksync::solidity::{
    r#match::MatchType, Match,
};
use std::sync::Arc;
use tokio::sync::Semaphore;
use tonic::{Request, Response, Status};
use verification_common::verifier_alliance;

pub struct Service {
    compilers: ZkSyncCompilers<ZkSolcCompiler>,
}

impl Service {
    pub async fn new(
        settings: ZksyncSoliditySettings,
        compilers_threads_semaphore: Arc<Semaphore>,
    ) -> anyhow::Result<Self> {
        let solc_validator = Arc::new(SolcValidator::default());
        let evm_fetcher = common::initialize_fetcher(
            settings.evm_fetcher,
            settings.evm_compilers_dir.clone(),
            settings.evm_refresh_versions_schedule.clone(),
            Some(solc_validator),
        )
        .await
        .context("zksync solc fetcher initialization")?;

        let era_evm_fetcher = common::initialize_fetcher(
            settings.era_evm_fetcher,
            settings.evm_compilers_dir.clone(),
            settings.evm_refresh_versions_schedule,
            None,
        )
        .await
        .context("zksync era solc fetcher initialization")?;

        let zk_fetcher = common::initialize_fetcher(
            settings.zk_fetcher,
            settings.zk_compilers_dir.clone(),
            settings.zk_refresh_versions_schedule,
            None,
        )
        .await
        .context("zksync zksolc fetcher initialization")?;

        let compilers = ZkSyncCompilers::new(
            evm_fetcher.clone(),
            era_evm_fetcher.clone(),
            zk_fetcher.clone(),
            compilers_threads_semaphore,
        );

        Ok(Self { compilers })
    }
}

#[async_trait::async_trait]
impl ZkSyncSolidityVerifier for Service {
    async fn verify_standard_json(
        &self,
        request: Request<VerifyStandardJsonRequest>,
    ) -> Result<Response<VerifyResponse>, Status> {
        let request: VerifyStandardJsonRequestWrapper = request.into_inner().into();

        let verification_request: zksync::VerificationRequest = {
            let request: Result<_, StandardJsonParseError> = request.try_into();
            if let Err(err) = request {
                return match err {
                    StandardJsonParseError::InvalidContent(_) => {
                        let response = compilation_failure(format!("Invalid standard json: {err}"));
                        Ok(Response::new(response))
                    }
                    StandardJsonParseError::BadRequest(_) => {
                        tracing::info!(err=%err, "Bad request");
                        Err(Status::invalid_argument(err.to_string()))
                    }
                };
            }
            request.unwrap()
        };

        let result = zksync::verify(&self.compilers, verification_request).await;

        let response = process_verification_result(result)?;
        Ok(Response::new(response))
    }

    async fn list_compilers(
        &self,
        _request: Request<ListCompilersRequest>,
    ) -> Result<Response<ListCompilersResponse>, Status> {
        Ok(Response::new(ListCompilersResponse {
            solc_compilers: self.compilers.all_evm_versions_sorted_str(),
            zk_compilers: self.compilers.all_zk_versions_sorted_str(),
        }))
    }
}

#[allow(clippy::result_large_err)]
fn process_verification_result(
    result: Result<zksync::VerificationResult, zksync::Error>,
) -> Result<VerifyResponse, Status> {
    match result {
        Ok(result) => match zksync::choose_best_success(result.successes) {
            Some(success) => {
                let proto_success = VerificationSuccess {
                    file_name: success.file_path,
                    contract_name: success.contract_name,
                    zk_compiler: Some(verification_success::Compiler {
                        compiler: result.zk_compiler,
                        version: result.zk_compiler_version.to_string(),
                    }),
                    evm_compiler: Some(verification_success::Compiler {
                        compiler: result.evm_compiler,
                        version: result.evm_compiler_version.to_string(),
                    }),
                    language: result.language.into(),
                    compiler_settings: result.compiler_settings.to_string(),
                    sources: result.sources,
                    compilation_artifacts: serde_json::Value::from(success.compilation_artifacts)
                        .to_string(),
                    creation_code_artifacts: serde_json::Value::from(
                        success.creation_code_artifacts,
                    )
                    .to_string(),
                    runtime_code_artifacts: serde_json::Value::from(success.runtime_code_artifacts)
                        .to_string(),
                    creation_match: success.creation_match.map(process_match),
                    runtime_match: Some(process_match(success.runtime_match)),
                };

                Ok(VerifyResponse {
                    verify_response: Some(verify_response::VerifyResponse::VerificationSuccess(
                        proto_success,
                    )),
                })
            }
            None => Ok(VerifyResponse {
                verify_response: Some(verify_response::VerifyResponse::VerificationFailure(
                    VerificationFailure {
                        message: "No contract could be verified with provided data".to_string(),
                    },
                )),
            }),
        },
        Err(ref err) => match err {
            zksync::Error::Compilation(_) => Ok(compilation_failure(err.to_string())),
            zksync::Error::ZkCompilerNotFound(_) | zksync::Error::EvmCompilerNotFound(_) => {
                Err(Status::invalid_argument(err.to_string()))
            }
            zksync::Error::Internal(_) => Err(Status::internal(err.to_string())),
        },
    }
}

fn process_match(internal_match: verifier_alliance::Match) -> Match {
    let match_type = if internal_match.metadata_match {
        MatchType::Full
    } else {
        MatchType::Partial
    };
    Match {
        r#type: match_type.into(),
        values: serde_json::Value::from(internal_match.values).to_string(),
        transformations: serde_json::Value::from(internal_match.transformations).to_string(),
    }
}

fn compilation_failure(message: String) -> VerifyResponse {
    VerifyResponse {
        verify_response: Some(verify_response::VerifyResponse::CompilationFailure(
            CompilationFailure { message },
        )),
    }
}

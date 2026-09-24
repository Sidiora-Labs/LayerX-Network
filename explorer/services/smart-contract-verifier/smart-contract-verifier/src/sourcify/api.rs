use super::{
    api_client::SourcifyApiClient,
    types::{ApiRequest, ApiVerificationResponse, Error, Files, ResultItem, Success},
};
use crate::MatchType;
use anyhow::anyhow;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationRequest {
    pub address: String,
    pub chain: String,
    pub files: BTreeMap<String, String>,
    pub chosen_contract: Option<usize>,
}

impl From<VerificationRequest> for ApiRequest {
    fn from(value: VerificationRequest) -> Self {
        Self {
            address: value.address,
            chain: value.chain,
            files: Files(value.files),
            chosen_contract: value.chosen_contract.map(|v| v.to_string()),
        }
    }
}

pub async fn verify(
    sourcify_client: Arc<SourcifyApiClient>,
    request: VerificationRequest,
) -> Result<Success, Error> {
    let params = request.into();
    let response = sourcify_client
        .verification_request(&params)
        .await
        .map_err(|err| {
            anyhow!(
                "error while making verification request to Sourcify: {}",
                err
            )
        })
        .map_err(Error::Internal)?;

    match response {
        ApiVerificationResponse::Verified { result } => {
            let match_type = validate_verification_result(result)?;

            let api_files_response = sourcify_client
                .source_files_request(&params)
                .await
                .map_err(|err| {
                    anyhow!(
                        "error while making source files request to Sourcify: {}",
                        err
                    )
                })
                .map_err(Error::Internal)?;
            let files = Files::try_from((api_files_response, &params.chain, &params.address))
                .map_err(|err| anyhow!("error while parsing Sourcify files response: {}", err))
                .map_err(Error::Internal)?;
            let success = Success::try_from((files, match_type))
                .map_err(|err| Error::Validation(err.to_string()))?;

            Ok(success)
        }
        ApiVerificationResponse::Error { error } => Err(Error::Verification(error)),
        ApiVerificationResponse::ValidationErrors { message, errors } => {
            let error_message = format!("{message}: {errors:?}");
            Err(Error::Validation(error_message))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyFromEtherscanRequest {
    pub address: bytes::Bytes,
    pub chain: String,
}

pub async fn verify_from_etherscan(
    sourcify_client: Arc<SourcifyApiClient>,
    request: VerifyFromEtherscanRequest,
) -> Result<Success, Error> {
    let lib_client = sourcify_client.lib_client();

    let verification_result = lib_client
        .verify_from_etherscan(request.chain.as_str(), request.address.clone())
        .await;

    match verification_result {
        Ok(_) => {}
        Err(sourcify::Error::Sourcify(sourcify::SourcifyError::InternalServerError(err)))
            if err.contains("directory already has entry by that name") => {}
        Err(error) => return Err(error_handler::process_sourcify_error(error)),
    }

    let source_files = lib_client
        .get_source_files_any(request.chain.as_str(), request.address)
        .await
        .map_err(error_handler::process_sourcify_error)?;

    let success = Success::try_from(source_files)?;

    Ok(success)
}

/// Validates verification result.
/// In case of success returns corresponding match type.
fn validate_verification_result(result: Vec<ResultItem>) -> Result<MatchType, Error> {
    let item = result
        .first()
        .ok_or_else(|| {
            anyhow::anyhow!("invalid number of result items returned while verification succeeded")
        })
        .map_err(Error::Internal)?;
    match item.status.as_deref() {
        Some("partial") => Ok(MatchType::Partial),
        Some("perfect") => Ok(MatchType::Full),
        _ => Err(Error::Verification(
            item.message.clone().unwrap_or_default(),
        )),
    }
}

mod error_handler {
    use super::Error;
    use sourcify::{EmptyCustomError, VerifyFromEtherscanError};

    // Is public just to make it possible to use it for generics in outer functions.
    // Implementations for required custom errors are supposed to be added inside this module.
    //
    // Added to avoid passing the handler inside `process_sourcify_error`.
    pub trait ErrorHandler: Sized {
        fn handle(self) -> Error;
    }

    impl ErrorHandler for EmptyCustomError {
        fn handle(self) -> Error {
            // Empty error cannot be initialized
            unreachable!()
        }
    }

    impl ErrorHandler for VerifyFromEtherscanError {
        fn handle(self) -> Error {
            match self {
                VerifyFromEtherscanError::ChainNotSupported(msg) => Error::Verification(msg),
                VerifyFromEtherscanError::TooManyRequests(msg) => {
                    Error::Internal(anyhow::anyhow!(msg))
                }
                VerifyFromEtherscanError::ApiResponseError(msg) => {
                    Error::Internal(anyhow::anyhow!(msg))
                }
                VerifyFromEtherscanError::ContractNotVerified(msg) => Error::Verification(msg),
                VerifyFromEtherscanError::CannotGenerateSolcJsonInput(msg) => {
                    Error::Verification(msg)
                }
                VerifyFromEtherscanError::VerifiedWithErrors(msg) => Error::Verification(msg),
            }
        }
    }

    pub fn process_sourcify_error<E: std::error::Error + ErrorHandler>(
        error: sourcify::Error<E>,
    ) -> Error {
        match error {
            sourcify::Error::Reqwest(_) | sourcify::Error::ReqwestMiddleware(_) => {
                Error::Internal(anyhow::anyhow!(error.to_string()))
            }
            sourcify::Error::Sourcify(sourcify::SourcifyError::InternalServerError(_)) => {
                Error::Internal(anyhow::anyhow!(error.to_string()))
            }
            sourcify::Error::Sourcify(sourcify::SourcifyError::NotFound(msg)) => {
                Error::BadRequest(anyhow::anyhow!("{msg}"))
            }
            sourcify::Error::Sourcify(sourcify::SourcifyError::ChainNotSupported(msg)) => {
                Error::BadRequest(anyhow::anyhow!("{msg}"))
            }
            sourcify::Error::Sourcify(sourcify::SourcifyError::BadRequest(_)) => {
                tracing::error!(target: "sourcify", "{error}");
                Error::Internal(anyhow::anyhow!("{error}"))
            }
            sourcify::Error::Sourcify(sourcify::SourcifyError::BadGateway(_)) => {
                tracing::error!(target: "sourcify", "{error}");
                Error::Internal(anyhow::anyhow!("{error}"))
            }
            sourcify::Error::Sourcify(sourcify::SourcifyError::UnexpectedStatusCode { .. }) => {
                tracing::error!(target: "sourcify", "{error}");
                Error::Internal(anyhow::anyhow!("{error}"))
            }
            sourcify::Error::Sourcify(sourcify::SourcifyError::Custom(err)) => {
                tracing::error!(target: "sourcify", "custom endpoint error: {err}");
                E::handle(err)
            }
        }
    }
}

use blockscout_display_bytes::ToHex;
use serde_json::Value;
use smart_contract_verifier::{Error, Language, VerificationResult};
use smart_contract_verifier_proto::blockscout::smart_contract_verifier::{
    v2 as proto,
    v2::{BatchVerifyResponse, CompilationFailure},
};
use tonic::Status;
use verification_common::verifier_alliance;

pub fn compilation_error(message: impl Into<String>) -> BatchVerifyResponse {
    BatchVerifyResponse {
        verification_result: Some(
            proto::batch_verify_response::VerificationResult::CompilationFailure(
                CompilationFailure {
                    message: message.into(),
                },
            ),
        ),
    }
}

#[allow(clippy::result_large_err)]
pub fn process_error(error: Error) -> Result<BatchVerifyResponse, Status> {
    match error {
        err @ Error::CompilerNotFound(_) => Err(Status::invalid_argument(err.to_string())),
        err @ Error::Internal(_) => {
            let formatted_error = format!("{err:#?}");
            tracing::error!(err = formatted_error, "internal error");
            Err(Status::internal(formatted_error))
        }
        err @ Error::NotConsistentBlueprintOnChainCode { .. } => {
            Err(Status::invalid_argument(err.to_string()))
        }
        err @ Error::Compilation(_) => Ok(compilation_error(err.to_string())),
    }
}

#[allow(clippy::result_large_err)]
pub fn process_verification_results(
    values: Vec<VerificationResult>,
) -> Result<BatchVerifyResponse, Status> {
    let items = values
        .into_iter()
        .map(process_verification_result)
        .collect::<Result<_, _>>()?;

    Ok(BatchVerifyResponse {
        verification_result: Some(
            proto::batch_verify_response::VerificationResult::ContractVerificationResults(
                proto::batch_verify_response::ContractVerificationResults { items },
            ),
        ),
    })
}

#[allow(clippy::result_large_err)]
fn process_verification_result(
    value: VerificationResult,
) -> Result<proto::ContractVerificationResult, Status> {
    if value.is_empty() {
        let verification_result = proto::contract_verification_result::VerificationResult::Failure(
            proto::ContractVerificationFailure {},
        );
        return Ok(proto::ContractVerificationResult {
            verification_result: Some(verification_result),
        });
    }

    let verifying_contract = value.into_iter().next().unwrap();

    let verification_result = proto::contract_verification_result::VerificationResult::Success(
        proto::ContractVerificationSuccess {
            creation_code: verifying_contract.creation_code.to_hex(),
            runtime_code: verifying_contract.runtime_code.to_hex(),
            compiler: proto_compiler_from_language(verifying_contract.language).into(),
            compiler_version: verifying_contract.compiler_version,
            language: proto_language_from_language(verifying_contract.language).into(),
            file_name: verifying_contract.fully_qualified_name.file_name(),
            contract_name: verifying_contract.fully_qualified_name.contract_name(),
            sources: verifying_contract.sources,
            compiler_settings: verifying_contract.compiler_settings.to_string(),
            compilation_artifacts: Value::from(verifying_contract.compilation_artifacts)
                .to_string(),
            creation_code_artifacts: Value::from(verifying_contract.creation_code_artifacts)
                .to_string(),
            runtime_code_artifacts: Value::from(verifying_contract.runtime_code_artifacts)
                .to_string(),
            creation_match_details: parse_maybe_match(verifying_contract.creation_match),
            runtime_match_details: parse_maybe_match(verifying_contract.runtime_match),
        },
    );

    Ok(proto::ContractVerificationResult {
        verification_result: Some(verification_result),
    })
}

fn proto_compiler_from_language(
    language: Language,
) -> proto::contract_verification_success::compiler::Compiler {
    match language {
        Language::Solidity | Language::Yul => {
            proto::contract_verification_success::compiler::Compiler::Solc
        }
        Language::Vyper => proto::contract_verification_success::compiler::Compiler::Vyper,
    }
}

fn proto_language_from_language(
    language: Language,
) -> proto::contract_verification_success::language::Language {
    match language {
        Language::Solidity => proto::contract_verification_success::language::Language::Solidity,
        Language::Yul => proto::contract_verification_success::language::Language::Yul,
        Language::Vyper => proto::contract_verification_success::language::Language::Vyper,
    }
}

fn parse_maybe_match(
    value: Option<verifier_alliance::Match>,
) -> Option<proto::contract_verification_success::MatchDetails> {
    if let Some(value) = value {
        let match_type = if value.metadata_match {
            proto::contract_verification_success::MatchType::Full
        } else {
            proto::contract_verification_success::MatchType::Partial
        };

        return Some(proto::contract_verification_success::MatchDetails {
            match_type: match_type.into(),
            values: Value::from(value.values).to_string(),
            transformations: Value::from(value.transformations).to_string(),
        });
    }
    None
}

use blockscout_display_bytes::ToHex;
use serde_json::Value;
use smart_contract_verifier::{Error, Language, VerificationResult, VerifyingContract};
use smart_contract_verifier_proto::blockscout::smart_contract_verifier::{
    v2, v2::verify_response::extra_data,
};
use tonic::Status;
use verification_common::{
    solidity_libraries,
    solidity_libraries::parse_manually_linked_libraries,
    verifier_alliance::{CborAuxdata, CompilationArtifacts, Match},
};

#[allow(clippy::result_large_err)]
pub fn process_error(error: Error) -> Result<v2::VerifyResponse, Status> {
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
        err @ Error::Compilation(_) => {
            let response = v2::VerifyResponse {
                message: err.to_string(),
                status: v2::verify_response::Status::Failure.into(),
                source: None,
                extra_data: None,
                post_action_responses: None,
            };
            Ok(response)
        }
    }
}

#[allow(clippy::result_large_err)]
pub fn process_verification_result(
    value: VerificationResult,
) -> Result<v2::VerifyResponse, Status> {
    if value.is_empty() {
        let response = v2::VerifyResponse {
            message: "No contract could be verified with provided data".to_string(),
            status: v2::verify_response::Status::Failure.into(),
            source: None,
            extra_data: None,
            post_action_responses: None,
        };
        return Ok(response);
    }

    let mut full_creation_and_runtime_match = None;
    let mut full_creation_only_match = None;
    let mut full_runtime_only_match = None;
    for verifying_contract in &value {
        match (
            &verifying_contract.creation_match,
            &verifying_contract.runtime_match,
        ) {
            (Some(creation_match), Some(runtime_match))
                if creation_match.metadata_match && runtime_match.metadata_match =>
            {
                full_creation_and_runtime_match = Some(verifying_contract)
            }
            (Some(creation_match), _) if creation_match.metadata_match => {
                full_creation_only_match = Some(verifying_contract)
            }
            (_, Some(runtime_match)) if runtime_match.metadata_match => {
                full_runtime_only_match = Some(verifying_contract)
            }
            _ => {}
        }
    }

    let verifying_contract = match (
        full_creation_and_runtime_match,
        full_creation_only_match,
        full_runtime_only_match,
    ) {
        (Some(full_creation_and_runtime_match), _, _) => full_creation_and_runtime_match,
        (None, Some(full_creation_only_match), _) => full_creation_only_match,
        (None, None, Some(full_runtime_only_match)) => full_runtime_only_match,
        _ => value.first().unwrap(),
    };

    let extra_data = into_extra_data(verifying_contract);
    let source = try_into_source(verifying_contract.clone())?;

    let response = v2::VerifyResponse {
        message: "OK".to_string(),
        status: v2::verify_response::Status::Success.into(),
        source: Some(source),
        extra_data: Some(extra_data),
        post_action_responses: Some(v2::verify_response::PostActionResponses {
            lookup_methods: None,
        }),
    };
    Ok(response)
}

fn into_extra_data(verifying_contract: &VerifyingContract) -> v2::verify_response::ExtraData {
    let creation_code_parts = parse_re_compiled_code_parts(
        &verifying_contract.creation_code,
        &verifying_contract.creation_code_artifacts.cbor_auxdata,
    );
    let runtime_code_parts = parse_re_compiled_code_parts(
        &verifying_contract.runtime_code,
        &verifying_contract.runtime_code_artifacts.cbor_auxdata,
    );

    v2::verify_response::ExtraData {
        local_creation_input_parts: creation_code_parts,
        local_deployed_bytecode_parts: runtime_code_parts,
    }
}

fn parse_re_compiled_code_parts(
    code: &[u8],
    cbor_auxdata_artifacts: &Option<CborAuxdata>,
) -> Vec<extra_data::BytecodePart> {
    let mut code_parts = vec![];
    if let Some(cbor_auxdata_artifacts) = cbor_auxdata_artifacts {
        let mut sorted_cbor_auxdata_values: Vec<_> = cbor_auxdata_artifacts.values().collect();
        sorted_cbor_auxdata_values.sort_by_key(|value| value.offset);

        let mut index = 0usize;
        for value in sorted_cbor_auxdata_values {
            let offset = value.offset as usize;
            assert!(index <= offset);

            if index < offset {
                code_parts.push(new_bytecode_part("main", &code[index..offset]))
            }
            code_parts.push(new_bytecode_part("meta", &value.value));
            index = offset + value.value.len();
        }

        if index < code.len() {
            code_parts.push(new_bytecode_part("main", &code[index..]));
        }
    }

    code_parts
}

fn new_bytecode_part(type_: &str, data: &[u8]) -> extra_data::BytecodePart {
    extra_data::BytecodePart {
        r#type: type_.to_string(),
        data: data.to_hex(),
    }
}

#[allow(clippy::result_large_err)]
fn try_into_source(verifying_contract: VerifyingContract) -> Result<v2::Source, Status> {
    let compilation_artifacts = verifying_contract.compilation_artifacts;
    let creation_code_artifacts = verifying_contract.creation_code_artifacts;
    let runtime_code_artifacts = verifying_contract.runtime_code_artifacts;

    let mut libraries = solidity_libraries::try_parse_compiler_linked_libraries(
        &verifying_contract.compiler_settings,
    )
    .map_err(|err| Status::internal(err.to_string()))?;
    if let Some(creation_match_) = verifying_contract.creation_match.as_ref() {
        libraries.extend(parse_manually_linked_libraries(creation_match_));
    }
    if let Some(runtime_match_) = verifying_contract.runtime_match.as_ref() {
        libraries.extend(parse_manually_linked_libraries(runtime_match_));
    }

    let source = v2::Source {
        file_name: verifying_contract.fully_qualified_name.file_name(),
        contract_name: verifying_contract.fully_qualified_name.contract_name(),
        compiler_version: verifying_contract.compiler_version,
        compiler_settings: verifying_contract.compiler_settings.to_string(),
        source_type: parse_source_type(verifying_contract.language).into(),
        source_files: verifying_contract.sources,
        abi: parse_abi(&compilation_artifacts),
        constructor_arguments: parse_constructor_arguments(&verifying_contract.creation_match),
        match_type: parse_match_type(
            &verifying_contract.creation_match,
            &verifying_contract.runtime_match,
        )?
        .into(),
        compilation_artifacts: Some(Value::from(compilation_artifacts).to_string()),
        creation_input_artifacts: Some(Value::from(creation_code_artifacts).to_string()),
        deployed_bytecode_artifacts: Some(Value::from(runtime_code_artifacts).to_string()),
        is_blueprint: verifying_contract.is_blueprint,
        libraries,
    };
    Ok(source)
}

fn parse_source_type(language: Language) -> v2::source::SourceType {
    match language {
        Language::Solidity => v2::source::SourceType::Solidity,
        Language::Yul => v2::source::SourceType::Yul,
        Language::Vyper => v2::source::SourceType::Vyper,
    }
}

fn parse_abi(compilation_artifacts: &CompilationArtifacts) -> Option<String> {
    compilation_artifacts
        .abi
        .as_ref()
        .map(|value| value.to_string())
}

fn parse_constructor_arguments(creation_match: &Option<Match>) -> Option<String> {
    let creation_match = match creation_match {
        Some(creation_match) => creation_match,
        None => return None,
    };

    creation_match
        .values
        .constructor_arguments
        .as_ref()
        .map(|value| value.to_hex())
}

#[allow(clippy::result_large_err)]
fn parse_match_type(
    creation_match: &Option<Match>,
    runtime_match: &Option<Match>,
) -> Result<v2::source::MatchType, Status> {
    let match_type_from_match = |match_: &Match| {
        if match_.metadata_match {
            return v2::source::MatchType::Full;
        }
        v2::source::MatchType::Partial
    };

    if let Some(creation_match) = creation_match {
        Ok(match_type_from_match(creation_match))
    } else if let Some(runtime_match) = runtime_match {
        Ok(match_type_from_match(runtime_match))
    } else {
        Err(Status::internal(
            "verifying contract doesn't have neither creation nor runtime matches",
        ))
    }
}

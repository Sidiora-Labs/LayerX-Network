use super::{
    compilation,
    compilation::CompilationResult,
    evm_compilers::{EvmCompiler, EvmCompilersPool},
    verification, Error,
};
use crate::{DetailedVersion, FullyQualifiedName, Language, OnChainContract};
use blockscout_display_bytes::ToHex;
use bytes::Bytes;
use std::collections::BTreeMap;
use verification_common::{
    blueprint_contracts,
    verifier_alliance::{CompilationArtifacts, CreationCodeArtifacts, Match, RuntimeCodeArtifacts},
};

#[derive(Clone, Debug)]
pub struct VerifyingContract {
    pub fully_qualified_name: FullyQualifiedName,
    pub language: Language,
    pub compiler_version: String,
    pub compiler_settings: serde_json::Value,
    pub sources: BTreeMap<String, String>,
    pub creation_code: Vec<u8>,
    pub runtime_code: Vec<u8>,
    pub compilation_artifacts: CompilationArtifacts,
    pub creation_code_artifacts: CreationCodeArtifacts,
    pub runtime_code_artifacts: RuntimeCodeArtifacts,
    pub runtime_match: Option<Match>,
    pub creation_match: Option<Match>,
    pub is_blueprint: bool,
}

pub type VerificationResult = Vec<VerifyingContract>;

pub async fn compile_and_verify<C: EvmCompiler>(
    to_verify: Vec<OnChainContract>,
    compilers: &EvmCompilersPool<C>,
    compiler_version: &DetailedVersion,
    compiler_input: C::CompilerInput,
) -> Result<Vec<VerificationResult>, Error> {
    let compilation_result =
        compilation::compile(compilers, compiler_version, compiler_input).await?;

    let mut verification_results = vec![];
    for contract in to_verify {
        verification_results.push(verify_on_chain_contract(contract, &compilation_result)?);
    }

    Ok(verification_results)
}

fn verify_on_chain_contract(
    contract: OnChainContract,
    compilation_result: &CompilationResult,
) -> Result<VerificationResult, Error> {
    let blueprint_initcode = try_extract_blueprint_initcode(&contract)?;
    let is_blueprint = blueprint_initcode.is_some();

    let mut successes = vec![];
    for (fully_qualified_name, contract_artifacts) in &compilation_result.artifacts {
        let verify_contract_result = match blueprint_initcode.clone() {
            Some(initcode) => verification::verify_blueprint_contract(
                initcode,
                contract_artifacts.code.clone(),
                &contract_artifacts.creation_code_artifacts,
            ),
            None => verification::verify_contract(
                contract.code.clone(),
                contract_artifacts.code.clone(),
                &contract_artifacts.compilation_artifacts,
                &contract_artifacts.creation_code_artifacts,
                &contract_artifacts.runtime_code_artifacts,
            ),
        };

        let maybe_verifying_contract = process_verify_contract_result(
            compilation_result,
            fully_qualified_name,
            &verify_contract_result,
            is_blueprint,
        )?;

        if let Some(verifying_contract) = maybe_verifying_contract {
            successes.push(verifying_contract);
        }
    }

    Ok(successes)
}

fn process_verify_contract_result(
    compilation_result: &CompilationResult,
    contract_fully_qualified_name: &FullyQualifiedName,
    verify_contract_result: &verification::VerificationResult,
    is_blueprint: bool,
) -> Result<Option<VerifyingContract>, Error> {
    let (runtime_match, creation_match) = match verify_contract_result.clone() {
        verification::VerificationResult::Failure => return Ok(None),
        verification::VerificationResult::RuntimeMatch { runtime_match } => {
            (Some(runtime_match), None)
        }
        verification::VerificationResult::CreationMatch { creation_match } => {
            (None, Some(creation_match))
        }
        verification::VerificationResult::CompleteMatch {
            runtime_match,
            creation_match,
        } => (Some(runtime_match), Some(creation_match)),
    };

    let contract_compilation_artifacts = compilation_result
        .artifacts
        .get(contract_fully_qualified_name)
        .expect("key was obtained by iterating through `compilation_result.artifacts`");

    let verifying_contract = VerifyingContract {
        fully_qualified_name: contract_fully_qualified_name.clone(),
        language: compilation_result.language,
        compiler_version: compilation_result.compiler_version.clone(),
        compiler_settings: compilation_result.compiler_settings.clone(),
        sources: compilation_result.sources.clone(),
        creation_code: contract_compilation_artifacts.code.creation.clone(),
        runtime_code: contract_compilation_artifacts.code.runtime.clone(),
        compilation_artifacts: contract_compilation_artifacts.compilation_artifacts.clone(),
        creation_code_artifacts: contract_compilation_artifacts
            .creation_code_artifacts
            .clone(),
        runtime_code_artifacts: contract_compilation_artifacts
            .runtime_code_artifacts
            .clone(),
        runtime_match,
        creation_match,
        is_blueprint,
    };

    Ok(Some(verifying_contract))
}

/// In case only one of creation or runtime code correspond to blueprint,
/// or they correspond to different initcodes, returns `Error::NotConsistentBlueprintOnChainCode`.
fn try_extract_blueprint_initcode(contract: &OnChainContract) -> Result<Option<Vec<u8>>, Error> {
    let creation_code = contract.code.creation.as_ref();
    let runtime_code = contract.code.runtime.as_ref();

    match (creation_code, runtime_code) {
        (Some(creation_code), Some(runtime_code)) => {
            let creation_blueprint =
                blueprint_contracts::from_creation_code(Bytes::copy_from_slice(creation_code));
            let runtime_blueprint =
                blueprint_contracts::from_runtime_code(Bytes::copy_from_slice(runtime_code));
            if creation_blueprint != runtime_blueprint {
                return Err(Error::NotConsistentBlueprintOnChainCode {
                    chain_id: contract.chain_id.clone(),
                    address: contract.address.map(|address| address.to_hex()),
                });
            }
            Ok(creation_blueprint.map(|value| value.initcode.to_vec()))
        }
        (Some(creation_code), None) => {
            let blueprint =
                blueprint_contracts::from_creation_code(Bytes::copy_from_slice(creation_code));
            Ok(blueprint.map(|value| value.initcode.to_vec()))
        }
        (None, Some(runtime_code)) => {
            let blueprint =
                blueprint_contracts::from_runtime_code(Bytes::copy_from_slice(runtime_code));
            Ok(blueprint.map(|value| value.initcode.to_vec()))
        }
        (None, None) => Ok(None),
    }
}

use crate::{
    solidity,
    verify::{SolcInput, VyperInput},
    vyper, OnChainCode, OnChainContract, RequestParseError,
};
use anyhow::Context;
use foundry_compilers::artifacts::EvmVersion;
use smart_contract_verifier_proto::blockscout::smart_contract_verifier::v2::{
    BatchVerifySolidityMultiPartRequest, BatchVerifySolidityStandardJsonRequest, BytecodeType,
    Contract, VerificationMetadata, VerifySolidityMultiPartRequest,
    VerifySolidityStandardJsonRequest, VerifyVyperMultiPartRequest, VerifyVyperStandardJsonRequest,
};
use std::{collections::BTreeMap, path::PathBuf, str::FromStr};

impl TryFrom<Contract> for OnChainContract {
    type Error = RequestParseError;

    fn try_from(value: Contract) -> Result<Self, Self::Error> {
        let runtime_code =
            helpers::decode_optional_hex(value.runtime_code).context("invalid runtime code")?;
        let creation_code =
            helpers::decode_optional_hex(value.creation_code).context("invalid creation code")?;

        let on_chain_code = match (runtime_code, creation_code) {
            (None, None) => Err(anyhow::anyhow!(
                "both runtime and creation code cannot be empty"
            ))?,
            (Some(runtime_code), None) => OnChainCode::runtime(runtime_code),
            (None, Some(creation_code)) => OnChainCode::creation(creation_code),
            (Some(runtime_code), Some(creation_code)) => {
                OnChainCode::complete(runtime_code, creation_code)
            }
        };

        let (chain_id, address) = helpers::decode_verification_metadata(value.metadata);

        Ok(Self {
            code: on_chain_code,
            chain_id,
            address,
        })
    }
}

impl TryFrom<VerifySolidityMultiPartRequest> for solidity::multi_part::VerificationRequest {
    type Error = RequestParseError;

    fn try_from(request: VerifySolidityMultiPartRequest) -> Result<Self, Self::Error> {
        let contract =
            build_on_chain_contract(&request.bytecode, request.bytecode_type(), request.metadata)?;
        let compiler_version = helpers::decode_compiler_version(&request.compiler_version)?;
        let content = build_solidity_multi_part_content(
            request.source_files,
            request.evm_version,
            request.optimization_runs.map(|value| value as u32),
        )?;

        Ok(Self {
            contract,
            compiler_version,
            content,
        })
    }
}

impl TryFrom<VerifySolidityStandardJsonRequest> for solidity::standard_json::VerificationRequest {
    type Error = RequestParseError;

    fn try_from(request: VerifySolidityStandardJsonRequest) -> Result<Self, Self::Error> {
        let contract =
            build_on_chain_contract(&request.bytecode, request.bytecode_type(), request.metadata)?;
        let compiler_version = helpers::decode_compiler_version(&request.compiler_version)?;
        let content = build_solidity_standard_json_content(request.input)?;

        Ok(Self {
            contract,
            compiler_version,
            content,
        })
    }
}

impl TryFrom<BatchVerifySolidityMultiPartRequest>
    for solidity::multi_part::BatchVerificationRequest
{
    type Error = RequestParseError;

    fn try_from(request: BatchVerifySolidityMultiPartRequest) -> Result<Self, Self::Error> {
        let contracts = request
            .contracts
            .into_iter()
            .map(OnChainContract::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        let compiler_version = helpers::decode_compiler_version(&request.compiler_version)?;
        let content = build_solidity_multi_part_content(
            request.sources,
            request.evm_version,
            request.optimization_runs,
        )?;

        Ok(Self {
            contracts,
            compiler_version,
            content,
        })
    }
}

impl TryFrom<BatchVerifySolidityStandardJsonRequest>
    for solidity::standard_json::BatchVerificationRequest
{
    type Error = RequestParseError;

    fn try_from(request: BatchVerifySolidityStandardJsonRequest) -> Result<Self, Self::Error> {
        let contracts = request
            .contracts
            .into_iter()
            .map(OnChainContract::try_from)
            .collect::<Result<Vec<_>, _>>()?;

        let compiler_version = helpers::decode_compiler_version(&request.compiler_version)?;
        let content = build_solidity_standard_json_content(request.input)?;

        Ok(Self {
            contracts,
            compiler_version,
            content,
        })
    }
}

impl TryFrom<VerifyVyperMultiPartRequest> for vyper::multi_part::VerificationRequest {
    type Error = RequestParseError;

    fn try_from(request: VerifyVyperMultiPartRequest) -> Result<Self, Self::Error> {
        let contract =
            build_on_chain_contract(&request.bytecode, request.bytecode_type(), request.metadata)?;
        let compiler_version = helpers::decode_compiler_version(&request.compiler_version)?;
        let content = build_vyper_multi_part_content(
            request.source_files,
            request.interfaces,
            request.evm_version,
        )?;

        Ok(Self {
            contract,
            compiler_version,
            content,
        })
    }
}

impl TryFrom<VerifyVyperStandardJsonRequest> for vyper::standard_json::VerificationRequest {
    type Error = RequestParseError;

    fn try_from(request: VerifyVyperStandardJsonRequest) -> Result<Self, Self::Error> {
        let contract =
            build_on_chain_contract(&request.bytecode, request.bytecode_type(), request.metadata)?;
        let compiler_version = helpers::decode_compiler_version(&request.compiler_version)?;
        let content = build_vyper_standard_json_content(request.input)?;

        Ok(Self {
            contract,
            compiler_version,
            content,
        })
    }
}

fn build_solidity_multi_part_content(
    sources: BTreeMap<String, String>,
    evm_version: Option<String>,
    optimization_runs: Option<u32>,
) -> Result<solidity::multi_part::Content, RequestParseError> {
    let sources: BTreeMap<PathBuf, String> = sources
        .into_iter()
        .map(|(name, content)| (PathBuf::from(name), content))
        .collect();

    let evm_version = match evm_version {
        Some(version) if version != "default" => Some(
            EvmVersion::from_str(&version)
                .map_err(|err| anyhow::anyhow!("invalid evm_version: {err}"))?,
        ),
        _ => None,
    };

    Ok(solidity::multi_part::Content {
        sources,
        evm_version,
        optimization_runs,
    })
}

fn build_solidity_standard_json_content(
    solc_input: String,
) -> Result<SolcInput, RequestParseError> {
    let deserializer = &mut serde_json::Deserializer::from_str(&solc_input);
    Ok(serde_path_to_error::deserialize(deserializer)?)
}

fn build_vyper_multi_part_content(
    sources: BTreeMap<String, String>,
    interfaces: BTreeMap<String, String>,
    evm_version: Option<String>,
) -> Result<vyper::multi_part::Content, RequestParseError> {
    let sources: BTreeMap<PathBuf, String> = sources
        .into_iter()
        .map(|(name, content)| (PathBuf::from(name), content))
        .collect();

    let interfaces: BTreeMap<PathBuf, String> = interfaces
        .into_iter()
        .map(|(name, content)| (PathBuf::from(name), content))
        .collect();

    let evm_version = match evm_version {
        Some(version) if version != "default" => Some(
            EvmVersion::from_str(&version)
                .map_err(|err| anyhow::anyhow!("invalid evm_version: {err}"))?,
        ),
        _ => None,
    };

    Ok(vyper::multi_part::Content {
        sources,
        interfaces,
        evm_version,
    })
}

fn build_vyper_standard_json_content(input: String) -> Result<VyperInput, RequestParseError> {
    let deserializer = &mut serde_json::Deserializer::from_str(&input);
    Ok(serde_path_to_error::deserialize(deserializer)?)
}

fn build_on_chain_contract(
    bytecode: &str,
    bytecode_type: BytecodeType,
    metadata: Option<VerificationMetadata>,
) -> Result<OnChainContract, RequestParseError> {
    let on_chain_code = helpers::decode_on_chain_code_from_value_and_type(bytecode, bytecode_type)?;
    let (chain_id, address) = helpers::decode_verification_metadata(metadata);
    Ok(OnChainContract {
        code: on_chain_code,
        chain_id,
        address,
    })
}

mod helpers {
    use crate::{DetailedVersion, OnChainCode};
    use alloy_core::primitives::Address;
    use anyhow::Context;
    use smart_contract_verifier_proto::blockscout::smart_contract_verifier::v2::{
        BytecodeType, VerificationMetadata,
    };
    use std::str::FromStr;

    pub fn decode_compiler_version(value: &str) -> anyhow::Result<DetailedVersion> {
        DetailedVersion::from_str(value).context("invalid compiler version")
    }

    pub fn decode_optional_hex(
        maybe_value: Option<String>,
    ) -> Result<Option<Vec<u8>>, hex::FromHexError> {
        maybe_value
            .map(|value| blockscout_display_bytes::decode_hex(&value))
            .transpose()
    }

    pub fn decode_verification_metadata(
        maybe_value: Option<VerificationMetadata>,
    ) -> (Option<String>, Option<Address>) {
        match maybe_value {
            None => (None, None),
            Some(metadata) => {
                let chain_id = metadata.chain_id;
                let contract_address = metadata
                    .contract_address
                    .map(|value| alloy_core::primitives::Address::from_str(&value))
                    .transpose()
                    .ok()
                    .flatten();
                (chain_id, contract_address)
            }
        }
    }

    pub fn decode_on_chain_code_from_value_and_type(
        value: &str,
        bytecode_type: BytecodeType,
    ) -> anyhow::Result<OnChainCode> {
        let code_value =
            blockscout_display_bytes::decode_hex(value).context("bytecode is not valid hex")?;
        match bytecode_type {
            BytecodeType::Unspecified => Err(anyhow::anyhow!("bytecode type is unspecified")),
            BytecodeType::CreationInput => Ok(OnChainCode::creation(code_value)),
            BytecodeType::DeployedBytecode => Ok(OnChainCode::runtime(code_value)),
        }
    }
}

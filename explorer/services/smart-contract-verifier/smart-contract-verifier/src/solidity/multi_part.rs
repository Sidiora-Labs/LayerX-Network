use crate::{
    compiler::DetailedVersion, verify, Error, EvmCompilersPool, OnChainContract, SolcCompiler,
    SolcInput, VerificationResult,
};
use foundry_compilers::artifacts;
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Content {
    pub sources: BTreeMap<PathBuf, String>,
    pub evm_version: Option<artifacts::EvmVersion>,
    pub optimization_runs: Option<u32>,
}

impl From<Content> for Vec<SolcInput> {
    fn from(content: Content) -> Self {
        let mut settings = artifacts::solc::Settings::default();
        if let Some(optimization_runs) = content.optimization_runs {
            settings.optimizer.enabled = Some(true);
            settings.optimizer.runs = Some(optimization_runs as usize);
        }
        settings.evm_version = content.evm_version;

        let sources: artifacts::Sources = content
            .sources
            .into_iter()
            .map(|(name, content)| (name, artifacts::Source::new(content)))
            .collect();
        let inputs: Vec<_> = helpers::input_from_sources_and_settings(sources, settings.clone())
            .into_iter()
            .map(SolcInput)
            .collect();
        inputs
    }
}

pub struct VerificationRequest {
    pub contract: OnChainContract,
    pub compiler_version: DetailedVersion,
    pub content: Content,
}

pub async fn verify(
    compilers: &EvmCompilersPool<SolcCompiler>,
    request: VerificationRequest,
) -> Result<VerificationResult, Error> {
    let to_verify = vec![request.contract];

    let solc_inputs: Vec<SolcInput> = request.content.into();
    for solc_input in solc_inputs {
        for metadata in helpers::settings_metadata(&request.compiler_version) {
            let mut solc_input = solc_input.clone();
            solc_input.0.settings.metadata = metadata;

            let results = verify::compile_and_verify(
                to_verify.clone(),
                compilers,
                &request.compiler_version,
                solc_input,
            )
            .await?;
            let result = results
                .into_iter()
                .next()
                .expect("we sent exactly one contract to verify");

            if result.is_empty() {
                continue;
            }

            return Ok(result);
        }
    }

    // no contracts could be verified
    Ok(vec![])
}
#[derive(Clone, Debug)]
pub struct BatchVerificationRequest {
    pub contracts: Vec<OnChainContract>,
    pub compiler_version: DetailedVersion,
    pub content: Content,
}

pub async fn batch_verify(
    compilers: &EvmCompilersPool<SolcCompiler>,
    request: BatchVerificationRequest,
) -> Result<Vec<VerificationResult>, Error> {
    let to_verify = request.contracts;

    let solc_inputs: Vec<SolcInput> = request.content.into();
    if solc_inputs.len() != 1 {
        return Err(Error::Compilation(vec![
            "exactly one of `.sol` or `.yul` files should exist".to_string(),
        ]));
    }

    let content = solc_inputs.into_iter().next().unwrap();
    let results =
        verify::compile_and_verify(to_verify, compilers, &request.compiler_version, content)
            .await?;

    Ok(results)
}

mod helpers {
    use crate::DetailedVersion;
    use foundry_compilers::{
        artifacts,
        artifacts::{BytecodeHash, SettingsMetadata},
    };
    use semver::VersionReq;
    use std::{collections::BTreeMap, ffi::OsStr};

    pub fn input_from_sources_and_settings(
        sources: artifacts::Sources,
        settings: artifacts::solc::Settings,
    ) -> Vec<artifacts::SolcInput> {
        let mut solidity_sources = BTreeMap::new();
        let mut yul_sources = BTreeMap::new();
        for (path, source) in sources {
            if &path == ".yul" || path.extension() == Some(OsStr::new("yul")) {
                yul_sources.insert(path, source);
            } else {
                solidity_sources.insert(path, source);
            }
        }
        let mut res = Vec::new();
        if !yul_sources.is_empty() {
            res.push(artifacts::SolcInput {
                language: artifacts::SolcLanguage::Yul,
                sources: artifacts::Sources(yul_sources),
                settings: settings.clone(),
            });
        }
        if !solidity_sources.is_empty() {
            res.push(artifacts::SolcInput {
                language: artifacts::SolcLanguage::Solidity,
                sources: artifacts::Sources(solidity_sources),
                settings,
            });
        }
        res
    }

    /// Iterates through possible bytecode if required and creates
    /// a corresponding variants of settings metadata for each of them.
    ///
    /// Multi-file input type does not specify it explicitly, thus, we may
    /// have to iterate through all possible options.
    ///
    /// See "settings_metadata" (https://docs.soliditylang.org/en/v0.8.15/using-the-compiler.html?highlight=compiler%20input#input-description)
    pub fn settings_metadata(compiler_version: &DetailedVersion) -> Vec<Option<SettingsMetadata>> {
        // Options are sorted by their probability of occurring
        const BYTECODE_HASHES: [BytecodeHash; 3] =
            [BytecodeHash::Ipfs, BytecodeHash::None, BytecodeHash::Bzzr1];

        if VersionReq::parse("<0.6.0")
            .unwrap()
            .matches(compiler_version.version())
        {
            [None].into()
        } else {
            BYTECODE_HASHES
                .map(|hash| Some(SettingsMetadata::from(hash)))
                .into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use foundry_compilers::artifacts::EvmVersion;
    use pretty_assertions::assert_eq;
    use std::{collections::BTreeMap, path::PathBuf};

    fn sources(sources: &[(&str, &str)]) -> BTreeMap<PathBuf, String> {
        sources
            .iter()
            .map(|(name, content)| (PathBuf::from(name), content.to_string()))
            .collect()
    }

    fn test_to_input(multi_part: Content, expected: Vec<&str>) {
        let inputs: Vec<SolcInput> = multi_part.into();
        assert_eq!(
            inputs.len(),
            expected.len(),
            "invalid number of compiler inputs"
        );
        for i in 0..expected.len() {
            let input_json = serde_json::to_string(&inputs[i]).unwrap();
            println!("{input_json}");
            assert_eq!(input_json, expected[i]);
        }
    }

    #[test]
    fn multi_part_to_input() {
        let multi_part = Content {
            sources: sources(&[("source.sol", "pragma")]),
            evm_version: Some(EvmVersion::London),
            optimization_runs: Some(200),
        };
        let expected = r#"{"language":"Solidity","sources":{"source.sol":{"content":"pragma"}},"settings":{"optimizer":{"enabled":true,"runs":200},"outputSelection":{"*":{"":["ast"],"*":["abi","evm.bytecode.object","evm.bytecode.sourceMap","evm.bytecode.linkReferences","evm.deployedBytecode.object","evm.deployedBytecode.sourceMap","evm.deployedBytecode.linkReferences","evm.deployedBytecode.immutableReferences","evm.methodIdentifiers"]}},"evmVersion":"london","libraries":{}}}"#;
        test_to_input(multi_part, vec![expected]);
        let multi_part = Content {
            sources: sources(&[("source.sol", "")]),
            evm_version: Some(EvmVersion::SpuriousDragon),
            optimization_runs: None,
        };
        let expected = r#"{"language":"Solidity","sources":{"source.sol":{"content":""}},"settings":{"optimizer":{"enabled":false,"runs":200},"outputSelection":{"*":{"":["ast"],"*":["abi","evm.bytecode.object","evm.bytecode.sourceMap","evm.bytecode.linkReferences","evm.deployedBytecode.object","evm.deployedBytecode.sourceMap","evm.deployedBytecode.linkReferences","evm.deployedBytecode.immutableReferences","evm.methodIdentifiers"]}},"evmVersion":"spuriousDragon","libraries":{}}}"#;
        test_to_input(multi_part, vec![expected]);
    }

    #[test]
    fn yul_and_solidity_to_inputs() {
        let multi_part = Content {
            sources: sources(&[
                ("source.sol", "pragma"),
                ("source2.yul", "object \"A\" {}"),
                (".yul", "object \"A\" {}"),
            ]),
            evm_version: Some(EvmVersion::London),
            optimization_runs: Some(200),
        };
        let expected_solidity = r#"{"language":"Solidity","sources":{"source.sol":{"content":"pragma"}},"settings":{"optimizer":{"enabled":true,"runs":200},"outputSelection":{"*":{"":["ast"],"*":["abi","evm.bytecode.object","evm.bytecode.sourceMap","evm.bytecode.linkReferences","evm.deployedBytecode.object","evm.deployedBytecode.sourceMap","evm.deployedBytecode.linkReferences","evm.deployedBytecode.immutableReferences","evm.methodIdentifiers"]}},"evmVersion":"london","libraries":{}}}"#;
        let expected_yul = r#"{"language":"Yul","sources":{".yul":{"content":"object \"A\" {}"},"source2.yul":{"content":"object \"A\" {}"}},"settings":{"optimizer":{"enabled":true,"runs":200},"outputSelection":{"*":{"":["ast"],"*":["abi","evm.bytecode.object","evm.bytecode.sourceMap","evm.bytecode.linkReferences","evm.deployedBytecode.object","evm.deployedBytecode.sourceMap","evm.deployedBytecode.linkReferences","evm.deployedBytecode.immutableReferences","evm.methodIdentifiers"]}},"evmVersion":"london","libraries":{}}}"#;
        test_to_input(multi_part, vec![expected_yul, expected_solidity]);
    }
}

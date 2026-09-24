use crate::proto::{source, Source};
use blockscout_display_bytes::Bytes as DisplayBytes;
use smart_contract_verifier::{MatchType, SourcifySuccess};

pub fn from_sourcify_success(value: SourcifySuccess) -> Source {
    let match_type = match value.match_type {
        MatchType::Partial => source::MatchType::Partial,
        MatchType::Full => source::MatchType::Full,
    };

    Source {
        file_name: value.file_name,
        contract_name: value.contract_name,
        compiler_version: value.compiler_version,
        compiler_settings: value.compiler_settings,
        source_type: source::SourceType::Solidity.into(),
        source_files: value.sources,
        abi: Some(value.abi),
        constructor_arguments: value
            .constructor_arguments
            .map(|bytes| DisplayBytes::from(bytes).to_string()),
        match_type: match_type.into(),
        compilation_artifacts: None,
        creation_input_artifacts: None,
        deployed_bytecode_artifacts: None,
        is_blueprint: false,
        libraries: value.contract_libraries,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::{collections::BTreeMap, str::FromStr};

    #[test]
    fn test_from_sourcify_success() {
        let verification_success = SourcifySuccess {
            file_name: "file_name".to_string(),
            compiler_version: "v0.8.17+commit.8df45f5f".to_string(),
            evm_version: Some("london".to_string()),
            optimization: Some(true),
            optimization_runs: Some(200),
            constructor_arguments: Some(DisplayBytes::from_str("0x123456").unwrap().0),
            contract_name: "contract_name".to_string(),
            abi: "abi".to_string(),
            sources: BTreeMap::from([("file_name".into(), "content".into())]),
            contract_libraries: BTreeMap::from([("file_name:lib_name".into(), "0x1234".into())]),
            compiler_settings: "compiler_settings".to_string(),
            match_type: MatchType::Full,
        };
        let result = from_sourcify_success(verification_success);

        let expected = Source {
            file_name: "file_name".to_string(),
            contract_name: "contract_name".to_string(),
            compiler_version: "v0.8.17+commit.8df45f5f".to_string(),
            compiler_settings: "compiler_settings".to_string(),
            source_type: source::SourceType::Solidity.into(),
            source_files: BTreeMap::from([("file_name".into(), "content".into())]),
            constructor_arguments: Some("0x123456".into()),
            abi: Some("abi".to_string()),
            match_type: source::MatchType::Full.into(),
            compilation_artifacts: None,
            creation_input_artifacts: None,
            deployed_bytecode_artifacts: None,
            is_blueprint: false,
            libraries: BTreeMap::from([("file_name:lib_name".into(), "0x1234".into())]),
        };

        assert_eq!(expected, result);
    }
}

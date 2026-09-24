use std::fmt::{Display, Formatter};

#[derive(thiserror::Error, Debug)]
pub enum RequestParseError {
    #[error("content is not a valid standard json: {0}")]
    InvalidContent(#[from] serde_path_to_error::Error<serde_json::Error>),
    #[error("{0:#}")]
    BadRequest(#[from] anyhow::Error),
}

/// The enum representing how provided bytecode corresponds
/// to the local result of source codes compilation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchType {
    Partial,
    Full,
}

impl From<sourcify::MatchType> for MatchType {
    fn from(value: sourcify::MatchType) -> Self {
        match value {
            sourcify::MatchType::Full => MatchType::Full,
            sourcify::MatchType::Partial => MatchType::Partial,
        }
    }
}

pub struct Contract {
    pub creation_code: Option<Vec<u8>>,
    pub runtime_code: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialOrd, PartialEq, Hash, Eq, Ord)]
pub struct FullyQualifiedName {
    file_name: String,
    contract_name: String,
}

impl Display for FullyQualifiedName {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.fully_qualified_name())
    }
}

impl FullyQualifiedName {
    pub fn from_file_and_contract_names(
        file_name: impl Into<String>,
        contract_name: impl Into<String>,
    ) -> Self {
        Self {
            file_name: file_name.into(),
            contract_name: contract_name.into(),
        }
    }

    pub fn fully_qualified_name(&self) -> String {
        format!("{}:{}", &self.file_name, &self.contract_name)
    }

    pub fn file_name(&self) -> String {
        self.file_name.clone()
    }

    pub fn contract_name(&self) -> String {
        self.contract_name.clone()
    }
}

#[derive(Clone, Copy, Debug, PartialOrd, PartialEq, Hash, Eq, Ord)]
pub enum Language {
    Solidity,
    Yul,
    Vyper,
}

/// The contract to be verified.
#[derive(Clone, Debug)]
pub struct OnChainContract {
    pub code: OnChainCode,
    pub chain_id: Option<String>,
    pub address: Option<alloy_core::primitives::Address>,
}

#[derive(Clone, Debug)]
pub struct OnChainCode {
    pub(crate) runtime: Option<Vec<u8>>,
    pub(crate) creation: Option<Vec<u8>>,
}

impl OnChainCode {
    pub fn runtime(runtime_code: Vec<u8>) -> Self {
        Self {
            runtime: Some(runtime_code),
            creation: None,
        }
    }

    pub fn creation(creation_code: Vec<u8>) -> Self {
        Self {
            runtime: None,
            creation: Some(creation_code),
        }
    }

    pub fn complete(runtime_code: Vec<u8>, creation_code: Vec<u8>) -> Self {
        Self {
            runtime: Some(runtime_code),
            creation: Some(creation_code),
        }
    }
}

use super::{
    compilation,
    evm_compilers::{EvmCompiler, EvmCompilersPool},
};
use crate::{DetailedVersion, ListFetcher};
use std::sync::Arc;
use tokio::sync::Semaphore;

async fn compilers<Compiler: EvmCompiler>(list_url: &str) -> EvmCompilersPool<Compiler> {
    let tempdir = tempfile::tempdir().unwrap();
    let url = list_url.try_into().expect("Getting url");
    let fetcher = ListFetcher::<DetailedVersion>::new(url, tempdir.into_path(), None, None)
        .await
        .expect("Fetch releases");
    let threads_semaphore = Arc::new(Semaphore::new(1));
    EvmCompilersPool::new(Arc::new(fetcher), threads_semaphore)
}

mod solidity {
    use super::*;
    use crate::{
        verify::{
            solc_compiler::{SolcCompiler, SolcInput},
            Error,
        },
        DEFAULT_SOLIDITY_COMPILER_LIST,
    };
    use foundry_compilers::artifacts;
    use std::str::FromStr;

    async fn compilers() -> EvmCompilersPool<SolcCompiler> {
        super::compilers(DEFAULT_SOLIDITY_COMPILER_LIST).await
    }

    struct Input {
        source_code: String,
    }

    impl Input {
        pub fn with_source_code(source_code: String) -> Self {
            Self { source_code }
        }
    }

    impl From<Input> for SolcInput {
        fn from(input: Input) -> Self {
            let mut compiler_input = artifacts::SolcInput {
                language: artifacts::SolcLanguage::Solidity,
                sources: artifacts::Sources::from([(
                    "source.sol".into(),
                    artifacts::Source::new(input.source_code),
                )]),
                settings: Default::default(),
            };
            compiler_input.settings.evm_version = None;
            SolcInput(compiler_input)
        }
    }

    #[tokio::test]
    async fn successful_compilation() {
        let source_code = r#"
            pragma solidity ^0.8.10;
            // SPDX-License-Identifier: MIT

            contract SimpleStorage {
                uint storedData;

                function set(uint x) public {
                    storedData = x;
                }

                function get() public view returns (uint) {
                    return storedData;
                }
            }"#;

        let compilers = compilers().await;
        let input = Input::with_source_code(source_code.into()).into();
        let version =
            DetailedVersion::from_str("v0.8.10+commit.fc410830").expect("Compiler version");

        let result = compilation::compile(&compilers, &version, input)
            .await
            .expect("Compilation failed");

        assert!(
            !result.artifacts.is_empty(),
            "Result should consists of at least one contract"
        );
    }

    #[tokio::test]
    async fn can_compile_large_file() {
        let source_code = include_str!("../tests/data/large_smart_contract.sol");

        let compilers = compilers().await;
        let input = Input::with_source_code(source_code.into()).into();
        let version =
            DetailedVersion::from_str("v0.5.9+commit.c68bc34e").expect("Compiler version");

        let result = compilation::compile(&compilers, &version, input)
            .await
            .expect("Compilation failed");

        assert!(
            !result.artifacts.is_empty(),
            "Result should consists of at least one contract"
        );
    }

    #[tokio::test]
    async fn returns_compilation_error() {
        let source_code = r#"pragma solidity ^0.8.10; cont SimpleStorage {"#;

        let compilers = compilers().await;
        let input = Input::with_source_code(source_code.into()).into();
        let version =
            DetailedVersion::from_str("v0.8.10+commit.fc410830").expect("Compiler version");

        let result = compilation::compile(&compilers, &version, input)
            .await
            .expect_err("Compilation should fail");
        match result {
            Error::Compilation(errors) => {
                assert!(errors.into_iter().any(|err| err.contains("ParserError")))
            }
            _ => panic!("Invalid compilation error: {result:?}"),
        }
    }
}

mod vyper {
    use super::*;
    use crate::{
        verify::vyper_compiler::{VyperCompiler, VyperInput},
        DetailedVersion, FullyQualifiedName, DEFAULT_VYPER_COMPILER_LIST,
    };
    use foundry_compilers::artifacts::Source;
    use std::{
        collections::{BTreeMap, HashSet},
        path::PathBuf,
        str::FromStr,
    };

    async fn compilers() -> EvmCompilersPool<VyperCompiler> {
        super::compilers(DEFAULT_VYPER_COMPILER_LIST).await
    }

    fn input_with_sources(sources: BTreeMap<PathBuf, String>) -> VyperInput {
        let mut compiler_input = VyperInput {
            language: "Vyper".to_string(),
            sources: sources
                .into_iter()
                .map(|(name, content)| (name, Source::new(content)))
                .collect(),
            interfaces: Default::default(),
            settings: Default::default(),
        };
        compiler_input.settings.evm_version = None;
        compiler_input
    }

    fn input_with_source(source_code: String) -> VyperInput {
        input_with_sources(BTreeMap::from([("source.vy".into(), source_code)]))
    }

    #[tokio::test]
    async fn compile_success() {
        let source_code = r#"
# @version ^0.3.1

userName: public(String[100])

@external
def __init__(name: String[100]):
    self.userName = name

@view
@external
def getUserName() -> String[100]:
    return self.userName
"#;

        let compilers = compilers().await;
        let input = input_with_source(source_code.into());
        let version = DetailedVersion::from_str("0.3.6+commit.4a2124d0").expect("Compiler version");

        let result = compilation::compile(&compilers, &version, input)
            .await
            .expect("Compilation failed");

        let contracts: HashSet<_> = result.artifacts.keys().cloned().collect();

        assert_eq!(
            contracts,
            HashSet::from_iter([FullyQualifiedName::from_file_and_contract_names(
                "source.vy",
                "source"
            )]),
            "compilation output should contain 1 contract",
        )
    }

    #[tokio::test]
    async fn compile_failed() {
        let compilers = compilers().await;
        let version =
            DetailedVersion::from_str("v0.2.11+commit.5db35ef").expect("Compiler version");

        for sources in [
            BTreeMap::from_iter([("source.vy".into(), "some wrong vyper code".into())]),
            BTreeMap::from_iter([(
                "source.vy".into(),
                "\n\n# @version =0.3.1\n\n# wrong vyper version".into(),
            )]),
        ] {
            let input = input_with_sources(sources);
            compilation::compile(&compilers, &version, input)
                .await
                .expect_err("Compilation should fail");
        }
    }
}

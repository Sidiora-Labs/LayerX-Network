mod cbor_auxdata;
mod compilation;
mod compiler_output;
mod evm_compilers;
mod solc_compiler;
mod solc_compiler_cli;
mod verification;
mod verifier;
mod vyper_compiler;

#[cfg(test)]
mod test_compilation;

pub mod vyper_compiler_input;

pub use evm_compilers::EvmCompilersPool;
pub use solc_compiler::{SolcCompiler, SolcInput};
pub use verifier::{compile_and_verify, VerificationResult, VerifyingContract};
pub use vyper_compiler::{VyperCompiler, VyperInput};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("One of creation or runtime code is blueprint while another is not; chain_id={chain_id:?}, address={address:?}")]
    NotConsistentBlueprintOnChainCode {
        chain_id: Option<String>,
        address: Option<String>,
    },
    #[error("Compiler version not found: {0}")]
    CompilerNotFound(String),
    #[error("Compilation error: {0:#?}")]
    Compilation(Vec<String>),
    #[error("{0:#?}")]
    Internal(#[from] anyhow::Error),
}

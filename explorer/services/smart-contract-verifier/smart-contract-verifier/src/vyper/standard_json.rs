use crate::{
    compiler::DetailedVersion, verify, Error, EvmCompilersPool, OnChainContract,
    VerificationResult, VyperCompiler, VyperInput,
};

pub type Content = VyperInput;

#[derive(Clone, Debug)]
pub struct VerificationRequest {
    pub contract: OnChainContract,
    pub compiler_version: DetailedVersion,
    pub content: Content,
}

pub async fn verify(
    compilers: &EvmCompilersPool<VyperCompiler>,
    request: VerificationRequest,
) -> Result<VerificationResult, Error> {
    let to_verify = vec![request.contract];

    let results = verify::compile_and_verify(
        to_verify,
        compilers,
        &request.compiler_version,
        request.content,
    )
    .await?;
    let result = results
        .into_iter()
        .next()
        .expect("we sent exactly one contract to verify");

    Ok(result)
}

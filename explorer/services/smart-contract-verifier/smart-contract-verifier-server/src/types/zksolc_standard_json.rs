use super::StandardJsonParseError;
use crate::proto::zksync::solidity::VerifyStandardJsonRequest;
use amplify::{From, Wrapper};
use anyhow::anyhow;
use blockscout_display_bytes::Bytes as DisplayBytes;
use smart_contract_verifier::{
    zksync::{zksolc_standard_json, VerificationRequest},
    CompactVersion, DetailedVersion,
};
use std::str::FromStr;

#[derive(Wrapper, From, Clone, Debug, PartialEq)]
pub struct VerifyStandardJsonRequestWrapper(VerifyStandardJsonRequest);

impl TryFrom<VerifyStandardJsonRequestWrapper> for VerificationRequest {
    type Error = StandardJsonParseError;

    fn try_from(request: VerifyStandardJsonRequestWrapper) -> Result<Self, Self::Error> {
        let request = request.into_inner();

        let code = DisplayBytes::from_str(&request.code)
            .map_err(|err| anyhow!("Invalid deployed bytecode: {:#?}", err))?
            .0;
        let constructor_arguments = request
            .constructor_arguments
            .as_deref()
            .map(DisplayBytes::from_str)
            .transpose()
            .map_err(|err| anyhow!("Invalid constructor arguments: {:#?}", err))?
            .map(|v| v.0);
        let zk_compiler = CompactVersion::from_str(&request.zk_compiler)
            .map_err(|err| anyhow!("Invalid zk compiler: {}", err))?;
        let solc_compiler = DetailedVersion::from_str(&request.solc_compiler)
            .map_err(|err| anyhow!("Invalid solc compiler: {}", err))?;

        let content: zksolc_standard_json::input::Input = serde_json::from_str(&request.input)?;

        Ok(Self {
            code,
            constructor_arguments,
            zk_compiler,
            solc_compiler,
            content,
        })
    }
}

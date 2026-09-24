use crate::proto::VerifySourcifyRequest;
use serde::{Deserialize, Serialize};
use smart_contract_verifier::sourcify::api::VerificationRequest;
use std::ops::Deref;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct VerifySourcifyRequestWrapper(VerifySourcifyRequest);

impl From<VerifySourcifyRequest> for VerifySourcifyRequestWrapper {
    fn from(inner: VerifySourcifyRequest) -> Self {
        Self(inner)
    }
}

impl Deref for VerifySourcifyRequestWrapper {
    type Target = VerifySourcifyRequest;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl VerifySourcifyRequestWrapper {
    pub fn new(inner: VerifySourcifyRequest) -> Self {
        Self(inner)
    }

    pub fn into_inner(self) -> VerifySourcifyRequest {
        self.0
    }
}

impl TryFrom<VerifySourcifyRequestWrapper> for VerificationRequest {
    type Error = tonic::Status;

    fn try_from(request: VerifySourcifyRequestWrapper) -> Result<Self, Self::Error> {
        let request = request.into_inner();
        Ok(Self {
            address: request.address,
            chain: request.chain,
            files: request.files,
            chosen_contract: request.chosen_contract.map(|i| i as usize),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::collections::BTreeMap;

    #[test]
    fn try_into_verification_request() {
        let request = VerifySourcifyRequest {
            address: "0x0123456789abcdef".to_string(),
            chain: "77".to_string(),
            files: BTreeMap::from([("metadata".into(), "metadata_content".into())]),
            chosen_contract: Some(2),
        };

        let verification_request: VerificationRequest =
            <VerifySourcifyRequestWrapper>::from(request)
                .try_into()
                .expect("Try_into verification request failed");

        let expected = VerificationRequest {
            address: "0x0123456789abcdef".to_string(),
            chain: "77".to_string(),
            files: BTreeMap::from([("metadata".into(), "metadata_content".into())]),
            chosen_contract: Some(2),
        };

        assert_eq!(expected, verification_request);
    }
}

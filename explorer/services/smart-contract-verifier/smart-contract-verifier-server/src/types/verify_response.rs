use crate::proto::{
    verify_response::{ExtraData, PostActionResponses, Status},
    Source, VerifyResponse,
};
use serde::{Deserialize, Serialize};
use smart_contract_verifier::SourcifySuccess;
use std::{fmt::Display, ops::Deref};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct VerifyResponseWrapper(VerifyResponse);

impl From<VerifyResponse> for VerifyResponseWrapper {
    fn from(inner: VerifyResponse) -> Self {
        Self(inner)
    }
}

impl Deref for VerifyResponseWrapper {
    type Target = VerifyResponse;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl VerifyResponseWrapper {
    pub fn into_inner(self) -> VerifyResponse {
        self.0
    }
}

pub trait VerifyResponseOk {
    fn result(self) -> (Source, ExtraData);
}

impl VerifyResponseOk for SourcifySuccess {
    fn result(self) -> (Source, ExtraData) {
        let extra_data = ExtraData {
            local_creation_input_parts: vec![],
            local_deployed_bytecode_parts: vec![],
        };
        let source = super::source::from_sourcify_success(self);

        (source, extra_data)
    }
}

impl VerifyResponseWrapper {
    pub fn ok<T: VerifyResponseOk>(success: T) -> Self {
        let (source, extra_data) = success.result();
        VerifyResponse {
            message: "OK".to_string(),
            status: Status::Success.into(),
            source: Some(source),
            extra_data: Some(extra_data),
            post_action_responses: Some(PostActionResponses {
                lookup_methods: None,
            }),
        }
        .into()
    }

    pub fn err(message: impl Display) -> Self {
        VerifyResponse {
            message: message.to_string(),
            status: Status::Failure.into(),
            source: None,
            extra_data: None,
            post_action_responses: None,
        }
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use smart_contract_verifier::MatchType;

    #[test]
    fn ok_verify_response() {
        let verification_success = SourcifySuccess {
            file_name: "file_path".to_string(),
            contract_name: "contract_name".to_string(),
            compiler_version: "v0.8.17+commit.8df45f5f".to_string(),
            evm_version: None,
            optimization: None,
            optimization_runs: None,
            constructor_arguments: None,
            contract_libraries: Default::default(),
            abi: "[]".to_string(),
            sources: Default::default(),
            compiler_settings: "{}".to_string(),
            match_type: MatchType::Partial,
        };

        let response = VerifyResponseWrapper::ok(verification_success.clone()).into_inner();

        let expected = VerifyResponse {
            message: "OK".to_string(),
            status: Status::Success.into(),
            source: Some(super::super::source::from_sourcify_success(
                verification_success,
            )),
            extra_data: Some(ExtraData {
                local_creation_input_parts: vec![],
                local_deployed_bytecode_parts: vec![],
            }),
            post_action_responses: Some(PostActionResponses {
                lookup_methods: None,
            }),
        };

        assert_eq!(expected, response);
    }

    #[test]
    fn err_verify_response() {
        let response = VerifyResponseWrapper::err("parse error").into_inner();
        let expected = VerifyResponse {
            message: "parse error".to_string(),
            status: Status::Failure.into(),
            source: None,
            extra_data: None,
            post_action_responses: None,
        };
        assert_eq!(expected, response);
    }
}

use crate::proto;
use alloy_json_abi::JsonAbi;
use amplify::{From, Wrapper};
use blockscout_display_bytes::Bytes as DisplayBytes;
use foundry_compilers::artifacts::sourcemap;
use smart_contract_verifier::{LookupMethodsRequest, LookupMethodsResponse};
use std::{collections::BTreeMap, str::FromStr};

#[derive(Wrapper, From, Clone, Debug, PartialEq)]
pub struct LookupMethodsRequestWrapper(proto::LookupMethodsRequest);

impl TryFrom<LookupMethodsRequestWrapper> for LookupMethodsRequest {
    type Error = tonic::Status;

    fn try_from(request: LookupMethodsRequestWrapper) -> Result<Self, Self::Error> {
        let bytecode = DisplayBytes::from_str(&request.0.bytecode)
            .map_err(|e| tonic::Status::invalid_argument(format!("Invalid bytecode: {e:?}")))?
            .0;
        let abi = JsonAbi::from_json_str(&request.0.abi)
            .map_err(|e| tonic::Status::invalid_argument(format!("Invalid abi: {e:?}")))?;
        let source_map = sourcemap::parse(&request.0.source_map)
            .map_err(|e| tonic::Status::invalid_argument(format!("Invalid source_map: {e:?}")))?;
        let file_ids = request.0.file_ids.clone();

        Ok(Self {
            bytecode,
            abi,
            source_map,
            file_ids,
        })
    }
}

#[derive(Wrapper, From, Clone, Debug, PartialEq)]
pub struct LookupMethodsResponseWrapper(proto::LookupMethodsResponse);

impl From<LookupMethodsResponse> for LookupMethodsResponseWrapper {
    fn from(response: LookupMethodsResponse) -> Self {
        Self(proto::LookupMethodsResponse {
            methods: response
                .methods
                .into_iter()
                .map(|(selector, method)| {
                    (
                        selector,
                        proto::lookup_methods_response::Method {
                            file_name: method.filename,
                            file_offset: method.offset as u32,
                            length: method.length as u32,
                        },
                    )
                })
                .collect::<BTreeMap<String, proto::lookup_methods_response::Method>>(),
        })
    }
}

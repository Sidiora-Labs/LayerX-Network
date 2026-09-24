use async_trait::async_trait;
use ethabi::{ethereum_types::H256, RawLog};
use sig_provider::SourceAggregator;
use sig_provider_proto::blockscout::sig_provider::v1::{
    abi_service_server::AbiService, signature_service_server::SignatureService,
    BatchGetEventAbisRequest, BatchGetEventAbisResponse, CreateSignaturesRequest,
    CreateSignaturesResponse, GetEventAbiRequest, GetEventAbiResponse, GetFunctionAbiRequest,
    GetFunctionAbiResponse,
};
use std::sync::Arc;

#[derive(Clone)]
pub struct Service {
    agg: Arc<SourceAggregator>,
}

impl Service {
    pub fn new(agg: Arc<SourceAggregator>) -> Self {
        Self { agg }
    }
}

#[async_trait]
impl SignatureService for Service {
    async fn create_signatures(
        &self,
        request: tonic::Request<CreateSignaturesRequest>,
    ) -> Result<tonic::Response<CreateSignaturesResponse>, tonic::Status> {
        let request = request.into_inner();
        let agg = self.agg.clone();
        tokio::spawn(async move {
            let _result = agg.create_signatures(&request.abi).await;
        });
        Ok(tonic::Response::new(CreateSignaturesResponse {}))
    }
}

// todo: remove once https://github.com/hyperium/tonic/pull/2282 is released
#[allow(clippy::result_large_err)]
fn decode(str: &str) -> Result<Vec<u8>, tonic::Status> {
    hex::decode(str.strip_prefix("0x").unwrap_or(str))
        .map_err(|e| tonic::Status::invalid_argument(e.to_string()))
}

#[async_trait]
impl AbiService for Service {
    async fn get_function_abi(
        &self,
        request: tonic::Request<GetFunctionAbiRequest>,
    ) -> Result<tonic::Response<GetFunctionAbiResponse>, tonic::Status> {
        let request = request.into_inner();
        let bytes = decode(&request.tx_input)?;
        self.agg
            .get_function_abi(&bytes)
            .await
            .map(|abi| tonic::Response::new(GetFunctionAbiResponse { abi }))
            .map_err(|e| tonic::Status::internal(e.to_string()))
    }

    async fn get_event_abi(
        &self,
        request: tonic::Request<GetEventAbiRequest>,
    ) -> Result<tonic::Response<GetEventAbiResponse>, tonic::Status> {
        let request = request.into_inner();

        let topics = parse_topics(request.topics)?;
        self.agg
            .get_event_abi(RawLog {
                data: decode(&request.data)?,
                topics,
            })
            .await
            .map(|abi| GetEventAbiResponse { abi })
            .map_err(|e| tonic::Status::internal(e.to_string()))
            .map(tonic::Response::new)
    }

    async fn batch_get_event_abis(
        &self,
        request: tonic::Request<BatchGetEventAbisRequest>,
    ) -> Result<tonic::Response<BatchGetEventAbisResponse>, tonic::Status> {
        let batch_request = request.into_inner();

        let mut raw_logs = Vec::new();
        for request in batch_request.requests {
            let topics = parse_topics(request.topics)?;
            raw_logs.push(RawLog {
                data: decode(&request.data)?,
                topics,
            });
        }

        let batch_abis = self
            .agg
            .batch_get_event_abi(raw_logs)
            .await
            .map_err(|e| tonic::Status::internal(e.to_string()))?;

        let mut responses = Vec::new();
        for abi in batch_abis {
            responses.push(GetEventAbiResponse { abi })
        }

        Ok(tonic::Response::new(BatchGetEventAbisResponse {
            responses,
        }))
    }
}

// todo: remove once https://github.com/hyperium/tonic/pull/2282 is released
#[allow(clippy::result_large_err)]
fn parse_topics(topics: String) -> Result<Vec<H256>, tonic::Status> {
    topics
        .split(',')
        .map(|topic| {
            let hex = decode(topic)?;
            if hex.len() != 32 {
                return Err(tonic::Status::invalid_argument(
                    "topic len must be 32 bytes",
                ));
            }
            Ok(H256::from_slice(&hex))
        })
        .collect()
}

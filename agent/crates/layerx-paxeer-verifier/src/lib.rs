#![forbid(unsafe_code)]

mod abi;
mod checkpoint;
mod contract;
mod encoding;
mod json;
mod publication;
mod rpc;

pub use checkpoint::{
    PaxeerCheckpointPolicy, PaxeerCheckpointVerifier, VerifiedCheckpointPublication,
};
pub use json::{parse as parse_json, Json, JsonError, JsonErrorReason};
pub use publication::{publication, publication_at, BlockAnchor, Publication};
pub use rpc::{
    canonical_endpoint_identity, raw_call, EndpointConfig, EndpointFailure, EndpointFault,
    EndpointTransport,
};

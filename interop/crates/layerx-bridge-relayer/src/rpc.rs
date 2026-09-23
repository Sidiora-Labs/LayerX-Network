//! JSON-RPC transport seam. Ethereum reads go through the strict-majority
//! HTTPS quorum of `layerx_mirror::rpc::RpcCluster`; Paxeer reads go through
//! the pinned-TLS transport of `layerx_paxeer_verifier::raw_call`, with a
//! strict majority across the configured Paxeer endpoints.

use std::collections::BTreeMap;
use std::fmt;

use layerx_mirror::rpc::{BroadcastResult, RpcCluster, RpcError};
use layerx_paxeer_verifier::{raw_call, EndpointConfig, EndpointFault, Json};
use serde_json::{json, Value};

use crate::hex;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RpcFault {
    Configuration,
    Unavailable,
    RateLimited { retry_after_seconds: u64 },
    Divergence,
    Malformed,
    Rejected { code: i64, message: String },
}

impl fmt::Display for RpcFault {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Configuration => formatter.write_str("rpc configuration refused"),
            Self::Unavailable => formatter.write_str("rpc quorum unavailable"),
            Self::RateLimited {
                retry_after_seconds,
            } => write!(formatter, "rpc rate limited for {retry_after_seconds}s"),
            Self::Divergence => formatter.write_str("rpc endpoints diverge"),
            Self::Malformed => formatter.write_str("rpc response malformed"),
            Self::Rejected { code, message } => write!(formatter, "rpc rejected {code}: {message}"),
        }
    }
}

impl std::error::Error for RpcFault {}

impl From<RpcError> for RpcFault {
    fn from(value: RpcError) -> Self {
        match value {
            RpcError::Configuration => Self::Configuration,
            RpcError::Unavailable => Self::Unavailable,
            RpcError::RateLimited {
                retry_after_seconds,
            } => Self::RateLimited {
                retry_after_seconds,
            },
            RpcError::Divergence => Self::Divergence,
            RpcError::ResponseMismatch => Self::Malformed,
            RpcError::Rejected { code, message } => Self::Rejected { code, message },
        }
    }
}

/// Result of broadcasting already signed bytes. `Unknown` is never
/// permission to sign a replacement: the same bytes are rebroadcast.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SendOutcome {
    Accepted,
    Unknown,
}

/// One chain's JSON-RPC. Reads must be agreed values; sends take the exact
/// signed bytes and the transaction hash they must be acknowledged with.
pub trait JsonRpc {
    /// # Errors
    ///
    /// Returns the transport's fault when no agreed value is available.
    fn call(&self, method: &str, params: Value) -> Result<Value, RpcFault>;

    /// # Errors
    ///
    /// Returns `Rejected` when the endpoints deterministically refuse the
    /// transaction.
    fn send_raw_transaction(&self, raw: &[u8], hash: &[u8; 32]) -> Result<SendOutcome, RpcFault>;
}

impl JsonRpc for RpcCluster {
    fn call(&self, method: &str, params: Value) -> Result<Value, RpcFault> {
        RpcCluster::call(self, method, params).map_err(RpcFault::from)
    }

    fn send_raw_transaction(&self, raw: &[u8], hash: &[u8; 32]) -> Result<SendOutcome, RpcFault> {
        match self.broadcast(
            "eth_sendRawTransaction",
            json!([hex::prefixed(raw)]),
            &hex::prefixed(hash),
        ) {
            Ok(BroadcastResult::Accepted) => Ok(SendOutcome::Accepted),
            Ok(BroadcastResult::Unknown) => Ok(SendOutcome::Unknown),
            Err(error) => Err(RpcFault::from(error)),
        }
    }
}

/// Paxeer JSON-RPC over the endpoints `layerx_paxeer_verifier` authenticates
/// (pinned TLS, or an explicit loopback emulator). Every request re-checks
/// the endpoint's chain id; a value is accepted only when a strict majority of
/// the configured endpoints return it byte for byte.
pub struct PaxeerRpc {
    endpoints: Vec<EndpointConfig>,
}

impl PaxeerRpc {
    /// # Errors
    ///
    /// Refuses an empty or oversized endpoint list, or endpoints bound to
    /// different chain ids.
    pub fn new(endpoints: Vec<EndpointConfig>) -> Result<Self, RpcFault> {
        let Some(first) = endpoints.first() else {
            return Err(RpcFault::Configuration);
        };
        if endpoints.len() > 8
            || endpoints
                .iter()
                .any(|endpoint| endpoint.expected_chain_id != first.expected_chain_id)
        {
            return Err(RpcFault::Configuration);
        }
        Ok(Self { endpoints })
    }

    fn majority(&self) -> usize {
        self.endpoints.len() / 2 + 1
    }

    fn request(&self, method: &str, params: &Value) -> Vec<Result<Value, EndpointFault>> {
        let converted = params
            .as_array()
            .map(|values| {
                values
                    .iter()
                    .map(|value| layerx_paxeer_verifier::parse_json(&value.to_string()))
                    .collect::<Result<Vec<Json>, _>>()
            })
            .unwrap_or_else(|| Ok(Vec::new()));
        let Ok(converted) = converted else {
            return vec![Err(EndpointFault::MalformedResponse)];
        };
        self.endpoints
            .iter()
            .map(|endpoint| {
                raw_call(endpoint, method, &converted)
                    .map_err(|failure| failure.fault)
                    .and_then(|value| {
                        serde_json::from_str::<Value>(&value.render())
                            .map_err(|_| EndpointFault::MalformedResponse)
                    })
            })
            .collect()
    }
}

impl JsonRpc for PaxeerRpc {
    fn call(&self, method: &str, params: Value) -> Result<Value, RpcFault> {
        let mut agreed: BTreeMap<String, (usize, Value)> = BTreeMap::new();
        let mut refusals: BTreeMap<(i64, String), usize> = BTreeMap::new();
        for outcome in self.request(method, &params) {
            match outcome {
                Ok(value) => {
                    let entry = agreed.entry(value.to_string()).or_insert((0, value));
                    entry.0 += 1;
                }
                Err(EndpointFault::Rpc { code, message }) => {
                    *refusals.entry((code, message)).or_default() += 1;
                }
                Err(_) => {}
            }
        }
        let majority = self.majority();
        if let Some((_, value)) = agreed.into_values().find(|(count, _)| *count >= majority) {
            return Ok(value);
        }
        if let Some(((code, message), _)) =
            refusals.into_iter().find(|(_, count)| *count >= majority)
        {
            return Err(RpcFault::Rejected { code, message });
        }
        Err(RpcFault::Unavailable)
    }

    fn send_raw_transaction(&self, raw: &[u8], hash: &[u8; 32]) -> Result<SendOutcome, RpcFault> {
        let expected = hex::prefixed(hash);
        let mut refusals: BTreeMap<(i64, String), usize> = BTreeMap::new();
        for outcome in self.request("eth_sendRawTransaction", &json!([hex::prefixed(raw)])) {
            match outcome {
                Ok(value) if value.as_str() == Some(expected.as_str()) => {
                    return Ok(SendOutcome::Accepted);
                }
                Err(EndpointFault::Rpc { code, message }) => {
                    *refusals.entry((code, message)).or_default() += 1;
                }
                _ => {}
            }
        }
        if let Some(((code, message), _)) = refusals
            .into_iter()
            .find(|(_, count)| *count >= self.majority())
        {
            return Err(RpcFault::Rejected { code, message });
        }
        Ok(SendOutcome::Unknown)
    }
}

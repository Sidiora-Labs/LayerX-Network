pub use crate::rpc_subscription::{RpcSubscription, SubscriptionTopic};
use serde_json::{json, Value};
pub type RpcValue = Value;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::programs::{LayerXKeyCredential, ProgramOperationError};

#[derive(Debug)]
pub enum RpcError {
    Transport,
    Verification,
    Signing(layerx_crypto::signer::SignError),
    StaleSourceSequence,
    MissingFinalityTrust,
    Pending {
        activity_id: [u8; 32],
    },
    InvalidResponse,
    InvalidRequest,
    Configuration(ProgramOperationError),
    Remote {
        code: i64,
        message: String,
        data: Option<Value>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Commitment {
    Executed,
    Batched,
    Finalised,
}

impl Commitment {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Executed => "executed",
            Self::Batched => "batched",
            Self::Finalised => "finalised",
        }
    }
}

pub struct RpcClient {
    agent: ureq::Agent,
    endpoint: url::Url,
    credential: Option<LayerXKeyCredential>,
    next_id: AtomicU64,
}

macro_rules! read_methods {
    ($($name:ident => $method:literal),+ $(,)?) => {$(
        /// # Errors
        /// Preserves RPC refusals and rejects malformed or mismatched responses.
        pub fn $name(&self, selector: &str) -> Result<Value, RpcError> {
            if selector.is_empty() { return Err(RpcError::InvalidRequest); }
            self.call($method, &json!([selector]))
        }
    )+};
}

pub struct RpcWallet<'a> {
    rpc: &'a RpcClient,
    native_asset: [u8; 32],
}

impl RpcWallet<'_> {
    /// # Errors
    /// Preserves unavailable enumeration and other RPC errors.
    pub fn accounts(&self, did: &str) -> Result<Value, RpcError> {
        self.rpc.get_balances(did)
    }

    /// # Errors
    /// Rejects invalid DIDs and preserves RPC errors. The returned read is unverified.
    pub fn balance(&self, did: &str, asset: [u8; 32]) -> Result<Value, RpcError> {
        let account = wallet_account(did, asset, self.native_asset)?;
        self.rpc.get_balance(&encode_hex(&account))
    }
}

/// # Errors
/// Rejects noncanonical DIDs or oversized account names.
pub fn wallet_account(
    did: &str,
    asset: [u8; 32],
    native_asset: [u8; 32],
) -> Result<[u8; 32], RpcError> {
    let account = layerx_types::account::AccountId::for_asset(did, asset, native_asset)
        .map_err(|_| RpcError::InvalidRequest)?;
    layerx_wire::hash::account_id_for_protocol(
        &account,
        layerx_wire::limits::STATE_COMMITMENT_PROTOCOL_VERSION,
    )
    .map_err(|_| RpcError::InvalidRequest)
}

impl RpcClient {
    #[must_use]
    pub const fn wallet(&self, native_asset: [u8; 32]) -> RpcWallet<'_> {
        RpcWallet {
            rpc: self,
            native_asset,
        }
    }

    /// # Errors
    /// Rejects insecure non-loopback endpoints and embedded credentials.
    pub fn connect(
        endpoint: &str,
        credential: Option<LayerXKeyCredential>,
    ) -> Result<Self, RpcError> {
        let mut endpoint =
            crate::programs::http::validate_endpoint(endpoint).map_err(RpcError::Configuration)?;
        let base = endpoint.path().trim_end_matches('/');
        let path = if base.ends_with("/rpc") {
            base.to_owned()
        } else {
            format!("{base}/rpc")
        };
        endpoint.set_path(&path);
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .http_status_as_error(false)
            .max_redirects(0)
            .build()
            .into();
        Ok(Self {
            agent,
            endpoint,
            credential,
            next_id: AtomicU64::new(1),
        })
    }

    read_methods! {
        get_account => "lx_getAccount", get_balance => "lx_getBalance",
        get_balances => "lx_getBalances", get_sequence => "lx_getSequence",
        get_receipt => "lx_getReceipt", get_activity_status => "lx_getActivityStatus",
        get_batch_header => "lx_getBatchHeader", get_checkpoint => "lx_getCheckpoint",
    }

    /// Opens an authenticated public subscription. Notifications are unverified hints.
    /// # Errors
    /// Refuses invalid topic selectors, transport failures and mismatched acknowledgements.
    pub fn subscribe(
        &self,
        topic: SubscriptionTopic,
        account: Option<[u8; 32]>,
    ) -> Result<RpcSubscription, RpcError> {
        crate::rpc_subscription::connect(&self.endpoint, self.credential.as_ref(), topic, account)
    }

    /// # Errors
    /// Preserves node-info RPC refusals.
    pub fn get_node_info(&self) -> Result<Value, RpcError> {
        self.call("lx_getNodeInfo", &json!([]))
    }

    /// # Errors
    /// Preserves native asset-listing refusals; returned read data is unverified.
    pub fn list_assets(&self) -> Result<Value, RpcError> {
        self.call("lx_listAssets", &json!([]))
    }

    /// # Errors
    /// Preserves native asset metadata refusals; returned read data is unverified.
    pub fn get_asset(&self, asset: [u8; 32]) -> Result<Value, RpcError> {
        self.call("lx_getAsset", &json!([encode_hex(&asset)]))
    }

    /// # Errors
    /// Refuses empty or oversized activities and preserves native fee-estimation errors.
    pub fn estimate_fee(&self, canonical: &[u8]) -> Result<Value, RpcError> {
        if canonical.is_empty() || canonical.len() > 524_288 {
            return Err(RpcError::InvalidRequest);
        }
        self.call("lx_estimateFee", &json!([encode_hex(canonical)]))
    }

    /// # Errors
    /// Rejects invalid proof selectors and preserves RPC refusals.
    pub fn get_proof(
        &self,
        kind: &str,
        activity: &str,
        account: Option<&str>,
    ) -> Result<Value, RpcError> {
        let params = match (kind, account) {
            ("activity" | "receipt", None) => json!([kind, activity]),
            ("account", Some(account)) => json!([kind, activity, account]),
            _ => return Err(RpcError::InvalidRequest),
        };
        self.call("lx_getProof", &params)
    }

    /// Returns the RPC outcome, including pending; this is not a verified receipt.
    /// # Errors
    /// Rejects empty or oversized bytes and preserves submission refusals.
    pub fn send_activity(
        &self,
        canonical: &[u8],
        commitment: Commitment,
    ) -> Result<Value, RpcError> {
        if canonical.is_empty() || canonical.len() > 524_288 {
            return Err(RpcError::InvalidRequest);
        }
        let hex: String = encode_hex(canonical);
        self.call("lx_sendActivity", &json!([hex, commitment.as_str()]))
    }

    fn call(&self, method: &str, params: &Value) -> Result<Value, RpcError> {
        let id = self
            .next_id
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .map_err(|_| RpcError::InvalidRequest)?;
        let body = serde_json::to_vec(
            &json!({"jsonrpc":"2.0", "id":id.to_string(), "method":method, "params":params}),
        )
        .map_err(|_| RpcError::InvalidRequest)?;
        if body.len() > 1_048_576 + 4096 {
            return Err(RpcError::InvalidRequest);
        }
        let mut request = self
            .agent
            .post(self.endpoint.as_str())
            .header("Content-Type", "application/json")
            .header("Accept", "application/json");
        let authorization = self
            .credential
            .as_ref()
            .map(LayerXKeyCredential::authorization)
            .transpose()
            .map_err(RpcError::Configuration)?;
        if let Some(value) = authorization.as_deref() {
            request = request.header("Authorization", value);
        }
        let mut response = request
            .send(body.as_slice())
            .map_err(|_| RpcError::Transport)?;
        if response.status().as_u16() != 200
            || response
                .headers()
                .get("content-type")
                .and_then(|s| s.to_str().ok())
                .and_then(|s| s.split(';').next())
                .map(str::trim)
                != Some("application/json")
        {
            return Err(RpcError::InvalidResponse);
        }
        let bytes = response
            .body_mut()
            .with_config()
            .limit(9 * 1_048_576)
            .read_to_vec()
            .map_err(|_| RpcError::Transport)?;
        let document: Value =
            serde_json::from_slice(&bytes).map_err(|_| RpcError::InvalidResponse)?;
        decode_response(&document, &id.to_string())
    }
}

fn decode_response(document: &Value, id: &str) -> Result<Value, RpcError> {
    let object = document.as_object().ok_or(RpcError::InvalidResponse)?;
    if object.len() != 3 || document["jsonrpc"] != "2.0" || document["id"] != id {
        return Err(RpcError::InvalidResponse);
    }
    if let Some(error) = object.get("error") {
        return Err(RpcError::Remote {
            code: error
                .get("code")
                .and_then(Value::as_i64)
                .ok_or(RpcError::InvalidResponse)?,
            message: error
                .get("message")
                .and_then(Value::as_str)
                .ok_or(RpcError::InvalidResponse)?
                .into(),
            data: error.get("data").cloned(),
        });
    }
    object
        .get("result")
        .filter(|r| r.is_object())
        .cloned()
        .ok_or(RpcError::InvalidResponse)
}

pub(crate) fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|b| {
            [
                char::from(HEX[usize::from(b >> 4)]),
                char::from(HEX[usize::from(b & 15)]),
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wallet_account_vectors_preserve_native_and_token_namespaces() {
        assert_eq!(
            wallet_account("did:layerx:alice", [0; 32], [0; 32])
                .map(|id| encode_hex(&id))
                .ok()
                .as_deref(),
            Some("575498fd80da9b17115311af107ab11639acf474a69f944c9c9b1d0ea28ed205")
        );
        assert_eq!(
            wallet_account("did:layerx:alice", [0x11; 32], [0; 32])
                .map(|id| encode_hex(&id))
                .ok()
                .as_deref(),
            Some("c25ada37deae26ed54923dab01b56e9dded08b4ca710b0fd0b1d172f44404324")
        );
        assert!(wallet_account("did::alice", [0; 32], [0; 32]).is_err());
        assert!(RpcClient::connect("http://example.com", None).is_err());
        assert!(RpcClient::connect("https://user:secret@example.com", None).is_err());
    }

    #[test]
    fn response_correlation_and_errors_are_strict() {
        assert!(decode_response(&json!({"jsonrpc":"2.0","id":"1","result":{}}), "2").is_err());
        assert!(decode_response(
            &json!({"jsonrpc":"2.0","id":"1","result":{},"error":{}}),
            "1"
        )
        .is_err());
        assert!(matches!(
            decode_response(
                &json!({"jsonrpc":"2.0","id":"1","error":{"code":-32005,"message":"Read unavailable","data":{"retry":1}}}),
                "1"
            ),
            Err(RpcError::Remote { code: -32005, .. })
        ));
        assert_eq!(
            decode_response(
                &json!({"jsonrpc":"2.0","id":"1","result":{"state":"pending"}}),
                "1"
            )
            .ok(),
            Some(json!({"state":"pending"}))
        );
    }
    #[test]
    fn fee_estimation_refuses_invalid_lengths_before_transport() {
        let rpc =
            RpcClient::connect("http://127.0.0.1:1", None).unwrap_or_else(|e| panic!("{e:?}"));
        assert!(matches!(
            rpc.estimate_fee(&[]),
            Err(RpcError::InvalidRequest)
        ));
        assert!(matches!(
            rpc.estimate_fee(&vec![0; 524_289]),
            Err(RpcError::InvalidRequest)
        ));
    }
}

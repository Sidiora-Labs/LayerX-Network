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

#[cfg(test)]
#[path = "../../../../platform/tests/support/tls_boundary.rs"]
mod tls_boundary;

#[test]
fn rpc_system_tls_checks_the_actual_server_identity() {
    tls_boundary::qualify(
        "rpc::rpc_system_tls_checks_the_actual_server_identity",
        |endpoint| {
            let uppercase = endpoint.replacen("https://", "HTTPS://", 1);
            let client =
                RpcClient::connect(&uppercase, None).map_err(|error| format!("{error:?}"))?;
            client
                .agent
                .get(format!("{endpoint}/livez"))
                .call()
                .map_err(|error| error.to_string())?
                .body_mut()
                .read_to_vec()
                .map_err(|error| error.to_string())
        },
    );
}

#[test]
fn rpc_private_ca_tls_checks_the_actual_server_identity() {
    tls_boundary::qualify(
        "rpc::rpc_private_ca_tls_checks_the_actual_server_identity",
        |endpoint| {
            let ca = std::fs::read(
                std::env::var("LAYERX_TLS_QUAL_CA_DER").map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())?;
            let client = RpcClient::connect_with_ca_der(endpoint, None, &ca)
                .map_err(|error| format!("{error:?}"))?;
            client
                .agent
                .get(format!("{endpoint}/livez"))
                .call()
                .map_err(|error| error.to_string())?
                .body_mut()
                .read_to_vec()
                .map_err(|error| error.to_string())
        },
    );
}

#[test]
fn rpc_subscription_private_ca_checks_the_actual_server_identity() {
    use std::io::{Read as _, Write as _};
    tls_boundary::qualify(
        "rpc::rpc_subscription_private_ca_checks_the_actual_server_identity",
        |endpoint| {
            let ca = std::fs::read(
                std::env::var("LAYERX_TLS_QUAL_CA_DER").map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())?;
            let client = RpcClient::connect_with_ca_der(endpoint, None, &ca)
                .map_err(|error| format!("{error:?}"))?;
            let endpoint = url::Url::parse(endpoint).map_err(|error| error.to_string())?;
            let name = endpoint.host_str().ok_or("missing TLS host")?.to_owned();
            let host = rustls::pki_types::ServerName::try_from(name.clone())
                .map_err(|error| error.to_string())?;
            let configuration = client
                .subscription_tls
                .ok_or("missing subscription trust")?;
            let connection = rustls::ClientConnection::new(configuration, host)
                .map_err(|error| error.to_string())?;
            let socket =
                std::net::TcpStream::connect(("127.0.0.1", endpoint.port().ok_or("missing port")?))
                    .map_err(|error| error.to_string())?;
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .map_err(|error| error.to_string())?;
            let mut stream = rustls::StreamOwned::new(connection, socket);
            stream
                .write_all(
                    format!("GET /livez HTTP/1.1\r\nHost: {name}\r\nConnection: close\r\n\r\n")
                        .as_bytes(),
                )
                .map_err(|error| error.to_string())?;
            let mut header = Vec::new();
            while !header.ends_with(b"\r\n\r\n") {
                if header.len() >= 8192 {
                    return Err("HTTP header limit".to_owned());
                }
                let mut byte = [0];
                stream
                    .read_exact(&mut byte)
                    .map_err(|error| error.to_string())?;
                header.push(byte[0]);
            }
            let text = std::str::from_utf8(&header).map_err(|error| error.to_string())?;
            let length = text
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .ok_or("missing content length")?;
            if length > 8192 {
                return Err("HTTP body limit".to_owned());
            }
            let mut body = vec![0; length];
            stream
                .read_exact(&mut body)
                .map_err(|error| error.to_string())?;
            Ok(body)
        },
    );
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

macro_rules! read_result {
    ($($name:ident),+ $(,)?) => {$(
        #[derive(Clone, Debug, PartialEq)]
        pub struct $name(serde_json::Map<String, Value>);
        impl $name {
            #[must_use]
            pub const fn unverified_fields(&self) -> &serde_json::Map<String, Value> { &self.0 }
            #[must_use]
            pub fn into_value(self) -> Value { Value::Object(self.0) }
        }
        impl TryFrom<Value> for $name {
            type Error = RpcError;
            fn try_from(value: Value) -> Result<Self, Self::Error> {
                match value { Value::Object(fields) => Ok(Self(fields)), _ => Err(RpcError::InvalidResponse) }
            }
        }
    )+};
}
read_result!(BalancesSnapshot, FeeEstimate);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetMetadata {
    pub asset_id: [u8; 32],
    pub symbol: String,
    pub name: String,
    pub decimals: u8,
    pub custody_kind: u8,
    pub custody_reference: Vec<u8>,
    pub paused: bool,
    pub supply_cap: u128,
    pub issuer_did: [u8; 32],
    pub issuer_kind: u8,
    pub total_units: u128,
    pub salt: [u8; 32],
}

#[derive(Clone, Debug, PartialEq)]
pub struct AssetSnapshot {
    pub asset: AssetMetadata,
    pub observed_head_sequence: u64,
    pub state_root: [u8; 32],
    raw: serde_json::Map<String, Value>,
}

impl AssetSnapshot {
    #[must_use]
    pub const fn unverified_fields(&self) -> &serde_json::Map<String, Value> {
        &self.raw
    }

    #[must_use]
    pub fn into_value(self) -> Value {
        Value::Object(self.raw)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AssetListSnapshot {
    pub assets: Vec<AssetMetadata>,
    pub observed_head_sequence: u64,
    pub state_root: [u8; 32],
    raw: serde_json::Map<String, Value>,
}

impl AssetListSnapshot {
    #[must_use]
    pub const fn unverified_fields(&self) -> &serde_json::Map<String, Value> {
        &self.raw
    }

    #[must_use]
    pub fn into_value(self) -> Value {
        Value::Object(self.raw)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdentitySequenceSnapshot {
    pub did: String,
    pub next_sequence: u64,
    pub observed_head_sequence: u64,
    pub state_root: [u8; 32],
}

/// One faucet grant the gateway confirmed as funded for the requesting DID.
///
/// Only a complete grant becomes a value: an unfunded, pending or incomplete
/// faucet document is an [`RpcError::InvalidResponse`], never a zero amount.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FaucetGrant {
    pub funding_id: String,
    pub transaction_id: String,
    pub amount: u128,
    pub network: String,
    raw: serde_json::Map<String, Value>,
}

impl FaucetGrant {
    #[must_use]
    pub const fn unverified_fields(&self) -> &serde_json::Map<String, Value> {
        &self.raw
    }

    #[must_use]
    pub fn into_value(self) -> Value {
        Value::Object(self.raw)
    }
}

impl TryFrom<Value> for FaucetGrant {
    type Error = RpcError;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        let fields = object(&value)?;
        if fields.get("funded") != Some(&Value::Bool(true)) {
            return Err(RpcError::InvalidResponse);
        }
        let funding_id = text_field(fields, "funding_id")?;
        if funding_id.is_empty()
            || funding_id.len() > 128
            || !funding_id.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(RpcError::InvalidResponse);
        }
        let transaction_id = text_field(fields, "transaction_id")?;
        if transaction_id.is_empty() || transaction_id.len() > 128 {
            return Err(RpcError::InvalidResponse);
        }
        let network = text_field(fields, "network")?;
        if network.is_empty() || network.len() > 64 {
            return Err(RpcError::InvalidResponse);
        }
        let amount = decimal_u128_field(fields, "amount")?;
        if amount == 0 {
            return Err(RpcError::InvalidResponse);
        }
        let grant = Self {
            funding_id: funding_id.to_owned(),
            transaction_id: transaction_id.to_owned(),
            amount,
            network: network.to_owned(),
            raw: fields.clone(),
        };
        Ok(grant)
    }
}

fn method_and_identifier(did: &str) -> Option<(&str, &str)> {
    let (method, identifier) = did.strip_prefix("did:")?.split_once(':')?;
    (!method.is_empty() && !identifier.is_empty()).then_some((method, identifier))
}

fn faucet_selector(did: &str, signer_public_key: &[u8; 32]) -> Result<Value, RpcError> {
    layerx_types::ids::Did::new(did.as_bytes()).map_err(|_| RpcError::InvalidRequest)?;
    if method_and_identifier(did).is_none()
        || did.len() > 512
        || did
            .bytes()
            .any(|byte| !byte.is_ascii_alphanumeric() && !b"-._:".contains(&byte))
        || signer_public_key == &[0_u8; 32]
    {
        return Err(RpcError::InvalidRequest);
    }
    Ok(json!([did, encode_hex(signer_public_key)]))
}

fn object(value: &Value) -> Result<&serde_json::Map<String, Value>, RpcError> {
    value.as_object().ok_or(RpcError::InvalidResponse)
}

fn text_field<'a>(
    fields: &'a serde_json::Map<String, Value>,
    key: &str,
) -> Result<&'a str, RpcError> {
    fields
        .get(key)
        .and_then(Value::as_str)
        .ok_or(RpcError::InvalidResponse)
}

fn decimal_u128_field(
    fields: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<u128, RpcError> {
    let text = text_field(fields, key)?;
    let value: u128 = text.parse().map_err(|_| RpcError::InvalidResponse)?;
    if value.to_string() != text {
        return Err(RpcError::InvalidResponse);
    }
    Ok(value)
}

fn decimal_u64_field(fields: &serde_json::Map<String, Value>, key: &str) -> Result<u64, RpcError> {
    let value = decimal_u128_field(fields, key)?;
    u64::try_from(value).map_err(|_| RpcError::InvalidResponse)
}

fn unsigned_field<T>(fields: &serde_json::Map<String, Value>, key: &str) -> Result<T, RpcError>
where
    T: TryFrom<u64>,
{
    fields
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| T::try_from(value).ok())
        .ok_or(RpcError::InvalidResponse)
}

fn decode_hex<const N: usize>(text: &str) -> Result<[u8; N], RpcError> {
    if text.len() != N * 2
        || !text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(RpcError::InvalidResponse);
    }
    let mut output = [0; N];
    for (index, pair) in text.as_bytes().chunks_exact(2).enumerate() {
        let pair = std::str::from_utf8(pair).map_err(|_| RpcError::InvalidResponse)?;
        output[index] = u8::from_str_radix(pair, 16).map_err(|_| RpcError::InvalidResponse)?;
    }
    Ok(output)
}

fn fixed_hex_field<const N: usize>(
    fields: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<[u8; N], RpcError> {
    decode_hex(text_field(fields, key)?)
}

fn identity_sequence_params(did: &str) -> Result<Value, RpcError> {
    layerx_types::ids::Did::new(did.as_bytes()).map_err(|_| RpcError::InvalidRequest)?;
    if did
        .bytes()
        .any(|byte| !byte.is_ascii_alphanumeric() && !b"-._:".contains(&byte))
    {
        return Err(RpcError::InvalidRequest);
    }
    Ok(json!([did, "identity"]))
}

fn variable_hex_field(
    fields: &serde_json::Map<String, Value>,
    key: &str,
    maximum: usize,
) -> Result<Vec<u8>, RpcError> {
    let text = text_field(fields, key)?;
    if text.len() > maximum * 2
        || !text.len().is_multiple_of(2)
        || !text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(RpcError::InvalidResponse);
    }
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            std::str::from_utf8(pair)
                .ok()
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                .ok_or(RpcError::InvalidResponse)
        })
        .collect()
}

impl TryFrom<&Value> for AssetMetadata {
    type Error = RpcError;

    fn try_from(value: &Value) -> Result<Self, Self::Error> {
        let fields = object(value)?;
        let metadata = Self {
            asset_id: fixed_hex_field(fields, "asset_id")?,
            symbol: text_field(fields, "symbol")?.to_owned(),
            name: text_field(fields, "name")?.to_owned(),
            decimals: unsigned_field(fields, "decimals")?,
            custody_kind: unsigned_field(fields, "custody_kind")?,
            custody_reference: variable_hex_field(fields, "custody_reference", 128)?,
            paused: fields
                .get("paused")
                .and_then(Value::as_bool)
                .ok_or(RpcError::InvalidResponse)?,
            supply_cap: decimal_u128_field(fields, "supply_cap")?,
            issuer_did: fixed_hex_field(fields, "issuer_did")?,
            issuer_kind: unsigned_field(fields, "issuer_kind")?,
            total_units: decimal_u128_field(fields, "total_units")?,
            salt: fixed_hex_field(fields, "salt")?,
        };
        if metadata.asset_id == [0; 32]
            || !(1..=16).contains(&metadata.symbol.len())
            || !metadata.symbol.is_ascii()
            || metadata.name.is_empty()
            || metadata.name.len() > 32
            || metadata.decimals > 38
            || metadata.issuer_kind > 2
            || (metadata.issuer_kind != 0 && metadata.issuer_did == [0; 32])
            || (metadata.issuer_kind == 1 && !metadata.custody_reference.is_empty())
            || (metadata.issuer_kind == 0 && metadata.custody_reference.is_empty())
            || (metadata.supply_cap != 0 && metadata.total_units > metadata.supply_cap)
        {
            return Err(RpcError::InvalidResponse);
        }
        Ok(metadata)
    }
}

fn committed_snapshot(
    fields: &serde_json::Map<String, Value>,
) -> Result<(u64, [u8; 32]), RpcError> {
    if text_field(fields, "verification")? != "authenticated_committed_snapshot" {
        return Err(RpcError::InvalidResponse);
    }
    let state_root = fixed_hex_field(fields, "state_root")?;
    if state_root == [0; 32] {
        return Err(RpcError::InvalidResponse);
    }
    Ok((
        decimal_u64_field(fields, "observed_head_sequence")?,
        state_root,
    ))
}

impl TryFrom<Value> for AssetSnapshot {
    type Error = RpcError;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        let fields = object(&value)?;
        let asset = fields
            .get("asset")
            .ok_or(RpcError::InvalidResponse)?
            .try_into()?;
        let (observed_head_sequence, state_root) = committed_snapshot(fields)?;
        Ok(Self {
            asset,
            observed_head_sequence,
            state_root,
            raw: fields.clone(),
        })
    }
}

impl TryFrom<Value> for AssetListSnapshot {
    type Error = RpcError;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        let fields = object(&value)?;
        let values = fields
            .get("assets")
            .and_then(Value::as_array)
            .filter(|values| values.len() <= 64)
            .ok_or(RpcError::InvalidResponse)?;
        let assets = values
            .iter()
            .map(AssetMetadata::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        if assets
            .windows(2)
            .any(|pair| pair[0].asset_id >= pair[1].asset_id)
        {
            return Err(RpcError::InvalidResponse);
        }
        let (observed_head_sequence, state_root) = committed_snapshot(fields)?;
        Ok(Self {
            assets,
            observed_head_sequence,
            state_root,
            raw: fields.clone(),
        })
    }
}

impl TryFrom<Value> for IdentitySequenceSnapshot {
    type Error = RpcError;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        let fields = object(&value)?;
        if text_field(fields, "verification")? != "authenticated_node_snapshot" {
            return Err(RpcError::InvalidResponse);
        }
        let state_root = fixed_hex_field(fields, "state_root")?;
        if state_root == [0; 32] {
            return Err(RpcError::InvalidResponse);
        }
        Ok(Self {
            did: text_field(fields, "did")?.to_owned(),
            next_sequence: decimal_u64_field(fields, "next_sequence")?,
            observed_head_sequence: decimal_u64_field(fields, "observed_head_sequence")?,
            state_root,
        })
    }
}

pub struct RpcClient {
    agent: ureq::Agent,
    endpoint: url::Url,
    credential: Option<LayerXKeyCredential>,
    subscription_tls: Option<std::sync::Arc<rustls::ClientConfig>>,
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
    pub fn accounts(&self, did: &str) -> Result<BalancesSnapshot, RpcError> {
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
    let account = layerx_wire::account::account_name_for_asset(did, asset, native_asset)
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
        let endpoint =
            crate::programs::http::validate_endpoint(endpoint).map_err(RpcError::Configuration)?;
        let roots = crate::tls::system_roots(endpoint.as_str()).map_err(RpcError::Configuration)?;
        Self::connect_with_roots(endpoint.as_str(), credential, roots, None)
    }

    /// Connects with one explicitly trusted DER root certificate.
    /// # Errors
    /// Rejects an empty or oversized trust root and invalid endpoints.
    pub fn connect_with_ca_der(
        endpoint: &str,
        credential: Option<LayerXKeyCredential>,
        ca_der: &[u8],
    ) -> Result<Self, RpcError> {
        if ca_der.is_empty() || ca_der.len() > 1_048_576 {
            return Err(RpcError::InvalidRequest);
        }
        let certificate = ureq::tls::Certificate::from_der(ca_der).to_owned();
        let mut roots = rustls::RootCertStore::empty();
        roots
            .add(rustls::pki_types::CertificateDer::from(ca_der.to_vec()))
            .map_err(|_| RpcError::InvalidRequest)?;
        let subscription_tls = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|_| RpcError::InvalidRequest)?
        .with_root_certificates(roots)
        .with_no_client_auth();
        Self::connect_with_roots(
            endpoint,
            credential,
            ureq::tls::RootCerts::new_with_certs(&[certificate]),
            Some(std::sync::Arc::new(subscription_tls)),
        )
    }

    fn connect_with_roots(
        endpoint: &str,
        credential: Option<LayerXKeyCredential>,
        roots: ureq::tls::RootCerts,
        subscription_tls: Option<std::sync::Arc<rustls::ClientConfig>>,
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
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .provider(ureq::tls::TlsProvider::Rustls)
                    .root_certs(roots)
                    .build(),
            )
            .build()
            .into();
        Ok(Self {
            agent,
            endpoint,
            credential,
            subscription_tls,
            next_id: AtomicU64::new(1),
        })
    }

    read_methods! {
        get_account => "lx_getAccount", get_balance => "lx_getBalance",
        get_sequence => "lx_getSequence",
        get_receipt => "lx_getReceipt", get_activity_status => "lx_getActivityStatus",
        get_batch_header => "lx_getBatchHeader", get_checkpoint => "lx_getCheckpoint",
    }

    /// Registers one self-service principal in the public beta tenant.
    ///
    /// `signature` must be this key's Ed25519 signature over
    /// [`crate::register::binding`]; the returned principal is accepted only
    /// when the gateway confirms the subject this key derives locally.
    ///
    /// # Errors
    /// Preserves registration refusals and rejects a principal that names
    /// another key or tenant.
    pub fn register(
        &self,
        signer_public_key: [u8; 32],
        signature: &[u8; 64],
    ) -> Result<crate::register::Registration, RpcError> {
        let params = json!([encode_hex(&signer_public_key), encode_hex(signature)]);
        crate::register::decode(signer_public_key, self.call("lx_register", &params)?)
    }

    /// Claims one testnet faucet grant for the authenticated identity principal.
    ///
    /// `signer_public_key` must be a key the calling session authorises; the
    /// gateway derives the faucet idempotency key from the principal, the DID
    /// and the key, so a repeated request returns the same grant.
    ///
    /// # Errors
    /// Rejects malformed DIDs and the all-zero key before transport, preserves
    /// faucet refusals, and refuses any grant that is not confirmed as funded.
    pub fn request_funds(
        &self,
        did: &str,
        signer_public_key: &[u8; 32],
    ) -> Result<FaucetGrant, RpcError> {
        let params = faucet_selector(did, signer_public_key)?;
        self.call("lx_requestFunds", &params)?.try_into()
    }

    /// Reads the authenticated identity sequence used by the activity envelope.
    /// # Errors
    /// Rejects malformed DIDs and any mismatched or malformed snapshot.
    pub fn get_identity_sequence(&self, did: &str) -> Result<IdentitySequenceSnapshot, RpcError> {
        let params = identity_sequence_params(did)?;
        let snapshot: IdentitySequenceSnapshot =
            self.call("lx_getSequence", &params)?.try_into()?;
        if snapshot.did != did {
            return Err(RpcError::InvalidResponse);
        }
        Ok(snapshot)
    }

    /// # Errors
    /// Rejects malformed DID selectors and preserves upstream enumeration unavailability.
    pub fn get_balances(&self, did: &str) -> Result<BalancesSnapshot, RpcError> {
        layerx_types::ids::Did::new(did.as_bytes()).map_err(|_| RpcError::InvalidRequest)?;
        self.call("lx_getBalances", &json!([did]))?.try_into()
    }

    /// Opens an authenticated public subscription. Notifications are unverified hints.
    /// # Errors
    /// Refuses invalid topic selectors, transport failures and mismatched acknowledgements.
    pub fn subscribe(
        &self,
        topic: SubscriptionTopic,
        account: Option<[u8; 32]>,
    ) -> Result<RpcSubscription, RpcError> {
        crate::rpc_subscription::connect(
            &self.endpoint,
            self.credential.as_ref(),
            self.subscription_tls.clone(),
            topic,
            account,
            None,
        )
    }

    /// Reopens a subscription from the cursor of the last notification the caller observed.
    /// The server replays the topic from that position before delivering live notifications and
    /// refuses cursors outside its resume window, which the caller reconciles through reads.
    /// # Errors
    /// Refuses invalid topic selectors, refused resumes, transport failures and mismatched
    /// acknowledgements.
    pub fn subscribe_from(
        &self,
        topic: SubscriptionTopic,
        account: Option<[u8; 32]>,
        cursor: u64,
    ) -> Result<RpcSubscription, RpcError> {
        crate::rpc_subscription::connect(
            &self.endpoint,
            self.credential.as_ref(),
            self.subscription_tls.clone(),
            topic,
            account,
            Some(cursor),
        )
    }

    /// # Errors
    /// Preserves node-info RPC refusals.
    pub fn get_node_info(&self) -> Result<Value, RpcError> {
        self.call("lx_getNodeInfo", &json!([]))
    }

    /// # Errors
    /// Preserves native asset-listing refusals; returned read data is unverified.
    pub fn list_assets(&self) -> Result<AssetListSnapshot, RpcError> {
        self.call("lx_listAssets", &json!([]))?.try_into()
    }

    /// # Errors
    /// Preserves native asset metadata refusals; returned read data is unverified.
    pub fn get_asset(&self, asset: [u8; 32]) -> Result<AssetSnapshot, RpcError> {
        let snapshot: AssetSnapshot = self
            .call("lx_getAsset", &json!([encode_hex(&asset)]))?
            .try_into()?;
        if snapshot.asset.asset_id != asset {
            return Err(RpcError::InvalidResponse);
        }
        Ok(snapshot)
    }

    /// # Errors
    /// Refuses empty or oversized activities and preserves native fee-estimation errors.
    pub fn estimate_fee(&self, canonical: &[u8]) -> Result<FeeEstimate, RpcError> {
        if canonical.is_empty() || canonical.len() > 524_288 {
            return Err(RpcError::InvalidRequest);
        }
        self.call("lx_estimateFee", &json!([encode_hex(canonical)]))?
            .try_into()
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
        assert!(RpcClient::connect_with_ca_der("https://127.0.0.1:1", None, &[]).is_err());
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
    fn a_faucet_request_names_a_did_and_a_real_signer_key() {
        let key = [0xab_u8; 32];
        assert_eq!(
            faucet_selector("did:layerx:alice", &key).ok(),
            Some(json!(["did:layerx:alice", encode_hex(&key)]))
        );
        for (did, key) in [
            ("did:layerx:alice", [0_u8; 32]),
            ("layerx:alice", key),
            ("did::alice", key),
            ("did:layerx:a b", key),
            ("did:layerx:../bob", key),
            ("", key),
        ] {
            assert!(
                matches!(
                    faucet_selector(did, &key),
                    Err(RpcError::InvalidRequest | RpcError::InvalidResponse)
                ),
                "{did} accepted"
            );
        }
    }

    #[test]
    fn only_a_confirmed_funded_grant_becomes_a_faucet_value() {
        let document = json!({
            "funded": true,
            "funding_id": "ef".repeat(32),
            "transaction_id": "cd".repeat(32),
            "amount": "1000000",
            "network": "layerx-testnet",
        });
        let grant =
            FaucetGrant::try_from(document.clone()).unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(grant.funding_id, "ef".repeat(32));
        assert_eq!(grant.transaction_id, "cd".repeat(32));
        assert_eq!(grant.amount, 1_000_000);
        assert_eq!(grant.network, "layerx-testnet");
        assert_eq!(
            grant.unverified_fields().get("funded"),
            Some(&Value::Bool(true))
        );
        assert_eq!(grant.into_value(), document);

        for incomplete in [
            json!({"funded": false, "funding_id": "ef", "transaction_id": "cd", "amount": "1", "network": "n"}),
            json!({"funded": "true", "funding_id": "ef", "transaction_id": "cd", "amount": "1", "network": "n"}),
            json!({"funding_id": "ef", "transaction_id": "cd", "amount": "1", "network": "n"}),
            json!({"funded": true, "transaction_id": "cd", "amount": "1", "network": "n"}),
            json!({"funded": true, "funding_id": "zz", "transaction_id": "cd", "amount": "1", "network": "n"}),
            json!({"funded": true, "funding_id": "ef", "amount": "1", "network": "n"}),
            json!({"funded": true, "funding_id": "ef", "transaction_id": "cd", "network": "n"}),
            json!({"funded": true, "funding_id": "ef", "transaction_id": "cd", "amount": "0", "network": "n"}),
            json!({"funded": true, "funding_id": "ef", "transaction_id": "cd", "amount": "01", "network": "n"}),
            json!({"funded": true, "funding_id": "ef", "transaction_id": "cd", "amount": 1, "network": "n"}),
            json!({"funded": true, "funding_id": "ef", "transaction_id": "cd", "amount": "1", "network": ""}),
            json!({"funded": true, "funding_id": "ef", "transaction_id": "cd", "amount": "1"}),
            json!([]),
            Value::Null,
        ] {
            assert!(
                matches!(
                    FaucetGrant::try_from(incomplete.clone()),
                    Err(RpcError::InvalidResponse)
                ),
                "{incomplete}"
            );
        }
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

#[cfg(test)]
mod read_result_tests {
    use super::*;

    fn asset_value(id: &str) -> Value {
        json!({
            "asset_id": id,
            "symbol": "USD",
            "name": "Test Dollar",
            "decimals": 6,
            "custody_kind": 0,
            "custody_reference": "",
            "paused": false,
            "supply_cap": "1000000",
            "issuer_did": "22".repeat(32),
            "issuer_kind": 1,
            "total_units": "100",
            "salt": "33".repeat(32)
        })
    }

    #[test]
    fn openrpc_typed_assets_refuse_malformed_or_unordered_metadata() {
        for value in [
            Value::Null,
            json!([]),
            json!(true),
            json!(12),
            json!("accepted"),
        ] {
            assert!(AssetSnapshot::try_from(value.clone()).is_err());
            assert!(AssetListSnapshot::try_from(value.clone()).is_err());
            assert!(BalancesSnapshot::try_from(value.clone()).is_err());
            assert!(FeeEstimate::try_from(value).is_err());
        }
        let asset = asset_value(&"11".repeat(32));
        let fields = json!({
            "asset": asset.clone(),
            "observed_head_sequence": "9",
            "state_root": "44".repeat(32),
            "verification": "authenticated_committed_snapshot",
            "unrecognised_native_field": "preserved"
        });
        let snapshot =
            AssetSnapshot::try_from(fields.clone()).unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(snapshot.asset.total_units, 100);
        assert_eq!(snapshot.observed_head_sequence, 9);
        assert_eq!(snapshot.into_value(), fields);

        let unordered = json!({
            "assets": [asset_value(&"22".repeat(32)), asset],
            "observed_head_sequence": "9",
            "state_root": "44".repeat(32),
            "verification": "authenticated_committed_snapshot"
        });
        assert!(AssetListSnapshot::try_from(unordered).is_err());
        let mut malformed = asset_value(&"11".repeat(32));
        malformed["supply_cap"] = json!("0100");
        assert!(AssetMetadata::try_from(&malformed).is_err());
    }

    #[test]
    fn identity_sequence_uses_the_two_parameter_domain_and_strict_snapshot() {
        assert_eq!(
            identity_sequence_params("did:layerx:alice").ok(),
            Some(json!(["did:layerx:alice", "identity"]))
        );
        assert!(identity_sequence_params("did:layerx:ali ce").is_err());
        let snapshot = IdentitySequenceSnapshot::try_from(json!({
            "did": "did:layerx:alice",
            "next_sequence": "7",
            "observed_head_sequence": "11",
            "state_root": "44".repeat(32),
            "verification": "authenticated_node_snapshot"
        }))
        .unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(snapshot.next_sequence, 7);
        assert_eq!(snapshot.observed_head_sequence, 11);
        assert!(IdentitySequenceSnapshot::try_from(json!({
            "did": "did:layerx:alice",
            "next_sequence": "07",
            "observed_head_sequence": "11",
            "state_root": "44".repeat(32),
            "verification": "authenticated_node_snapshot"
        }))
        .is_err());
    }
}

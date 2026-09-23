//! Relayer configuration: one JSON file naming the journal, the remote signer
//! and its key handles, Paxeer, and every Ethereum chain with its vault,
//! finality depth and RPC quorum. The file holds handles and public keys
//! only; no private key is ever configured into this process.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use layerx_mirror::rpc::{RpcCluster, RpcQuorumConfig};
use layerx_mirror::signer::{RemoteChainSigner, RemoteSignerConfig, SignerEndpoint};
use layerx_paxeer_verifier::{EndpointConfig, EndpointTransport};
use serde::Deserialize;

use crate::cosign::CosignDirectory;
use crate::hex;
use crate::journal::Journal;
use crate::relayer::{
    ChainLink, ChainSettings, GasPolicy, PaxeerLink, PaxeerSettings, RelayerError, RelayerParts,
};
use crate::rpc::PaxeerRpc;
use crate::signer::{Attestor, Submitter, ALGORITHM};

const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(tag = "transport", rename_all = "snake_case", deny_unknown_fields)]
pub enum SignerTransportConfig {
    Uds {
        socket: PathBuf,
    },
    MutualTls {
        endpoint: SocketAddr,
        server_name: String,
        trust_anchor: PathBuf,
        client_certificate: PathBuf,
        client_private_key: PathBuf,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SignerConfig {
    pub endpoint: SignerTransportConfig,
    pub timeout_ms: u64,
}

/// An opaque signer policy handle and the public key it must answer for.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct KeyHandleConfig {
    pub handle: String,
    /// SEC1 secp256k1 public key, `0x`-prefixed hex.
    #[serde(with = "hex::bytes_serde")]
    pub public_key: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GasConfig {
    pub gas_limit: u64,
    pub max_fee_per_gas: u128,
    pub max_priority_fee_per_gas: u128,
}

impl From<GasConfig> for GasPolicy {
    fn from(value: GasConfig) -> Self {
        Self {
            gas_limit: value.gas_limit,
            max_fee_per_gas: value.max_fee_per_gas,
            max_priority_fee_per_gas: value.max_priority_fee_per_gas,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PaxeerEndpointConfig {
    pub url: String,
    /// DER trust anchor of a TLS endpoint. Absent only with `local_emulator`.
    #[serde(default)]
    pub trust_anchor_der: Option<PathBuf>,
    #[serde(default)]
    pub local_emulator: bool,
    pub request_timeout_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PaxeerConfig {
    pub chain_id: u64,
    pub endpoints: Vec<PaxeerEndpointConfig>,
    pub finality_depth: u64,
    pub start_block: u64,
    pub max_block_range: u64,
    pub submitter: KeyHandleConfig,
    pub gas: GasConfig,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ChainConfig {
    pub chain_id: u64,
    #[serde(with = "hex::fixed_serde")]
    pub vault: [u8; 20],
    pub finality_depth: u64,
    pub start_block: u64,
    pub max_block_range: u64,
    pub rpc: RpcQuorumConfig,
    pub submitter: KeyHandleConfig,
    pub gas: GasConfig,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RelayerConfig {
    pub journal_path: PathBuf,
    /// Shared directory for exchanging attestor signatures when a threshold
    /// above one needs several relayer instances.
    #[serde(default)]
    pub cosign_directory: Option<PathBuf>,
    pub poll_interval_ms: u64,
    pub max_submissions: u32,
    pub signer: SignerConfig,
    pub attestor: KeyHandleConfig,
    pub paxeer: PaxeerConfig,
    pub chains: Vec<ChainConfig>,
}

fn invalid(detail: impl Into<String>) -> RelayerError {
    RelayerError::Configuration(detail.into())
}

impl RelayerConfig {
    /// Reads and validates a configuration file.
    ///
    /// # Errors
    ///
    /// Refuses unreadable, oversized, unknown-field or inconsistent files.
    pub fn load(path: &Path) -> Result<Self, RelayerError> {
        let metadata = std::fs::metadata(path).map_err(|error| invalid(error.to_string()))?;
        if metadata.len() > MAX_CONFIG_BYTES {
            return Err(invalid("configuration file is too large"));
        }
        let text = std::fs::read_to_string(path).map_err(|error| invalid(error.to_string()))?;
        let config: Self =
            serde_json::from_str(&text).map_err(|error| invalid(error.to_string()))?;
        config.validate()?;
        Ok(config)
    }

    /// Checks everything that can be checked without the network.
    ///
    /// # Errors
    ///
    /// Names the first inconsistency.
    pub fn validate(&self) -> Result<(), RelayerError> {
        if self.poll_interval_ms == 0 || self.max_submissions == 0 {
            return Err(invalid(
                "poll interval and submission budget must be positive",
            ));
        }
        if self.signer.timeout_ms == 0 {
            return Err(invalid("signer timeout must be positive"));
        }
        if self.paxeer.endpoints.is_empty() {
            return Err(invalid("paxeer needs at least one endpoint"));
        }
        for endpoint in &self.paxeer.endpoints {
            if endpoint.request_timeout_ms == 0
                || endpoint.local_emulator == endpoint.trust_anchor_der.is_some()
            {
                return Err(invalid(format!(
                    "paxeer endpoint {} needs a timeout and exactly one of a trust anchor or local_emulator",
                    endpoint.url
                )));
            }
        }
        if self.chains.is_empty() {
            return Err(invalid("at least one chain is required"));
        }
        let handles = std::iter::once(&self.attestor)
            .chain(std::iter::once(&self.paxeer.submitter))
            .chain(self.chains.iter().map(|chain| &chain.submitter));
        for key in handles {
            if key.handle.is_empty() || key.public_key.is_empty() {
                return Err(invalid("every key needs a handle and a public key"));
            }
        }
        if self
            .chains
            .iter()
            .any(|chain| chain.submitter.handle == self.attestor.handle)
            || self.paxeer.submitter.handle == self.attestor.handle
        {
            return Err(invalid("the attestor key must not pay transaction fees"));
        }
        Ok(())
    }

    fn signer_endpoint(&self) -> SignerEndpoint {
        match &self.signer.endpoint {
            SignerTransportConfig::Uds { socket } => SignerEndpoint::Uds {
                socket: socket.clone(),
            },
            SignerTransportConfig::MutualTls {
                endpoint,
                server_name,
                trust_anchor,
                client_certificate,
                client_private_key,
            } => SignerEndpoint::MutualTls {
                endpoint: *endpoint,
                server_name: server_name.clone(),
                trust_anchor: trust_anchor.clone(),
                client_certificate: client_certificate.clone(),
                client_private_key: client_private_key.clone(),
            },
        }
    }

    fn remote_signer(&self, key: &KeyHandleConfig) -> Result<RemoteChainSigner, RelayerError> {
        RemoteChainSigner::new(RemoteSignerConfig {
            endpoint: self.signer_endpoint(),
            algorithm: ALGORITHM,
            key_handle: key.handle.clone(),
            public_key: key.public_key.clone(),
            timeout: Duration::from_millis(self.signer.timeout_ms),
        })
        .map_err(|error| invalid(format!("signer handle {}: {error:?}", key.handle)))
    }

    fn paxeer_endpoints(&self) -> Result<Vec<EndpointConfig>, RelayerError> {
        self.paxeer
            .endpoints
            .iter()
            .map(|endpoint| {
                let transport = match &endpoint.trust_anchor_der {
                    Some(path) => EndpointTransport::PinnedTls {
                        trust_anchor_der: std::fs::read(path)
                            .map_err(|error| invalid(format!("{}: {error}", path.display())))?,
                    },
                    None => EndpointTransport::LocalEmulator,
                };
                Ok(EndpointConfig {
                    url: endpoint.url.clone(),
                    request_timeout: Duration::from_millis(endpoint.request_timeout_ms),
                    transport,
                    expected_chain_id: self.paxeer.chain_id,
                })
            })
            .collect()
    }

    /// Opens the journal and connects every transport and key handle.
    ///
    /// # Errors
    ///
    /// Returns the first transport, signer or journal failure.
    pub fn build(&self) -> Result<RelayerParts, RelayerError> {
        self.validate()?;
        let attestor = Attestor::new(self.remote_signer(&self.attestor)?)
            .map_err(|error| invalid(format!("attestor: {error:?}")))?;
        let paxeer = PaxeerLink {
            settings: PaxeerSettings {
                chain_id: self.paxeer.chain_id,
                finality_depth: self.paxeer.finality_depth,
                start_block: self.paxeer.start_block,
                max_block_range: self.paxeer.max_block_range,
                gas: self.paxeer.gas.into(),
            },
            rpc: Box::new(PaxeerRpc::new(self.paxeer_endpoints()?)?),
            submitter: Submitter::paxeer(self.remote_signer(&self.paxeer.submitter)?)
                .map_err(|error| invalid(format!("paxeer submitter: {error:?}")))?,
        };
        let mut chains = Vec::with_capacity(self.chains.len());
        for chain in &self.chains {
            chains.push(ChainLink {
                settings: ChainSettings {
                    chain_id: chain.chain_id,
                    vault: chain.vault,
                    finality_depth: chain.finality_depth,
                    start_block: chain.start_block,
                    max_block_range: chain.max_block_range,
                    gas: chain.gas.into(),
                },
                rpc: Box::new(RpcCluster::new(&chain.rpc).map_err(|error| {
                    invalid(format!("chain {} rpc: {error:?}", chain.chain_id))
                })?),
                submitter: Submitter::ethereum(self.remote_signer(&chain.submitter)?).map_err(
                    |error| invalid(format!("chain {} submitter: {error:?}", chain.chain_id)),
                )?,
            });
        }
        Ok(RelayerParts {
            attestor,
            paxeer,
            chains,
            journal: Journal::open(&self.journal_path)?,
            cosign: self.cosign_directory.clone().map(CosignDirectory::new),
            max_submissions: self.max_submissions,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = r#"{
        "journal_path": "/var/lib/layerx-bridge-relayer/journal.jsonl",
        "cosign_directory": "/var/lib/layerx-bridge-relayer/cosign",
        "poll_interval_ms": 4000,
        "max_submissions": 3,
        "signer": {"endpoint": {"transport": "uds", "socket": "/run/layerx-signer.sock"}, "timeout_ms": 2000},
        "attestor": {"handle": "bridge-attestor-1", "public_key": "0x02aa"},
        "paxeer": {
            "chain_id": 229,
            "endpoints": [{"url": "http://127.0.0.1:8545", "local_emulator": true, "request_timeout_ms": 3000}],
            "finality_depth": 2, "start_block": 0, "max_block_range": 500,
            "submitter": {"handle": "bridge-paxeer-fees", "public_key": "0x02bb"},
            "gas": {"gas_limit": 1000000, "max_fee_per_gas": 100000000000, "max_priority_fee_per_gas": 2000000000}
        },
        "chains": [{
            "chain_id": 1,
            "vault": "0x1111111111111111111111111111111111111111",
            "finality_depth": 64, "start_block": 19000000, "max_block_range": 1000,
            "rpc": {"endpoints": [], "quorum": 2, "connect_timeout_ms": 1000, "request_timeout_ms": 3000, "maximum_response_bytes": 1048576},
            "submitter": {"handle": "bridge-ethereum-fees", "public_key": "0x02cc"},
            "gas": {"gas_limit": 400000, "max_fee_per_gas": 200000000000, "max_priority_fee_per_gas": 3000000000}
        }]
    }"#;

    fn example() -> RelayerConfig {
        serde_json::from_str(EXAMPLE).unwrap_or_else(|error| panic!("example: {error}"))
    }

    #[test]
    fn the_example_configuration_parses_and_validates() {
        let config = example();
        assert_eq!(config.chains[0].vault, [0x11; 20]);
        assert_eq!(config.attestor.public_key, vec![0x02, 0xaa]);
        assert_eq!(config.validate(), Ok(()));
    }

    #[test]
    fn unknown_fields_and_inline_secrets_are_refused() {
        let with_secret = EXAMPLE.replacen(
            r#""handle": "bridge-attestor-1","#,
            r#""handle": "bridge-attestor-1", "private_key": "0x01","#,
            1,
        );
        assert!(serde_json::from_str::<RelayerConfig>(&with_secret).is_err());
    }

    #[test]
    fn an_attestor_that_pays_fees_is_refused() {
        let mut config = example();
        config.paxeer.submitter.handle = config.attestor.handle.clone();
        assert!(config.validate().is_err());
    }

    #[test]
    fn a_paxeer_endpoint_needs_exactly_one_trust_mode() {
        let mut config = example();
        config.paxeer.endpoints[0].local_emulator = false;
        assert!(config.validate().is_err());
        config.paxeer.endpoints[0].trust_anchor_der = Some(PathBuf::from("/etc/anchor.der"));
        assert_eq!(config.validate(), Ok(()));
    }
}

use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use layerx_client::lni::transport::{Limits, MutualTlsConfig};
use layerx_human_service::custody::RemoteKmsProvider;
use rustls::{
    pki_types::{CertificateDer, PrivateKeyDer},
    RootCertStore,
};

use layerx_paxeer_client::{DepositProofConfig, EndpointConfig, EndpointTransport, TrackerConfig};
use layerx_types::intent::EvmAddress;

use crate::journal::read_private;
use crate::listener::ListenerConfig;
use crate::Error;

pub(crate) const MAX_FRAME: usize = 1_048_576;

pub(crate) struct Config {
    pub listener: ListenerConfig,
    pub state_root: PathBuf,
    pub evidence_root: PathBuf,
    pub custody_profile: Option<[u8; 207]>,
    pub tracker: TrackerConfig,
    pub proof: DepositProofConfig,
    pub vault: EvmAddress,
    pub checkpoint_registry: EvmAddress,
    pub claims_contract: EvmAddress,
    pub exit_contract: EvmAddress,
    pub executor: Option<Arc<RemoteKmsProvider>>,
    pub checkpoint_interval_seconds: u64,
    pub paxeer_block_seconds: u64,
    pub reminder_interval_seconds: u64,
}

impl Config {
    pub fn from_environment() -> Result<Self, Error> {
        let mode = required("MODE")?;
        if mode != "evidence-only" && mode != "movement" {
            return Err(Error::Configuration);
        }
        let deadline = Duration::from_secs(bounded("DEADLINE_SECONDS", 1, 60)?);
        let frame = usize::try_from(bounded("MAX_FRAME_BYTES", 2, MAX_FRAME as u64)?)
            .map_err(|_| Error::Configuration)?;
        let chain = bounded("PAXEER_CHAIN_ID", 1, u64::MAX)?;
        let ca = read_private(&path("PAXEER_CA_DER")?, 65_536)?;
        let urls: Vec<String> = serde_json::from_str(&required("PAXEER_RPC_URLS")?)
            .map_err(|_| Error::Configuration)?;
        if urls.len() < 2 || urls.len() > 8 {
            return Err(Error::Configuration);
        }
        independent_hosts(&urls)?;
        let endpoints = urls
            .into_iter()
            .map(|url| EndpointConfig {
                url,
                request_timeout: deadline,
                transport: EndpointTransport::PinnedTls {
                    trust_anchor_der: ca.clone(),
                },
                expected_chain_id: chain,
            })
            .collect::<Vec<_>>();
        let agreement = usize::try_from(bounded(
            "PAXEER_MINIMUM_AGREEMENT",
            2,
            endpoints.len() as u64,
        )?)
        .map_err(|_| Error::Configuration)?;
        let confirmations = bounded("PAXEER_CONFIRMATIONS", 1, u64::MAX)?;
        let protocol =
            u16::try_from(bounded("PROTOCOL_VERSION", 2, 3)?).map_err(|_| Error::Configuration)?;
        Ok(Self {
            listener: ListenerConfig {
                socket: path("SOCKET")?,
                allowed_uid: u32::try_from(bounded("ALLOWED_UID", 0, u64::from(u32::MAX))?)
                    .map_err(|_| Error::Configuration)?,
                allowed_gid: u32::try_from(bounded("ALLOWED_GID", 0, u64::from(u32::MAX))?)
                    .map_err(|_| Error::Configuration)?,
                maximum_frame_bytes: frame,
                deadline,
                protocol,
            },
            state_root: path("STATE_ROOT")?,
            evidence_root: path("EVIDENCE_ROOT")?,
            custody_profile: if protocol == 3 {
                Some(
                    read_private(&path("CUSTODY_PROFILE")?, 207)?
                        .try_into()
                        .map_err(|_| Error::Configuration)?,
                )
            } else {
                None
            },
            vault: EvmAddress::new(hex(&required("PAXEER_VAULT")?)?),
            checkpoint_registry: EvmAddress::new(hex(&required("PAXEER_CHECKPOINT_REGISTRY")?)?),
            claims_contract: EvmAddress::new(hex(&required("PAXEER_CLAIMS_CONTRACT")?)?),
            exit_contract: EvmAddress::new(hex(&required("PAXEER_EXIT_CONTRACT")?)?),
            executor: if mode == "movement" {
                Some(Arc::new(executor(deadline)?))
            } else {
                None
            },
            checkpoint_interval_seconds: bounded("CHECKPOINT_INTERVAL_SECONDS", 1, u64::MAX)?,
            paxeer_block_seconds: bounded("PAXEER_BLOCK_SECONDS", 1, u64::MAX)?,
            reminder_interval_seconds: bounded("REMINDER_INTERVAL_SECONDS", 1, u64::MAX)?,
            tracker: TrackerConfig {
                endpoints: endpoints.clone(),
                minimum_endpoint_agreement: agreement,
                required_confirmations: confirmations,
                poll_cadence: Duration::from_secs(bounded("POLL_SECONDS", 1, 60)?),
                delayed_after_polls: bounded("DELAYED_AFTER_POLLS", 1, u64::MAX)?,
            },
            proof: DepositProofConfig {
                endpoints,
                minimum_endpoint_agreement: agreement,
                required_confirmations: confirmations,
                paxeer_chain_id: chain,
                paxeer_checkpoint_authority: hex(&required("PAXEER_CHECKPOINT_AUTHORITY")?)?,
                custody_reference: hex(&required("CUSTODY_REFERENCE")?)?,
                layerx_network_id: u32::try_from(bounded("NETWORK_ID", 1, u64::from(u32::MAX))?)
                    .map_err(|_| Error::Configuration)?,
                layerx_protocol_version: protocol,
            },
        })
    }
}

fn independent_hosts(urls: &[String]) -> Result<(), Error> {
    let mut hosts = std::collections::BTreeSet::new();
    for url in urls {
        let authority = url
            .strip_prefix("https://")
            .and_then(|rest| rest.split('/').next())
            .filter(|value| !value.is_empty() && !value.contains(['@', '?', '#']))
            .ok_or(Error::Configuration)?;
        let host = if let Some(ipv6) = authority.strip_prefix('[') {
            ipv6.split_once(']')
                .map(|(host, _)| host)
                .ok_or(Error::Configuration)?
        } else {
            authority.split(':').next().ok_or(Error::Configuration)?
        };
        let canonical = host.parse::<std::net::IpAddr>().map_or_else(
            |_| host.trim_end_matches('.').to_ascii_lowercase(),
            |address| address.to_string(),
        );
        if canonical.is_empty() || !hosts.insert(canonical) {
            return Err(Error::Configuration);
        }
    }
    Ok(())
}

fn executor(deadline: Duration) -> Result<RemoteKmsProvider, Error> {
    let mut roots = RootCertStore::empty();
    roots
        .add(CertificateDer::from(read_private(
            &path("KMS_CA_DER")?,
            65_536,
        )?))
        .map_err(|_| Error::Configuration)?;
    let certificate = CertificateDer::from(read_private(&path("KMS_CLIENT_CERT_DER")?, 65_536)?);
    let key = PrivateKeyDer::try_from(read_private(&path("KMS_CLIENT_KEY_DER")?, 65_536)?)
        .map_err(|_| Error::Configuration)?;
    let tls =
        MutualTlsConfig::new(roots, vec![certificate], key).map_err(|_| Error::Configuration)?;
    RemoteKmsProvider::new(
        required("KMS_PROVIDER_REFERENCE")?,
        required("KMS_ENDPOINT")?
            .parse()
            .map_err(|_| Error::Configuration)?,
        required("KMS_SERVER_NAME")?,
        tls,
        Limits {
            maximum_frame_bytes: 2_097_152,
            maximum_connections: 1,
            maximum_streams: 1,
            maximum_queued_bytes: 2_097_152,
            deadline,
        },
    )
    .map_err(|_| Error::Configuration)
}

fn required(suffix: &str) -> Result<String, Error> {
    env::var(format!("LAYERX_HUMAN_MOVEMENT_PROVIDER_{suffix}"))
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or(Error::Configuration)
}
fn path(suffix: &str) -> Result<PathBuf, Error> {
    let value = PathBuf::from(required(suffix)?);
    if !value.is_absolute() {
        return Err(Error::Configuration);
    }
    Ok(value)
}
fn bounded(suffix: &str, min: u64, max: u64) -> Result<u64, Error> {
    let value = required(suffix)?
        .parse()
        .map_err(|_| Error::Configuration)?;
    if !(min..=max).contains(&value) {
        return Err(Error::Configuration);
    }
    Ok(value)
}
pub(crate) fn hex<const N: usize>(value: &str) -> Result<[u8; N], Error> {
    let digits = value.strip_prefix("0x").ok_or(Error::Configuration)?;
    if digits.len() != N * 2 || !digits.is_ascii() {
        return Err(Error::Configuration);
    }
    let mut out = [0; N];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte =
            u8::from_str_radix(&digits[i * 2..i * 2 + 2], 16).map_err(|_| Error::Configuration)?;
    }
    Ok(out)
}
pub(crate) fn hex_string(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(char::from(DIGITS[usize::from(byte >> 4)]));
        result.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    result
}

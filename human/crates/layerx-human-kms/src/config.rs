use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use rustix::fs::{open, Mode, OFlags};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{RootCertStore, ServerConfig};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::net::SocketAddr;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use zeroize::Zeroizing;

pub(crate) struct Config {
    pub listen: SocketAddr,
    pub state: PathBuf,
    pub provider: String,
    pub network: u32,
    pub protocol: u16,
    pub registry: ModuleRegistry,
    pub seal: Zeroizing<Vec<u8>>,
    pub tls: Arc<ServerConfig>,
    pub client_pin: [u8; 32],
    pub deadline: Duration,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistrySnapshot {
    network_id: u32,
    protocol_version: u16,
    modules: Vec<Module>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Module {
    module_id: u16,
    activity_types: Vec<u32>,
}
pub(crate) fn protected(
    path: &Path,
    maximum: usize,
    secret: bool,
) -> Result<Zeroizing<Vec<u8>>, String> {
    if !path.is_absolute() {
        return Err("provider paths must be absolute".into());
    }
    let descriptor = open(
        path,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| "provider file cannot be opened")?;
    let mut file = File::from(descriptor);
    let metadata = file
        .metadata()
        .map_err(|_| "provider file metadata unavailable")?;
    if !metadata.is_file()
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.mode() & (if secret { 0o077 } else { 0o022 }) != 0
    {
        return Err("provider file ownership or permissions refused".into());
    }
    let mut bytes = Zeroizing::new(Vec::new());
    file.by_ref()
        .take(u64::try_from(maximum).map_err(|_| "file bound invalid")? + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "provider file unreadable")?;
    if bytes.is_empty() || bytes.len() > maximum {
        return Err("provider file size refused".into());
    }
    Ok(bytes)
}
fn required(suffix: &str) -> Result<String, String> {
    std::env::var(format!("LAYERX_HUMAN_KMS_{suffix}"))
        .ok()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| format!("LAYERX_HUMAN_KMS_{suffix} is required"))
}
fn read(suffix: &str, max: usize, secret: bool) -> Result<Zeroizing<Vec<u8>>, String> {
    protected(Path::new(&required(suffix)?), max, secret)
}
impl Config {
    pub fn load() -> Result<Self, String> {
        let provider = required("PROVIDER_REFERENCE")?;
        if provider.len() > 256 || provider.contains('\0') {
            return Err("provider reference refused".into());
        }
        let snapshot: RegistrySnapshot =
            serde_json::from_slice(&read("REGISTRY_FILE", 65536, false)?)
                .map_err(|_| "registry snapshot invalid")?;
        if snapshot.network_id == 0
            || !layerx_wire::limits::protocol_version_supported(snapshot.protocol_version)
            || snapshot.modules.is_empty()
            || snapshot.modules.len() > 32
        {
            return Err("registry scope refused".into());
        }
        let mut modules = Vec::new();
        for module in snapshot.modules {
            if module.activity_types.is_empty() || module.activity_types.len() > 256 {
                return Err("registry module refused".into());
            }
            let id = ModuleId::from_u16(module.module_id).map_err(|_| "registry module invalid")?;
            let types = module
                .activity_types
                .into_iter()
                .map(ActivityType::from_u32)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| "registry activity invalid")?;
            modules.push(
                ModuleRegistration::new(id, &types).map_err(|_| "registry registration invalid")?,
            );
        }
        let registry = ModuleRegistry::new(&modules).map_err(|_| "registry invalid")?;
        let mut roots = RootCertStore::empty();
        roots
            .add(CertificateDer::from(
                read("CLIENT_CA_DER", 65536, false)?.to_vec(),
            ))
            .map_err(|_| "client CA invalid")?;
        let verifier = rustls::server::WebPkiClientVerifier::builder(Arc::new(roots))
            .build()
            .map_err(|_| "client verifier invalid")?;
        let tls = ServerConfig::builder()
            .with_client_cert_verifier(verifier)
            .with_single_cert(
                vec![CertificateDer::from(
                    read("TLS_CERT_DER", 65536, false)?.to_vec(),
                )],
                PrivateKeyDer::try_from(read("TLS_KEY_DER", 65536, true)?.to_vec())
                    .map_err(|_| "TLS key invalid")?,
            )
            .map_err(|_| "TLS identity invalid")?;
        let seal = read("SEAL_SECRET_FILE", 32, true)?;
        if seal.len() != 32 {
            return Err("seal secret must contain 32 random bytes".into());
        }
        let client_pin = Sha256::digest(&*read("CLIENT_CERT_DER", 65536, false)?).into();
        let seconds: u64 = required("DEADLINE_SECONDS")?
            .parse()
            .map_err(|_| "deadline invalid")?;
        if !(1..=60).contains(&seconds) {
            return Err("deadline refused".into());
        }
        Ok(Self {
            listen: required("LISTEN")?
                .parse()
                .map_err(|_| "listen address invalid")?,
            state: PathBuf::from(required("STATE_DIR")?),
            provider,
            network: snapshot.network_id,
            protocol: snapshot.protocol_version,
            registry,
            seal,
            tls: Arc::new(tls),
            client_pin,
            deadline: Duration::from_secs(seconds),
        })
    }
}

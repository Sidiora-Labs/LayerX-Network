//! Hosted program-registry service. It answers the developer CLI's registry
//! routes from durable local evidence only: registry reads are re-verified
//! against the canonical deployment journal, and verified-source status is
//! produced by rebuilding published source in a pinned toolchain environment.

mod builder;
mod http;
mod journal;
mod mirror;
mod node_state;
mod program_state;
mod routes;
mod verified;

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub use builder::HermeticBuilder;
pub use http::{parse_request, write_response};
pub use journal::FileDeploymentJournal;
pub use mirror::{MirrorRefusal, MirroredSource, SourceMirror};
pub use node_state::{NodeProgramStateSource, ProgramStateCursor};
pub use program_state::FileProgramStateJournal;
pub use routes::{refusal, Registrar, Request, Response};
pub use verified::{VerifiedSource, VerifiedSourceStore};

/// Declared configuration of the hosted registry service.
#[derive(Clone, Debug)]
pub struct Config {
    pub listen: String,
    pub journal: PathBuf,
    pub mirror: PathBuf,
    pub verified: PathBuf,
    pub workspace: PathBuf,
    pub builder_image_digest: [u8; 32],
    pub builder_path: String,
    pub build_timeout_seconds: u64,
    pub attempts: u32,
    pub staleness_ms: u64,
    pub node_endpoint: String,
    pub node_authorization: String,
    pub receipt_authority_endpoint: String,
    pub receipt_authority_authorization: String,
    pub receipt_authority_replica_id: [u8; 32],
    pub sequencer_trust_history: PathBuf,
}

/// Stable graph anchor for the hosted program registry.
#[must_use]
pub const fn platform_program_registry() -> &'static str {
    "receipt-verified-program-registry-v1"
}

pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| io::Error::other("record path has no file name"))?
        .to_owned();
    name.push_str(".tmp");
    let temporary = path.with_file_name(name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&temporary, path)
}

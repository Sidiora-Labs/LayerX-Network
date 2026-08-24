//! Durable record of completed source verifications.
//!
//! Rebuilding is expensive, so a completed verification is persisted and
//! replayed at start-up. A replayed record never asserts a verdict on its own:
//! it carries the digest the rebuild produced, and the registry compares that
//! digest with registered protocol state again, so a record for a program that
//! has since been upgraded resurfaces as a visible mismatch.

use std::fs;
use std::path::PathBuf;

use layerx_programs::{hex, BuildPlan, ProgramId};
use serde_json::{json, Value};

use crate::write_atomic;

const RECORD_SUFFIX: &str = "verified";

/// One completed rebuild of one registered program version.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedSource {
    pub program: ProgramId,
    pub version: u32,
    pub source_uri: String,
    pub source_digest: [u8; 32],
    pub artifact_digest: [u8; 32],
    pub plan: BuildPlan,
}

/// Store of completed rebuilds, one file per program version.
#[derive(Clone, Debug)]
pub struct VerifiedSourceStore {
    root: PathBuf,
}

impl VerifiedSourceStore {
    /// Opens, creating the store directory when it is absent.
    ///
    /// # Errors
    ///
    /// Returns the filesystem error that prevented the directory from opening.
    pub fn open(root: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&root)
            .map_err(|error| format!("could not open the verified source store: {error}"))?;
        Ok(Self { root })
    }

    /// Persists one completed rebuild.
    ///
    /// # Errors
    ///
    /// Returns the filesystem error that prevented durable persistence.
    pub fn record(&self, entry: &VerifiedSource) -> Result<(), String> {
        let document = json!({
            "program": hex::encode(&entry.program.bytes()),
            "version": entry.version,
            "source_uri": entry.source_uri,
            "source_digest": hex::encode(&entry.source_digest),
            "artifact_digest": hex::encode(&entry.artifact_digest),
            "plan": entry.plan.encode(),
        });
        let path = self.root.join(format!(
            "{}-{}.{RECORD_SUFFIX}",
            hex::encode(&entry.program.bytes()),
            entry.version
        ));
        write_atomic(&path, document.to_string().as_bytes())
            .map_err(|error| format!("could not persist {}: {error}", path.display()))
    }

    /// Reads every completed rebuild in program order.
    ///
    /// # Errors
    ///
    /// Returns unreadable directories and refuses corrupt records rather than
    /// dropping a verification silently.
    pub fn records(&self) -> Result<Vec<VerifiedSource>, String> {
        let mut paths = Vec::new();
        let entries = fs::read_dir(&self.root)
            .map_err(|error| format!("could not read the verified source store: {error}"))?;
        for entry in entries {
            let path = entry
                .map_err(|error| format!("could not read the verified source store: {error}"))?
                .path();
            if path.extension().is_some_and(|value| value == RECORD_SUFFIX) {
                paths.push(path);
            }
        }
        paths.sort();
        let mut records = Vec::with_capacity(paths.len());
        for path in paths {
            let bytes = fs::read(&path)
                .map_err(|error| format!("could not read {}: {error}", path.display()))?;
            let record = decode(&bytes).ok_or_else(|| format!("{} is corrupt", path.display()))?;
            records.push(record);
        }
        Ok(records)
    }
}

fn decode(bytes: &[u8]) -> Option<VerifiedSource> {
    let document: Value = serde_json::from_slice(bytes).ok()?;
    Some(VerifiedSource {
        program: ProgramId::new(hex::decode_digest(document["program"].as_str()?).ok()?).ok()?,
        version: u32::try_from(document["version"].as_u64()?).ok()?,
        source_uri: document["source_uri"].as_str()?.to_owned(),
        source_digest: hex::decode_digest(document["source_digest"].as_str()?).ok()?,
        artifact_digest: hex::decode_digest(document["artifact_digest"].as_str()?).ok()?,
        plan: BuildPlan::parse(document["plan"].as_str()?).ok()?,
    })
}

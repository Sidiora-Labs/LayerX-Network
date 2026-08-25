//! Durable protocol deployment proofs and derived registry projections.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use layerx_programs::{
    hex, DeploymentJournal, DeploymentProof, DeploymentRecord, ObservedHead, RegistryError,
    VerifiedDeploymentEvidence,
};

use crate::write_atomic;

const RECORD_SUFFIX: &str = "deployment";
const ADMISSION_SUFFIX: &str = "admission";
const HEAD_FILE: &str = "head";

/// Journal of accepted deployments and upgrades observed at the node boundary.
#[derive(Clone, Debug)]
pub struct FileDeploymentJournal {
    root: PathBuf,
}

impl FileDeploymentJournal {
    /// Opens, creating the journal directory when it is absent.
    ///
    /// # Errors
    ///
    /// Returns the filesystem error that prevented the directory from opening.
    pub fn open(root: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&root)
            .map_err(|error| format!("could not open the deployment journal: {error}"))?;
        Ok(Self { root })
    }

    /// Reads every untrusted protocol proof for cryptographic replay by the
    /// configured verifier.
    ///
    /// # Errors
    ///
    /// Returns unreadable directories, undecodable records and misfiled
    /// records.
    pub fn proofs(&self) -> Result<Vec<DeploymentProof>, String> {
        let mut paths = Vec::new();
        let mut projections = BTreeSet::new();
        let entries = fs::read_dir(&self.root)
            .map_err(|error| format!("could not read the deployment journal: {error}"))?;
        for entry in entries {
            let path = entry
                .map_err(|error| format!("could not read the deployment journal: {error}"))?
                .path();
            if path.extension().is_some_and(|value| value == ADMISSION_SUFFIX) {
                paths.push(path);
            } else if path.extension().is_some_and(|value| value == RECORD_SUFFIX) {
                let name = path
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .ok_or_else(|| format!("{} has no canonical file name", path.display()))?;
                projections.insert(name.to_owned());
            }
        }
        paths.sort();
        let admissions = paths
            .iter()
            .map(|path| {
                path.file_stem()
                    .and_then(|value| value.to_str())
                    .map(str::to_owned)
                    .ok_or_else(|| format!("{} has no canonical file name", path.display()))
            })
            .collect::<Result<BTreeSet<_>, _>>()?;
        if admissions != projections {
            return Err(
                "every deployment projection must have one protocol admission proof".to_owned(),
            );
        }
        let mut proofs = Vec::with_capacity(paths.len());
        for path in paths {
            let bytes = fs::read(&path)
                .map_err(|error| format!("could not read {}: {error}", path.display()))?;
            let proof = DeploymentProof::decode(&bytes)
                .map_err(|error| format!("{} is corrupt: {error}", path.display()))?;
            let named = path
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            let claimed = proof
                .claimed_receipt_digest()
                .map_err(|error| format!("{} has no canonical receipt: {error}", path.display()))?;
            if named != hex::encode(&claimed) {
                return Err(format!(
                    "{} is filed under a different receipt digest",
                    path.display()
                ));
            }
            proofs.push(proof);
        }
        Ok(proofs)
    }

    /// Appends one accepted deployment and returns the digest it is filed
    /// under.
    ///
    /// # Errors
    ///
    /// Refuses records that fail their own validation and returns the
    /// filesystem error that prevented durable persistence.
    pub fn append(&self, evidence: &VerifiedDeploymentEvidence) -> Result<[u8; 32], String> {
        let digest = evidence.receipt_digest();
        let record_path = self
            .root
            .join(format!("{}.{RECORD_SUFFIX}", hex::encode(&digest)));
        write_atomic(&record_path, &evidence.record().canonical_encoding())
            .map_err(|error| format!("could not persist {}: {error}", record_path.display()))?;
        let proof_path = self
            .root
            .join(format!("{}.{ADMISSION_SUFFIX}", hex::encode(&digest)));
        write_atomic(&proof_path, &evidence.proof().canonical_encoding())
            .map_err(|error| format!("could not persist {}: {error}", proof_path.display()))?;
        Ok(digest)
    }

    /// Checks that the local projection sidecar is exactly the record derived
    /// from cryptographically verified protocol evidence.
    pub fn audit_projection(&self, evidence: &VerifiedDeploymentEvidence) -> Result<(), String> {
        let path = self.root.join(format!(
            "{}.{RECORD_SUFFIX}",
            hex::encode(&evidence.receipt_digest())
        ));
        let bytes = fs::read(&path)
            .map_err(|error| format!("could not read {}: {error}", path.display()))?;
        let record = DeploymentRecord::decode(&bytes)
            .map_err(|error| format!("{} is corrupt: {error}", path.display()))?;
        record
            .validate()
            .map_err(|error| format!("{} is corrupt: {error}", path.display()))?;
        if &record != evidence.record() {
            return Err(format!(
                "{} disagrees with its verified protocol proof",
                path.display()
            ));
        }
        Ok(())
    }

    /// Removes one record that the registry projection refused, so the journal
    /// and the projection never disagree.
    ///
    /// # Errors
    ///
    /// Returns the filesystem error that prevented removal.
    pub fn discard(&self, digest: [u8; 32]) -> Result<(), String> {
        let proof_path = self
            .root
            .join(format!("{}.{ADMISSION_SUFFIX}", hex::encode(&digest)));
        fs::remove_file(&proof_path)
            .map_err(|error| format!("could not discard {}: {error}", proof_path.display()))?;
        let record_path = self
            .root
            .join(format!("{}.{RECORD_SUFFIX}", hex::encode(&digest)));
        fs::remove_file(&record_path)
            .map_err(|error| format!("could not discard {}: {error}", record_path.display()))
    }

    /// Records the protocol head last observed by the node boundary.
    ///
    /// # Errors
    ///
    /// Refuses absent observations and returns the filesystem error that
    /// prevented durable persistence.
    pub fn refresh_head(&self, head: ObservedHead) -> Result<(), String> {
        if head.sequence == 0 || head.observed_at == 0 {
            return Err("an observed head needs a sequence and an observation time".to_owned());
        }
        let path = self.root.join(HEAD_FILE);
        write_atomic(
            &path,
            format!("{}\t{}\n", head.sequence, head.observed_at).as_bytes(),
        )
        .map_err(|error| format!("could not persist {}: {error}", path.display()))
    }
}

impl DeploymentJournal for FileDeploymentJournal {
    fn canonical_record(&self, receipt_digest: [u8; 32]) -> Result<Vec<u8>, RegistryError> {
        let path = self
            .root
            .join(format!("{}.{RECORD_SUFFIX}", hex::encode(&receipt_digest)));
        fs::read(path).map_err(|_| RegistryError::JournalUnavailable)
    }

    fn observed_head(&self) -> Result<ObservedHead, RegistryError> {
        let text = fs::read_to_string(self.root.join(HEAD_FILE))
            .map_err(|_| RegistryError::JournalUnavailable)?;
        let (sequence, observed_at) = text
            .trim()
            .split_once('\t')
            .ok_or(RegistryError::JournalUnavailable)?;
        Ok(ObservedHead {
            sequence: sequence
                .parse()
                .map_err(|_| RegistryError::JournalUnavailable)?,
            observed_at: observed_at
                .parse()
                .map_err(|_| RegistryError::JournalUnavailable)?,
        })
    }
}

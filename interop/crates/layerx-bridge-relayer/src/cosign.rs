//! Exchange of attestor signatures between relayer instances.
//!
//! Neither verifier aggregates partial signatures across transactions:
//! `bridgeIn` and `PaxeerXVault.release` each need `threshold` signatures,
//! ascending by signer, in one call. Each relayer instance holds one attestor
//! key, so with `threshold > 1` the instances publish their signature for a
//! digest into a shared directory (a shared volume or a synchronised bucket
//! mount) at `{digest}/{signer}.sig`, and every instance reads the others'.
//! Entries are untrusted: each is recovered against the digest and kept only
//! when it recovers to the signer its name claims and that signer is a current
//! attestor on the destination.

use std::fs;
use std::io::Write as _;
use std::path::PathBuf;

use crate::attestation::recover_signer;
use crate::hex;
use crate::journal::JournalError;

const MAX_ENTRIES: usize = 256;
const MAX_ENTRY_BYTES: u64 = 256;

pub struct CosignDirectory {
    root: PathBuf,
}

impl CosignDirectory {
    #[must_use]
    pub const fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Publishes this instance's signature for `digest` atomically.
    ///
    /// # Errors
    ///
    /// Returns any i/o failure.
    pub fn publish(
        &self,
        digest: &[u8; 32],
        signer: &[u8; 20],
        signature: &[u8; 65],
    ) -> Result<(), JournalError> {
        let directory = self.root.join(hex::encode(digest));
        fs::create_dir_all(&directory).map_err(|error| JournalError::Io(error.to_string()))?;
        let name = format!("{}.sig", hex::encode(signer));
        let target = directory.join(&name);
        let staging = directory.join(format!(".{name}.{}", std::process::id()));
        let mut file =
            fs::File::create(&staging).map_err(|error| JournalError::Io(error.to_string()))?;
        file.write_all(hex::prefixed(signature).as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(|error| JournalError::Io(error.to_string()))?;
        fs::rename(&staging, &target).map_err(|error| JournalError::Io(error.to_string()))
    }

    /// Every well-formed signature published for `digest` whose recovered
    /// signer matches its file name. Unreadable or malformed entries are
    /// skipped, never fatal.
    #[must_use]
    pub fn collect(&self, digest: &[u8; 32]) -> Vec<[u8; 65]> {
        let Ok(entries) = fs::read_dir(self.root.join(hex::encode(digest))) else {
            return Vec::new();
        };
        let mut signatures = Vec::new();
        for entry in entries.flatten().take(MAX_ENTRIES) {
            let name = entry.file_name();
            let Some(stem) = name.to_str().and_then(|name| name.strip_suffix(".sig")) else {
                continue;
            };
            let Ok(claimed) = hex::fixed::<20>(&format!("0x{stem}")) else {
                continue;
            };
            if entry.metadata().map_or(true, |metadata| {
                !metadata.is_file() || metadata.len() > MAX_ENTRY_BYTES
            }) {
                continue;
            }
            let Ok(text) = fs::read_to_string(entry.path()) else {
                continue;
            };
            let Ok(signature) = hex::fixed::<65>(text.trim()) else {
                continue;
            };
            if recover_signer(digest, &signature) == Ok(claimed) {
                signatures.push(signature);
            }
        }
        signatures
    }
}

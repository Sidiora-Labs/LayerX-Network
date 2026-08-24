use std::fs;
use std::path::{Path, PathBuf};

use layerx_programs::{hex, ProgramId};
use layerx_programs_protocol_adapter::ProtocolProgramStateRead;
use sha2::{Digest as _, Sha256};

use crate::write_atomic;
use crate::ProgramStateCursor;

const RECORD_SUFFIX: &str = ".program-state";
const CURSOR_FILE: &str = "canonical.cursor";

/// Durable, proof-carrying projection of Programs lifecycle, primary account
/// bindings, exit routes, history and the last verified account-state head.
pub struct FileProgramStateJournal {
    root: PathBuf,
}

impl FileProgramStateJournal {
    pub fn open(root: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&root).map_err(|error| {
            format!(
                "cannot create program-state journal {}: {error}",
                root.display()
            )
        })?;
        Ok(Self { root })
    }

    pub fn store(&self, state: &ProtocolProgramStateRead) -> Result<(), String> {
        let bytes = state
            .canonical_encode()
            .map_err(|error| format!("program-state record is not canonical: {error:?}"))?;
        let digest: [u8; 32] = Sha256::digest(&bytes).into();
        let path = self.record_path(state.program(), digest);
        write_atomic(&path, &bytes).map_err(|error| {
            format!(
                "cannot persist program-state record {}: {error}",
                path.display()
            )
        })
    }

    /// Hash-checks every local cache candidate. This never constructs a
    /// verified read: restart publication requires a fresh node receipt/head
    /// resolution and `ProtocolProgramStateRead::restore_verified`.
    pub fn audit(&self) -> Result<(), String> {
        let mut paths = fs::read_dir(&self.root)
            .map_err(|error| {
                format!(
                    "cannot read program-state journal {}: {error}",
                    self.root.display()
                )
            })?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("cannot enumerate program-state journal: {error}"))?;
        paths.retain(|path| is_record(path));
        paths.sort();

        for path in paths {
            let bytes = fs::read(&path).map_err(|error| {
                format!(
                    "cannot read program-state record {}: {error}",
                    path.display()
                )
            })?;
            let digest: [u8; 32] = Sha256::digest(&bytes).into();
            let named = path
                .file_stem()
                .and_then(|name| name.to_str())
                .and_then(|name| name.rsplit_once('.'))
                .map(|(_, digest)| digest);
            let expected = hex::encode(&digest);
            if named != Some(expected.as_str()) {
                return Err(format!(
                    "program-state cache {} does not match its content digest",
                    path.display()
                ));
            }
        }
        Ok(())
    }

    pub fn cursor(&self) -> Result<ProgramStateCursor, String> {
        let path = self.root.join(CURSOR_FILE);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ProgramStateCursor::default())
            }
            Err(error) => {
                return Err(format!(
                    "cannot read program-state cursor {}: {error}",
                    path.display()
                ))
            }
        };
        let (sequence, ordinal) = text
            .trim()
            .split_once('\t')
            .ok_or_else(|| format!("program-state cursor {} is corrupt", path.display()))?;
        Ok(ProgramStateCursor {
            sequence: sequence
                .parse()
                .map_err(|_| format!("program-state cursor {} is corrupt", path.display()))?,
            ordinal: ordinal
                .parse()
                .map_err(|_| format!("program-state cursor {} is corrupt", path.display()))?,
        })
    }

    pub fn advance(&self, cursor: ProgramStateCursor) -> Result<(), String> {
        if cursor.ordinal != 0 {
            return Err("program-state scan cursor cannot carry an event ordinal".to_owned());
        }
        if cursor < self.cursor()? {
            return Err("program-state cursor cannot regress".to_owned());
        }
        let path = self.root.join(CURSOR_FILE);
        write_atomic(
            &path,
            format!("{}\t{}\n", cursor.sequence, cursor.ordinal).as_bytes(),
        )
        .map_err(|error| {
            format!(
                "cannot persist program-state cursor {}: {error}",
                path.display()
            )
        })
    }

    fn record_path(&self, program: ProgramId, digest: [u8; 32]) -> PathBuf {
        self.root.join(format!(
            "{}.{}{}",
            hex::encode(&program.bytes()),
            hex::encode(&digest),
            RECORD_SUFFIX
        ))
    }
}

fn is_record(path: &Path) -> bool {
    path.is_file()
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(RECORD_SUFFIX))
}

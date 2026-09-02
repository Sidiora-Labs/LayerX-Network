//! Durable protocol deployment proofs and derived registry projections.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Display};
use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use layerx_programs::{
    hex, DeploymentJournal, DeploymentProof, DeploymentRecord, ObservedHead, RegistryError,
    VerifiedDeploymentEvidence,
};
use sha2::{Digest as _, Sha256};

use crate::write_atomic;

const ENVELOPE_SUFFIX: &str = "envelope";
const ENVELOPE_DOMAIN: &[u8] = b"LayerX/deployment-envelope/v1\0";
const RECORD_SUFFIX: &str = "deployment";
const ADMISSION_SUFFIX: &str = "admission";
const TEMPORARY_SUFFIX: &str = "tmp";
const HEAD_FILE: &str = "head";
const SEAL_BYTES: usize = 32;

/// One accepted deployment record together with the protocol admission proof
/// it was derived from. The two are published to the journal as a single
/// commit unit: one canonical envelope staged in a temporary file, fsynced and
/// renamed into place, so a restart observes both halves or neither.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeploymentEnvelope {
    receipt_digest: [u8; 32],
    record: DeploymentRecord,
    proof: DeploymentProof,
}

impl DeploymentEnvelope {
    /// Seals the record and proof carried by cryptographically verified
    /// evidence.
    #[must_use]
    pub fn from_evidence(evidence: &VerifiedDeploymentEvidence) -> Self {
        Self {
            receipt_digest: evidence.receipt_digest(),
            record: evidence.record().clone(),
            proof: evidence.proof().clone(),
        }
    }

    /// Pairs a decoded record with the proof it was filed beside.
    ///
    /// # Errors
    ///
    /// Refuses a proof whose receipt cannot be digested.
    pub fn pair(record: DeploymentRecord, proof: DeploymentProof) -> Result<Self, UnitDefect> {
        let receipt_digest =
            proof
                .claimed_receipt_digest()
                .map_err(|error| UnitDefect::Corrupt {
                    part: UnitPart::Proof,
                    reason: error.to_string(),
                })?;
        Ok(Self {
            receipt_digest,
            record,
            proof,
        })
    }

    /// Returns the receipt digest the unit is filed under.
    #[must_use]
    pub const fn receipt_digest(&self) -> [u8; 32] {
        self.receipt_digest
    }

    #[must_use]
    pub const fn record(&self) -> &DeploymentRecord {
        &self.record
    }

    #[must_use]
    pub const fn proof(&self) -> &DeploymentProof {
        &self.proof
    }

    #[must_use]
    pub fn into_proof(self) -> DeploymentProof {
        self.proof
    }

    /// Encodes the unit: the envelope domain, the framed canonical record, the
    /// framed canonical proof and a seal over everything before it.
    #[must_use]
    pub fn canonical_encoding(&self) -> Vec<u8> {
        let (mut bytes, proof_frame, seal) = self.frames();
        bytes.extend_from_slice(&proof_frame);
        bytes.extend_from_slice(&seal);
        bytes
    }

    /// Decodes one committed unit.
    ///
    /// # Errors
    ///
    /// Names the part the bytes end before, the part that does not decode, or
    /// a seal that does not cover the bytes.
    pub fn decode(bytes: &[u8]) -> Result<Self, UnitDefect> {
        let (record_bytes, proof_bytes) = sealed_frames(bytes)?;
        let record = decode_record(record_bytes)?;
        let proof = DeploymentProof::decode(proof_bytes).map_err(|error| UnitDefect::Corrupt {
            part: UnitPart::Proof,
            reason: error.to_string(),
        })?;
        Self::pair(record, proof)
    }

    fn frames(&self) -> (Vec<u8>, Vec<u8>, [u8; SEAL_BYTES]) {
        let record = self.record.canonical_encoding();
        let proof = self.proof.canonical_encoding();
        let mut record_frame = Vec::with_capacity(ENVELOPE_DOMAIN.len() + 4 + record.len());
        record_frame.extend_from_slice(ENVELOPE_DOMAIN);
        put_frame(&mut record_frame, &record);
        let mut proof_frame = Vec::with_capacity(4 + proof.len());
        put_frame(&mut proof_frame, &proof);
        let mut seal = Sha256::new();
        seal.update(&record_frame);
        seal.update(&proof_frame);
        (record_frame, proof_frame, seal.finalize().into())
    }
}

fn put_frame(bytes: &mut Vec<u8>, frame: &[u8]) {
    bytes.extend_from_slice(&u32::try_from(frame.len()).unwrap_or(u32::MAX).to_be_bytes());
    bytes.extend_from_slice(frame);
}

fn take_frame<'a>(bytes: &'a [u8], cursor: &mut usize) -> Option<&'a [u8]> {
    let start = cursor.checked_add(4)?;
    let prefix: [u8; 4] = bytes.get(*cursor..start)?.try_into().ok()?;
    let length = usize::try_from(u32::from_be_bytes(prefix)).ok()?;
    let end = start.checked_add(length)?;
    let frame = bytes.get(start..end)?;
    *cursor = end;
    Some(frame)
}

fn sealed_frames(bytes: &[u8]) -> Result<(&[u8], &[u8]), UnitDefect> {
    let domain = bytes.get(..ENVELOPE_DOMAIN.len()).unwrap_or(bytes);
    if !ENVELOPE_DOMAIN.starts_with(domain) {
        return Err(UnitDefect::Corrupt {
            part: UnitPart::Record,
            reason: "the envelope carries a foreign domain".to_owned(),
        });
    }
    if domain.len() < ENVELOPE_DOMAIN.len() {
        return Err(UnitDefect::Missing(UnitPart::Record));
    }
    let mut cursor = ENVELOPE_DOMAIN.len();
    let record = take_frame(bytes, &mut cursor).ok_or(UnitDefect::Missing(UnitPart::Record))?;
    let proof = take_frame(bytes, &mut cursor).ok_or(UnitDefect::Missing(UnitPart::Proof))?;
    let sealed = cursor;
    let seal = bytes
        .get(sealed..sealed.saturating_add(SEAL_BYTES))
        .filter(|seal| seal.len() == SEAL_BYTES)
        .ok_or(UnitDefect::Missing(UnitPart::Seal))?;
    if sealed.saturating_add(SEAL_BYTES) != bytes.len() {
        return Err(UnitDefect::Corrupt {
            part: UnitPart::Seal,
            reason: "the envelope carries bytes after its seal".to_owned(),
        });
    }
    let expected: [u8; SEAL_BYTES] = Sha256::digest(&bytes[..sealed]).into();
    if seal != expected {
        return Err(UnitDefect::Corrupt {
            part: UnitPart::Seal,
            reason: "the seal does not cover the record and proof".to_owned(),
        });
    }
    Ok((record, proof))
}

fn decode_record(bytes: &[u8]) -> Result<DeploymentRecord, UnitDefect> {
    let record = DeploymentRecord::decode(bytes).map_err(|error| UnitDefect::Corrupt {
        part: UnitPart::Record,
        reason: error.to_string(),
    })?;
    record.validate().map_err(|error| UnitDefect::Corrupt {
        part: UnitPart::Record,
        reason: error.to_string(),
    })?;
    Ok(record)
}

/// One half of a commit unit, its seal, or the commit that publishes it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnitPart {
    Record,
    Proof,
    Seal,
    Commit,
}

impl Display for UnitPart {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Record => "deployment record",
            Self::Proof => "admission proof",
            Self::Seal => "seal",
            Self::Commit => "commit",
        })
    }
}

/// Why one unit was set aside instead of loaded. Every variant names what the
/// unit is missing or which part failed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UnitDefect {
    /// Publication stopped before the unit was committed; the interrupted
    /// attempt ends before this part.
    Interrupted(UnitPart),
    /// A committed unit ends before this part.
    Missing(UnitPart),
    /// A committed part does not decode.
    Corrupt { part: UnitPart, reason: String },
    /// The proof names a different receipt digest than the file it is filed
    /// under.
    Misfiled { claimed: [u8; 32] },
    /// A unit file could not be read.
    Unreadable(String),
}

impl Display for UnitDefect {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Interrupted(part) => {
                write!(formatter, "publication was interrupted before its {part}")
            }
            Self::Missing(part) => write!(formatter, "the committed unit has no {part}"),
            Self::Corrupt { part, reason } => {
                write!(formatter, "its {part} does not decode: {reason}")
            }
            Self::Misfiled { claimed } => write!(
                formatter,
                "its proof is filed under a different receipt digest {}",
                hex::encode(claimed)
            ),
            Self::Unreadable(reason) => write!(formatter, "it could not be read: {reason}"),
        }
    }
}

/// One unit the journal quarantined at load, with the files that belong to it
/// and the typed reason it is incomplete.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuarantinedUnit {
    pub receipt_digest: [u8; 32],
    pub paths: Vec<PathBuf>,
    pub defect: UnitDefect,
}

impl Display for QuarantinedUnit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "deployment unit {} was quarantined: {}",
            hex::encode(&self.receipt_digest),
            self.defect
        )?;
        for path in &self.paths {
            write!(formatter, " [{}]", path.display())?;
        }
        Ok(())
    }
}

/// Everything the journal holds: the complete units in digest order, the
/// incomplete units it quarantined, and files left behind by attempts that a
/// later commit superseded.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct JournalLoad {
    pub units: Vec<DeploymentEnvelope>,
    pub quarantined: Vec<QuarantinedUnit>,
    pub leftovers: Vec<PathBuf>,
}

/// The steps of one deployment publication, in order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WriteStep {
    CreateTemporary,
    WriteRecord,
    WriteProof,
    WriteSeal,
    SyncTemporary,
    Commit,
    SyncDirectory,
}

impl WriteStep {
    pub const ALL: [Self; 7] = [
        Self::CreateTemporary,
        Self::WriteRecord,
        Self::WriteProof,
        Self::WriteSeal,
        Self::SyncTemporary,
        Self::Commit,
        Self::SyncDirectory,
    ];
}

impl Display for WriteStep {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::CreateTemporary => "create-temporary",
            Self::WriteRecord => "write-record",
            Self::WriteProof => "write-proof",
            Self::WriteSeal => "write-seal",
            Self::SyncTemporary => "sync-temporary",
            Self::Commit => "commit",
            Self::SyncDirectory => "sync-directory",
        })
    }
}

#[derive(Default)]
struct UnitFiles {
    envelope: Option<PathBuf>,
    envelope_temporary: Option<PathBuf>,
    record: Option<PathBuf>,
    proof: Option<PathBuf>,
    legacy_temporaries: Vec<PathBuf>,
}

impl UnitFiles {
    fn all(&self) -> Vec<PathBuf> {
        self.envelope
            .iter()
            .chain(self.envelope_temporary.iter())
            .chain(self.record.iter())
            .chain(self.proof.iter())
            .chain(self.legacy_temporaries.iter())
            .cloned()
            .collect()
    }

    fn except(&self, kept: &Path) -> Vec<PathBuf> {
        self.all().into_iter().filter(|path| path != kept).collect()
    }
}

/// Journal of accepted deployments and upgrades observed at the node boundary.
#[derive(Clone, Debug)]
pub struct FileDeploymentJournal {
    root: PathBuf,
    interrupt_before: Option<WriteStep>,
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
        Ok(Self {
            root,
            interrupt_before: None,
        })
    }

    /// Makes every publication stop immediately before the named step,
    /// leaving the directory exactly as a crash at that point would.
    #[must_use]
    pub const fn interrupt_before(mut self, step: WriteStep) -> Self {
        self.interrupt_before = Some(step);
        self
    }

    /// Loads every complete unit and quarantines every incomplete one with a
    /// typed report. One incomplete unit never rejects the journal and no
    /// unit is dropped without a report.
    ///
    /// # Errors
    ///
    /// Returns only an unreadable journal directory.
    pub fn load(&self) -> Result<JournalLoad, String> {
        let mut files: BTreeMap<[u8; 32], UnitFiles> = BTreeMap::new();
        let entries = fs::read_dir(&self.root)
            .map_err(|error| format!("could not read the deployment journal: {error}"))?;
        for entry in entries {
            let path = entry
                .map_err(|error| format!("could not read the deployment journal: {error}"))?
                .path();
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            let Some((stem, extension)) = name.rsplit_once('.') else {
                continue;
            };
            let (stem, kind, temporary) = if extension == TEMPORARY_SUFFIX {
                let Some((stem, kind)) = stem.rsplit_once('.') else {
                    continue;
                };
                (stem, kind, true)
            } else {
                (stem, extension, false)
            };
            if !matches!(kind, ENVELOPE_SUFFIX | RECORD_SUFFIX | ADMISSION_SUFFIX) {
                continue;
            }
            let Ok(digest) = hex::decode_digest(stem) else {
                continue;
            };
            if hex::encode(&digest) != stem {
                continue;
            }
            let unit = files.entry(digest).or_default();
            match (kind, temporary) {
                (ENVELOPE_SUFFIX, false) => unit.envelope = Some(path),
                (ENVELOPE_SUFFIX, true) => unit.envelope_temporary = Some(path),
                (RECORD_SUFFIX, false) => unit.record = Some(path),
                (ADMISSION_SUFFIX, false) => unit.proof = Some(path),
                _ => unit.legacy_temporaries.push(path),
            }
        }
        let mut loaded = JournalLoad::default();
        for (digest, unit) in files {
            classify_unit(digest, &unit, &mut loaded);
        }
        Ok(loaded)
    }

    /// Reads every untrusted protocol proof for cryptographic replay by the
    /// configured verifier.
    ///
    /// # Errors
    ///
    /// Returns unreadable directories, undecodable records, misfiled records
    /// and a journal whose committed projections and admission proofs are not
    /// the same set.
    pub fn proofs(&self) -> Result<Vec<DeploymentProof>, String> {
        let loaded = self.load()?;
        let mut projections = BTreeSet::new();
        let mut admissions = BTreeSet::new();
        for unit in &loaded.units {
            projections.insert(unit.receipt_digest());
            admissions.insert(unit.receipt_digest());
        }
        for quarantined in &loaded.quarantined {
            match quarantined.defect {
                UnitDefect::Missing(UnitPart::Proof) => {
                    projections.insert(quarantined.receipt_digest);
                }
                UnitDefect::Missing(UnitPart::Record) => {
                    admissions.insert(quarantined.receipt_digest);
                }
                _ => {}
            }
        }
        if admissions != projections {
            return Err(
                "every deployment projection must have one protocol admission proof".to_owned(),
            );
        }
        for quarantined in &loaded.quarantined {
            let path = quarantined
                .paths
                .first()
                .map(|path| path.display().to_string())
                .unwrap_or_default();
            match &quarantined.defect {
                UnitDefect::Interrupted(_) => {}
                UnitDefect::Missing(part) => {
                    return Err(format!("{path} is corrupt: it ends before its {part}"))
                }
                UnitDefect::Corrupt { reason, .. } => {
                    return Err(format!("{path} is corrupt: {reason}"))
                }
                UnitDefect::Misfiled { .. } => {
                    return Err(format!("{path} is filed under a different receipt digest"))
                }
                UnitDefect::Unreadable(reason) => {
                    return Err(format!("could not read {path}: {reason}"))
                }
            }
        }
        Ok(loaded
            .units
            .into_iter()
            .map(DeploymentEnvelope::into_proof)
            .collect())
    }

    /// Appends one accepted deployment as a single commit unit and returns the
    /// digest it is filed under.
    ///
    /// # Errors
    ///
    /// Returns the filesystem error, or the injected interruption, that
    /// stopped the unit from being committed; nothing partial is published.
    pub fn append(&self, evidence: &VerifiedDeploymentEvidence) -> Result<[u8; 32], String> {
        let envelope = DeploymentEnvelope::from_evidence(evidence);
        self.publish(&envelope)?;
        Ok(envelope.receipt_digest())
    }

    fn publish(&self, envelope: &DeploymentEnvelope) -> Result<(), String> {
        let digest = envelope.receipt_digest();
        let path = self.unit_path(digest, ENVELOPE_SUFFIX);
        let temporary = self.temporary_path(digest, ENVELOPE_SUFFIX);
        let (record_frame, proof_frame, seal) = envelope.frames();
        self.reach(WriteStep::CreateTemporary)?;
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary)
            .map_err(|error| format!("could not stage {}: {error}", temporary.display()))?;
        self.reach(WriteStep::WriteRecord)?;
        file.write_all(&record_frame)
            .map_err(|error| format!("could not stage {}: {error}", temporary.display()))?;
        self.reach(WriteStep::WriteProof)?;
        file.write_all(&proof_frame)
            .map_err(|error| format!("could not stage {}: {error}", temporary.display()))?;
        self.reach(WriteStep::WriteSeal)?;
        file.write_all(&seal)
            .map_err(|error| format!("could not stage {}: {error}", temporary.display()))?;
        self.reach(WriteStep::SyncTemporary)?;
        file.sync_all()
            .map_err(|error| format!("could not sync {}: {error}", temporary.display()))?;
        drop(file);
        self.reach(WriteStep::Commit)?;
        fs::rename(&temporary, &path)
            .map_err(|error| format!("could not commit {}: {error}", path.display()))?;
        self.reach(WriteStep::SyncDirectory)?;
        File::open(&self.root)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| format!("could not sync {}: {error}", self.root.display()))
    }

    fn reach(&self, step: WriteStep) -> Result<(), String> {
        if self.interrupt_before == Some(step) {
            return Err(format!(
                "deployment publication was interrupted before {step}"
            ));
        }
        Ok(())
    }

    /// Checks that the local projection is exactly the record derived from
    /// cryptographically verified protocol evidence.
    ///
    /// # Errors
    ///
    /// Returns an unreadable or corrupt unit, and a committed record that
    /// disagrees with the verified evidence.
    pub fn audit_projection(&self, evidence: &VerifiedDeploymentEvidence) -> Result<(), String> {
        let (path, record) = self.committed_record(evidence.receipt_digest())?;
        if &record != evidence.record() {
            return Err(format!(
                "{} disagrees with its verified protocol proof",
                path.display()
            ));
        }
        Ok(())
    }

    /// Removes one unit that the registry projection refused, so the journal
    /// and the projection never disagree.
    ///
    /// # Errors
    ///
    /// Returns the filesystem error that prevented removal.
    pub fn discard(&self, digest: [u8; 32]) -> Result<(), String> {
        let mut removed = false;
        for path in [
            self.unit_path(digest, ENVELOPE_SUFFIX),
            self.unit_path(digest, ADMISSION_SUFFIX),
            self.unit_path(digest, RECORD_SUFFIX),
            self.temporary_path(digest, ENVELOPE_SUFFIX),
            self.temporary_path(digest, ADMISSION_SUFFIX),
            self.temporary_path(digest, RECORD_SUFFIX),
        ] {
            match fs::remove_file(&path) {
                Ok(()) => removed = true,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("could not discard {}: {error}", path.display())),
            }
        }
        if removed {
            Ok(())
        } else {
            Err(format!(
                "could not discard {}: no such unit",
                self.unit_path(digest, ENVELOPE_SUFFIX).display()
            ))
        }
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

    fn committed_record(&self, digest: [u8; 32]) -> Result<(PathBuf, DeploymentRecord), String> {
        let envelope = self.unit_path(digest, ENVELOPE_SUFFIX);
        match fs::read(&envelope) {
            Ok(bytes) => {
                let (record, _) = sealed_frames(&bytes)
                    .map_err(|defect| format!("{} is corrupt: {defect}", envelope.display()))?;
                let record = decode_record(record)
                    .map_err(|defect| format!("{} is corrupt: {defect}", envelope.display()))?;
                return Ok((envelope, record));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("could not read {}: {error}", envelope.display())),
        }
        let legacy = self.unit_path(digest, RECORD_SUFFIX);
        let bytes = fs::read(&legacy)
            .map_err(|error| format!("could not read {}: {error}", legacy.display()))?;
        let record = decode_record(&bytes)
            .map_err(|defect| format!("{} is corrupt: {defect}", legacy.display()))?;
        Ok((legacy, record))
    }

    fn unit_path(&self, digest: [u8; 32], suffix: &str) -> PathBuf {
        self.root.join(format!("{}.{suffix}", hex::encode(&digest)))
    }

    fn temporary_path(&self, digest: [u8; 32], suffix: &str) -> PathBuf {
        self.root.join(format!(
            "{}.{suffix}.{TEMPORARY_SUFFIX}",
            hex::encode(&digest)
        ))
    }
}

fn classify_unit(digest: [u8; 32], unit: &UnitFiles, loaded: &mut JournalLoad) {
    if let Some(path) = &unit.envelope {
        match read_envelope(path) {
            Ok(envelope) if envelope.receipt_digest() == digest => {
                loaded.units.push(envelope);
                loaded.leftovers.extend(unit.except(path));
            }
            Ok(envelope) => loaded.quarantined.push(QuarantinedUnit {
                receipt_digest: digest,
                paths: unit.all(),
                defect: UnitDefect::Misfiled {
                    claimed: envelope.receipt_digest(),
                },
            }),
            Err(defect) => loaded.quarantined.push(QuarantinedUnit {
                receipt_digest: digest,
                paths: unit.all(),
                defect,
            }),
        }
        return;
    }
    match (&unit.record, &unit.proof) {
        (Some(record_path), Some(proof_path)) => match read_legacy(record_path, proof_path) {
            Ok(envelope) if envelope.receipt_digest() == digest => {
                loaded.units.push(envelope);
                loaded.leftovers.extend(
                    unit.envelope_temporary
                        .iter()
                        .chain(unit.legacy_temporaries.iter())
                        .cloned(),
                );
            }
            Ok(envelope) => loaded.quarantined.push(QuarantinedUnit {
                receipt_digest: digest,
                paths: unit.all(),
                defect: UnitDefect::Misfiled {
                    claimed: envelope.receipt_digest(),
                },
            }),
            Err(defect) => loaded.quarantined.push(QuarantinedUnit {
                receipt_digest: digest,
                paths: unit.all(),
                defect,
            }),
        },
        (Some(_), None) => loaded.quarantined.push(QuarantinedUnit {
            receipt_digest: digest,
            paths: unit.all(),
            defect: UnitDefect::Missing(UnitPart::Proof),
        }),
        (None, Some(_)) => loaded.quarantined.push(QuarantinedUnit {
            receipt_digest: digest,
            paths: unit.all(),
            defect: UnitDefect::Missing(UnitPart::Record),
        }),
        (None, None) => {
            let defect = match &unit.envelope_temporary {
                Some(path) => match read_envelope(path) {
                    Ok(_) => UnitDefect::Interrupted(UnitPart::Commit),
                    Err(UnitDefect::Missing(part)) => UnitDefect::Interrupted(part),
                    Err(defect) => defect,
                },
                None => UnitDefect::Interrupted(UnitPart::Record),
            };
            loaded.quarantined.push(QuarantinedUnit {
                receipt_digest: digest,
                paths: unit.all(),
                defect,
            });
        }
    }
}

fn read_envelope(path: &Path) -> Result<DeploymentEnvelope, UnitDefect> {
    let bytes = fs::read(path).map_err(|error| UnitDefect::Unreadable(error.to_string()))?;
    DeploymentEnvelope::decode(&bytes)
}

fn read_legacy(record_path: &Path, proof_path: &Path) -> Result<DeploymentEnvelope, UnitDefect> {
    let record_bytes =
        fs::read(record_path).map_err(|error| UnitDefect::Unreadable(error.to_string()))?;
    let record = decode_record(&record_bytes)?;
    let proof_bytes =
        fs::read(proof_path).map_err(|error| UnitDefect::Unreadable(error.to_string()))?;
    let proof = DeploymentProof::decode(&proof_bytes).map_err(|error| UnitDefect::Corrupt {
        part: UnitPart::Proof,
        reason: error.to_string(),
    })?;
    DeploymentEnvelope::pair(record, proof)
}

impl DeploymentJournal for FileDeploymentJournal {
    fn canonical_record(&self, receipt_digest: [u8; 32]) -> Result<Vec<u8>, RegistryError> {
        self.committed_record(receipt_digest)
            .map(|(_, record)| record.canonical_encoding())
            .map_err(|_| RegistryError::JournalUnavailable)
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

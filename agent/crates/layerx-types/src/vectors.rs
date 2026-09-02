//! Loader and coverage accounting for repository-owned conformance corpora.

pub mod checkpoint;

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use crate::payload::ActivityType;
use crate::result::ResultCode;

/// Protocol version from which every current domain definition is derived.
pub const DERIVED_PROTOCOL_VERSION: u32 = 1;

/// Domain definitions and their source protocol version.
pub const TYPE_PROTOCOL_VERSIONS: [(&str, u32); 12] = [
    ("Envelope", 1),
    ("Payload", 1),
    ("ActivityReceipt", 1),
    ("LxpReceipt", 1),
    ("BatchHeader", 1),
    ("CheckpointCertificate", 1),
    ("ActivityInclusionProof", 1),
    ("StateInclusionProof", 1),
    ("AvailabilityChunkInclusionProof", 1),
    ("AccountId", 1),
    ("AssetId", 1),
    ("ResultCode", 1),
];

/// Every published vector class understood by this crate.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum VectorClass {
    /// Canonical fixed-width integer codec vectors.
    CanonicalU64,
    /// Unknown-tag rejection vectors.
    TagRejection,
    /// Bounded byte-string rejection vectors.
    BoundedBytes,
    /// Sorted-sequence rejection vectors.
    SortedSequence,
    /// Canonical activity envelopes from replay history.
    ActivityEnvelope,
    /// Core-produced receipt bytes from replay history.
    ActivityReceipt,
    /// State roots committed by replay history.
    StateRoot,
    /// Batch roots committed at replay boundaries.
    BatchRoot,
    /// Ordered event records from replay history.
    Event,
    /// Published large-corpus qualification digests.
    QualificationDigest,
}

impl VectorClass {
    const ALL: [Self; 10] = [
        Self::CanonicalU64,
        Self::TagRejection,
        Self::BoundedBytes,
        Self::SortedSequence,
        Self::ActivityEnvelope,
        Self::ActivityReceipt,
        Self::StateRoot,
        Self::BatchRoot,
        Self::Event,
        Self::QualificationDigest,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::CanonicalU64 => "canonical-u64",
            Self::TagRejection => "tag-rejection",
            Self::BoundedBytes => "bounded-bytes",
            Self::SortedSequence => "sorted-sequence",
            Self::ActivityEnvelope => "activity-envelope",
            Self::ActivityReceipt => "activity-receipt",
            Self::StateRoot => "state-root",
            Self::BatchRoot => "batch-root",
            Self::Event => "event",
            Self::QualificationDigest => "qualification-digest",
        }
    }
}

/// One published text codec vector.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodecVector {
    /// Vector class name from the corpus.
    pub kind: String,
    /// Stable case name from the corpus.
    pub name: String,
    /// Exact published bytes.
    pub bytes: Vec<u8>,
    /// Exact expected protocol result.
    pub expected_result: ResultCode,
    /// Expected SHA-256 when the case succeeds.
    pub expected_sha256: Option<[u8; 32]>,
}

/// Metadata proven while walking the binary replay corpus.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplayCorpus {
    /// Protocol version declared by the corpus.
    pub protocol_version: u32,
    /// Count of canonical activity records.
    pub activity_count: usize,
    /// Every activity type in replay order.
    pub activity_types: Vec<ActivityType>,
    /// Canonical activity bytes in replay order.
    pub canonical_activities: Vec<Vec<u8>>,
    /// Core-produced receipt bytes in replay order.
    pub expected_receipts: Vec<Vec<u8>>,
}

/// All repository-owned published vectors loaded without copying them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Corpus {
    /// Canonical codec cases.
    pub valid_codec: Vec<CodecVector>,
    /// Adversarial codec cases.
    pub adversarial_codec: Vec<CodecVector>,
    /// Binary replay corpus metadata.
    pub replay: ReplayCorpus,
    /// Named qualification digest entries.
    pub qualification_digests: Vec<(String, String)>,
    exercised: BTreeSet<VectorClass>,
}

impl Corpus {
    /// Loads every currently published corpus directly beneath a repository.
    ///
    /// # Errors
    ///
    /// Returns a named I/O, format, unsupported-version, missing-capability, or
    /// unrepresentable-vector error. Unknown vector classes are never skipped.
    pub fn load(repository_root: &Path) -> Result<Self, CorpusError> {
        let valid_path = repository_root.join("tests/vectors/codec/valid.lxv");
        let adversarial_path = repository_root.join("tests/vectors/codec/adversarial.lxv");
        let replay_path = repository_root.join("tests/vectors/replay_corpus.lxb");
        let digest_path = repository_root.join("tests/vectors/qualification_replay_10m.digest");
        let (valid_codec, valid_classes) = load_codec(&valid_path)?;
        let (adversarial_codec, adversarial_classes) = load_codec(&adversarial_path)?;
        let (replay, replay_classes) = load_replay(&replay_path)?;
        let qualification_digests = load_digests(&digest_path)?;
        let mut exercised = valid_classes;
        exercised.extend(adversarial_classes);
        exercised.extend(replay_classes);
        if qualification_digests.is_empty() {
            return Err(CorpusError::Format {
                path: digest_path,
                detail: "qualification digest is empty".to_owned(),
            });
        }
        exercised.insert(VectorClass::QualificationDigest);
        Ok(Self {
            valid_codec,
            adversarial_codec,
            replay,
            qualification_digests,
            exercised,
        })
    }
}

/// Complete accounting of vector classes consumed by the suite.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoverageReport {
    /// Classes exercised by at least one published vector.
    pub exercised: Vec<VectorClass>,
    /// Known classes with no vector usage.
    pub unused: Vec<VectorClass>,
}

impl CoverageReport {
    /// Renders a stable human-readable coverage report.
    #[must_use]
    pub fn render(&self) -> String {
        let mut report = String::new();
        for class in &self.exercised {
            let _ = writeln!(report, "exercised {}", class.name());
        }
        for class in &self.unused {
            let _ = writeln!(report, "unused {}", class.name());
        }
        report
    }
}

/// Builds complete coverage accounting and rejects silent class omissions.
///
/// # Errors
///
/// Returns [`CorpusError::UnusedClasses`] when any known class was not
/// exercised by the loaded repository corpora.
pub fn coverage_report(corpus: &Corpus) -> Result<CoverageReport, CorpusError> {
    let exercised: Vec<_> = VectorClass::ALL
        .into_iter()
        .filter(|class| corpus.exercised.contains(class))
        .collect();
    let unused: Vec<_> = VectorClass::ALL
        .into_iter()
        .filter(|class| !corpus.exercised.contains(class))
        .collect();
    if !unused.is_empty() {
        return Err(CorpusError::UnusedClasses(unused));
    }
    Ok(CoverageReport { exercised, unused })
}

fn read(path: &Path) -> Result<Vec<u8>, CorpusError> {
    fs::read(path).map_err(|error| CorpusError::Io {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })
}

fn load_codec(path: &Path) -> Result<(Vec<CodecVector>, BTreeSet<VectorClass>), CorpusError> {
    let bytes = read(path)?;
    let text = std::str::from_utf8(&bytes).map_err(|error| CorpusError::Format {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })?;
    let mut vectors = Vec::new();
    let mut classes = BTreeSet::new();
    for (index, line) in text.lines().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<_> = line.split('|').collect();
        if fields.len() != 5 {
            return Err(format_error(path, index, "expected five fields"));
        }
        let class = match fields[0] {
            "u64" => VectorClass::CanonicalU64,
            "tag" => VectorClass::TagRejection,
            "bytes4" => VectorClass::BoundedBytes,
            "seq" => VectorClass::SortedSequence,
            unknown => {
                return Err(CorpusError::MissingCapability {
                    vector: fields[1].to_owned(),
                    capability: unknown.to_owned(),
                });
            }
        };
        let vector_bytes = decode_hex(fields[2]).map_err(|error| match error {
            CorpusError::Hex(detail) => format_error(path, index, detail),
            other => other,
        })?;
        let expected_raw = fields[3]
            .parse::<i32>()
            .map_err(|error| format_error(path, index, error.to_string()))?;
        let expected_result = ResultCode::from_raw(expected_raw);
        if expected_result.known().is_none() {
            return Err(CorpusError::UnrepresentableVector {
                vector: fields[1].to_owned(),
                capability: format!("result code {expected_raw}"),
            });
        }
        let expected_sha256 = if fields[4] == "-" {
            None
        } else {
            let digest = decode_hex(fields[4])?;
            let digest: [u8; 32] = digest.try_into().map_err(|_| CorpusError::Format {
                path: path.to_path_buf(),
                detail: format!("line {} digest is not 32 bytes", index + 1),
            })?;
            Some(digest)
        };
        classes.insert(class);
        vectors.push(CodecVector {
            kind: fields[0].to_owned(),
            name: fields[1].to_owned(),
            bytes: vector_bytes,
            expected_result,
            expected_sha256,
        });
    }
    if vectors.is_empty() {
        return Err(CorpusError::Format {
            path: path.to_path_buf(),
            detail: "codec corpus is empty".to_owned(),
        });
    }
    Ok((vectors, classes))
}

fn load_replay(path: &Path) -> Result<(ReplayCorpus, BTreeSet<VectorClass>), CorpusError> {
    let bytes = read(path)?;
    let mut reader = Reader::new(path, &bytes);
    if reader.take(8)? != b"LXPRP001" {
        return Err(reader.error("invalid replay magic"));
    }
    let protocol_version = reader.u32()?;
    if protocol_version != DERIVED_PROTOCOL_VERSION {
        return Err(CorpusError::UnsupportedVersion {
            path: path.to_path_buf(),
            declared: protocol_version,
            supported: DERIVED_PROTOCOL_VERSION,
        });
    }
    let count = usize::try_from(reader.u32()?).map_err(|error| reader.error(error.to_string()))?;
    if count == 0 || count > 256 {
        return Err(reader.error("replay record count is out of bounds"));
    }
    let _ = reader.take(64)?;
    let mut activity_types = Vec::with_capacity(count);
    let mut canonical_activities = Vec::with_capacity(count);
    let mut expected_receipts = Vec::with_capacity(count);
    let mut last_boundary = false;
    for index in 0..count {
        let sequence = reader.u64()?;
        if sequence != u64::try_from(index + 1).map_err(|error| reader.error(error.to_string()))? {
            return Err(reader.error("replay sequence gap"));
        }
        let boundary = reader.u8()?;
        if boundary > 1 {
            return Err(reader.error("invalid batch-boundary flag"));
        }
        last_boundary = boundary == 1;
        let activity = reader.sized(1_048_576)?;
        if activity.len() < 18 || activity[..5] != [0, 1, 0x10, 1, 12] {
            return Err(CorpusError::UnrepresentableVector {
                vector: format!("replay-activity-{}", index + 1),
                capability: "canonical activity envelope".to_owned(),
            });
        }
        let activity_version = u16::from_be_bytes([activity[6], activity[7]]);
        if u32::from(activity_version) != DERIVED_PROTOCOL_VERSION {
            return Err(CorpusError::UnsupportedVersion {
                path: path.to_path_buf(),
                declared: u32::from(activity_version),
                supported: DERIVED_PROTOCOL_VERSION,
            });
        }
        let raw_type = u32::from_be_bytes([activity[14], activity[15], activity[16], activity[17]]);
        let activity_type =
            ActivityType::from_u32(raw_type).map_err(|_| CorpusError::UnrepresentableVector {
                vector: format!("replay-activity-{}", index + 1),
                capability: format!("activity type {raw_type:#010x}"),
            })?;
        activity_types.push(activity_type);
        canonical_activities.push(activity.to_vec());
        let _ = reader.take(32)?;
        let receipt = reader.sized(106)?;
        if receipt.len() != 106 {
            return Err(reader.error("unexpected replay receipt width"));
        }
        expected_receipts.push(receipt.to_vec());
        let event = reader.sized(36)?;
        if event.len() != 36 {
            return Err(reader.error("unexpected replay event width"));
        }
        let _ = reader.take(32)?;
    }
    if !last_boundary || !reader.is_finished() {
        return Err(reader.error("replay corpus has trailing bytes or an open batch"));
    }
    let classes = BTreeSet::from([
        VectorClass::ActivityEnvelope,
        VectorClass::ActivityReceipt,
        VectorClass::StateRoot,
        VectorClass::BatchRoot,
        VectorClass::Event,
    ]);
    Ok((
        ReplayCorpus {
            protocol_version,
            activity_count: count,
            activity_types,
            canonical_activities,
            expected_receipts,
        },
        classes,
    ))
}

fn load_digests(path: &Path) -> Result<Vec<(String, String)>, CorpusError> {
    let bytes = read(path)?;
    let text = std::str::from_utf8(&bytes).map_err(|error| CorpusError::Format {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })?;
    text.lines()
        .enumerate()
        .map(|(index, line)| {
            let Some((name, value)) = line.split_once('=') else {
                return Err(format_error(path, index, "digest line lacks equals"));
            };
            if name.is_empty() || value.is_empty() {
                return Err(format_error(path, index, "empty digest name or value"));
            }
            Ok((name.to_owned(), value.to_owned()))
        })
        .collect()
}

fn decode_hex(value: &str) -> Result<Vec<u8>, CorpusError> {
    if !value.len().is_multiple_of(2) {
        return Err(CorpusError::Hex("odd hex length".to_owned()));
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = hex_nibble(pair[0])?;
            let low = hex_nibble(pair[1])?;
            Ok((high << 4) | low)
        })
        .collect()
}

fn hex_nibble(value: u8) -> Result<u8, CorpusError> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err(CorpusError::Hex("invalid hex digit".to_owned())),
    }
}

fn format_error(path: &Path, index: usize, detail: impl Into<String>) -> CorpusError {
    CorpusError::Format {
        path: path.to_path_buf(),
        detail: format!("line {}: {}", index + 1, detail.into()),
    }
}

struct Reader<'a> {
    path: &'a Path,
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    const fn new(path: &'a Path, bytes: &'a [u8]) -> Self {
        Self {
            path,
            bytes,
            offset: 0,
        }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], CorpusError> {
        let Some(end) = self.offset.checked_add(length) else {
            return Err(self.error("offset overflow"));
        };
        let Some(value) = self.bytes.get(self.offset..end) else {
            return Err(self.error("truncated replay corpus"));
        };
        self.offset = end;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, CorpusError> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, CorpusError> {
        let value: [u8; 4] = self
            .take(4)?
            .try_into()
            .map_err(|_| self.error("invalid u32"))?;
        Ok(u32::from_be_bytes(value))
    }

    fn u64(&mut self) -> Result<u64, CorpusError> {
        let value: [u8; 8] = self
            .take(8)?
            .try_into()
            .map_err(|_| self.error("invalid u64"))?;
        Ok(u64::from_be_bytes(value))
    }

    fn sized(&mut self, maximum: usize) -> Result<&'a [u8], CorpusError> {
        let length = usize::try_from(self.u32()?).map_err(|error| self.error(error.to_string()))?;
        if length > maximum {
            return Err(self.error("length exceeds declared maximum"));
        }
        self.take(length)
    }

    const fn is_finished(&self) -> bool {
        self.offset == self.bytes.len()
    }

    fn error(&self, detail: impl Into<String>) -> CorpusError {
        CorpusError::Format {
            path: self.path.to_path_buf(),
            detail: format!("offset {}: {}", self.offset, detail.into()),
        }
    }
}

/// Failure to load or account for a published conformance vector.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CorpusError {
    /// A repository-owned vector file could not be read.
    Io { path: PathBuf, detail: String },
    /// A corpus violated its published format.
    Format { path: PathBuf, detail: String },
    /// The corpus declared a protocol version this crate does not implement.
    UnsupportedVersion {
        path: PathBuf,
        declared: u32,
        supported: u32,
    },
    /// A named vector class has no implementation capability.
    MissingCapability { vector: String, capability: String },
    /// A known vector named a domain value this crate cannot represent.
    UnrepresentableVector { vector: String, capability: String },
    /// One or more known vector classes were silently unused.
    UnusedClasses(Vec<VectorClass>),
    /// A text vector contained invalid hexadecimal data.
    Hex(String),
}

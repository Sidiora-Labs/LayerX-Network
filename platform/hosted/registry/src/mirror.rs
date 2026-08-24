//! Content-addressed mirror of published program source.
//!
//! A verification request names a source location and a source digest. The
//! mirror is the operator-maintained store the service rebuilds from: it
//! returns an archive only when the archive hashes to exactly the requested
//! digest and was published under exactly the requested location.

use std::fmt::{self, Display};
use std::fs;
use std::path::PathBuf;

use layerx_programs::{hex, BuildPlan, BuildRefusal, PublishedSource, SourceArchive};
use sha2::{Digest as _, Sha256};

use crate::write_atomic;

const ARCHIVE_SUFFIX: &str = "archive";
const PLAN_SUFFIX: &str = "plan";
const URI_SUFFIX: &str = "uri";
const MAX_URI: usize = 512;

/// Published source, its declared build recipe and the location it was
/// published under.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MirroredSource {
    pub source: PublishedSource,
    pub plan: BuildPlan,
}

/// Typed refusal produced when the mirror cannot supply verifiable source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MirrorRefusal {
    NotMirrored,
    UriMismatch { published: String },
    DigestMismatch { mirrored: [u8; 32] },
    Unreadable { reason: String },
    Plan(BuildRefusal),
}

impl Display for MirrorRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotMirrored => {
                formatter.write_str("no published source is mirrored for that digest")
            }
            Self::UriMismatch { published } => {
                write!(formatter, "mirrored source was published at {published}")
            }
            Self::DigestMismatch { mirrored } => write!(
                formatter,
                "mirrored archive hashes to {}",
                hex::encode(mirrored)
            ),
            Self::Unreadable { reason } => write!(formatter, "source mirror unreadable: {reason}"),
            Self::Plan(refusal) => write!(formatter, "published build plan refused: {refusal}"),
        }
    }
}

impl std::error::Error for MirrorRefusal {}

/// Operator-maintained store of published program source.
#[derive(Clone, Debug)]
pub struct SourceMirror {
    root: PathBuf,
}

impl SourceMirror {
    /// Opens, creating the mirror directory when it is absent.
    ///
    /// # Errors
    ///
    /// Returns the filesystem error that prevented the directory from opening.
    pub fn open(root: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&root)
            .map_err(|error| format!("could not open the source mirror: {error}"))?;
        Ok(Self { root })
    }

    /// Publishes one canonical archive with its build recipe and returns the
    /// source digest verification requests must name.
    ///
    /// # Errors
    ///
    /// Refuses malformed locations and returns the filesystem error that
    /// prevented durable persistence.
    pub fn publish(
        &self,
        uri: &str,
        plan: &BuildPlan,
        archive: &SourceArchive,
    ) -> Result<[u8; 32], String> {
        if uri.is_empty()
            || uri.len() > MAX_URI
            || uri
                .bytes()
                .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
        {
            return Err("published source location is malformed".to_owned());
        }
        let bytes = archive.encode();
        let digest: [u8; 32] = Sha256::digest(&bytes).into();
        self.write(&digest, ARCHIVE_SUFFIX, &bytes)?;
        self.write(&digest, PLAN_SUFFIX, plan.encode().as_bytes())?;
        self.write(&digest, URI_SUFFIX, uri.as_bytes())?;
        Ok(digest)
    }

    /// Returns mirrored source only when it matches the requested location and
    /// digest exactly.
    ///
    /// # Errors
    ///
    /// Refuses absent, unreadable, relocated and altered source, and refuses
    /// published recipes that are not admissible build plans.
    pub fn fetch(&self, uri: &str, digest: [u8; 32]) -> Result<MirroredSource, MirrorRefusal> {
        let archive_path = self.path(&digest, ARCHIVE_SUFFIX);
        if !archive_path.exists() {
            return Err(MirrorRefusal::NotMirrored);
        }
        let canonical_archive =
            fs::read(&archive_path).map_err(|error| MirrorRefusal::Unreadable {
                reason: error.to_string(),
            })?;
        let mirrored: [u8; 32] = Sha256::digest(&canonical_archive).into();
        if mirrored != digest {
            return Err(MirrorRefusal::DigestMismatch { mirrored });
        }
        let published = fs::read_to_string(self.path(&digest, URI_SUFFIX)).map_err(|error| {
            MirrorRefusal::Unreadable {
                reason: error.to_string(),
            }
        })?;
        let published = published.trim().to_owned();
        if published != uri {
            return Err(MirrorRefusal::UriMismatch { published });
        }
        let text = fs::read_to_string(self.path(&digest, PLAN_SUFFIX)).map_err(|error| {
            MirrorRefusal::Unreadable {
                reason: error.to_string(),
            }
        })?;
        let plan = BuildPlan::parse(&text).map_err(MirrorRefusal::Plan)?;
        Ok(MirroredSource {
            source: PublishedSource {
                uri: published,
                canonical_archive,
            },
            plan,
        })
    }

    fn path(&self, digest: &[u8; 32], suffix: &str) -> PathBuf {
        self.root.join(format!("{}.{suffix}", hex::encode(digest)))
    }

    fn write(&self, digest: &[u8; 32], suffix: &str, bytes: &[u8]) -> Result<(), String> {
        let path = self.path(digest, suffix);
        write_atomic(&path, bytes)
            .map_err(|error| format!("could not persist {}: {error}", path.display()))
    }
}

//! Generated from platform/sdk/schema/mirror-v2.kvx.

pub const MIRROR_SCHEMA_VERSION: u16 = 2;
pub const MIRROR_ARCHIVE_MAGIC: [u8; 8] = *b"LXMIRROR";
pub const MIRROR_MAX_ARCHIVE_BYTES: usize = 67_108_864;
pub const MIRROR_MAX_SOURCES: usize = 8;
pub const MIRROR_MAX_JSON_DEPTH: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeneratedMirrorPolicy {
    Exact,
    OrderedPreference,
    Agreement,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeneratedMirrorError {
    Configuration,
    Unavailable,
    RateLimited,
    Missing,
    TargetMismatch,
    SourceMismatch,
    Malformed,
    Bounds,
    Commitment,
    Authorization,
    Proof,
    CheckpointUnavailable,
    Divergent,
    InsufficientAgreement,
    Reorged,
}

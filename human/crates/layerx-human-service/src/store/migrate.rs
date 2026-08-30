use std::collections::BTreeSet;
use std::fmt::{Display, Formatter};
use std::fs;
use std::io;
use std::path::Path;

use super::codec::{self, CodecError, Decoder};
use super::{PrincipalId, RowKey, PRINCIPALS_DIR, STORE_FILE, STORE_MAGIC};

const VERSION_FILE: &str = "store.version";
const VERSION_TEMP_FILE: &str = "store.version.tmp";
const MIGRATING_FILE: &str = "store.bin.migrating";
const VERSION_MAGIC: &[u8; 4] = b"LXHV";

const MIGRATION_DEFINITIONS: &[&str] = &[
    include_str!("../../migrations/0001-principal-row-layout.kvx"),
    include_str!("../../migrations/0002-audit-evidence-pinning.kvx"),
];

type Transform = fn(&[u8]) -> Result<Vec<u8>, MigrationError>;

/// Migration refusal and I/O failures.
#[derive(Debug)]
pub enum MigrationError {
    Io(io::Error),
    InvalidManifest(&'static str),
    InvalidVersionFile,
    NewerSchema { found: u32, supported: u32 },
    UnknownVersion(u32),
    CorruptStore(&'static str),
    SizeOverflow,
}

impl Display for MigrationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "store migration I/O failure: {error}"),
            Self::InvalidManifest(reason) => {
                write!(formatter, "invalid migration manifest: {reason}")
            }
            Self::InvalidVersionFile => formatter.write_str("invalid store version file"),
            Self::NewerSchema { found, supported } => write!(
                formatter,
                "store version {found} is newer than supported version {supported}"
            ),
            Self::UnknownVersion(version) => write!(
                formatter,
                "store version {version} is not defined by the migration manifest"
            ),
            Self::CorruptStore(reason) => write!(formatter, "corrupt principal store: {reason}"),
            Self::SizeOverflow => formatter.write_str("migrated store exceeds encoding bounds"),
        }
    }
}

impl std::error::Error for MigrationError {}

impl From<io::Error> for MigrationError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<CodecError> for MigrationError {
    fn from(value: CodecError) -> Self {
        match value {
            CodecError::Truncated => Self::CorruptStore("truncated bytes"),
            CodecError::Overflow => Self::SizeOverflow,
        }
    }
}

pub(crate) fn prepare(root: &Path) -> Result<u32, MigrationError> {
    let versions = declared_versions()?;
    let current = u32::try_from(versions.len()).map_err(|_| MigrationError::SizeOverflow)?;
    for from in 1..current {
        if transform_from(from).is_none() {
            return Err(MigrationError::InvalidManifest(
                "missing migration transform",
            ));
        }
    }
    match read_version_file(root)? {
        None => write_version_file(root, current)?,
        Some(found) if found == current => {}
        Some(found) if found > current => {
            return Err(MigrationError::NewerSchema {
                found,
                supported: current,
            })
        }
        Some(0) => return Err(MigrationError::UnknownVersion(0)),
        Some(found) => {
            for from in found..current {
                apply_step(root, from)?;
            }
            write_version_file(root, current)?;
        }
    }
    Ok(current)
}

fn declared_versions() -> Result<Vec<u32>, MigrationError> {
    let mut versions = Vec::new();
    for definition in MIGRATION_DEFINITIONS {
        versions.push(parse_definition(definition)?);
    }
    if versions.is_empty() {
        return Err(MigrationError::InvalidManifest("empty migration manifest"));
    }
    for (index, version) in versions.iter().enumerate() {
        let expected = u32::try_from(index)
            .ok()
            .and_then(|value| value.checked_add(1))
            .ok_or(MigrationError::SizeOverflow)?;
        if *version != expected {
            return Err(MigrationError::InvalidManifest(
                "non-contiguous migration versions",
            ));
        }
    }
    Ok(versions)
}

fn parse_definition(text: &str) -> Result<u32, MigrationError> {
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    if lines.next() != Some("[migration]") {
        return Err(MigrationError::InvalidManifest("missing migration header"));
    }
    let mut version = None;
    let mut titled = false;
    for line in lines {
        let (key, value) = line
            .split_once('=')
            .ok_or(MigrationError::InvalidManifest("malformed manifest line"))?;
        match key.trim() {
            "version" => {
                version = Some(
                    value
                        .trim()
                        .parse::<u32>()
                        .map_err(|_| MigrationError::InvalidManifest("invalid version"))?,
                );
            }
            "title" => {
                let quoted = value
                    .trim()
                    .strip_prefix('"')
                    .and_then(|tail| tail.strip_suffix('"'))
                    .ok_or(MigrationError::InvalidManifest("unquoted title"))?;
                if quoted.is_empty() {
                    return Err(MigrationError::InvalidManifest("empty title"));
                }
                titled = true;
            }
            _ => return Err(MigrationError::InvalidManifest("unknown manifest key")),
        }
    }
    if !titled {
        return Err(MigrationError::InvalidManifest("missing title"));
    }
    version.ok_or(MigrationError::InvalidManifest("missing version"))
}

fn transform_from(version: u32) -> Option<Transform> {
    match version {
        1 => Some(transform_v1_to_v2),
        _ => None,
    }
}

fn apply_step(root: &Path, from: u32) -> Result<(), MigrationError> {
    let Some(transform) = transform_from(from) else {
        return Err(MigrationError::UnknownVersion(from));
    };
    let principals = root.join(PRINCIPALS_DIR);
    if !principals.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(&principals)? {
        let entry = entry?;
        let directory = entry.path();
        let name = entry.file_name();
        let name = name
            .to_str()
            .ok_or(MigrationError::CorruptStore("foreign principals entry"))?;
        if PrincipalId::new(name).is_err() || !directory.is_dir() {
            return Err(MigrationError::CorruptStore("foreign principals entry"));
        }
        migrate_principal_file(&directory, from, transform)?;
    }
    Ok(())
}

fn migrate_principal_file(
    directory: &Path,
    from: u32,
    transform: Transform,
) -> Result<(), MigrationError> {
    let path = directory.join(STORE_FILE);
    if !path.exists() {
        return Ok(());
    }
    let bytes = fs::read(&path)?;
    let found = header_version(&bytes)?;
    let target = from.checked_add(1).ok_or(MigrationError::SizeOverflow)?;
    if found == target {
        return Ok(());
    }
    if found != from {
        return Err(MigrationError::CorruptStore(
            "unexpected principal file version",
        ));
    }
    let migrated = transform(&bytes)?;
    codec::write_atomic(directory, STORE_FILE, MIGRATING_FILE, &migrated)?;
    Ok(())
}

fn header_version(bytes: &[u8]) -> Result<u32, MigrationError> {
    let mut decoder = Decoder::new(bytes);
    if decoder.take(4)? != STORE_MAGIC {
        return Err(MigrationError::CorruptStore("invalid principal header"));
    }
    Ok(decoder.u32()?)
}

fn transform_v1_to_v2(bytes: &[u8]) -> Result<Vec<u8>, MigrationError> {
    let mut decoder = Decoder::new(bytes);
    if decoder.take(4)? != STORE_MAGIC {
        return Err(MigrationError::CorruptStore("invalid principal header"));
    }
    if decoder.u32()? != 1 {
        return Err(MigrationError::CorruptStore(
            "unexpected principal file version",
        ));
    }
    let count = decoder.length()?;
    let mut rows = Vec::new();
    let mut audits = Vec::new();
    let mut seen_rows = BTreeSet::new();
    let mut seen_audit = BTreeSet::new();
    for _ in 0..count {
        let code = decoder.byte()?;
        let key = decode_v1_key(&mut decoder)?;
        let written_at = decoder.u64()?;
        let value = decoder.bytes()?.to_vec();
        match code {
            1 | 2 | 4 => {
                if !seen_rows.insert((code, key.clone())) {
                    return Err(MigrationError::CorruptStore("duplicate row"));
                }
                rows.push((code, key, written_at, value));
            }
            3 => {
                if !seen_audit.insert(key.clone()) {
                    return Err(MigrationError::CorruptStore("duplicate audit entry"));
                }
                audits.push((key, written_at, value));
            }
            _ => return Err(MigrationError::CorruptStore("unknown v1 table")),
        }
    }
    if !decoder.is_empty() {
        return Err(MigrationError::CorruptStore("trailing bytes"));
    }
    rows.sort();
    audits.sort();
    encode_v2(&rows, &audits)
}

fn decode_v1_key(decoder: &mut Decoder<'_>) -> Result<Vec<u8>, MigrationError> {
    let raw = decoder.bytes()?;
    let text = std::str::from_utf8(raw)
        .map_err(|_| MigrationError::CorruptStore("stored key is not UTF-8"))?;
    if RowKey::new(text).is_err() {
        return Err(MigrationError::CorruptStore("invalid stored key"));
    }
    Ok(raw.to_vec())
}

fn encode_v2(
    rows: &[(u8, Vec<u8>, u64, Vec<u8>)],
    audits: &[(Vec<u8>, u64, Vec<u8>)],
) -> Result<Vec<u8>, MigrationError> {
    let mut output = Vec::new();
    output.extend_from_slice(STORE_MAGIC);
    output.extend_from_slice(&2_u32.to_be_bytes());
    codec::push_length(&mut output, rows.len())?;
    for (code, key, written_at, value) in rows {
        output.push(*code);
        codec::push_bytes(&mut output, key)?;
        output.extend_from_slice(&written_at.to_be_bytes());
        codec::push_bytes(&mut output, value)?;
    }
    codec::push_length(&mut output, audits.len())?;
    for (key, written_at, value) in audits {
        codec::push_bytes(&mut output, key)?;
        output.extend_from_slice(&written_at.to_be_bytes());
        codec::push_bytes(&mut output, value)?;
        output.push(0);
        codec::push_length(&mut output, 0)?;
    }
    Ok(output)
}

fn read_version_file(root: &Path) -> Result<Option<u32>, MigrationError> {
    let path = root.join(VERSION_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(&path)?;
    if bytes.len() != 8 || bytes.get(..4) != Some(VERSION_MAGIC.as_slice()) {
        return Err(MigrationError::InvalidVersionFile);
    }
    let mut encoded = [0_u8; 4];
    encoded.copy_from_slice(bytes.get(4..8).ok_or(MigrationError::InvalidVersionFile)?);
    Ok(Some(u32::from_be_bytes(encoded)))
}

fn write_version_file(root: &Path, version: u32) -> Result<(), MigrationError> {
    let mut bytes = Vec::with_capacity(8);
    bytes.extend_from_slice(VERSION_MAGIC);
    bytes.extend_from_slice(&version.to_be_bytes());
    codec::write_atomic(root, VERSION_FILE, VERSION_TEMP_FILE, &bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{declared_versions, parse_definition, MigrationError};

    #[test]
    fn manifest_declares_contiguous_versions_from_one() {
        let versions =
            declared_versions().unwrap_or_else(|error| panic!("manifest rejected: {error}"));
        assert_eq!(versions, vec![1, 2]);
    }

    #[test]
    fn malformed_definitions_are_refused() {
        let cases = [
            "nonsense",
            "[migration]\nversion = 1",
            "[migration]\ntitle = \"only a title\"",
            "[migration]\nversion = one\ntitle = \"x\"",
            "[migration]\nversion = 1\ntitle = unquoted",
            "[migration]\nversion = 1\ntitle = \"\"",
            "[migration]\nversion = 1\ntitle = \"x\"\nextra = \"y\"",
        ];
        for case in cases {
            assert!(
                matches!(
                    parse_definition(case),
                    Err(MigrationError::InvalidManifest(_))
                ),
                "accepted malformed definition {case:?}"
            );
        }
    }
}

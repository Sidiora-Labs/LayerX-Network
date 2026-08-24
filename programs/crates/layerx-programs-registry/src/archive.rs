//! Canonical published-source archive shared by publishers and verifiers.

use core::fmt::{self, Display};
use std::collections::BTreeMap;

use crate::hash::sha256;

const MAGIC: &[u8; 8] = b"LXSRCv1\0";
const HEADER_BYTES: usize = 12;
const MAX_FILES: usize = 8_192;
const MAX_PATH_BYTES: usize = 256;
const MAX_FILE_BYTES: usize = 8 * 1024 * 1024;
const MAX_ARCHIVE_BYTES: usize = 64 * 1024 * 1024;

/// One published source file carried by the canonical archive.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceFile {
    pub path: String,
    pub executable: bool,
    pub content: Vec<u8>,
}

/// Deterministic archive of published program source. Ordering, framing and
/// permissions are canonical so that the same tree always encodes to the same
/// bytes and therefore to the same source digest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceArchive {
    files: Vec<SourceFile>,
}

impl SourceArchive {
    /// Orders and validates published files into one canonical archive.
    ///
    /// # Errors
    ///
    /// Refuses empty archives, unsafe paths, duplicate paths and inputs beyond
    /// the declared bounds.
    pub fn new(files: Vec<SourceFile>) -> Result<Self, ArchiveError> {
        if files.is_empty() {
            return Err(ArchiveError::Empty);
        }
        if files.len() > MAX_FILES {
            return Err(ArchiveError::TooLarge);
        }
        let mut total = 0_usize;
        let mut ordered = BTreeMap::new();
        for file in files {
            validate_path(&file.path)?;
            if file.content.len() > MAX_FILE_BYTES {
                return Err(ArchiveError::TooLarge);
            }
            total = total.saturating_add(file.content.len());
            if ordered.insert(file.path.clone(), file).is_some() {
                return Err(ArchiveError::DuplicatePath);
            }
        }
        if total > MAX_ARCHIVE_BYTES {
            return Err(ArchiveError::TooLarge);
        }
        Ok(Self {
            files: ordered.into_values().collect(),
        })
    }

    /// Borrows the canonically ordered files.
    #[must_use]
    pub fn files(&self) -> &[SourceFile] {
        &self.files
    }

    /// Returns the file published at an exact archive path.
    #[must_use]
    pub fn file(&self, path: &str) -> Option<&SourceFile> {
        self.files.iter().find(|file| file.path == path)
    }

    /// Encodes the canonical archive bytes.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(
            &(u32::try_from(self.files.len()).unwrap_or(u32::MAX)).to_be_bytes(),
        );
        for file in &self.files {
            bytes.extend_from_slice(
                &(u16::try_from(file.path.len()).unwrap_or(u16::MAX)).to_be_bytes(),
            );
            bytes.extend_from_slice(file.path.as_bytes());
            bytes.push(u8::from(file.executable));
            bytes.extend_from_slice(
                &(u32::try_from(file.content.len()).unwrap_or(u32::MAX)).to_be_bytes(),
            );
            bytes.extend_from_slice(&file.content);
        }
        bytes
    }

    /// Decodes canonical archive bytes.
    ///
    /// # Errors
    ///
    /// Refuses a foreign magic, truncated framing, non-canonical ordering,
    /// unsafe paths, unknown permission modes and trailing bytes.
    pub fn decode(bytes: &[u8]) -> Result<Self, ArchiveError> {
        if bytes.len() > MAX_ARCHIVE_BYTES {
            return Err(ArchiveError::TooLarge);
        }
        if bytes.len() < HEADER_BYTES || bytes.get(..MAGIC.len()) != Some(MAGIC.as_slice()) {
            return Err(ArchiveError::Magic);
        }
        let mut cursor = MAGIC.len();
        let count = usize::try_from(u32::from_be_bytes(take_array::<4>(bytes, &mut cursor)?))
            .map_err(|_| ArchiveError::Encoding)?;
        if count == 0 {
            return Err(ArchiveError::Empty);
        }
        if count > MAX_FILES {
            return Err(ArchiveError::TooLarge);
        }
        let mut files: Vec<SourceFile> = Vec::with_capacity(count);
        for _ in 0..count {
            let path_bytes = usize::from(u16::from_be_bytes(take_array::<2>(bytes, &mut cursor)?));
            let path = core::str::from_utf8(take_slice(bytes, &mut cursor, path_bytes)?)
                .map_err(|_| ArchiveError::Path)?
                .to_owned();
            validate_path(&path)?;
            let executable = match take_array::<1>(bytes, &mut cursor)? {
                [0] => false,
                [1] => true,
                _ => return Err(ArchiveError::Encoding),
            };
            let content_bytes =
                usize::try_from(u32::from_be_bytes(take_array::<4>(bytes, &mut cursor)?))
                    .map_err(|_| ArchiveError::Encoding)?;
            if content_bytes > MAX_FILE_BYTES {
                return Err(ArchiveError::TooLarge);
            }
            let content = take_slice(bytes, &mut cursor, content_bytes)?.to_vec();
            if files.last().is_some_and(|earlier| earlier.path >= path) {
                return Err(ArchiveError::Order);
            }
            files.push(SourceFile {
                path,
                executable,
                content,
            });
        }
        if cursor != bytes.len() {
            return Err(ArchiveError::Encoding);
        }
        Ok(Self { files })
    }

    /// Returns the canonical source digest published beside the program.
    #[must_use]
    pub fn digest(&self) -> [u8; 32] {
        sha256(&self.encode())
    }
}

/// Typed refusal produced while ordering or decoding published source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArchiveError {
    Magic,
    Encoding,
    Empty,
    TooLarge,
    Path,
    Order,
    DuplicatePath,
}

impl Display for ArchiveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Magic => "published source is not a canonical archive",
            Self::Encoding => "canonical source archive framing is invalid",
            Self::Empty => "canonical source archive is empty",
            Self::TooLarge => "canonical source archive exceeds the declared bounds",
            Self::Path => "canonical source archive path is unsafe",
            Self::Order => "canonical source archive is not in canonical order",
            Self::DuplicatePath => "canonical source archive repeats a path",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for ArchiveError {}

pub(crate) fn validate_path(path: &str) -> Result<(), ArchiveError> {
    if path.is_empty()
        || path.len() > MAX_PATH_BYTES
        || path.starts_with('/')
        || path.ends_with('/')
        || path
            .bytes()
            .any(|byte| byte == b'\\' || !byte.is_ascii_graphic())
    {
        return Err(ArchiveError::Path);
    }
    for component in path.split('/') {
        if component.is_empty() || component == "." || component == ".." {
            return Err(ArchiveError::Path);
        }
    }
    Ok(())
}

fn take_array<const N: usize>(bytes: &[u8], cursor: &mut usize) -> Result<[u8; N], ArchiveError> {
    take_slice(bytes, cursor, N)?
        .try_into()
        .map_err(|_| ArchiveError::Encoding)
}

fn take_slice<'bytes>(
    bytes: &'bytes [u8],
    cursor: &mut usize,
    length: usize,
) -> Result<&'bytes [u8], ArchiveError> {
    let end = cursor.checked_add(length).ok_or(ArchiveError::Encoding)?;
    let slice = bytes.get(*cursor..end).ok_or(ArchiveError::Encoding)?;
    *cursor = end;
    Ok(slice)
}

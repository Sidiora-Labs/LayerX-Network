//! Adapter descriptors for the interoperability gateway. An adapter is an
//! edge translator only: its descriptor grants no protocol authority, and it
//! cannot be declared without pinning a versioned upstream specification and
//! the conformance suite that proves the pin, so an upstream version bump is
//! an explicit adapter change rather than silent behavioural drift.

use std::fmt::{Display, Formatter};

use sha2::{Digest as _, Sha256};

const IDENTIFIER_LIMIT: usize = 64;
const VERSION_LIMIT: usize = 32;

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= IDENTIFIER_LIMIT
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
        })
}

/// A validated adapter identifier limited to `a-z`, `0-9`, `-` and `_`.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AdapterId(String);

impl AdapterId {
    /// Creates a bounded adapter identifier.
    ///
    /// # Errors
    ///
    /// Rejects empty, oversize and out-of-charset identifiers.
    pub fn new(value: impl Into<String>) -> Result<Self, AdapterError> {
        let value = value.into();
        if valid_identifier(&value) {
            Ok(Self(value))
        } else {
            Err(AdapterError::InvalidIdentifier)
        }
    }

    /// Returns the identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An exact upstream specification version: dot-separated numeric components
/// with no ranges, wildcards or comparators, so every adapter is pinned to
/// one published upstream revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpecVersion(String);

impl SpecVersion {
    /// Parses an exact pinned version such as `2` or `2.0.1`.
    ///
    /// # Errors
    ///
    /// Refuses empty values, oversize values, empty components, and any
    /// character outside ASCII digits and the component separator.
    pub fn parse(value: &str) -> Result<Self, AdapterError> {
        if value.is_empty() || value.len() > VERSION_LIMIT {
            return Err(AdapterError::UnpinnedVersion);
        }
        for component in value.split('.') {
            if component.is_empty() || !component.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(AdapterError::UnpinnedVersion);
            }
        }
        Ok(Self(value.to_owned()))
    }

    /// Returns the pinned version.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for SpecVersion {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// One versioned upstream specification pin: the protocol name, the exact
/// published version, and the content digest of the pinned specification
/// document.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PinnedSpec {
    protocol: AdapterId,
    version: SpecVersion,
    document_digest: [u8; 32],
}

impl PinnedSpec {
    /// Pins one upstream specification revision.
    ///
    /// # Errors
    ///
    /// Refuses the zero document digest: a pin must name exact content.
    pub fn new(
        protocol: AdapterId,
        version: SpecVersion,
        document_digest: [u8; 32],
    ) -> Result<Self, AdapterError> {
        if document_digest == [0; 32] {
            return Err(AdapterError::UnpinnedDocument);
        }
        Ok(Self {
            protocol,
            version,
            document_digest,
        })
    }

    /// Returns the upstream protocol name.
    #[must_use]
    pub const fn protocol(&self) -> &AdapterId {
        &self.protocol
    }

    /// Returns the exact pinned version.
    #[must_use]
    pub const fn version(&self) -> &SpecVersion {
        &self.version
    }

    /// Returns the content digest of the pinned specification document.
    #[must_use]
    pub const fn document_digest(&self) -> [u8; 32] {
        self.document_digest
    }

    /// Verifies exact specification document bytes against this pin.
    ///
    /// # Errors
    ///
    /// Refuses empty documents and any document whose SHA-256 digest differs
    /// from the pinned content digest.
    pub fn verify_document(&self, document: &[u8]) -> Result<(), AdapterError> {
        if document.is_empty() {
            return Err(AdapterError::DocumentDigestMismatch);
        }
        let digest: [u8; 32] = Sha256::digest(document).into();
        if digest != self.document_digest {
            return Err(AdapterError::DocumentDigestMismatch);
        }
        Ok(())
    }
}

/// The conformance suite an adapter declares at registration: its identifier,
/// the number of vectors it carries, and the content digest of the suite, all
/// bound to the pinned specification revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConformanceSuite {
    suite: AdapterId,
    vector_count: u64,
    suite_digest: [u8; 32],
}

impl ConformanceSuite {
    /// Declares one conformance suite.
    ///
    /// # Errors
    ///
    /// Refuses an empty suite and the zero suite digest: a declared suite
    /// must carry real vectors with pinned content.
    pub fn new(
        suite: AdapterId,
        vector_count: u64,
        suite_digest: [u8; 32],
    ) -> Result<Self, AdapterError> {
        if vector_count == 0 {
            return Err(AdapterError::EmptyConformanceSuite);
        }
        if suite_digest == [0; 32] {
            return Err(AdapterError::UnpinnedConformanceSuite);
        }
        Ok(Self {
            suite,
            vector_count,
            suite_digest,
        })
    }

    /// Returns the suite identifier.
    #[must_use]
    pub const fn suite(&self) -> &AdapterId {
        &self.suite
    }

    /// Returns the number of vectors the suite carries.
    #[must_use]
    pub const fn vector_count(&self) -> u64 {
        self.vector_count
    }

    /// Returns the content digest of the suite.
    #[must_use]
    pub const fn suite_digest(&self) -> [u8; 32] {
        self.suite_digest
    }
}

/// One complete adapter declaration. The type makes an unpinned or
/// suite-less adapter unrepresentable, and holds no capability: a descriptor
/// conveys facts about the adapter, never protocol authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdapterDescriptor {
    id: AdapterId,
    spec: PinnedSpec,
    conformance: ConformanceSuite,
}

impl AdapterDescriptor {
    /// Declares one adapter with its specification pin and conformance suite.
    #[must_use]
    pub const fn new(id: AdapterId, spec: PinnedSpec, conformance: ConformanceSuite) -> Self {
        Self {
            id,
            spec,
            conformance,
        }
    }

    /// Returns the adapter identifier.
    #[must_use]
    pub const fn id(&self) -> &AdapterId {
        &self.id
    }

    /// Returns the pinned upstream specification.
    #[must_use]
    pub const fn spec(&self) -> &PinnedSpec {
        &self.spec
    }

    /// Returns the declared conformance suite.
    #[must_use]
    pub const fn conformance(&self) -> &ConformanceSuite {
        &self.conformance
    }
}

/// Adapter declaration failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdapterError {
    InvalidIdentifier,
    UnpinnedVersion,
    UnpinnedDocument,
    DocumentDigestMismatch,
    EmptyConformanceSuite,
    UnpinnedConformanceSuite,
}

impl Display for AdapterError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidIdentifier => formatter.write_str("invalid adapter identifier"),
            Self::UnpinnedVersion => {
                formatter.write_str("adapter version must pin one exact upstream revision")
            }
            Self::UnpinnedDocument => {
                formatter.write_str("adapter must pin the upstream specification content")
            }
            Self::DocumentDigestMismatch => {
                formatter.write_str("specification document does not match the pinned digest")
            }
            Self::EmptyConformanceSuite => {
                formatter.write_str("adapter conformance suite declares no vectors")
            }
            Self::UnpinnedConformanceSuite => {
                formatter.write_str("adapter conformance suite must pin its content")
            }
        }
    }
}

impl std::error::Error for AdapterError {}

#[cfg(test)]
mod tests {
    use super::{AdapterError, AdapterId, ConformanceSuite, PinnedSpec, SpecVersion};

    fn identifier(name: &str) -> AdapterId {
        AdapterId::new(name).unwrap_or_else(|error| panic!("identifier {name}: {error}"))
    }

    #[test]
    fn only_exact_upstream_versions_are_accepted() {
        for pinned in ["2", "2.0", "2.0.1", "10.42.7"] {
            SpecVersion::parse(pinned)
                .unwrap_or_else(|error| panic!("pinned version {pinned} refused: {error}"));
        }
        for unpinned in ["", "*", "2.x", "^2.0", "~2.0.1", ">=2", "2.", ".2", "2..0", "latest"] {
            assert_eq!(
                SpecVersion::parse(unpinned),
                Err(AdapterError::UnpinnedVersion),
                "{unpinned} must be refused"
            );
        }
    }

    #[test]
    fn pinned_documents_verify_by_content_and_refuse_altered_bytes() {
        use sha2::{Digest as _, Sha256};
        let document = b"upstream specification bytes";
        let digest: [u8; 32] = Sha256::digest(document).into();
        let version = SpecVersion::parse("2").unwrap_or_else(|error| panic!("version: {error}"));
        let pin = PinnedSpec::new(identifier("x402"), version, digest)
            .unwrap_or_else(|error| panic!("pin: {error}"));
        assert!(pin.verify_document(document).is_ok());
        assert_eq!(
            pin.verify_document(b"upstream specification bytes altered"),
            Err(AdapterError::DocumentDigestMismatch)
        );
        assert_eq!(
            pin.verify_document(&[]),
            Err(AdapterError::DocumentDigestMismatch)
        );
    }

    #[test]
    fn pins_and_suites_without_content_are_refused() {
        let version = SpecVersion::parse("2")
            .unwrap_or_else(|error| panic!("version: {error}"));
        assert_eq!(
            PinnedSpec::new(identifier("x402"), version, [0; 32]),
            Err(AdapterError::UnpinnedDocument)
        );
        assert_eq!(
            ConformanceSuite::new(identifier("x402-v2"), 0, [9; 32]),
            Err(AdapterError::EmptyConformanceSuite)
        );
        assert_eq!(
            ConformanceSuite::new(identifier("x402-v2"), 128, [0; 32]),
            Err(AdapterError::UnpinnedConformanceSuite)
        );
    }
}

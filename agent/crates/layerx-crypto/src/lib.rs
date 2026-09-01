//! Key custody and disclosure-bound `LayerX` signing.

#[cfg(feature = "custody")]
pub mod ct;
#[cfg(feature = "custody")]
pub mod disclosure;
pub mod ed25519;
#[cfg(feature = "custody")]
pub mod keystore;
#[cfg(feature = "custody")]
pub mod local;
#[cfg(feature = "custody")]
pub mod redact;
#[cfg(feature = "custody")]
pub mod remote;
pub mod secp256k1;
#[cfg(feature = "custody")]
pub mod session;
#[cfg(feature = "custody")]
pub mod signer;

use layerx_types::result::{KnownResult, ResultCode};
use layerx_wire::hash::Domain;
use sha2::{Digest as _, Sha256};

/// Canonical bytes coupled to the protocol and network they are valid on.
#[derive(Clone, Copy)]
pub struct SignatureMessage<'a> {
    domain: Domain,
    protocol_version: u16,
    network_id: u32,
    canonical: &'a [u8],
}

impl<'a> SignatureMessage<'a> {
    /// Binds canonical bytes to one supported protocol version and non-zero
    /// network identifier.
    ///
    /// # Errors
    ///
    /// Rejects unsupported versions and the reserved zero network.
    pub const fn new(
        domain: Domain,
        protocol_version: u16,
        network_id: u32,
        canonical: &'a [u8],
    ) -> Result<Self, VerifyError> {
        if !matches!(protocol_version, 1 | 2) {
            return Err(VerifyError::VersionUnsupported);
        }
        if network_id == 0 {
            return Err(VerifyError::WrongNetwork);
        }
        Ok(Self {
            domain,
            protocol_version,
            network_id,
            canonical,
        })
    }

    /// Computes the exact core-compatible scoped domain digest.
    #[must_use]
    pub fn digest(self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(self.domain.tag());
        hasher.update(self.canonical);
        hasher.finalize().into()
    }

    pub(crate) const fn protocol_version(self) -> u16 {
        self.protocol_version
    }

    pub(crate) const fn network_id(self) -> u32 {
        self.network_id
    }

    pub(crate) const fn canonical(self) -> &'a [u8] {
        self.canonical
    }
}

impl std::fmt::Debug for SignatureMessage<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SignatureMessage")
            .field("domain", &self.domain)
            .field("protocol_version", &self.protocol_version)
            .field("network_id", &self.network_id)
            .field("canonical_length", &self.canonical.len())
            .finish()
    }
}

/// Exact protocol classification for signature verification failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerifyError {
    /// The protocol version is not supported by this build.
    VersionUnsupported,
    /// Network zero is reserved and never a valid signing scope.
    WrongNetwork,
    /// The key, signature encoding, scalar form, or verification equation failed.
    BadSignature,
}

impl VerifyError {
    /// Returns the core result code corresponding to this failure.
    #[must_use]
    pub const fn result_code(self) -> ResultCode {
        match self {
            Self::VersionUnsupported => ResultCode::from_raw(KnownResult::VersionUnsupported.raw()),
            Self::WrongNetwork => ResultCode::from_raw(KnownResult::WrongNetwork.raw()),
            Self::BadSignature => ResultCode::from_raw(KnownResult::BadSignature.raw()),
        }
    }
}

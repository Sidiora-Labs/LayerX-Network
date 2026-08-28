//! Closed storage namespace identities fixed by the host before guest entry.

use super::{PrincipalId, ProgramId};
use core::cmp::Ordering;

const PRINCIPAL_SCOPED_TAG: u8 = 0;
const PROGRAM_SHARED_TAG: u8 = 1;
const PROTOCOL_PRIVATE_TAG: u8 = 2;

/// A durable namespace owned by exactly one program.
///
/// Ordering is protocol-significant and is implemented explicitly as owning
/// program, frozen scope tag, then principal when present. Guests never
/// construct this value; the runtime fixes both variants from the executing
/// frame before guest entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageNamespace {
    /// State isolated by both executing program and invoking principal.
    PrincipalScoped {
        program: ProgramId,
        principal: PrincipalId,
    },
    /// State shared by every principal invoking the owning program.
    ProgramShared { program: ProgramId },
    ProtocolPrivate { program: ProgramId, scope: [u8; 32] },
}

impl StorageNamespace {
    /// Fixes a principal-scoped namespace for one executing frame.
    #[must_use]
    pub const fn principal(program: ProgramId, principal: PrincipalId) -> Self {
        Self::PrincipalScoped { program, principal }
    }

    /// Fixes the program-shared namespace for one executing frame.
    #[must_use]
    pub const fn shared(program: ProgramId) -> Self {
        Self::ProgramShared { program }
    }

    #[must_use]
    pub const fn protocol_private(program: ProgramId, scope: [u8; 32]) -> Self {
        Self::ProtocolPrivate { program, scope }
    }

    /// Returns the program that exclusively owns this namespace.
    #[must_use]
    pub const fn program(self) -> ProgramId {
        match self {
            Self::PrincipalScoped { program, .. } | Self::ProgramShared { program }
            | Self::ProtocolPrivate { program, .. } => program,
        }
    }

    /// Returns the principal scope, or `None` for program-shared state.
    #[must_use]
    pub const fn principal_scope(self) -> Option<PrincipalId> {
        match self {
            Self::PrincipalScoped { principal, .. } => Some(principal),
            Self::ProgramShared { .. } => None,
            Self::ProtocolPrivate { .. } => None,
        }
    }

    /// Returns the frozen canonical namespace bytes used by ordered storage
    /// consumers: program, scope tag, then principal for principal scope.
    #[must_use]
    pub fn canonical_bytes(self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(65);
        bytes.extend_from_slice(&self.program().bytes());
        match self {
            Self::PrincipalScoped { principal, .. } => {
                bytes.push(PRINCIPAL_SCOPED_TAG);
                bytes.extend_from_slice(&principal.bytes());
            }
            Self::ProgramShared { .. } => bytes.push(PROGRAM_SHARED_TAG),
            Self::ProtocolPrivate { scope, .. } => {
                bytes.push(PROTOCOL_PRIVATE_TAG);
                bytes.extend_from_slice(&scope);
            }
        }
        bytes
    }

    pub(crate) fn write_canonical(self, output: &mut [u8; 65]) -> usize {
        output[..32].copy_from_slice(&self.program().bytes());
        match self {
            Self::PrincipalScoped { principal, .. } => {
                output[32] = PRINCIPAL_SCOPED_TAG;
                output[33..65].copy_from_slice(&principal.bytes());
                65
            }
            Self::ProgramShared { .. } => {
                output[32] = PROGRAM_SHARED_TAG;
                33
            }
            Self::ProtocolPrivate { scope, .. } => {
                output[32] = PROTOCOL_PRIVATE_TAG;
                output[33..65].copy_from_slice(&scope);
                65
            }
        }
    }
}

impl Ord for StorageNamespace {
    fn cmp(&self, other: &Self) -> Ordering {
        self.program()
            .cmp(&other.program())
            .then_with(|| match (*self, *other) {
                (
                    Self::PrincipalScoped {
                        principal: left, ..
                    },
                    Self::PrincipalScoped {
                        principal: right, ..
                    },
                ) => left.cmp(&right),
                (Self::PrincipalScoped { .. }, Self::ProgramShared { .. }) => Ordering::Less,
                (Self::ProgramShared { .. }, Self::PrincipalScoped { .. }) => Ordering::Greater,
                (Self::ProgramShared { .. }, Self::ProgramShared { .. }) => Ordering::Equal,
                (Self::ProtocolPrivate { scope: left, .. }, Self::ProtocolPrivate { scope: right, .. }) => left.cmp(&right),
                (Self::ProtocolPrivate { .. }, _) => Ordering::Greater,
                (_, Self::ProtocolPrivate { .. }) => Ordering::Less,
            })
    }
}

impl PartialOrd for StorageNamespace {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

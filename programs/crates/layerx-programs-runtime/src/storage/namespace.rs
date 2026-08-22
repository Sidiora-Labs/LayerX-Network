//! Closed storage namespace identities fixed by the host before guest entry.

use super::{PrincipalId, ProgramId};
use core::cmp::Ordering;

const PRINCIPAL_SCOPED_TAG: u8 = 0;
const PROGRAM_SHARED_TAG: u8 = 1;

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

    /// Returns the program that exclusively owns this namespace.
    #[must_use]
    pub const fn program(self) -> ProgramId {
        match self {
            Self::PrincipalScoped { program, .. } | Self::ProgramShared { program } => program,
        }
    }

    /// Returns the principal scope, or `None` for program-shared state.
    #[must_use]
    pub const fn principal_scope(self) -> Option<PrincipalId> {
        match self {
            Self::PrincipalScoped { principal, .. } => Some(principal),
            Self::ProgramShared { .. } => None,
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
        }
        bytes
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
            })
    }
}

impl PartialOrd for StorageNamespace {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

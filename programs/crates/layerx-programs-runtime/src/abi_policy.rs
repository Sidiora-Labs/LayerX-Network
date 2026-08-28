//! Frozen ABI transition policy shared by consensus-derived projections.

use core::fmt::{self, Display};

use crate::{ABI_V1_VERSION, ABI_V2_VERSION};

/// The sole typed refusal for an invalid ABI version transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AbiVersionRefusal {
    Unsupported { requested: u16 },
    Downgrade { current: u16, requested: u16 },
}

impl Display for AbiVersionRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported { requested } => {
                write!(formatter, "unsupported program ABI version {requested}")
            }
            Self::Downgrade { current, requested } => write!(
                formatter,
                "program ABI version {requested} downgrades current version {current}"
            ),
        }
    }
}

impl std::error::Error for AbiVersionRefusal {}

/// Accepts an ABI version for a new deployment or historical replay.
pub const fn admit_abi_version(requested: u16) -> Result<(), AbiVersionRefusal> {
    match requested {
        ABI_V1_VERSION | ABI_V2_VERSION => Ok(()),
        _ => Err(AbiVersionRefusal::Unsupported { requested }),
    }
}

/// Freezes upgrades as monotonic transitions across supported ABI versions.
pub const fn admit_abi_upgrade(
    current: u16,
    requested: u16,
) -> Result<(), AbiVersionRefusal> {
    match (admit_abi_version(current), admit_abi_version(requested)) {
        (Err(refusal), _) | (_, Err(refusal)) => Err(refusal),
        (Ok(()), Ok(())) if requested < current => {
            Err(AbiVersionRefusal::Downgrade { current, requested })
        }
        (Ok(()), Ok(())) => Ok(()),
    }
}

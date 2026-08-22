//! Capability grants, canonical encoding, and downward-only narrowing.

use std::collections::BTreeMap;

use crate::storage::ProgramId;

use super::{AbiError, MAX_CAPABILITIES};

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum CapabilityKey {
    StorageRead,
    StorageWrite,
    EmitEvent,
    Call(ProgramId),
    Transfer { asset: [u8; 32], to: [u8; 32] },
    ReceiptRead([u8; 32]),
    SharedStorageRead,
    SharedStorageWrite,
}

/// One explicit authority granted by the invoking activity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Capability {
    StorageRead,
    StorageWrite,
    SharedStorageRead,
    SharedStorageWrite,
    EmitEvent,
    Call {
        program: ProgramId,
    },
    Transfer402 {
        asset: [u8; 32],
        to: [u8; 32],
        maximum_amount: u128,
    },
    ReceiptRead {
        receipt_digest: [u8; 32],
    },
}

impl Capability {
    fn key(&self) -> CapabilityKey {
        match self {
            Self::StorageRead => CapabilityKey::StorageRead,
            Self::StorageWrite => CapabilityKey::StorageWrite,
            Self::SharedStorageRead => CapabilityKey::SharedStorageRead,
            Self::SharedStorageWrite => CapabilityKey::SharedStorageWrite,
            Self::EmitEvent => CapabilityKey::EmitEvent,
            Self::Call { program } => CapabilityKey::Call(*program),
            Self::Transfer402 { asset, to, .. } => CapabilityKey::Transfer {
                asset: *asset,
                to: *to,
            },
            Self::ReceiptRead { receipt_digest } => CapabilityKey::ReceiptRead(*receipt_digest),
        }
    }

    fn valid(&self) -> bool {
        match self {
            Self::Transfer402 {
                asset,
                to,
                maximum_amount,
            } => asset != &[0; 32] && to != &[0; 32] && *maximum_amount != 0,
            Self::ReceiptRead { receipt_digest } => receipt_digest != &[0; 32],
            Self::StorageRead
            | Self::StorageWrite
            | Self::SharedStorageRead
            | Self::SharedStorageWrite
            | Self::EmitEvent
            | Self::Call { .. } => true,
        }
    }
}

/// Closed set of explicit capabilities. Duplicate authority keys are refused,
/// preventing ambiguous limits.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CapabilitySet(BTreeMap<CapabilityKey, Capability>);

impl CapabilitySet {
    /// Constructs a validated capability set.
    ///
    /// # Errors
    ///
    /// Refuses invalid or duplicate grants.
    pub fn new(grants: impl IntoIterator<Item = Capability>) -> Result<Self, AbiError> {
        let mut capabilities = BTreeMap::new();
        for grant in grants {
            if capabilities.len() == MAX_CAPABILITIES {
                return Err(AbiError::InvalidCapability);
            }
            if !grant.valid() {
                return Err(AbiError::InvalidCapability);
            }
            if capabilities.insert(grant.key(), grant).is_some() {
                return Err(AbiError::DuplicateCapability);
            }
        }
        Ok(Self(capabilities))
    }

    /// Encodes this set into the frozen deterministic capability-list format
    /// consumed by `program_call`.
    #[must_use]
    pub fn canonical_encoding(&self) -> Vec<u8> {
        let count = u16::try_from(self.0.len()).unwrap_or(u16::MAX);
        let mut encoded = Vec::with_capacity(2 + self.0.len().saturating_mul(81));
        encoded.extend_from_slice(&count.to_be_bytes());
        for capability in self.0.values() {
            match capability {
                Capability::StorageRead => encoded.push(1),
                Capability::StorageWrite => encoded.push(2),
                Capability::SharedStorageRead => encoded.push(7),
                Capability::SharedStorageWrite => encoded.push(8),
                Capability::EmitEvent => encoded.push(3),
                Capability::Call { program } => {
                    encoded.push(4);
                    encoded.extend_from_slice(&program.bytes());
                }
                Capability::Transfer402 {
                    asset,
                    to,
                    maximum_amount,
                } => {
                    encoded.push(5);
                    encoded.extend_from_slice(asset);
                    encoded.extend_from_slice(to);
                    encoded.extend_from_slice(&maximum_amount.to_be_bytes());
                }
                Capability::ReceiptRead { receipt_digest } => {
                    encoded.push(6);
                    encoded.extend_from_slice(receipt_digest);
                }
            }
        }
        encoded
    }

    /// Returns an empty ambient-authority-free set.
    #[must_use]
    pub const fn empty() -> Self {
        Self(BTreeMap::new())
    }

    /// Narrows this authority to an explicitly requested subset.
    ///
    /// # Errors
    ///
    /// Refuses every missing grant or increased transfer limit.
    pub fn narrow(
        &self,
        requested: impl IntoIterator<Item = Capability>,
    ) -> Result<Self, AbiError> {
        let narrowed = Self::new(requested)?;
        for (key, request) in &narrowed.0 {
            let parent = self.0.get(key).ok_or(AbiError::CapabilityDenied)?;
            if let (
                Capability::Transfer402 {
                    maximum_amount: requested,
                    ..
                },
                Capability::Transfer402 {
                    maximum_amount: granted,
                    ..
                },
            ) = (request, parent)
            {
                if requested > granted {
                    return Err(AbiError::CapabilityEscalation);
                }
            }
        }
        Ok(narrowed)
    }

    pub(super) fn grant(&self, key: &CapabilityKey) -> Result<&Capability, AbiError> {
        self.0.get(key).ok_or(AbiError::CapabilityDenied)
    }

    pub(crate) fn permits_transfer(&self, asset: [u8; 32], to: [u8; 32], amount: u128) -> bool {
        matches!(
            self.0.get(&CapabilityKey::Transfer { asset, to }),
            Some(Capability::Transfer402 { maximum_amount, .. }) if amount <= *maximum_amount
        )
    }

    /// Returns whether every grant in `requested` is a non-escalating subset
    /// of this exact frame's authority.
    pub(crate) fn contains_narrowed(&self, requested: &Self) -> bool {
        requested
            .0
            .iter()
            .all(|(key, request)| match (self.0.get(key), request) {
                (
                    Some(Capability::Transfer402 {
                        maximum_amount: parent,
                        ..
                    }),
                    Capability::Transfer402 {
                        maximum_amount: child,
                        ..
                    },
                ) => child <= parent,
                (Some(_), _) => true,
                (None, _) => false,
            })
    }

    pub(crate) fn decode_canonical(bytes: &[u8]) -> Result<Vec<Capability>, AbiError> {
        if bytes.len() < 2 {
            return Err(AbiError::InvalidEncoding);
        }
        let count = usize::from(u16::from_be_bytes([bytes[0], bytes[1]]));
        let mut cursor = 2usize;
        let mut grants = Vec::with_capacity(count);
        for _ in 0..count {
            let tag = *bytes.get(cursor).ok_or(AbiError::InvalidEncoding)?;
            cursor = cursor.checked_add(1).ok_or(AbiError::InvalidEncoding)?;
            let grant = match tag {
                1 => Capability::StorageRead,
                2 => Capability::StorageWrite,
                7 => Capability::SharedStorageRead,
                8 => Capability::SharedStorageWrite,
                3 => Capability::EmitEvent,
                4 => Capability::Call {
                    program: ProgramId::new(take_array::<32>(bytes, &mut cursor)?)?,
                },
                5 => Capability::Transfer402 {
                    asset: take_array::<32>(bytes, &mut cursor)?,
                    to: take_array::<32>(bytes, &mut cursor)?,
                    maximum_amount: u128::from_be_bytes(take_array::<16>(bytes, &mut cursor)?),
                },
                6 => Capability::ReceiptRead {
                    receipt_digest: take_array::<32>(bytes, &mut cursor)?,
                },
                _ => return Err(AbiError::InvalidEncoding),
            };
            grants.push(grant);
        }
        if cursor != bytes.len() {
            return Err(AbiError::InvalidEncoding);
        }
        let canonical = Self::new(grants.clone())?.canonical_encoding();
        if canonical != bytes {
            return Err(AbiError::InvalidEncoding);
        }
        Ok(grants)
    }

    pub(super) fn receipt_digests(&self) -> impl Iterator<Item = [u8; 32]> + '_ {
        self.0.values().filter_map(|capability| match capability {
            Capability::ReceiptRead { receipt_digest } => Some(*receipt_digest),
            _ => None,
        })
    }
}

fn take_array<const N: usize>(bytes: &[u8], cursor: &mut usize) -> Result<[u8; N], AbiError> {
    let end = cursor.checked_add(N).ok_or(AbiError::InvalidEncoding)?;
    let slice = bytes.get(*cursor..end).ok_or(AbiError::InvalidEncoding)?;
    let mut output = [0u8; N];
    output.copy_from_slice(slice);
    *cursor = end;
    Ok(output)
}

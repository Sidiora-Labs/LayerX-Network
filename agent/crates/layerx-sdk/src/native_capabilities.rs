use sha2::{Digest as _, Sha256};

pub const MAX_NATIVE_CAPABILITIES: usize = 238;
pub const MAX_NATIVE_BALANCE_VIEWS: usize = 32;
pub const MAX_NATIVE_CAPABILITY_BYTES: usize = 65_452;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativeCapability {
    StorageRead,
    StorageWrite,
    EmitEvent,
    Call {
        program: [u8; 32],
    },
    Transfer402 {
        asset: [u8; 32],
        to: [u8; 32],
        maximum_amount: u128,
    },
    ProgramSpend {
        owner_program: [u8; 32],
        seed: Vec<u8>,
        source_account: [u8; 32],
        asset: [u8; 32],
        to: [u8; 32],
        maximum_amount: u128,
    },
    ReceiptRead {
        receipt_digest: [u8; 32],
    },
    BalanceView {
        account: [u8; 32],
        asset: [u8; 32],
        receipt_digest: [u8; 32],
    },
    SharedStorageRead,
    SharedStorageWrite,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeCapabilityError {
    Bounds,
    InvalidGrant,
    Duplicate,
    NonCanonical,
    Widening,
}

impl NativeCapability {
    fn key(&self) -> (u8, Vec<Vec<u8>>) {
        match self {
            Self::StorageRead => (0, vec![]),
            Self::StorageWrite => (1, vec![]),
            Self::EmitEvent => (2, vec![]),
            Self::Call { program } => (3, vec![program.to_vec()]),
            Self::Transfer402 { asset, to, .. } => (4, vec![asset.to_vec(), to.to_vec()]),
            Self::ProgramSpend {
                owner_program,
                seed,
                source_account,
                asset,
                to,
                ..
            } => (
                5,
                vec![
                    owner_program.to_vec(),
                    seed.clone(),
                    source_account.to_vec(),
                    asset.to_vec(),
                    to.to_vec(),
                ],
            ),
            Self::ReceiptRead { receipt_digest } => (6, vec![receipt_digest.to_vec()]),
            Self::BalanceView { account, asset, .. } => (7, vec![account.to_vec(), asset.to_vec()]),
            Self::SharedStorageRead => (8, vec![]),
            Self::SharedStorageWrite => (9, vec![]),
        }
    }

    fn validate(&self) -> Result<(), NativeCapabilityError> {
        let valid = match self {
            Self::Call { program } => *program != [0; 32],
            Self::Transfer402 {
                asset,
                to,
                maximum_amount,
            } => *asset != [0; 32] && *to != [0; 32] && *maximum_amount != 0,
            Self::ProgramSpend {
                owner_program,
                seed,
                source_account,
                asset,
                to,
                maximum_amount,
            } => {
                *asset != [0; 32]
                    && *to != [0; 32]
                    && *maximum_amount != 0
                    && derive_native_program_account(*owner_program, seed)? == *source_account
            }
            Self::ReceiptRead { receipt_digest } => *receipt_digest != [0; 32],
            Self::BalanceView {
                account,
                asset,
                receipt_digest,
            } => *account != [0; 32] && *asset != [0; 32] && *receipt_digest != [0; 32],
            _ => true,
        };
        if valid {
            Ok(())
        } else {
            Err(NativeCapabilityError::InvalidGrant)
        }
    }

    fn append(&self, output: &mut Vec<u8>) -> Result<(), NativeCapabilityError> {
        match self {
            Self::StorageRead => output.push(1),
            Self::StorageWrite => output.push(2),
            Self::EmitEvent => output.push(3),
            Self::Call { program } => {
                output.push(4);
                output.extend_from_slice(program);
            }
            Self::Transfer402 {
                asset,
                to,
                maximum_amount,
            } => {
                output.push(5);
                output.extend_from_slice(asset);
                output.extend_from_slice(to);
                output.extend_from_slice(&maximum_amount.to_be_bytes());
            }
            Self::ProgramSpend {
                owner_program,
                seed,
                source_account,
                asset,
                to,
                maximum_amount,
            } => {
                output.push(9);
                output.extend_from_slice(owner_program);
                output.extend_from_slice(
                    &u16::try_from(seed.len())
                        .map_err(|_| NativeCapabilityError::Bounds)?
                        .to_be_bytes(),
                );
                output.extend_from_slice(seed);
                output.extend_from_slice(source_account);
                output.extend_from_slice(asset);
                output.extend_from_slice(to);
                output.extend_from_slice(&maximum_amount.to_be_bytes());
            }
            Self::ReceiptRead { receipt_digest } => {
                output.push(6);
                output.extend_from_slice(receipt_digest);
            }
            Self::BalanceView {
                account,
                asset,
                receipt_digest,
            } => {
                output.push(10);
                output.extend_from_slice(account);
                output.extend_from_slice(asset);
                output.extend_from_slice(receipt_digest);
            }
            Self::SharedStorageRead => output.push(7),
            Self::SharedStorageWrite => output.push(8),
        }
        Ok(())
    }
}

/// # Errors
/// Refuses a zero owner or a seed exceeding the frozen 128-byte bound.
pub fn derive_native_program_account(
    owner: [u8; 32],
    seed: &[u8],
) -> Result<[u8; 32], NativeCapabilityError> {
    if owner == [0; 32] || seed.len() > 128 {
        return Err(NativeCapabilityError::InvalidGrant);
    }
    let mut hash = Sha256::new();
    hash.update(b"LayerX/programs/program-account/v1\0");
    hash.update(owner);
    hash.update(
        u32::try_from(seed.len())
            .map_err(|_| NativeCapabilityError::Bounds)?
            .to_be_bytes(),
    );
    hash.update(seed);
    Ok(hash.finalize().into())
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NativeCapabilitySet(Vec<NativeCapability>);

impl NativeCapabilitySet {
    /// # Errors
    /// Refuses invalid grants, duplicate authority keys, and frozen count bounds.
    pub fn new(mut grants: Vec<NativeCapability>) -> Result<Self, NativeCapabilityError> {
        if grants.len() > MAX_NATIVE_CAPABILITIES
            || grants
                .iter()
                .filter(|grant| matches!(grant, NativeCapability::BalanceView { .. }))
                .count()
                > MAX_NATIVE_BALANCE_VIEWS
        {
            return Err(NativeCapabilityError::Bounds);
        }
        for grant in &grants {
            grant.validate()?;
        }
        grants.sort_by_key(NativeCapability::key);
        if grants.windows(2).any(|pair| pair[0].key() == pair[1].key()) {
            return Err(NativeCapabilityError::Duplicate);
        }
        Ok(Self(grants))
    }

    #[must_use]
    pub fn grants(&self) -> &[NativeCapability] {
        &self.0
    }

    /// # Errors
    /// Refuses any encoding exceeding the frozen canonical byte bound.
    pub fn encode(&self) -> Result<Vec<u8>, NativeCapabilityError> {
        let mut output = u16::try_from(self.0.len())
            .map_err(|_| NativeCapabilityError::Bounds)?
            .to_be_bytes()
            .to_vec();
        for grant in &self.0 {
            grant.append(&mut output)?;
        }
        if output.len() > MAX_NATIVE_CAPABILITY_BYTES {
            return Err(NativeCapabilityError::Bounds);
        }
        Ok(output)
    }

    /// # Errors
    /// Refuses unknown tags, malformed fields, duplicate keys, and noncanonical ordering.
    pub fn decode(encoded: &[u8]) -> Result<Self, NativeCapabilityError> {
        if encoded.len() < 2 || encoded.len() > MAX_NATIVE_CAPABILITY_BYTES {
            return Err(NativeCapabilityError::Bounds);
        }
        let mut remaining = encoded;
        let count = usize::from(u16::from_be_bytes(take(&mut remaining)?));
        if count > MAX_NATIVE_CAPABILITIES {
            return Err(NativeCapabilityError::Bounds);
        }
        let mut grants = Vec::with_capacity(count);
        for _ in 0..count {
            grants.push(decode_grant(&mut remaining)?);
        }
        let result = Self::new(grants)?;
        if !remaining.is_empty() || result.encode()? != encoded {
            return Err(NativeCapabilityError::NonCanonical);
        }
        Ok(result)
    }

    /// # Errors
    /// Refuses new authority keys, increased amounts, or changed `BalanceView` receipt digests.
    pub fn narrow(&self, requested: Self) -> Result<Self, NativeCapabilityError> {
        for child in &requested.0 {
            let parent = self
                .0
                .iter()
                .find(|parent| parent.key() == child.key())
                .ok_or(NativeCapabilityError::Widening)?;
            let admitted = match (parent, child) {
                (
                    NativeCapability::Transfer402 {
                        maximum_amount: ceiling,
                        ..
                    },
                    NativeCapability::Transfer402 { maximum_amount, .. },
                )
                | (
                    NativeCapability::ProgramSpend {
                        maximum_amount: ceiling,
                        ..
                    },
                    NativeCapability::ProgramSpend { maximum_amount, .. },
                ) => maximum_amount <= ceiling,
                (
                    NativeCapability::BalanceView {
                        receipt_digest: expected,
                        ..
                    },
                    NativeCapability::BalanceView { receipt_digest, .. },
                ) => receipt_digest == expected,
                _ => true,
            };
            if !admitted {
                return Err(NativeCapabilityError::Widening);
            }
        }
        Ok(requested)
    }
}

fn take<const LENGTH: usize>(remaining: &mut &[u8]) -> Result<[u8; LENGTH], NativeCapabilityError> {
    let bytes = remaining
        .get(..LENGTH)
        .ok_or(NativeCapabilityError::NonCanonical)?;
    let result = bytes
        .try_into()
        .map_err(|_| NativeCapabilityError::NonCanonical)?;
    *remaining = &remaining[LENGTH..];
    Ok(result)
}

fn decode_grant(remaining: &mut &[u8]) -> Result<NativeCapability, NativeCapabilityError> {
    Ok(match take::<1>(remaining)?[0] {
        1 => NativeCapability::StorageRead,
        2 => NativeCapability::StorageWrite,
        3 => NativeCapability::EmitEvent,
        4 => NativeCapability::Call {
            program: take(remaining)?,
        },
        5 => NativeCapability::Transfer402 {
            asset: take(remaining)?,
            to: take(remaining)?,
            maximum_amount: u128::from_be_bytes(take(remaining)?),
        },
        6 => NativeCapability::ReceiptRead {
            receipt_digest: take(remaining)?,
        },
        7 => NativeCapability::SharedStorageRead,
        8 => NativeCapability::SharedStorageWrite,
        9 => {
            let owner_program = take(remaining)?;
            let length = usize::from(u16::from_be_bytes(take(remaining)?));
            if length > 128 {
                return Err(NativeCapabilityError::Bounds);
            }
            let seed = remaining
                .get(..length)
                .ok_or(NativeCapabilityError::NonCanonical)?
                .to_vec();
            *remaining = &remaining[length..];
            NativeCapability::ProgramSpend {
                owner_program,
                seed,
                source_account: take(remaining)?,
                asset: take(remaining)?,
                to: take(remaining)?,
                maximum_amount: u128::from_be_bytes(take(remaining)?),
            }
        }
        10 => NativeCapability::BalanceView {
            account: take(remaining)?,
            asset: take(remaining)?,
            receipt_digest: take(remaining)?,
        },
        _ => return Err(NativeCapabilityError::NonCanonical),
    })
}

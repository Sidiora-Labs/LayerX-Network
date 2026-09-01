//! Explicit capability grants and their canonical encoding.
//!
//! A program holds no ambient authority. Every effect it produces is checked
//! against a grant the invoking activity fixed before guest code began, and a
//! call to another program may only hand on a narrowing of what this program
//! already holds. Sets are kept in the runtime's authority-key order and
//! refuse duplicate keys, so the bytes handed to `program_call` are the same
//! bytes the host would have produced for the same grants.

use core::cmp::Ordering;

use crate::abi::{MAX_CAPABILITIES, MAX_CAPABILITY_ENCODING_BYTES};
use crate::amount::Amount;
use crate::error::{Field, ProgramError, Reason};
use crate::ids::{AccountId, AssetId, ProgramId, ReceiptDigest};

const TAG_STORAGE_READ: u8 = 1;
const TAG_STORAGE_WRITE: u8 = 2;
const TAG_EMIT_EVENT: u8 = 3;
const TAG_CALL: u8 = 4;
const TAG_TRANSFER: u8 = 5;
const TAG_RECEIPT_READ: u8 = 6;
const TAG_SHARED_STORAGE_READ: u8 = 7;
const TAG_SHARED_STORAGE_WRITE: u8 = 8;

const COUNT_BYTES: usize = 2;
const TAG_BYTES: usize = 1;
const IDENTIFIER_BYTES: usize = 32;
const AMOUNT_BYTES: usize = 16;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum CapabilityKey {
    StorageRead,
    StorageWrite,
    EmitEvent,
    Call([u8; IDENTIFIER_BYTES]),
    Transfer {
        asset: [u8; IDENTIFIER_BYTES],
        to: [u8; IDENTIFIER_BYTES],
    },
    ReceiptRead([u8; IDENTIFIER_BYTES]),
    SharedStorageRead,
    SharedStorageWrite,
}

/// One explicit authority granted by the invoking activity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Capability {
    /// Authority to read this program's namespaced storage.
    StorageRead,
    /// Authority to write and delete this program's namespaced storage.
    StorageWrite,
    /// Authority to read this program's shared storage.
    SharedStorageRead,
    /// Authority to write this program's shared storage.
    SharedStorageWrite,
    /// Authority to emit events under this program's namespace.
    EmitEvent,
    /// Authority to call one named program.
    Call {
        /// Program this authority may call.
        program: ProgramId,
    },
    /// Authority to request 402LXP transfers of one asset to one account,
    /// bounded by an exact integer ceiling.
    Transfer402 {
        /// Asset this authority may move.
        asset: AssetId,
        /// Account this authority may credit.
        to: AccountId,
        /// Exact integer ceiling the total may not exceed.
        maximum_amount: Amount,
    },
    /// Authority to read the facts of one verified receipt.
    ReceiptRead {
        /// Digest naming the readable receipt.
        receipt_digest: ReceiptDigest,
    },
}

impl Capability {
    /// Grants call authority over one named program.
    #[must_use]
    pub const fn call(program: ProgramId) -> Self {
        Self::Call { program }
    }

    /// Grants bounded 402LXP transfer authority.
    ///
    /// # Errors
    ///
    /// Refuses the zero ceiling the runtime's monetary law rejects.
    pub const fn transfer(
        asset: AssetId,
        to: AccountId,
        maximum_amount: Amount,
    ) -> Result<Self, ProgramError> {
        if maximum_amount.is_zero() {
            return Err(ProgramError::value(Field::Amount, Reason::Zero));
        }
        Ok(Self::Transfer402 {
            asset,
            to,
            maximum_amount,
        })
    }

    /// Grants read authority over the facts of one verified receipt.
    #[must_use]
    pub const fn receipt_read(receipt_digest: ReceiptDigest) -> Self {
        Self::ReceiptRead { receipt_digest }
    }

    /// Returns the number of bytes this grant occupies in the canonical
    /// capability-list encoding.
    #[must_use]
    pub const fn encoded_len(self) -> usize {
        match self {
            Self::StorageRead
            | Self::StorageWrite
            | Self::SharedStorageRead
            | Self::SharedStorageWrite
            | Self::EmitEvent => TAG_BYTES,
            Self::Call { .. } | Self::ReceiptRead { .. } => TAG_BYTES + IDENTIFIER_BYTES,
            Self::Transfer402 { .. } => {
                TAG_BYTES + IDENTIFIER_BYTES + IDENTIFIER_BYTES + AMOUNT_BYTES
            }
        }
    }

    const fn key(self) -> CapabilityKey {
        match self {
            Self::StorageRead => CapabilityKey::StorageRead,
            Self::StorageWrite => CapabilityKey::StorageWrite,
            Self::SharedStorageRead => CapabilityKey::SharedStorageRead,
            Self::SharedStorageWrite => CapabilityKey::SharedStorageWrite,
            Self::EmitEvent => CapabilityKey::EmitEvent,
            Self::Call { program } => CapabilityKey::Call(program.bytes()),
            Self::Transfer402 { asset, to, .. } => CapabilityKey::Transfer {
                asset: asset.bytes(),
                to: to.bytes(),
            },
            Self::ReceiptRead { receipt_digest } => {
                CapabilityKey::ReceiptRead(receipt_digest.bytes())
            }
        }
    }
}

/// Closed set of explicit capabilities holding at most `N` grants.
///
/// Grants are stored in the runtime's authority-key order and duplicate keys
/// are refused, so no ambiguous limit can reach the host.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CapabilitySet<const N: usize> {
    grants: [Capability; N],
    length: usize,
}

impl<const N: usize> CapabilitySet<N> {
    /// Returns an empty ambient-authority-free set.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            grants: [Capability::StorageRead; N],
            length: 0,
        }
    }

    /// Builds a validated set from an explicit list of grants.
    ///
    /// # Errors
    ///
    /// Refuses a duplicate authority key and a list past the declared
    /// capacity or the ABI's grant ceiling.
    pub fn from_grants(grants: &[Capability]) -> Result<Self, ProgramError> {
        let mut set = Self::empty();
        for grant in grants {
            set.insert(*grant)?;
        }
        Ok(set)
    }

    /// Adds one grant in authority-key order.
    ///
    /// # Errors
    ///
    /// Refuses a duplicate authority key and a set past the declared capacity
    /// or the ABI's grant ceiling.
    pub fn insert(&mut self, grant: Capability) -> Result<(), ProgramError> {
        if self.length >= N || self.length >= MAX_CAPABILITIES {
            return Err(ProgramError::value(Field::Capability, Reason::TooLarge));
        }
        let key = grant.key();
        let mut position = 0;
        while position < self.length {
            match self.grants[position].key().cmp(&key) {
                Ordering::Less => position = position.saturating_add(1),
                Ordering::Equal => {
                    return Err(ProgramError::value(Field::Capability, Reason::Duplicate));
                }
                Ordering::Greater => break,
            }
        }
        let mut index = self.length;
        while index > position {
            self.grants[index] = self.grants[index.saturating_sub(1)];
            index = index.saturating_sub(1);
        }
        self.grants[position] = grant;
        self.length = self.length.saturating_add(1);
        Ok(())
    }

    /// Returns the number of grants held.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.length
    }

    /// Reports whether the set holds no grant at all.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.length == 0
    }

    /// Borrows the held grants in authority-key order.
    #[must_use]
    pub fn grants(&self) -> &[Capability] {
        &self.grants[..self.length]
    }

    /// Reports whether the set already holds the given authority key.
    #[must_use]
    pub fn holds(&self, grant: Capability) -> bool {
        let key = grant.key();
        self.grants().iter().any(|held| held.key() == key)
    }

    /// Returns the exact length of this set's canonical encoding.
    #[must_use]
    pub fn encoded_len(&self) -> usize {
        let mut total = COUNT_BYTES;
        for grant in self.grants() {
            total = total.saturating_add(grant.encoded_len());
        }
        total
    }

    /// Encodes this set into the frozen deterministic capability-list format
    /// consumed by `program_call`, returning the number of bytes written.
    ///
    /// # Errors
    ///
    /// Refuses an encoding past the ABI bound and an output shorter than the
    /// encoding needs.
    pub fn encode_into(&self, output: &mut [u8]) -> Result<usize, ProgramError> {
        let encoded_len = self.encoded_len();
        if encoded_len > MAX_CAPABILITY_ENCODING_BYTES {
            return Err(ProgramError::value(
                Field::CapabilityEncoding,
                Reason::TooLarge,
            ));
        }
        if output.len() < encoded_len {
            return Err(ProgramError::value(Field::Buffer, Reason::TooSmall));
        }
        let count = u16::try_from(self.length)
            .map_err(|_| ProgramError::value(Field::Capability, Reason::TooLarge))?;
        let mut cursor = 0;
        write_bytes(output, &mut cursor, &count.to_be_bytes())?;
        for grant in self.grants() {
            match *grant {
                Capability::StorageRead => write_bytes(output, &mut cursor, &[TAG_STORAGE_READ])?,
                Capability::StorageWrite => write_bytes(output, &mut cursor, &[TAG_STORAGE_WRITE])?,
                Capability::SharedStorageRead => {
                    write_bytes(output, &mut cursor, &[TAG_SHARED_STORAGE_READ])?
                }
                Capability::SharedStorageWrite => {
                    write_bytes(output, &mut cursor, &[TAG_SHARED_STORAGE_WRITE])?
                }
                Capability::EmitEvent => write_bytes(output, &mut cursor, &[TAG_EMIT_EVENT])?,
                Capability::Call { program } => {
                    write_bytes(output, &mut cursor, &[TAG_CALL])?;
                    write_bytes(output, &mut cursor, &program.bytes())?;
                }
                Capability::Transfer402 {
                    asset,
                    to,
                    maximum_amount,
                } => {
                    write_bytes(output, &mut cursor, &[TAG_TRANSFER])?;
                    write_bytes(output, &mut cursor, &asset.bytes())?;
                    write_bytes(output, &mut cursor, &to.bytes())?;
                    write_bytes(output, &mut cursor, &maximum_amount.to_be_bytes())?;
                }
                Capability::ReceiptRead { receipt_digest } => {
                    write_bytes(output, &mut cursor, &[TAG_RECEIPT_READ])?;
                    write_bytes(output, &mut cursor, &receipt_digest.bytes())?;
                }
            }
        }
        Ok(cursor)
    }
}

impl<const N: usize> Default for CapabilitySet<N> {
    fn default() -> Self {
        Self::empty()
    }
}

fn write_bytes(output: &mut [u8], cursor: &mut usize, bytes: &[u8]) -> Result<(), ProgramError> {
    let end = cursor
        .checked_add(bytes.len())
        .ok_or_else(|| ProgramError::value(Field::Buffer, Reason::TooLarge))?;
    let target = output
        .get_mut(*cursor..end)
        .ok_or_else(|| ProgramError::value(Field::Buffer, Reason::TooSmall))?;
    target.copy_from_slice(bytes);
    *cursor = end;
    Ok(())
}

#[cfg(test)]
mod parity_vectors {
    use alloc::{vec, vec::Vec};

    use super::{Capability, CapabilitySet};
    use crate::{
        AccountId, Amount, AssetId, GrantedCapabilities, ProgramId,
        MAX_CANONICAL_CAPABILITY_SET_BYTES,
    };

    const FIXTURE: &str = include_str!("../../vectors/capability-boundary.kvx");

    fn fixture_hex() -> &'static str {
        FIXTURE
            .lines()
            .find_map(|line| line.strip_prefix("encoded_hex = \"")?.strip_suffix('"'))
            .unwrap_or_else(|| panic!("mixed_v1 encoded_hex fixture"))
    }

    fn decode_hex(value: &str) -> Vec<u8> {
        value
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let digit = |byte: u8| match byte {
                    b'0'..=b'9' => byte - b'0',
                    b'a'..=b'f' => byte - b'a' + 10,
                    _ => panic!("fixture hex digit"),
                };
                (digit(pair[0]) << 4) | digit(pair[1])
            })
            .collect()
    }

    #[test]
    fn mixed_v1_encoding_matches_shared_sdk_fixture() {
        let program = ProgramId::new([0x11; 32])
            .unwrap_or_else(|error| panic!("program: {error}"));
        let asset = AssetId::new([0x22; 32])
            .unwrap_or_else(|error| panic!("asset: {error}"));
        let account = AccountId::new([0x33; 32])
            .unwrap_or_else(|error| panic!("account: {error}"));
        let transfer = Capability::transfer(asset, account, Amount::from_u128(7))
            .unwrap_or_else(|error| panic!("transfer: {error}"));
        let set = CapabilitySet::<3>::from_grants(&[
            Capability::EmitEvent,
            Capability::call(program),
            transfer,
        ])
        .unwrap_or_else(|error| panic!("capabilities: {error}"));
        let expected = decode_hex(fixture_hex());
        let mut actual = vec![0; set.encoded_len()];
        let written = set
            .encode_into(&mut actual)
            .unwrap_or_else(|error| panic!("encoding: {error}"));
        assert_eq!(written, expected.len());
        assert_eq!(actual, expected);
        assert_eq!(MAX_CANONICAL_CAPABILITY_SET_BYTES, 65_452);
        assert!(GrantedCapabilities::new(&vec![0; 65_535]).is_ok());
        assert!(GrantedCapabilities::new(&vec![0; 65_536]).is_err());
    }
}

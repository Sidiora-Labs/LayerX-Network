//! The Solana account model mapped onto namespaced storage.
//!
//! A Solana account is a byte buffer with an owner, a lamport balance and a
//! rent-exempt minimum. An Anchor account additionally begins with an eight
//! byte discriminator and holds `borsh`-encoded fields after it.
//!
//! Only the data part carries over, and it carries over exactly: the ported
//! cell holds the same discriminator followed by the same `borsh` bytes, so an
//! existing client decodes it unchanged. The parts that do not carry over are
//! the parts that are properties of an account rather than of its data - the
//! lamport balance, the owner and rent - and each is refused by name rather
//! than being modelled as bytes in a cell.

use layerx_programs_runtime::storage::MAX_STORAGE_VALUE_BYTES;

use crate::anchor::{account_discriminator, DISCRIMINATOR_BYTES};
use crate::error::PortRefusal;
use crate::pubkey::PUBKEY_BYTES;

/// Maximum number of fields one ported account schema may declare.
pub const MAX_FIELDS: usize = 64;

/// One `borsh` field type a ported account may declare.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FieldType {
    /// `u8`.
    U8,
    /// `u16`, little-endian.
    U16,
    /// `u32`, little-endian.
    U32,
    /// `u64`, little-endian.
    U64,
    /// `i64`, little-endian two's complement.
    I64,
    /// `bool`, one byte holding zero or one.
    Bool,
    /// `Pubkey`, thirty-two raw bytes.
    Pubkey,
}

impl FieldType {
    /// Returns the fixed `borsh` width of the field.
    #[must_use]
    pub const fn width(self) -> usize {
        match self {
            Self::U8 | Self::Bool => 1,
            Self::U16 => 2,
            Self::U32 => 4,
            Self::U64 | Self::I64 => 8,
            Self::Pubkey => PUBKEY_BYTES,
        }
    }
}

/// One declared field of a ported account, in declaration order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Field {
    /// The field name as the Rust struct declares it.
    pub name: String,
    /// The field's `borsh` type.
    pub kind: FieldType,
}

/// One value of a declared field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FieldValue {
    /// A `u8` value.
    U8(u8),
    /// A `u16` value.
    U16(u16),
    /// A `u32` value.
    U32(u32),
    /// A `u64` value.
    U64(u64),
    /// An `i64` value.
    I64(i64),
    /// A `bool` value.
    Bool(bool),
    /// A `Pubkey` value.
    Pubkey([u8; PUBKEY_BYTES]),
}

impl FieldValue {
    /// Returns the field type this value belongs to.
    #[must_use]
    pub const fn kind(&self) -> FieldType {
        match self {
            Self::U8(_) => FieldType::U8,
            Self::U16(_) => FieldType::U16,
            Self::U32(_) => FieldType::U32,
            Self::U64(_) => FieldType::U64,
            Self::I64(_) => FieldType::I64,
            Self::Bool(_) => FieldType::Bool,
            Self::Pubkey(_) => FieldType::Pubkey,
        }
    }

    /// Appends the value's `borsh` bytes, little-endian for every integer.
    pub fn encode(&self, out: &mut Vec<u8>) {
        match self {
            Self::U8(value) => out.push(*value),
            Self::U16(value) => out.extend_from_slice(&value.to_le_bytes()),
            Self::U32(value) => out.extend_from_slice(&value.to_le_bytes()),
            Self::U64(value) => out.extend_from_slice(&value.to_le_bytes()),
            Self::I64(value) => out.extend_from_slice(&value.to_le_bytes()),
            Self::Bool(value) => out.push(u8::from(*value)),
            Self::Pubkey(value) => out.extend_from_slice(value),
        }
    }
}

/// The declared shape of one ported Anchor account.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountSchema {
    name: String,
    discriminator: [u8; DISCRIMINATOR_BYTES],
    fields: Vec<Field>,
}

impl AccountSchema {
    /// Declares an account schema and computes its Anchor discriminator.
    ///
    /// # Errors
    ///
    /// Refuses an unnamed account, more fields than the declared bound, an
    /// unnamed or repeated field and a schema beyond the storage value bound.
    pub fn new(name: &str, fields: Vec<Field>) -> Result<Self, PortRefusal> {
        if name.is_empty() || fields.len() > MAX_FIELDS {
            return Err(PortRefusal::SchemaMismatch);
        }
        for (index, field) in fields.iter().enumerate() {
            if field.name.is_empty()
                || fields
                    .iter()
                    .skip(index.saturating_add(1))
                    .any(|other| other.name == field.name)
            {
                return Err(PortRefusal::SchemaMismatch);
            }
        }
        let schema = Self {
            name: name.to_owned(),
            discriminator: account_discriminator(name),
            fields,
        };
        if schema.space() > MAX_STORAGE_VALUE_BYTES {
            return Err(PortRefusal::AccountBounds);
        }
        Ok(schema)
    }

    /// Returns the account name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the eight-byte Anchor discriminator, byte-identical to the one
    /// an existing client checks for.
    #[must_use]
    pub const fn discriminator(&self) -> [u8; DISCRIMINATOR_BYTES] {
        self.discriminator
    }

    /// Borrows the declared fields in declaration order.
    #[must_use]
    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    /// Returns the `space` the Anchor account declaration reserves: the
    /// discriminator plus every field.
    #[must_use]
    pub fn space(&self) -> usize {
        self.fields
            .iter()
            .fold(DISCRIMINATOR_BYTES, |total, field| {
                total.saturating_add(field.kind.width())
            })
    }

    /// Returns the byte offset of a named field inside the account data,
    /// counting the discriminator.
    #[must_use]
    pub fn offset(&self, name: &str) -> Option<usize> {
        let mut offset = DISCRIMINATOR_BYTES;
        for field in &self.fields {
            if field.name == name {
                return Some(offset);
            }
            offset = offset.saturating_add(field.kind.width());
        }
        None
    }

    /// Encodes account data exactly as Anchor writes it: the discriminator
    /// followed by every field in `borsh` little-endian order.
    ///
    /// # Errors
    ///
    /// Refuses a value list that does not match the declared schema.
    pub fn encode(&self, values: &[FieldValue]) -> Result<Vec<u8>, PortRefusal> {
        if values.len() != self.fields.len() {
            return Err(PortRefusal::SchemaMismatch);
        }
        let mut encoded = Vec::with_capacity(self.space());
        encoded.extend_from_slice(&self.discriminator);
        for (field, value) in self.fields.iter().zip(values) {
            if value.kind() != field.kind {
                return Err(PortRefusal::SchemaMismatch);
            }
            value.encode(&mut encoded);
        }
        Ok(encoded)
    }

    /// Decodes account data, checking the discriminator first exactly as
    /// Anchor does on every account load.
    ///
    /// # Errors
    ///
    /// Refuses data carrying a different discriminator and data whose length
    /// is not the declared space.
    pub fn decode(&self, data: &[u8]) -> Result<Vec<FieldValue>, PortRefusal> {
        if data.get(..DISCRIMINATOR_BYTES) != Some(self.discriminator.as_slice()) {
            return Err(PortRefusal::DiscriminatorMismatch);
        }
        if data.len() != self.space() {
            return Err(PortRefusal::AccountBounds);
        }
        let mut cursor = DISCRIMINATOR_BYTES;
        let mut values = Vec::with_capacity(self.fields.len());
        for field in &self.fields {
            let end = cursor.saturating_add(field.kind.width());
            let bytes = data.get(cursor..end).ok_or(PortRefusal::AccountBounds)?;
            values.push(decode_field(field.kind, bytes)?);
            cursor = end;
        }
        Ok(values)
    }
}

/// What a ported program is allowed to do with an account it did not create.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountRole {
    /// `#[account(init)]` or `init_if_needed`: state the program owns inside
    /// the invoking principal's namespace.
    ProgramState,
    /// A program-owned account holding state shared across every caller,
    /// such as a total supply or a pool reserve. Maps onto the program-shared
    /// namespace `(program)`.
    ProgramOwnedShared,
    /// A `Signer` account. It is the invoking principal, and the runtime
    /// authenticates it before guest code runs.
    Signer,
    /// A `SystemAccount` or `UncheckedAccount` credited by a transfer.
    TransferTarget,
    /// A token account whose balance the instruction would move.
    TokenBalance,
    /// The `System Program`, `Token Program` or any other program passed in to
    /// be invoked by `CPI`.
    ProgramHandle,
}

impl AccountRole {
    /// Returns the namespaced-storage or capability form the role takes.
    ///
    /// # Errors
    ///
    /// Refuses a token balance, which is a 402LXP asset the kernel owns and
    /// never bytes a program writes.
    pub fn translate(self) -> Result<AccountMapping, PortRefusal> {
        match self {
            Self::ProgramState => Ok(AccountMapping::NamespacedCell),
            Self::ProgramOwnedShared => Ok(AccountMapping::SharedCell),
            Self::Signer => Ok(AccountMapping::InvokingPrincipal),
            Self::TransferTarget => Ok(AccountMapping::TransferRecipient),
            Self::ProgramHandle => Ok(AccountMapping::CallCapability),
            Self::TokenBalance => Err(PortRefusal::LamportMutation),
        }
    }
}

/// What one account in an Anchor `Accounts` struct becomes after the port.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountMapping {
    /// A cell in the `(program, principal)` namespace.
    NamespacedCell,
    /// A cell in the `(program)` shared namespace, readable and writable by
    /// every principal invoking this program.
    SharedCell,
    /// The principal the runtime fixed before guest code ran.
    InvokingPrincipal,
    /// The recipient of an authenticated 402LXP transfer.
    TransferRecipient,
    /// A narrowed `Call` capability naming the callee program.
    CallCapability,
}

/// Returns the account an Anchor `#[account(...)]` constraint set describes,
/// mapped onto its namespaced-storage key.
///
/// # Errors
///
/// Refuses a schema, seed path or framed key the declared bounds reject.
pub fn ported_account(
    schema: &AccountSchema,
    seeds: &crate::pubkey::SeedPath,
    envelope: &[usize],
) -> Result<(Vec<u8>, usize), PortRefusal> {
    let key = seeds.collapse(envelope)?.storage_key()?;
    Ok((key, schema.space()))
}

fn decode_field(kind: FieldType, bytes: &[u8]) -> Result<FieldValue, PortRefusal> {
    let value = match kind {
        FieldType::U8 => FieldValue::U8(*bytes.first().ok_or(PortRefusal::AccountBounds)?),
        FieldType::U16 => FieldValue::U16(u16::from_le_bytes(take::<2>(bytes)?)),
        FieldType::U32 => FieldValue::U32(u32::from_le_bytes(take::<4>(bytes)?)),
        FieldType::U64 => FieldValue::U64(u64::from_le_bytes(take::<8>(bytes)?)),
        FieldType::I64 => FieldValue::I64(i64::from_le_bytes(take::<8>(bytes)?)),
        FieldType::Bool => match bytes.first() {
            Some(0) => FieldValue::Bool(false),
            Some(1) => FieldValue::Bool(true),
            _ => return Err(PortRefusal::AccountBounds),
        },
        FieldType::Pubkey => FieldValue::Pubkey(take::<PUBKEY_BYTES>(bytes)?),
    };
    Ok(value)
}

fn take<const N: usize>(bytes: &[u8]) -> Result<[u8; N], PortRefusal> {
    bytes
        .get(..N)
        .and_then(|slice| slice.try_into().ok())
        .ok_or(PortRefusal::AccountBounds)
}

//! Ahead-of-execution access recipes for call-activity tooling.

use crate::{AccountId, AssetId, ProgramError, ProgramId, StorageKey};

const MAX_STORAGE_ENTRIES: usize = 1_024;
const MAX_ACCOUNT_ENTRIES: usize = 512;
const MAX_CALLEE_ENTRIES: usize = 512;
const MAX_DECLARATION_BYTES: usize = 1_048_576;

/// Host-fixed namespace selected by a storage access recipe.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum AccessScope { Principal, Shared }

/// Whether the activity only observes or may mutate a resource.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum AccessMode { Read, Write }

/// Canonical key region declared before execution.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum KeyAccess<'a> {
    Exact(StorageKey<'a>),
    Prefix(&'a [u8]),
    Range { start: StorageKey<'a>, end: StorageKey<'a> },
    WholeNamespace,
}

impl<'a> KeyAccess<'a> {
    /// Constructs a bounded prefix. # Errors Refuses an oversized prefix.
    pub const fn prefix(bytes: &'a [u8]) -> Result<Self, ProgramError> {
        if bytes.len() > crate::MAX_STORAGE_KEY_BYTES {
            Err(ProgramError::value(crate::Field::StorageKey, crate::Reason::TooLarge))
        } else { Ok(Self::Prefix(bytes)) }
    }
    /// Constructs a nonempty half-open range. # Errors Refuses reversed bounds.
    pub fn range(start: StorageKey<'a>, end: StorageKey<'a>) -> Result<Self, ProgramError> {
        if start.bytes() >= end.bytes() {
            Err(ProgramError::value(crate::Field::StorageKey, crate::Reason::Malformed))
        } else { Ok(Self::Range { start, end }) }
    }
}

/// One SDK-proved access. Tooling supplies the executing program/principal
/// identities when projecting principal and shared scopes into protocol bytes.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum AccessEntry<'a> {
    Storage {
        /// Omit for the root program; name a program for callee state.
        program: Option<ProgramId>,
        scope: AccessScope,
        mode: AccessMode,
        keys: KeyAccess<'a>,
    },
    Account { account: AccountId, asset: AssetId, mode: AccessMode },
    Call { callee: ProgramId },
}

/// Presence-sensitive access field embedded in a canonical call activity.
pub enum ActivityAccess<'a, const N: usize> {
    /// Declares the whole capability-reachable set.
    Absent,
    /// Commits the exact SDK recipe.
    Explicit(AccessRecipe<'a, N>),
}

/// Fixed-capacity declaration recipe usable without allocation.
pub struct AccessRecipe<'a, const N: usize> {
    entries: [Option<AccessEntry<'a>>; N],
    length: usize,
}

impl<'a, const N: usize> AccessRecipe<'a, N> {
    /// Starts an explicit declaration recipe.
    #[must_use]
    pub const fn explicit() -> Self { Self { entries: [None; N], length: 0 } }

    /// Derives a recipe solely from canonical calldata. The callback has no
    /// state handle, making prior-state-dependent accesses remain explicit.
    pub fn derive_from_calldata(
        calldata: &'a [u8],
        derive: fn(&'a [u8], &mut Self) -> Result<(), ProgramError>,
    ) -> Result<Self, ProgramError> {
        let mut recipe = Self::explicit();
        derive(calldata, &mut recipe)?;
        Ok(recipe)
    }

    /// Adds one proved entry. # Errors Refuses capacity exhaustion.
    pub fn push(&mut self, entry: AccessEntry<'a>) -> Result<(), ProgramError> {
        if self.entries[..self.length].iter().flatten().any(|existing| *existing == entry) {
            return Err(ProgramError::value(crate::Field::Buffer, crate::Reason::Malformed));
        }
        let Some(slot) = self.entries.get_mut(self.length) else {
            return Err(ProgramError::value(crate::Field::Buffer, crate::Reason::TooLarge));
        };
        *slot = Some(entry);
        self.length += 1;
        Ok(())
    }

    /// Borrows entries in declaration order for host tooling to canonicalize.
    pub fn entries(&self) -> impl Iterator<Item = AccessEntry<'a>> + '_ {
        self.entries[..self.length].iter().flatten().copied()
    }

    fn sorted_entries(
        &self,
        executing_program: ProgramId,
    ) -> Result<[Option<AccessEntry<'a>>; N], ProgramError> {
        let mut entries = self.entries;
        for entry in entries[..self.length].iter_mut().flatten() {
            if let AccessEntry::Storage { program, .. } = entry {
                if program.is_none() { *program = Some(executing_program); }
            }
        }
        entries[..self.length].sort_unstable();
        if entries[..self.length].windows(2).any(|window| window[0] == window[1]) {
            return Err(ProgramError::value(crate::Field::Buffer, crate::Reason::Malformed));
        }
        Ok(entries)
    }
}

impl<'a, const N: usize> Default for AccessRecipe<'a, N> {
    fn default() -> Self { Self::explicit() }
}

impl<'a, const N: usize> ActivityAccess<'a, N> {
    /// Encodes bytes accepted by the runtime's strict access-declaration
    /// decoder. The caller supplies the executing identities used to project
    /// principal/shared SDK scopes.
    ///
    /// # Errors
    ///
    /// Refuses a short output buffer or a count outside the canonical u16 bound.
    pub fn encode_canonical(
        &self,
        executing_program: ProgramId,
        principal: crate::Principal,
        output: &mut [u8],
    ) -> Result<usize, ProgramError> {
        let mut writer = Writer::new(output);
        writer.put(b"LayerX/programs/access-declaration/v1\0")?;
        match self {
            Self::Absent => writer.byte(0)?,
            Self::Explicit(recipe) => {
                writer.byte(1)?;
                let length_offset = writer.reserve(4)?;
                let start = writer.offset;
                writer.put(b"LayerX/programs/access-set/v1\0")?;
                let entries = recipe.sorted_entries(executing_program)?;
                let storage_count = entries[..recipe.length].iter().flatten()
                    .filter(|entry| matches!(entry, AccessEntry::Storage { .. })).count();
                if storage_count > MAX_STORAGE_ENTRIES {
                    return Err(ProgramError::value(crate::Field::Buffer, crate::Reason::TooLarge));
                }
                writer.u16(storage_count)?;
                for entry in entries[..recipe.length].iter().flatten() {
                    if let AccessEntry::Storage { program, scope, mode, keys } = entry {
                        writer.put(&program.unwrap_or(executing_program).bytes())?;
                        match scope {
                            AccessScope::Principal => { writer.byte(0)?; writer.put(&principal.bytes())?; }
                            AccessScope::Shared => writer.byte(1)?,
                        }
                        writer.byte(match mode { AccessMode::Read => 0, AccessMode::Write => 1 })?;
                        match keys {
                            KeyAccess::Exact(key) => { writer.byte(0)?; writer.key(key.bytes())?; }
                            KeyAccess::Prefix(prefix) => { writer.byte(1)?; writer.key(prefix)?; }
                            KeyAccess::Range { start, end } => {
                                writer.byte(2)?; writer.key(start.bytes())?; writer.key(end.bytes())?;
                            }
                            KeyAccess::WholeNamespace => { writer.byte(1)?; writer.key(&[])?; }
                        }
                    }
                }
                let account_count = entries[..recipe.length].iter().flatten()
                    .filter(|entry| matches!(entry, AccessEntry::Account { .. })).count();
                if account_count > MAX_ACCOUNT_ENTRIES {
                    return Err(ProgramError::value(crate::Field::Buffer, crate::Reason::TooLarge));
                }
                writer.u16(account_count)?;
                for entry in entries[..recipe.length].iter().flatten() {
                    if let AccessEntry::Account { account, asset, mode } = entry {
                        writer.put(&account.bytes())?;
                        writer.put(&asset.bytes())?;
                        writer.byte(match mode { AccessMode::Read => 0, AccessMode::Write => 1 })?;
                    }
                }
                let call_count = entries[..recipe.length].iter().flatten()
                    .filter(|entry| matches!(entry, AccessEntry::Call { .. })).count();
                if call_count > MAX_CALLEE_ENTRIES {
                    return Err(ProgramError::value(crate::Field::Buffer, crate::Reason::TooLarge));
                }
                writer.u16(call_count)?;
                for entry in entries[..recipe.length].iter().flatten() {
                    if let AccessEntry::Call { callee } = entry { writer.put(&callee.bytes())?; }
                }
                let encoded_length = writer.offset.checked_sub(start)
                    .ok_or_else(|| ProgramError::value(crate::Field::Buffer, crate::Reason::Malformed))?;
                let encoded_length = u32::try_from(encoded_length)
                    .map_err(|_| ProgramError::value(crate::Field::Buffer, crate::Reason::TooLarge))?;
                writer.output[length_offset..length_offset + 4]
                    .copy_from_slice(&encoded_length.to_be_bytes());
            }
        }
        if writer.offset > MAX_DECLARATION_BYTES {
            return Err(ProgramError::value(crate::Field::Buffer, crate::Reason::TooLarge));
        }
        Ok(writer.offset)
    }
}

struct Writer<'a> { output: &'a mut [u8], offset: usize }
impl<'a> Writer<'a> {
    const fn new(output: &'a mut [u8]) -> Self { Self { output, offset: 0 } }
    fn reserve(&mut self, length: usize) -> Result<usize, ProgramError> {
        let start = self.offset;
        let end = start.checked_add(length)
            .ok_or_else(|| ProgramError::value(crate::Field::Buffer, crate::Reason::TooLarge))?;
        if end > self.output.len() { return Err(ProgramError::value(crate::Field::Buffer, crate::Reason::TooLarge)); }
        self.output[start..end].fill(0);
        self.offset = end;
        Ok(start)
    }
    fn put(&mut self, bytes: &[u8]) -> Result<(), ProgramError> {
        let start = self.reserve(bytes.len())?;
        self.output[start..start + bytes.len()].copy_from_slice(bytes);
        Ok(())
    }
    fn byte(&mut self, byte: u8) -> Result<(), ProgramError> { self.put(&[byte]) }
    fn u16(&mut self, value: usize) -> Result<(), ProgramError> {
        let value = u16::try_from(value)
            .map_err(|_| ProgramError::value(crate::Field::Buffer, crate::Reason::TooLarge))?;
        self.put(&value.to_be_bytes())
    }
    fn key(&mut self, key: &[u8]) -> Result<(), ProgramError> {
        self.u16(key.len())?;
        self.put(key)
    }
}

//! Receipt-bound views of the canonical kernel account tree.

use core::fmt::{self, Display};

use layerx_programs_runtime::{derive_program_account, ProgramId};

use crate::{hash::sha256, ReadFreshness};

const PROTOCOL_VERSION_ACCOUNT_TREE: u16 = 2;
const MODULE_VALUE_KIND: u8 = 13;
const MAX_ACCOUNT_NAME_BYTES: usize = 512;
const MAX_PROOF_DEPTH: usize = 32;
pub const MAX_PROGRAM_VALUE_ACCOUNTS: usize = 512;
const STATE_LEAF_DOMAIN: &[u8] = b"LXP/v1/state-leaf\0";
const STATE_NODE_DOMAIN: &[u8] = b"LXP/v1/state-node\0";
const ACCOUNT_TREE_KEY: &[u8] = b"account-tree";
const PROGRAM_ACCOUNT_NAME_PREFIX: &[u8] = b"module:programs:value:";
const PROGRAM_ACCOUNT_PRIMARY_PREFIX: &[u8] = b"program-account\0p";
const PROGRAMS_MODULE_ID: u16 = 9;

/// Durable registry binding produced by the ABI-two program-account activity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramValueAccountBinding {
    pub record_version: u8,
    pub program: ProgramId,
    pub seed: Vec<u8>,
    pub account_id: [u8; 32],
    pub asset_id: [u8; 32],
    pub registered_sequence: u64,
    pub registration_event_digest: [u8; 32],
}

/// Exact account leaf material committed by `lx_account_registry_root`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalAccountLeaf {
    pub account_id: [u8; 32],
    pub name: Vec<u8>,
    pub kind: u8,
    pub balance: u128,
    pub asset_id: [u8; 32],
    pub has_asset: bool,
    pub next_sequence: u64,
    pub created_at_sequence: u64,
    pub frozen: bool,
    pub has_open_reference: bool,
    pub authority_key: [u8; 32],
    pub has_authority_key: bool,
}

/// Canonical proof shape used by the account, universal and module-root trees.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StateProof {
    pub leaf_index: u32,
    pub leaf_count: u32,
    pub siblings: Vec<[u8; 32]>,
}

/// One canonical account leaf and its membership proof in the account root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProvenAccountLeaf {
    pub leaf: CanonicalAccountLeaf,
    pub proof: StateProof,
}

/// One exact version-two Programs primary record and its membership proof in
/// the Programs module subtree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProvenProgramBinding {
    pub binding: ProgramValueAccountBinding,
    pub proof: StateProof,
}

/// Account-tree evidence bound through the universal subtree to a receipt
/// state root. Only program accounts required by the registry need individual
/// account proofs; the two outer proofs bind them to the same canonical tree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedAccountSnapshot {
    pub protocol_version: u16,
    pub receipt_digest: [u8; 32],
    pub state_root: [u8; 32],
    pub universal_root: [u8; 32],
    pub programs_root: [u8; 32],
    pub account_root: [u8; 32],
    pub account_tree_proof: StateProof,
    pub universal_root_proof: StateProof,
    pub programs_root_proof: StateProof,
    pub freshness: ReadFreshness,
    pub bindings: Vec<ProvenProgramBinding>,
    pub accounts: Vec<ProvenAccountLeaf>,
}

/// A real, currently observed program balance after registry derivation and
/// account-tree membership have both been checked.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValueAccount {
    pub seed: Vec<u8>,
    pub account_id: [u8; 32],
    pub asset_id: [u8; 32],
    pub balance: u128,
    pub frozen: bool,
    pub observed_sequence: u64,
    pub receipt_digest: [u8; 32],
    pub state_root: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccountStateHead {
    pub receipt_digest: [u8; 32],
    pub state_root: [u8; 32],
    pub freshness: ReadFreshness,
}

pub trait AccountStateJournal {
    fn account_state_head(
        &self,
        receipt_digest: [u8; 32],
    ) -> Result<AccountStateHead, AccountStateError>;

    fn current_account_state_head(&self) -> Result<AccountStateHead, AccountStateError>;
}

#[derive(Clone, Copy, Debug)]
pub struct JournalAccountStateAuthority<J> {
    journal: J,
    now: u64,
    staleness_limit: u64,
}

impl<J: AccountStateJournal> JournalAccountStateAuthority<J> {
    pub fn new(journal: J, now: u64, staleness_limit: u64) -> Result<Self, AccountStateError> {
        if now == 0 || staleness_limit == 0 {
            return Err(AccountStateError::InvalidFreshness);
        }
        Ok(Self {
            journal,
            now,
            staleness_limit,
        })
    }

    fn verify(
        &self,
        snapshot: &VerifiedAccountSnapshot,
        current: bool,
    ) -> Result<(), AccountStateError> {
        let head = if current {
            self.journal.current_account_state_head()?
        } else {
            self.journal.account_state_head(snapshot.receipt_digest)?
        };
        if head.receipt_digest != snapshot.receipt_digest
            || head.state_root != snapshot.state_root
            || head.freshness != snapshot.freshness
        {
            return Err(AccountStateError::UnverifiedReceipt);
        }
        if self.now < head.freshness.observed_at
            || self.now.saturating_sub(head.freshness.observed_at) > self.staleness_limit
        {
            return Err(AccountStateError::StaleRead);
        }
        Ok(())
    }
}

/// Typed refusal from canonical account binding or proof verification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountStateError {
    LegacyProtocol,
    UnknownProgram,
    InactiveProgram,
    InvalidBinding,
    BindingConflict,
    UnverifiedRegistration,
    JournalUnavailable,
    StaleRead,
    InvalidFreshness,
    InvalidAccountLeaf,
    MissingAccount { account_id: [u8; 32] },
    DuplicateAccount { account_id: [u8; 32] },
    AccountMismatch { account_id: [u8; 32] },
    ProofTooDeep,
    InvalidProof,
    AccountRootMismatch,
    UniversalRootMismatch,
    StateRootMismatch,
    UnverifiedReceipt,
}

impl Display for AccountStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LegacyProtocol => {
                formatter.write_str("account tree is unavailable before protocol version two")
            }
            Self::UnknownProgram => formatter.write_str("program is not registered"),
            Self::InactiveProgram => {
                formatter.write_str("program account registration requires an active program")
            }
            Self::InvalidBinding => formatter.write_str("program account binding is invalid"),
            Self::BindingConflict => {
                formatter.write_str("program account binding conflicts with registry history")
            }
            Self::UnverifiedRegistration => {
                formatter.write_str("program account registration receipt is unverified")
            }
            Self::JournalUnavailable => {
                formatter.write_str("canonical account-state journal is unavailable")
            }
            Self::StaleRead => formatter.write_str("account-state head is stale"),
            Self::InvalidFreshness => formatter.write_str("account snapshot freshness is invalid"),
            Self::InvalidAccountLeaf => formatter.write_str("canonical account leaf is invalid"),
            Self::MissingAccount { account_id } => write!(
                formatter,
                "program account {account_id:02x?} is missing from the account snapshot"
            ),
            Self::DuplicateAccount { account_id } => write!(
                formatter,
                "program account {account_id:02x?} is duplicated in the account snapshot"
            ),
            Self::AccountMismatch { account_id } => write!(
                formatter,
                "program account {account_id:02x?} does not match its durable binding"
            ),
            Self::ProofTooDeep => {
                formatter.write_str("account-state proof exceeds the protocol depth bound")
            }
            Self::InvalidProof => formatter.write_str("account-state proof shape is non-canonical"),
            Self::AccountRootMismatch => {
                formatter.write_str("account leaf is not included in the canonical account root")
            }
            Self::UniversalRootMismatch => {
                formatter.write_str("account root is not included in the universal state subtree")
            }
            Self::StateRootMismatch => {
                formatter.write_str("universal subtree is not included in the receipt state root")
            }
            Self::UnverifiedReceipt => {
                formatter.write_str("account snapshot receipt binding is unverified")
            }
        }
    }
}

impl std::error::Error for AccountStateError {}

impl ProgramValueAccountBinding {
    pub(crate) fn validate(&self) -> Result<(), AccountStateError> {
        let derived = derive_program_account(self.program, &self.seed)
            .map_err(|_| AccountStateError::InvalidBinding)?;
        if self.record_version != 2
            || !derived.matches(&self.account_id)
            || self.asset_id == [0; 32]
            || self.registered_sequence == 0
            || self.registration_event_digest != program_account_registration_commitment(self)
        {
            return Err(AccountStateError::InvalidBinding);
        }
        Ok(())
    }

    /// Returns the exact Programs-module primary-index key.
    #[must_use]
    pub fn primary_key(&self) -> Vec<u8> {
        let mut key = Vec::with_capacity(PROGRAM_ACCOUNT_PRIMARY_PREFIX.len() + 64);
        key.extend_from_slice(PROGRAM_ACCOUNT_PRIMARY_PREFIX);
        key.extend_from_slice(&self.program.bytes());
        key.extend_from_slice(&sha256(&self.seed));
        key
    }

    /// Returns the exact Programs-module version-two primary-index value.
    ///
    /// # Errors
    ///
    /// Refuses a non-canonical registration binding.
    pub fn primary_value(&self) -> Result<Vec<u8>, AccountStateError> {
        self.validate()?;
        let seed_length =
            u16::try_from(self.seed.len()).map_err(|_| AccountStateError::InvalidBinding)?;
        let mut value = Vec::with_capacity(139 + self.seed.len());
        value.push(self.record_version);
        value.extend_from_slice(&self.program.bytes());
        value.extend_from_slice(&self.account_id);
        value.extend_from_slice(&self.asset_id);
        value.extend_from_slice(&seed_length.to_be_bytes());
        value.extend_from_slice(&self.registered_sequence.to_be_bytes());
        value.extend_from_slice(&self.registration_event_digest);
        value.extend_from_slice(&self.seed);
        Ok(value)
    }

    /// Computes the exact Programs-module primary-record leaf commitment.
    ///
    /// # Errors
    ///
    /// Refuses a non-canonical version-two registration record.
    pub fn primary_commitment(&self) -> Result<[u8; 32], AccountStateError> {
        Ok(state_leaf_hash(&self.primary_key(), &self.primary_value()?))
    }
}

/// Computes the exact digest committed by the C ABI-two account-registration
/// event and version-two Programs primary record.
#[must_use]
pub fn program_account_registration_commitment(binding: &ProgramValueAccountBinding) -> [u8; 32] {
    let seed_digest = sha256(&binding.seed);
    let seed_length = u16::try_from(binding.seed.len()).unwrap_or(u16::MAX);
    let mut event = Vec::with_capacity(143);
    event.extend_from_slice(b"LXPA1");
    event.extend_from_slice(&binding.program.bytes());
    event.extend_from_slice(&binding.account_id);
    event.extend_from_slice(&binding.asset_id);
    event.extend_from_slice(&seed_length.to_be_bytes());
    event.extend_from_slice(&seed_digest);
    event.extend_from_slice(&binding.registered_sequence.to_be_bytes());
    sha256(&event)
}

impl CanonicalAccountLeaf {
    fn validate_module_value(&self) -> Result<(), AccountStateError> {
        if self.account_id == [0; 32]
            || self.asset_id == [0; 32]
            || !self.has_asset
            || self.kind != MODULE_VALUE_KIND
            || self.name != program_account_name(self.account_id)
            || self.name.len() > MAX_ACCOUNT_NAME_BYTES
            || self.has_authority_key
            || self.authority_key != [0; 32]
        {
            return Err(AccountStateError::InvalidAccountLeaf);
        }
        Ok(())
    }

    fn key(&self) -> [u8; 33] {
        let mut key = [0_u8; 33];
        key[0] = 4;
        key[1..].copy_from_slice(&self.account_id);
        key
    }

    fn value(&self) -> Result<Vec<u8>, AccountStateError> {
        self.validate_module_value()?;
        let name_length =
            u16::try_from(self.name.len()).map_err(|_| AccountStateError::InvalidAccountLeaf)?;
        let mut value = Vec::with_capacity(103 + self.name.len());
        value.extend_from_slice(&name_length.to_be_bytes());
        value.extend_from_slice(&self.name);
        value.push(self.kind);
        value.extend_from_slice(&self.balance.to_be_bytes());
        value.extend_from_slice(&self.asset_id);
        value.push(u8::from(self.has_asset));
        value.extend_from_slice(&self.next_sequence.to_be_bytes());
        value.extend_from_slice(&self.created_at_sequence.to_be_bytes());
        value.push(u8::from(self.frozen));
        value.push(u8::from(self.has_open_reference));
        value.extend_from_slice(&self.authority_key);
        value.push(u8::from(self.has_authority_key));
        Ok(value)
    }

    /// Computes the exact leaf commitment used by the C account tree.
    ///
    /// # Errors
    ///
    /// Refuses a record which is not a canonical `MODULE_VALUE` account.
    pub fn commitment(&self) -> Result<[u8; 32], AccountStateError> {
        Ok(state_leaf_hash(&self.key(), &self.value()?))
    }
}

impl VerifiedAccountSnapshot {
    /// Verifies the receipt, both outer account-tree inclusions and every
    /// supplied account leaf before returning any balance.
    ///
    /// # Errors
    ///
    /// Refuses legacy, stale, duplicate, malformed or root-mismatched evidence.
    pub fn verify(
        &self,
        authority: &JournalAccountStateAuthority<impl AccountStateJournal>,
    ) -> Result<(), AccountStateError> {
        self.verify_with_head(authority, true)
    }

    pub fn verify_historical(
        &self,
        authority: &JournalAccountStateAuthority<impl AccountStateJournal>,
    ) -> Result<(), AccountStateError> {
        self.verify_with_head(authority, false)
    }

    fn verify_with_head(
        &self,
        authority: &JournalAccountStateAuthority<impl AccountStateJournal>,
        current: bool,
    ) -> Result<(), AccountStateError> {
        if self.protocol_version != PROTOCOL_VERSION_ACCOUNT_TREE {
            return Err(AccountStateError::LegacyProtocol);
        }
        if self.receipt_digest == [0; 32]
            || self.state_root == [0; 32]
            || self.universal_root == [0; 32]
            || self.programs_root == [0; 32]
            || self.account_root == [0; 32]
        {
            return Err(AccountStateError::UnverifiedReceipt);
        }
        if self.freshness.observed_sequence == 0 || self.freshness.observed_at == 0 {
            return Err(AccountStateError::InvalidFreshness);
        }
        authority.verify(self, current)?;
        let account_tree_leaf = state_leaf_hash(ACCOUNT_TREE_KEY, &self.account_root);
        verify_state_proof(
            account_tree_leaf,
            &self.account_tree_proof,
            self.universal_root,
        )
        .map_err(|_| AccountStateError::UniversalRootMismatch)?;
        let universal_leaf = state_leaf_hash(&0_u16.to_be_bytes(), &self.universal_root);
        verify_state_proof(universal_leaf, &self.universal_root_proof, self.state_root)
            .map_err(|_| AccountStateError::StateRootMismatch)?;
        let programs_leaf = state_leaf_hash(&PROGRAMS_MODULE_ID.to_be_bytes(), &self.programs_root);
        verify_state_proof(programs_leaf, &self.programs_root_proof, self.state_root)
            .map_err(|_| AccountStateError::StateRootMismatch)?;
        if self.bindings.len() > MAX_PROGRAM_VALUE_ACCOUNTS {
            return Err(AccountStateError::InvalidProof);
        }
        let mut prior_binding_key: Option<Vec<u8>> = None;
        for binding in &self.bindings {
            binding.binding.validate()?;
            let key = binding.binding.primary_key();
            if prior_binding_key
                .as_ref()
                .is_some_and(|prior| prior >= &key)
            {
                return Err(AccountStateError::InvalidProof);
            }
            verify_state_proof(
                binding.binding.primary_commitment()?,
                &binding.proof,
                self.programs_root,
            )
            .map_err(|_| AccountStateError::InvalidProof)?;
            prior_binding_key = Some(key);
        }
        if self.accounts.len() > MAX_PROGRAM_VALUE_ACCOUNTS {
            return Err(AccountStateError::InvalidProof);
        }
        let mut prior = None;
        for account in &self.accounts {
            account.leaf.validate_module_value()?;
            if let Some(previous) = prior {
                if previous >= account.leaf.account_id {
                    return Err(if previous == account.leaf.account_id {
                        AccountStateError::DuplicateAccount {
                            account_id: previous,
                        }
                    } else {
                        AccountStateError::InvalidProof
                    });
                }
            }
            let leaf_hash = account.leaf.commitment()?;
            verify_state_proof(leaf_hash, &account.proof, self.account_root)
                .map_err(|_| AccountStateError::AccountRootMismatch)?;
            prior = Some(account.leaf.account_id);
        }
        Ok(())
    }

    /// Resolves every durable registry binding to an independently proven live
    /// protocol balance. The snapshot must contain exactly one proof for every
    /// binding and cannot introduce an unregistered program account.
    ///
    /// # Errors
    ///
    /// Refuses missing, extra, mismatched or unverified account state.
    pub fn resolve_program(
        &self,
        program: ProgramId,
        bindings: &[ProgramValueAccountBinding],
        authority: &JournalAccountStateAuthority<impl AccountStateJournal>,
    ) -> Result<Vec<ValueAccount>, AccountStateError> {
        self.resolve_program_with_head(program, bindings, authority, true)
    }

    pub fn resolve_program_historical(
        &self,
        program: ProgramId,
        bindings: &[ProgramValueAccountBinding],
        authority: &JournalAccountStateAuthority<impl AccountStateJournal>,
    ) -> Result<Vec<ValueAccount>, AccountStateError> {
        self.resolve_program_with_head(program, bindings, authority, false)
    }

    fn resolve_program_with_head(
        &self,
        program: ProgramId,
        bindings: &[ProgramValueAccountBinding],
        authority: &JournalAccountStateAuthority<impl AccountStateJournal>,
        current: bool,
    ) -> Result<Vec<ValueAccount>, AccountStateError> {
        self.verify_with_head(authority, current)?;
        if bindings.len() != self.accounts.len() || bindings.len() != self.bindings.len() {
            let missing = bindings
                .iter()
                .find(|binding| {
                    !self
                        .accounts
                        .iter()
                        .any(|account| account.leaf.account_id == binding.account_id)
                })
                .map_or([0; 32], |binding| binding.account_id);
            return Err(AccountStateError::MissingAccount {
                account_id: missing,
            });
        }
        for binding in bindings {
            if !self
                .bindings
                .iter()
                .any(|proven| proven.binding == *binding)
            {
                return Err(AccountStateError::InvalidBinding);
            }
        }
        let mut resolved = Vec::with_capacity(bindings.len());
        for binding in bindings {
            binding.validate()?;
            if binding.program != program {
                return Err(AccountStateError::InvalidBinding);
            }
            let account = self
                .accounts
                .iter()
                .find(|account| account.leaf.account_id == binding.account_id)
                .ok_or(AccountStateError::MissingAccount {
                    account_id: binding.account_id,
                })?;
            if account.leaf.asset_id != binding.asset_id
                || account.leaf.created_at_sequence != binding.registered_sequence
            {
                return Err(AccountStateError::AccountMismatch {
                    account_id: binding.account_id,
                });
            }
            resolved.push(ValueAccount {
                seed: binding.seed.clone(),
                account_id: binding.account_id,
                asset_id: binding.asset_id,
                balance: account.leaf.balance,
                frozen: account.leaf.frozen,
                observed_sequence: self.freshness.observed_sequence,
                receipt_digest: self.receipt_digest,
                state_root: self.state_root,
            });
        }
        resolved.sort_by_key(|account| account.account_id);
        Ok(resolved)
    }
}

fn program_account_name(account_id: [u8; 32]) -> Vec<u8> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut name = Vec::with_capacity(PROGRAM_ACCOUNT_NAME_PREFIX.len() + 64);
    name.extend_from_slice(PROGRAM_ACCOUNT_NAME_PREFIX);
    for byte in account_id {
        name.push(HEX[usize::from(byte >> 4)]);
        name.push(HEX[usize::from(byte & 0x0f)]);
    }
    name
}

fn state_leaf_hash(key: &[u8], value: &[u8]) -> [u8; 32] {
    let mut material = Vec::with_capacity(STATE_LEAF_DOMAIN.len() + 8 + key.len() + value.len());
    material.extend_from_slice(STATE_LEAF_DOMAIN);
    material.extend_from_slice(&(key.len() as u32).to_be_bytes());
    material.extend_from_slice(&(value.len() as u32).to_be_bytes());
    material.extend_from_slice(key);
    material.extend_from_slice(value);
    sha256(&material)
}

/// Computes a canonical state leaf from its exact key and value bytes.
#[must_use]
pub fn state_leaf_commitment(key: &[u8], value: &[u8]) -> [u8; 32] {
    state_leaf_hash(key, value)
}

/// Verifies one exact key/value membership witness under a canonical protocol
/// state-tree root.
///
/// # Errors
///
/// Refuses malformed proof geometry, an excessive path, or a root mismatch.
pub fn verify_state_membership(
    key: &[u8],
    value: &[u8],
    proof: &StateProof,
    expected_root: [u8; 32],
) -> Result<(), AccountStateError> {
    verify_state_proof(state_leaf_hash(key, value), proof, expected_root)
}

fn state_node_hash(left: [u8; 32], right: [u8; 32]) -> [u8; 32] {
    let mut material = Vec::with_capacity(STATE_NODE_DOMAIN.len() + 64);
    material.extend_from_slice(STATE_NODE_DOMAIN);
    material.extend_from_slice(&left);
    material.extend_from_slice(&right);
    sha256(&material)
}

/// Computes the protocol state-node commitment for two child hashes.
#[must_use]
pub fn state_node_commitment(left: [u8; 32], right: [u8; 32]) -> [u8; 32] {
    state_node_hash(left, right)
}

/// Computes the universal-subtree leaf which commits the account-tree root.
#[must_use]
pub fn account_tree_commitment(account_root: [u8; 32]) -> [u8; 32] {
    state_leaf_hash(ACCOUNT_TREE_KEY, &account_root)
}

/// Computes the outer state leaf for universal module id zero.
#[must_use]
pub fn universal_root_commitment(universal_root: [u8; 32]) -> [u8; 32] {
    state_leaf_hash(&0_u16.to_be_bytes(), &universal_root)
}

/// Computes the outer state leaf for the Programs module id.
#[must_use]
pub fn programs_root_commitment(programs_root: [u8; 32]) -> [u8; 32] {
    state_leaf_hash(&PROGRAMS_MODULE_ID.to_be_bytes(), &programs_root)
}

fn proof_depth(mut count: u32) -> usize {
    let mut depth = 0;
    while count > 1 {
        count = count.div_ceil(2);
        depth += 1;
    }
    depth
}

fn verify_state_proof(
    mut current: [u8; 32],
    proof: &StateProof,
    expected_root: [u8; 32],
) -> Result<(), AccountStateError> {
    if proof.leaf_count == 0
        || proof.leaf_index >= proof.leaf_count
        || proof.siblings.len() > MAX_PROOF_DEPTH
        || proof.siblings.len() != proof_depth(proof.leaf_count)
    {
        return Err(if proof.siblings.len() > MAX_PROOF_DEPTH {
            AccountStateError::ProofTooDeep
        } else {
            AccountStateError::InvalidProof
        });
    }
    let mut index = proof.leaf_index;
    let mut count = proof.leaf_count;
    for sibling in &proof.siblings {
        if (index ^ 1) >= count && sibling != &current {
            return Err(AccountStateError::InvalidProof);
        }
        current = if index & 1 == 0 {
            state_node_hash(current, *sibling)
        } else {
            state_node_hash(*sibling, current)
        };
        index /= 2;
        count = count.div_ceil(2);
    }
    if current == expected_root {
        Ok(())
    } else {
        Err(AccountStateError::InvalidProof)
    }
}

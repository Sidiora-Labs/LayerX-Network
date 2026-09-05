//! Canonical nested account-state verification under a signed batch root.

use sha2::{Digest, Sha256};

use layerx_wire::receipt::Receipt;

use crate::inclusion::{
    verify_header, verify_receipt, InclusionError, SequencerAuthorization, VerifiedBatchHeader,
};
use crate::merkle::{MerkleError, Proof, MAX_DEPTH};

const STATE_LEAF_DOMAIN: &[u8] = b"LXP/v1/state-leaf\0";
const STATE_NODE_DOMAIN: &[u8] = b"LXP/v1/state-node\0";
const ACCOUNT_IDENTIFIER_DOMAIN: &[u8] = b"LX:ACCOUNT:v1";
const ACCOUNT_TREE_KEY: &[u8] = b"account-tree";
const MAX_ACCOUNT_NAME_BYTES: usize = 512;
const MIN_ACCOUNT_VALUE_BYTES: usize = 103;

/// The asset an account holds together with its committed balance; an account
/// without an asset commits a zero asset identifier and a zero balance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccountAsset {
    pub asset_id: [u8; 32],
    pub balance: u128,
}

/// Exact canonical account value committed by `lx_account_registry_root`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalAccount {
    pub account_id: [u8; 32],
    pub name: Vec<u8>,
    pub kind: u8,
    pub asset: Option<AccountAsset>,
    pub next_sequence: u64,
    pub created_at_sequence: u64,
    pub frozen: bool,
    pub has_open_reference: bool,
    pub authority_key: Option<[u8; 32]>,
}

impl CanonicalAccount {
    /// Reports whether the account holds an asset.
    #[must_use]
    pub const fn has_asset(&self) -> bool {
        self.asset.is_some()
    }

    /// Returns the committed asset identifier, zero when no asset is held.
    #[must_use]
    pub const fn asset_id(&self) -> [u8; 32] {
        match self.asset {
            Some(asset) => asset.asset_id,
            None => [0; 32],
        }
    }

    /// Returns the committed balance, zero when no asset is held.
    #[must_use]
    pub const fn balance(&self) -> u128 {
        match self.asset {
            Some(asset) => asset.balance,
            None => 0,
        }
    }

    /// Reports whether an authority key is bound to the account.
    #[must_use]
    pub const fn has_authority_key(&self) -> bool {
        self.authority_key.is_some()
    }
}

/// Exact three-link account proof produced by the core state implementation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NestedAccountProof {
    pub account_id: [u8; 32],
    pub account_root: [u8; 32],
    pub universal_root: [u8; 32],
    pub resulting_state_root: [u8; 32],
    pub account_proof: Proof,
    pub account_tree_proof: Proof,
    pub universal_root_proof: Proof,
    pub receipt_bytes: Vec<u8>,
    pub receipt_proof: Proof,
    pub header_bytes: Vec<u8>,
    pub header_signature: [u8; 64],
}

/// A canonical account whose complete root chain and signed header verified.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedAccountState {
    account: CanonicalAccount,
    header: VerifiedBatchHeader,
    receipt_activity_id: [u8; 32],
    observed_sequence: u64,
    observed_at_ms: u64,
}

impl VerifiedAccountState {
    #[must_use]
    pub const fn account(&self) -> &CanonicalAccount {
        &self.account
    }

    #[must_use]
    pub const fn header(&self) -> &VerifiedBatchHeader {
        &self.header
    }

    #[must_use]
    pub const fn receipt_activity_id(&self) -> [u8; 32] {
        self.receipt_activity_id
    }

    #[must_use]
    pub const fn observed_sequence(&self) -> u64 {
        self.observed_sequence
    }

    #[must_use]
    pub const fn observed_at_ms(&self) -> u64 {
        self.observed_at_ms
    }
}

/// Exact refusal class for canonical account-state evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AccountProofError {
    AccountEncoding,
    AccountIdentity,
    AssetIdentity,
    AccountProof(MerkleError),
    AccountRoot,
    UniversalRoot,
    StateRoot,
    Header(InclusionError),
    ReceiptEncoding,
    ReceiptSignature,
    ReceiptBinding,
    Hash,
}

/// Decodes and validates the exact variable-length account value committed by C core.
///
/// # Errors
///
/// Refuses malformed names, account-kind disagreement, invalid boolean fields,
/// zero/nonzero asset and authority inconsistencies, trailing bytes, and an
/// account identifier that is not derived from the canonical account name.
pub fn decode_account_value(
    account_id: [u8; 32],
    bytes: &[u8],
) -> Result<CanonicalAccount, AccountProofError> {
    if bytes.len() < MIN_ACCOUNT_VALUE_BYTES {
        return Err(AccountProofError::AccountEncoding);
    }
    let mut reader = Reader::new(bytes);
    let name_length = usize::from(reader.u16()?);
    if name_length == 0 || name_length > MAX_ACCOUNT_NAME_BYTES {
        return Err(AccountProofError::AccountEncoding);
    }
    let name = reader.bytes(name_length)?.to_vec();
    let kind = reader.u8()?;
    if account_kind(&name) != Some(kind) || derive_account_id(&name)? != account_id {
        return Err(AccountProofError::AccountIdentity);
    }
    let balance = reader.u128()?;
    let asset_id = reader.array()?;
    let has_asset = reader.boolean()?;
    let next_sequence = reader.u64()?;
    let created_at_sequence = reader.u64()?;
    let frozen = reader.boolean()?;
    let has_open_reference = reader.boolean()?;
    let authority_key = reader.array()?;
    let has_authority_key = reader.boolean()?;
    reader.finish()?;
    if (!has_asset && (balance != 0 || asset_id != [0; 32]))
        || (has_asset && asset_id == [0; 32])
        || (!has_authority_key && authority_key != [0; 32])
        || (has_authority_key && authority_key == [0; 32])
    {
        return Err(AccountProofError::AccountEncoding);
    }
    Ok(CanonicalAccount {
        account_id,
        name,
        kind,
        asset: has_asset.then_some(AccountAsset { asset_id, balance }),
        next_sequence,
        created_at_sequence,
        frozen,
        has_open_reference,
        authority_key: has_authority_key.then_some(authority_key),
    })
}

/// Verifies account -> account-tree -> universal module -> signed global state.
///
/// # Errors
///
/// Every link is independently domain-separated and identity-bound. A proof
/// from the generic batch Merkle tree is therefore never accepted as a state
/// proof, and a valid account proof cannot be attached to a different account,
/// asset, intermediate root, signed header, or sequencer authority.
pub fn verify_nested_account(
    account_value: &[u8],
    expected_account: [u8; 32],
    expected_asset: Option<[u8; 32]>,
    proof: &NestedAccountProof,
    authorization: &SequencerAuthorization,
) -> Result<VerifiedAccountState, AccountProofError> {
    if proof.account_id != expected_account {
        return Err(AccountProofError::AccountIdentity);
    }
    let account = decode_account_value(expected_account, account_value)?;
    if expected_asset.is_some_and(|asset| account.asset.is_none_or(|held| held.asset_id != asset)) {
        return Err(AccountProofError::AssetIdentity);
    }
    let mut account_key = [0_u8; 33];
    account_key[0] = 4;
    account_key[1..].copy_from_slice(&expected_account);
    let account_leaf = state_leaf(&account_key, account_value)?;
    verify_state_path(account_leaf, &proof.account_proof, proof.account_root)
        .map_err(AccountProofError::AccountProof)?;

    let account_tree_leaf = state_leaf(ACCOUNT_TREE_KEY, &proof.account_root)?;
    verify_state_path(
        account_tree_leaf,
        &proof.account_tree_proof,
        proof.universal_root,
    )
    .map_err(|error| root_error(error, AccountProofError::UniversalRoot))?;

    let universal_leaf = state_leaf(&0_u16.to_be_bytes(), &proof.universal_root)?;
    verify_state_path(
        universal_leaf,
        &proof.universal_root_proof,
        proof.resulting_state_root,
    )
    .map_err(|error| root_error(error, AccountProofError::StateRoot))?;

    let header = verify_header(&proof.header_bytes, &proof.header_signature, authorization)
        .map_err(AccountProofError::Header)?;
    if header.header().resulting_state_root() != proof.resulting_state_root {
        return Err(AccountProofError::StateRoot);
    }
    verify_receipt(
        &proof.receipt_bytes,
        &proof.receipt_proof,
        &proof.header_bytes,
        &proof.header_signature,
        authorization,
    )
    .map_err(AccountProofError::Header)?;
    let receipt = crate::receipt::verify_sequencer_signature(
        &proof.receipt_bytes,
        authorization.public_key(),
    )
    .map_err(|_| AccountProofError::ReceiptSignature)?;
    let Receipt::Protocol(receipt) = receipt else {
        return Err(AccountProofError::ReceiptEncoding);
    };
    if receipt.global_sequence() != header.header().last_sequence()
        || receipt.resulting_state_root() != proof.resulting_state_root
        || receipt.timestamp() == 0
    {
        return Err(AccountProofError::ReceiptBinding);
    }
    Ok(VerifiedAccountState {
        account,
        header,
        receipt_activity_id: receipt.activity_id(),
        observed_sequence: receipt.global_sequence(),
        observed_at_ms: receipt.timestamp(),
    })
}

fn root_error(error: MerkleError, mismatch: AccountProofError) -> AccountProofError {
    match error {
        MerkleError::EmptyTree
        | MerkleError::LeafIndex { .. }
        | MerkleError::PathLength { .. }
        | MerkleError::PromotionSibling { .. }
        | MerkleError::TreeTooLarge
        | MerkleError::Encoding => AccountProofError::AccountProof(error),
        MerkleError::Hash | MerkleError::RootMismatch => mismatch,
    }
}

fn state_leaf(key: &[u8], value: &[u8]) -> Result<[u8; 32], AccountProofError> {
    let key_length = u32::try_from(key.len()).map_err(|_| AccountProofError::Hash)?;
    let value_length = u32::try_from(value.len()).map_err(|_| AccountProofError::Hash)?;
    let capacity = STATE_LEAF_DOMAIN
        .len()
        .checked_add(8)
        .and_then(|length| length.checked_add(key.len()))
        .and_then(|length| length.checked_add(value.len()))
        .ok_or(AccountProofError::Hash)?;
    let mut bytes = Vec::with_capacity(capacity);
    bytes.extend_from_slice(STATE_LEAF_DOMAIN);
    bytes.extend_from_slice(&key_length.to_be_bytes());
    bytes.extend_from_slice(&value_length.to_be_bytes());
    bytes.extend_from_slice(key);
    bytes.extend_from_slice(value);
    Ok(Sha256::digest(bytes).into())
}

fn state_node(left: [u8; 32], right: [u8; 32]) -> [u8; 32] {
    let mut bytes = [0_u8; STATE_NODE_DOMAIN.len() + 64];
    bytes[..STATE_NODE_DOMAIN.len()].copy_from_slice(STATE_NODE_DOMAIN);
    bytes[STATE_NODE_DOMAIN.len()..STATE_NODE_DOMAIN.len() + 32].copy_from_slice(&left);
    bytes[STATE_NODE_DOMAIN.len() + 32..].copy_from_slice(&right);
    Sha256::digest(bytes).into()
}

fn verify_state_path(
    mut current: [u8; 32],
    proof: &Proof,
    expected_root: [u8; 32],
) -> Result<(), MerkleError> {
    if proof.siblings().len() > MAX_DEPTH {
        return Err(MerkleError::PathLength {
            expected: MAX_DEPTH,
            actual: proof.siblings().len(),
        });
    }
    let mut index = proof.leaf_index();
    let mut count = proof.leaf_count();
    for (level, sibling) in proof.siblings().iter().enumerate() {
        if (index ^ 1) >= count && sibling != &current {
            return Err(MerkleError::PromotionSibling { level });
        }
        current = if index & 1 == 0 {
            state_node(current, *sibling)
        } else {
            state_node(*sibling, current)
        };
        index /= 2;
        count = count.div_ceil(2);
    }
    if current == expected_root {
        Ok(())
    } else {
        Err(MerkleError::RootMismatch)
    }
}

fn derive_account_id(name: &[u8]) -> Result<[u8; 32], AccountProofError> {
    if account_kind(name) == Some(13) {
        let encoded = name
            .get(name.len().saturating_sub(64)..)
            .ok_or(AccountProofError::AccountIdentity)?;
        let mut identifier = [0_u8; 32];
        for (index, pair) in encoded.chunks_exact(2).enumerate() {
            identifier[index] = (hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?;
        }
        return Ok(identifier);
    }
    let length = u32::try_from(name.len()).map_err(|_| AccountProofError::AccountIdentity)?;
    let mut bytes = Vec::with_capacity(ACCOUNT_IDENTIFIER_DOMAIN.len() + 4 + name.len());
    bytes.extend_from_slice(ACCOUNT_IDENTIFIER_DOMAIN);
    bytes.extend_from_slice(&length.to_be_bytes());
    bytes.extend_from_slice(name);
    Ok(Sha256::digest(bytes).into())
}

fn hex_nibble(byte: u8) -> Result<u8, AccountProofError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(AccountProofError::AccountIdentity),
    }
}

fn account_kind(name: &[u8]) -> Option<u8> {
    if !canonical_name(name) {
        return None;
    }
    if name == b"system:insurance" {
        Some(9)
    } else if name == b"system:fees" {
        Some(10)
    } else if name == b"system:paxeer-reserve" {
        Some(11)
    } else if name == b"system:paxeer-withdrawals" {
        Some(12)
    } else if system_tail(name, b"system:liquidity:") {
        Some(6)
    } else if system_funding(name, b":long") {
        Some(7)
    } else if system_funding(name, b":short") {
        Some(8)
    } else if name.starts_with(b"agent:") && name.len() > 11 && name.ends_with(b":main") {
        Some(1)
    } else if agent_shape(name, b":budget:") {
        Some(2)
    } else if agent_shape(name, b":escrow:") {
        Some(3)
    } else if agent_shape(name, b":stream:") {
        Some(4)
    } else if agent_shape(name, b":margin:") {
        Some(5)
    } else if module_value(name) {
        Some(13)
    } else {
        None
    }
}

fn canonical_name(name: &[u8]) -> bool {
    if name.is_empty() || name.len() > MAX_ACCOUNT_NAME_BYTES {
        return false;
    }
    let mut previous_colon = true;
    for byte in name {
        let valid = byte.is_ascii_lowercase()
            || byte.is_ascii_digit()
            || matches!(byte, b'.' | b'_' | b'-' | b':');
        if !valid || (*byte == b':' && previous_colon) {
            return false;
        }
        previous_colon = *byte == b':';
    }
    !previous_colon
}

fn agent_shape(name: &[u8], marker: &[u8]) -> bool {
    name.starts_with(b"agent:")
        && name.len() > 6 + marker.len()
        && name[6..]
            .windows(marker.len())
            .enumerate()
            .any(|(offset, value)| {
                value == marker
                    && offset > 0
                    && 6 + offset + marker.len() < name.len()
                    && !name[6 + offset + marker.len()..].contains(&b':')
            })
}

fn system_funding(name: &[u8], suffix: &[u8]) -> bool {
    const PREFIX: &[u8] = b"system:funding:";
    name.len() > PREFIX.len() + suffix.len()
        && name.starts_with(PREFIX)
        && name.ends_with(suffix)
        && !name[PREFIX.len()..name.len() - suffix.len()].contains(&b':')
}

fn system_tail(name: &[u8], prefix: &[u8]) -> bool {
    name.len() > prefix.len() && name.starts_with(prefix) && !name[prefix.len()..].contains(&b':')
}

fn module_value(name: &[u8]) -> bool {
    const PREFIX: &[u8] = b"module:";
    const MARKER: &[u8] = b":value:";
    const IDENTIFIER_BYTES: usize = 64;
    if name.len() <= PREFIX.len() + MARKER.len() + IDENTIFIER_BYTES
        || !name.starts_with(PREFIX)
        || &name[name.len() - IDENTIFIER_BYTES - MARKER.len()..name.len() - IDENTIFIER_BYTES]
            != MARKER
    {
        return false;
    }
    let module = &name[PREFIX.len()..name.len() - IDENTIFIER_BYTES - MARKER.len()];
    let identifier = &name[name.len() - IDENTIFIER_BYTES..];
    !module.is_empty()
        && module.len() <= 31
        && module
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
        && identifier
            .iter()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn bytes(&mut self, length: usize) -> Result<&'a [u8], AccountProofError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(AccountProofError::AccountEncoding)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(AccountProofError::AccountEncoding)?;
        self.offset = end;
        Ok(value)
    }

    fn array<const LENGTH: usize>(&mut self) -> Result<[u8; LENGTH], AccountProofError> {
        self.bytes(LENGTH)?
            .try_into()
            .map_err(|_| AccountProofError::AccountEncoding)
    }

    fn u8(&mut self) -> Result<u8, AccountProofError> {
        self.array().map(u8::from_be_bytes)
    }

    fn u16(&mut self) -> Result<u16, AccountProofError> {
        self.array().map(u16::from_be_bytes)
    }

    fn u64(&mut self) -> Result<u64, AccountProofError> {
        self.array().map(u64::from_be_bytes)
    }

    fn u128(&mut self) -> Result<u128, AccountProofError> {
        self.array().map(u128::from_be_bytes)
    }

    fn boolean(&mut self) -> Result<bool, AccountProofError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(AccountProofError::AccountEncoding),
        }
    }

    fn finish(self) -> Result<(), AccountProofError> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(AccountProofError::AccountEncoding)
        }
    }
}

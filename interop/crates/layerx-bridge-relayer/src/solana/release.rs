//! Releases on Solana.
//!
//! A Paxeer burn addressed to Solana is paid out by the custody program's
//! release instruction. The attestation is the outbound attestation every
//! other destination verifies: keccak256 of the 185-byte outbound preimage,
//! signed with secp256k1 by at least threshold of the current attestors. The
//! program carries no elliptic-curve code, so the signatures travel in the
//! native secp256k1 program's instruction placed directly before the release,
//! and every signature, address and message offset of that instruction
//! resolves inside its own data. The transaction's only signer is the ed25519
//! fee payer the remote signer holds.
//!
//! Addresses the program derives for itself are derived here the same way: a
//! program address is sha256 of its seeds, the program id and
//! `ProgramDerivedAddress`, and it must not be a point on the ed25519 curve.

use ed25519_dalek::VerifyingKey;
use k256::sha2::{Digest as _, Sha256};

use super::observe::{INSTRUCTION_MAGIC, INSTRUCTION_VERSION, LAYOUT_VERSION};
use super::rpc::MAX_TRANSACTION_BYTES;
use super::HANDLE_BYTES;
use crate::attestation::{OutboundAttestation, OUT_PREIMAGE_LENGTH};

/// The custody program's release operation.
pub const OP_RELEASE: u8 = 10;
/// Magic, version, operation, paxeerTxHash, paxeerNonce, recipient, amount.
pub const RELEASE_DATA_BYTES: usize = 4 + 2 + 1 + 32 + 8 + 32 + 8;
/// The exact length of the outbound preimage the attestors sign.
pub const OUTBOUND_PREIMAGE_BYTES: usize = OUT_PREIMAGE_LENGTH;
/// The width of one entry of the secp256k1 instruction's offsets table.
pub const SECP256K1_OFFSETS_BYTES: usize = 11;
/// A signature inside a secp256k1 instruction: r, s and the recovery id.
pub const SECP256K1_SIGNATURE_BYTES: usize = 65;
/// Blocks a blockhash stays usable after the block that produced it.
pub const MAX_PROCESSING_AGE: u64 = 150;

pub const CONFIG_SEED: &[u8] = b"config";
pub const VAULT_SEED: &[u8] = b"vault-authority";
pub const ASSET_SEED: &[u8] = b"asset";
pub const RECIPIENT_SEED: &[u8] = b"recipient";
pub const NULLIFIER_SEED: &[u8] = b"nullifier";

pub const CONFIG_MAGIC: &[u8; 8] = b"PXBRCFG0";
pub const RECIPIENT_MAGIC: &[u8; 8] = b"PXBRREC0";
/// The most attestors a config record holds.
pub const MAX_ATTESTORS: usize = 64;
/// Where the attestor table starts inside the config record.
pub const CONFIG_ATTESTORS_OFFSET: usize = 87;
pub const CONFIG_BYTES: usize = CONFIG_ATTESTORS_OFFSET + MAX_ATTESTORS * HANDLE_BYTES;
pub const RECIPIENT_BYTES: usize = 62;
/// An SPL token account.
pub const TOKEN_ACCOUNT_BYTES: usize = 165;
const TOKEN_ACCOUNT_STATE: usize = 108;
const TOKEN_ACCOUNT_INITIALIZED: u8 = 1;

/// `KeccakSecp256k11111111111111111111111111111`.
pub const SECP256K1_PROGRAM: [u8; 32] = [
    0x04, 0xc6, 0xfc, 0x20, 0xf0, 0x50, 0xcc, 0xf0, 0x55, 0x84, 0xd7, 0x21, 0x1c, 0x9f, 0x8c, 0xf5,
    0x9e, 0xc1, 0x47, 0x85, 0xbb, 0x16, 0x6a, 0x1e, 0x28, 0x30, 0xe8, 0x12, 0x20, 0x00, 0x00, 0x00,
];
/// `Sysvar1nstructions1111111111111111111111111`.
pub const INSTRUCTIONS_SYSVAR: [u8; 32] = [
    0x06, 0xa7, 0xd5, 0x17, 0x18, 0x7b, 0xd1, 0x66, 0x35, 0xda, 0xd4, 0x04, 0x55, 0xfd, 0xc2, 0xc0,
    0xc1, 0x24, 0xc6, 0x8f, 0x21, 0x56, 0x75, 0xa5, 0xdb, 0xba, 0xcb, 0x5f, 0x08, 0x00, 0x00, 0x00,
];
/// `TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA`.
pub const TOKEN_PROGRAM: [u8; 32] = [
    0x06, 0xdd, 0xf6, 0xe1, 0xd7, 0x65, 0xa1, 0x93, 0xd9, 0xcb, 0xe1, 0x46, 0xce, 0xeb, 0x79, 0xac,
    0x1c, 0xb4, 0x85, 0xed, 0x5f, 0x5b, 0x37, 0x91, 0x3a, 0x8c, 0xf5, 0x85, 0x7e, 0xff, 0x00, 0xa9,
];
/// `ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL`.
pub const ASSOCIATED_TOKEN_PROGRAM: [u8; 32] = [
    0x8c, 0x97, 0x25, 0x8f, 0x4e, 0x24, 0x89, 0xf1, 0xbb, 0x3d, 0x10, 0x29, 0x14, 0x8e, 0x0d, 0x83,
    0x0b, 0x5a, 0x13, 0x99, 0xda, 0xff, 0x10, 0x84, 0x04, 0x8e, 0x7b, 0xd8, 0xdb, 0xe9, 0xf8, 0x59,
];
/// `11111111111111111111111111111111`.
pub const SYSTEM_PROGRAM: [u8; 32] = [0; 32];

/// A release that cannot be expressed as a Solana transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReleaseError {
    /// No signatures, or more than one instruction's offsets can address.
    Signatures,
    /// A signature's `v` is not 27 or 28.
    RecoveryId,
    /// The amount does not fit a Solana token amount.
    Amount,
    /// The transaction exceeds the 1232-byte packet limit.
    Oversized { bytes: usize },
}

impl std::fmt::Display for ReleaseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Signatures => formatter.write_str("the release carries no usable signature set"),
            Self::RecoveryId => formatter.write_str("an attestor signature has no recovery id"),
            Self::Amount => formatter.write_str("the amount is not a Solana token amount"),
            Self::Oversized { bytes } => write!(
                formatter,
                "the release transaction needs {bytes} bytes, above {MAX_TRANSACTION_BYTES}"
            ),
        }
    }
}

impl std::error::Error for ReleaseError {}

fn on_curve(bytes: &[u8; 32]) -> bool {
    VerifyingKey::from_bytes(bytes).is_ok()
}

/// The program address `seeds` derive under `program_id`, or `None` when the
/// hash lands on the curve or a seed is longer than 32 bytes.
#[must_use]
pub fn create_program_address(seeds: &[&[u8]], program_id: &[u8; 32]) -> Option<[u8; 32]> {
    if seeds.len() > 16 || seeds.iter().any(|seed| seed.len() > 32) {
        return None;
    }
    let mut hasher = Sha256::new();
    for seed in seeds {
        hasher.update(seed);
    }
    hasher.update(program_id);
    hasher.update(b"ProgramDerivedAddress");
    let address: [u8; 32] = hasher.finalize().into();
    (!on_curve(&address)).then_some(address)
}

/// The first program address `seeds` derive with a bump from 255 down, and
/// that bump.
#[must_use]
pub fn find_program_address(seeds: &[&[u8]], program_id: &[u8; 32]) -> Option<([u8; 32], u8)> {
    (1..=u8::MAX).rev().find_map(|bump| {
        let bump_seed = [bump];
        let mut with_bump: Vec<&[u8]> = seeds.to_vec();
        with_bump.push(&bump_seed);
        create_program_address(&with_bump, program_id).map(|address| (address, bump))
    })
}

/// The associated token account of `wallet` for `mint`.
#[must_use]
pub fn associated_token_address(wallet: &[u8; 32], mint: &[u8; 32]) -> Option<[u8; 32]> {
    find_program_address(&[wallet, &TOKEN_PROGRAM, mint], &ASSOCIATED_TOKEN_PROGRAM)
        .map(|(address, _)| address)
}

/// The nullifier PDA of a burn.
#[must_use]
pub fn nullifier_address(program_id: &[u8; 32], nullifier: &[u8; 32]) -> Option<[u8; 32]> {
    find_program_address(&[NULLIFIER_SEED, nullifier], program_id).map(|(address, _)| address)
}

/// The recipient PDA of a 20-byte handle.
#[must_use]
pub fn recipient_address(program_id: &[u8; 32], handle: &[u8; HANDLE_BYTES]) -> Option<[u8; 32]> {
    find_program_address(&[RECIPIENT_SEED, handle], program_id).map(|(address, _)| address)
}

/// The asset PDA of a mint.
#[must_use]
pub fn asset_address(program_id: &[u8; 32], mint: &[u8; 32]) -> Option<[u8; 32]> {
    find_program_address(&[ASSET_SEED, mint], program_id).map(|(address, _)| address)
}

/// The config PDA.
#[must_use]
pub fn config_address(program_id: &[u8; 32]) -> Option<[u8; 32]> {
    find_program_address(&[CONFIG_SEED], program_id).map(|(address, _)| address)
}

/// The vault-authority PDA the config record's bump derives.
#[must_use]
pub fn vault_authority(program_id: &[u8; 32], bump: u8) -> Option<[u8; 32]> {
    create_program_address(&[VAULT_SEED, &[bump]], program_id)
}

fn be_u16(bytes: &[u8]) -> Option<u16> {
    bytes.try_into().ok().map(u16::from_be_bytes)
}

fn array<const N: usize>(bytes: &[u8]) -> Option<[u8; N]> {
    bytes.try_into().ok()
}

/// What a release needs from the config PDA.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigRecord {
    pub paused: bool,
    pub threshold: u8,
    pub vault_bump: u8,
    pub attestors: Vec<[u8; HANDLE_BYTES]>,
}

impl ConfigRecord {
    /// Decodes a config record; `None` for any other length, magic, layout
    /// version, flag or attestor count.
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != CONFIG_BYTES
            || &bytes[..8] != CONFIG_MAGIC
            || be_u16(&bytes[8..10])? != LAYOUT_VERSION
        {
            return None;
        }
        let paused = match bytes[74] {
            0 => false,
            1 => true,
            _ => return None,
        };
        let count = usize::from(bytes[75]);
        let threshold = bytes[76];
        if count > MAX_ATTESTORS || usize::from(threshold) > count {
            return None;
        }
        let attestors = (0..count)
            .map(|index| {
                let start = CONFIG_ATTESTORS_OFFSET + index * HANDLE_BYTES;
                array(&bytes[start..start + HANDLE_BYTES])
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self {
            paused,
            threshold,
            vault_bump: bytes[78],
            attestors,
        })
    }
}

/// The recipient PDA: the 32-byte key a 20-byte handle stands for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecipientRecord {
    pub handle: [u8; HANDLE_BYTES],
    pub key: [u8; 32],
}

impl RecipientRecord {
    /// Decodes a recipient record; `None` for any other length, magic or
    /// layout version.
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != RECIPIENT_BYTES
            || &bytes[..8] != RECIPIENT_MAGIC
            || be_u16(&bytes[8..10])? != LAYOUT_VERSION
        {
            return None;
        }
        Some(Self {
            handle: array(&bytes[10..30])?,
            key: array(&bytes[30..62])?,
        })
    }
}

/// Whether `data` is an initialised SPL token account of `mint` owned by
/// `owner`.
#[must_use]
pub fn is_token_account(data: &[u8], mint: &[u8; 32], owner: &[u8; 32]) -> bool {
    data.len() == TOKEN_ACCOUNT_BYTES
        && data[..32] == mint[..]
        && data[32..64] == owner[..]
        && data[TOKEN_ACCOUNT_STATE] == TOKEN_ACCOUNT_INITIALIZED
}

/// The accounts one release names, in the order the program reads them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReleaseAccounts {
    pub program_id: [u8; 32],
    pub fee_payer: [u8; 32],
    pub config: [u8; 32],
    pub asset: [u8; 32],
    pub mint: [u8; 32],
    pub vault_authority: [u8; 32],
    pub vault_token: [u8; 32],
    pub recipient_token: [u8; 32],
    pub nullifier: [u8; 32],
}

/// One release: the attested burn, the 32-byte key its recipient handle
/// stands for, and the attestor signatures `r || s || v` in ascending signer
/// order with their signers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Release {
    pub attestation: OutboundAttestation,
    pub recipient: [u8; 32],
    pub signatures: Vec<([u8; HANDLE_BYTES], [u8; 65])>,
    pub accounts: ReleaseAccounts,
}

/// The 185-byte outbound preimage of an attestation.
#[must_use]
pub fn outbound_preimage(attestation: &OutboundAttestation) -> [u8; OUTBOUND_PREIMAGE_BYTES] {
    attestation.preimage()
}

fn u16_offset(value: usize) -> Result<[u8; 2], ReleaseError> {
    u16::try_from(value)
        .map(u16::to_le_bytes)
        .map_err(|_| ReleaseError::Signatures)
}

/// The native secp256k1 instruction data verifying every signature over
/// `preimage`, every offset naming `own`, the instruction's own index: the
/// count, one offsets row per signature, then each signature with its
/// recovery id and signer, then the preimage once, shared by every row.
///
/// # Errors
///
/// Refuses an empty signature set, more signatures than the offsets can
/// address and a `v` other than 27 or 28.
pub fn secp256k1_instruction_data(
    signatures: &[([u8; HANDLE_BYTES], [u8; 65])],
    preimage: &[u8; OUTBOUND_PREIMAGE_BYTES],
    own: u8,
) -> Result<Vec<u8>, ReleaseError> {
    let count = u8::try_from(signatures.len()).map_err(|_| ReleaseError::Signatures)?;
    if count == 0 {
        return Err(ReleaseError::Signatures);
    }
    let entry = SECP256K1_SIGNATURE_BYTES + HANDLE_BYTES;
    let table = 1 + signatures.len() * SECP256K1_OFFSETS_BYTES;
    let message_offset = table + signatures.len() * entry;
    let mut data = Vec::with_capacity(message_offset + OUTBOUND_PREIMAGE_BYTES);
    data.push(count);
    for index in 0..signatures.len() {
        let signature_offset = table + index * entry;
        data.extend_from_slice(&u16_offset(signature_offset)?);
        data.push(own);
        data.extend_from_slice(&u16_offset(signature_offset + SECP256K1_SIGNATURE_BYTES)?);
        data.push(own);
        data.extend_from_slice(&u16_offset(message_offset)?);
        data.extend_from_slice(&u16_offset(OUTBOUND_PREIMAGE_BYTES)?);
        data.push(own);
    }
    for (signer, signature) in signatures {
        let recovery = signature[64]
            .checked_sub(27)
            .filter(|recovery| *recovery <= 1)
            .ok_or(ReleaseError::RecoveryId)?;
        data.extend_from_slice(&signature[..64]);
        data.push(recovery);
        data.extend_from_slice(signer);
    }
    u16_offset(message_offset + OUTBOUND_PREIMAGE_BYTES)?;
    data.extend_from_slice(preimage);
    Ok(data)
}

/// The custody program's release instruction data.
///
/// # Errors
///
/// Refuses an amount above `u64::MAX`.
pub fn release_instruction_data(
    attestation: &OutboundAttestation,
    recipient: &[u8; 32],
) -> Result<Vec<u8>, ReleaseError> {
    let amount = token_amount(&attestation.amount).ok_or(ReleaseError::Amount)?;
    let mut data = Vec::with_capacity(RELEASE_DATA_BYTES);
    data.extend_from_slice(INSTRUCTION_MAGIC);
    data.extend_from_slice(&INSTRUCTION_VERSION.to_be_bytes());
    data.push(OP_RELEASE);
    data.extend_from_slice(&attestation.paxeer_tx_hash);
    data.extend_from_slice(&attestation.paxeer_nonce.to_be_bytes());
    data.extend_from_slice(recipient);
    data.extend_from_slice(&amount.to_be_bytes());
    Ok(data)
}

/// A uint256 amount as a Solana token amount, when it fits.
#[must_use]
pub fn token_amount(amount: &[u8; 32]) -> Option<u64> {
    if amount[..24].iter().any(|byte| *byte != 0) {
        return None;
    }
    array(&amount[24..]).map(u64::from_be_bytes)
}

struct AccountMeta {
    key: [u8; 32],
    signer: bool,
    writable: bool,
}

struct CompiledInstruction {
    program: [u8; 32],
    accounts: Vec<AccountMeta>,
    data: Vec<u8>,
}

/// Appends Solana's compact length encoding of `value`.
pub fn shortvec(out: &mut Vec<u8>, value: usize) {
    let mut rest = value;
    loop {
        let low = (rest & 0x7f).to_le_bytes()[0];
        rest >>= 7;
        if rest == 0 {
            out.push(low);
            return;
        }
        out.push(low | 0x80);
    }
}

fn index_of(keys: &[[u8; 32]], key: &[u8; 32]) -> Result<u8, ReleaseError> {
    keys.iter()
        .position(|candidate| candidate == key)
        .and_then(|position| u8::try_from(position).ok())
        .ok_or(ReleaseError::Signatures)
}

/// A legacy message: the fee payer first, then writable signers, read-only
/// signers, writable accounts and read-only accounts, each in first-use order.
fn compile_message(
    fee_payer: &[u8; 32],
    instructions: &[CompiledInstruction],
    recent_blockhash: &[u8; 32],
) -> Result<Vec<u8>, ReleaseError> {
    let mut metas: Vec<AccountMeta> = vec![AccountMeta {
        key: *fee_payer,
        signer: true,
        writable: true,
    }];
    let mut add = |key: [u8; 32], signer: bool, writable: bool| {
        if let Some(existing) = metas.iter_mut().find(|meta| meta.key == key) {
            existing.signer |= signer;
            existing.writable |= writable;
        } else {
            metas.push(AccountMeta {
                key,
                signer,
                writable,
            });
        }
    };
    for instruction in instructions {
        for account in &instruction.accounts {
            add(account.key, account.signer, account.writable);
        }
        add(instruction.program, false, false);
    }
    let class = |meta: &AccountMeta| match (meta.signer, meta.writable) {
        (true, true) => 0,
        (true, false) => 1,
        (false, true) => 2,
        (false, false) => 3,
    };
    let payer = metas.remove(0);
    metas.sort_by_key(class);
    metas.insert(0, payer);
    let count = |wanted: fn(&AccountMeta) -> bool| {
        u8::try_from(metas.iter().filter(|meta| wanted(meta)).count())
            .map_err(|_| ReleaseError::Signatures)
    };
    let signers = count(|meta| meta.signer)?;
    let readonly_signed = count(|meta| meta.signer && !meta.writable)?;
    let readonly_unsigned = count(|meta| !meta.signer && !meta.writable)?;
    let keys: Vec<[u8; 32]> = metas.iter().map(|meta| meta.key).collect();
    let mut message = vec![signers, readonly_signed, readonly_unsigned];
    shortvec(&mut message, keys.len());
    for key in &keys {
        message.extend_from_slice(key);
    }
    message.extend_from_slice(recent_blockhash);
    shortvec(&mut message, instructions.len());
    for instruction in instructions {
        message.push(index_of(&keys, &instruction.program)?);
        shortvec(&mut message, instruction.accounts.len());
        for account in &instruction.accounts {
            message.push(index_of(&keys, &account.key)?);
        }
        shortvec(&mut message, instruction.data.len());
        message.extend_from_slice(&instruction.data);
    }
    Ok(message)
}

/// The wire transaction of a message whose only signer is the fee payer.
#[must_use]
pub fn wire_transaction(signature: &[u8; 64], message: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(1 + 64 + message.len());
    shortvec(&mut raw, 1);
    raw.extend_from_slice(signature);
    raw.extend_from_slice(message);
    raw
}

/// The message of a release transaction: the native secp256k1 instruction
/// verifying the attestor signatures over the outbound preimage at index 0,
/// directly followed by the custody program's release at index 1, paid for by
/// the fee payer against `recent_blockhash`. The fee payer signs these bytes.
///
/// # Errors
///
/// Refuses an unusable signature set, an amount above `u64::MAX` and a
/// transaction above the packet limit.
pub fn build_release_transaction(
    release: &Release,
    recent_blockhash: &[u8; 32],
) -> Result<Vec<u8>, ReleaseError> {
    let accounts = &release.accounts;
    let preimage = outbound_preimage(&release.attestation);
    let meta = |key: [u8; 32], writable: bool| AccountMeta {
        key,
        signer: false,
        writable,
    };
    let instructions = [
        CompiledInstruction {
            program: SECP256K1_PROGRAM,
            accounts: Vec::new(),
            data: secp256k1_instruction_data(&release.signatures, &preimage, 0)?,
        },
        CompiledInstruction {
            program: accounts.program_id,
            accounts: vec![
                AccountMeta {
                    key: accounts.fee_payer,
                    signer: true,
                    writable: true,
                },
                meta(accounts.config, false),
                meta(accounts.asset, true),
                meta(accounts.mint, false),
                meta(accounts.vault_authority, false),
                meta(accounts.vault_token, true),
                meta(accounts.recipient_token, true),
                meta(accounts.nullifier, true),
                meta(INSTRUCTIONS_SYSVAR, false),
                meta(TOKEN_PROGRAM, false),
                meta(SYSTEM_PROGRAM, false),
            ],
            data: release_instruction_data(&release.attestation, &release.recipient)?,
        },
    ];
    let message = compile_message(&accounts.fee_payer, &instructions, recent_blockhash)?;
    let bytes = 1 + 64 + message.len();
    if bytes > MAX_TRANSACTION_BYTES {
        return Err(ReleaseError::Oversized { bytes });
    }
    Ok(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attestation::uint256_from_u64;
    use crate::hex;
    use crate::solana::{base58_encode, base58_fixed, handle};

    fn key(text: &str) -> [u8; 32] {
        base58_fixed::<32>(text).unwrap_or_else(|error| panic!("{text}: {error}"))
    }

    #[test]
    fn program_ids_are_the_well_known_keys() {
        assert_eq!(
            base58_encode(&SECP256K1_PROGRAM),
            "KeccakSecp256k11111111111111111111111111111"
        );
        assert_eq!(
            base58_encode(&INSTRUCTIONS_SYSVAR),
            "Sysvar1nstructions1111111111111111111111111"
        );
        assert_eq!(
            base58_encode(&TOKEN_PROGRAM),
            "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
        );
        assert_eq!(
            base58_encode(&ASSOCIATED_TOKEN_PROGRAM),
            "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"
        );
        assert_eq!(
            base58_encode(&SYSTEM_PROGRAM),
            "11111111111111111111111111111111"
        );
    }

    #[test]
    fn program_addresses_match_the_pinned_vector() {
        let program = key("A7SZbByPYuHpunZ9pyMDMrhMYvK44ANT1AVqb8U1FpM9");
        let (authority, bump) = find_program_address(&[VAULT_SEED], &program)
            .unwrap_or_else(|| panic!("vault authority"));
        assert_eq!(
            base58_encode(&authority),
            "GxxA9Cs9v5pAGVsaCe2jjDrtmieeBijcY4S5HHTY8Vq6"
        );
        assert_eq!(bump, 255);
        assert_eq!(vault_authority(&program, bump), Some(authority));
        assert_eq!(
            hex::prefixed(&handle(&authority)),
            "0x334121a65b47bd45c3f6381537d9180e98e445bc"
        );
        let mint = key("5w3wVdJaESaJKyLmStM6Hv9UyUkmZ1b9DLQquAqqpump");
        assert_eq!(
            asset_address(&program, &mint).map(|address| base58_encode(&address)),
            Some("5XvCgGUysuU85ut4iTLDjiShTJWLzhqwJ3azMjPrknQ1".to_owned())
        );
        assert!(create_program_address(&[&[0_u8; 33]], &program).is_none());
    }

    #[test]
    fn shortvec_matches_the_compact_length_encoding() {
        for (value, encoded) in [
            (0_usize, vec![0_u8]),
            (0x7f, vec![0x7f]),
            (0x80, vec![0x80, 0x01]),
            (0x3fff, vec![0xff, 0x7f]),
            (0x4000, vec![0x80, 0x80, 0x01]),
        ] {
            let mut out = Vec::new();
            shortvec(&mut out, value);
            assert_eq!(out, encoded, "{value}");
        }
    }

    #[test]
    fn records_decode_only_their_own_layout() {
        let mut config = vec![0_u8; CONFIG_BYTES];
        config[..8].copy_from_slice(CONFIG_MAGIC);
        config[8..10].copy_from_slice(&LAYOUT_VERSION.to_be_bytes());
        config[75] = 2;
        config[76] = 1;
        config[78] = 254;
        config[87..107].fill(0x11);
        config[107..127].fill(0x22);
        assert_eq!(
            ConfigRecord::decode(&config),
            Some(ConfigRecord {
                paused: false,
                threshold: 1,
                vault_bump: 254,
                attestors: vec![[0x11; 20], [0x22; 20]],
            })
        );
        config[76] = 3;
        assert_eq!(ConfigRecord::decode(&config), None);
        config[76] = 1;
        config[74] = 2;
        assert_eq!(ConfigRecord::decode(&config), None);
        assert_eq!(ConfigRecord::decode(&config[1..]), None);

        let mut recipient = vec![0_u8; RECIPIENT_BYTES];
        recipient[..8].copy_from_slice(RECIPIENT_MAGIC);
        recipient[8..10].copy_from_slice(&LAYOUT_VERSION.to_be_bytes());
        recipient[10..30].fill(0x33);
        recipient[30..].fill(0x44);
        assert_eq!(
            RecipientRecord::decode(&recipient),
            Some(RecipientRecord {
                handle: [0x33; 20],
                key: [0x44; 32],
            })
        );
        recipient[0] = b'Q';
        assert_eq!(RecipientRecord::decode(&recipient), None);
    }

    #[test]
    fn amounts_above_u64_and_bad_recovery_ids_are_refused() {
        let mut amount = uint256_from_u64(u64::MAX);
        assert_eq!(token_amount(&amount), Some(u64::MAX));
        amount[23] = 1;
        assert_eq!(token_amount(&amount), None);
        let preimage = [0_u8; OUTBOUND_PREIMAGE_BYTES];
        assert_eq!(
            secp256k1_instruction_data(&[], &preimage, 0),
            Err(ReleaseError::Signatures)
        );
        assert_eq!(
            secp256k1_instruction_data(&[([0x11; 20], [0x01; 65])], &preimage, 0),
            Err(ReleaseError::RecoveryId)
        );
        let mut signature = [0x01; 65];
        signature[64] = 28;
        let data = secp256k1_instruction_data(&[([0x11; 20], signature)], &preimage, 0)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(data.len(), 1 + 11 + 65 + 20 + OUTBOUND_PREIMAGE_BYTES);
        assert_eq!(data[12 + 64], 1);
    }
}

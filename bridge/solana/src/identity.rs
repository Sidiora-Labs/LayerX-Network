//! The one place the Solana identity mapping lives.
//!
//! An attestation digest carries 20-byte handles where Solana carries 32-byte
//! keys, so a Solana key enters a digest as the last 20 bytes of keccak256 of
//! that key. The vault Paxeer registers for Solana is the handle of this
//! program's vault-authority PDA, and the default 20 bytes of a mint are the
//! handle of the mint. Every one of those derivations goes through this module,
//! so the mapping cannot be spelled two ways.

use solana_program::account_info::{next_account_info, AccountInfo};
use solana_program::entrypoint::ProgramResult;
use solana_program::keccak;
use solana_program::msg;
use solana_program::program_error::ProgramError;
use solana_program::pubkey::Pubkey;

use crate::state::{
    find_recipient_address, Config, RecipientRecord, RECIPIENT_BYTES, RECIPIENT_SEED, VAULT_SEED,
};
use crate::{create_pda, require_payer, require_signer, require_writable, BridgeError, Reader};

/// The width of a handle, and of every address inside an attestation preimage.
pub const HANDLE_BYTES: usize = 20;

/// Solana's chain id on the Paxeer side: the ASCII bytes of SOLANA, left-padded
/// to eight bytes and read big-endian, which is 0x0000534f4c414e41. It sits far
/// above every EIP-155 chain id in use, so it cannot collide with one.
pub const SOLANA_CHAIN_ID: u64 = 91_600_046_870_081;

/// Sidiora's mint on Solana. It is a classic SPL Token mint, so the deposit's
/// spl-token CPI moves it like any other registered asset.
pub const SIDIORA_MINT: Pubkey =
    Pubkey::from_str_const("5w3wVdJaESaJKyLmStM6Hv9UyUkmZ1b9DLQquAqqpump");

/// The fixed 20 bytes Paxeer already maps to Sidiora's denom. Sidiora is
/// registered under this id rather than under the handle of its mint.
pub const SIDIORA_ASSET_ID: [u8; HANDLE_BYTES] = [
    0x21, 0xf7, 0xb2, 0x0a, 0x55, 0x51, 0x99, 0xfa, 0x73, 0xa2, 0x38, 0xb1, 0xa9, 0x1f, 0xd0, 0xf5,
    0x49, 0x06, 0x8f, 0xee,
];

/// The last 20 bytes of keccak256 of a 32-byte Solana key.
pub fn handle(key: &[u8; 32]) -> [u8; HANDLE_BYTES] {
    let digest = keccak::hash(key).to_bytes();
    let mut out = [0_u8; HANDLE_BYTES];
    out.copy_from_slice(&digest[32 - HANDLE_BYTES..]);
    out
}

/// The handle of a pubkey.
pub fn pubkey_handle(key: &Pubkey) -> [u8; HANDLE_BYTES] {
    handle(&key.to_bytes())
}

/// The vault-authority PDA and its bump. This account signs for every token
/// account the program holds custody in, and its handle is what governance
/// registers on Paxeer as Solana's vault.
pub fn find_vault_authority(program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[VAULT_SEED], program_id)
}

/// The vault-authority PDA that `bump` derives, which the config record stores
/// so the program does not search for it on every instruction.
pub fn vault_authority(program_id: &Pubkey, bump: u8) -> Result<Pubkey, ProgramError> {
    Pubkey::create_program_address(&[VAULT_SEED, &[bump]], program_id)
        .map_err(|_| BridgeError::Pda.into())
}

/// The handle of the vault-authority PDA: the 20 bytes that stand for this
/// program inside every attestation digest either side of the bridge signs.
pub fn vault_handle(program_id: &Pubkey) -> [u8; HANDLE_BYTES] {
    pubkey_handle(&find_vault_authority(program_id).0)
}

/// Lower-case hexadecimal, for the log records the relayer reads.
pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}

/// The 20-byte Paxeer address a 32-byte recipient field carries, refusing a
/// field whose high twelve bytes are not zero and one whose low twenty bytes
/// are, so a deposit can only name an address Paxeer can pay.
pub fn paxeer_address(recipient: &[u8; 32]) -> Result<[u8; HANDLE_BYTES], ProgramError> {
    let (high, low) = recipient.split_at(32 - HANDLE_BYTES);
    if high.iter().any(|byte| *byte != 0) || low.iter().all(|byte| *byte == 0) {
        return Err(BridgeError::Recipient.into());
    }
    let mut out = [0_u8; HANDLE_BYTES];
    out.copy_from_slice(low);
    Ok(out)
}

/// Refuse a registration that would give Sidiora's fixed asset id to any mint
/// but Sidiora's, or give Sidiora's mint any asset id but its fixed one. The
/// binding holds both ways, so no owner key can route the Paxeer side of
/// Sidiora through a mint that is not Sidiora, or register Sidiora under an id
/// Paxeer does not map to its denom.
pub fn require_sidiora_binding(mint: &Pubkey, asset_id: &[u8; HANDLE_BYTES]) -> ProgramResult {
    if (*mint == SIDIORA_MINT) != (*asset_id == SIDIORA_ASSET_ID) {
        return Err(BridgeError::Binding.into());
    }
    Ok(())
}

/// Record the 32-byte key a 20-byte handle stands for.
///
/// A transfer out of Paxeer names its Solana recipient by handle only, and a
/// handle cannot be turned back into the key it came from. So any Solana
/// account may publish its own key under its handle, once, and the relayer
/// pays a release to the key the record holds. The account signs, the handle
/// it names must be the handle of its own key, and the record sits at the PDA
/// that handle seeds; anyone may pay the rent for it.
pub fn register_recipient(
    program_id: &Pubkey,
    accounts: &[AccountInfo<'_>],
    reader: &mut Reader<'_>,
) -> ProgramResult {
    let declared = reader.array::<HANDLE_BYTES>()?;
    reader.finish()?;
    let mut accounts = accounts.iter();
    let payer = next_account_info(&mut accounts)?;
    let config_account = next_account_info(&mut accounts)?;
    let registrant = next_account_info(&mut accounts)?;
    let record_account = next_account_info(&mut accounts)?;
    let system = next_account_info(&mut accounts)?;
    require_payer(payer)?;
    require_signer(registrant)?;
    require_writable(record_account)?;
    let config = Config::load(config_account, program_id)?;
    config.require_unpaused()?;
    if pubkey_handle(registrant.key) != declared {
        return Err(BridgeError::Recipient.into());
    }
    let (expected, bump) = find_recipient_address(program_id, &declared);
    if record_account.key != &expected {
        return Err(BridgeError::Pda.into());
    }
    create_pda(
        payer,
        record_account,
        system,
        program_id,
        RECIPIENT_BYTES,
        &[RECIPIENT_SEED, &declared, &[bump]],
    )?;
    let record = RecipientRecord {
        handle: declared,
        key: *registrant.key,
    };
    record.store(record_account)?;
    msg!("{}", recipient_record(&record));
    Ok(())
}

/// The record a recipient registration logs: the handle a release is
/// addressed to and the key it pays.
pub fn recipient_record(record: &RecipientRecord) -> String {
    format!(
        "PXBR/register-recipient/v1 handle={} key={}",
        hex(&record.handle),
        record.key
    )
}

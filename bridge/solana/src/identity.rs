//! The one place the Solana identity mapping lives.
//!
//! An attestation digest carries 20-byte handles where Solana carries 32-byte
//! keys, so a Solana key enters a digest as the last 20 bytes of keccak256 of
//! that key. The vault Paxeer registers for Solana is the handle of this
//! program's vault-authority PDA, and the default 20 bytes of a mint are the
//! handle of the mint. Every one of those derivations goes through this module,
//! so the mapping cannot be spelled two ways.

use solana_program::keccak;
use solana_program::program_error::ProgramError;
use solana_program::pubkey::Pubkey;

use crate::state::VAULT_SEED;
use crate::BridgeError;

/// The width of a handle, and of every address inside an attestation preimage.
pub const HANDLE_BYTES: usize = 20;

/// Solana's chain id on the Paxeer side: the ASCII bytes of SOLANA, left-padded
/// to eight bytes and read big-endian, which is 0x0000534f4c414e41. It sits far
/// above every EIP-155 chain id in use, so it cannot collide with one.
pub const SOLANA_CHAIN_ID: u64 = 91_600_046_870_081;

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

//! The Solana recipient of a release.
//!
//! A transfer out of Paxeer names its Solana recipient by a 20-byte handle, the
//! attestors sign that handle, and a release names the full 32-byte pubkey the
//! handle was derived from. The program derives the handle of that pubkey
//! itself, so a release can pay only the key the attestors signed for, and it
//! pays only a token account that key owns. A handle is published against its
//! key through the permissionless recipient registration in the identity
//! module, which the relayer reads to learn the pubkey a handle stands for.

use solana_program::account_info::AccountInfo;
use solana_program::program_error::ProgramError;
use solana_program::program_pack::Pack;
use solana_program::pubkey::Pubkey;
use spl_token::state::Account as TokenAccount;

use crate::identity::{pubkey_handle, HANDLE_BYTES};
use crate::BridgeError;

/// The 20 bytes the outbound preimage carries for the pubkey a release pays:
/// the handle of that pubkey, derived by the one handle derivation this
/// program carries.
pub fn recipient_handle(recipient: &Pubkey) -> [u8; HANDLE_BYTES] {
    pubkey_handle(recipient)
}

/// Require `account` to be an SPL token account of `mint` owned by
/// `recipient`, so the release can pay no one but the key it names.
pub fn require_recipient_account(
    account: &AccountInfo<'_>,
    mint: &Pubkey,
    recipient: &Pubkey,
) -> Result<TokenAccount, ProgramError> {
    if account.owner != &spl_token::id() {
        return Err(BridgeError::Recipient.into());
    }
    let state =
        TokenAccount::unpack(&account.try_borrow_data()?).map_err(|_| BridgeError::Recipient)?;
    if &state.mint != mint || &state.owner != recipient {
        return Err(BridgeError::Recipient.into());
    }
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{handle, hex};
    use solana_program::keccak;

    #[test]
    fn the_recipient_handle_is_the_handle_of_its_key() {
        let key = keccak::hash(b"PAXEERX_BRIDGE_SOLANA_VECTOR_RECIPIENT").to_bytes();
        let recipient = Pubkey::new_from_array(key);
        assert_eq!(
            recipient.to_string(),
            "59TLtNdRpCZysEHkGDMPFHkHiHXkBqQVqAVxAupzNoQb"
        );
        assert_eq!(recipient_handle(&recipient), handle(&key));
        assert_eq!(
            hex(&recipient_handle(&recipient)),
            "fb02125a3275d53a9f6538626b49894d2aae80cc"
        );
    }
}

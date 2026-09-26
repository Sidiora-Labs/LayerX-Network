//! The release half of custody on Solana.
//!
//! A Paxeer burn of a bridged denom is attested by the attestors over the
//! outbound preimage, and a release pays the locked tokens out against that
//! attestation: it rebuilds the preimage from the Solana chain id, the vault
//! handle, the burn's paxeerTxHash and paxeerNonce, the handle of the recipient
//! pubkey it names, the asset id the registry holds and the amount; has the
//! native secp256k1 program's verification of the attestors' signatures read
//! back; creates the nullifier PDA the burn seeds, so the same burn can never
//! be paid twice; reduces the asset's outstanding amount; moves the tokens out
//! of the vault-authority PDA's token account by spl-token CPI; and logs the
//! record the relayer reads.

use solana_program::account_info::{next_account_info, AccountInfo};
use solana_program::clock::Clock;
use solana_program::entrypoint::ProgramResult;
use solana_program::keccak;
use solana_program::msg;
use solana_program::program::invoke_signed;
use solana_program::program_pack::Pack;
use solana_program::pubkey::Pubkey;
use solana_program::sysvar::Sysvar;
use spl_token::state::Account as TokenAccount;

use crate::attestation::{outbound_preimage, verify_attestation, Outbound};
use crate::identity::{hex, pubkey_handle, vault_authority, HANDLE_BYTES, SOLANA_CHAIN_ID};
use crate::recipient::{recipient_handle, require_recipient_account};
use crate::state::{
    find_nullifier_address, Asset, Config, NullifierRecord, NULLIFIER_BYTES, NULLIFIER_SEED,
    VAULT_SEED,
};
use crate::{create_pda, require_payer, require_writable, BridgeError, Reader};

/// The nullifier of a Paxeer burn: keccak256 of its paxeerTxHash followed by
/// its paxeerNonce as a big-endian uint64. It seeds the nullifier PDA.
pub fn nullifier(paxeer_tx_hash: &[u8; 32], paxeer_nonce: u64) -> [u8; 32] {
    keccak::hashv(&[paxeer_tx_hash, &paxeer_nonce.to_be_bytes()]).to_bytes()
}

/// Pay `amount` of a registered mint to the Solana `recipient` a Paxeer burn
/// attested, once.
pub fn release(
    program_id: &Pubkey,
    accounts: &[AccountInfo<'_>],
    reader: &mut Reader<'_>,
) -> ProgramResult {
    let paxeer_tx_hash = reader.array::<32>()?;
    let paxeer_nonce = reader.u64()?;
    let recipient = reader.pubkey()?;
    let amount = reader.u64()?;
    reader.finish()?;

    let mut accounts = accounts.iter();
    let payer = next_account_info(&mut accounts)?;
    let config_account = next_account_info(&mut accounts)?;
    let asset_account = next_account_info(&mut accounts)?;
    let mint_account = next_account_info(&mut accounts)?;
    let vault_account = next_account_info(&mut accounts)?;
    let vault_token = next_account_info(&mut accounts)?;
    let recipient_token = next_account_info(&mut accounts)?;
    let nullifier_account = next_account_info(&mut accounts)?;
    let instructions = next_account_info(&mut accounts)?;
    let token_program = next_account_info(&mut accounts)?;
    let system = next_account_info(&mut accounts)?;

    require_payer(payer)?;
    require_writable(asset_account)?;
    require_writable(vault_token)?;
    require_writable(recipient_token)?;
    require_writable(nullifier_account)?;

    let config = Config::load(config_account, program_id)?;
    config.require_unpaused()?;
    let burn = nullifier(&paxeer_tx_hash, paxeer_nonce);
    let (expected_nullifier, nullifier_bump) = find_nullifier_address(program_id, &burn);
    if nullifier_account.key != &expected_nullifier {
        return Err(BridgeError::Pda.into());
    }
    if nullifier_account.owner == program_id || !nullifier_account.data_is_empty() {
        return Err(BridgeError::Replayed.into());
    }

    let mut asset = Asset::load(asset_account, program_id)?;
    if !asset.enabled || &asset.mint != mint_account.key {
        return Err(BridgeError::Asset.into());
    }
    let outstanding = asset.withdraw(amount)?;

    if token_program.key != &spl_token::id() {
        return Err(BridgeError::Account.into());
    }
    let vault = vault_authority(program_id, config.vault_bump)?;
    if vault_account.key != &vault {
        return Err(BridgeError::Pda.into());
    }
    if vault_token.owner != &spl_token::id() {
        return Err(BridgeError::Account.into());
    }
    let vault_state = TokenAccount::unpack(&vault_token.try_borrow_data()?)?;
    if &vault_state.mint != mint_account.key || vault_state.owner != vault {
        return Err(BridgeError::Account.into());
    }
    let paid = require_recipient_account(recipient_token, mint_account.key, &recipient)?.amount;

    let recipient_20 = recipient_handle(&recipient);
    let preimage = outbound_preimage(&Outbound {
        chain_id: SOLANA_CHAIN_ID,
        vault: pubkey_handle(&vault),
        paxeer_tx_hash,
        paxeer_nonce,
        recipient: recipient_20,
        asset: asset.asset_id,
        amount,
    });
    verify_attestation(instructions, &preimage, &config.attestors, config.threshold)?;

    create_pda(
        payer,
        nullifier_account,
        system,
        program_id,
        NULLIFIER_BYTES,
        &[NULLIFIER_SEED, &burn, &[nullifier_bump]],
    )?;
    let record = NullifierRecord {
        paxeer_tx_hash,
        paxeer_nonce,
        mint: *mint_account.key,
        recipient,
        amount,
        slot: Clock::get()?.slot,
    };
    record.store(nullifier_account)?;

    invoke_signed(
        &spl_token::instruction::transfer_checked(
            token_program.key,
            vault_token.key,
            mint_account.key,
            recipient_token.key,
            vault_account.key,
            &[],
            amount,
            asset.decimals,
        )?,
        &[
            vault_token.clone(),
            mint_account.clone(),
            recipient_token.clone(),
            vault_account.clone(),
            token_program.clone(),
        ],
        &[&[VAULT_SEED, &[config.vault_bump]]],
    )?;

    let received = TokenAccount::unpack(&recipient_token.try_borrow_data()?)?
        .amount
        .checked_sub(paid)
        .ok_or(BridgeError::Account)?;
    if received != amount {
        return Err(BridgeError::Account.into());
    }

    asset.outstanding = outstanding;
    asset.store(asset_account)?;

    msg!(
        "{}",
        release_record(&record, &asset.asset_id, &recipient_20)
    );
    Ok(())
}

/// The record a release logs: the asset id and mint paid out, the amount, the
/// recipient pubkey and the handle the attestors signed for it, and the
/// paxeerTxHash and paxeerNonce of the burn it answers.
pub fn release_record(
    record: &NullifierRecord,
    asset_id: &[u8; HANDLE_BYTES],
    recipient: &[u8; HANDLE_BYTES],
) -> String {
    format!(
        "PXBR/release/v1 asset={} mint={} amount={} recipient={} handle={} paxeer_tx_hash={} paxeer_nonce={}",
        hex(asset_id),
        record.mint,
        record.amount,
        record.recipient,
        hex(recipient),
        hex(&record.paxeer_tx_hash),
        record.paxeer_nonce
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The nullifier bridge/ATTESTATION-SOLANA.md pins for its outbound vector.
    #[test]
    fn the_nullifier_is_the_pinned_vector() {
        let burn = keccak::hash(b"PAXEERX_BRIDGE_SOLANA_VECTOR_BURN").to_bytes();
        assert_eq!(
            hex(&nullifier(&burn, 11)),
            "d653f4968eb9b70e1eaef15fb134f2f7da8c15c38335af0069c94585e521ea3a"
        );
        assert_ne!(nullifier(&burn, 11), nullifier(&burn, 12));
    }
}

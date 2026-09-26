//! The lock half of custody on Solana.
//!
//! A deposit moves SPL tokens from the depositor's token account into the
//! vault-authority PDA's token account, admits the amount against the asset's
//! caps, takes the next deposit nonce, writes the receipt PDA that nonce seeds
//! and logs the record the relayer turns into an inbound attestation. The
//! program never learns its own transaction signature and does not need to: the
//! relayer computes the inbound txHash from the signature it observed and reads
//! the logIndex from this receipt.

use solana_program::account_info::{next_account_info, AccountInfo};
use solana_program::clock::Clock;
use solana_program::entrypoint::ProgramResult;
use solana_program::msg;
use solana_program::program::invoke;
use solana_program::program_pack::Pack;
use solana_program::pubkey::Pubkey;
use solana_program::sysvar::Sysvar;
use spl_token::state::Account as TokenAccount;

use crate::identity::{hex, paxeer_address, vault_authority, HANDLE_BYTES};
use crate::state::{
    find_receipt_address, Asset, Config, DepositReceipt, RECEIPT_BYTES, RECEIPT_SEED,
};
use crate::{create_pda, require_payer, require_writable, BridgeError, Reader};

/// Lock `amount` of a registered mint for the 20-byte Paxeer address the
/// 32-byte `paxeer_recipient` field carries.
pub fn deposit(
    program_id: &Pubkey,
    accounts: &[AccountInfo<'_>],
    reader: &mut Reader<'_>,
) -> ProgramResult {
    let amount = reader.u64()?;
    let paxeer_recipient = reader.array::<32>()?;
    reader.finish()?;
    let recipient = paxeer_address(&paxeer_recipient)?;

    let mut accounts = accounts.iter();
    let depositor = next_account_info(&mut accounts)?;
    let config_account = next_account_info(&mut accounts)?;
    let asset_account = next_account_info(&mut accounts)?;
    let mint_account = next_account_info(&mut accounts)?;
    let source = next_account_info(&mut accounts)?;
    let vault_token = next_account_info(&mut accounts)?;
    let receipt_account = next_account_info(&mut accounts)?;
    let token_program = next_account_info(&mut accounts)?;
    let system = next_account_info(&mut accounts)?;

    require_payer(depositor)?;
    require_writable(config_account)?;
    require_writable(asset_account)?;
    require_writable(source)?;
    require_writable(vault_token)?;
    require_writable(receipt_account)?;

    let mut config = Config::load(config_account, program_id)?;
    config.require_unpaused()?;
    let mut asset = Asset::load(asset_account, program_id)?;
    if !asset.enabled || &asset.mint != mint_account.key {
        return Err(BridgeError::Asset.into());
    }
    let outstanding = asset.admit(amount)?;

    if token_program.key != &spl_token::id() {
        return Err(BridgeError::Account.into());
    }
    let vault = vault_authority(program_id, config.vault_bump)?;
    let source_state = TokenAccount::unpack(&source.try_borrow_data()?)?;
    let vault_state = TokenAccount::unpack(&vault_token.try_borrow_data()?)?;
    if &source_state.mint != mint_account.key
        || &vault_state.mint != mint_account.key
        || vault_state.owner != vault
    {
        return Err(BridgeError::Account.into());
    }
    let held = vault_state.amount;

    let nonce = config
        .deposit_nonce
        .checked_add(1)
        .ok_or(BridgeError::Bounds)?;
    let (expected_receipt, receipt_bump) = find_receipt_address(program_id, nonce);
    if receipt_account.key != &expected_receipt {
        return Err(BridgeError::Pda.into());
    }

    invoke(
        &spl_token::instruction::transfer_checked(
            token_program.key,
            source.key,
            mint_account.key,
            vault_token.key,
            depositor.key,
            &[],
            amount,
            asset.decimals,
        )?,
        &[
            source.clone(),
            mint_account.clone(),
            vault_token.clone(),
            depositor.clone(),
            token_program.clone(),
        ],
    )?;

    let received = TokenAccount::unpack(&vault_token.try_borrow_data()?)?
        .amount
        .checked_sub(held)
        .ok_or(BridgeError::Account)?;
    if received != amount {
        return Err(BridgeError::Account.into());
    }

    create_pda(
        depositor,
        receipt_account,
        system,
        program_id,
        RECEIPT_BYTES,
        &[RECEIPT_SEED, &nonce.to_be_bytes(), &[receipt_bump]],
    )?;
    let receipt = DepositReceipt {
        nonce,
        mint: *mint_account.key,
        amount,
        paxeer_recipient,
        depositor: *depositor.key,
        slot: Clock::get()?.slot,
    };
    receipt.store(receipt_account)?;

    asset.outstanding = outstanding;
    asset.store(asset_account)?;
    config.deposit_nonce = nonce;
    config.store(config_account)?;

    msg!("{}", deposit_record(&receipt, &asset.asset_id, &recipient));
    Ok(())
}

/// The record a deposit logs, and the one the relayer turns into an inbound
/// attestation: the nonce it reads as the logIndex, the mint, the 20 bytes the
/// mint occupies in the digest, the amount, the Paxeer address the funds are
/// destined for, the depositor and the slot.
pub fn deposit_record(
    receipt: &DepositReceipt,
    asset_id: &[u8; HANDLE_BYTES],
    recipient: &[u8; HANDLE_BYTES],
) -> String {
    format!(
        "PXBR/deposit/v1 nonce={} mint={} asset={} amount={} recipient={} depositor={} slot={}",
        receipt.nonce,
        receipt.mint,
        hex(asset_id),
        receipt.amount,
        hex(recipient),
        receipt.depositor,
        receipt.slot
    )
}

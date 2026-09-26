//! The owner's instructions: initialisation, the two-step ownership transfer,
//! the shared attestor set, the asset registry, the caps and the pause.
//!
//! Every instruction here is the owner's alone, refuses any other signer, and is
//! refused while custody is paused. The single exception is the unpause, which
//! is the only way out of a pause.

use solana_program::account_info::{next_account_info, AccountInfo};
use solana_program::entrypoint::ProgramResult;
use solana_program::msg;
use solana_program::program_pack::Pack;
use solana_program::pubkey::Pubkey;
use spl_token::state::Mint;

use crate::identity::{find_vault_authority, hex, pubkey_handle, vault_handle, HANDLE_BYTES};
use crate::state::{
    find_asset_address, find_config_address, Asset, Config, ASSET_BYTES, ASSET_SEED, CONFIG_BYTES,
    CONFIG_SEED, MAX_ATTESTORS,
};
use crate::{create_pda, require_payer, require_signer, require_writable, BridgeError, Reader};

/// Create the config PDA and record the owner the deployment names. The account
/// that pays signs; the owner it installs is the one every later instruction
/// answers to.
pub fn initialise(
    program_id: &Pubkey,
    accounts: &[AccountInfo<'_>],
    reader: &mut Reader<'_>,
) -> ProgramResult {
    let owner = reader.pubkey()?;
    reader.finish()?;
    if owner == Pubkey::default() {
        return Err(BridgeError::Authority.into());
    }
    let mut accounts = accounts.iter();
    let payer = next_account_info(&mut accounts)?;
    let config_account = next_account_info(&mut accounts)?;
    let system = next_account_info(&mut accounts)?;
    require_payer(payer)?;
    require_writable(config_account)?;
    let (expected, config_bump) = find_config_address(program_id);
    if config_account.key != &expected {
        return Err(BridgeError::Pda.into());
    }
    let (vault, vault_bump) = find_vault_authority(program_id);
    create_pda(
        payer,
        config_account,
        system,
        program_id,
        CONFIG_BYTES,
        &[CONFIG_SEED, &[config_bump]],
    )?;
    let config = Config {
        owner,
        pending_owner: Pubkey::default(),
        paused: false,
        threshold: 0,
        config_bump,
        vault_bump,
        deposit_nonce: 0,
        attestors: Vec::new(),
    };
    config.store(config_account)?;
    msg!(
        "PXBR/initialise/v1 owner={} vault={} vault_handle={}",
        owner,
        vault,
        hex(&vault_handle(program_id))
    );
    Ok(())
}

/// Name the account that may take ownership. Nothing changes hands here: the
/// named account must accept in a second transaction.
pub fn propose_owner(
    program_id: &Pubkey,
    accounts: &[AccountInfo<'_>],
    reader: &mut Reader<'_>,
) -> ProgramResult {
    let pending_owner = reader.pubkey()?;
    reader.finish()?;
    if pending_owner == Pubkey::default() {
        return Err(BridgeError::Authority.into());
    }
    let mut accounts = accounts.iter();
    let owner = next_account_info(&mut accounts)?;
    let config_account = next_account_info(&mut accounts)?;
    require_writable(config_account)?;
    let mut config = Config::load(config_account, program_id)?;
    config.require_owner(owner)?;
    config.require_unpaused()?;
    config.pending_owner = pending_owner;
    config.store(config_account)?;
    msg!(
        "PXBR/propose-owner/v1 owner={} pending={}",
        config.owner,
        pending_owner
    );
    Ok(())
}

/// Take ownership. Only the account the owner named can do this, and doing it
/// clears the proposal.
pub fn accept_owner(
    program_id: &Pubkey,
    accounts: &[AccountInfo<'_>],
    reader: &mut Reader<'_>,
) -> ProgramResult {
    reader.finish()?;
    let mut accounts = accounts.iter();
    let pending_owner = next_account_info(&mut accounts)?;
    let config_account = next_account_info(&mut accounts)?;
    require_signer(pending_owner)?;
    require_writable(config_account)?;
    let mut config = Config::load(config_account, program_id)?;
    config.require_unpaused()?;
    if config.pending_owner == Pubkey::default() || pending_owner.key != &config.pending_owner {
        return Err(BridgeError::Authority.into());
    }
    config.owner = config.pending_owner;
    config.pending_owner = Pubkey::default();
    config.store(config_account)?;
    msg!("PXBR/accept-owner/v1 owner={}", config.owner);
    Ok(())
}

/// Record the shared attestor set and its threshold. The set is the one every
/// destination of this bridge uses, so it is accepted only strictly ascending
/// with no repeat and no zero, and the threshold only between one and the count.
pub fn set_attestors(
    program_id: &Pubkey,
    accounts: &[AccountInfo<'_>],
    reader: &mut Reader<'_>,
) -> ProgramResult {
    let count = usize::from(reader.u8()?);
    if count == 0 || count > MAX_ATTESTORS {
        return Err(BridgeError::Attestors.into());
    }
    let mut attestors: Vec<[u8; HANDLE_BYTES]> = Vec::with_capacity(count);
    for _ in 0..count {
        let attestor = reader.array::<HANDLE_BYTES>()?;
        if attestor == [0_u8; HANDLE_BYTES] {
            return Err(BridgeError::Attestors.into());
        }
        if let Some(previous) = attestors.last() {
            if attestor.as_slice() <= previous.as_slice() {
                return Err(BridgeError::Attestors.into());
            }
        }
        attestors.push(attestor);
    }
    let threshold = reader.u8()?;
    reader.finish()?;
    if threshold == 0 || usize::from(threshold) > count {
        return Err(BridgeError::Attestors.into());
    }
    let mut accounts = accounts.iter();
    let owner = next_account_info(&mut accounts)?;
    let config_account = next_account_info(&mut accounts)?;
    require_writable(config_account)?;
    let mut config = Config::load(config_account, program_id)?;
    config.require_owner(owner)?;
    config.require_unpaused()?;
    config.attestors = attestors;
    config.threshold = threshold;
    config.store(config_account)?;
    msg!(
        "PXBR/set-attestors/v1 count={} threshold={}",
        count,
        threshold
    );
    Ok(())
}

/// Register a mint for bridging. The 20 bytes the mint occupies in a digest
/// default to the handle of the mint; the owner may record an explicit id
/// instead, which is how Sidiora's Solana mint carries the address the Paxeer
/// side already fixed for it.
pub fn register_asset(
    program_id: &Pubkey,
    accounts: &[AccountInfo<'_>],
    reader: &mut Reader<'_>,
) -> ProgramResult {
    let explicit = reader.flag()?;
    let declared = if explicit {
        Some(reader.array::<HANDLE_BYTES>()?)
    } else {
        None
    };
    let per_tx_cap = reader.u64()?;
    let total_cap = reader.u64()?;
    reader.finish()?;
    if per_tx_cap == 0 || total_cap == 0 || per_tx_cap > total_cap {
        return Err(BridgeError::Cap.into());
    }
    let mut accounts = accounts.iter();
    let owner = next_account_info(&mut accounts)?;
    let config_account = next_account_info(&mut accounts)?;
    let asset_account = next_account_info(&mut accounts)?;
    let mint_account = next_account_info(&mut accounts)?;
    let system = next_account_info(&mut accounts)?;
    require_payer(owner)?;
    require_writable(asset_account)?;
    let config = Config::load(config_account, program_id)?;
    config.require_owner(owner)?;
    config.require_unpaused()?;
    if mint_account.owner != &spl_token::id() {
        return Err(BridgeError::Account.into());
    }
    let mint = Mint::unpack(&mint_account.try_borrow_data()?)?;
    let asset_id = match declared {
        Some(declared) => {
            if declared == [0_u8; HANDLE_BYTES] {
                return Err(BridgeError::Asset.into());
            }
            declared
        }
        None => pubkey_handle(mint_account.key),
    };
    let (expected, bump) = find_asset_address(program_id, mint_account.key);
    if asset_account.key != &expected {
        return Err(BridgeError::Pda.into());
    }
    create_pda(
        owner,
        asset_account,
        system,
        program_id,
        ASSET_BYTES,
        &[ASSET_SEED, mint_account.key.as_ref(), &[bump]],
    )?;
    let asset = Asset {
        mint: *mint_account.key,
        asset_id,
        decimals: mint.decimals,
        enabled: true,
        bump,
        per_tx_cap,
        total_cap,
        outstanding: 0,
    };
    asset.store(asset_account)?;
    msg!(
        "PXBR/register-asset/v1 mint={} asset={} decimals={} per_tx_cap={} total_cap={}",
        asset.mint,
        hex(&asset.asset_id),
        asset.decimals,
        per_tx_cap,
        total_cap
    );
    Ok(())
}

/// Move a registered asset's caps and its open flag. A total cap below what the
/// program already holds in custody is refused, so the recorded outstanding
/// balance can never exceed the cap that admits it.
pub fn set_cap(
    program_id: &Pubkey,
    accounts: &[AccountInfo<'_>],
    reader: &mut Reader<'_>,
) -> ProgramResult {
    let per_tx_cap = reader.u64()?;
    let total_cap = reader.u64()?;
    let enabled = reader.flag()?;
    reader.finish()?;
    if per_tx_cap == 0 || total_cap == 0 || per_tx_cap > total_cap {
        return Err(BridgeError::Cap.into());
    }
    let mut accounts = accounts.iter();
    let owner = next_account_info(&mut accounts)?;
    let config_account = next_account_info(&mut accounts)?;
    let asset_account = next_account_info(&mut accounts)?;
    require_writable(asset_account)?;
    let config = Config::load(config_account, program_id)?;
    config.require_owner(owner)?;
    config.require_unpaused()?;
    let mut asset = Asset::load(asset_account, program_id)?;
    if total_cap < asset.outstanding {
        return Err(BridgeError::Cap.into());
    }
    asset.per_tx_cap = per_tx_cap;
    asset.total_cap = total_cap;
    asset.enabled = enabled;
    asset.store(asset_account)?;
    msg!(
        "PXBR/set-cap/v1 mint={} per_tx_cap={} total_cap={} enabled={}",
        asset.mint,
        per_tx_cap,
        total_cap,
        u8::from(enabled)
    );
    Ok(())
}

/// Pause or unpause custody. Pausing refuses every other instruction, including
/// a second pause; unpausing is the only instruction a paused program accepts.
pub fn set_pause(
    program_id: &Pubkey,
    accounts: &[AccountInfo<'_>],
    reader: &mut Reader<'_>,
) -> ProgramResult {
    let paused = reader.flag()?;
    reader.finish()?;
    let mut accounts = accounts.iter();
    let owner = next_account_info(&mut accounts)?;
    let config_account = next_account_info(&mut accounts)?;
    require_writable(config_account)?;
    let mut config = Config::load(config_account, program_id)?;
    config.require_owner(owner)?;
    if paused {
        config.require_unpaused()?;
    }
    config.paused = paused;
    config.store(config_account)?;
    msg!("PXBR/set-pause/v1 paused={}", u8::from(paused));
    Ok(())
}

//! Receipt-verified, sight-only balances.
use crate::{AccountId, Amount, AssetId, ProgramError};

/// Reads a verified balance without granting spending authority.
#[cfg(target_arch="wasm32")]
pub fn read(account:AccountId, asset:AssetId)->Result<Amount,ProgramError>{
    let mut encoded=[0u8;16];
    crate::host::balance_read(&account.bytes(),&asset.bytes(),&mut encoded)?;
    Ok(Amount::from_be_bytes(encoded))
}

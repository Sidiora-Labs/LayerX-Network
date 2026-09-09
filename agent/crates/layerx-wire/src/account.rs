use layerx_types::account::{AccountError, AccountId};

/// # Errors
/// Refuses malformed DIDs and asset suffixes while retaining the native main account.
pub fn account_name_for_asset(
    did: &str,
    asset: [u8; 32],
    native_asset: [u8; 32],
) -> Result<AccountId, AccountError> {
    AccountId::for_asset(did, asset, native_asset)
}

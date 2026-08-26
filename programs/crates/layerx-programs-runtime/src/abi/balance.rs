//! Receipt- and state-proof-bound balance facts.

/// Maximum distinct account/asset pairs one activity may observe.
pub const MAX_BALANCE_VIEW_GRANTS: usize = 32;

/// A balance fact issued only by Core's canonical account-state proof path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BalanceView {
    pub account: [u8; 32],
    pub asset: [u8; 32],
    pub balance: u128,
    pub receipt_digest: [u8; 32],
    pub state_root: [u8; 32],
    pub observed_sequence: u64,
}

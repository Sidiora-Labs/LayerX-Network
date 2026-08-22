//! Solidity value flow translated onto the 402LXP monetary law.
//!
//! Solidity gives a contract an account: it receives `msg.value`, holds a
//! balance and later pays that balance out with `transfer`, `send` or `call`.
//! `LayerX` gives a program no account at all. A program never holds balance
//! and never mutates one; it can only request an authenticated 402LXP transfer
//! that debits the invoking principal, and the kernel applies the whole set
//! atomically or none of it.
//!
//! The consequence for a port is exact and worth stating plainly: only value
//! flow the caller funds inside the same invocation survives translation. A
//! payout from an accumulated contract balance has no equivalent and is
//! refused here rather than emulated with a shadow ledger, because a shadow
//! ledger would be a second, unauthenticated money supply.

use layerx_programs_runtime::Capability;

use crate::error::PortRefusal;

/// One translated 402LXP transfer request: the asset, the recipient account
/// and the exact amount the invoking principal pays.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Transfer402Plan {
    asset: [u8; 32],
    to: [u8; 32],
    amount: u128,
}

impl Transfer402Plan {
    /// Builds one transfer leg.
    ///
    /// # Errors
    ///
    /// Refuses the reserved zero asset, the reserved zero recipient and a zero
    /// amount, all of which the capability ABI rejects as invalid grants.
    pub fn new(asset: [u8; 32], to: [u8; 32], amount: u128) -> Result<Self, PortRefusal> {
        if asset == [0u8; 32] || to == [0u8; 32] || amount == 0 {
            return Err(PortRefusal::OutOfRange);
        }
        Ok(Self { asset, to, amount })
    }

    /// Returns the asset the leg moves.
    #[must_use]
    pub const fn asset(&self) -> [u8; 32] {
        self.asset
    }

    /// Returns the account credited by the leg.
    #[must_use]
    pub const fn to(&self) -> [u8; 32] {
        self.to
    }

    /// Returns the exact amount debited from the invoking principal.
    #[must_use]
    pub const fn amount(&self) -> u128 {
        self.amount
    }

    /// Returns the grant an activity must carry for the leg to be admitted.
    #[must_use]
    pub const fn capability(&self) -> Capability {
        Capability::Transfer402 {
            asset: self.asset,
            to: self.to,
            maximum_amount: self.amount,
        }
    }

    /// Returns the amount split into the two `i64` halves the `transfer_402`
    /// host function takes, high limb first. The ABI is integer-only and has
    /// no 128-bit value type, so the amount crosses the boundary as two limbs
    /// reassembled by the host as `high << 64 | low`.
    #[must_use]
    pub fn amount_limbs(&self) -> (i64, i64) {
        let high = u64::try_from(self.amount >> 64).unwrap_or(u64::MAX);
        let low = u64::try_from(self.amount & u128::from(u64::MAX)).unwrap_or(u64::MAX);
        (
            i64::from_be_bytes(high.to_be_bytes()),
            i64::from_be_bytes(low.to_be_bytes()),
        )
    }
}

/// How a Solidity statement moves value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValueFlow {
    /// `msg.value` forwarded onwards inside the same call, or a `payable`
    /// function that pulls the price from its caller.
    CallerFunded {
        /// The credited account.
        recipient: [u8; 32],
        /// The exact amount the caller pays.
        amount: u128,
    },
    /// `recipient.transfer(amount)` paid out of the contract's own balance.
    ContractFunded {
        /// The credited account.
        recipient: [u8; 32],
        /// The amount the contract would have paid from its balance.
        amount: u128,
    },
    /// `transferFrom(owner, recipient, amount)` drawn against a stored
    /// allowance.
    AllowanceSpend {
        /// The account whose balance the allowance would debit.
        owner: [u8; 32],
        /// The credited account.
        recipient: [u8; 32],
        /// The amount the allowance would move.
        amount: u128,
    },
    /// `selfdestruct(recipient)` sweeping whatever balance the contract holds.
    SelfDestructSweep {
        /// The account the sweep would credit.
        recipient: [u8; 32],
    },
}

impl ValueFlow {
    /// Translates one Solidity value flow into a 402LXP transfer leg paid by
    /// the invoking principal.
    ///
    /// # Errors
    ///
    /// Refuses [`PortRefusal::ContractHeldBalance`] for any payout from a
    /// balance the contract accumulated, because no program holds balance, and
    /// [`PortRefusal::DelegatedSpend`] for an allowance drawn on an account
    /// other than the invoking principal, because delegated spending on
    /// `LayerX` is an explicit capability the payer grants at invocation and
    /// never state a program stores about a third party.
    pub fn translate(
        &self,
        asset: [u8; 32],
        principal: [u8; 32],
    ) -> Result<Transfer402Plan, PortRefusal> {
        match self {
            Self::CallerFunded { recipient, amount } => {
                Transfer402Plan::new(asset, *recipient, *amount)
            }
            Self::ContractFunded { .. } | Self::SelfDestructSweep { .. } => {
                Err(PortRefusal::ContractHeldBalance)
            }
            Self::AllowanceSpend {
                owner,
                recipient,
                amount,
            } => {
                if owner == &principal {
                    Transfer402Plan::new(asset, *recipient, *amount)
                } else {
                    Err(PortRefusal::DelegatedSpend)
                }
            }
        }
    }

    /// Returns whether the flow can be carried over at all, without building
    /// the leg. A porting tool uses this to report every unportable statement
    /// in one pass instead of stopping at the first.
    #[must_use]
    pub const fn portable(&self) -> bool {
        matches!(
            self,
            Self::CallerFunded { .. } | Self::AllowanceSpend { .. }
        )
    }
}

/// Translates a whole function body's value flow, keeping declaration order so
/// the emitted transfer set matches the order the Solidity source pays in.
///
/// # Errors
///
/// Returns the first refusal encountered, naming the construct that cannot be
/// carried over.
pub fn translate_all(
    flows: &[ValueFlow],
    asset: [u8; 32],
    principal: [u8; 32],
) -> Result<Vec<Transfer402Plan>, PortRefusal> {
    let mut plans = Vec::with_capacity(flows.len());
    for flow in flows {
        plans.push(flow.translate(asset, principal)?);
    }
    Ok(plans)
}

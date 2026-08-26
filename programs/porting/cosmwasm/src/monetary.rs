//! `CosmWasm` value flow translated onto the 402LXP monetary law.
//!
//! A `CosmWasm` contract has a bank account. Funds arrive attached to a message
//! as `info.funds`, sit in the contract's balance, and leave later as a
//! `BankMsg` the chain dispatches after the handler returns.
//!
//! A `LayerX` program cannot mutate a balance. Contract-held value maps to a
//! real derived account and leaves through a bounded owner-frame 402LXP
//! request that the kernel rederives and applies atomically.
//!
//! So caller-attached funds and bounded payouts from a supplied derived account
//! carry over exactly. Supply mutation and context-free custody are refused
//! rather than emulated with a shadow ledger,
//! because a shadow ledger would be a second, unauthenticated money supply.
//! A denom does not carry over either: a program is paid in an authenticated
//! 402LXP asset, and a port names which asset stands in for the denom.

use layerx_programs_runtime::{derive_program_account, Capability, ProgramId};

use crate::error::PortRefusal;

/// One translated 402LXP transfer request: the asset, the recipient account
/// and the exact amount the invoking principal pays.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Transfer402Plan {
    asset: [u8; 32],
    to: [u8; 32],
    amount: u128,
}

/// A `BankMsg` or funded sub-message debited from the contract's derived account.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramAccountTransferPlan {
    owner_program: ProgramId,
    seed: Vec<u8>,
    source: [u8; 32],
    asset: [u8; 32],
    to: [u8; 32],
    amount: u128,
}

impl ProgramAccountTransferPlan {
    /// Builds an owner-frame payout from an exact rederived contract account.
    ///
    /// # Errors
    ///
    /// Refuses reserved monetary fields, an oversized seed, or a source that
    /// does not match the owner program and seed.
    pub fn new(
        owner_program: ProgramId,
        seed: &[u8],
        source: [u8; 32],
        asset: [u8; 32],
        to: [u8; 32],
        amount: u128,
    ) -> Result<Self, PortRefusal> {
        if asset == [0; 32]
            || to == [0; 32]
            || amount == 0
            || derive_program_account(owner_program, seed)
                .map_err(|_| PortRefusal::InvalidProgramAccount)?
                .bytes() != source
        {
            return Err(PortRefusal::InvalidProgramAccount);
        }
        Ok(Self {
            owner_program,
            seed: seed.to_vec(),
            source,
            asset,
            to,
            amount,
        })
    }
    /// Returns the contract program that owns the source.
    #[must_use]
    pub const fn owner_program(&self) -> ProgramId {
        self.owner_program
    }
    /// Returns the public account-derivation seed.
    #[must_use]
    pub fn seed(&self) -> &[u8] {
        &self.seed
    }
    /// Returns the rederived source account.
    #[must_use]
    pub const fn source(&self) -> [u8; 32] {
        self.source
    }
    /// Returns the transferred asset.
    #[must_use]
    pub const fn asset(&self) -> [u8; 32] {
        self.asset
    }
    /// Returns the credited account.
    #[must_use]
    pub const fn to(&self) -> [u8; 32] {
        self.to
    }
    /// Returns the exact amount.
    #[must_use]
    pub const fn amount(&self) -> u128 {
        self.amount
    }
    /// Returns the exact owner-bound runtime capability.
    #[must_use]
    pub fn capability(&self) -> Capability {
        Capability::ProgramSpend {
            owner_program: self.owner_program,
            seed: self.seed.clone(),
            source_account: self.source,
            asset: self.asset,
            to: self.to,
            maximum_amount: self.amount,
        }
    }
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
    /// the host reassembles as `high << 64 | low`.
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

/// How a `CosmWasm` handler moves value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValueFlow {
    /// Funds the caller attached as `info.funds` and the handler forwards to a
    /// named recipient in the same invocation.
    SentFunds {
        /// The credited account.
        recipient: [u8; 32],
        /// The exact amount the caller pays.
        amount: u128,
    },
    /// `BankMsg::Send` paid out of the contract's own balance.
    BankSend {
        /// The credited account.
        recipient: [u8; 32],
        /// The amount the contract would have paid from its balance.
        amount: u128,
    },
    /// `BankMsg::Burn`, which destroys balance the contract holds.
    BankBurn {
        /// The amount the burn would destroy.
        amount: u128,
    },
    /// `WasmMsg::Execute { funds, .. }` attaching funds to a sub-message out
    /// of the contract's own balance.
    SubMessageFunds {
        /// The callee the sub-message would pay.
        recipient: [u8; 32],
        /// The amount the sub-message would attach.
        amount: u128,
    },
    /// A `cw20` `TransferFrom` drawn against an allowance the contract stored
    /// for a third party.
    AllowanceSpend {
        /// The account whose balance the allowance would debit.
        owner: [u8; 32],
        /// The credited account.
        recipient: [u8; 32],
        /// The amount the allowance would move.
        amount: u128,
    },
    /// `IbcMsg::Transfer`, which moves value to another chain.
    IbcTransfer {
        /// The account on the far chain the transfer would credit.
        recipient: [u8; 32],
        /// The amount the transfer would move.
        amount: u128,
    },
}

impl ValueFlow {
    /// Translates with the derived account that carries the contract balance.
    ///
    /// # Errors
    ///
    /// Refuses invalid derived-account context, supply mutation and source
    /// constructs that have no deterministic LayerX equivalent.
    pub fn translate_with_program_account(
        &self,
        asset: [u8; 32],
        principal: [u8; 32],
        owner_program: ProgramId,
        seed: &[u8],
        source: [u8; 32],
    ) -> Result<TranslatedValueFlow, PortRefusal> {
        match self {
            Self::BankSend { recipient, amount } | Self::SubMessageFunds { recipient, amount } => {
                ProgramAccountTransferPlan::new(
                    owner_program, seed, source, asset, *recipient, *amount,
                )
                .map(TranslatedValueFlow::ProgramAccount)
            }
            Self::BankBurn { .. } => Err(PortRefusal::SupplyMutation),
            _ => self
                .translate(asset, principal)
                .map(TranslatedValueFlow::Principal),
        }
    }
    /// Translates one `CosmWasm` value flow into a 402LXP transfer leg paid by
    /// the invoking principal.
    ///
    /// # Errors
    ///
    /// Refuses [`PortRefusal::ContractHeldBalance`] for any payout, burn or
    /// sub-message funded from a balance the contract accumulated when no
    /// derived-account context was supplied; [`PortRefusal::DelegatedSpend`]
    /// for an allowance
    /// drawn on an account other than the invoking principal, because
    /// delegated spending on `LayerX` is an explicit capability the payer
    /// grants at invocation and never state a contract stores about a third
    /// party; and [`PortRefusal::ChainQuery`] for an inter-chain transfer,
    /// which has no equivalent inside a deterministic execution.
    pub fn translate(
        &self,
        asset: [u8; 32],
        principal: [u8; 32],
    ) -> Result<Transfer402Plan, PortRefusal> {
        match self {
            Self::SentFunds { recipient, amount } => {
                Transfer402Plan::new(asset, *recipient, *amount)
            }
            Self::BankSend { .. } | Self::BankBurn { .. } | Self::SubMessageFunds { .. } => {
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
            Self::IbcTransfer { .. } => Err(PortRefusal::ChainQuery),
        }
    }

    /// Returns whether the flow can be carried over at all, without building
    /// the leg. A porting tool uses this to report every unportable statement
    /// in one pass instead of stopping at the first.
    #[must_use]
    pub const fn portable(&self) -> bool {
        matches!(self, Self::SentFunds { .. } | Self::AllowanceSpend { .. })
    }
}

/// The authentic monetary source selected for one translated CosmWasm flow.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TranslatedValueFlow {
    /// Debits the invoking principal through ordinary transfer authority.
    Principal(Transfer402Plan),
    /// Debits the rederived account owned by the current program.
    ProgramAccount(ProgramAccountTransferPlan),
}

/// Translates a whole handler's value flow, keeping declaration order so the
/// emitted transfer set matches the order the `Response` carries messages in.
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

#[cfg(test)]
mod custody_tests {
    use super::*;

    #[test]
    fn bank_send_uses_derived_account_authority() {
        let owner = ProgramId::new([7; 32]).unwrap_or_else(|error| panic!("owner: {error}"));
        let source = derive_program_account(owner, b"contract")
            .unwrap_or_else(|error| panic!("derive: {error}"))
            .bytes();
        let translated = (ValueFlow::BankSend { recipient: [4; 32], amount: 9 })
            .translate_with_program_account([3; 32], [2; 32], owner, b"contract", source)
            .unwrap_or_else(|error| panic!("translate: {error}"));
        assert!(matches!(translated, TranslatedValueFlow::ProgramAccount(_)));
        assert_eq!(
            (ValueFlow::BankBurn { amount: 9 }).translate_with_program_account(
                [3; 32], [2; 32], owner, b"contract", source,
            ),
            Err(PortRefusal::SupplyMutation)
        );
    }
}

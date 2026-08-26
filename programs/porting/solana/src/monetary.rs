//! Solana lamport and token flow translated onto the 402LXP monetary law.
//!
//! On Solana every account holds lamports, and a program that owns an account
//! may debit it by writing the balance field directly. A program-derived
//! address gives a program its own signing authority, so a program can also
//! pay out of an account it controls with `invoke_signed`.
//!
//! `LayerX` gives a program no balance-writing primitive. PDA-held value maps
//! to a real derived account and leaves through a bounded owner-frame 402LXP
//! request that the kernel rederives and applies atomically.
//!
//! So a payer-signed transfer and a bounded `invoke_signed` payout carry over.
//! Direct lamport writes and an amount-less `close = recipient` rent sweep do
//! not. Each is refused by name rather than
//! emulated with a shadow balance, because a shadow balance would be a second,
//! unauthenticated money supply.

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

/// A payout signed by the program through its LayerX-derived value account.
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
    /// Builds an owner-frame payout from an exact rederived program account.
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
    /// Returns the program that owns the source.
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

/// How a Solana instruction moves value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValueFlow {
    /// `system_program::transfer` whose `from` account is a `Signer` on the
    /// instruction, which is the pattern every price guard uses.
    SignerFunded {
        /// The account the signer's lamports are credited to.
        recipient: [u8; 32],
        /// The exact amount the signer pays.
        amount: u128,
    },
    /// A direct lamport write on a program-owned account, as in
    /// `**account.try_borrow_mut_lamports()? -= amount`.
    LamportWrite {
        /// The account the write would credit.
        recipient: [u8; 32],
        /// The amount the write would move.
        amount: u128,
    },
    /// An `invoke_signed` transfer whose source is a program-derived address,
    /// paying out of a balance the program itself controls.
    ProgramAuthorityFunded {
        /// The program-derived address that would sign the payout.
        authority: [u8; 32],
        /// The account the payout would credit.
        recipient: [u8; 32],
        /// The amount the payout would move.
        amount: u128,
    },
    /// An SPL token `transfer` whose `authority` account signs the debit.
    TokenTransfer {
        /// The account that signs the debit.
        authority: [u8; 32],
        /// The token account credited by the transfer.
        recipient: [u8; 32],
        /// The amount the transfer moves.
        amount: u128,
    },
    /// A `#[account(mut, close = recipient)]` constraint sweeping an account's
    /// rent-exempt lamports on close.
    RentSweep {
        /// The account the sweep would credit.
        recipient: [u8; 32],
    },
}

impl ValueFlow {
    /// Translates an Anchor/PDA flow with the LayerX derived account that
    /// replaces the PDA's value-holding role.
    ///
    /// # Errors
    ///
    /// Refuses invalid derived-account context, direct lamport writes and
    /// amount-less rent sweeps.
    pub fn translate_with_program_account(
        &self,
        asset: [u8; 32],
        principal: [u8; 32],
        owner_program: ProgramId,
        seed: &[u8],
        source: [u8; 32],
    ) -> Result<TranslatedValueFlow, PortRefusal> {
        match self {
            Self::ProgramAuthorityFunded { authority, recipient, amount } => {
                if authority != &source {
                    return Err(PortRefusal::InvalidProgramAccount);
                }
                ProgramAccountTransferPlan::new(
                    owner_program, seed, source, asset, *recipient, *amount,
                )
                .map(TranslatedValueFlow::ProgramAccount)
            }
            Self::LamportWrite { .. } => Err(PortRefusal::LamportMutation),
            Self::RentSweep { .. } => Err(PortRefusal::UnboundedRentSweep),
            _ => self
                .translate(asset, principal)
                .map(TranslatedValueFlow::Principal),
        }
    }
    /// Translates one Solana value flow into a 402LXP transfer leg paid by the
    /// invoking principal.
    ///
    /// # Errors
    ///
    /// Refuses [`PortRefusal::LamportMutation`] for a direct balance write, an
    /// `invoke_signed` payout and a rent sweep, because no program holds or
    /// writes balance, and [`PortRefusal::DelegatedSpend`] for a token debit
    /// authorised by anyone other than the invoking principal, because
    /// delegated spending on `LayerX` is an explicit capability the payer
    /// grants at invocation and never an authority a program holds over
    /// somebody else's account.
    pub fn translate(
        &self,
        asset: [u8; 32],
        principal: [u8; 32],
    ) -> Result<Transfer402Plan, PortRefusal> {
        match self {
            Self::SignerFunded { recipient, amount } => {
                Transfer402Plan::new(asset, *recipient, *amount)
            }
            Self::LamportWrite { .. }
            | Self::ProgramAuthorityFunded { .. }
            | Self::RentSweep { .. } => Err(PortRefusal::LamportMutation),
            Self::TokenTransfer {
                authority,
                recipient,
                amount,
            } => {
                if authority == &principal {
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
        matches!(self, Self::SignerFunded { .. } | Self::TokenTransfer { .. })
    }
}

/// The authentic monetary source selected for one translated Solana flow.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TranslatedValueFlow {
    /// Debits the invoking principal through ordinary transfer authority.
    Principal(Transfer402Plan),
    /// Debits the rederived account owned by the current program.
    ProgramAccount(ProgramAccountTransferPlan),
}

/// Translates a whole instruction handler's value flow, keeping declaration
/// order so the emitted transfer set matches the order the Anchor source pays
/// in.
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
    fn invoke_signed_uses_derived_account_authority() {
        let owner = ProgramId::new([7; 32]).unwrap_or_else(|error| panic!("owner: {error}"));
        let source = derive_program_account(owner, b"pool")
            .unwrap_or_else(|error| panic!("derive: {error}"))
            .bytes();
        let translated = (ValueFlow::ProgramAuthorityFunded {
            authority: source,
            recipient: [4; 32],
            amount: 9,
        })
            .translate_with_program_account([3; 32], [2; 32], owner, b"pool", source)
            .unwrap_or_else(|error| panic!("translate: {error}"));
        assert!(matches!(translated, TranslatedValueFlow::ProgramAccount(_)));
        assert_eq!(
            (ValueFlow::RentSweep { recipient: [4; 32] })
                .translate_with_program_account([3; 32], [2; 32], owner, b"pool", source),
            Err(PortRefusal::UnboundedRentSweep)
        );
    }
}

//! The 402LXP transfer binding.
//!
//! A program cannot write a balance. It can only request an authenticated
//! transfer the kernel applies atomically after the whole execution succeeds,
//! and only inside the ceiling its capability grant fixed. Amounts are exact
//! protocol integers, refused at construction when zero.

use crate::amount::Amount;
use crate::abi::MAX_PROGRAM_ACCOUNT_SEED_BYTES;
use crate::error::{Field, ProgramError, Reason};
use crate::ids::{AccountId, AssetId};

#[cfg(target_arch = "wasm32")]
use crate::host;

/// One authenticated 402LXP transfer the kernel will apply.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Payment {
    asset: AssetId,
    to: AccountId,
    amount: Amount,
}

/// Bounded public seed identifying an account owned by the current program.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgramAccountSeed<'a>(&'a [u8]);

impl<'a> ProgramAccountSeed<'a> {
    /// Constructs a seed within the canonical program-account derivation bound.
    ///
    /// # Errors
    ///
    /// Refuses a seed longer than the runtime's canonical derivation bound.
    pub const fn new(bytes: &'a [u8]) -> Result<Self, ProgramError> {
        if bytes.len() > MAX_PROGRAM_ACCOUNT_SEED_BYTES {
            return Err(ProgramError::value(Field::Account, Reason::TooLarge));
        }
        Ok(Self(bytes))
    }

    /// Returns the public derivation seed. It grants no spending authority.
    #[must_use]
    pub const fn bytes(self) -> &'a [u8] {
        self.0
    }
}

/// One candidate transfer from an account derived for the current program.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgramAccountPayment<'a> {
    seed: ProgramAccountSeed<'a>,
    source: AccountId,
    asset: AssetId,
    to: AccountId,
    amount: Amount,
}

/// Principal-funded deposit into the current program's derived value account.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgramDeposit<'a> {
    seed: ProgramAccountSeed<'a>,
    destination: AccountId,
    asset: AssetId,
    amount: Amount,
}

impl<'a> ProgramDeposit<'a> {
    /// Builds a bounded deposit request. The host rederives the destination and
    /// the kernel verifies its current registry and asset binding before commit.
    pub const fn new(
        seed: ProgramAccountSeed<'a>,
        destination: AccountId,
        asset: AssetId,
        amount: Amount,
    ) -> Result<Self, ProgramError> {
        if amount.is_zero() {
            return Err(ProgramError::value(Field::Amount, Reason::Zero));
        }
        Ok(Self {
            seed,
            destination,
            asset,
            amount,
        })
    }
    /// Returns the public derivation seed.
    #[must_use]
    pub const fn seed(self) -> ProgramAccountSeed<'a> {
        self.seed
    }
    /// Returns the exact derived destination.
    #[must_use]
    pub const fn destination(self) -> AccountId {
        self.destination
    }
    /// Returns the registry-bound asset.
    #[must_use]
    pub const fn asset(self) -> AssetId {
        self.asset
    }
    /// Returns the exact deposit amount.
    #[must_use]
    pub const fn amount(self) -> Amount {
        self.amount
    }
}

impl<'a> ProgramAccountPayment<'a> {
    /// Builds a bounded program-account payment request without constructing authority.
    ///
    /// # Errors
    ///
    /// Refuses the zero amount the runtime's monetary law rejects.
    pub const fn new(
        seed: ProgramAccountSeed<'a>,
        source: AccountId,
        asset: AssetId,
        to: AccountId,
        amount: Amount,
    ) -> Result<Self, ProgramError> {
        if amount.is_zero() {
            return Err(ProgramError::value(Field::Amount, Reason::Zero));
        }
        Ok(Self {
            seed,
            source,
            asset,
            to,
            amount,
        })
    }

    /// Returns the public derivation seed.
    #[must_use]
    pub const fn seed(self) -> ProgramAccountSeed<'a> {
        self.seed
    }

    /// Returns the claimed source account the runtime must rederive.
    #[must_use]
    pub const fn source(self) -> AccountId {
        self.source
    }

    /// Returns the asset to move.
    #[must_use]
    pub const fn asset(self) -> AssetId {
        self.asset
    }

    /// Returns the destination account.
    #[must_use]
    pub const fn to(self) -> AccountId {
        self.to
    }

    /// Returns the exact amount.
    #[must_use]
    pub const fn amount(self) -> Amount {
        self.amount
    }
}

impl Payment {
    /// Builds a payment of an exact nonzero integer amount.
    ///
    /// # Errors
    ///
    /// Refuses the zero amount the runtime's monetary law rejects.
    pub const fn new(asset: AssetId, to: AccountId, amount: Amount) -> Result<Self, ProgramError> {
        if amount.is_zero() {
            return Err(ProgramError::value(Field::Amount, Reason::Zero));
        }
        Ok(Self { asset, to, amount })
    }

    /// Returns the asset this payment moves.
    #[must_use]
    pub const fn asset(self) -> AssetId {
        self.asset
    }

    /// Returns the account this payment credits.
    #[must_use]
    pub const fn to(self) -> AccountId {
        self.to
    }

    /// Returns the exact integer amount.
    #[must_use]
    pub const fn amount(self) -> Amount {
        self.amount
    }
}

/// Requests one authenticated 402LXP transfer.
///
/// # Errors
///
/// Refuses missing transfer authority, an amount above the granted ceiling,
/// and every meter refusal.
#[cfg(target_arch = "wasm32")]
pub fn pay(payment: Payment) -> Result<(), ProgramError> {
    let (amount_high, amount_low) = payment.amount().split();
    let asset = payment.asset().bytes();
    let recipient = payment.to().bytes();
    host::transfer_402(amount_high, amount_low, &asset, &recipient)?;
    Ok(())
}

/// Funds the current program's exact registered derived account.
#[cfg(target_arch = "wasm32")]
pub fn fund_program_account(deposit: ProgramDeposit<'_>) -> Result<(), ProgramError> {
    let (high, low) = deposit.amount().split();
    host::fund_program_402(
        high,
        low,
        deposit.seed().bytes(),
        &deposit.destination().bytes(),
        &deposit.asset().bytes(),
    )?;
    Ok(())
}

/// Requests a candidate transfer from an account owned by the current program.
///
/// The runtime rederives `source` from the current program and `seed`, checks
/// the cumulative ProgramSpend grant at the current call frame, and creates the
/// opaque authority token internally. This binding never constructs authority.
///
/// # Errors
///
/// Refuses a mismatched derived source, missing or exceeded ProgramSpend
/// authority, a zero or cumulative overflowing amount, and meter refusal.
#[cfg(target_arch = "wasm32")]
pub fn pay_from_program_account(payment: ProgramAccountPayment<'_>) -> Result<(), ProgramError> {
    let (amount_high, amount_low) = payment.amount().split();
    let source = payment.source().bytes();
    let asset = payment.asset().bytes();
    let recipient = payment.to().bytes();
    host::transfer_program_402(
        amount_high,
        amount_low,
        payment.seed().bytes(),
        &source,
        &asset,
        &recipient,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ProgramAccountPayment, ProgramAccountSeed};
    use crate::{AccountId, Amount, AssetId, MAX_PROGRAM_ACCOUNT_SEED_BYTES};

    #[test]
    fn program_account_seed_enforces_the_runtime_bound() {
        let exact = std::vec![7; MAX_PROGRAM_ACCOUNT_SEED_BYTES];
        assert!(ProgramAccountSeed::new(&exact).is_ok());
        let oversized = std::vec![7; MAX_PROGRAM_ACCOUNT_SEED_BYTES + 1];
        assert!(ProgramAccountSeed::new(&oversized).is_err());
    }

    #[test]
    fn program_account_payment_refuses_zero_amount() {
        let seed = ProgramAccountSeed::new(b"vault").unwrap_or_else(|error| panic!("seed: {error}"));
        let source = AccountId::new([1; 32]).unwrap_or_else(|error| panic!("source: {error}"));
        let asset = AssetId::new([2; 32]).unwrap_or_else(|error| panic!("asset: {error}"));
        let to = AccountId::new([3; 32]).unwrap_or_else(|error| panic!("to: {error}"));
        assert!(
            ProgramAccountPayment::new(seed, source, asset, to, Amount::from_u128(0)).is_err()
        );
    }
}

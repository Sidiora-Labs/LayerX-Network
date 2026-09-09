use sha2::{Digest, Sha256};

use crate::{
    AccountId, Amount, AssetId, Bytes, Capability, Field, ProgramAccountPayment,
    ProgramAccountSeed, ProgramDeposit, ProgramError, ProgramId, Reason,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreparedProgramAccount<'a> {
    program: ProgramId,
    seed: ProgramAccountSeed<'a>,
    asset: AssetId,
    account: AccountId,
}

impl<'a> PreparedProgramAccount<'a> {
    /// # Errors
    /// Refuses an oversized seed or reserved derived identifier.
    pub fn new(program: ProgramId, seed: &'a [u8], asset: AssetId) -> Result<Self, ProgramError> {
        let seed = ProgramAccountSeed::new(seed)?;
        let length = u32::try_from(seed.bytes().len())
            .map_err(|_| ProgramError::value(Field::Account, Reason::TooLarge))?;
        let mut hash = Sha256::new();
        hash.update(b"LayerX/programs/program-account/v1\0");
        hash.update(program.bytes());
        hash.update(length.to_be_bytes());
        hash.update(seed.bytes());
        Ok(Self {
            program,
            seed,
            asset,
            account: AccountId::new(hash.finalize().into())?,
        })
    }

    #[must_use]
    pub const fn account(self) -> AccountId {
        self.account
    }

    /// Produces the native `ProgramAccount` registration payload. Submission requires
    /// the deployment's registration authority and a signed activity envelope.
    /// # Errors
    /// Refuses a length that cannot be encoded.
    pub fn registration_payload(self) -> Result<Bytes<201>, ProgramError> {
        let mut output = Bytes::empty();
        output.extend(&self.program.bytes())?;
        output.extend(b"LXPA1")?;
        output.extend(&self.asset.bytes())?;
        let length = u32::try_from(self.seed.bytes().len())
            .map_err(|_| ProgramError::value(Field::Account, Reason::TooLarge))?;
        output.extend(&length.to_be_bytes())?;
        output.extend(self.seed.bytes())?;
        Ok(output)
    }

    /// # Errors
    /// Refuses zero funding.
    pub const fn deposit(self, amount: Amount) -> Result<ProgramDeposit<'a>, ProgramError> {
        ProgramDeposit::new(self.seed, self.account, self.asset, amount)
    }

    /// # Errors
    /// Refuses a zero ceiling. The returned grant must be authorized by the caller.
    pub const fn funding_grant(self, maximum: Amount) -> Result<Capability, ProgramError> {
        Capability::transfer(self.asset, self.account, maximum)
    }

    /// # Errors
    /// Refuses a zero payment.
    pub const fn payment(
        self,
        to: AccountId,
        amount: Amount,
    ) -> Result<ProgramAccountPayment<'a>, ProgramError> {
        ProgramAccountPayment::new(self.seed, self.account, self.asset, to, amount)
    }

    /// Encodes one ABI-v2 `ProgramSpend` grant, including its one-grant set prefix.
    /// # Errors
    /// Refuses a zero ceiling or unencodable seed length.
    pub fn spend_grant(self, to: AccountId, maximum: Amount) -> Result<Bytes<277>, ProgramError> {
        self.payment(to, maximum)?;
        let mut output = Bytes::empty();
        output.extend(&[0, 1, 9])?;
        output.extend(&self.program.bytes())?;
        let length = u16::try_from(self.seed.bytes().len())
            .map_err(|_| ProgramError::value(Field::Account, Reason::TooLarge))?;
        output.extend(&length.to_be_bytes())?;
        output.extend(self.seed.bytes())?;
        output.extend(&self.account.bytes())?;
        output.extend(&self.asset.bytes())?;
        output.extend(&to.bytes())?;
        output.extend(&maximum.to_be_bytes())?;
        Ok(output)
    }
}

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaymentGrant<'a> {
    Basic(Capability),
    ProgramSpend {
        account: PreparedProgramAccount<'a>,
        to: AccountId,
        maximum: Amount,
    },
}

impl PaymentGrant<'_> {
    fn validate(self) -> Result<(), ProgramError> {
        match self {
            Self::Basic(Capability::Transfer402 {
                asset,
                to,
                maximum_amount,
            }) => {
                Capability::transfer(asset, to, maximum_amount)?;
            }
            Self::ProgramSpend {
                account,
                to,
                maximum,
            } => {
                account.payment(to, maximum)?;
            }
            Self::Basic(_) => {}
        }
        Ok(())
    }

    fn key_cmp(self, other: Self) -> core::cmp::Ordering {
        match (self, other) {
            (Self::Basic(left), Self::Basic(right)) => left.authority_key_cmp(right),
            (
                Self::ProgramSpend {
                    account: left,
                    to: left_to,
                    ..
                },
                Self::ProgramSpend {
                    account: right,
                    to: right_to,
                    ..
                },
            ) => (
                left.program.bytes(),
                left.seed.bytes(),
                left.account.bytes(),
                left.asset.bytes(),
                left_to.bytes(),
            )
                .cmp(&(
                    right.program.bytes(),
                    right.seed.bytes(),
                    right.account.bytes(),
                    right.asset.bytes(),
                    right_to.bytes(),
                )),
            (Self::Basic(basic), Self::ProgramSpend { .. }) => basic_spend_order(basic),
            (Self::ProgramSpend { .. }, Self::Basic(basic)) => basic_spend_order(basic).reverse(),
        }
    }

    fn encoded_len(self) -> usize {
        match self {
            Self::Basic(grant) => grant.encoded_len(),
            Self::ProgramSpend { account, .. } => 147 + account.seed.bytes().len(),
        }
    }
}

fn basic_spend_order(grant: Capability) -> core::cmp::Ordering {
    match grant {
        Capability::ReceiptRead { .. }
        | Capability::SharedStorageRead
        | Capability::SharedStorageWrite => core::cmp::Ordering::Greater,
        _ => core::cmp::Ordering::Less,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgramPaymentCapabilities<'a, const N: usize> {
    grants: [PaymentGrant<'a>; N],
    length: usize,
}

impl<'a, const N: usize> ProgramPaymentCapabilities<'a, N> {
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            grants: [PaymentGrant::Basic(Capability::StorageRead); N],
            length: 0,
        }
    }

    /// # Errors
    /// Refuses zero ceilings, duplicate authority keys and capacity overflow.
    pub fn insert(&mut self, grant: PaymentGrant<'a>) -> Result<(), ProgramError> {
        grant.validate()?;
        if self.length >= N || self.length >= crate::MAX_CAPABILITIES {
            return Err(ProgramError::value(Field::Capability, Reason::TooLarge));
        }
        let mut position = 0;
        while position < self.length {
            match self.grants[position].key_cmp(grant) {
                core::cmp::Ordering::Less => position += 1,
                core::cmp::Ordering::Equal => {
                    return Err(ProgramError::value(Field::Capability, Reason::Duplicate))
                }
                core::cmp::Ordering::Greater => break,
            }
        }
        self.grants.copy_within(position..self.length, position + 1);
        self.grants[position] = grant;
        self.length += 1;
        Ok(())
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.length
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.length == 0
    }

    #[must_use]
    pub fn encoded_len(&self) -> usize {
        self.grants[..self.length]
            .iter()
            .fold(2_usize, |length, grant| {
                length.saturating_add(grant.encoded_len())
            })
    }

    /// # Errors
    /// Refuses an encoding past the ABI limit or a short output buffer without writing it.
    pub fn encode_into(&self, output: &mut [u8]) -> Result<usize, ProgramError> {
        let length = self.encoded_len();
        if length > crate::MAX_CAPABILITY_ENCODING_BYTES {
            return Err(ProgramError::value(
                Field::CapabilityEncoding,
                Reason::TooLarge,
            ));
        }
        if output.len() < length {
            return Err(ProgramError::value(Field::Buffer, Reason::TooSmall));
        }
        let count = u16::try_from(self.length)
            .map_err(|_| ProgramError::value(Field::Capability, Reason::TooLarge))?;
        output[..2].copy_from_slice(&count.to_be_bytes());
        let mut cursor = 2;
        for grant in &self.grants[..self.length] {
            let end = cursor + grant.encoded_len();
            match *grant {
                PaymentGrant::Basic(basic) => {
                    let set = crate::CapabilitySet::<1>::from_grants(&[basic])?;
                    let mut scratch = [0; 83];
                    let used = set.encode_into(&mut scratch)?;
                    output[cursor..end].copy_from_slice(&scratch[2..used]);
                }
                PaymentGrant::ProgramSpend {
                    account,
                    to,
                    maximum,
                } => {
                    let encoded = account.spend_grant(to, maximum)?;
                    output[cursor..end].copy_from_slice(&encoded.as_slice()[2..]);
                }
            }
            cursor = end;
        }
        Ok(cursor)
    }
}

impl<const N: usize> Default for ProgramPaymentCapabilities<'_, N> {
    fn default() -> Self {
        Self::empty()
    }
}

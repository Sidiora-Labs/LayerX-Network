use crate::{AccountId, Amount, Bytes, Field, Principal, ProgramError, Reason};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Request {
    Transfer {
        to: AccountId,
        amount: Amount,
    },
    Approve {
        spender: Principal,
        amount: Amount,
    },
    TransferFrom {
        owner: Principal,
        to: AccountId,
        amount: Amount,
    },
    BalanceOf {
        owner: Principal,
    },
    Allowance {
        owner: Principal,
        spender: Principal,
    },
    TotalSupply,
    Metadata,
}

impl Request {
    #[must_use]
    pub const fn method(self) -> &'static str {
        match self {
            Self::Transfer { .. } => "transfer",
            Self::Approve { .. } => "approve",
            Self::TransferFrom { .. } => "transfer_from",
            Self::BalanceOf { .. } => "balance_of",
            Self::Allowance { .. } => "allowance",
            Self::TotalSupply => "total_supply",
            Self::Metadata => "metadata",
        }
    }

    #[must_use]
    pub const fn discriminator(self) -> [u8; 4] {
        [
            b'L',
            b'X',
            20,
            match self {
                Self::Transfer { .. } => 1,
                Self::Approve { .. } => 2,
                Self::TransferFrom { .. } => 3,
                Self::BalanceOf { .. } => 4,
                Self::Allowance { .. } => 5,
                Self::TotalSupply => 6,
                Self::Metadata => 7,
            },
        ]
    }

    /// Encodes a discriminator followed by canonical `LayerX` bounded bytes.
    /// # Errors
    /// Refuses zero transfers. Approval zero explicitly revokes the allowance.
    pub fn encode(self) -> Result<Bytes<90>, ProgramError> {
        let mut payload = Bytes::<80>::empty();
        match self {
            Self::Transfer { to, amount } => {
                nonzero(amount)?;
                payload.extend(&to.bytes())?;
                payload.extend(&amount.to_be_bytes())?;
            }
            Self::Approve { spender, amount } => {
                payload.extend(&spender.bytes())?;
                payload.extend(&amount.to_be_bytes())?;
            }
            Self::TransferFrom { owner, to, amount } => {
                nonzero(amount)?;
                payload.extend(&owner.bytes())?;
                payload.extend(&to.bytes())?;
                payload.extend(&amount.to_be_bytes())?;
            }
            Self::BalanceOf { owner } => payload.extend(&owner.bytes())?,
            Self::Allowance { owner, spender } => {
                payload.extend(&owner.bytes())?;
                payload.extend(&spender.bytes())?;
            }
            Self::TotalSupply | Self::Metadata => {}
        }
        let mut output = Bytes::empty();
        output.extend(&self.discriminator())?;
        output.extend(&[1, 0x20])?;
        let length = u32::try_from(payload.len()).map_err(|_| malformed())?;
        output.extend(&length.to_be_bytes())?;
        output.extend(payload.as_slice())?;
        Ok(output)
    }

    /// # Errors
    /// Refuses unknown selectors, wrong tags, lengths, trailing bytes, reserved
    /// identifiers and zero transfers.
    pub fn decode(input: &[u8]) -> Result<Self, ProgramError> {
        if input.get(..3) != Some(&[b'L', b'X', 20]) || input.get(4..6) != Some(&[1, 0x20]) {
            return Err(malformed());
        }
        let length =
            usize::try_from(u32::from_be_bytes(array(input, 6)?)).map_err(|_| malformed())?;
        let payload = input.get(10..).ok_or_else(malformed)?;
        if payload.len() != length || length > 80 {
            return Err(malformed());
        }
        let request = match (input[3], length) {
            (1, 48) => Self::Transfer {
                to: AccountId::new(array(payload, 0)?)?,
                amount: Amount::from_be_bytes(array(payload, 32)?),
            },
            (2, 48) => Self::Approve {
                spender: Principal::new(array(payload, 0)?)?,
                amount: Amount::from_be_bytes(array(payload, 32)?),
            },
            (3, 80) => Self::TransferFrom {
                owner: Principal::new(array(payload, 0)?)?,
                to: AccountId::new(array(payload, 32)?)?,
                amount: Amount::from_be_bytes(array(payload, 64)?),
            },
            (4, 32) => Self::BalanceOf {
                owner: Principal::new(array(payload, 0)?)?,
            },
            (5, 64) => Self::Allowance {
                owner: Principal::new(array(payload, 0)?)?,
                spender: Principal::new(array(payload, 32)?)?,
            },
            (6, 0) => Self::TotalSupply,
            (7, 0) => Self::Metadata,
            _ => return Err(malformed()),
        };
        request.encode()?;
        Ok(request)
    }
}

fn array<const N: usize>(input: &[u8], offset: usize) -> Result<[u8; N], ProgramError> {
    input
        .get(offset..offset + N)
        .ok_or_else(malformed)?
        .try_into()
        .map_err(|_| malformed())
}

fn malformed() -> ProgramError {
    ProgramError::value(Field::CallInput, Reason::Malformed)
}

fn nonzero(amount: Amount) -> Result<(), ProgramError> {
    if amount.is_zero() {
        Err(ProgramError::value(Field::Amount, Reason::Zero))
    } else {
        Ok(())
    }
}

pub const REFERENCE_ASSET: [u8; 32] = [0x44; 32];
pub const REFERENCE_ISSUER_DID: &str = "did:lxp:token-issuer";
pub const REFERENCE_ISSUER: [u8; 32] = [
    169, 78, 156, 227, 126, 170, 174, 185, 0, 3, 18, 206, 26, 39, 12, 168, 40, 21, 203, 244, 203,
    82, 107, 199, 155, 197, 120, 202, 151, 153, 242, 143,
];
pub const REFERENCE_SUPPLY: u128 = 1_000_000;
pub const REFERENCE_CEILING: u128 = 100_000;
pub const REFERENCE_METADATA: &[u8] = b"LXT-20 settlement reference|LXT|18";

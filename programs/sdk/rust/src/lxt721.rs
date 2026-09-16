use crate::{Bytes, Field, Principal, ProgramError, Reason};

pub const STANDARD: u16 = 721;

const TAG: u8 = STANDARD.to_le_bytes()[0];

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct TokenId(u128);

impl TokenId {
    /// # Errors
    /// Refuses the zero identifier, which is reserved for absence.
    pub const fn new(value: u128) -> Result<Self, ProgramError> {
        if value == 0 {
            return Err(ProgramError::value(Field::CallInput, Reason::Zero));
        }
        Ok(Self(value))
    }

    /// # Errors
    /// Refuses the zero identifier, which is reserved for absence.
    pub const fn from_be_bytes(bytes: [u8; 16]) -> Result<Self, ProgramError> {
        Self::new(u128::from_be_bytes(bytes))
    }

    #[must_use]
    pub const fn to_be_bytes(self) -> [u8; 16] {
        self.0.to_be_bytes()
    }

    #[must_use]
    pub const fn value(self) -> u128 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Request {
    Mint { to: Principal, token: TokenId },
    Transfer { to: Principal, token: TokenId },
    Approve { spender: Principal, token: TokenId },
    SetApprovalForAll { operator: Principal, approved: bool },
    OwnerOf { token: TokenId },
    BalanceOf { owner: Principal },
    TokenUri { token: TokenId },
    TotalSupply,
    Metadata,
}

impl Request {
    #[must_use]
    pub const fn method(self) -> &'static str {
        match self {
            Self::Mint { .. } => "mint",
            Self::Transfer { .. } => "transfer",
            Self::Approve { .. } => "approve",
            Self::SetApprovalForAll { .. } => "set_approval_for_all",
            Self::OwnerOf { .. } => "owner_of",
            Self::BalanceOf { .. } => "balance_of",
            Self::TokenUri { .. } => "token_uri",
            Self::TotalSupply => "total_supply",
            Self::Metadata => "metadata",
        }
    }

    #[must_use]
    pub const fn discriminator(self) -> [u8; 4] {
        [
            b'L',
            b'X',
            TAG,
            match self {
                Self::Mint { .. } => 1,
                Self::Transfer { .. } => 2,
                Self::Approve { .. } => 3,
                Self::SetApprovalForAll { .. } => 4,
                Self::OwnerOf { .. } => 5,
                Self::BalanceOf { .. } => 6,
                Self::TokenUri { .. } => 7,
                Self::TotalSupply => 8,
                Self::Metadata => 9,
            },
        ]
    }

    /// Encodes a discriminator followed by canonical `LayerX` bounded bytes.
    /// # Errors
    /// Refuses the reserved zero token identifier.
    pub fn encode(self) -> Result<Bytes<58>, ProgramError> {
        let mut payload = Bytes::<48>::empty();
        match self {
            Self::Mint { to, token }
            | Self::Transfer { to, token }
            | Self::Approve { spender: to, token } => {
                payload.extend(&to.bytes())?;
                payload.extend(&token.to_be_bytes())?;
            }
            Self::SetApprovalForAll { operator, approved } => {
                payload.extend(&operator.bytes())?;
                payload.extend(&[u8::from(approved)])?;
            }
            Self::OwnerOf { token } | Self::TokenUri { token } => {
                payload.extend(&token.to_be_bytes())?;
            }
            Self::BalanceOf { owner } => payload.extend(&owner.bytes())?,
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
    /// identifiers and noncanonical approval flags.
    pub fn decode(input: &[u8]) -> Result<Self, ProgramError> {
        if input.get(..3) != Some(&[b'L', b'X', TAG]) || input.get(4..6) != Some(&[1, 0x20]) {
            return Err(malformed());
        }
        let length =
            usize::try_from(u32::from_be_bytes(array(input, 6)?)).map_err(|_| malformed())?;
        let payload = input.get(10..).ok_or_else(malformed)?;
        if payload.len() != length || length > 48 {
            return Err(malformed());
        }
        let request = match (input[3], length) {
            (1, 48) => Self::Mint {
                to: Principal::new(array(payload, 0)?)?,
                token: TokenId::from_be_bytes(array(payload, 32)?)?,
            },
            (2, 48) => Self::Transfer {
                to: Principal::new(array(payload, 0)?)?,
                token: TokenId::from_be_bytes(array(payload, 32)?)?,
            },
            (3, 48) => Self::Approve {
                spender: Principal::new(array(payload, 0)?)?,
                token: TokenId::from_be_bytes(array(payload, 32)?)?,
            },
            (4, 33) => Self::SetApprovalForAll {
                operator: Principal::new(array(payload, 0)?)?,
                approved: flag(payload[32])?,
            },
            (5, 16) => Self::OwnerOf {
                token: TokenId::from_be_bytes(array(payload, 0)?)?,
            },
            (6, 32) => Self::BalanceOf {
                owner: Principal::new(array(payload, 0)?)?,
            },
            (7, 16) => Self::TokenUri {
                token: TokenId::from_be_bytes(array(payload, 0)?)?,
            },
            (8, 0) => Self::TotalSupply,
            (9, 0) => Self::Metadata,
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

fn flag(byte: u8) -> Result<bool, ProgramError> {
    match byte {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(malformed()),
    }
}

fn malformed() -> ProgramError {
    ProgramError::value(Field::CallInput, Reason::Malformed)
}

pub const REFERENCE_ISSUER_DID: &str = "did:lxp:nft-issuer";
pub const REFERENCE_ISSUER: [u8; 32] = [
    36, 159, 129, 145, 150, 79, 127, 231, 235, 13, 86, 215, 43, 253, 58, 102, 224, 143, 149, 51,
    54, 89, 248, 92, 82, 63, 29, 58, 216, 172, 20, 141,
];
pub const REFERENCE_MAX_SUPPLY: u128 = 10_000;
pub const REFERENCE_BASE_URI: &[u8] = b"lxp:nft:reference/";
pub const REFERENCE_METADATA: &[u8] = b"LXT-721 collection reference|LXNFT";

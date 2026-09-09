//! Fixture encoders for version-one native asset payloads.
//!
//! `Register` encodes issuer kind 1 with an empty custody reference. Other
//! operations delegate to [`layerx_crypto::payments::Payment`].

pub use layerx_crypto::payments::asset_id;
use layerx_crypto::payments::Payment;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetRegistration<'a> {
    pub issuer: &'a str,
    pub salt: [u8; 32],
    pub symbol: &'a str,
    pub name: &'a str,
    pub decimals: u8,
    pub supply_cap: u128,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AssetOperation<'a> {
    Register(AssetRegistration<'a>),
    OpenAccount {
        asset: [u8; 32],
    },
    RevokeGrant {
        grant: [u8; 32],
        sequence: u64,
    },
    Mint {
        asset: [u8; 32],
        account: [u8; 32],
        amount: u128,
    },
    Burn {
        asset: [u8; 32],
        account: [u8; 32],
        amount: u128,
    },
}

impl AssetOperation<'_> {
    #[must_use]
    pub const fn ordinal(&self) -> u16 {
        match self {
            Self::Register(_) => 1,
            Self::OpenAccount { .. } => 4,
            Self::RevokeGrant { .. } => 8,
            Self::Mint { .. } => 10,
            Self::Burn { .. } => 11,
        }
    }

    /// Encodes the version-one asset payload without an envelope or signature.
    ///
    /// # Errors
    /// Refuses invalid metadata bounds, decimals above 38, and zero amounts.
    pub fn encode(&self) -> Result<Vec<u8>, String> {
        match self {
            Self::Register(registration) => {
                let issuer = native_issuer_id(registration.issuer)?;
                Payment::Register(layerx_crypto::payments::Registration {
                    asset: asset_id(&issuer, &registration.salt),
                    salt: registration.salt,
                    symbol: registration.symbol.into(),
                    name: registration.name.into(),
                    decimals: registration.decimals,
                    supply_cap: registration.supply_cap,
                    issuer_kind: 1,
                    custody_ref: Vec::new(),
                })
                .encode(registration.issuer.as_bytes())
                .map_err(|e| e.to_string())
            }
            Self::OpenAccount { asset } => Payment::OpenAccount { asset: *asset }
                .encode(&[])
                .map_err(|e| e.to_string()),
            Self::RevokeGrant { grant, sequence } => Payment::RevokeGrant {
                grant: *grant,
                revocation_sequence: *sequence,
            }
            .encode(&[])
            .map_err(|e| e.to_string()),
            Self::Mint {
                asset,
                account,
                amount,
            } => Payment::Mint {
                asset: *asset,
                to: *account,
                amount: *amount,
            }
            .encode(&[])
            .map_err(|e| e.to_string()),
            Self::Burn {
                asset,
                account,
                amount,
            } => Payment::Burn {
                asset: *asset,
                from: *account,
                amount: *amount,
            }
            .encode(&[])
            .map_err(|e| e.to_string()),
        }
    }
}

/// # Errors
/// Requires a valid DID before deriving the native identity identifier.
pub fn native_issuer_id(did: &str) -> Result<[u8; 32], String> {
    use sha2::Digest as _;
    layerx_types::ids::Did::new(did.as_bytes()).map_err(|e| format!("invalid DID: {e:?}"))?;
    let length = u16::try_from(did.len()).map_err(|e| e.to_string())?;
    let mut hash = sha2::Sha256::new();
    hash.update(b"LXP/v1/did-id\0");
    hash.update(length.to_be_bytes());
    hash.update(did.as_bytes());
    Ok(hash.finalize().into())
}

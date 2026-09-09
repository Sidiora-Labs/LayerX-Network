pub use layerx_crypto::payments::asset_id;
use layerx_crypto::payments::Payment;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetRegistration<'a> {
    pub issuer: [u8; 32],
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
        let mut bytes = 1_u16.to_be_bytes().to_vec();
        match self {
            Self::Register(registration) => {
                let symbol_len = bounded_length(registration.symbol, 16, "symbol")?;
                let name_len = bounded_length(registration.name, 32, "name")?;
                if !registration.symbol.is_ascii() {
                    return Err("asset symbol must be ASCII".into());
                }
                if registration.decimals > 38 {
                    return Err("asset decimals must be at most 38".into());
                }
                bytes.extend_from_slice(&asset_id(&registration.issuer, &registration.salt));
                bytes.extend_from_slice(&registration.salt);
                bytes.push(symbol_len);
                bytes.extend_from_slice(registration.symbol.as_bytes());
                bytes.push(name_len);
                bytes.extend_from_slice(registration.name.as_bytes());
                bytes.push(registration.decimals);
                bytes.extend_from_slice(&registration.supply_cap.to_be_bytes());
                bytes.extend_from_slice(&[1, 0]);
            }
            Self::OpenAccount { asset } => {
                return Payment::OpenAccount { asset: *asset }
                    .encode(&[])
                    .map_err(|e| e.to_string())
            }
            Self::RevokeGrant { grant, sequence } => {
                return Payment::RevokeGrant {
                    grant: *grant,
                    revocation_sequence: *sequence,
                }
                .encode(&[])
                .map_err(|e| e.to_string())
            }
            Self::Mint {
                asset,
                account,
                amount,
            } => {
                return Payment::Mint {
                    asset: *asset,
                    to: *account,
                    amount: *amount,
                }
                .encode(&[])
                .map_err(|e| e.to_string())
            }
            Self::Burn {
                asset,
                account,
                amount,
            } => {
                return Payment::Burn {
                    asset: *asset,
                    from: *account,
                    amount: *amount,
                }
                .encode(&[])
                .map_err(|e| e.to_string())
            }
        }
        Ok(bytes)
    }
}

fn bounded_length(value: &str, maximum: u8, field: &str) -> Result<u8, String> {
    let length = u8::try_from(value.len()).map_err(|_| format!("asset {field} is too long"))?;
    if length == 0 || length > maximum {
        return Err(format!("asset {field} must contain 1..{maximum} bytes"));
    }
    Ok(length)
}

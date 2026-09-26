//! The four assets a paid route accepts: SID, PAX, USDC and USDL, each named
//! by its registered kernel asset id and priced per request in its own base
//! units. `AcceptedAssets` is the only way the payment path learns them, and
//! it refuses a missing, extra, repeated, zero or placeholder asset.

use crate::config::{AssetConfig, AssetSymbol};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssetRefusal {
    Missing(AssetSymbol),
    Extra,
    Repeated(AssetSymbol),
    ZeroId(AssetSymbol),
    PlaceholderId(AssetSymbol),
    DuplicateId(AssetSymbol),
    PlaceholderPrice(AssetSymbol),
}

impl std::fmt::Display for AssetRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing(symbol) => write!(f, "asset {} is missing", symbol.code()),
            Self::Extra => f.write_str("more than the four accepted assets are configured"),
            Self::Repeated(symbol) => write!(f, "asset {} is configured twice", symbol.code()),
            Self::ZeroId(symbol) => write!(f, "asset {} has a zero id", symbol.code()),
            Self::PlaceholderId(symbol) => {
                write!(f, "asset {} has a placeholder id", symbol.code())
            }
            Self::DuplicateId(symbol) => {
                write!(
                    f,
                    "asset {} shares its id with another asset",
                    symbol.code()
                )
            }
            Self::PlaceholderPrice(symbol) => {
                write!(f, "asset {} has a placeholder price", symbol.code())
            }
        }
    }
}

impl std::error::Error for AssetRefusal {}

/// One accepted asset: its symbol, kernel asset id and price per request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AcceptedAsset {
    pub symbol: AssetSymbol,
    pub asset_id: [u8; 32],
    pub price: u128,
}

impl AcceptedAsset {
    /// The asset id as 64 lowercase hexadecimal characters.
    #[must_use]
    pub fn id_hex(&self) -> String {
        crate::payment::hex(&self.asset_id)
    }
}

/// Exactly SID, PAX, USDC and USDL, in that order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AcceptedAssets {
    assets: [AcceptedAsset; 4],
}

impl AcceptedAssets {
    /// # Errors
    /// Refuses a missing, extra or repeated asset, a zero id, an id whose
    /// bytes are all equal, an id shared by two assets and a zero price.
    pub fn new(configured: &[AssetConfig]) -> Result<Self, AssetRefusal> {
        let mut assets = [AcceptedAsset {
            symbol: AssetSymbol::Sid,
            asset_id: [0; 32],
            price: 0,
        }; 4];
        for (slot, symbol) in assets.iter_mut().zip(AssetSymbol::ALL) {
            let mut matching = configured.iter().filter(|asset| asset.symbol == symbol);
            let asset = matching.next().ok_or(AssetRefusal::Missing(symbol))?;
            if matching.next().is_some() {
                return Err(AssetRefusal::Repeated(symbol));
            }
            if asset.asset_id == [0; 32] {
                return Err(AssetRefusal::ZeroId(symbol));
            }
            if asset.asset_id.iter().all(|byte| *byte == asset.asset_id[0]) {
                return Err(AssetRefusal::PlaceholderId(symbol));
            }
            if asset.price == 0 {
                return Err(AssetRefusal::PlaceholderPrice(symbol));
            }
            *slot = AcceptedAsset {
                symbol,
                asset_id: asset.asset_id,
                price: asset.price,
            };
        }
        if configured.len() != AssetSymbol::ALL.len() {
            return Err(AssetRefusal::Extra);
        }
        for (index, asset) in assets.iter().enumerate() {
            if assets[..index]
                .iter()
                .any(|earlier| earlier.asset_id == asset.asset_id)
            {
                return Err(AssetRefusal::DuplicateId(asset.symbol));
            }
        }
        Ok(Self { assets })
    }

    #[must_use]
    pub const fn all(&self) -> &[AcceptedAsset; 4] {
        &self.assets
    }

    #[must_use]
    pub fn get(&self, symbol: AssetSymbol) -> &AcceptedAsset {
        let index = AssetSymbol::ALL
            .iter()
            .position(|candidate| *candidate == symbol)
            .unwrap_or_default();
        &self.assets[index]
    }

    #[must_use]
    pub fn by_id(&self, asset_id: &[u8; 32]) -> Option<&AcceptedAsset> {
        self.assets.iter().find(|asset| asset.asset_id == *asset_id)
    }
}

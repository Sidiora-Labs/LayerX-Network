use layerx_platform_cli::wallet_encoding::{asset_id, AssetOperation, AssetRegistration};
use serde_json::Value;

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(char::from(DIGITS[usize::from(byte >> 4)]));
        result.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    result
}

fn registration<'a>() -> AssetRegistration<'a> {
    AssetRegistration {
        issuer: "did:layerx:alice",
        salt: core::array::from_fn(|index| {
            u8::try_from(index + 32).unwrap_or_else(|error| panic!("fixture index: {error}"))
        }),
        symbol: "PAY",
        name: "Payment €",
        decimals: 38,
        supply_cap: u128::MAX,
    }
}

#[test]
fn canonical_asset_vectors() -> Result<(), String> {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/wallet-assets-v1.json"))
        .map_err(|error| error.to_string())?;
    let registration = registration();
    let issuer = layerx_platform_cli::wallet_encoding::native_issuer_id(registration.issuer)?;
    let asset = asset_id(&issuer, &registration.salt);
    let account = core::array::from_fn(|index| {
        u8::try_from(index + 64).unwrap_or_else(|error| panic!("fixture index: {error}"))
    });
    let amount = (1_u128 << 127) + 257;
    assert_eq!(hex(&issuer), fixture["issuer"]);
    assert_eq!(hex(&registration.salt), fixture["salt"]);
    assert_eq!(hex(&asset), fixture["asset_id"]);
    assert_eq!(hex(&account), fixture["account"]);
    assert_eq!(amount.to_string(), fixture["amount"]);
    for (key, ordinal, operation) in [
        ("register", 1, AssetOperation::Register(registration)),
        ("open_account", 4, AssetOperation::OpenAccount { asset }),
        (
            "revoke_grant",
            8,
            AssetOperation::RevokeGrant {
                grant: account,
                sequence: 0x0102_0304_0506_0708,
            },
        ),
        (
            "mint",
            10,
            AssetOperation::Mint {
                asset,
                account,
                amount,
            },
        ),
        (
            "burn",
            11,
            AssetOperation::Burn {
                asset,
                account,
                amount,
            },
        ),
    ] {
        assert_eq!(operation.ordinal(), ordinal);
        assert_eq!(hex(&operation.encode()?), fixture[key]);
    }
    Ok(())
}

#[test]
fn metadata_bounds_and_amount_refusals() {
    for symbol in ["", "ABCDEFGHIJKLMNOPQ", "é"] {
        let mut value = registration();
        value.symbol = symbol;
        assert!(AssetOperation::Register(value).encode().is_err());
    }
    for name in ["", "012345678901234567890123456789012", "€€€€€€€€€€€"] {
        let mut value = registration();
        value.name = name;
        assert!(AssetOperation::Register(value).encode().is_err());
    }
    let mut value = registration();
    value.decimals = 39;
    assert!(AssetOperation::Register(value).encode().is_err());
    for operation in [
        AssetOperation::Mint {
            asset: [1; 32],
            account: [2; 32],
            amount: 0,
        },
        AssetOperation::Burn {
            asset: [1; 32],
            account: [2; 32],
            amount: 0,
        },
    ] {
        assert!(operation.encode().is_err());
    }
}

#[test]
fn exact_bounds_uncapped_and_domain_separation() -> Result<(), String> {
    let mut value = registration();
    value.symbol = "ABCDEFGHIJKLMNOP";
    value.name = "01234567890123456789012345678901";
    value.decimals = 0;
    value.supply_cap = 0;
    let encoded = AssetOperation::Register(value.clone()).encode()?;
    assert_eq!(encoded.len(), 135);
    assert_eq!(&encoded[117..133], &[0; 16]);
    assert_eq!(&encoded[133..], &[1, 0]);
    assert_ne!(
        asset_id(
            &layerx_platform_cli::wallet_encoding::native_issuer_id(value.issuer)?,
            &value.salt
        ),
        asset_id(
            &value.salt,
            &layerx_platform_cli::wallet_encoding::native_issuer_id(value.issuer)?
        )
    );
    let mut salt = value.salt;
    salt[31] ^= 1;
    assert_ne!(
        asset_id(
            &layerx_platform_cli::wallet_encoding::native_issuer_id(value.issuer)?,
            &value.salt
        ),
        asset_id(
            &layerx_platform_cli::wallet_encoding::native_issuer_id(value.issuer)?,
            &salt
        )
    );
    Ok(())
}

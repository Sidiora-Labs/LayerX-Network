//! Reads a live Paxeer endpoint the way the boundary does after the switch to
//! the native custody precompile: the published deposit-root registration is
//! still fetched from the registry log, while custody state itself is now read
//! from `0x…1013` and finality from the anchor precompile `0x…1014`. Nothing
//! here is simulated — every value comes back from the endpoint named on the
//! command line.

use std::time::Duration;

use ed25519_dalek::{Signature, VerifyingKey};
use layerx_paxeer_client::custody::{
    base_units_from_wei, decode_asset, decode_bool, exit_eligible_calldata, get_asset_calldata,
    native_asset_id_calldata, native_value_wei, WEI_PER_BASE_UNIT,
};
use layerx_paxeer_client::{
    deposit_root_registration_message, raw_call, CustodyDeposit, EndpointConfig, EndpointTransport,
    Json, PublishedDepositProof, ANCHOR_PRECOMPILE, CUSTODY_PRECOMPILE,
};
use layerx_types::{amount::Amount, ids::AssetId, intent::EvmAddress};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// `latestFinalized()`, `finalizedStateRoot(uint64)`, `finalizedReceiptRoot(uint64)`.
const SELECTOR_LATEST_FINALIZED: [u8; 4] = [0x6c, 0xdd, 0x45, 0xae];
const SELECTOR_FINALIZED_STATE_ROOT: [u8; 4] = [0x0f, 0x60, 0x7f, 0xe4];
const SELECTOR_FINALIZED_RECEIPT_ROOT: [u8; 4] = [0xe0, 0xa3, 0xcc, 0xaa];

fn bytes<const N: usize>(value: &str) -> Result<[u8; N]> {
    let value = value.strip_prefix("0x").ok_or("hex prefix")?;
    if value.len() != N * 2 {
        return Err("hex length".into());
    }
    let mut result = [0; N];
    for (i, byte) in result.iter_mut().enumerate() {
        *byte = u8::from_str_radix(value.get(i * 2..i * 2 + 2).ok_or("hex digits")?, 16)?;
    }
    Ok(result)
}

fn hex(value: &[u8]) -> String {
    let mut text = String::from("0x");
    for byte in value {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

fn checked<T, E: std::fmt::Debug>(result: std::result::Result<T, E>) -> Result<T> {
    result.map_err(|error| format!("{error:?}").into())
}

fn endpoint(url: &str, chain_id: u64) -> EndpointConfig {
    EndpointConfig {
        url: url.to_owned(),
        request_timeout: Duration::from_secs(30),
        transport: EndpointTransport::LocalEmulator,
        expected_chain_id: chain_id,
    }
}

/// One `eth_call` against a precompile, returned as the raw answer bytes.
fn view(endpoint: &EndpointConfig, target: EvmAddress, data: &[u8]) -> Result<Vec<u8>> {
    let answer = checked(raw_call(
        endpoint,
        "eth_call",
        &[
            Json::Object(vec![
                ("to".to_owned(), Json::Text(hex(&target.bytes()))),
                ("data".to_owned(), Json::Text(hex(data))),
            ]),
            Json::Text("latest".to_owned()),
        ],
    ))?;
    let Json::Text(text) = &answer else {
        return Err("eth_call: expected a data string".into());
    };
    let digits = text.strip_prefix("0x").ok_or("eth_call: hex prefix")?;
    if digits.len() % 2 != 0 {
        return Err("eth_call: odd hex".into());
    }
    (0..digits.len())
        .step_by(2)
        .map(|at| {
            Ok(u8::from_str_radix(
                digits.get(at..at + 2).ok_or("eth_call: hex digits")?,
                16,
            )?)
        })
        .collect()
}

/// Splits the `(value, bool)` pair every anchor view returns.
fn optional_word(answer: &[u8]) -> Result<Option<[u8; 32]>> {
    if answer.len() != 64 {
        return Err("anchor view: expected two words".into());
    }
    let value: [u8; 32] = answer.get(..32).ok_or("anchor view")?.try_into()?;
    let present: [u8; 32] = answer.get(32..).ok_or("anchor view")?.try_into()?;
    match present[31] {
        0 if present[..31].iter().all(|byte| *byte == 0) => Ok(None),
        1 if present[..31].iter().all(|byte| *byte == 0) => Ok(Some(value)),
        _ => Err("anchor view: expected a boolean".into()),
    }
}

fn anchor_view(
    endpoint: &EndpointConfig,
    selector: [u8; 4],
    batch: u64,
) -> Result<Option<[u8; 32]>> {
    let mut data = selector.to_vec();
    let mut word = [0_u8; 32];
    word[24..].copy_from_slice(&batch.to_be_bytes());
    data.extend_from_slice(&word);
    optional_word(&view(endpoint, ANCHOR_PRECOMPILE, &data)?)
}

/// The deposit-root registration is still published to the registry contract;
/// this is the whole surviving publication path.
fn fetch_deposit_registration(args: &[String], chain_id: u64) -> Result<[u8; 32]> {
    let endpoint = endpoint(args.first().ok_or("endpoint")?, chain_id);
    let registry = EvmAddress::new(bytes(args.get(1).ok_or("registry")?)?);
    let vault = EvmAddress::new(bytes(args.get(2).ok_or("vault")?)?);
    let checkpoint: [u8; 32] = bytes(args.get(3).ok_or("checkpoint")?)?;
    let amount: u128 = args.get(9).ok_or("amount")?.parse()?;
    let custody = CustodyDeposit {
        deposit_id: bytes(args.get(4).ok_or("deposit id")?)?,
        asset: AssetId::new(bytes(args.get(6).ok_or("asset")?)?),
        payer: EvmAddress::new(bytes(args.get(7).ok_or("payer")?)?),
        beneficiary: bytes(args.get(5).ok_or("account")?)?,
        amount: Amount::from_u128(amount),
        nonce: args.get(8).ok_or("nonce")?.parse()?,
    };
    let deposit = checked(PublishedDepositProof::fetch_published(
        &endpoint, vault, registry, checkpoint, custody, 1,
    ))?;
    assert_eq!(deposit.registration.checkpoint_id, checkpoint);
    assert_ne!(deposit.registration.deposit_root, [0; 32]);
    assert_ne!(deposit.registration.protocol_version, 0);

    let authority = VerifyingKey::from_bytes(&bytes(args.get(10).ok_or("authority key")?)?)?;
    authority.verify_strict(
        &checked(deposit_root_registration_message(&deposit.registration))?,
        &Signature::from_bytes(&deposit.registration.signature),
    )?;

    // A checkpoint nothing was published under is refused, never invented.
    let mut absent = checkpoint;
    absent[0] ^= 1;
    assert!(PublishedDepositProof::fetch_published(
        &endpoint,
        vault,
        registry,
        absent,
        CustodyDeposit {
            deposit_id: bytes(args.get(4).ok_or("deposit id")?)?,
            asset: AssetId::new(bytes(args.get(6).ok_or("asset")?)?),
            payer: EvmAddress::new(bytes(args.get(7).ok_or("payer")?)?),
            beneficiary: bytes(args.get(5).ok_or("account")?)?,
            amount: Amount::from_u128(amount),
            nonce: args.get(8).ok_or("nonce")?.parse()?,
        },
        1,
    )
    .is_err());

    // The custody precompile denominates the native coin in base units.
    let wei = checked(native_value_wei(amount))?;
    assert_eq!(checked(base_units_from_wei(&wei))?, amount);
    assert_eq!(
        u128::from_be_bytes(wei.get(16..).ok_or("wei")?.try_into()?),
        amount.saturating_mul(WEI_PER_BASE_UNIT)
    );
    println!(
        "deposit root registration fetched and verified: checkpoint {} root {}",
        hex(&deposit.registration.checkpoint_id),
        hex(&deposit.registration.deposit_root)
    );
    Ok(deposit.registration.checkpoint_state_root)
}

/// Custody state now comes from the precompiles, not from a registry log.
fn read_precompiles(endpoint: &EndpointConfig) -> Result<()> {
    let answer = view(endpoint, CUSTODY_PRECOMPILE, &native_asset_id_calldata())?;
    let asset_id: [u8; 32] = answer
        .as_slice()
        .try_into()
        .map_err(|_| "nativeAssetId: expected one word")?;
    assert_ne!(asset_id, [0; 32]);
    let asset = checked(decode_asset(&view(
        endpoint,
        CUSTODY_PRECOMPILE,
        &get_asset_calldata(asset_id),
    )?))?;
    assert_eq!(asset.asset_id, asset_id);
    assert!(asset.enabled && !asset.paused);
    assert!(!asset.denom.is_empty());
    assert_eq!(asset.pointer, EvmAddress::new([0; 20]));

    let eligible = checked(decode_bool(&view(
        endpoint,
        CUSTODY_PRECOMPILE,
        &exit_eligible_calldata(),
    )?))?;

    let latest = optional_word(&view(
        endpoint,
        ANCHOR_PRECOMPILE,
        &SELECTOR_LATEST_FINALIZED,
    )?)?;
    let Some(word) = latest else {
        println!(
            "custody asset {} ({}); no finalized checkpoint yet; forced exit eligible: {eligible}",
            asset.denom,
            hex(&asset_id)
        );
        return Ok(());
    };
    if word[..24].iter().any(|byte| *byte != 0) {
        return Err("latestFinalized: quantity exceeds u64".into());
    }
    let batch = u64::from_be_bytes(word.get(24..).ok_or("batch")?.try_into()?);
    let state_root = anchor_view(endpoint, SELECTOR_FINALIZED_STATE_ROOT, batch)?
        .ok_or("finalizedStateRoot: latest batch has no finalized root")?;
    let receipt_root = anchor_view(endpoint, SELECTOR_FINALIZED_RECEIPT_ROOT, batch)?
        .ok_or("finalizedReceiptRoot: latest batch has no finalized root")?;
    assert_ne!(state_root, [0; 32]);
    assert_ne!(receipt_root, [0; 32]);
    // Nothing is finalized beyond the latest batch the anchor declares.
    assert!(anchor_view(
        endpoint,
        SELECTOR_FINALIZED_STATE_ROOT,
        batch.saturating_add(1)
    )?
    .is_none());
    println!(
        "custody asset {} ({}); anchor batch {batch} state {} receipt {}; forced exit eligible: {eligible}",
        asset.denom,
        hex(&asset_id),
        hex(&state_root),
        hex(&receipt_root)
    );
    Ok(())
}

fn main() -> Result<()> {
    let inputs: Vec<String> = std::env::args().skip(1).collect();
    let (args, chain_id) = if inputs.first().is_some_and(|v| v == "--chain-id") {
        let value = inputs.get(1).ok_or("missing chain id")?;
        let chain_id: u64 = value.parse()?;
        if chain_id == 0 || chain_id.to_string() != *value {
            return Err("invalid chain id".into());
        }
        (inputs.get(2..).ok_or("missing inputs")?, chain_id)
    } else {
        (inputs.as_slice(), 31_337)
    };
    if args.len() != 11 {
        return Err("expected: <endpoint> <registry> <vault> <checkpoint> <deposit-id> <account> <asset> <payer> <nonce> <amount> <authority-key>".into());
    }
    let state_root = fetch_deposit_registration(args, chain_id)?;
    assert_ne!(state_root, [0; 32]);
    let endpoint = endpoint(args.first().ok_or("endpoint")?, chain_id);
    read_precompiles(&endpoint)?;
    println!("native custody precompile state read from {}", endpoint.url);
    Ok(())
}

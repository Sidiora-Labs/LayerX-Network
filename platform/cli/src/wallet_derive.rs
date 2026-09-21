//! `layerx wallet derive`: one mnemonic, one account on both sides of the
//! Paxeer X Network. The phrase is read from standard input or a file, never
//! from the command line, and private keys leave the process only through an
//! explicit export to a new owner-only file.

use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};

use clap::Args;
use layerx_client::account_binding::{
    bind_nonce_call, bound_did_call, decode_bind_nonce, decode_bound_did, plan_bind,
    sign_bind_transaction, BindError, BindFees, BindPlan, ADDR_PRECOMPILE,
};
use layerx_crypto::account_derivation::{
    derive_from_mnemonic, evm_path, layerx_path, DerivedAccount,
};
use layerx_platform_cli::rpc::RpcClient;
use serde_json::{json, Value};
use zeroize::Zeroizing;

use crate::encoding::{hex_decode, hex_encode};
use crate::output::CommandOutput;

const MAX_SECRET_BYTES: u64 = 4096;

#[derive(Args)]
pub struct DeriveArgs {
    /// Read the BIP-39 phrase from this file instead of standard input.
    #[arg(long)]
    mnemonic_file: Option<PathBuf>,
    /// Read the optional BIP-39 passphrase from this file.
    #[arg(long)]
    passphrase_file: Option<PathBuf>,
    /// Account index, the same on the EVM and the `LayerX` side.
    #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u32).range(0..=0x7fff_ffff))]
    index: u32,
    /// Write both private keys to a new file only its owner can read.
    #[arg(long)]
    export_private_keys: Option<PathBuf>,
    /// Bind the pair on-chain through the addr precompile; needs --rpc.
    #[arg(long)]
    bind: bool,
}

fn read_bounded(mut reader: impl std::io::Read, source: &str) -> Result<Zeroizing<String>, String> {
    let mut value = Zeroizing::new(String::new());
    reader
        .by_ref()
        .take(MAX_SECRET_BYTES + 1)
        .read_to_string(&mut value)
        .map_err(|error| format!("could not read {source}: {error}"))?;
    if value.len() as u64 > MAX_SECRET_BYTES {
        return Err(format!("{source} exceeds {MAX_SECRET_BYTES} bytes"));
    }
    Ok(value)
}

fn read_file(path: &Path, what: &str) -> Result<Zeroizing<String>, String> {
    let file = std::fs::File::open(path)
        .map_err(|error| format!("could not open {what} file {}: {error}", path.display()))?;
    read_bounded(file, what)
}

fn export(path: &Path, account: &DerivedAccount) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt as _;
    let body = Zeroizing::new(format!(
        "{}\n",
        json!({
            "index": account.index(),
            "evm_address": account.evm_address_text(),
            "evm_private_key": account.evm_secret().map(|key| hex_encode(key)),
            "did": account.did(),
            "layerx_seed": hex_encode(account.layerx_seed()),
        })
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| {
            format!(
                "private_key_export_refused: could not create {}: {error}",
                path.display()
            )
        })?;
    file.write_all(body.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("could not write {}: {error}", path.display()))
}

fn quantity(value: &Value, what: &str) -> Result<u128, String> {
    let text = value
        .as_str()
        .and_then(|text| text.strip_prefix("0x"))
        .filter(|text| !text.is_empty() && text.len() <= 32)
        .ok_or_else(|| format!("{what} is not a hexadecimal quantity"))?;
    u128::from_str_radix(text, 16).map_err(|_| format!("{what} is not a hexadecimal quantity"))
}

fn eth_call(rpc: &RpcClient, data: &[u8]) -> Result<Vec<u8>, String> {
    let answer = rpc.call(
        "eth_call",
        &json!([{"to": format!("0x{}", hex_encode(&ADDR_PRECOMPILE)), "data": format!("0x{}", hex_encode(data))}, "latest"]),
    )?;
    let text = answer
        .as_str()
        .and_then(|text| text.strip_prefix("0x"))
        .ok_or("eth_call answer is not hexadecimal")?;
    hex_decode("eth_call answer", text)
}

fn bind_error(error: BindError) -> String {
    match error {
        BindError::BoundToDifferentDid { bound } => format!(
            "bound_to_different_did: this address is already bound to did:layerx:{}; nothing was sent",
            hex_encode(&bound)
        ),
        other => format!("{other}: binding was not sent"),
    }
}

fn bind(rpc: &RpcClient, account: &DerivedAccount) -> Result<Value, String> {
    let chain_id = u64::try_from(quantity(&rpc.call("eth_chainId", &json!([]))?, "chain id")?)
        .map_err(|_| "chain id exceeds 64 bits".to_owned())?;
    let address = account.evm_address();
    let bound = decode_bound_did(&eth_call(rpc, &bound_did_call(&address))?).map_err(bind_error)?;
    let nonce =
        decode_bind_nonce(&eth_call(rpc, &bind_nonce_call(&address))?).map_err(bind_error)?;
    let call = match plan_bind(account, chain_id, bound, nonce).map_err(bind_error)? {
        BindPlan::AlreadyBound => return Ok(json!({"status":"already_bound","chain_id":chain_id})),
        BindPlan::Bind(call) => call,
    };
    let from = account.evm_address_text();
    let data = format!("0x{}", hex_encode(call.data()));
    let to = format!("0x{}", hex_encode(&call.to()));
    let evm_nonce = u64::try_from(quantity(
        &rpc.call("eth_getTransactionCount", &json!([from, "pending"]))?,
        "transaction count",
    )?)
    .map_err(|_| "transaction count exceeds 64 bits".to_owned())?;
    let gas_price = quantity(&rpc.call("eth_gasPrice", &json!([]))?, "gas price")?;
    let gas_limit = u64::try_from(quantity(
        &rpc.call(
            "eth_estimateGas",
            &json!([{"from": from, "to": to, "data": data}]),
        )?,
        "gas estimate",
    )?)
    .map_err(|_| "gas estimate exceeds 64 bits".to_owned())?;
    let fees = BindFees {
        evm_nonce,
        max_priority_fee_per_gas: gas_price,
        max_fee_per_gas: gas_price
            .checked_mul(2)
            .ok_or("gas price is out of range")?,
        gas_limit,
    };
    let raw = sign_bind_transaction(account, chain_id, &call, fees).map_err(bind_error)?;
    let hash = rpc.call(
        "eth_sendRawTransaction",
        &json!([format!("0x{}", hex_encode(&raw))]),
    )?;
    Ok(json!({"status":"submitted","chain_id":chain_id,"bind_nonce":nonce,"transaction_hash":hash}))
}

fn path_text(path: &[u32]) -> String {
    let mut text = String::from("m");
    for component in path {
        text.push('/');
        text.push_str(&(component & 0x7fff_ffff).to_string());
        if component & 0x8000_0000 != 0 {
            text.push('\'');
        }
    }
    text
}

pub fn run(
    arguments: &DeriveArgs,
    rpc: Option<&str>,
    gateway: Option<&str>,
) -> Result<CommandOutput, String> {
    if arguments.bind && rpc.is_none() {
        return Err(
            "bind_requires_rpc: --bind needs --rpc pointing at the Paxeer X Network endpoint"
                .into(),
        );
    }
    let mnemonic = match &arguments.mnemonic_file {
        Some(path) => read_file(path, "mnemonic")?,
        None => read_bounded(std::io::stdin(), "mnemonic on standard input")?,
    };
    let passphrase = match &arguments.passphrase_file {
        Some(path) => {
            let text = read_file(path, "passphrase")?;
            Zeroizing::new(text.trim_end_matches(['\r', '\n']).to_owned())
        }
        None => Zeroizing::new(String::new()),
    };
    let account = derive_from_mnemonic(&mnemonic, &passphrase, arguments.index)
        .map_err(|error| format!("{error}: no account was derived"))?;
    drop(mnemonic);
    drop(passphrase);
    let layerx: Vec<u32> = layerx_path(arguments.index)
        .iter()
        .map(|component| component | 0x8000_0000)
        .collect();
    let mut data = json!({
        "index": arguments.index,
        "evm_address": account.evm_address_text(),
        "evm_path": path_text(&evm_path(arguments.index)),
        "did": account.did(),
        "layerx_public_key": hex_encode(&account.layerx_public_key()),
        "layerx_path": path_text(&layerx),
    });
    if let Some(path) = &arguments.export_private_keys {
        export(path, &account)?;
        data["private_keys_file"] = json!(path.display().to_string());
    }
    if arguments.bind {
        let stored = gateway
            .map(|alias| {
                crate::credential::gateway(alias)?
                    .ok_or_else(|| format!("gateway credential {alias} does not exist"))
            })
            .transpose()?;
        let client = RpcClient::new(rpc.ok_or("bind_requires_rpc: --rpc is missing")?, stored)?;
        data["binding"] = bind(&client, &account)?;
    }
    Ok(CommandOutput::new(
        "wallet.derived",
        "Derived one account for both sides of the Paxeer X Network",
        data,
    ))
}

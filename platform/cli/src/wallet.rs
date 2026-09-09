use clap::{Args, Subcommand, ValueEnum};
use layerx_crypto::payments::{asset_id, Payment, Registration};
use layerx_platform_cli::rpc::RpcClient;
use layerx_types::account::AccountId;
use layerx_wire::hash::account_id_for_protocol;
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};

use crate::config::{Configuration, KeyMetadata};
use crate::encoding::{fixed_hex, hex_decode, hex_encode};
use crate::http::Client;
use crate::output::CommandOutput;

#[derive(Subcommand)]
pub enum WalletCommand {
    /// Generate a key, register its identity, and open its main account.
    Create {
        name: String,
        #[arg(long)]
        did: Option<String>,
    },
    /// Import a hexadecimal Ed25519 seed from standard input.
    Import {
        name: String,
        #[arg(long)]
        did: Option<String>,
    },
    /// List local wallet public metadata.
    List,
    /// Read all accounts of the selected DID.
    Balance {
        #[arg(long)]
        did: Option<String>,
        #[arg(long)]
        asset: Option<String>,
    },
    /// Submit a transfer with independent identity and source sequences.
    Send(TransferArgs),
    /// Read the selected DID's activity history.
    History {
        #[arg(long)]
        did: Option<String>,
    },
    /// Verify an executed receipt using the configured sequencer key.
    Receipt {
        activity_id: String,
        #[arg(long, value_enum, default_value = "executed")]
        wait: Commitment,
        #[arg(long)]
        receipt_policy: Option<std::path::PathBuf>,
    },
    /// Open the selected wallet's account for an asset.
    OpenAccount {
        #[arg(long)]
        asset: String,
        #[arg(long)]
        key: Option<String>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Commitment {
    Executed,
    Batched,
    Finalised,
}

#[derive(Args)]
pub struct TransferArgs {
    #[arg(long)]
    to: String,
    #[arg(long)]
    asset: String,
    #[arg(long)]
    amount: String,
    #[arg(long)]
    key: Option<String>,
    #[arg(long, value_enum, default_value = "executed")]
    wait: Commitment,
}

#[derive(Subcommand)]
pub enum TokenCommand {
    /// Register a natively issued token.
    Create {
        #[arg(long)]
        symbol: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        decimals: u8,
        #[arg(long, default_value = "0")]
        supply_cap: String,
        #[arg(long)]
        salt: String,
        #[arg(long)]
        key: Option<String>,
    },
    /// Mint units to an existing asset account.
    Mint {
        #[arg(long)]
        asset: String,
        #[arg(long)]
        to: String,
        #[arg(long)]
        amount: String,
        #[arg(long)]
        key: Option<String>,
    },
    /// Burn units from the selected wallet's asset account.
    Burn {
        #[arg(long)]
        asset: String,
        #[arg(long)]
        amount: String,
        #[arg(long)]
        key: Option<String>,
    },
    /// Transfer token units.
    Transfer(TransferArgs),
    /// Read token metadata.
    Info { asset: String },
    /// List registered tokens.
    List,
}

pub struct Transport {
    rest: Client,
    rpc: Option<RpcClient>,
    emulator: bool,
}

impl Transport {
    pub fn new(
        config: &Configuration,
        rpc: Option<&str>,
        gateway: Option<&str>,
    ) -> Result<Self, String> {
        let (name, environment) = config.active_environment()?;
        let stored = gateway
            .map(|alias| {
                crate::credential::gateway(alias)?
                    .ok_or_else(|| format!("gateway credential {alias} does not exist"))
            })
            .transpose()?;
        let rest = if let Some(value) = &stored {
            Client::new_gateway(&environment.endpoint, value.clone())?
        } else {
            Client::new(&environment.endpoint, crate::credential::token(name)?)?
        };
        let rpc = rpc.map(|url| RpcClient::new(url, stored)).transpose()?;
        let emulator = name == "emulator" && rpc.is_none();
        Ok(Self {
            rest,
            rpc,
            emulator,
        })
    }

    fn read(&self, method: &str, params: &Value, path: &str) -> Result<Value, String> {
        match &self.rpc {
            Some(rpc) => rpc.call(method, params),
            None => self
                .rest
                .get(path)?
                .get("result")
                .cloned()
                .ok_or_else(|| format!("{path} omitted result")),
        }
    }

    fn balances(&self, did: &str) -> Result<Value, String> {
        crate::http::validate_resource_id(did, "DID")?;
        if self.emulator {
            let state = self.rest.get("/v1/state")?;
            let accounts = state
                .pointer("/result/accounts")
                .and_then(Value::as_array)
                .ok_or("emulator state omitted accounts")?;
            let prefix = format!("agent:{did}:");
            let accounts: Vec<_> = accounts
                .iter()
                .filter(|a| {
                    a["name"]
                        .as_str()
                        .and_then(|n| n.strip_prefix(&prefix))
                        .is_some_and(|tail| {
                            tail == "main"
                                || ["asset:", "budget:", "escrow:", "margin:"].iter().any(
                                    |marker| {
                                        tail.strip_prefix(marker).is_some_and(|component| {
                                            !component.is_empty() && !component.contains(':')
                                        })
                                    },
                                )
                        })
                })
                .cloned()
                .collect();
            return Ok(json!({"did":did,"accounts":accounts}));
        }
        self.read(
            "lx_getBalances",
            &json!([did]),
            &format!("/v1/dids/{did}/accounts"),
        )
    }

    fn source_sequence(&self, account: &str) -> Result<u64, String> {
        if self.emulator {
            let state = self.rest.get("/v1/state")?;
            let value = state
                .pointer("/result/accounts")
                .and_then(Value::as_array)
                .and_then(|accounts| accounts.iter().find(|a| a["id"] == account))
                .ok_or("source account not found")?;
            return sequence(&value["next_sequence"]);
        }
        let value = self.read(
            "lx_getSequence",
            &json!([account]),
            &format!("/v1/accounts/{account}/balance"),
        )?;
        sequence(&value["next_sequence"])
    }
}

fn create_wallet(
    config: &mut Configuration,
    name: &str,
    did: Option<&str>,
    rpc: Option<&str>,
    gateway: Option<&str>,
) -> Result<CommandOutput, String> {
    let transport = Transport::new(config, rpc, gateway)?;
    if !transport.emulator {
        return Err("wallet_registration_unavailable: the gateway contract does not expose DID/public-key registration and main-account creation; no key was generated".into());
    }
    let metadata = if let Some(existing) = config.keys.get(name) {
        if did.is_some_and(|d| d != existing.did) {
            return Err("existing wallet DID differs from --did".into());
        }
        existing.clone()
    } else {
        crate::credential::create_key(config, name, did.map(str::to_owned))?
    };
    let registered = crate::account::create(
        &transport.rest,
        "emulator",
        Some(&metadata),
        "0",
        None,
        None,
        None,
    )
    .map_err(|e| format!("wallet registration failed; key {name} is retained for retry: {e}"))?;
    Ok(CommandOutput::new(
        "wallet.created",
        "Created wallet identity and main account",
        json!({"name":name,"did":metadata.did,"public_key":metadata.public_key,"registration":registered}),
    ))
}

pub fn run_wallet(
    command: WalletCommand,
    rpc: Option<&str>,
    gateway: Option<&str>,
) -> Result<CommandOutput, String> {
    let mut config = Configuration::load()?;
    match command {
        WalletCommand::List => crate::key(crate::KeyCommand::List),
        WalletCommand::Import { name, did } => {
            let metadata = crate::credential::import_key(&mut config, &name, did)?;
            Ok(CommandOutput::new(
                "wallet.imported",
                "Imported wallet key; registration is unchanged",
                json!({"name":name,"did":metadata.did,"public_key":metadata.public_key}),
            ))
        }
        WalletCommand::Create { name, did } => {
            create_wallet(&mut config, &name, did.as_deref(), rpc, gateway)
        }
        WalletCommand::Balance { did, asset } => {
            let did = selected_did(&config, did.as_deref())?;
            let transport = Transport::new(&config, rpc, gateway)?;
            let result = if let Some(asset) = asset {
                let asset = fixed_hex::<32>("asset", &asset)?;
                let account = account(&did, &asset)?;
                if transport.emulator {
                    let mut balances = transport.balances(&did)?;
                    let accounts = balances["accounts"]
                        .as_array_mut()
                        .ok_or("accounts missing")?;
                    accounts.retain(|a| a["id"] == account);
                    balances
                } else {
                    transport.read(
                        "lx_getBalance",
                        &json!([account]),
                        &format!("/v1/accounts/{account}/balance"),
                    )?
                }
            } else {
                transport.balances(&did)?
            };
            Ok(CommandOutput::new(
                "wallet.balance",
                "Read wallet balances",
                result,
            ))
        }
        WalletCommand::Receipt {
            activity_id,
            wait,
            receipt_policy,
        } => wallet_receipt(
            &config,
            rpc,
            gateway,
            &activity_id,
            wait,
            receipt_policy.as_deref(),
        ),
        WalletCommand::History { did } => {
            let did = selected_did(&config, did.as_deref())?;
            Err(format!("wallet_history_unavailable: no DID activity-history method or REST route is published for {did}"))
        }
        WalletCommand::Send(args) => {
            transfer(&config, &Transport::new(&config, rpc, gateway)?, &args)
        }
        WalletCommand::OpenAccount { asset, key } => {
            let metadata = metadata(&config, key.as_deref())?;
            payment_preflight(
                &Payment::OpenAccount {
                    asset: fixed_hex("asset", &asset)?,
                },
                metadata,
            )
        }
    }
}

fn wallet_receipt(
    config: &Configuration,
    rpc: Option<&str>,
    gateway: Option<&str>,
    activity_id: &str,
    wait: Commitment,
    receipt_policy: Option<&std::path::Path>,
) -> Result<CommandOutput, String> {
    if rpc.is_some() {
        let client = sdk_rpc(config, rpc, gateway)?;
        let policy = read_policy(receipt_policy, config)?;
        return verified_output(
            &client
                .wait_for(
                    fixed_hex("activity id", activity_id)?,
                    sdk_commitment(wait),
                    &policy,
                    std::time::Duration::from_secs(60),
                )
                .map_err(rpc_error)?,
        );
    }
    if !matches!(wait, Commitment::Executed) {
        return Err("verified commitment waits require --rpc and --receipt-policy".into());
    }
    let activity = fixed_hex::<32>("activity id", activity_id)?;
    let transport = Transport::new(config, rpc, gateway)?;
    let response = transport.read(
        "lx_getReceipt",
        &json!([hex_encode(&activity)]),
        &format!("/v1/receipts/{}", hex_encode(&activity)),
    )?;
    let key = config
        .active_environment()?
        .1
        .sequencer_trust_anchor
        .as_deref()
        .ok_or("configure a sequencer trust anchor before verifying receipts")?;
    let receipt = verify_receipt(&response, activity, fixed_hex("sequencer public key", key)?)?;
    let code = receipt["result_code"]
        .as_i64()
        .ok_or("receipt result missing")?;
    if code != 0 {
        return Err(format!(
            "activity {}: receipt result {code}; commitment reached executed",
            hex_encode(&activity)
        ));
    }
    Ok(CommandOutput::new(
        "wallet.receipt",
        format!(
            "Activity {}: receipt result {code}; commitment reached executed",
            hex_encode(&activity)
        ),
        receipt,
    ))
}

pub fn run_token(
    command: TokenCommand,
    rpc: Option<&str>,
    gateway: Option<&str>,
) -> Result<CommandOutput, String> {
    let config = Configuration::load()?;
    match command {
        TokenCommand::Info { asset } => {
            let asset = hex_encode(&fixed_hex::<32>("asset", &asset)?);
            unavailable_asset_read(&config, rpc, gateway, "lx_getAsset", &json!([asset]))
        }
        TokenCommand::List => {
            unavailable_asset_read(&config, rpc, gateway, "lx_listAssets", &json!([]))
        }
        TokenCommand::Transfer(args) => {
            transfer(&config, &Transport::new(&config, rpc, gateway)?, &args)
        }
        TokenCommand::Create {
            symbol,
            name,
            decimals,
            supply_cap,
            salt,
            key,
        } => {
            let owner = metadata(&config, key.as_deref())?;
            let salt = fixed_hex("salt", &salt)?;
            let issuer = issuer_id(&owner.did)?;
            let payment = Payment::Register(Registration {
                asset: asset_id(&issuer, &salt),
                salt,
                symbol,
                name,
                decimals,
                supply_cap: units(&supply_cap, false)?,
                issuer_kind: 1,
                custody_ref: Vec::new(),
            });
            payment_preflight(&payment, owner)
        }
        TokenCommand::Mint {
            asset,
            to,
            amount,
            key,
        } => {
            let owner = metadata(&config, key.as_deref())?;
            let asset = fixed_hex("asset", &asset)?;
            let to = destination(&to, &asset)?;
            payment_preflight(
                &Payment::Mint {
                    asset,
                    to,
                    amount: units(&amount, true)?,
                },
                owner,
            )
        }
        TokenCommand::Burn { asset, amount, key } => {
            let owner = metadata(&config, key.as_deref())?;
            let asset = fixed_hex("asset", &asset)?;
            let from = fixed_hex("source account", &account(&owner.did, &asset)?)?;
            payment_preflight(
                &Payment::Burn {
                    asset,
                    from,
                    amount: units(&amount, true)?,
                },
                owner,
            )
        }
    }
}

fn unavailable_asset_read(
    config: &Configuration,
    rpc: Option<&str>,
    gateway: Option<&str>,
    method: &str,
    params: &Value,
) -> Result<CommandOutput, String> {
    if let Some(rpc) = Transport::new(config, rpc, gateway)?.rpc {
        let value = rpc.call(method, params)?;
        return Ok(CommandOutput::new("token.read", "Read token data", value));
    }
    Err(format!("rpc_method_unavailable: {method} and its REST equivalent are absent from the published gateway contract"))
}

fn payment_preflight(payment: &Payment, owner: &KeyMetadata) -> Result<CommandOutput, String> {
    let bytes = payment
        .encode(owner.did.as_bytes())
        .map_err(|e| format!("invalid payment: {e}"))?;
    let (module, ordinal) = payment.activity_type();
    Payment::decode(module, ordinal, &bytes, owner.did.as_bytes())
        .map_err(|e| format!("invalid canonical payment: {e}"))?;
    Err(format!("identity_sequence_unavailable: the gateway contract has no identity.next_sequence read for {}; validated asset ordinal {ordinal}, but no activity was signed or submitted", owner.did))
}

fn transfer(
    config: &Configuration,
    transport: &Transport,
    args: &TransferArgs,
) -> Result<CommandOutput, String> {
    let owner = metadata(config, args.key.as_deref())?;
    let asset = fixed_hex("asset", &args.asset)?;
    let _amount = units(&args.amount, true)?;
    let destination = destination(&args.to, &asset)?;
    let source = account(&owner.did, &asset)?;
    if hex_encode(&destination) == source {
        return Err("source and destination accounts must differ".into());
    }
    let source_sequence = transport.source_sequence(&source)?;
    let commitment = match args.wait {
        Commitment::Executed => "executed",
        Commitment::Batched => "batched",
        Commitment::Finalised => "finalised",
    };
    Err(format!("identity_sequence_unavailable: source account next_sequence={source_sequence}; envelope sequence unavailable because the gateway contract has no identity.next_sequence read. Requested commitment {commitment}; no activity signed or submitted"))
}

fn sdk_commitment(value: Commitment) -> layerx_sdk::rpc::Commitment {
    match value {
        Commitment::Executed => layerx_sdk::rpc::Commitment::Executed,
        Commitment::Batched => layerx_sdk::rpc::Commitment::Batched,
        Commitment::Finalised => layerx_sdk::rpc::Commitment::Finalised,
    }
}

fn rpc_error(error: layerx_sdk::rpc::RpcError) -> String {
    match error {
        layerx_sdk::rpc::RpcError::Remote {
            code,
            message,
            data,
        } => json!({"code":code,"message":message,"data":data}).to_string(),
        layerx_sdk::rpc::RpcError::Pending { activity_id } => format!(
            "activity {}: receipt result unavailable; requested commitment not reached",
            hex_encode(&activity_id)
        ),
        other => format!("wallet RPC failed: {other:?}"),
    }
}

fn sdk_rpc(
    config: &Configuration,
    rpc: Option<&str>,
    gateway: Option<&str>,
) -> Result<layerx_sdk::rpc::RpcClient, String> {
    let endpoint = rpc.unwrap_or(&config.active_environment()?.1.endpoint);
    let credential = gateway
        .map(|alias| {
            let stored =
                crate::credential::gateway(alias)?.ok_or("gateway credential does not exist")?;
            let (id, secret) = stored
                .split_once(':')
                .ok_or("gateway credential is malformed")?;
            let secret = layerx_sdk::production::SecretBytes::new(secret.as_bytes())
                .map_err(|_| "gateway credential is malformed")?;
            layerx_sdk::programs::LayerXKeyCredential::new(id, secret)
                .map_err(|_| "gateway credential is malformed".to_owned())
        })
        .transpose()?;
    layerx_sdk::rpc::RpcClient::connect(endpoint, credential).map_err(rpc_error)
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct WalletPolicy {
    protocol_version: u16,
    network_id: u32,
    sequencer_id: String,
    sequencer_key: String,
    first_batch: u64,
    last_batch: u64,
    checkpoint_context_digest: Option<String>,
}

fn read_policy(
    path: Option<&std::path::Path>,
    config: &Configuration,
) -> Result<layerx_sdk::rpc_verification::ReceiptPolicy, String> {
    let path = path.ok_or(
        "provide --receipt-policy with independently trusted sequencer and checkpoint authority",
    )?;
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read receipt policy: {e}"))?;
    let policy: WalletPolicy =
        serde_json::from_slice(&bytes).map_err(|e| format!("invalid receipt policy: {e}"))?;
    if policy.protocol_version != 3
        || policy.network_id != config.active_environment()?.1.network_id
        || policy.first_batch == 0
        || policy.last_batch < policy.first_batch
    {
        return Err("receipt policy scope or batch range is invalid".into());
    }
    Ok(layerx_sdk::rpc_verification::ReceiptPolicy {
        protocol_version: policy.protocol_version,
        network_id: policy.network_id,
        sequencer: layerx_proof::inclusion::SequencerAuthorization::new(
            fixed_hex("sequencer id", &policy.sequencer_id)?,
            fixed_hex("sequencer key", &policy.sequencer_key)?,
            policy.first_batch,
            policy.last_batch,
        ),
        trusted_checkpoint_context_digest: policy
            .checkpoint_context_digest
            .as_deref()
            .map(|s| fixed_hex("checkpoint context digest", s))
            .transpose()?,
    })
}

fn verified_output(
    receipt: &layerx_sdk::rpc_verification::VerifiedRpcReceipt,
) -> Result<CommandOutput, String> {
    let facts = receipt
        .receipt()
        .protocol()
        .ok_or("verified receipt omitted protocol facts")?;
    let id = hex_encode(&facts.activity_id());
    let code = facts.result_code();
    let commitment = receipt.commitment().as_str();
    let message = format!("Activity {id}: receipt result {code}; commitment reached {commitment}");
    if code != 0 {
        return Err(message);
    }
    Ok(CommandOutput::new(
        "wallet.receipt",
        message,
        json!({"activity_id":id,"result_code":code,"commitment":commitment,"receipt":hex_encode(receipt.canonical_bytes())}),
    ))
}

fn metadata<'a>(config: &'a Configuration, key: Option<&str>) -> Result<&'a KeyMetadata, String> {
    let name = key
        .or(config.default_key.as_deref())
        .ok_or("select a wallet key")?;
    config
        .keys
        .get(name)
        .ok_or_else(|| format!("wallet key {name} does not exist"))
}

fn selected_did(config: &Configuration, did: Option<&str>) -> Result<String, String> {
    match did {
        Some(did) => Ok(did.to_owned()),
        None => Ok(metadata(config, None)?.did.clone()),
    }
}

fn units(text: &str, positive: bool) -> Result<u128, String> {
    let amount = text
        .parse::<u128>()
        .map_err(|_| "amount must be an unsigned integer in base units")?;
    if positive && amount == 0 {
        return Err("amount must be greater than zero".into());
    }
    Ok(amount)
}

fn native_asset() -> [u8; 32] {
    let mut asset = [0; 32];
    asset[0] = 1;
    asset
}

fn account(did: &str, asset: &[u8; 32]) -> Result<String, String> {
    let name = if *asset == native_asset() {
        format!("agent:{did}:main")
    } else {
        format!("agent:{did}:asset:{}", hex_encode(asset))
    };
    let parsed = AccountId::parse(&name).map_err(|e| format!("invalid account: {e:?}"))?;
    account_id_for_protocol(&parsed, 3)
        .map(|id| hex_encode(&id))
        .map_err(|e| format!("invalid account: {e:?}"))
}

fn destination(to: &str, asset: &[u8; 32]) -> Result<[u8; 32], String> {
    if to.starts_with("did:") {
        fixed_hex("destination", &account(to, asset)?)
    } else {
        fixed_hex("destination account", to)
    }
}

fn issuer_id(did: &str) -> Result<[u8; 32], String> {
    layerx_types::ids::Did::new(did.as_bytes()).map_err(|e| format!("invalid DID: {e:?}"))?;
    let length = u16::try_from(did.len()).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    hash.update(b"LXP/v1/did-id\0");
    hash.update(length.to_be_bytes());
    hash.update(did.as_bytes());
    Ok(hash.finalize().into())
}

fn sequence(value: &Value) -> Result<u64, String> {
    if let Some(value) = value.as_u64() {
        return Ok(value);
    }
    if let Some(text) = value.as_str() {
        if let Ok(value) = text.parse::<u64>() {
            if value.to_string() == text {
                return Ok(value);
            }
        }
    }
    Err("account snapshot omitted canonical next_sequence".into())
}

fn verify_receipt(response: &Value, activity: [u8; 32], key: [u8; 32]) -> Result<Value, String> {
    let encoded = response["receipt"]
        .as_str()
        .ok_or("receipt_unavailable: admission does not establish execution")?;
    let bytes = hex_decode("receipt", encoded)?;
    let receipt = layerx_proof::receipt::verify_sequencer_signature(&bytes, key)
        .map_err(|e| format!("receipt verification failed at {:?}", e.check))?;
    let facts = receipt.protocol().ok_or("receipt lacks protocol facts")?;
    if facts.activity_id() != activity {
        return Err("receipt activity does not match the requested activity".into());
    }
    Ok(
        json!({"activity_id":hex_encode(&activity),"result_code":facts.result_code(),"commitment":"executed","receipt":encoded}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespaces_and_sequence_bounds() -> Result<(), String> {
        let did = "did:layerx:alice";
        assert_ne!(account(did, &native_asset())?, account(did, &[1; 32])?);
        assert_eq!(
            hex_encode(&destination(did, &[1; 32])?),
            account(did, &[1; 32])?
        );
        assert_eq!(sequence(&json!(u64::MAX.to_string()))?, u64::MAX);
        for bad in [
            json!(-1),
            json!("01"),
            json!("18446744073709551616"),
            json!(null),
            json!(1.1),
        ] {
            assert!(sequence(&bad).is_err());
        }
        assert!(units("0", true).is_err());
        assert!(units("1.5", true).is_err());
        assert_eq!(units("0", false)?, 0);
        Ok(())
    }

    #[test]
    fn acknowledgements_never_become_receipts() {
        for response in [
            json!({"state":"accepted"}),
            json!({"state":"executed","receipt":""}),
            json!({"receipt":"00"}),
        ] {
            assert!(verify_receipt(&response, [1; 32], [2; 32]).is_err());
        }
    }
}

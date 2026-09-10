use clap::{Args, Subcommand, ValueEnum};
use layerx_crypto::payments::{asset_id, Payment, Registration};
use layerx_platform_cli::rpc::RpcClient;
use layerx_wire::hash::account_id_for_protocol;
use serde_json::{json, Value};

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
    /// Unavailable: no authenticated DID activity-history backend is published.
    #[command(hide = true)]
    History {
        #[arg(long)]
        did: Option<String>,
    },
    /// Read one live notification; use receipt verification to establish commitment.
    Watch {
        #[arg(value_enum)]
        topic: SubscriptionTopic,
        #[arg(long)]
        account_id: Option<String>,
        #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u64).range(1..=300))]
        timeout_seconds: u64,
    },
    /// Estimate fees from canonical activity bytes using the native fee schedule.
    EstimateFee { canonical_hex: String },
    /// Verify receipt evidence at the requested commitment.
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
        #[command(flatten)]
        write: WriteOptions,
    },
}

#[derive(Clone, Copy, ValueEnum)]
pub enum SubscriptionTopic {
    Receipts,
    Checkpoints,
    Account,
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
    #[command(flatten)]
    write: TransferWriteOptions,
}

#[derive(Args)]
pub struct TransferWriteOptions {
    #[arg(long)]
    receipt_policy: Option<std::path::PathBuf>,
    #[arg(long)]
    fee_limit: Option<String>,
    #[arg(long, value_enum, default_value = "executed")]
    wait: Commitment,
    #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u64).range(1..=300))]
    timeout_seconds: u64,
}

#[derive(Args)]
pub struct WriteOptions {
    #[arg(long)]
    receipt_policy: std::path::PathBuf,
    #[arg(long)]
    fee_limit: String,
    #[arg(long, value_enum, default_value = "executed")]
    wait: Commitment,
    #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u64).range(1..=300))]
    timeout_seconds: u64,
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
        #[command(flatten)]
        write: WriteOptions,
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
        #[command(flatten)]
        write: WriteOptions,
    },
    /// Burn units from the selected wallet's asset account.
    Burn {
        #[arg(long)]
        asset: String,
        #[arg(long)]
        amount: String,
        #[arg(long)]
        key: Option<String>,
        #[command(flatten)]
        write: WriteOptions,
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

fn estimate_fee(
    config: &Configuration,
    rpc: Option<&str>,
    gateway: Option<&str>,
    canonical: &str,
) -> Result<CommandOutput, String> {
    let canonical = hex_decode("canonical activity", canonical)?;
    let result = sdk_rpc(config, rpc, gateway)?
        .estimate_fee(&canonical)
        .map_err(rpc_error)?
        .into_value();
    Ok(CommandOutput::new(
        "wallet.fee",
        "Read native fee estimate",
        result,
    ))
}

fn watch(
    config: &Configuration,
    rpc: Option<&str>,
    gateway: Option<&str>,
    topic: SubscriptionTopic,
    account_id: Option<String>,
    timeout_seconds: u64,
) -> Result<CommandOutput, String> {
    let topic = match topic {
        SubscriptionTopic::Receipts => "receipts",
        SubscriptionTopic::Checkpoints => "checkpoints",
        SubscriptionTopic::Account => "account",
    };
    let params = match account_id {
        Some(account) => json!([topic, account]),
        None => json!([topic]),
    };
    let transport = Transport::new(config, rpc, gateway)?;
    let client = transport
        .rpc
        .ok_or("rpc_transport_required: subscriptions require --rpc")?;
    let result = client.subscribe(&params, std::time::Duration::from_secs(timeout_seconds))?;
    Ok(CommandOutput::new(
        "wallet.notification",
        "Received unverified live notification; verify receipts separately",
        result,
    ))
}

pub fn run_wallet(
    command: WalletCommand,
    rpc: Option<&str>,
    gateway: Option<&str>,
) -> Result<CommandOutput, String> {
    let mut config = Configuration::load()?;
    match command {
        WalletCommand::EstimateFee { canonical_hex } => {
            estimate_fee(&config, rpc, gateway, &canonical_hex)
        }
        WalletCommand::Watch {
            topic,
            account_id,
            timeout_seconds,
        } => watch(&config, rpc, gateway, topic, account_id, timeout_seconds),
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
            let result = if rpc.is_some() {
                let balances = sdk_rpc(&config, rpc, gateway)?
                    .get_balances(&did)
                    .map_err(rpc_error)?
                    .into_value();
                if let Some(asset) = asset {
                    filter_rpc_balances(balances, &did, fixed_hex::<32>("asset", &asset)?)?
                } else {
                    validated_account_records(&balances, &did)?;
                    balances
                }
            } else if let Some(asset) = asset {
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
        WalletCommand::Send(args) => transfer(&config, rpc, gateway, &args),
        WalletCommand::OpenAccount { asset, key, write } => execute_payment(
            &config,
            rpc,
            gateway,
            key.as_deref(),
            &write,
            &Payment::OpenAccount {
                asset: fixed_hex("asset", &asset)?,
            },
        ),
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
        TokenCommand::Transfer(args) => transfer(&config, rpc, gateway, &args),
        TokenCommand::Create {
            symbol,
            name,
            decimals,
            supply_cap,
            salt,
            key,
            write,
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
            execute_payment(&config, rpc, gateway, key.as_deref(), &write, &payment)
        }
        TokenCommand::Mint {
            asset,
            to,
            amount,
            key,
            write,
        } => {
            let asset = fixed_hex("asset", &asset)?;
            execute_write(
                &config,
                rpc,
                gateway,
                key.as_deref(),
                &write,
                WriteRequest::Mint {
                    asset,
                    to,
                    amount: units(&amount, true)?,
                },
            )
        }
        TokenCommand::Burn {
            asset,
            amount,
            key,
            write,
        } => {
            let asset = fixed_hex("asset", &asset)?;
            execute_write(
                &config,
                rpc,
                gateway,
                key.as_deref(),
                &write,
                WriteRequest::Burn {
                    asset,
                    amount: units(&amount, true)?,
                },
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
    let client = sdk_rpc(config, rpc, gateway)?;
    let value = match method {
        "lx_getAsset" => client
            .get_asset(fixed_hex(
                "asset",
                params[0].as_str().ok_or("asset missing")?,
            )?)
            .map(layerx_sdk::rpc::AssetSnapshot::into_value),
        "lx_listAssets" => client
            .list_assets()
            .map(layerx_sdk::rpc::AssetListSnapshot::into_value),
        _ => return Err("rpc_method_unavailable: asset method not published".into()),
    }
    .map_err(|error| match error {
        layerx_sdk::rpc::RpcError::Remote { .. } => rpc_error(error),
        other => format!("rpc_method_unavailable: {}", rpc_error(other)),
    })?;
    Ok(CommandOutput::new("token.read", "Read token data", value))
}

fn execute_payment(
    config: &Configuration,
    rpc: Option<&str>,
    gateway: Option<&str>,
    key: Option<&str>,
    write: &WriteOptions,
    payment: &Payment,
) -> Result<CommandOutput, String> {
    execute_write(
        config,
        rpc,
        gateway,
        key,
        write,
        WriteRequest::Payment(payment),
    )
}

enum WriteRequest<'a> {
    Payment(&'a Payment),
    Send {
        asset: [u8; 32],
        to: String,
        amount: u128,
    },
    Mint {
        asset: [u8; 32],
        to: String,
        amount: u128,
    },
    Burn {
        asset: [u8; 32],
        amount: u128,
    },
}

fn execute_write(
    config: &Configuration,
    rpc: Option<&str>,
    gateway: Option<&str>,
    key: Option<&str>,
    write: &WriteOptions,
    request: WriteRequest<'_>,
) -> Result<CommandOutput, String> {
    use layerx_crypto::signer::{LocalSigner, Signer as _};
    use layerx_platform_cli::wallet_signing::{PreparedPayment, SigningFacts};
    let owner = metadata(config, key)?;
    let policy = read_policy(Some(&write.receipt_policy), config)?;
    if matches!(write.wait, Commitment::Finalised)
        && policy.trusted_checkpoint_context_digest.is_none()
    {
        return Err(rpc_error(layerx_sdk::rpc::RpcError::MissingFinalityTrust));
    }
    let client = sdk_rpc(config, rpc, gateway)?;
    let transport = Transport::new(config, rpc, gateway)?;
    let identity = transport
        .rpc
        .as_ref()
        .ok_or("rpc_transport_required: writes require --rpc")?
        .call("lx_getSequence", &json!([owner.did, "identity"]))?;
    let identity_sequence = identity_sequence(&identity, &owner.did)?;
    let seed = crate::credential::key_seed(
        key.or(config.default_key.as_deref())
            .ok_or("select a wallet key")?,
    )?;
    let signer = LocalSigner::new(*seed);
    drop(seed);
    if signer.public_key() != fixed_hex::<32>("wallet public key", &owner.public_key)? {
        return Err("wallet signer does not match the selected public key".into());
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?;
    let not_before = u64::try_from(now.as_millis()).map_err(|e| e.to_string())?;
    let mut idempotency_key = [0; 32];
    getrandom::fill(&mut idempotency_key).map_err(|_| "wallet randomness unavailable")?;
    let facts = SigningFacts {
        actor: &owner.did,
        public_key: signer.public_key(),
        network_id: policy.network_id,
        identity_next_sequence: identity_sequence,
        not_before_ms: not_before,
        expires_at_ms: not_before
            .checked_add(300_000)
            .ok_or("wallet validity overflow")?,
        fee_limit: units(&write.fee_limit, false)?,
        idempotency_key,
    };
    let prepared = match request {
        WriteRequest::Payment(payment) => PreparedPayment::new(payment, &facts)?,
        WriteRequest::Send { asset, to, amount } => {
            let to = destination(&client, &to, asset)?;
            prepare_send(&client, &signer, &facts, asset, to, amount)?
        }
        WriteRequest::Mint { asset, to, amount } => {
            let to = destination(&client, &to, asset)?;
            PreparedPayment::new(&Payment::Mint { asset, to, amount }, &facts)?
        }
        WriteRequest::Burn { asset, amount } => {
            let from = account_for_asset(&client, facts.actor, asset)?;
            PreparedPayment::new(
                &Payment::Burn {
                    asset,
                    from,
                    amount,
                },
                &facts,
            )?
        }
    };
    print_disclosure(&prepared.confirmation())?;
    let (canonical, activity) = prepared.sign_with_id(&signer)?;
    print_disclosure(
        &json!({"activity_id":hex_encode(&activity),"receipt_result":null,"commitment_reached":null}),
    )?;
    match client.send_activity(&canonical, sdk_commitment(write.wait)) {
        Ok(_)
        | Err(layerx_sdk::rpc::RpcError::Transport | layerx_sdk::rpc::RpcError::InvalidResponse) => {
        }
        Err(error) => return Err(rpc_error(error)),
    }
    verified_output(
        &client
            .wait_for(
                activity,
                sdk_commitment(write.wait),
                &policy,
                std::time::Duration::from_secs(write.timeout_seconds),
            )
            .map_err(rpc_error)?,
    )
}

fn prepare_send(
    client: &layerx_sdk::rpc::RpcClient,
    signer: &dyn layerx_crypto::signer::Signer,
    facts: &layerx_platform_cli::wallet_signing::SigningFacts<'_>,
    asset: [u8; 32],
    to: [u8; 32],
    amount: u128,
) -> Result<layerx_platform_cli::wallet_signing::PreparedPayment, String> {
    let from = account_for_asset(client, facts.actor, asset)?;
    if from == to {
        return Err("source and destination accounts must differ".into());
    }
    let snapshot = client.get_account(&hex_encode(&from)).map_err(rpc_error)?;
    if snapshot["account_id"] != hex_encode(&from) {
        return Err("source account snapshot mismatch".into());
    }
    if snapshot["asset_id"] != hex_encode(&asset) {
        return Err("source account asset mismatch".into());
    }
    if !snapshot["next_sequence"].is_string() {
        return Err("source snapshot omitted canonical sequence".into());
    }
    let source_sequence = sequence(&snapshot["next_sequence"])?;
    let debit = layerx_crypto::send::SendDebit {
        from,
        to,
        asset,
        amount,
        source_sequence,
        idempotency_key: facts.idempotency_key,
        expires_at: facts.expires_at_ms,
        context_hash: [0; 32],
        conditions: Vec::new(),
        authorization_kind: 1,
        network_id: facts.network_id,
        protocol_version: 3,
    };
    print_disclosure(
        &json!({"native_debit":format!("{debit:?}"),"actor":facts.actor,"public_key":hex_encode(&facts.public_key),"identity_sequence":facts.identity_next_sequence}),
    )?;
    let payload =
        complete(debit.sign(signer)).map_err(|e| format!("send_signing_failed: {e:?}"))?;
    layerx_platform_cli::wallet_signing::PreparedPayment::from_send(&payload, facts)
}

fn identity_sequence(snapshot: &Value, did: &str) -> Result<u64, String> {
    if snapshot["did"] != did || snapshot["verification"] != "authenticated_node_snapshot" {
        return Err("identity_sequence_unavailable: identity snapshot binding missing".into());
    }
    if !snapshot["next_sequence"].is_string() {
        return Err("identity_sequence_unavailable: canonical sequence missing".into());
    }
    sequence(&snapshot["next_sequence"])
}

fn print_disclosure(value: &Value) -> Result<(), String> {
    use std::io::Write as _;
    let mut stderr = std::io::stderr().lock();
    writeln!(
        stderr,
        "{}",
        serde_json::to_string_pretty(value).map_err(|e| e.to_string())?
    )
    .map_err(|e| e.to_string())?;
    stderr.flush().map_err(|e| e.to_string())
}

struct ThreadWake(std::thread::Thread);
impl std::task::Wake for ThreadWake {
    fn wake(self: std::sync::Arc<Self>) {
        self.0.unpark();
    }
}
fn complete<F: std::future::Future>(future: F) -> F::Output {
    let waker = std::sync::Arc::new(ThreadWake(std::thread::current())).into();
    let mut context = std::task::Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut context) {
            std::task::Poll::Ready(value) => return value,
            std::task::Poll::Pending => std::thread::park(),
        }
    }
}

fn transfer(
    config: &Configuration,
    rpc: Option<&str>,
    gateway: Option<&str>,
    args: &TransferArgs,
) -> Result<CommandOutput, String> {
    let owner = metadata(config, args.key.as_deref())?;
    let asset = fixed_hex("asset", &args.asset)?;
    let amount = units(&args.amount, true)?;
    let transport = Transport::new(config, rpc, gateway)?;
    if transport.emulator {
        let source = account(&owner.did, &asset)?;
        let source_sequence = transport.source_sequence(&source)?;
        return Err(format!("identity_sequence_unavailable: emulator source next_sequence={source_sequence}; hosted identity preparation requires --rpc; no activity signed or submitted"));
    }
    let write = WriteOptions {
        receipt_policy: args
            .write
            .receipt_policy
            .clone()
            .ok_or("provide --receipt-policy with independently trusted authority")?,
        fee_limit: args.write.fee_limit.clone().ok_or("provide --fee-limit")?,
        wait: args.write.wait,
        timeout_seconds: args.write.timeout_seconds,
    };
    execute_write(
        config,
        rpc,
        gateway,
        args.key.as_deref(),
        &write,
        WriteRequest::Send {
            asset,
            to: args.to.clone(),
            amount,
        },
    )
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
    let parsed = layerx_wire::account::account_name_for_asset(did, *asset, native_asset())
        .map_err(|e| format!("invalid account: {e:?}"))?;
    account_id_for_protocol(&parsed, 3)
        .map(|id| hex_encode(&id))
        .map_err(|e| format!("invalid account: {e:?}"))
}

fn validated_account_records<'a>(snapshot: &'a Value, did: &str) -> Result<&'a [Value], String> {
    if snapshot["did"] != did || snapshot["verification"] != "authenticated_node_snapshot" {
        return Err(
            "wallet_accounts_unavailable: authenticated DID snapshot binding missing".into(),
        );
    }
    let accounts = snapshot["accounts"]
        .as_array()
        .filter(|accounts| accounts.len() <= 64)
        .ok_or("wallet_accounts_unavailable: bounded account list missing")?;
    let prefix = format!("agent:{did}:");
    let mut identifiers = std::collections::BTreeSet::new();
    for record in accounts {
        let name = record["name"]
            .as_str()
            .and_then(|name| name.strip_prefix(&prefix).map(|tail| (name, tail)))
            .ok_or("wallet_accounts_unavailable: account name is not bound to the DID")?;
        let namespace = name.1;
        let valid_namespace = namespace == "main"
            || ["asset:", "budget:", "escrow:", "margin:"]
                .iter()
                .any(|marker| {
                    namespace.strip_prefix(marker).is_some_and(|component| {
                        !component.is_empty()
                            && !component.contains(':')
                            && (*marker != "asset:"
                                || (component.len() == 64
                                    && component.bytes().all(|byte| {
                                        byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
                                    })))
                    })
                });
        if !valid_namespace {
            return Err("wallet_accounts_unavailable: unsupported account namespace".into());
        }
        let parsed = layerx_types::account::AccountId::parse(name.0)
            .map_err(|e| format!("wallet_accounts_unavailable: invalid account name: {e:?}"))?;
        let expected = account_id_for_protocol(&parsed, 3)
            .map_err(|e| format!("wallet_accounts_unavailable: invalid account id: {e:?}"))?;
        let id = record["account_id"]
            .as_str()
            .ok_or("wallet_accounts_unavailable: account id missing")?;
        let decoded = fixed_hex::<32>("account id", id)?;
        if decoded != expected || id != hex_encode(&decoded) || !identifiers.insert(decoded) {
            return Err("wallet_accounts_unavailable: account id binding is invalid".into());
        }
        let asset = record["asset_id"]
            .as_str()
            .ok_or("wallet_accounts_unavailable: asset id missing")?;
        let decoded_asset = fixed_hex::<32>("asset id", asset)?;
        if asset != hex_encode(&decoded_asset)
            || !canonical_u128(&record["balance"])
            || !canonical_u64(&record["next_sequence"])
            || !canonical_u64(&record["observed_head_sequence"])
            || !canonical_u64(&record["batch_number"])
            || !canonical_hex_bytes(&record["canonical_value"])
            || !canonical_hex_bytes(&record["proof_material"])
        {
            return Err("wallet_accounts_unavailable: account evidence is noncanonical".into());
        }
    }
    Ok(accounts)
}

fn account_for_asset(
    client: &layerx_sdk::rpc::RpcClient,
    did: &str,
    asset: [u8; 32],
) -> Result<[u8; 32], String> {
    let snapshot = client.get_balances(did).map_err(rpc_error)?.into_value();
    account_for_asset_in_snapshot(&snapshot, did, asset)
}

fn account_for_asset_in_snapshot(
    snapshot: &Value,
    did: &str,
    asset: [u8; 32],
) -> Result<[u8; 32], String> {
    let expected_asset = hex_encode(&asset);
    let main = format!("agent:{did}:main");
    let per_asset = format!("agent:{did}:asset:{expected_asset}");
    let mut selected = validated_account_records(snapshot, did)?
        .iter()
        .filter(|record| {
            record["asset_id"] == expected_asset
                && matches!(record["name"].as_str(), Some(name) if name == main || name == per_asset)
        })
        .map(|record| {
            fixed_hex::<32>(
                "account id",
                record["account_id"]
                    .as_str()
                    .ok_or("wallet account id missing")?,
            )
        });
    let account = selected
        .next()
        .transpose()?
        .ok_or("wallet_account_unavailable: no account exists for the requested asset")?;
    if selected.next().is_some() {
        return Err("wallet_account_unavailable: multiple source accounts match the asset".into());
    }
    Ok(account)
}

fn filter_rpc_balances(mut snapshot: Value, did: &str, asset: [u8; 32]) -> Result<Value, String> {
    let expected = hex_encode(&asset);
    let selected = validated_account_records(&snapshot, did)?
        .iter()
        .filter(|record| record["asset_id"] == expected)
        .cloned()
        .collect();
    snapshot["accounts"] = Value::Array(selected);
    Ok(snapshot)
}

fn canonical_u64(value: &Value) -> bool {
    value.as_str().is_some_and(|text| {
        text.parse::<u64>()
            .is_ok_and(|number| number.to_string() == text)
    })
}

fn canonical_u128(value: &Value) -> bool {
    value.as_str().is_some_and(|text| {
        text.parse::<u128>()
            .is_ok_and(|number| number.to_string() == text)
    })
}

fn canonical_hex_bytes(value: &Value) -> bool {
    value.as_str().is_some_and(|text| {
        !text.is_empty()
            && text.len().is_multiple_of(2)
            && text
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn destination(
    client: &layerx_sdk::rpc::RpcClient,
    to: &str,
    asset: [u8; 32],
) -> Result<[u8; 32], String> {
    if to.starts_with("did:") {
        account_for_asset(client, to, asset)
    } else {
        fixed_hex("destination account", to)
    }
}

fn issuer_id(did: &str) -> Result<[u8; 32], String> {
    layerx_platform_cli::wallet_encoding::native_issuer_id(did)
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

    fn account_record(did: &str, name: &str, asset: [u8; 32], sequence: u64) -> Value {
        let parsed = layerx_types::account::AccountId::parse(name)
            .unwrap_or_else(|error| panic!("invalid test account: {error:?}"));
        let account = account_id_for_protocol(&parsed, 3)
            .unwrap_or_else(|error| panic!("invalid test account id: {error:?}"));
        assert!(name.starts_with(&format!("agent:{did}:")));
        json!({
            "account_id": hex_encode(&account),
            "name": name,
            "asset_id": hex_encode(&asset),
            "balance": "100",
            "next_sequence": sequence.to_string(),
            "canonical_value": "0102",
            "proof_material": "0304",
            "observed_head_sequence": "7",
            "batch_number": "3",
        })
    }

    fn account_snapshot(did: &str, accounts: Vec<Value>) -> Value {
        let mut snapshot = json!({
            "did": did,
            "accounts": [],
            "verification": "authenticated_node_snapshot",
        });
        snapshot["accounts"] = Value::Array(accounts);
        snapshot
    }

    #[test]
    fn identity_snapshot_requires_exact_actor_authentication_and_sequence() -> Result<(), String> {
        let good = json!({"did":"did:layerx:alice","verification":"authenticated_node_snapshot","next_sequence":"19"});
        assert_eq!(identity_sequence(&good, "did:layerx:alice")?, 19);
        for (field, value) in [
            ("did", json!("did:layerx:bob")),
            ("verification", json!("unverified")),
            ("next_sequence", json!("019")),
            ("next_sequence", json!(19)),
            ("next_sequence", json!(null)),
        ] {
            let mut bad = good.clone();
            bad[field] = value;
            assert!(identity_sequence(&bad, "did:layerx:alice").is_err());
        }
        Ok(())
    }

    #[test]
    fn sdk_remote_error_retains_every_field() -> Result<(), String> {
        let message = "DID enumeration unavailable: account index not ready";
        let error = rpc_error(layerx_sdk::rpc::RpcError::Remote {
            code: -32005,
            message: message.into(),
            data: Some(json!({"did":"did:layerx:alice","retry":false})),
        });
        let decoded: Value = serde_json::from_str(&error).map_err(|e| e.to_string())?;
        assert_eq!(
            decoded,
            json!({"code":-32005,"message":message,"data":{"did":"did:layerx:alice","retry":false}})
        );
        Ok(())
    }

    #[test]
    fn policy_scope_and_sdk_wait_fail_closed() -> Result<(), String> {
        let config = Configuration::default();
        let directory = tempfile::tempdir().map_err(|e| e.to_string())?;
        let path = directory.path().join("policy.json");
        let key = ed25519_dalek::SigningKey::from_bytes(&[19; 32])
            .verifying_key()
            .to_bytes();
        let good = json!({"protocol_version":3,"network_id":402,"sequencer_id":"11".repeat(32),"sequencer_key":hex_encode(&key),"first_batch":1,"last_batch":100,"checkpoint_context_digest":null});
        std::fs::write(&path, good.to_string()).map_err(|e| e.to_string())?;
        let policy = read_policy(Some(&path), &config)?;
        let client = layerx_sdk::rpc::RpcClient::connect("http://127.0.0.1:1/rpc", None)
            .map_err(rpc_error)?;
        let id = [7; 32];
        assert!(
            matches!(client.wait_for(id, layerx_sdk::rpc::Commitment::Executed, &policy, std::time::Duration::ZERO), Err(layerx_sdk::rpc::RpcError::Pending { activity_id }) if activity_id == id)
        );
        assert!(matches!(
            client.wait_for(
                id,
                layerx_sdk::rpc::Commitment::Finalised,
                &policy,
                std::time::Duration::ZERO
            ),
            Err(layerx_sdk::rpc::RpcError::MissingFinalityTrust)
        ));
        assert!(read_policy(None, &config).is_err());
        for (field, value) in [
            ("protocol_version", json!(2)),
            ("network_id", json!(401)),
            ("first_batch", json!(0)),
            ("last_batch", json!(0)),
            ("sequencer_key", json!("broken")),
            ("checkpoint_context_digest", json!("broken")),
        ] {
            let mut bad = good.clone();
            bad[field] = value;
            std::fs::write(&path, bad.to_string()).map_err(|e| e.to_string())?;
            assert!(read_policy(Some(&path), &config).is_err(), "{field}");
        }
        Ok(())
    }

    #[test]
    fn namespaces_and_sequence_bounds() -> Result<(), String> {
        let did = "did:layerx:alice";
        assert_ne!(account(did, &native_asset())?, account(did, &[1; 32])?);
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
    fn authenticated_account_resolution_uses_the_deployed_native_asset() -> Result<(), String> {
        let did = "did:layerx:alice";
        let native = [0x44; 32];
        let token = [0x55; 32];
        let main_name = format!("agent:{did}:main");
        let token_name = format!("agent:{did}:asset:{}", hex_encode(&token));
        let main = account_record(did, &main_name, native, 7);
        let token_account = account_record(did, &token_name, token, 9);
        let snapshot = account_snapshot(did, vec![main.clone(), token_account.clone()]);

        assert_eq!(
            hex_encode(&account_for_asset_in_snapshot(&snapshot, did, native)?),
            main["account_id"]
        );
        assert_eq!(
            hex_encode(&account_for_asset_in_snapshot(&snapshot, did, token)?),
            token_account["account_id"]
        );
        let filtered = filter_rpc_balances(snapshot.clone(), did, token)?;
        assert_eq!(filtered["accounts"], json!([token_account]));
        assert!(account_for_asset_in_snapshot(&snapshot, did, [0x66; 32]).is_err());

        let duplicate = account_record(
            did,
            &format!("agent:{did}:asset:{}", hex_encode(&native)),
            native,
            10,
        );
        assert!(account_for_asset_in_snapshot(
            &account_snapshot(did, vec![main, duplicate]),
            did,
            native,
        )
        .is_err());
        Ok(())
    }

    #[test]
    fn authenticated_account_resolution_refuses_unbound_or_noncanonical_evidence() {
        let did = "did:layerx:alice";
        let asset = [0x44; 32];
        let name = format!("agent:{did}:main");
        let good = account_record(did, &name, asset, 7);
        for (field, value) in [
            ("account_id", json!("11".repeat(32))),
            ("asset_id", json!("AA".repeat(32))),
            ("balance", json!("0100")),
            ("next_sequence", json!(7)),
            ("canonical_value", json!("")),
            ("proof_material", json!("xyz")),
            ("observed_head_sequence", json!("07")),
            ("batch_number", json!(null)),
        ] {
            let mut changed = good.clone();
            changed[field] = value;
            assert!(validated_account_records(&account_snapshot(did, vec![changed]), did).is_err());
        }
        let mut wrong_did = account_snapshot(did, vec![good.clone()]);
        wrong_did["did"] = json!("did:layerx:bob");
        assert!(validated_account_records(&wrong_did, did).is_err());

        let mut wrong_verification = account_snapshot(did, vec![good]);
        wrong_verification["verification"] = json!("unverified");
        assert!(validated_account_records(&wrong_verification, did).is_err());
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

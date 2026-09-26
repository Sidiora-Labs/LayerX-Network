//! The owner's client for the Paxeer X Network custody program on Solana.
//!
//! It reads the Solana chain configuration, refuses every placeholder and every
//! value the program would refuse, and builds, prints, signs and sends the
//! owner's transactions: initialise with the attestor set and threshold,
//! register every asset in configuration order, set the caps, pause and
//! unpause, and register a recipient's key under its handle. Every instruction
//! is encoded through the program crate's own constants and every account is
//! read back through its own layouts, so the client and the program share one
//! definition of every byte.
//!
//! The endpoint and the keys arrive only through environment variables: the
//! endpoint and the fee-payer keypair file through the variables the
//! configuration names in `environment.rpc_url` and `environment.deploy_key`,
//! the owner's keypair file through `PAXEER_BRIDGE_SOLANA_OWNER_KEYPAIR_FILE`
//! and a registering recipient's keypair file through
//! `PAXEER_BRIDGE_SOLANA_RECIPIENT_KEYPAIR_FILE`. No endpoint, key or address
//! is compiled in, and the endpoint is never printed.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::str::FromStr;
use std::thread;
use std::time::Duration;

use base64::Engine;
use paxeer_x_bridge_solana_program::identity::{
    find_vault_authority, hex, pubkey_handle, HANDLE_BYTES, SIDIORA_ASSET_ID, SIDIORA_MINT,
    SOLANA_CHAIN_ID,
};
use paxeer_x_bridge_solana_program::state::{
    find_asset_address, find_config_address, find_recipient_address, Asset, Config,
    RecipientRecord, MAX_ATTESTORS,
};
use paxeer_x_bridge_solana_program::{
    BridgeError, INSTRUCTION_MAGIC, INSTRUCTION_VERSION, OP_INITIALISE, OP_REGISTER_ASSET,
    OP_REGISTER_RECIPIENT, OP_SET_ATTESTORS, OP_SET_CAP, OP_SET_PAUSE,
};
use serde::Deserialize;
use solana_account_decoder_client_types::UiAccountEncoding;
use solana_commitment_config::CommitmentConfig;
use solana_loader_v3_interface::state::UpgradeableLoaderState;
use solana_program::program_pack::Pack;
use solana_rpc_client::rpc_client::RpcClient;
use solana_rpc_client_api::config::{RpcAccountInfoConfig, RpcSendTransactionConfig};
use solana_sdk::instruction::{AccountMeta, Instruction, InstructionError};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signature};
use solana_sdk::signer::keypair::read_keypair_file;
use solana_sdk::signer::Signer;
use solana_sdk::transaction::{Transaction, TransactionError};
use solana_sdk_ids::{bpf_loader_upgradeable, system_program};
use spl_token::state::Mint;

/// The variable naming the owner's keypair file. The configuration records the
/// owner's public key; the key that signs as that owner arrives here.
pub const OWNER_KEY_VARIABLE: &str = "PAXEER_BRIDGE_SOLANA_OWNER_KEYPAIR_FILE";
/// The variable naming the keypair file of an account registering its own key
/// under its handle.
pub const RECIPIENT_KEY_VARIABLE: &str = "PAXEER_BRIDGE_SOLANA_RECIPIENT_KEYPAIR_FILE";
/// The variable naming a chains root, the same one the deploy script reads: the
/// configuration is `<root>/solana/config.json`.
pub const CHAINS_ROOT_VARIABLE: &str = "PAXEER_BRIDGE_SOLANA_CHAINS_ROOT";
/// The configuration committed beside this crate, read when neither `--config`
/// nor the chains root variable names another one.
const COMMITTED_CONFIGURATION: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../chains/solana/config.json");

const PLACEHOLDER_PREFIX: &str = "PLACEHOLDER:";
const NATIVE_SYMBOL: &str = "SOL";
const NATIVE_DECIMALS: u8 = 9;
const SIDIORA_DECIMALS: u8 = 6;
/// How often and how long a sent transaction is polled for its commitment.
const CONFIRM_INTERVAL: Duration = Duration::from_millis(500);
const CONFIRM_ATTEMPTS: u32 = 120;

const USAGE: &str = "usage: paxeer-x-bridge-solana-admin <command> [--config <file>] [options]

commands:
  apply               initialise the program, register every configured asset in order,
                      set every cap, then print the on-chain state
  initialise          initialise the program with the configured owner, attestors and threshold
  register-asset      register one configured asset (--mint) and set its caps
  set-cap             set the caps of every configured asset, or of one with --mint
  pause               pause custody
  unpause             unpause custody
  register-recipient  register the recipient key's own pubkey under its handle
  show                print the on-chain state

Values passed as --url, --keypair, --program-id, --commitment, --owner, --attestors,
--threshold, --asset-id, --decimals, --per-tx-cap and --total-cap are checked against
the configuration and the environment it names; a disagreement stops the run.";

/// A refusal: the run stops, and the message says why.
#[derive(Debug)]
struct Refusal(String);

impl fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

fn refuse<T>(message: impl Into<String>) -> Result<T, Refusal> {
    Err(Refusal(message.into()))
}

// ---------------------------------------------------------------------------
// The configuration: the same fields bridge/deploy/chainconfig decodes, with
// unknown fields refused.
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChainFile {
    chain: String,
    kind: String,
    chain_id: u64,
    native: NativeFile,
    environment: EnvironmentFile,
    owner: String,
    attestors: Vec<String>,
    threshold: u32,
    finality_depth: u64,
    #[serde(default)]
    big_blocks: Option<BigBlocksFile>,
    #[serde(default)]
    solana: Option<SolanaFile>,
    assets: Vec<AssetFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeFile {
    symbol: String,
    decimals: u8,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EnvironmentFile {
    rpc_url: String,
    deploy_key: String,
    #[serde(default)]
    explorer_key: Option<String>,
    #[serde(default)]
    toolchain_bin: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BigBlocksFile {
    #[serde(rename = "required")]
    _required: bool,
    #[serde(rename = "acknowledged")]
    _acknowledged: bool,
    #[serde(rename = "requirement")]
    _requirement: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SolanaFile {
    program_id: String,
    commitment: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AssetFile {
    symbol: String,
    address: String,
    asset_id: String,
    decimals: u8,
    per_tx_cap: String,
    total_cap: String,
}

/// A configuration the program would accept, in the program's own types.
struct Settings {
    path: PathBuf,
    rpc_variable: String,
    deploy_key_variable: String,
    owner: Pubkey,
    attestors: Vec<[u8; HANDLE_BYTES]>,
    threshold: u8,
    program_id: Pubkey,
    commitment_name: String,
    commitment: CommitmentConfig,
    assets: Vec<AssetPlan>,
}

/// One configured asset, as the program registers it.
struct AssetPlan {
    index: usize,
    symbol: String,
    mint: Pubkey,
    asset_id: [u8; HANDLE_BYTES],
    decimals: u8,
    per_tx_cap: u64,
    total_cap: u64,
}

impl Settings {
    fn refusal(&self, field: &str, reason: impl fmt::Display) -> Refusal {
        field_refusal(&self.path, field, reason)
    }

    fn asset_by_mint(&self, mint: &Pubkey) -> Result<&AssetPlan, Refusal> {
        self.assets
            .iter()
            .find(|asset| &asset.mint == mint)
            .ok_or_else(|| {
                self.refusal("assets", format!("no configured asset has the mint {mint}"))
            })
    }
}

fn field_refusal(path: &Path, field: &str, reason: impl fmt::Display) -> Refusal {
    Refusal(format!("{}: {field}: {reason}", path.display()))
}

/// Read and check the configuration. Every refusal names the file and the
/// field, and nothing that reaches the cluster is left unchecked here.
fn load_configuration(path: &Path) -> Result<Settings, Refusal> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| Refusal(format!("{}: {error}", path.display())))?;
    let file: ChainFile = serde_json::from_str(&text)
        .map_err(|error| Refusal(format!("{}: {error}", path.display())))?;
    let refusal = |field: &str, reason: String| field_refusal(path, field, reason);

    if file.chain != "solana" {
        return Err(refusal(
            "chain",
            format!("{} is not the Solana chain", file.chain),
        ));
    }
    if file.kind != "solana" {
        return Err(refusal(
            "kind",
            format!("{} is not the Solana kind", file.kind),
        ));
    }
    if file.chain_id != SOLANA_CHAIN_ID {
        return Err(refusal(
            "chain_id",
            format!(
                "Solana is chain {SOLANA_CHAIN_ID} on the Paxeer side, not {}",
                file.chain_id
            ),
        ));
    }
    if file.native.symbol != NATIVE_SYMBOL {
        return Err(refusal(
            "native.symbol",
            format!(
                "Solana's native coin is {NATIVE_SYMBOL}, not {}",
                file.native.symbol
            ),
        ));
    }
    if file.native.decimals != NATIVE_DECIMALS {
        return Err(refusal(
            "native.decimals",
            format!(
                "{NATIVE_SYMBOL} carries {NATIVE_DECIMALS} decimals, not {}",
                file.native.decimals
            ),
        ));
    }
    require_variable_name(path, "environment.rpc_url", &file.environment.rpc_url)?;
    require_variable_name(path, "environment.deploy_key", &file.environment.deploy_key)?;
    if let Some(name) = &file.environment.explorer_key {
        require_variable_name(path, "environment.explorer_key", name)?;
    }
    if let Some(name) = &file.environment.toolchain_bin {
        require_variable_name(path, "environment.toolchain_bin", name)?;
    }

    let owner = parse_pubkey(path, "owner", &file.owner)?;

    if file.attestors.is_empty() {
        return Err(refusal("attestors", "the attestor set is empty".into()));
    }
    if file.attestors.len() > MAX_ATTESTORS {
        return Err(refusal(
            "attestors",
            format!(
                "{} attestors do not fit the program's {MAX_ATTESTORS}",
                file.attestors.len()
            ),
        ));
    }
    let mut attestors: Vec<[u8; HANDLE_BYTES]> = Vec::with_capacity(file.attestors.len());
    for (index, value) in file.attestors.iter().enumerate() {
        let field = format!("attestors[{index}]");
        require_no_placeholder(path, &field, value)?;
        let attestor = parse_address(value).ok_or_else(|| {
            refusal(
                &field,
                format!("{value} is not a 20-byte secp256k1 address"),
            )
        })?;
        if attestor == [0_u8; HANDLE_BYTES] {
            return Err(refusal(
                &field,
                "the zero address is not an attestor".into(),
            ));
        }
        if let Some(previous) = attestors.last() {
            if attestor <= *previous {
                return Err(refusal(
                    &field,
                    format!(
                        "{value} does not follow the attestor before it; the set is strictly ascending"
                    ),
                ));
            }
        }
        attestors.push(attestor);
    }
    let threshold = u8::try_from(file.threshold)
        .ok()
        .filter(|threshold| *threshold != 0 && usize::from(*threshold) <= attestors.len())
        .ok_or_else(|| {
            refusal(
                "threshold",
                format!(
                    "{} is not between one and the {} attestors of the set",
                    file.threshold,
                    attestors.len()
                ),
            )
        })?;
    if file.big_blocks.is_some() {
        return Err(refusal(
            "big_blocks",
            "the big-block acknowledgement belongs to hyperevm, not to Solana".into(),
        ));
    }
    if file.finality_depth == 0 {
        return Err(refusal(
            "finality_depth",
            "a slot depth of zero is not a depth".into(),
        ));
    }

    let solana = file
        .solana
        .as_ref()
        .ok_or_else(|| refusal("solana", "the Solana section is required".into()))?;
    let program_id = parse_pubkey(path, "solana.program_id", &solana.program_id)?;
    let commitment = match solana.commitment.as_str() {
        "confirmed" => CommitmentConfig::confirmed(),
        "finalized" => CommitmentConfig::finalized(),
        other => {
            return Err(refusal(
                "solana.commitment",
                format!("{other} would read state that can still be dropped"),
            ))
        }
    };

    if file.assets.is_empty() {
        return Err(refusal("assets", "the asset list is empty".into()));
    }
    let mut assets: Vec<AssetPlan> = Vec::with_capacity(file.assets.len());
    for (index, entry) in file.assets.iter().enumerate() {
        let field = |name: &str| format!("assets[{index}].{name}");
        let mint = parse_pubkey(path, &field("address"), &entry.address)?;
        if assets.iter().any(|asset| asset.mint == mint) {
            return Err(refusal(
                &field("address"),
                format!("{mint} is configured twice"),
            ));
        }
        let asset_id = parse_address(&entry.asset_id).ok_or_else(|| {
            refusal(
                &field("asset_id"),
                format!("{} is not a 20-byte asset id", entry.asset_id),
            )
        })?;
        if asset_id == [0_u8; HANDLE_BYTES] {
            return Err(refusal(
                &field("asset_id"),
                "the zero id is not an asset id".into(),
            ));
        }
        if mint == SIDIORA_MINT {
            if asset_id != SIDIORA_ASSET_ID {
                return Err(refusal(
                    &field("asset_id"),
                    format!(
                        "Sidiora's mint enters the digests as 0x{}, not as {}",
                        hex(&SIDIORA_ASSET_ID),
                        entry.asset_id
                    ),
                ));
            }
            if entry.decimals != SIDIORA_DECIMALS {
                return Err(refusal(
                    &field("decimals"),
                    format!(
                        "Sidiora carries {SIDIORA_DECIMALS} decimals, not {}",
                        entry.decimals
                    ),
                ));
            }
        } else {
            if asset_id == SIDIORA_ASSET_ID {
                return Err(refusal(
                    &field("asset_id"),
                    format!(
                        "0x{} is the id fixed for Sidiora's mint {SIDIORA_MINT}, not for {mint}",
                        hex(&SIDIORA_ASSET_ID)
                    ),
                ));
            }
            let derived = pubkey_handle(&mint);
            if asset_id != derived {
                return Err(refusal(
                    &field("asset_id"),
                    format!(
                        "{mint} enters the digests as its derived handle 0x{}, not as {}",
                        hex(&derived),
                        entry.asset_id
                    ),
                ));
            }
        }
        let per_tx_cap = parse_cap(path, &field("per_tx_cap"), &entry.per_tx_cap)?;
        let total_cap = parse_cap(path, &field("total_cap"), &entry.total_cap)?;
        if per_tx_cap > total_cap {
            return Err(refusal(
                &field("per_tx_cap"),
                format!(
                    "{per_tx_cap} is above the total cap {total_cap}, which the program refuses"
                ),
            ));
        }
        assets.push(AssetPlan {
            index,
            symbol: entry.symbol.clone(),
            mint,
            asset_id,
            decimals: entry.decimals,
            per_tx_cap,
            total_cap,
        });
    }
    let native = &assets[0];
    if native.mint != spl_token::native_mint::id() {
        return Err(refusal(
            "assets[0].address",
            format!(
                "the first asset of Solana is the wrapped SOL mint {}, not {}",
                spl_token::native_mint::id(),
                native.mint
            ),
        ));
    }
    if native.decimals != NATIVE_DECIMALS {
        return Err(refusal(
            "assets[0].decimals",
            format!(
                "wrapped SOL carries {NATIVE_DECIMALS} decimals, not {}",
                native.decimals
            ),
        ));
    }

    Ok(Settings {
        path: path.to_path_buf(),
        rpc_variable: file.environment.rpc_url.clone(),
        deploy_key_variable: file.environment.deploy_key.clone(),
        owner,
        attestors,
        threshold,
        program_id,
        commitment_name: solana.commitment.clone(),
        commitment,
        assets,
    })
}

fn require_no_placeholder(path: &Path, field: &str, value: &str) -> Result<(), Refusal> {
    if value.starts_with(PLACEHOLDER_PREFIX) {
        return Err(field_refusal(
            path,
            field,
            format!("{value} is a placeholder; fill in the real value first"),
        ));
    }
    Ok(())
}

fn require_variable_name(path: &Path, field: &str, name: &str) -> Result<(), Refusal> {
    let well_formed = name.split('_').enumerate().all(|(index, part)| {
        !part.is_empty()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
            && (index != 0 || part.as_bytes()[0].is_ascii_uppercase())
    });
    if !well_formed {
        return Err(field_refusal(
            path,
            field,
            format!("{name:?} is not an upper snake case environment variable name"),
        ));
    }
    Ok(())
}

fn parse_pubkey(path: &Path, field: &str, value: &str) -> Result<Pubkey, Refusal> {
    require_no_placeholder(path, field, value)?;
    let key = Pubkey::from_str(value)
        .map_err(|_| field_refusal(path, field, format!("{value} is not a base58 Solana key")))?;
    if key == Pubkey::default() {
        return Err(field_refusal(path, field, "the zero pubkey is not a value"));
    }
    Ok(key)
}

fn parse_cap(path: &Path, field: &str, value: &str) -> Result<u64, Refusal> {
    let digits = !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit());
    if !digits || (value.len() > 1 && value.starts_with('0')) {
        return Err(field_refusal(
            path,
            field,
            format!("{value:?} is not a decimal cap"),
        ));
    }
    let cap = value.parse::<u64>().map_err(|_| {
        field_refusal(
            path,
            field,
            format!("{value} does not fit the program's 64-bit cap"),
        )
    })?;
    if cap == 0 {
        return Err(field_refusal(path, field, "a cap of zero admits nothing"));
    }
    Ok(cap)
}

/// A `0x`-prefixed 20-byte hexadecimal address, in either case.
fn parse_address(value: &str) -> Option<[u8; HANDLE_BYTES]> {
    let digits = value.strip_prefix("0x")?;
    if digits.len() != HANDLE_BYTES * 2 {
        return None;
    }
    let mut out = [0_u8; HANDLE_BYTES];
    for (index, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(digits.get(index * 2..index * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// The environment: the endpoint and the keys.
// ---------------------------------------------------------------------------

fn required_variable(name: &str) -> Result<String, Refusal> {
    match std::env::var(name) {
        Ok(value) if !value.is_empty() => Ok(value),
        _ => refuse(format!("{name} is required and is not set")),
    }
}

fn keypair_from(variable: &str) -> Result<Keypair, Refusal> {
    let path = required_variable(variable)?;
    read_keypair_file(&path).map_err(|_| {
        Refusal(format!(
            "{variable} names {path}, which is not a readable Solana keypair file"
        ))
    })
}

/// The owner's key, which must be the owner the configuration names.
fn owner_keypair(settings: &Settings) -> Result<Keypair, Refusal> {
    let owner = keypair_from(OWNER_KEY_VARIABLE)?;
    if owner.pubkey() != settings.owner {
        return Err(settings.refusal(
            "owner",
            format!(
                "{} is the configured owner and {OWNER_KEY_VARIABLE} holds {}",
                settings.owner,
                owner.pubkey()
            ),
        ));
    }
    Ok(owner)
}

// ---------------------------------------------------------------------------
// The command line.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Command {
    Apply,
    Initialise,
    RegisterAsset,
    SetCap,
    Pause,
    Unpause,
    RegisterRecipient,
    Show,
}

impl Command {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "apply" => Self::Apply,
            "initialise" => Self::Initialise,
            "register-asset" => Self::RegisterAsset,
            "set-cap" => Self::SetCap,
            "pause" => Self::Pause,
            "unpause" => Self::Unpause,
            "register-recipient" => Self::RegisterRecipient,
            "show" => Self::Show,
            _ => return None,
        })
    }

    /// The options a command accepts beyond the common ones.
    fn options(self) -> &'static [&'static str] {
        match self {
            Self::Initialise => &["--owner", "--attestors", "--threshold"],
            Self::RegisterAsset => &[
                "--mint",
                "--asset-id",
                "--decimals",
                "--per-tx-cap",
                "--total-cap",
            ],
            Self::SetCap => &["--mint"],
            _ => &[],
        }
    }
}

const COMMON_OPTIONS: [&str; 5] = [
    "--config",
    "--url",
    "--keypair",
    "--program-id",
    "--commitment",
];

struct Invocation {
    command: Command,
    options: BTreeMap<String, String>,
}

impl Invocation {
    fn option(&self, name: &str) -> Option<&str> {
        self.options.get(name).map(String::as_str)
    }
}

fn parse_arguments(arguments: &[String]) -> Result<Invocation, Refusal> {
    let (name, rest) = arguments
        .split_first()
        .ok_or_else(|| Refusal("a command is required".into()))?;
    let command =
        Command::parse(name).ok_or_else(|| Refusal(format!("{name} is not a command")))?;
    let mut options = BTreeMap::new();
    let mut rest = rest.iter();
    while let Some(flag) = rest.next() {
        if !COMMON_OPTIONS.contains(&flag.as_str()) && !command.options().contains(&flag.as_str()) {
            return refuse(format!("{flag} is not an option of {name}"));
        }
        let value = rest
            .next()
            .ok_or_else(|| Refusal(format!("{flag} needs a value")))?;
        if options.insert(flag.clone(), value.clone()).is_some() {
            return refuse(format!("{flag} is given twice"));
        }
    }
    Ok(Invocation { command, options })
}

fn configuration_path(invocation: &Invocation) -> PathBuf {
    if let Some(path) = invocation.option("--config") {
        return PathBuf::from(path);
    }
    match std::env::var(CHAINS_ROOT_VARIABLE) {
        Ok(root) if !root.is_empty() => Path::new(&root).join("solana").join("config.json"),
        _ => PathBuf::from(COMMITTED_CONFIGURATION),
    }
}

/// Refuse a value given on the command line that the configuration, or the
/// environment variable it names, disagrees with. The configuration is the one
/// source of every value; a flag can only confirm it.
fn require_agreement(
    settings: &Settings,
    invocation: &Invocation,
    rpc: &str,
) -> Result<(), Refusal> {
    let agree = |flag: &str, field: &str, expected: &str| -> Result<(), Refusal> {
        match invocation.option(flag) {
            Some(given) if given != expected => Err(settings.refusal(
                field,
                format!("{flag} {given} disagrees with the configured {expected}"),
            )),
            _ => Ok(()),
        }
    };
    if let Some(given) = invocation.option("--url") {
        if given != rpc {
            return refuse(format!(
                "--url is not the endpoint {} carries",
                settings.rpc_variable
            ));
        }
    }
    if let Some(given) = invocation.option("--keypair") {
        let expected = required_variable(&settings.deploy_key_variable)?;
        if given != expected {
            return refuse(format!(
                "--keypair {given} is not the keypair file {} names",
                settings.deploy_key_variable
            ));
        }
    }
    agree(
        "--program-id",
        "solana.program_id",
        &settings.program_id.to_string(),
    )?;
    agree(
        "--commitment",
        "solana.commitment",
        &settings.commitment_name,
    )?;
    agree("--owner", "owner", &settings.owner.to_string())?;
    agree("--threshold", "threshold", &settings.threshold.to_string())?;
    if let Some(given) = invocation.option("--attestors") {
        let parsed: Option<Vec<[u8; HANDLE_BYTES]>> = given.split(',').map(parse_address).collect();
        if parsed.as_deref() != Some(settings.attestors.as_slice()) {
            return Err(settings.refusal(
                "attestors",
                format!("--attestors {given} disagrees with the configured attestor set"),
            ));
        }
    }
    if let Some(mint) = invocation.option("--mint") {
        let mint = Pubkey::from_str(mint)
            .map_err(|_| Refusal(format!("--mint {mint} is not a base58 Solana key")))?;
        let asset = settings.asset_by_mint(&mint)?;
        let prefix = format!("assets[{}]", asset.index);
        if let Some(given) = invocation.option("--asset-id") {
            if parse_address(given) != Some(asset.asset_id) {
                return Err(settings.refusal(
                    &format!("{prefix}.asset_id"),
                    format!(
                        "--asset-id {given} disagrees with the configured 0x{}",
                        hex(&asset.asset_id)
                    ),
                ));
            }
        }
        agree(
            "--decimals",
            &format!("{prefix}.decimals"),
            &asset.decimals.to_string(),
        )?;
        agree(
            "--per-tx-cap",
            &format!("{prefix}.per_tx_cap"),
            &asset.per_tx_cap.to_string(),
        )?;
        agree(
            "--total-cap",
            &format!("{prefix}.total_cap"),
            &asset.total_cap.to_string(),
        )?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The cluster.
// ---------------------------------------------------------------------------

/// The owner's view of one cluster: account reads at the configured
/// commitment, and transactions that are printed, signed, sent and confirmed.
struct Cluster {
    rpc: RpcClient,
    commitment: CommitmentConfig,
}

/// An account's owner and bytes, as the cluster answers them.
struct Fetched {
    owner: Pubkey,
    data: Vec<u8>,
}

impl Cluster {
    fn new(url: String, commitment: CommitmentConfig) -> Self {
        Self {
            rpc: RpcClient::new_with_commitment(url, commitment),
            commitment,
        }
    }

    fn account(&self, key: &Pubkey) -> Result<Option<Fetched>, Refusal> {
        let config = RpcAccountInfoConfig {
            encoding: Some(UiAccountEncoding::Base64),
            data_slice: None,
            commitment: Some(self.commitment),
            min_context_slot: None,
        };
        let response = self
            .rpc
            .get_account_with_config(key, config)
            .map_err(|error| Refusal(format!("getAccountInfo {key}: {error}")))?;
        Ok(response.value.map(|account| Fetched {
            owner: account.owner,
            data: account.data,
        }))
    }

    /// Build the transaction, print it, sign it, send it and wait until it
    /// reaches the configured commitment. `payer` pays the fee; `signers` are
    /// the other keys the instructions require.
    fn submit(
        &self,
        label: &str,
        instructions: &[Instruction],
        payer: &Keypair,
        signers: &[&Keypair],
    ) -> Result<Signature, Refusal> {
        let (blockhash, _) = self
            .rpc
            .get_latest_blockhash_with_commitment(self.commitment)
            .map_err(|error| Refusal(format!("getLatestBlockhash: {error}")))?;
        let mut keys: Vec<&Keypair> = vec![payer];
        for signer in signers {
            if !keys.iter().any(|key| key.pubkey() == signer.pubkey()) {
                keys.push(signer);
            }
        }
        let mut transaction = Transaction::new_with_payer(instructions, Some(&payer.pubkey()));
        transaction.try_sign(&keys, blockhash).map_err(|error| {
            Refusal(format!(
                "{label}: the transaction cannot be signed: {error}"
            ))
        })?;
        print_transaction(label, instructions, &transaction)?;
        let config = RpcSendTransactionConfig {
            skip_preflight: false,
            preflight_commitment: Some(self.commitment.commitment),
            ..RpcSendTransactionConfig::default()
        };
        let signature = self
            .rpc
            .send_transaction_with_config(&transaction, config)
            .map_err(|error| match error.get_transaction_error() {
                Some(failure) => Refusal(format!(
                    "{label}: the program refused the transaction: {}",
                    describe_failure(&failure)
                )),
                None => Refusal(format!(
                    "{label}: the cluster refused the transaction: {error}"
                )),
            })?;
        for _ in 0..CONFIRM_ATTEMPTS {
            let statuses = self
                .rpc
                .get_signature_statuses(&[signature])
                .map_err(|error| Refusal(format!("getSignatureStatuses {signature}: {error}")))?;
            if let Some(Some(status)) = statuses.value.first() {
                if let Some(failure) = &status.err {
                    return refuse(format!(
                        "{label}: {signature} failed: {}",
                        describe_failure(failure)
                    ));
                }
                if status.satisfies_commitment(self.commitment) {
                    println!("confirmed {label} {signature}");
                    return Ok(signature);
                }
            }
            thread::sleep(CONFIRM_INTERVAL);
        }
        refuse(format!(
            "{label}: {signature} did not reach the {:?} commitment",
            self.commitment.commitment
        ))
    }

    fn config_record(&self, program_id: &Pubkey) -> Result<Option<Config>, Refusal> {
        let key = find_config_address(program_id).0;
        let Some(account) = self.account(&key)? else {
            return Ok(None);
        };
        if &account.owner != program_id {
            return refuse(format!(
                "the config account {key} is not owned by {program_id}"
            ));
        }
        Config::decode(&account.data)
            .map(Some)
            .map_err(|_| Refusal(format!("the config account {key} is not a config record")))
    }

    fn asset_record(&self, program_id: &Pubkey, mint: &Pubkey) -> Result<Option<Asset>, Refusal> {
        let key = find_asset_address(program_id, mint).0;
        let Some(account) = self.account(&key)? else {
            return Ok(None);
        };
        if &account.owner != program_id {
            return refuse(format!(
                "the asset account {key} is not owned by {program_id}"
            ));
        }
        Asset::decode(&account.data)
            .map(Some)
            .map_err(|_| Refusal(format!("the asset account {key} is not an asset record")))
    }
}

fn print_transaction(
    label: &str,
    instructions: &[Instruction],
    transaction: &Transaction,
) -> Result<(), Refusal> {
    let bytes = bincode::serialize(transaction).map_err(|error| {
        Refusal(format!(
            "{label}: the transaction does not serialise: {error}"
        ))
    })?;
    println!("transaction {label}");
    println!("  fee payer {}", transaction.message.account_keys[0]);
    println!(
        "  recent blockhash {}",
        transaction.message.recent_blockhash
    );
    for (index, instruction) in instructions.iter().enumerate() {
        println!(
            "  instruction {index} program {} data {}",
            instruction.program_id,
            hex(&instruction.data)
        );
        for meta in &instruction.accounts {
            println!(
                "    account {}{}{}",
                meta.pubkey,
                if meta.is_signer { " signer" } else { "" },
                if meta.is_writable { " writable" } else { "" }
            );
        }
    }
    for signature in &transaction.signatures {
        println!("  signature {signature}");
    }
    println!(
        "  base64 {}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// The program's instructions, encoded through the program crate's constants.
// ---------------------------------------------------------------------------

fn payload(opcode: u8, tail: &[u8]) -> Vec<u8> {
    let mut data = INSTRUCTION_MAGIC.to_vec();
    data.extend_from_slice(&INSTRUCTION_VERSION.to_be_bytes());
    data.push(opcode);
    data.extend_from_slice(tail);
    data
}

fn program_data_address(program_id: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[program_id.as_ref()], &bpf_loader_upgradeable::id()).0
}

fn initialise_instruction(program_id: &Pubkey, payer: &Pubkey, owner: &Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        *program_id,
        &payload(OP_INITIALISE, owner.as_ref()),
        vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new(find_config_address(program_id).0, false),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new_readonly(program_data_address(program_id), false),
        ],
    )
}

fn set_attestors_instruction(
    program_id: &Pubkey,
    owner: &Pubkey,
    attestors: &[[u8; HANDLE_BYTES]],
    threshold: u8,
) -> Result<Instruction, Refusal> {
    let count = u8::try_from(attestors.len())
        .map_err(|_| Refusal(format!("{} attestors do not fit one byte", attestors.len())))?;
    let mut tail = vec![count];
    for attestor in attestors {
        tail.extend_from_slice(attestor);
    }
    tail.push(threshold);
    Ok(Instruction::new_with_bytes(
        *program_id,
        &payload(OP_SET_ATTESTORS, &tail),
        vec![
            AccountMeta::new_readonly(*owner, true),
            AccountMeta::new(find_config_address(program_id).0, false),
        ],
    ))
}

fn register_asset_instruction(
    program_id: &Pubkey,
    owner: &Pubkey,
    asset: &AssetPlan,
) -> Instruction {
    let mut tail = Vec::new();
    if asset.asset_id == pubkey_handle(&asset.mint) {
        tail.push(0);
    } else {
        tail.push(1);
        tail.extend_from_slice(&asset.asset_id);
    }
    tail.extend_from_slice(&asset.per_tx_cap.to_be_bytes());
    tail.extend_from_slice(&asset.total_cap.to_be_bytes());
    Instruction::new_with_bytes(
        *program_id,
        &payload(OP_REGISTER_ASSET, &tail),
        vec![
            AccountMeta::new(*owner, true),
            AccountMeta::new_readonly(find_config_address(program_id).0, false),
            AccountMeta::new(find_asset_address(program_id, &asset.mint).0, false),
            AccountMeta::new_readonly(asset.mint, false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
    )
}

fn set_cap_instruction(program_id: &Pubkey, owner: &Pubkey, asset: &AssetPlan) -> Instruction {
    let mut tail = Vec::new();
    tail.extend_from_slice(&asset.per_tx_cap.to_be_bytes());
    tail.extend_from_slice(&asset.total_cap.to_be_bytes());
    tail.push(1);
    Instruction::new_with_bytes(
        *program_id,
        &payload(OP_SET_CAP, &tail),
        vec![
            AccountMeta::new_readonly(*owner, true),
            AccountMeta::new_readonly(find_config_address(program_id).0, false),
            AccountMeta::new(find_asset_address(program_id, &asset.mint).0, false),
        ],
    )
}

fn set_pause_instruction(program_id: &Pubkey, owner: &Pubkey, paused: bool) -> Instruction {
    Instruction::new_with_bytes(
        *program_id,
        &payload(OP_SET_PAUSE, &[u8::from(paused)]),
        vec![
            AccountMeta::new_readonly(*owner, true),
            AccountMeta::new(find_config_address(program_id).0, false),
        ],
    )
}

fn register_recipient_instruction(
    program_id: &Pubkey,
    payer: &Pubkey,
    registrant: &Pubkey,
) -> Instruction {
    let handle = pubkey_handle(registrant);
    Instruction::new_with_bytes(
        *program_id,
        &payload(OP_REGISTER_RECIPIENT, &handle),
        vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new_readonly(find_config_address(program_id).0, false),
            AccountMeta::new_readonly(*registrant, true),
            AccountMeta::new(find_recipient_address(program_id, &handle).0, false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
    )
}

// ---------------------------------------------------------------------------
// The owner's operations.
// ---------------------------------------------------------------------------

/// Refuse a program that is already initialised for another owner, and a
/// paused one, whose owner instructions the program refuses.
fn require_configured_owner(settings: &Settings, config: &Config) -> Result<(), Refusal> {
    if config.owner != settings.owner {
        return Err(settings.refusal(
            "owner",
            format!(
                "program {} is already initialised with owner {}, and the configuration names {}",
                settings.program_id, config.owner, settings.owner
            ),
        ));
    }
    Ok(())
}

fn require_unpaused(settings: &Settings, config: &Config) -> Result<(), Refusal> {
    if config.paused {
        return refuse(format!(
            "program {} is paused, and it refuses every owner instruction but the unpause",
            settings.program_id
        ));
    }
    Ok(())
}

/// Initialise the program with the configured owner, then record the attestor
/// set and threshold; on a program already initialised for this owner, bring
/// the attestor set to the configured one. The fee payer must be the program's
/// upgrade authority, the only key the program lets initialise it.
fn initialise_config(
    cluster: &Cluster,
    settings: &Settings,
    payer: &Keypair,
    owner: &Keypair,
) -> Result<(), Refusal> {
    let program_id = &settings.program_id;
    match cluster.config_record(program_id)? {
        None => {
            require_upgrade_authority(cluster, settings, payer)?;
            let instructions = vec![
                initialise_instruction(program_id, &payer.pubkey(), &settings.owner),
                set_attestors_instruction(
                    program_id,
                    &settings.owner,
                    &settings.attestors,
                    settings.threshold,
                )?,
            ];
            cluster.submit("initialise", &instructions, payer, &[owner])?;
        }
        Some(config) => {
            require_configured_owner(settings, &config)?;
            if config.attestors == settings.attestors && config.threshold == settings.threshold {
                println!(
                    "program {program_id} is initialised for owner {} with the configured attestor set",
                    config.owner
                );
                return Ok(());
            }
            require_unpaused(settings, &config)?;
            let instruction = set_attestors_instruction(
                program_id,
                &settings.owner,
                &settings.attestors,
                settings.threshold,
            )?;
            cluster.submit("set-attestors", &[instruction], payer, &[owner])?;
        }
    }
    Ok(())
}

fn require_upgrade_authority(
    cluster: &Cluster,
    settings: &Settings,
    payer: &Keypair,
) -> Result<(), Refusal> {
    let address = program_data_address(&settings.program_id);
    let account = cluster.account(&address)?.ok_or_else(|| {
        settings.refusal(
            "solana.program_id",
            format!(
                "{} has no program data account {address}; it is not an upgradeable program on this cluster",
                settings.program_id
            ),
        )
    })?;
    let metadata = UpgradeableLoaderState::size_of_programdata_metadata();
    let state = if account.owner == bpf_loader_upgradeable::id() {
        account
            .data
            .get(..metadata)
            .and_then(|bytes| bincode::deserialize::<UpgradeableLoaderState>(bytes).ok())
    } else {
        None
    };
    let authority = match state {
        Some(UpgradeableLoaderState::ProgramData {
            upgrade_authority_address,
            ..
        }) => upgrade_authority_address,
        _ => {
            return refuse(format!(
                "{address} is not the program data account of {}",
                settings.program_id
            ))
        }
    };
    if authority != Some(payer.pubkey()) {
        return refuse(format!(
            "the upgrade authority of {} is {}, and {} holds {}; only the upgrade authority initialises the program",
            settings.program_id,
            authority.map_or_else(|| "removed".to_string(), |key| key.to_string()),
            settings.deploy_key_variable,
            payer.pubkey()
        ));
    }
    Ok(())
}

/// Check that the mint on the cluster is an SPL Token mint with the configured
/// decimals, which the program records at registration.
fn require_mint(cluster: &Cluster, settings: &Settings, asset: &AssetPlan) -> Result<(), Refusal> {
    let field = format!("assets[{}].address", asset.index);
    let account = cluster.account(&asset.mint)?.ok_or_else(|| {
        settings.refusal(&field, format!("the mint {} does not exist", asset.mint))
    })?;
    if account.owner != spl_token::id() {
        return Err(settings.refusal(&field, format!("{} is not an SPL Token mint", asset.mint)));
    }
    let mint = Mint::unpack(&account.data).map_err(|_| {
        settings.refusal(&field, format!("{} is not an SPL Token mint", asset.mint))
    })?;
    if mint.decimals != asset.decimals {
        return Err(settings.refusal(
            &format!("assets[{}].decimals", asset.index),
            format!(
                "the mint {} carries {} decimals on the cluster, not {}",
                asset.mint, mint.decimals, asset.decimals
            ),
        ));
    }
    Ok(())
}

/// Register each asset that is not registered yet, in configuration order, so
/// the native wrapped SOL mint is always first. An asset already registered
/// under another id is refused rather than left looking configured.
fn register_assets(
    cluster: &Cluster,
    settings: &Settings,
    payer: &Keypair,
    owner: &Keypair,
    only: Option<&Pubkey>,
) -> Result<(), Refusal> {
    let program_id = &settings.program_id;
    let config = cluster
        .config_record(program_id)?
        .ok_or_else(|| Refusal(format!("program {program_id} is not initialised")))?;
    require_configured_owner(settings, &config)?;
    for asset in settings
        .assets
        .iter()
        .filter(|asset| only.is_none_or(|mint| &asset.mint == mint))
    {
        match cluster.asset_record(program_id, &asset.mint)? {
            None => {
                require_unpaused(settings, &config)?;
                require_mint(cluster, settings, asset)?;
                let instruction = register_asset_instruction(program_id, &settings.owner, asset);
                cluster.submit(
                    &format!("register-asset {}", asset.symbol),
                    &[instruction],
                    payer,
                    &[owner],
                )?;
            }
            Some(record) => {
                if record.asset_id != asset.asset_id {
                    return Err(settings.refusal(
                        &format!("assets[{}].asset_id", asset.index),
                        format!(
                            "{} is registered with id 0x{}, and the configuration names 0x{}",
                            asset.mint,
                            hex(&record.asset_id),
                            hex(&asset.asset_id)
                        ),
                    ));
                }
                println!("asset {} ({}) is registered", asset.symbol, asset.mint);
            }
        }
    }
    Ok(())
}

/// Bring every configured asset's per-transaction and total caps to the
/// configured values and open it.
fn set_caps(
    cluster: &Cluster,
    settings: &Settings,
    payer: &Keypair,
    owner: &Keypair,
    only: Option<&Pubkey>,
) -> Result<(), Refusal> {
    let program_id = &settings.program_id;
    let config = cluster
        .config_record(program_id)?
        .ok_or_else(|| Refusal(format!("program {program_id} is not initialised")))?;
    require_configured_owner(settings, &config)?;
    for asset in settings
        .assets
        .iter()
        .filter(|asset| only.is_none_or(|mint| &asset.mint == mint))
    {
        let record = cluster
            .asset_record(program_id, &asset.mint)?
            .ok_or_else(|| {
                settings.refusal(
                    &format!("assets[{}].address", asset.index),
                    format!("{} is not registered with program {program_id}", asset.mint),
                )
            })?;
        if record.per_tx_cap == asset.per_tx_cap
            && record.total_cap == asset.total_cap
            && record.enabled
        {
            println!(
                "asset {} caps are {} per transaction and {} in total",
                asset.symbol, asset.per_tx_cap, asset.total_cap
            );
            continue;
        }
        require_unpaused(settings, &config)?;
        if asset.total_cap < record.outstanding {
            return Err(settings.refusal(
                &format!("assets[{}].total_cap", asset.index),
                format!(
                    "{} is below the {} the program already holds",
                    asset.total_cap, record.outstanding
                ),
            ));
        }
        let instruction = set_cap_instruction(program_id, &settings.owner, asset);
        cluster.submit(
            &format!("set-cap {}", asset.symbol),
            &[instruction],
            payer,
            &[owner],
        )?;
    }
    Ok(())
}

fn set_pause(
    cluster: &Cluster,
    settings: &Settings,
    payer: &Keypair,
    owner: &Keypair,
    paused: bool,
) -> Result<(), Refusal> {
    let program_id = &settings.program_id;
    let config = cluster
        .config_record(program_id)?
        .ok_or_else(|| Refusal(format!("program {program_id} is not initialised")))?;
    require_configured_owner(settings, &config)?;
    if config.paused == paused {
        println!("program {program_id} paused={paused}");
        return Ok(());
    }
    let instruction = set_pause_instruction(program_id, &settings.owner, paused);
    let label = if paused { "pause" } else { "unpause" };
    cluster.submit(label, &[instruction], payer, &[owner])?;
    Ok(())
}

fn register_recipient(
    cluster: &Cluster,
    settings: &Settings,
    payer: &Keypair,
    registrant: &Keypair,
) -> Result<(), Refusal> {
    let program_id = &settings.program_id;
    let handle = pubkey_handle(&registrant.pubkey());
    let address = find_recipient_address(program_id, &handle).0;
    if let Some(account) = cluster.account(&address)? {
        if &account.owner == program_id {
            let record = RecipientRecord::decode(&account.data)
                .map_err(|_| Refusal(format!("{address} is not a recipient record")))?;
            if record.key != registrant.pubkey() {
                return refuse(format!(
                    "handle 0x{} is already registered to {}",
                    hex(&handle),
                    record.key
                ));
            }
            println!(
                "recipient {} is registered under handle 0x{}",
                record.key,
                hex(&handle)
            );
            return Ok(());
        }
    }
    let instruction =
        register_recipient_instruction(program_id, &payer.pubkey(), &registrant.pubkey());
    cluster.submit("register-recipient", &[instruction], payer, &[registrant])?;
    println!(
        "recipient {} registered under handle 0x{} at {address}",
        registrant.pubkey(),
        hex(&handle)
    );
    Ok(())
}

/// Read the config and every configured asset back and print them.
fn print_state(cluster: &Cluster, settings: &Settings) -> Result<(), Refusal> {
    let program_id = &settings.program_id;
    let (vault, _) = find_vault_authority(program_id);
    println!("state program {program_id}");
    println!("  config {}", find_config_address(program_id).0);
    match cluster.config_record(program_id)? {
        None => println!("  initialised false"),
        Some(config) => {
            println!("  initialised true");
            println!("  owner {}", config.owner);
            if config.pending_owner == Pubkey::default() {
                println!("  pending owner none");
            } else {
                println!("  pending owner {}", config.pending_owner);
            }
            println!("  paused {}", config.paused);
            println!(
                "  threshold {} of {}",
                config.threshold,
                config.attestors.len()
            );
            for attestor in &config.attestors {
                println!("  attestor 0x{}", hex(attestor));
            }
            println!("  deposit nonce {}", config.deposit_nonce);
        }
    }
    println!(
        "  vault authority {vault} handle 0x{}",
        hex(&pubkey_handle(&vault))
    );
    for asset in &settings.assets {
        let address = find_asset_address(program_id, &asset.mint).0;
        match cluster.asset_record(program_id, &asset.mint)? {
            None => println!(
                "  asset {} mint {} pda {address} registered false",
                asset.symbol, asset.mint
            ),
            Some(record) => println!(
                "  asset {} mint {} pda {address} registered true asset_id 0x{} decimals {} per_tx_cap {} total_cap {} outstanding {} enabled {}",
                asset.symbol,
                record.mint,
                hex(&record.asset_id),
                record.decimals,
                record.per_tx_cap,
                record.total_cap,
                record.outstanding,
                record.enabled
            ),
        }
    }
    Ok(())
}

fn run(arguments: &[String]) -> Result<(), Refusal> {
    let invocation = parse_arguments(arguments)?;
    let path = configuration_path(&invocation);
    let settings = load_configuration(&path)?;
    println!("configuration {}", path.display());
    let rpc = required_variable(&settings.rpc_variable)?;
    require_agreement(&settings, &invocation, &rpc)?;
    let mint = invocation
        .option("--mint")
        .map(|mint| {
            Pubkey::from_str(mint)
                .map_err(|_| Refusal(format!("--mint {mint} is not a base58 Solana key")))
        })
        .transpose()?;

    let cluster = Cluster::new(rpc, settings.commitment);
    let owner_keys = || -> Result<(Keypair, Keypair), Refusal> {
        Ok((
            keypair_from(&settings.deploy_key_variable)?,
            owner_keypair(&settings)?,
        ))
    };
    match invocation.command {
        Command::Show => print_state(&cluster, &settings),
        Command::RegisterRecipient => {
            let payer = keypair_from(&settings.deploy_key_variable)?;
            let registrant = keypair_from(RECIPIENT_KEY_VARIABLE)?;
            register_recipient(&cluster, &settings, &payer, &registrant)
        }
        Command::Apply => {
            let (payer, owner) = owner_keys()?;
            initialise_config(&cluster, &settings, &payer, &owner)?;
            register_assets(&cluster, &settings, &payer, &owner, None)?;
            set_caps(&cluster, &settings, &payer, &owner, None)?;
            print_state(&cluster, &settings)
        }
        Command::Initialise => {
            let (payer, owner) = owner_keys()?;
            initialise_config(&cluster, &settings, &payer, &owner)?;
            print_state(&cluster, &settings)
        }
        Command::RegisterAsset => {
            let mint = mint.ok_or_else(|| Refusal("register-asset needs --mint".into()))?;
            let (payer, owner) = owner_keys()?;
            register_assets(&cluster, &settings, &payer, &owner, Some(&mint))?;
            set_caps(&cluster, &settings, &payer, &owner, Some(&mint))
        }
        Command::SetCap => {
            let (payer, owner) = owner_keys()?;
            set_caps(&cluster, &settings, &payer, &owner, mint.as_ref())
        }
        Command::Pause => {
            let (payer, owner) = owner_keys()?;
            set_pause(&cluster, &settings, &payer, &owner, true)
        }
        Command::Unpause => {
            let (payer, owner) = owner_keys()?;
            set_pause(&cluster, &settings, &payer, &owner, false)
        }
    }
}

/// A failed transaction, with the program's rule named when the program's own
/// refusal code is what failed it.
fn describe_failure(failure: &TransactionError) -> String {
    const RULES: [(BridgeError, &str); 13] = [
        (BridgeError::Instruction, "Instruction"),
        (BridgeError::Authority, "Authority"),
        (BridgeError::Pda, "Pda"),
        (BridgeError::Conflict, "Conflict"),
        (BridgeError::NotInitialised, "NotInitialised"),
        (BridgeError::Paused, "Paused"),
        (BridgeError::Bounds, "Bounds"),
        (BridgeError::Attestors, "Attestors"),
        (BridgeError::Asset, "Asset"),
        (BridgeError::Cap, "Cap"),
        (BridgeError::Recipient, "Recipient"),
        (BridgeError::Account, "Account"),
        (BridgeError::Binding, "Binding"),
    ];
    if let TransactionError::InstructionError(_, InstructionError::Custom(code)) = failure {
        if let Some((_, rule)) = RULES.iter().find(|(error, _)| *error as u32 == *code) {
            return format!("{failure} (the program's {rule} rule)");
        }
    }
    failure.to_string()
}

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments.is_empty() || arguments[0] == "--help" || arguments[0] == "help" {
        eprintln!("{USAGE}");
        return if arguments.is_empty() {
            ExitCode::from(2)
        } else {
            ExitCode::SUCCESS
        };
    }
    match run(&arguments) {
        Ok(()) => ExitCode::SUCCESS,
        Err(refusal) => {
            eprintln!("paxeer-x-bridge-solana-admin: error: {refusal}");
            ExitCode::FAILURE
        }
    }
}

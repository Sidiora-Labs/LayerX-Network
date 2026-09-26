//! The admin client against the real custody program.
//!
//! Every test runs the built client binary as the operator would, with the
//! configuration file and the environment variables it names, against the
//! real program in the real Solana runtime that `solana-program-test` starts.
//! The client speaks JSON-RPC over HTTP; the runtime is reached through a
//! loopback JSON-RPC endpoint in this file that answers the four methods the
//! client calls from the runtime's own banks: getAccountInfo from the bank's
//! account, getLatestBlockhash from the bank's blockhash, sendTransaction by
//! processing the transaction through the real program and rooting its slot,
//! and getSignatureStatuses from the bank's status. Nothing here dials beyond
//! the loopback interface, and every key is generated for the test run.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::Output;
use std::str::FromStr;
use std::sync::Arc;

use base64::Engine;
use paxeer_x_bridge_solana_program::identity::{
    find_vault_authority, hex, pubkey_handle, HANDLE_BYTES, SIDIORA_ASSET_ID, SIDIORA_MINT,
};
use paxeer_x_bridge_solana_program::process_instruction;
use paxeer_x_bridge_solana_program::state::{
    find_asset_address, find_config_address, find_recipient_address, Asset, Config, RecipientRecord,
};
use serde_json::{json, Value};
use solana_banks_interface::TransactionConfirmationStatus;
use solana_loader_v3_interface::state::UpgradeableLoaderState;
use solana_program::program_option::COption;
use solana_program::program_pack::Pack;
use solana_program::rent::Rent;
use solana_program_test::{processor, ProgramTest, ProgramTestContext};
use solana_sdk::account::{Account, AccountSharedData};
use solana_sdk::commitment_config::CommitmentLevel;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signature, Signer};
use solana_sdk::signer::keypair::write_keypair_file;
use solana_sdk::transaction::{Transaction, VersionedTransaction};
use solana_sdk_ids::bpf_loader_upgradeable;
use solana_system_interface::instruction as system_instruction;
use spl_token::state::Mint;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

const BINARY: &str = env!("CARGO_BIN_EXE_paxeer-x-bridge-solana-admin");
const COMMITTED_CONFIGURATION: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../chains/solana/config.json");
const RPC_VARIABLE: &str = "PAXEER_BRIDGE_SOLANA_RPC_URL";
const DEPLOY_KEY_VARIABLE: &str = "PAXEER_BRIDGE_SOLANA_KEYPAIR_FILE";
const OWNER_KEY_VARIABLE: &str = "PAXEER_BRIDGE_SOLANA_OWNER_KEYPAIR_FILE";
const RECIPIENT_KEY_VARIABLE: &str = "PAXEER_BRIDGE_SOLANA_RECIPIENT_KEYPAIR_FILE";
const WRAPPED_SOL_HANDLE: &str = "0xcf996523b5d068a26f0aa8a116602fe5033ee3a1";
const FUNDING: u64 = 10_000_000_000;

/// Five attestor addresses in strictly ascending order.
fn attestors() -> Vec<String> {
    [0x11_u8, 0x22, 0x33, 0x44, 0x55]
        .iter()
        .map(|byte| format!("0x{}", hex(&[*byte; HANDLE_BYTES])))
        .collect()
}

fn attestor_bytes() -> Vec<[u8; HANDLE_BYTES]> {
    [0x11_u8, 0x22, 0x33, 0x44, 0x55]
        .iter()
        .map(|byte| [*byte; HANDLE_BYTES])
        .collect()
}

// ---------------------------------------------------------------------------
// The loopback JSON-RPC endpoint over the runtime's banks.
// ---------------------------------------------------------------------------

type Runtime = Arc<Mutex<ProgramTestContext>>;

/// One field of a configuration changed for a refusal case.
type ConfigurationChange = Box<dyn Fn(&mut Value)>;

async fn serve(listener: TcpListener, runtime: Runtime) {
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            return;
        };
        let runtime = runtime.clone();
        tokio::spawn(async move { connection(stream, runtime).await });
    }
}

/// Answer every request on one keep-alive connection until the client closes
/// it.
async fn connection(stream: TcpStream, runtime: Runtime) {
    let mut reader = BufReader::new(stream);
    loop {
        let mut length = None;
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line).await {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some((name, value)) = line.split_once(':') {
                if name.eq_ignore_ascii_case("content-length") {
                    length = value.trim().parse::<usize>().ok();
                }
            }
        }
        let Some(length) = length else {
            return;
        };
        let mut body = vec![0_u8; length];
        if reader.read_exact(&mut body).await.is_err() {
            return;
        }
        let request: Value = serde_json::from_slice(&body).expect("the client sends JSON");
        let response = answer(&request, &runtime).await;
        let bytes = serde_json::to_vec(&response).expect("a response serialises");
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
            bytes.len()
        );
        let stream = reader.get_mut();
        if stream.write_all(head.as_bytes()).await.is_err()
            || stream.write_all(&bytes).await.is_err()
        {
            return;
        }
    }
}

async fn answer(request: &Value, runtime: &Runtime) -> Value {
    let id = request["id"].clone();
    let params = &request["params"];
    let mut context = runtime.lock().await;
    let slot = context
        .banks_client
        .get_root_slot()
        .await
        .expect("the runtime answers its slot");
    let outcome: Result<Value, Value> = match request["method"].as_str() {
        Some("getAccountInfo") => {
            let key =
                Pubkey::from_str(params[0].as_str().expect("a pubkey")).expect("a base58 pubkey");
            assert_eq!(params[1]["encoding"], "base64", "the client reads base64");
            let account = context
                .banks_client
                .get_account_with_commitment(key, CommitmentLevel::Processed)
                .await
                .expect("the runtime answers an account lookup");
            Ok(json!({
                "context": {"slot": slot},
                "value": account.map(|account| json!({
                    "lamports": account.lamports,
                    "data": [base64::engine::general_purpose::STANDARD.encode(&account.data), "base64"],
                    "owner": account.owner.to_string(),
                    "executable": account.executable,
                    "rentEpoch": account.rent_epoch,
                    "space": account.data.len(),
                })),
            }))
        }
        Some("getLatestBlockhash") => {
            let (blockhash, height) = context
                .banks_client
                .get_latest_blockhash_with_commitment(CommitmentLevel::Processed)
                .await
                .expect("the runtime answers its blockhash")
                .expect("the runtime has a blockhash");
            Ok(json!({
                "context": {"slot": slot},
                "value": {"blockhash": blockhash.to_string(), "lastValidBlockHeight": height},
            }))
        }
        Some("sendTransaction") => {
            assert_eq!(params[1]["encoding"], "base64", "the client sends base64");
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(params[0].as_str().expect("an encoded transaction"))
                .expect("base64");
            let transaction: VersionedTransaction =
                bincode::deserialize(&bytes).expect("a serialised transaction");
            let signature = transaction.signatures[0];
            let processed = context
                .banks_client
                .process_transaction_with_metadata(transaction)
                .await
                .expect("the runtime processes the transaction");
            match processed.result {
                Ok(()) => {
                    // The cluster roots the slot the transaction landed in.
                    context
                        .warp_to_slot(slot + 1)
                        .expect("the runtime roots the next slot");
                    Ok(json!(signature.to_string()))
                }
                Err(error) => Err(json!({
                    "code": -32002,
                    "message": format!("Transaction simulation failed: {error}"),
                    "data": {
                        "err": serde_json::to_value(&error).expect("an error serialises"),
                        "logs": processed.metadata.map(|metadata| metadata.log_messages),
                    },
                })),
            }
        }
        Some("getSignatureStatuses") => {
            let mut statuses = Vec::new();
            for signature in params[0].as_array().expect("a signature list") {
                let signature = Signature::from_str(signature.as_str().expect("a signature"))
                    .expect("a base58 signature");
                let status = context
                    .banks_client
                    .get_transaction_status(signature)
                    .await
                    .expect("the runtime answers a status");
                statuses.push(status.map(|status| {
                    json!({
                        "slot": status.slot,
                        "confirmations": status.confirmations,
                        "status": match &status.err {
                            None => json!({"Ok": null}),
                            Some(error) => json!({"Err": error}),
                        },
                        "err": status.err,
                        "confirmationStatus": status.confirmation_status.map(|level| match level {
                            TransactionConfirmationStatus::Processed => "processed",
                            TransactionConfirmationStatus::Confirmed => "confirmed",
                            TransactionConfirmationStatus::Finalized => "finalized",
                        }),
                    })
                }));
            }
            Ok(json!({"context": {"slot": slot}, "value": statuses}))
        }
        other => panic!("the client called {other:?}, which it does not need"),
    };
    match outcome {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Err(error) => json!({"jsonrpc": "2.0", "id": id, "error": error}),
    }
}

// ---------------------------------------------------------------------------
// The harness: the runtime, the endpoint, the keys and the configuration.
// ---------------------------------------------------------------------------

struct Harness {
    runtime: Runtime,
    program: Pubkey,
    url: String,
    directory: tempfile::TempDir,
    deployer: Keypair,
    owner: Keypair,
}

impl Harness {
    async fn start() -> Self {
        Self::launch(6).await
    }

    /// A runtime holding the real program, the wrapped SOL mint and a mint at
    /// Sidiora's address with `sidiora_decimals`. No keypair exists for either
    /// mint address, so both are placed at genesis, packed by the SPL Token
    /// crate's own `Mint` type with no mint authority and owned by the SPL
    /// Token program the runtime loads.
    async fn launch(sidiora_decimals: u8) -> Self {
        let program = Pubkey::new_unique();
        let mut test = ProgramTest::new(
            "paxeer_x_bridge_solana_program",
            program,
            processor!(process_instruction),
        );
        for (mint, decimals) in [
            (spl_token::native_mint::id(), 9),
            (SIDIORA_MINT, sidiora_decimals),
        ] {
            let mut data = vec![0_u8; Mint::LEN];
            Mint {
                mint_authority: COption::None,
                supply: 0,
                decimals,
                is_initialized: true,
                freeze_authority: COption::None,
            }
            .pack_into_slice(&mut data);
            test.add_account(
                mint,
                Account {
                    lamports: Rent::default().minimum_balance(Mint::LEN),
                    data,
                    owner: spl_token::id(),
                    executable: false,
                    rent_epoch: 0,
                },
            );
        }
        let deployer = Keypair::new();
        let owner = Keypair::new();
        for key in [deployer.pubkey(), owner.pubkey()] {
            test.add_account(
                key,
                Account::new(FUNDING, 0, &solana_sdk_ids::system_program::id()),
            );
        }
        let context = test.start_with_context().await;
        let runtime = Arc::new(Mutex::new(context));
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .await
            .expect("the loopback interface accepts a listener");
        let url = format!(
            "http://{}",
            listener.local_addr().expect("the listener has an address")
        );
        tokio::spawn(serve(listener, runtime.clone()));
        let harness = Self {
            runtime,
            program,
            url,
            directory: tempfile::tempdir().expect("a scratch directory"),
            deployer,
            owner,
        };
        harness
            .set_upgrade_authority(Some(harness.deployer.pubkey()))
            .await;
        harness
    }

    /// Place this program's `ProgramData` account, serialised from the
    /// upgradeable loader's own type, naming `authority` as the upgrade
    /// authority. The runtime runs the program as a builtin and keeps no
    /// `ProgramData` of its own; a deployment's publisher is its authority.
    async fn set_upgrade_authority(&self, authority: Option<Pubkey>) {
        let state = UpgradeableLoaderState::ProgramData {
            slot: 0,
            upgrade_authority_address: authority,
        };
        let bytes = UpgradeableLoaderState::size_of_programdata_metadata();
        let account = Account::new_data_with_space(
            Rent::default().minimum_balance(bytes),
            &state,
            bytes,
            &bpf_loader_upgradeable::id(),
        )
        .expect("the loader state serialises");
        let address =
            Pubkey::find_program_address(&[self.program.as_ref()], &bpf_loader_upgradeable::id()).0;
        self.runtime
            .lock()
            .await
            .set_account(&address, &AccountSharedData::from(account));
    }

    fn key_file(&self, name: &str, key: &Keypair) -> PathBuf {
        let path = self.directory.path().join(name);
        write_keypair_file(key, &path).expect("the keypair file is written");
        path
    }

    /// The committed Solana configuration with its placeholders filled in for
    /// this run: the owner, the attestor set and the program id.
    fn configuration(&self) -> Value {
        let text = std::fs::read_to_string(COMMITTED_CONFIGURATION)
            .expect("the committed configuration is readable");
        let mut configuration: Value = serde_json::from_str(&text).expect("it is JSON");
        configuration["owner"] = json!(self.owner.pubkey().to_string());
        configuration["attestors"] = json!(attestors());
        configuration["solana"]["program_id"] = json!(self.program.to_string());
        configuration
    }

    fn write_configuration(&self, name: &str, configuration: &Value) -> PathBuf {
        let path = self.directory.path().join(name);
        std::fs::write(
            &path,
            serde_json::to_vec_pretty(configuration).expect("the configuration serialises"),
        )
        .expect("the configuration is written");
        path
    }

    /// The environment an operator runs the client with.
    fn environment(&self) -> Vec<(String, String)> {
        let deployer = self.key_file("deployer.json", &self.deployer);
        let owner = self.key_file("owner.json", &self.owner);
        vec![
            (RPC_VARIABLE.into(), self.url.clone()),
            (DEPLOY_KEY_VARIABLE.into(), path_string(&deployer)),
            (OWNER_KEY_VARIABLE.into(), path_string(&owner)),
        ]
    }

    async fn config_record(&self) -> Option<Config> {
        let key = find_config_address(&self.program).0;
        self.data(key)
            .await
            .map(|data| Config::decode(&data).expect("the config record is this layout"))
    }

    async fn asset_record(&self, mint: &Pubkey) -> Option<Asset> {
        let key = find_asset_address(&self.program, mint).0;
        self.data(key)
            .await
            .map(|data| Asset::decode(&data).expect("the asset record is this layout"))
    }

    async fn data(&self, key: Pubkey) -> Option<Vec<u8>> {
        self.runtime
            .lock()
            .await
            .banks_client
            .get_account_with_commitment(key, CommitmentLevel::Processed)
            .await
            .expect("the runtime answers an account lookup")
            .map(|account| account.data)
    }

    async fn transfer(&self, to: &Pubkey, lamports: u64) {
        let context = self.runtime.lock().await;
        let blockhash = context
            .banks_client
            .get_latest_blockhash()
            .await
            .expect("the runtime has a blockhash");
        let payer = context.payer.insecure_clone();
        let transaction = Transaction::new_signed_with_payer(
            &[system_instruction::transfer(&payer.pubkey(), to, lamports)],
            Some(&payer.pubkey()),
            &[&payer],
            blockhash,
        );
        context
            .banks_client
            .process_transaction(transaction)
            .await
            .expect("the transfer lands");
    }
}

fn path_string(path: &Path) -> String {
    path.to_str()
        .expect("the scratch path is UTF-8")
        .to_string()
}

/// Run the client binary with exactly `environment`, nothing inherited.
async fn run(arguments: &[&str], environment: &[(String, String)]) -> Output {
    let arguments: Vec<String> = arguments.iter().map(|value| value.to_string()).collect();
    let environment = environment.to_vec();
    tokio::task::spawn_blocking(move || {
        std::process::Command::new(BINARY)
            .args(&arguments)
            .env_clear()
            .envs(environment)
            .output()
            .expect("the client binary runs")
    })
    .await
    .expect("the client run completes")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Whether a run printed a transaction, which it does before sending one.
fn sent_a_transaction(printed: &str) -> bool {
    printed.lines().any(|line| line.starts_with("transaction "))
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn accepted(output: &Output, what: &str) -> String {
    assert!(
        output.status.success(),
        "{what} was refused:\n{}\n{}",
        stdout(output),
        stderr(output)
    );
    stdout(output)
}

fn refused(output: &Output, what: &str, expected: &[&str]) -> String {
    assert!(
        !output.status.success(),
        "{what} was accepted:\n{}",
        stdout(output)
    );
    let message = stderr(output);
    for fragment in expected {
        assert!(
            message.contains(fragment),
            "{what} was refused without naming {fragment:?}:\n{message}"
        );
    }
    message
}

// ---------------------------------------------------------------------------
// The tests.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn apply_initialises_registers_both_assets_sets_the_caps_and_prints_the_state() {
    let harness = Harness::start().await;
    let configuration = harness.write_configuration("config.json", &harness.configuration());
    let config_path = path_string(&configuration);
    let environment = harness.environment();

    let output = run(&["apply", "--config", &config_path], &environment).await;
    let printed = accepted(&output, "apply");

    let config = harness
        .config_record()
        .await
        .expect("the program is initialised");
    assert_eq!(config.owner, harness.owner.pubkey());
    assert_eq!(config.pending_owner, Pubkey::default());
    assert!(!config.paused, "a fresh program is not paused");
    assert_eq!(config.attestors, attestor_bytes());
    assert_eq!(config.threshold, 3);

    let sol = harness
        .asset_record(&spl_token::native_mint::id())
        .await
        .expect("wrapped SOL is registered");
    assert_eq!(format!("0x{}", hex(&sol.asset_id)), WRAPPED_SOL_HANDLE);
    assert_eq!(sol.asset_id, pubkey_handle(&spl_token::native_mint::id()));
    assert_eq!(sol.decimals, 9);
    assert_eq!(sol.per_tx_cap, 5_000_000_000_000);
    assert_eq!(sol.total_cap, 100_000_000_000_000);
    assert!(sol.enabled);
    let sid = harness
        .asset_record(&SIDIORA_MINT)
        .await
        .expect("Sidiora is registered");
    assert_eq!(sid.asset_id, SIDIORA_ASSET_ID);
    assert_eq!(sid.decimals, 6);
    assert_eq!(sid.per_tx_cap, 1_000_000_000_000);
    assert_eq!(sid.total_cap, 50_000_000_000_000);
    assert!(sid.enabled);

    // Every transaction is printed before it is confirmed, native SOL first.
    let initialise = printed
        .find("transaction initialise")
        .expect("initialise is printed");
    let confirmed = printed
        .find("confirmed initialise ")
        .expect("initialise is confirmed");
    assert!(initialise < confirmed);
    let register_sol = printed
        .find("transaction register-asset SOL")
        .expect("wrapped SOL's registration is printed");
    let register_sid = printed
        .find("transaction register-asset SID")
        .expect("Sidiora's registration is printed");
    assert!(initialise < register_sol && register_sol < register_sid);
    assert!(printed.contains(&format!("  fee payer {}", harness.deployer.pubkey())));
    assert!(printed.contains("  base64 "));
    assert!(printed.contains(&format!("    account {} signer", harness.owner.pubkey())));
    assert!(
        !printed.contains(&harness.url),
        "the endpoint is never printed"
    );

    // The state read back from the program is printed after the transactions.
    let state = printed
        .find(&format!("state program {}", harness.program))
        .expect("the state is printed");
    assert!(state > register_sid);
    let tail = &printed[state..];
    assert!(tail.contains(&format!("  owner {}", harness.owner.pubkey())));
    assert!(tail.contains("  paused false"));
    assert!(tail.contains("  threshold 3 of 5"));
    for attestor in attestors() {
        assert!(tail.contains(&format!("  attestor {attestor}")));
    }
    let vault = find_vault_authority(&harness.program).0;
    assert!(tail.contains(&format!(
        "  vault authority {vault} handle 0x{}",
        hex(&pubkey_handle(&vault))
    )));
    assert!(tail.contains(&format!(
        "asset SOL mint {} pda {} registered true asset_id {WRAPPED_SOL_HANDLE} decimals 9 per_tx_cap 5000000000000 total_cap 100000000000000 outstanding 0 enabled true",
        spl_token::native_mint::id(),
        find_asset_address(&harness.program, &spl_token::native_mint::id()).0
    )));
    assert!(tail.contains(&format!(
        "asset SID mint {SIDIORA_MINT} pda {} registered true asset_id 0x{} decimals 6 per_tx_cap 1000000000000 total_cap 50000000000000 outstanding 0 enabled true",
        find_asset_address(&harness.program, &SIDIORA_MINT).0,
        hex(&SIDIORA_ASSET_ID)
    )));

    // A second run finds everything as configured and sends nothing.
    let again = run(&["apply", "--config", &config_path], &environment).await;
    let printed = accepted(&again, "a second apply");
    assert!(
        !sent_a_transaction(&printed),
        "a configured program needs no transaction:\n{printed}"
    );
    assert!(printed.contains("with the configured attestor set"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_deploy_script_invocation_initialises_and_registers_in_order() {
    let harness = Harness::start().await;
    let configuration = harness.configuration();
    let config_path = path_string(&harness.write_configuration("config.json", &configuration));
    let environment = harness.environment();
    let keypair = environment[1].1.clone();
    let program = harness.program.to_string();
    let owner = harness.owner.pubkey().to_string();
    let attestor_list = attestors().join(",");
    let common = [
        "--config",
        config_path.as_str(),
        "--url",
        harness.url.as_str(),
        "--keypair",
        keypair.as_str(),
        "--program-id",
        program.as_str(),
        "--commitment",
        "finalized",
    ];

    // A flag that disagrees with the configuration is refused before anything
    // is sent.
    let mut disagreeing = vec!["initialise"];
    disagreeing.extend_from_slice(&common);
    disagreeing.extend_from_slice(&[
        "--owner",
        owner.as_str(),
        "--attestors",
        attestor_list.as_str(),
        "--threshold",
        "2",
    ]);
    let output = run(&disagreeing, &environment).await;
    refused(
        &output,
        "a disagreeing threshold",
        &["threshold", "--threshold 2"],
    );
    assert!(harness.config_record().await.is_none());

    let mut initialise = vec!["initialise"];
    initialise.extend_from_slice(&common);
    initialise.extend_from_slice(&[
        "--owner",
        owner.as_str(),
        "--attestors",
        attestor_list.as_str(),
        "--threshold",
        "3",
    ]);
    let output = run(&initialise, &environment).await;
    accepted(&output, "the script's initialise");
    let config = harness
        .config_record()
        .await
        .expect("the program is initialised");
    assert_eq!(config.owner, harness.owner.pubkey());
    assert_eq!(config.attestors, attestor_bytes());
    assert_eq!(config.threshold, 3);

    for asset in configuration["assets"].as_array().expect("an asset list") {
        let mint = asset["address"].as_str().expect("a mint");
        let asset_id = asset["asset_id"].as_str().expect("an id");
        let decimals = asset["decimals"].to_string();
        let per_tx = asset["per_tx_cap"].as_str().expect("a cap");
        let total = asset["total_cap"].as_str().expect("a cap");
        let mut register = vec!["register-asset"];
        register.extend_from_slice(&common);
        register.extend_from_slice(&[
            "--mint",
            mint,
            "--asset-id",
            asset_id,
            "--decimals",
            decimals.as_str(),
            "--per-tx-cap",
            per_tx,
            "--total-cap",
            total,
        ]);
        let output = run(&register, &environment).await;
        accepted(&output, "the script's register-asset");
        let record = harness
            .asset_record(&Pubkey::from_str(mint).expect("a base58 mint"))
            .await
            .expect("the asset is registered");
        assert_eq!(
            format!("0x{}", hex(&record.asset_id)),
            asset_id.to_lowercase()
        );
        assert_eq!(record.per_tx_cap.to_string(), per_tx);
        assert_eq!(record.total_cap.to_string(), total);
    }

    // An asset id that disagrees with the configuration is refused by name.
    let mut wrong_id = vec!["register-asset"];
    wrong_id.extend_from_slice(&common);
    let sid = SIDIORA_MINT.to_string();
    wrong_id.extend_from_slice(&["--mint", sid.as_str(), "--asset-id", WRAPPED_SOL_HANDLE]);
    let output = run(&wrong_id, &environment).await;
    refused(&output, "a disagreeing asset id", &["assets[1].asset_id"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn set_cap_pause_and_unpause_follow_the_configuration() {
    let harness = Harness::start().await;
    let mut configuration = harness.configuration();
    let config_path = path_string(&harness.write_configuration("config.json", &configuration));
    let environment = harness.environment();
    accepted(
        &run(&["apply", "--config", &config_path], &environment).await,
        "apply",
    );

    configuration["assets"][1]["per_tx_cap"] = json!("2000000000000");
    configuration["assets"][1]["total_cap"] = json!("60000000000000");
    let raised = path_string(&harness.write_configuration("raised.json", &configuration));
    let output = run(&["set-cap", "--config", &raised], &environment).await;
    let printed = accepted(&output, "set-cap");
    assert!(printed.contains("transaction set-cap SID"));
    assert!(
        !printed.contains("transaction set-cap SOL"),
        "wrapped SOL's caps are unchanged"
    );
    let sid = harness
        .asset_record(&SIDIORA_MINT)
        .await
        .expect("registered");
    assert_eq!(sid.per_tx_cap, 2_000_000_000_000);
    assert_eq!(sid.total_cap, 60_000_000_000_000);
    assert!(sid.enabled);

    let output = run(&["pause", "--config", &raised], &environment).await;
    assert!(accepted(&output, "pause").contains("transaction pause"));
    assert!(harness.config_record().await.expect("initialised").paused);

    // A paused program refuses the owner's instructions, so the client stops
    // before sending one.
    configuration["assets"][1]["per_tx_cap"] = json!("3000000000000");
    let paused = path_string(&harness.write_configuration("paused.json", &configuration));
    let output = run(&["set-cap", "--config", &paused], &environment).await;
    refused(&output, "set-cap while paused", &["is paused"]);
    let sid = harness
        .asset_record(&SIDIORA_MINT)
        .await
        .expect("registered");
    assert_eq!(sid.per_tx_cap, 2_000_000_000_000);

    let output = run(&["unpause", "--config", &raised], &environment).await;
    assert!(accepted(&output, "unpause").contains("transaction unpause"));
    assert!(!harness.config_record().await.expect("initialised").paused);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_recipient_registers_its_own_key_under_its_handle() {
    let harness = Harness::start().await;
    let config_path =
        path_string(&harness.write_configuration("config.json", &harness.configuration()));
    let mut environment = harness.environment();
    accepted(
        &run(&["apply", "--config", &config_path], &environment).await,
        "apply",
    );

    let recipient = Keypair::new();
    harness.transfer(&recipient.pubkey(), 1_000_000).await;
    let key_file = harness.key_file("recipient.json", &recipient);
    environment.push((RECIPIENT_KEY_VARIABLE.into(), path_string(&key_file)));
    let output = run(
        &["register-recipient", "--config", &config_path],
        &environment,
    )
    .await;
    let printed = accepted(&output, "register-recipient");
    assert!(printed.contains("transaction register-recipient"));

    let handle = pubkey_handle(&recipient.pubkey());
    let address = find_recipient_address(&harness.program, &handle).0;
    let record = RecipientRecord::decode(
        &harness
            .data(address)
            .await
            .expect("the recipient record exists"),
    )
    .expect("the recipient record is this layout");
    assert_eq!(record.handle, handle);
    assert_eq!(record.key, recipient.pubkey());
    assert!(printed.contains(&format!("handle 0x{}", hex(&handle))));

    // A second registration finds the record and sends nothing.
    let output = run(
        &["register-recipient", "--config", &config_path],
        &environment,
    )
    .await;
    let printed = accepted(&output, "a second register-recipient");
    assert!(!sent_a_transaction(&printed));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn placeholders_and_values_the_program_refuses_are_refused_by_field() {
    let harness = Harness::start().await;
    let environment = harness.environment();

    // The committed configuration still carries its placeholders.
    let output = run(
        &["apply", "--config", COMMITTED_CONFIGURATION],
        &environment,
    )
    .await;
    refused(
        &output,
        "the committed configuration",
        &[
            "bridge/solana/admin/../chains/solana/config.json: owner:",
            "PLACEHOLDER:owner",
            "placeholder",
        ],
    );

    let cases: Vec<(&str, ConfigurationChange, &[&str])> = vec![
        (
            "a placeholder attestor",
            Box::new(|value: &mut Value| value["attestors"][2] = json!("PLACEHOLDER:attestor-3")),
            &["attestors[2]", "placeholder"],
        ),
        (
            "a placeholder program id",
            Box::new(|value: &mut Value| {
                value["solana"]["program_id"] = json!("PLACEHOLDER:program-id")
            }),
            &["solana.program_id", "placeholder"],
        ),
        (
            "a descending attestor set",
            Box::new(|value: &mut Value| {
                value["attestors"][1] = json!(format!("0x{}", hex(&[0x05; HANDLE_BYTES])))
            }),
            &["attestors[1]", "strictly ascending"],
        ),
        (
            "a threshold above the set",
            Box::new(|value: &mut Value| value["threshold"] = json!(6)),
            &["threshold"],
        ),
        (
            "wrapped SOL under an id that is not its handle",
            Box::new(|value: &mut Value| {
                value["assets"][0]["asset_id"] = json!(format!("0x{}", hex(&[0x5a; HANDLE_BYTES])))
            }),
            &["assets[0].asset_id", WRAPPED_SOL_HANDLE],
        ),
        (
            "Sidiora under its derived handle",
            Box::new(|value: &mut Value| {
                value["assets"][1]["asset_id"] =
                    json!(format!("0x{}", hex(&pubkey_handle(&SIDIORA_MINT))))
            }),
            &["assets[1].asset_id", "Sidiora"],
        ),
        (
            "a per-transaction cap above the total cap",
            Box::new(|value: &mut Value| {
                value["assets"][1]["per_tx_cap"] = json!("60000000000000")
            }),
            &["assets[1].per_tx_cap"],
        ),
        (
            "a cap beyond the program's 64 bits",
            Box::new(|value: &mut Value| {
                value["assets"][0]["total_cap"] = json!("18446744073709551616")
            }),
            &["assets[0].total_cap", "64-bit"],
        ),
        (
            "an unknown field",
            Box::new(|value: &mut Value| value["endpoint"] = json!("unused")),
            &["unknown field", "endpoint"],
        ),
    ];
    for (index, (what, change, expected)) in cases.iter().enumerate() {
        let mut configuration = harness.configuration();
        change(&mut configuration);
        let path = path_string(
            &harness.write_configuration(&format!("case-{index}.json"), &configuration),
        );
        let output = run(&["apply", "--config", &path], &environment).await;
        let message = refused(&output, what, expected);
        assert!(
            message.contains(&path),
            "{what} was refused without naming the file"
        );
        assert!(
            !sent_a_transaction(&stdout(&output)),
            "{what} sent a transaction"
        );
    }

    // A mint whose on-chain decimals disagree with the configuration.
    let mismatched = Harness::launch(9).await;
    let path =
        path_string(&mismatched.write_configuration("config.json", &mismatched.configuration()));
    let output = run(&["apply", "--config", &path], &mismatched.environment()).await;
    refused(
        &output,
        "Sidiora with nine decimals on the cluster",
        &["assets[1].decimals", "carries 9 decimals"],
    );
    assert!(mismatched.asset_record(&SIDIORA_MINT).await.is_none());

    assert!(
        harness.config_record().await.is_none(),
        "nothing was initialised"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_missing_variable_or_an_unreadable_key_file_is_refused_by_name() {
    let harness = Harness::start().await;
    let config_path =
        path_string(&harness.write_configuration("config.json", &harness.configuration()));
    let environment = harness.environment();

    for missing in [RPC_VARIABLE, DEPLOY_KEY_VARIABLE, OWNER_KEY_VARIABLE] {
        let reduced: Vec<(String, String)> = environment
            .iter()
            .filter(|(name, _)| name != missing)
            .cloned()
            .collect();
        let output = run(&["apply", "--config", &config_path], &reduced).await;
        let expected = format!("{missing} is required and is not set");
        refused(
            &output,
            &format!("a run without {missing}"),
            &[expected.as_str()],
        );
    }

    let mut unreadable = environment.clone();
    unreadable[2].1 = path_string(&harness.directory.path().join("absent.json"));
    let output = run(&["apply", "--config", &config_path], &unreadable).await;
    refused(
        &output,
        "an absent owner key file",
        &[OWNER_KEY_VARIABLE, "not a readable Solana keypair file"],
    );

    let stranger = harness.key_file("stranger.json", &Keypair::new());
    let mut wrong_owner = environment.clone();
    wrong_owner[2].1 = path_string(&stranger);
    let output = run(&["apply", "--config", &config_path], &wrong_owner).await;
    refused(
        &output,
        "an owner key that is not the configured owner",
        &["owner", OWNER_KEY_VARIABLE],
    );

    let output = run(
        &["register-recipient", "--config", &config_path],
        &environment,
    )
    .await;
    refused(
        &output,
        "a registration without a recipient key",
        &[RECIPIENT_KEY_VARIABLE],
    );

    assert!(
        harness.config_record().await.is_none(),
        "nothing was initialised"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_program_initialised_for_another_owner_or_by_another_authority_is_refused() {
    let harness = Harness::start().await;
    let config_path =
        path_string(&harness.write_configuration("config.json", &harness.configuration()));
    let environment = harness.environment();

    // Only the upgrade authority initialises the program.
    let other_authority = Keypair::new();
    harness
        .set_upgrade_authority(Some(other_authority.pubkey()))
        .await;
    let output = run(&["initialise", "--config", &config_path], &environment).await;
    refused(
        &output,
        "a fee payer that is not the upgrade authority",
        &["upgrade authority", DEPLOY_KEY_VARIABLE],
    );
    assert!(harness.config_record().await.is_none());
    harness
        .set_upgrade_authority(Some(harness.deployer.pubkey()))
        .await;

    accepted(
        &run(&["apply", "--config", &config_path], &environment).await,
        "apply",
    );

    let other_owner = Keypair::new();
    let mut configuration = harness.configuration();
    configuration["owner"] = json!(other_owner.pubkey().to_string());
    let other_path = path_string(&harness.write_configuration("other.json", &configuration));
    let other_key = harness.key_file("other-owner.json", &other_owner);
    let mut other_environment = environment.clone();
    other_environment[2].1 = path_string(&other_key);
    for command in ["apply", "initialise", "set-cap", "pause"] {
        let output = run(&[command, "--config", &other_path], &other_environment).await;
        let message = refused(
            &output,
            &format!("{command} for another owner"),
            &["owner", "already initialised with owner"],
        );
        assert!(message.contains(&harness.owner.pubkey().to_string()));
        assert!(message.contains(&other_owner.pubkey().to_string()));
        assert!(!sent_a_transaction(&stdout(&output)));
    }
    let config = harness.config_record().await.expect("initialised");
    assert_eq!(
        config.owner,
        harness.owner.pubkey(),
        "the owner is unchanged"
    );
    assert!(!config.paused);
}

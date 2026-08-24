use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use layerx_mcp::server::DeploymentMode;
use serde_json::{json, Value};

mod a2a;
mod account;
mod config;
mod credential;
mod encoding;
mod http;
mod install;
mod mcp;
mod output;
mod payment;
mod programs;
mod receipt;
mod scaffold;
mod toolset;
mod workspace;

use config::{Configuration, Environment};
use http::Client;
use output::CommandOutput;

#[derive(Parser)]
#[command(name = "layerx", version, about = "LayerX developer CLI")]
struct Cli {
    #[arg(
        long,
        global = true,
        help = "Emit one JSON object instead of human presentation"
    )]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Scaffold a deterministic Rust program project.
    New(NewArgs),
    /// Install, build, and test every repository module from one visual workspace.
    Workspace(workspace::WorkspaceArgs),
    /// Inspect or switch the emulator, testnet, and production endpoint.
    #[command(subcommand)]
    Environment(EnvironmentCommand),
    /// Manage Ed25519 keys in the operating-system credential store.
    #[command(subcommand)]
    Key(KeyCommand),
    /// Manage hosted API tokens in the operating-system credential store.
    #[command(subcommand)]
    Auth(AuthCommand),
    /// Create or inspect a developer account.
    #[command(subcommand)]
    Account(AccountCommand),
    /// Quote and commit a real test payment through the active endpoint.
    #[command(subcommand)]
    Payment(PaymentCommand),
    /// Fetch receipt material or verify a receipt independently and locally.
    #[command(subcommand)]
    Receipt(ReceiptCommand),
    /// Build, deploy, and inspect deterministic protocol programs.
    #[command(subcommand)]
    Program(ProgramCommand),
    /// Run the local gateway around the real protocol core transition.
    #[command(subcommand)]
    Emulator(EmulatorCommand),
    /// Install and register a payment-capable agent transport in one command.
    #[command(subcommand)]
    Install(InstallCommand),
    /// Serve the model context protocol transport on standard input and output.
    #[command(subcommand)]
    Mcp(McpCommand),
    /// Serve the agent-to-agent transport on a loopback endpoint.
    #[command(subcommand)]
    A2a(A2aCommand),
}

#[derive(Args)]
struct NewArgs {
    name: String,
    #[arg(long, default_value = ".")]
    directory: PathBuf,
}

#[derive(Subcommand)]
enum EnvironmentCommand {
    /// List configured endpoint profiles.
    List,
    /// Show the active endpoint profile.
    Current,
    /// Select a profile, configuring its endpoint when first used.
    Use {
        name: String,
        #[arg(long)]
        endpoint: Option<String>,
        #[arg(long)]
        network_id: Option<u32>,
    },
}

#[derive(Subcommand)]
enum KeyCommand {
    /// Generate a new Ed25519 seed from operating-system randomness.
    Create {
        name: String,
        #[arg(long)]
        did: Option<String>,
    },
    /// Import a 32-byte hexadecimal Ed25519 seed from standard input.
    Import {
        name: String,
        #[arg(long)]
        did: Option<String>,
    },
    /// List public key metadata without opening secret material.
    List,
    /// Show public metadata for one key.
    Show { name: String },
    /// Select the default key used by account commands.
    Default { name: String },
    /// Permanently delete a key from credential storage.
    Delete { name: String },
}

#[derive(Subcommand)]
enum AuthCommand {
    /// Read an API token from standard input and save it securely.
    Set {
        #[arg(long)]
        environment: Option<String>,
    },
    /// Report whether a token exists without printing it.
    Status {
        #[arg(long)]
        environment: Option<String>,
    },
    /// Permanently delete a stored API token.
    Delete {
        #[arg(long)]
        environment: Option<String>,
    },
}

#[derive(Subcommand)]
enum AccountCommand {
    /// Register an account on the active endpoint.
    Create {
        #[arg(long)]
        key: Option<String>,
        #[arg(long, default_value = "0")]
        initial_amount: String,
        #[arg(long)]
        email: Option<String>,
        #[arg(long)]
        display_name: Option<String>,
        #[arg(long)]
        idempotency_key: Option<String>,
    },
    /// Read the active hosted profile or one emulator DID account.
    Get {
        #[arg(long)]
        did: Option<String>,
    },
}

#[derive(Subcommand)]
enum PaymentCommand {
    /// Request a quote and commit it with a stable idempotency key.
    Test {
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        #[arg(long, visible_alias = "asset")]
        currency: String,
        #[arg(long)]
        amount: String,
        #[arg(long)]
        idempotency_key: String,
    },
}

#[derive(Subcommand)]
enum ReceiptCommand {
    /// Fetch exact receipt material from the active endpoint.
    Get { id: String },
    /// Verify a canonical receipt against independently supplied batch facts.
    Verify(VerifyReceiptArgs),
}

#[derive(Args)]
struct VerifyReceiptArgs {
    #[arg(long)]
    receipt: PathBuf,
    #[arg(long)]
    batch_id: String,
    #[arg(long)]
    asset: String,
    #[arg(long)]
    previous_state_root: String,
    #[arg(long)]
    resulting_state_root: String,
    #[arg(long)]
    sequencer_public_key: String,
}

#[derive(Subcommand)]
enum ProgramCommand {
    /// Compile to WASM and enforce the deterministic runtime policy locally.
    Build {
        #[arg(long, default_value = "Cargo.toml")]
        manifest_path: PathBuf,
        #[arg(long)]
        artifact: Option<PathBuf>,
    },
    /// Validate and submit a WASM artifact for receipt-backed deployment.
    Deploy {
        artifact: PathBuf,
        #[arg(long)]
        idempotency_key: String,
        #[arg(long)]
        upgrade_authority: Option<String>,
        #[arg(long)]
        source_uri: Option<String>,
    },
    /// Submit calldata to a deployed program and render the receipt-verified result.
    Call {
        program_id: String,
        #[arg(long)]
        calldata: Option<String>,
        #[arg(long)]
        fuel: u64,
        #[arg(long, default_value = "0")]
        fee_limit: String,
        #[arg(long = "capability")]
        capabilities: Vec<String>,
        #[arg(long)]
        idempotency_key: String,
    },
    /// Read the protocol registry or submit source-verification material.
    #[command(subcommand)]
    Registry(RegistryCommand),
}

#[derive(Subcommand)]
enum RegistryCommand {
    /// Read one program's receipt-backed registry record.
    Get { program_id: String },
    /// Submit a source digest and source location to the registry.
    VerifySource {
        program_id: String,
        #[arg(long)]
        source_uri: String,
        #[arg(long)]
        source_digest: String,
        #[arg(long)]
        idempotency_key: String,
    },
}

#[derive(Subcommand)]
enum EmulatorCommand {
    /// Start the local real-transition gateway.
    Up {
        #[arg(long)]
        listen: Option<String>,
        #[arg(long)]
        network_id: Option<u32>,
        #[arg(long)]
        time_ms: Option<u64>,
        #[arg(long)]
        prefund: Vec<String>,
    },
}

#[derive(Subcommand)]
enum InstallCommand {
    /// Install a payment-capable model context protocol server.
    Mcp(InstallMcpArgs),
    /// Install a payment-capable agent-to-agent server.
    A2a(InstallA2aArgs),
}

#[derive(Args)]
struct InstallMcpArgs {
    #[arg(long)]
    environment: Option<String>,
    #[arg(long)]
    host: Vec<String>,
    #[arg(long)]
    key: Option<String>,
    #[arg(long)]
    read_only: bool,
    #[arg(long)]
    token_stdin: bool,
    #[arg(long)]
    rotate: bool,
    #[arg(long)]
    source_account: Option<String>,
    #[arg(long)]
    asset: Option<String>,
}

#[derive(Args)]
struct InstallA2aArgs {
    #[arg(long)]
    environment: Option<String>,
    #[arg(long, default_value = "127.0.0.1:9433")]
    listen: String,
    #[arg(long)]
    key: Option<String>,
    #[arg(long)]
    well_known: Option<PathBuf>,
    #[arg(long)]
    read_only: bool,
    #[arg(long)]
    token_stdin: bool,
    #[arg(long)]
    rotate: bool,
    #[arg(long)]
    source_account: Option<String>,
    #[arg(long)]
    asset: Option<String>,
}

#[derive(Subcommand)]
enum McpCommand {
    /// Serve the installed tool surface for one environment and key.
    Serve {
        #[arg(long)]
        environment: Option<String>,
        #[arg(long)]
        key: Option<String>,
        #[arg(long)]
        gateway_credential: String,
        #[arg(long)]
        source_account: Option<String>,
        #[arg(long)]
        asset: Option<String>,
        #[arg(long)]
        read_only: bool,
    },
}

#[derive(Subcommand)]
enum A2aCommand {
    /// Serve the agent card and task interface for one environment and key.
    Serve {
        #[arg(long)]
        environment: Option<String>,
        #[arg(long)]
        key: Option<String>,
        #[arg(long)]
        gateway_credential: String,
        #[arg(long)]
        source_account: Option<String>,
        #[arg(long)]
        asset: Option<String>,
        #[arg(long, default_value = "127.0.0.1:9433")]
        listen: String,
        #[arg(long)]
        read_only: bool,
    },
    /// Start the installed managed A2A runtime.
    Start,
    /// Stop the installed managed A2A runtime.
    Stop,
    /// Report the installed managed A2A runtime state.
    Status,
}

/// Stable graph anchor for the unified developer CLI.
#[must_use]
pub const fn platform_cli() -> &'static str {
    "layerx-cli-v1"
}

fn run(command: Command, machine: bool) -> Result<Option<CommandOutput>, String> {
    match command {
        Command::New(arguments) => Ok(Some(CommandOutput::new(
            "project.created",
            format!("Created LayerX program project {}", arguments.name),
            scaffold::create(&arguments.name, &arguments.directory)?,
        ))),
        Command::Workspace(arguments) => workspace::run(arguments, machine),
        Command::Environment(command) => environment(command).map(Some),
        Command::Key(command) => key(command).map(Some),
        Command::Auth(command) => auth(command).map(Some),
        Command::Account(command) => account(command).map(Some),
        Command::Payment(command) => payment(command).map(Some),
        Command::Receipt(command) => receipt(command).map(Some),
        Command::Program(command) => program(command).map(Some),
        Command::Emulator(command) => {
            let arguments = emulator_arguments(command);
            if machine {
                CommandOutput::new(
                    "emulator.starting",
                    "Starting LayerX emulator",
                    json!({"core": "real-transition", "arguments": arguments}),
                )
                .emit(true)?;
            }
            layerx_platform_emulator::run(arguments)?;
            Ok(None)
        }
        Command::Install(command) => install(command).map(Some),
        Command::Mcp(McpCommand::Serve {
            environment,
            key,
            gateway_credential,
            source_account,
            asset,
            read_only,
        }) => {
            let configuration = serving_configuration(environment)?;
            let key = serving_key(&configuration, key.as_deref())?;
            mcp::serve(
                &configuration,
                &gateway_credential,
                key,
                source_account.as_deref(),
                asset.as_deref(),
                deployment_mode(read_only),
            )?;
            Ok(None)
        }
        Command::A2a(command) => match command {
            A2aCommand::Serve {
                environment,
                key,
                gateway_credential,
                source_account,
                asset,
                listen,
                read_only,
            } => {
                let configuration = serving_configuration(environment)?;
                let key = serving_key(&configuration, key.as_deref())?;
                a2a::serve(
                    &configuration,
                    &gateway_credential,
                    key,
                    source_account.as_deref(),
                    asset.as_deref(),
                    &listen,
                    deployment_mode(read_only),
                )?;
                Ok(None)
            }
            A2aCommand::Start => Ok(Some(CommandOutput::new(
                "a2a.started",
                "Started the installed LayerX A2A runtime",
                a2a::start_from_manifest()?,
            ))),
            A2aCommand::Stop => Ok(Some(CommandOutput::new(
                "a2a.stopped",
                "Stopped the installed LayerX A2A runtime",
                a2a::stop_installed()?,
            ))),
            A2aCommand::Status => Ok(Some(CommandOutput::new(
                "a2a.status",
                "Read the installed LayerX A2A runtime state",
                a2a::installed_status()?,
            ))),
        },
    }
}

fn install(command: InstallCommand) -> Result<CommandOutput, String> {
    let mut configuration = Configuration::load()?;
    match command {
        InstallCommand::Mcp(arguments) => {
            let request = install::mcp::Request {
                environment: arguments.environment,
                hosts: arguments.host,
                key: arguments.key,
                read_only: arguments.read_only,
                token_stdin: arguments.token_stdin,
                rotate: arguments.rotate,
                source_account: arguments.source_account,
                asset: arguments.asset,
            };
            let data = install::mcp::platform_install_mcp(&mut configuration, &request)?;
            let message = format!(
                "Installed the LayerX model context protocol server for {}",
                environment_of(&data)
            );
            Ok(CommandOutput::new("install.mcp", message, data))
        }
        InstallCommand::A2a(arguments) => {
            let request = install::a2a::Request {
                environment: arguments.environment,
                listen: arguments.listen,
                key: arguments.key,
                well_known: arguments.well_known,
                read_only: arguments.read_only,
                token_stdin: arguments.token_stdin,
                rotate: arguments.rotate,
                source_account: arguments.source_account,
                asset: arguments.asset,
            };
            let data = install::a2a::platform_install_a2a(&mut configuration, &request)?;
            let message = format!(
                "Installed the LayerX agent-to-agent server for {}",
                environment_of(&data)
            );
            Ok(CommandOutput::new("install.a2a", message, data))
        }
    }
}

fn environment_of(data: &Value) -> &str {
    data.get("environment")
        .and_then(Value::as_str)
        .unwrap_or("the active environment")
}

fn serving_configuration(environment: Option<String>) -> Result<Configuration, String> {
    let mut configuration = Configuration::load()?;
    if let Some(name) = environment {
        Configuration::validate_environment_name(&name)?;
        if !configuration.environments.contains_key(&name) {
            return Err(format!(
                "environment {name} is not configured; run layerx environment use {name} --endpoint <url> --network-id <id>"
            ));
        }
        configuration.current_environment = name;
    }
    Ok(configuration)
}

fn serving_key<'a>(
    configuration: &'a Configuration,
    key: Option<&'a str>,
) -> Result<&'a str, String> {
    let name = match key {
        Some(value) => value,
        None => match &configuration.default_key {
            Some(value) => value.as_str(),
            None => return Err("the installed runtime did not name a signing key".into()),
        },
    };
    if !configuration.keys.contains_key(name) {
        return Err(format!("key {name} does not exist"));
    }
    Ok(name)
}

const fn deployment_mode(read_only: bool) -> DeploymentMode {
    if read_only {
        DeploymentMode::ReadOnly
    } else {
        DeploymentMode::Full
    }
}

fn environment(command: EnvironmentCommand) -> Result<CommandOutput, String> {
    let mut configuration = Configuration::load()?;
    match command {
        EnvironmentCommand::List => {
            let values = configuration
                .environments
                .iter()
                .map(|(name, value)| {
                    json!({
                        "name": name,
                        "current": *name == configuration.current_environment,
                        "endpoint": value.endpoint,
                        "network_id": value.network_id,
                    })
                })
                .collect::<Vec<_>>();
            Ok(CommandOutput::new(
                "environment.list",
                format!("{} LayerX environments configured", values.len()),
                Value::Array(values),
            ))
        }
        EnvironmentCommand::Current => {
            let (name, value) = configuration.active_environment()?;
            Ok(CommandOutput::new(
                "environment.current",
                format!("Using LayerX {name}"),
                json!({"name": name, "endpoint": value.endpoint, "network_id": value.network_id}),
            ))
        }
        EnvironmentCommand::Use {
            name,
            endpoint,
            network_id,
        } => {
            Configuration::validate_environment_name(&name)?;
            if endpoint.is_some() != network_id.is_some() {
                return Err("--endpoint and --network-id must be supplied together".into());
            }
            if let (Some(endpoint), Some(network_id)) = (endpoint, network_id) {
                validate_endpoint(&endpoint)?;
                if network_id == 0 {
                    return Err("network id zero is reserved".into());
                }
                configuration.environments.insert(
                    name.clone(),
                    Environment {
                        endpoint,
                        network_id,
                    },
                );
            } else if !configuration.environments.contains_key(&name) {
                return Err(format!(
                    "environment {name} is not configured; supply --endpoint and --network-id"
                ));
            }
            configuration.current_environment.clone_from(&name);
            configuration.save()?;
            let value = configuration
                .environments
                .get(&name)
                .ok_or_else(|| "environment disappeared while saving configuration".to_string())?;
            Ok(CommandOutput::new(
                "environment.selected",
                format!("Using LayerX {name}"),
                json!({"name": name, "endpoint": value.endpoint, "network_id": value.network_id}),
            ))
        }
    }
}

fn key(command: KeyCommand) -> Result<CommandOutput, String> {
    let mut configuration = Configuration::load()?;
    match command {
        KeyCommand::Create { name, did } => {
            let metadata = credential::create_key(&mut configuration, &name, did)?;
            Ok(CommandOutput::new(
                "key.created",
                format!("Created key {name} in operating-system credential storage"),
                json!({"name": name, "did": metadata.did, "public_key": metadata.public_key}),
            ))
        }
        KeyCommand::Import { name, did } => {
            let metadata = credential::import_key(&mut configuration, &name, did)?;
            Ok(CommandOutput::new(
                "key.imported",
                format!("Imported key {name} into operating-system credential storage"),
                json!({"name": name, "did": metadata.did, "public_key": metadata.public_key}),
            ))
        }
        KeyCommand::List => {
            let values = configuration
                .keys
                .iter()
                .map(|(name, metadata)| {
                    json!({
                        "name": name,
                        "default": configuration.default_key.as_deref() == Some(name),
                        "did": metadata.did,
                        "public_key": metadata.public_key,
                    })
                })
                .collect::<Vec<_>>();
            Ok(CommandOutput::new(
                "key.list",
                format!("{} LayerX keys", values.len()),
                Value::Array(values),
            ))
        }
        KeyCommand::Show { name } => {
            let metadata = configuration
                .keys
                .get(&name)
                .ok_or_else(|| format!("key {name} does not exist"))?;
            Ok(CommandOutput::new(
                "key.metadata",
                format!("Key {name}"),
                json!({
                    "name": name,
                    "default": configuration.default_key.as_deref() == Some(&name),
                    "did": metadata.did,
                    "public_key": metadata.public_key,
                    "secret_storage": "operating-system-credential-store",
                }),
            ))
        }
        KeyCommand::Default { name } => {
            credential::set_default_key(&mut configuration, &name)?;
            Ok(CommandOutput::new(
                "key.default",
                format!("Key {name} is now the default"),
                json!({"name": name}),
            ))
        }
        KeyCommand::Delete { name } => {
            credential::delete_key(&mut configuration, &name)?;
            Ok(CommandOutput::new(
                "key.deleted",
                format!("Deleted key {name} from operating-system credential storage"),
                json!({"name": name}),
            ))
        }
    }
}

fn auth(command: AuthCommand) -> Result<CommandOutput, String> {
    let configuration = Configuration::load()?;
    match command {
        AuthCommand::Set { environment } => {
            let environment = selected_environment(&configuration, environment)?;
            credential::set_token(&environment)?;
            Ok(CommandOutput::new(
                "auth.saved",
                format!("Saved {environment} API token in operating-system credential storage"),
                json!({"environment": environment, "secret_storage": "operating-system-credential-store"}),
            ))
        }
        AuthCommand::Status { environment } => {
            let environment = selected_environment(&configuration, environment)?;
            let configured = credential::token(&environment)?.is_some();
            Ok(CommandOutput::new(
                "auth.status",
                if configured {
                    format!("An API token is configured for {environment}")
                } else {
                    format!("No API token is configured for {environment}")
                },
                json!({"environment": environment, "configured": configured}),
            ))
        }
        AuthCommand::Delete { environment } => {
            let environment = selected_environment(&configuration, environment)?;
            credential::delete_token(&environment)?;
            Ok(CommandOutput::new(
                "auth.deleted",
                format!("Deleted the {environment} API token"),
                json!({"environment": environment}),
            ))
        }
    }
}

fn account(command: AccountCommand) -> Result<CommandOutput, String> {
    let configuration = Configuration::load()?;
    let (environment, client) = active_client(&configuration)?;
    match command {
        AccountCommand::Create {
            key,
            initial_amount,
            email,
            display_name,
            idempotency_key,
        } => {
            let key_name = key.or_else(|| configuration.default_key.clone());
            let metadata = key_name
                .as_deref()
                .map(|name| {
                    configuration
                        .keys
                        .get(name)
                        .ok_or_else(|| format!("key {name} does not exist"))
                })
                .transpose()?;
            let value = account::create(
                &client,
                &environment,
                metadata,
                &initial_amount,
                email.as_deref(),
                display_name.as_deref(),
                idempotency_key.as_deref(),
            )?;
            Ok(CommandOutput::new(
                "account.created",
                format!("Created an account on {environment}"),
                value,
            ))
        }
        AccountCommand::Get { did } => Ok(CommandOutput::new(
            "account.read",
            format!("Read the active account from {environment}"),
            account::get(&client, &environment, did.as_deref())?,
        )),
    }
}

fn payment(command: PaymentCommand) -> Result<CommandOutput, String> {
    let configuration = Configuration::load()?;
    let (environment, client) = active_client(&configuration)?;
    match command {
        PaymentCommand::Test {
            from,
            to,
            currency,
            amount,
            idempotency_key,
        } => Ok(CommandOutput::new(
            "payment.started",
            format!("Started a test payment on {environment}"),
            payment::test_payment(&client, &from, &to, &currency, &amount, &idempotency_key)?,
        )),
    }
}

fn receipt(command: ReceiptCommand) -> Result<CommandOutput, String> {
    match command {
        ReceiptCommand::Get { id } => {
            http::validate_resource_id(&id, "receipt id")?;
            let configuration = Configuration::load()?;
            let (environment, client) = active_client(&configuration)?;
            Ok(CommandOutput::new(
                "receipt.read",
                format!("Read receipt {id} from {environment}"),
                client.get(&format!("/v1/receipts/{id}"))?,
            ))
        }
        ReceiptCommand::Verify(arguments) => Ok(CommandOutput::new(
            "receipt.verified",
            format!("Verified receipt {} locally", arguments.receipt.display()),
            receipt::verify_file(
                &arguments.receipt,
                receipt::VerificationFacts {
                    batch_id: &arguments.batch_id,
                    asset: &arguments.asset,
                    previous_state_root: &arguments.previous_state_root,
                    resulting_state_root: &arguments.resulting_state_root,
                    sequencer_public_key: &arguments.sequencer_public_key,
                },
            )?,
        )),
    }
}

fn program(command: ProgramCommand) -> Result<CommandOutput, String> {
    match command {
        ProgramCommand::Build {
            manifest_path,
            artifact,
        } => Ok(CommandOutput::new(
            "program.built",
            "Built and validated a deterministic LayerX program",
            programs::build(&manifest_path, artifact.as_deref())?,
        )),
        ProgramCommand::Deploy {
            artifact,
            idempotency_key,
            upgrade_authority,
            source_uri,
        } => {
            let configuration = Configuration::load()?;
            let (environment, client) = active_client(&configuration)?;
            Ok(CommandOutput::new(
                "program.deployment_started",
                format!("Submitted program deployment to {environment}"),
                programs::deploy(
                    &client,
                    &artifact,
                    upgrade_authority.as_deref(),
                    source_uri.as_deref(),
                    &idempotency_key,
                )?,
            ))
        }
        ProgramCommand::Call {
            program_id,
            calldata,
            fuel,
            fee_limit,
            capabilities,
            idempotency_key,
        } => {
            let configuration = Configuration::load()?;
            let (environment, client) = active_client(&configuration)?;
            Ok(CommandOutput::new(
                "program.call_started",
                format!("Submitted program call to {environment}"),
                programs::call(
                    &client,
                    &programs::CallRequest {
                        program_id: &program_id,
                        calldata: calldata.as_deref().unwrap_or(""),
                        fuel,
                        fee_limit: &fee_limit,
                        capabilities: &capabilities,
                        idempotency_key: &idempotency_key,
                    },
                )?,
            ))
        }
        ProgramCommand::Registry(command) => {
            let configuration = Configuration::load()?;
            let (environment, client) = active_client(&configuration)?;
            match command {
                RegistryCommand::Get { program_id } => Ok(CommandOutput::new(
                    "program.registry_read",
                    format!("Read program {program_id} from {environment}"),
                    programs::registry_get(&client, &program_id)?,
                )),
                RegistryCommand::VerifySource {
                    program_id,
                    source_uri,
                    source_digest,
                    idempotency_key,
                } => Ok(CommandOutput::new(
                    "program.source_submitted",
                    format!("Submitted source verification for {program_id} on {environment}"),
                    programs::registry_verify_source(
                        &client,
                        &program_id,
                        &source_uri,
                        &source_digest,
                        &idempotency_key,
                    )?,
                )),
            }
        }
    }
}

fn active_client(configuration: &Configuration) -> Result<(String, Client), String> {
    let (name, environment) = configuration.active_environment()?;
    let token = credential::token(name)?;
    Ok((name.to_owned(), Client::new(&environment.endpoint, token)?))
}

fn selected_environment(
    configuration: &Configuration,
    selected: Option<String>,
) -> Result<String, String> {
    let name = selected.unwrap_or_else(|| configuration.current_environment.clone());
    Configuration::validate_environment_name(&name)?;
    Ok(name)
}

fn validate_endpoint(endpoint: &str) -> Result<(), String> {
    Client::new(endpoint, None).map(|_| ())
}

fn emulator_arguments(command: EmulatorCommand) -> Vec<String> {
    let EmulatorCommand::Up {
        listen,
        network_id,
        time_ms,
        prefund,
    } = command;
    let mut arguments = vec!["up".to_string()];
    if let Some(value) = listen {
        arguments.extend(["--listen".into(), value]);
    }
    if let Some(value) = network_id {
        arguments.extend(["--network-id".into(), value.to_string()]);
    }
    if let Some(value) = time_ms {
        arguments.extend(["--time-ms".into(), value.to_string()]);
    }
    for value in prefund {
        arguments.extend(["--prefund".into(), value]);
    }
    arguments
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli.command, cli.json) {
        Ok(Some(output)) => match output.emit(cli.json) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                output::emit_error(&error, cli.json);
                ExitCode::FAILURE
            }
        },
        Ok(None) => ExitCode::SUCCESS,
        Err(error) => {
            output::emit_error(&error, cli.json);
            ExitCode::FAILURE
        }
    }
}

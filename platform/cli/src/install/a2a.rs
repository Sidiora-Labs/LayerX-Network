use std::path::PathBuf;

use layerx_mcp::server::{DeploymentMode, ToolDefinition};
use serde_json::{json, Value};

use crate::config::Configuration;
use crate::encoding::fixed_hex;
use crate::toolset;

use super::{
    apply, executable, layerx_directory, publish, report, select, variables, FileTransaction,
    Registration, SERVER_NAME,
};

pub const PROTOCOL_VERSION: &str = "0.3.0";
pub const ENVIRONMENT_EXTENSION: &str = "https://layerx.dev/a2a/extensions/environment/v1";

const CARD_FILE: &str = "agent-card.json";
const REGISTRY_FILE: &str = "runtime.json";
const REGISTRY_SECTION: &str = "agents";

pub struct Request {
    pub environment: Option<String>,
    pub listen: String,
    pub key: Option<String>,
    pub well_known: Option<PathBuf>,
    pub read_only: bool,
    pub token_stdin: bool,
    pub rotate: bool,
    pub source_account: Option<String>,
    pub asset: Option<String>,
}

/// Installs and registers a payment-capable `LayerX` agent-to-agent server.
pub fn platform_install_a2a(
    configuration: &mut Configuration,
    request: &Request,
) -> Result<Value, String> {
    let address = crate::a2a::endpoint(&request.listen)?;
    let listen = address.to_string();
    let payment = payment_binding(request)?;
    let mode = if request.read_only {
        DeploymentMode::ReadOnly
    } else {
        DeploymentMode::Full
    };
    let tools = toolset::surface(mode)?;
    let command = executable()?;
    let mut variables = variables()?;
    let selection = select(
        configuration,
        request.environment.clone(),
        request.key.clone(),
        "a2a",
        request.token_stdin,
        "a2a",
        request.read_only,
        request.rotate,
    )?;
    variables.insert(
        "LAYERX_GATEWAY_KEY_ID".to_owned(),
        selection.gateway_key_id.clone(),
    );
    let arguments = launch_arguments(
        &selection.environment,
        &selection.key,
        &selection.gateway_alias,
        payment.as_ref(),
        &listen,
        request.read_only,
    );
    let directory = layerx_directory()?.join("a2a");
    let card_path = directory.join(CARD_FILE);
    let card = agent_card(&selection.environment, &listen, mode, &tools);
    let registration = Registration {
        path: directory.join(REGISTRY_FILE),
        section: REGISTRY_SECTION,
        name: SERVER_NAME.to_owned(),
        entry: json!({
            "url": endpoint_url(&listen),
            "transport": "JSONRPC",
            "card": card_path.display().to_string(),
            "command": command,
            "args": arguments,
            "env": variables,
            "environment": selection.environment,
            "lifecycle": {
                "start": [command, "a2a", "start"],
                "stop": [command, "a2a", "stop"],
                "status": [command, "a2a", "status"],
            },
        }),
    };
    let descriptors: Vec<Value> = tools.iter().copied().map(toolset::descriptor).collect();
    let mut paths = vec![card_path.clone(), registration.path.clone()];
    if let Some(root) = &request.well_known {
        paths.push(root.join(".well-known").join(CARD_FILE));
    }
    let transaction = match FileTransaction::capture(&paths) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let mut published: Vec<Value> = Vec::new();
    let applied = (|| {
        let card_outcome = publish(&card_path, &card)?;
        let registry_outcome = apply(&registration)?;
        let mut changed = card_outcome.changed || registry_outcome.changed;
        if let Some(root) = &request.well_known {
            let path = root.join(".well-known").join(CARD_FILE);
            let outcome = publish(&path, &card)?;
            if outcome.changed {
                changed = true;
            }
            published.push(report(&path, "document", CARD_FILE, &outcome));
        }
        Ok::<_, String>((card_outcome, registry_outcome, changed))
    })();
    let (card_outcome, registry_outcome, changed) = match applied {
        Ok(value) => value,
        Err(error) => {
            let rollback = transaction.rollback();
            return match rollback {
                Ok(()) => Err(error),
                Err(rollback) => Err(format!("{error}; installation rollback failed: {rollback}")),
            };
        }
    };
    if selection.rotated_gateway_key {
        if let Err(error) = crate::a2a::stop_installed() {
            let rollback = transaction.rollback();
            return match rollback {
                Ok(()) => Err(error),
                Err(rollback) => Err(format!("{error}; installation rollback failed: {rollback}")),
            };
        }
    }
    let lifecycle = match crate::a2a::start_installed(&command, &arguments, &variables) {
        Ok(value) => value,
        Err(error) => {
            let rollback = transaction.rollback();
            return match rollback {
                Ok(()) => Err(error),
                Err(rollback) => Err(format!("{error}; installation rollback failed: {rollback}")),
            };
        }
    };
    Ok(json!({
        "component": "a2a",
        "transport": "JSONRPC",
        "environment": selection.environment,
        "endpoint": selection.endpoint,
        "network_id": selection.network_id,
        "deployment_mode": toolset::mode_name(mode),
        "listen": listen,
        "url": endpoint_url(&listen),
        "server": {
            "name": SERVER_NAME,
            "command": command,
            "args": arguments,
            "env": variables,
        },
        "lifecycle": lifecycle,
        "agent_card": report(&card_path, "document", CARD_FILE, &card_outcome),
        "registrations": [
            report(
                &registration.path,
                registration.section,
                &registration.name,
                &registry_outcome,
            ),
        ],
        "published": published,
        "skills": descriptors,
        "scopes": toolset::scopes(&tools),
        "credentials": selection.credentials(),
        "changed": changed,
        "idempotent": true,
    }))
}

pub fn agent_card(
    environment: &str,
    listen: &str,
    mode: DeploymentMode,
    tools: &[ToolDefinition],
) -> Value {
    let skills = tools
        .iter()
        .map(|tool| {
            json!({
                "id": tool.name,
                "name": tool.name,
                "description": toolset::description(tool.name),
                "tags": ["layerx", "payment", toolset::kind_name(tool.kind)],
                "inputModes": ["application/json"],
                "outputModes": ["application/json"],
            })
        })
        .collect::<Vec<_>>();
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "name": "LayerX",
        "description": "Receipt-verified LayerX payment agent bound to one environment and one key.",
        "url": endpoint_url(listen),
        "preferredTransport": "JSONRPC",
        "version": env!("CARGO_PKG_VERSION"),
        "capabilities": {
            "streaming": false,
            "pushNotifications": false,
            "stateTransitionHistory": false,
            "extensions": [
                {
                    "uri": ENVIRONMENT_EXTENSION,
                    "description": "Declares the bound LayerX environment, deployment mode and enforced hosted-gateway scopes.",
                    "required": false,
                    "params": {
                        "environment": environment,
                        "deployment_mode": toolset::mode_name(mode),
                        "scopes": toolset::scopes(tools),
                    },
                },
            ],
        },
        "defaultInputModes": ["application/json"],
        "defaultOutputModes": ["application/json"],
        "skills": skills,
    })
}

pub fn endpoint_url(listen: &str) -> String {
    format!("http://{listen}/")
}

fn payment_binding(request: &Request) -> Result<Option<(String, String)>, String> {
    if request.read_only {
        if request.source_account.is_some() || request.asset.is_some() {
            return Err("--source-account and --asset are not accepted in read-only mode".into());
        }
        return Ok(None);
    }
    let source = request.source_account.as_deref().ok_or_else(|| {
        "payment-capable installation requires --source-account <64-hex account id>".to_owned()
    })?;
    let asset = request.asset.as_deref().ok_or_else(|| {
        "payment-capable installation requires --asset <64-hex asset id>".to_owned()
    })?;
    fixed_hex::<32>("source account", source)?;
    fixed_hex::<32>("asset", asset)?;
    Ok(Some((
        source.to_ascii_lowercase(),
        asset.to_ascii_lowercase(),
    )))
}

fn launch_arguments(
    environment: &str,
    key: &str,
    gateway_alias: &str,
    payment: Option<&(String, String)>,
    listen: &str,
    read_only: bool,
) -> Vec<String> {
    let mut arguments = vec![
        "a2a".to_owned(),
        "serve".to_owned(),
        "--environment".to_owned(),
        environment.to_owned(),
        "--key".to_owned(),
        key.to_owned(),
        "--gateway-credential".to_owned(),
        gateway_alias.to_owned(),
        "--listen".to_owned(),
        listen.to_owned(),
    ];
    if let Some((source, asset)) = payment {
        arguments.extend([
            "--source-account".to_owned(),
            source.clone(),
            "--asset".to_owned(),
            asset.clone(),
        ]);
    }
    if read_only {
        arguments.push("--read-only".to_owned());
    }
    arguments
}

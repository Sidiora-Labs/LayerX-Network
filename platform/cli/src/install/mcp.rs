use layerx_mcp::server::DeploymentMode;
use serde_json::{json, Value};

use crate::config;
use crate::config::Configuration;
use crate::encoding::fixed_hex;
use crate::toolset;

use super::{
    apply, executable, hosts, report, select, variables, FileTransaction, Registration, SERVER_NAME,
};

pub struct Request {
    pub environment: Option<String>,
    pub hosts: Vec<String>,
    pub key: Option<String>,
    pub read_only: bool,
    pub token_stdin: bool,
    pub rotate: bool,
    pub source_account: Option<String>,
    pub asset: Option<String>,
}

/// Installs and registers the daemon-bound `LayerX` model context protocol server.
pub fn platform_install_mcp(
    configuration: &mut Configuration,
    request: &Request,
) -> Result<Value, String> {
    let payment = payment_binding(request)?;
    let mode = if request.read_only {
        DeploymentMode::ReadOnly
    } else {
        DeploymentMode::Full
    };
    let tools = toolset::daemon_surface(mode)?;
    let daemon_binding = daemon_binding_path()?;
    let command = executable()?;
    let mut variables = variables()?;
    let selected_hosts = hosts(&request.hosts)?;
    if selected_hosts.is_empty() {
        return Err("no agent runtime was selected for installation".into());
    }
    let selection = select(
        configuration,
        super::SelectionRequest {
            environment: request.environment.clone(),
            key: request.key.clone(),
            fallback_key: "mcp",
            token_stdin: request.token_stdin,
            component: "mcp",
            read_only: request.read_only,
            rotate: request.rotate,
        },
    )?;
    variables.insert(
        "LAYERX_GATEWAY_KEY_ID".to_owned(),
        selection.gateway_key_id.clone(),
    );
    let arguments = launch_arguments(&daemon_binding, request.read_only);
    let descriptors = tools
        .iter()
        .copied()
        .map(toolset::daemon_descriptor)
        .collect::<Result<Vec<Value>, String>>()?;
    let mut pending = Vec::new();
    for host in selected_hosts {
        let path = host.path()?;
        pending.push((
            host,
            Registration {
                path,
                section: host.section(),
                name: SERVER_NAME.to_owned(),
                entry: host.entry(&command, &arguments, &variables),
            },
        ));
    }
    let (registrations, changed) = publish_registrations(&pending)?;
    Ok(json!({
        "component": "mcp",
        "transport": "stdio",
        "environment": selection.environment,
        "endpoint": selection.endpoint,
        "network_id": selection.network_id,
        "deployment_mode": toolset::mode_name(mode),
        "daemon_binding": daemon_binding,
        "account_binding": payment
            .as_ref()
            .map(|(source, asset)| json!({"source_account": source, "asset": asset})),
        "server": {
            "name": SERVER_NAME,
            "command": command,
            "args": arguments,
            "env": variables,
        },
        "tools": descriptors,
        "scopes": toolset::scopes(&tools),
        "credentials": selection.credentials(),
        "registrations": registrations,
        "changed": changed,
        "idempotent": true,
    }))
}

fn publish_registrations(
    pending: &[(super::Host, Registration)],
) -> Result<(Vec<Value>, bool), String> {
    let paths = pending
        .iter()
        .map(|(_, registration)| registration.path.clone())
        .collect::<Vec<_>>();
    let mut transaction = FileTransaction::capture(&paths)?;
    let mut registrations = Vec::new();
    let mut changed = false;
    let applied = (|| {
        for (host, registration) in pending {
            transaction.begin_publication(&registration.path)?;
            let outcome = apply(registration)?;
            transaction.finish_publication(&registration.path, outcome.changed)?;
            if outcome.changed {
                changed = true;
            }
            let mut record = report(
                &registration.path,
                registration.section,
                &registration.name,
                &outcome,
            );
            if let Some(fields) = record.as_object_mut() {
                fields.insert("host".to_owned(), json!(host.name()));
            }
            registrations.push(record);
        }
        Ok::<(), String>(())
    })();
    if let Err(error) = applied {
        let rollback = transaction.rollback();
        return match rollback {
            Ok(()) => Err(error),
            Err(rollback) => Err(format!("{error}; installation rollback failed: {rollback}")),
        };
    }
    Ok((registrations, changed))
}

pub(super) fn payment_binding(request: &Request) -> Result<Option<(String, String)>, String> {
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

/// Resolves the daemon binding document the served path reads, beside the CLI configuration.
fn daemon_binding_path() -> Result<String, String> {
    let configuration = config::path()?;
    let directory = configuration
        .parent()
        .ok_or_else(|| "the CLI configuration path has no parent directory".to_owned())?;
    directory
        .join("mcp")
        .join("binding.json")
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| "the daemon binding path is not valid UTF-8".to_owned())
}

fn launch_arguments(daemon_binding: &str, read_only: bool) -> Vec<String> {
    let mut arguments = vec![
        "mcp".to_owned(),
        "serve".to_owned(),
        "--daemon-binding".to_owned(),
        daemon_binding.to_owned(),
    ];
    if read_only {
        arguments.push("--read-only".to_owned());
    }
    arguments
}

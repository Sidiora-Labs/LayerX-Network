use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};

use layerx_mcp::binding::Binding;
use layerx_mcp::server::DeploymentMode;
use serde_json::{json, Value};

use crate::config;
use crate::toolset;

use super::{apply, executable, hosts, report, FileTransaction, Registration, SERVER_NAME};

pub struct Request {
    pub hosts: Vec<String>,
    pub read_only: bool,
    pub daemon_binding: Option<PathBuf>,
}

/// Installs and registers the daemon-bound `LayerX` model context protocol server.
///
/// The installed launch line names the binding document that agent-daemon enrolment wrote; the
/// installer opens that document exactly as the served path does and refuses when it is absent,
/// so a registration never points at a catalogue the daemon will not serve. No gateway key,
/// environment, or payment flag takes part: the daemon holds every credential the tools need.
///
/// Returns the agent-daemon endpoint the registration is bound to and the recorded document.
///
/// # Errors
///
/// Returns the operator message when no host is selected, when the binding document cannot be
/// opened, or when a registration cannot be published.
pub fn platform_install_mcp(request: &Request) -> Result<(String, Value), String> {
    let selected_hosts = hosts(&request.hosts)?;
    if selected_hosts.is_empty() {
        return Err("no agent runtime was selected for installation".into());
    }
    let daemon_binding = match &request.daemon_binding {
        Some(explicit) => absolute_binding_path(explicit)?,
        None => daemon_binding_path()?,
    };
    let binding = Binding::open(&daemon_binding).map_err(|error| {
        format!(
            "the daemon binding document at {} could not be used: {}; agent-daemon enrolment writes it before the MCP server is installed",
            daemon_binding.display(),
            error.detail()
        )
    })?;
    let mode = if request.read_only {
        DeploymentMode::ReadOnly
    } else {
        binding.mode()
    };
    let tools = toolset::daemon_surface(mode)?;
    let command = executable()?;
    let variables: BTreeMap<String, String> = BTreeMap::new();
    let agent_endpoint = binding.agent_endpoint().to_owned();
    let binding_path = path_text(&daemon_binding)?;
    let arguments = launch_arguments(&binding_path, request.read_only);
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
    let document = json!({
        "component": "mcp",
        "transport": "stdio",
        "authorization": "agent-daemon",
        "deployment_mode": toolset::mode_name(mode),
        "daemon_binding": {
            "path": binding_path,
            "tenant": binding.tenant(),
            "declared_mode": toolset::mode_name(binding.mode()),
            "agent_endpoint": &agent_endpoint,
            "store": path_text(binding.store())?,
            "session_generation": binding.session_generation(),
        },
        "server": {
            "name": SERVER_NAME,
            "command": command,
            "args": arguments,
            "env": variables,
        },
        "tools": descriptors,
        "scopes": toolset::scopes(&tools),
        "registrations": registrations,
        "changed": changed,
        "idempotent": true,
    });
    Ok((agent_endpoint, document))
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

/// Resolves the daemon binding document the served path reads, beside the CLI configuration.
fn daemon_binding_path() -> Result<PathBuf, String> {
    let configuration = config::path()?;
    let directory = configuration
        .parent()
        .ok_or_else(|| "the CLI configuration path has no parent directory".to_owned())?;
    Ok(directory.join("mcp").join("binding.json"))
}

/// Anchors an operator-supplied binding path so the installed launch line never depends on the
/// working directory of the runtime that starts the server.
fn absolute_binding_path(explicit: &Path) -> Result<PathBuf, String> {
    if explicit.as_os_str().is_empty() {
        return Err("--daemon-binding requires a path".into());
    }
    if explicit.is_absolute() {
        return Ok(explicit.to_path_buf());
    }
    let current = env::current_dir()
        .map_err(|error| format!("the current directory is not available: {error}"))?;
    Ok(current.join(explicit))
}

fn path_text(path: &Path) -> Result<String, String> {
    path.to_str().map(str::to_owned).ok_or_else(|| {
        format!(
            "the path {} is not valid UTF-8 and cannot be written into a launch line",
            path.display()
        )
    })
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

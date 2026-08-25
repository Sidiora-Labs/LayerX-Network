use std::fs::{self, File};
use std::io::Read as _;
use std::path::{Path, PathBuf};

use layerx_mcp::server::{DeploymentMode, ToolDefinition};
use serde_json::{json, Value};
use zeroize::{Zeroize as _, Zeroizing};

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
const AUTHORIZATION_FILE: &str = "authorization";
const MAX_AUTHORIZATION_BYTES: usize = 256;

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
    let directory = layerx_directory()?.join("a2a");
    let authorization_path = directory.join(AUTHORIZATION_FILE);
    let authorization = prepare_authorization(&authorization_path, request.rotate)?;
    let arguments = launch_arguments(
        &selection.environment,
        &selection.key,
        &selection.gateway_alias,
        payment.as_ref(),
        &listen,
        request.read_only,
        &authorization_path,
    );
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
    let mut paths = vec![
        card_path.clone(),
        registration.path.clone(),
        authorization_path.clone(),
    ];
    if let Some(root) = &request.well_known {
        paths.push(root.join(".well-known").join(CARD_FILE));
    }
    let mut transaction = match FileTransaction::capture(&paths) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let mut published: Vec<Value> = Vec::new();
    let applied = (|| {
        if authorization.changed {
            transaction.begin_publication(&authorization_path)?;
            super::write_private(&authorization_path, authorization.value.as_str())?;
            transaction.finish_publication(&authorization_path, true)?;
        }
        transaction.begin_publication(&card_path)?;
        let card_outcome = publish(&card_path, &card)?;
        transaction.finish_publication(&card_path, card_outcome.changed)?;
        transaction.begin_publication(&registration.path)?;
        let registry_outcome = apply(&registration)?;
        transaction.finish_publication(&registration.path, registry_outcome.changed)?;
        let mut changed = authorization.changed || card_outcome.changed || registry_outcome.changed;
        if let Some(root) = &request.well_known {
            let path = root.join(".well-known").join(CARD_FILE);
            transaction.begin_publication(&path)?;
            let outcome = publish(&path, &card)?;
            transaction.finish_publication(&path, outcome.changed)?;
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
    if selection.rotated_gateway_key || authorization.changed {
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
        "authorization": {
            "scheme": "Bearer",
            "credential_file": authorization_path.display().to_string(),
            "permissions": "owner-only",
        },
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
        "securitySchemes": {
            "layerxLocalBearer": {
                "type": "http",
                "scheme": "bearer",
                "description": "Installation-local bearer credential delivered through the owner-only runtime credential file.",
            },
        },
        "security": [{"layerxLocalBearer": []}],
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
    authorization_path: &Path,
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
        "--authorization-file".to_owned(),
        authorization_path.display().to_string(),
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

struct PreparedAuthorization {
    value: Zeroizing<String>,
    changed: bool,
}

fn prepare_authorization(path: &Path, rotate: bool) -> Result<PreparedAuthorization, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => {
            super::private_file_metadata(path)?;
            if !rotate {
                return Ok(PreparedAuthorization {
                    value: read_authorization(path)?,
                    changed: false,
                });
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("could not inspect {}: {error}", path.display())),
    }
    let mut random = Zeroizing::new([0_u8; 32]);
    getrandom::fill(random.as_mut())
        .map_err(|error| format!("operating-system randomness failed: {error}"))?;
    Ok(PreparedAuthorization {
        value: Zeroizing::new(format!("lxa2a_{}", crate::encoding::hex_encode(random.as_ref()))),
        changed: true,
    })
}

pub(crate) fn read_authorization(path: &Path) -> Result<Zeroizing<String>, String> {
    let expected = super::private_file_metadata(path)?;
    let mut source = String::new();
    let file = File::open(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let opened = file
        .metadata()
        .map_err(|error| format!("could not inspect opened {}: {error}", path.display()))?;
    let current = super::private_file_metadata(path)?;
    if !super::same_file(&expected, &opened) || !super::same_file(&opened, &current) {
        return Err(format!("{} changed while its credential was opened", path.display()));
    }
    file
        .take((MAX_AUTHORIZATION_BYTES + 1) as u64)
        .read_to_string(&mut source)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    if source.len() > MAX_AUTHORIZATION_BYTES {
        source.zeroize();
        return Err("A2A authorization credential exceeds its bound".into());
    }
    let value = Zeroizing::new(source.trim_end_matches(['\r', '\n']).to_owned());
    source.zeroize();
    if !valid_authorization(&value) {
        return Err("A2A authorization credential is malformed".into());
    }
    Ok(value)
}

fn valid_authorization(value: &str) -> bool {
    value.len() == 70
        && value.starts_with("lxa2a_")
        && value[6..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(all(test, unix))]
mod authorization_file_tests {
    use std::fs;
    use std::os::unix::fs::{symlink, PermissionsExt as _};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::read_authorization;

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);
    const CREDENTIAL: &str =
        "lxa2a_0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn temporary_directory() -> PathBuf {
        std::env::temp_dir().join(format!(
            "layerx-a2a-auth-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn requires_private_current_owner_parent_and_leaf() -> Result<(), String> {
        let root = temporary_directory();
        let path = root.join("authorization");
        super::super::write_private(&path, CREDENTIAL)?;
        assert_eq!(read_authorization(&path)?.as_str(), CREDENTIAL);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644))
            .map_err(|error| error.to_string())?;
        assert!(read_authorization(&path).is_err());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .map_err(|error| error.to_string())?;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755))
            .map_err(|error| error.to_string())?;
        assert!(read_authorization(&path).is_err());
        fs::remove_dir_all(&root).map_err(|error| error.to_string())
    }

    #[test]
    fn rejects_a_symlinked_ancestor() -> Result<(), String> {
        let root = temporary_directory();
        let actual = root.join("actual");
        let path = actual.join("authorization");
        super::super::write_private(&path, CREDENTIAL)?;
        let alias = root.join("alias");
        symlink(&actual, &alias).map_err(|error| error.to_string())?;
        assert!(read_authorization(&alias.join("authorization")).is_err());
        fs::remove_dir_all(&root).map_err(|error| error.to_string())
    }
}

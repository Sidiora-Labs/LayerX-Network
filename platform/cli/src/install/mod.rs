pub mod a2a;
pub mod mcp;

use std::collections::BTreeMap;
use std::env;
use std::fs::{self, DirBuilder, File, Metadata, OpenOptions};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{json, Map, Value};
use sha2::{Digest as _, Sha256};
use zeroize::{Zeroize as _, Zeroizing};

use crate::config::{self, Configuration};
use crate::credential;
use crate::encoding::hex_encode;
use crate::http::Client;

pub const SERVER_NAME: &str = "layerx";

const SECRET_MARKERS: [&str; 9] = [
    "token",
    "api_key",
    "apikey",
    "secret",
    "password",
    "seed",
    "private_key",
    "authorization",
    "credential",
];
const MAX_INSTALLATION_SNAPSHOT_BYTES: usize = 16 * 1024 * 1024;

const HOSTS: [Host; 5] = [
    Host::Layerx,
    Host::ClaudeCode,
    Host::ClaudeDesktop,
    Host::Cursor,
    Host::VsCode,
];

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum Host {
    Layerx,
    ClaudeCode,
    ClaudeDesktop,
    Cursor,
    VsCode,
}

impl Host {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "layerx" => Ok(Self::Layerx),
            "claude-code" => Ok(Self::ClaudeCode),
            "claude-desktop" => Ok(Self::ClaudeDesktop),
            "cursor" => Ok(Self::Cursor),
            "vscode" => Ok(Self::VsCode),
            _ => Err(format!(
                "agent runtime {value} is not supported; use layerx, claude-code, claude-desktop, cursor, or vscode"
            )),
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Layerx => "layerx",
            Self::ClaudeCode => "claude-code",
            Self::ClaudeDesktop => "claude-desktop",
            Self::Cursor => "cursor",
            Self::VsCode => "vscode",
        }
    }

    pub const fn section(self) -> &'static str {
        match self {
            Self::VsCode => "servers",
            _ => "mcpServers",
        }
    }

    pub fn path(self) -> Result<PathBuf, String> {
        Ok(match self {
            Self::Layerx => layerx_directory()?.join("mcp.json"),
            Self::ClaudeCode => home()?.join(".claude.json"),
            Self::ClaudeDesktop => claude_desktop_directory()?.join("claude_desktop_config.json"),
            Self::Cursor => home()?.join(".cursor").join("mcp.json"),
            Self::VsCode => code_directory()?.join("User").join("mcp.json"),
        })
    }

    fn marker(self) -> Result<PathBuf, String> {
        Ok(match self {
            Self::Layerx => layerx_directory()?,
            Self::ClaudeCode => home()?.join(".claude"),
            Self::ClaudeDesktop => claude_desktop_directory()?,
            Self::Cursor => home()?.join(".cursor"),
            Self::VsCode => code_directory()?,
        })
    }

    pub fn entry(
        self,
        command: &str,
        arguments: &[String],
        variables: &BTreeMap<String, String>,
    ) -> Value {
        let mut entry = Map::new();
        if self == Self::VsCode {
            entry.insert("type".to_owned(), json!("stdio"));
        }
        entry.insert("command".to_owned(), json!(command));
        entry.insert("args".to_owned(), json!(arguments));
        entry.insert("env".to_owned(), json!(variables));
        Value::Object(entry)
    }
}

pub struct Registration {
    pub path: PathBuf,
    pub section: &'static str,
    pub name: String,
    pub entry: Value,
}

pub struct Outcome {
    pub action: &'static str,
    pub changed: bool,
}

struct Snapshot {
    path: PathBuf,
    bytes: Option<Zeroizing<Vec<u8>>>,
    original: Option<Metadata>,
    published: Option<Metadata>,
    publication_in_progress: bool,
}

pub struct FileTransaction {
    snapshots: Vec<Snapshot>,
}

impl FileTransaction {
    pub fn capture(paths: &[PathBuf]) -> Result<Self, String> {
        let mut snapshots = Vec::new();
        for path in paths {
            if snapshots
                .iter()
                .any(|snapshot: &Snapshot| snapshot.path == *path)
            {
                continue;
            }
            match open_stable_installation_file(path)? {
                Some((file, metadata)) => {
                    let mut bytes = Vec::new();
                    let mut source = file.take((MAX_INSTALLATION_SNAPSHOT_BYTES + 1) as u64);
                    source.read_to_end(&mut bytes).map_err(|error| {
                        format!("could not snapshot {}: {error}", path.display())
                    })?;
                    if bytes.len() > MAX_INSTALLATION_SNAPSHOT_BYTES {
                        bytes.zeroize();
                        return Err(format!(
                            "installation target {} exceeds its snapshot bound",
                            path.display()
                        ));
                    }
                    let file = source.into_inner();
                    let opened = file.metadata().map_err(|error| {
                        format!("could not inspect opened {}: {error}", path.display())
                    })?;
                    let current = fs::symlink_metadata(path).map_err(|error| {
                        format!("could not re-inspect {}: {error}", path.display())
                    })?;
                    if !stable_installation_metadata(&metadata, &opened)
                        || !stable_installation_metadata(&opened, &current)
                    {
                        bytes.zeroize();
                        return Err(format!("{} changed while it was read", path.display()));
                    }
                    snapshots.push(Snapshot {
                        path: path.clone(),
                        bytes: Some(Zeroizing::new(bytes)),
                        original: Some(metadata),
                        published: None,
                        publication_in_progress: false,
                    });
                }
                None => snapshots.push(Snapshot {
                    path: path.clone(),
                    bytes: None,
                    original: None,
                    published: None,
                    publication_in_progress: false,
                }),
            }
        }
        Ok(Self { snapshots })
    }

    pub fn begin_publication(&mut self, path: &Path) -> Result<(), String> {
        let snapshot = self
            .snapshots
            .iter_mut()
            .find(|snapshot| snapshot.path == path)
            .ok_or_else(|| format!("{} is outside the installation transaction", path.display()))?;
        if snapshot.publication_in_progress {
            return Err(format!(
                "{} already has an unresolved publication",
                path.display()
            ));
        }
        snapshot.publication_in_progress = true;
        Ok(())
    }

    pub fn finish_publication(&mut self, path: &Path, changed: bool) -> Result<(), String> {
        let observed = open_stable_installation_file(path)?.map(|(_, metadata)| metadata);
        let snapshot = self
            .snapshots
            .iter_mut()
            .find(|snapshot| snapshot.path == path)
            .ok_or_else(|| format!("{} is outside the installation transaction", path.display()))?;
        if !snapshot.publication_in_progress {
            return Err(format!(
                "{} has no pending installation publication",
                path.display()
            ));
        }
        if changed {
            snapshot.published = Some(observed.ok_or_else(|| {
                format!("{} vanished after installation publication", path.display())
            })?);
        } else {
            let expected = snapshot.published.as_ref().or(snapshot.original.as_ref());
            if !same_optional_installation_metadata(expected, observed.as_ref()) {
                return Err(format!(
                    "{} changed despite an unchanged publication",
                    path.display()
                ));
            }
        }
        snapshot.publication_in_progress = false;
        Ok(())
    }

    pub fn rollback(self) -> Result<(), String> {
        let mut failures = Vec::new();
        for snapshot in self.snapshots.into_iter().rev() {
            if snapshot.publication_in_progress {
                failures.push(format!(
                    "could not safely roll back {}: publication identity is unresolved",
                    snapshot.path.display()
                ));
                continue;
            }
            match (snapshot.bytes, snapshot.original, snapshot.published) {
                (Some(bytes), Some(original), Some(published)) => {
                    if let Err(error) = restore_snapshot(
                        &snapshot.path,
                        bytes.as_slice(),
                        &published,
                        original.permissions(),
                    ) {
                        failures.push(format!(
                            "could not restore {}: {error}",
                            snapshot.path.display()
                        ));
                    }
                }
                (Some(_), Some(original), None) => {
                    if let Err(error) =
                        require_expected_installation_leaf(&snapshot.path, Some(&original))
                    {
                        failures.push(format!(
                            "could not preserve unchanged {}: {error}",
                            snapshot.path.display()
                        ));
                    }
                }
                (None, None, Some(published)) => {
                    if let Err(error) = remove_created_file(&snapshot.path, &published) {
                        failures.push(format!(
                            "could not remove rolled-back {}: {error}",
                            snapshot.path.display()
                        ));
                    }
                }
                (None, None, None) => {
                    if let Err(error) = require_expected_installation_leaf(&snapshot.path, None) {
                        failures.push(format!(
                            "could not preserve absent {}: {error}",
                            snapshot.path.display()
                        ));
                    }
                }
                _ => failures.push(format!(
                    "could not safely roll back {}: captured state is inconsistent",
                    snapshot.path.display()
                )),
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; "))
        }
    }
}

fn open_stable_installation_file(path: &Path) -> Result<Option<(File, Metadata)>, String> {
    validate_existing_ancestors(path, false)?;
    let expected = match fs::symlink_metadata(path) {
        Ok(metadata)
            if metadata.is_file()
                && !metadata.file_type().is_symlink()
                && owned_by_current_user(&metadata) =>
        {
            metadata
        }
        Ok(_) => {
            return Err(format!(
                "installation target {} must be a current-owner regular non-symlink file",
                path.display()
            ))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("could not inspect {}: {error}", path.display())),
    };
    #[cfg(unix)]
    let file = {
        use rustix::fs::{open, Mode, OFlags};
        File::from(
            open(
                path,
                OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
                Mode::empty(),
            )
            .map_err(|error| format!("could not securely open {}: {error}", path.display()))?,
        )
    };
    #[cfg(not(unix))]
    let file =
        File::open(path).map_err(|error| format!("could not open {}: {error}", path.display()))?;
    let opened = file
        .metadata()
        .map_err(|error| format!("could not inspect opened {}: {error}", path.display()))?;
    let current = fs::symlink_metadata(path)
        .map_err(|error| format!("could not re-inspect {}: {error}", path.display()))?;
    if !stable_installation_metadata(&expected, &opened)
        || !stable_installation_metadata(&opened, &current)
    {
        return Err(format!("{} changed while it was opened", path.display()));
    }
    Ok(Some((file, expected)))
}

fn restore_snapshot(
    path: &Path,
    bytes: &[u8],
    expected_current: &Metadata,
    permissions: fs::Permissions,
) -> Result<(), String> {
    validate_existing_ancestors(path, false)?;
    require_expected_installation_leaf(path, Some(expected_current))?;
    let temporary = unique_temporary_path(path)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options
            .open(&temporary)
            .map_err(|error| format!("could not create {}: {error}", temporary.display()))?;
        file.write_all(bytes)
            .map_err(|error| format!("could not write {}: {error}", temporary.display()))?;
        file.set_permissions(permissions)
            .map_err(|error| format!("could not protect {}: {error}", temporary.display()))?;
        file.sync_all()
            .map_err(|error| format!("could not synchronize {}: {error}", temporary.display()))?;
        let temporary_metadata = file
            .metadata()
            .map_err(|error| format!("could not inspect {}: {error}", temporary.display()))?;
        validate_existing_ancestors(path, false)?;
        require_expected_installation_leaf(path, Some(expected_current))?;
        fs::rename(&temporary, path)
            .map_err(|error| format!("could not atomically restore {}: {error}", path.display()))?;
        let published = fs::symlink_metadata(path)
            .map_err(|error| format!("could not inspect restored {}: {error}", path.display()))?;
        if !stable_installation_metadata(&temporary_metadata, &published) {
            return Err(format!(
                "{} changed during rollback publication",
                path.display()
            ));
        }
        sync_parent(path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn remove_created_file(path: &Path, expected: &Metadata) -> Result<(), String> {
    validate_existing_ancestors(path, false)?;
    require_expected_installation_leaf(path, Some(expected))?;
    validate_existing_ancestors(path, false)?;
    require_expected_installation_leaf(path, Some(expected))?;
    fs::remove_file(path)
        .map_err(|error| format!("could not remove {}: {error}", path.display()))?;
    sync_parent(path)
}

fn installation_leaf_metadata(path: &Path) -> Result<Option<Metadata>, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata)
            if metadata.is_file()
                && !metadata.file_type().is_symlink()
                && owned_by_current_user(&metadata) =>
        {
            Ok(Some(metadata))
        }
        Ok(_) => Err(format!(
            "{} is not a current-owner regular non-symlink file",
            path.display()
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("could not inspect {}: {error}", path.display())),
    }
}

fn same_optional_installation_metadata(left: Option<&Metadata>, right: Option<&Metadata>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => stable_installation_metadata(left, right),
        (None, None) => true,
        _ => false,
    }
}

fn require_expected_installation_leaf(
    path: &Path,
    expected: Option<&Metadata>,
) -> Result<(), String> {
    let observed = installation_leaf_metadata(path)?;
    if same_optional_installation_metadata(expected, observed.as_ref()) {
        Ok(())
    } else {
        Err(format!(
            "{} changed since its transaction publication",
            path.display()
        ))
    }
}

#[cfg(unix)]
fn stable_installation_metadata(left: &Metadata, right: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    left.dev() == right.dev()
        && left.ino() == right.ino()
        && left.uid() == right.uid()
        && left.mode() == right.mode()
}

#[cfg(not(unix))]
fn stable_installation_metadata(left: &Metadata, right: &Metadata) -> bool {
    same_file(left, right) && left.permissions().readonly() == right.permissions().readonly()
}

fn unique_temporary_path(path: &Path) -> Result<PathBuf, String> {
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random)
        .map_err(|error| format!("could not name a private temporary file: {error}"))?;
    Ok(path.with_extension(format!("tmp-{}", hex_encode(&random))))
}

fn sync_parent(path: &Path) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", path.display()))?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("could not synchronize {}: {error}", parent.display()))
}

pub struct Selection {
    pub environment: String,
    pub endpoint: String,
    pub network_id: u32,
    pub key: String,
    pub did: String,
    pub public_key: String,
    pub created_key: bool,
    pub gateway_alias: String,
    pub gateway_key_id: String,
    pub gateway_scopes: Vec<String>,
    pub rotated_gateway_key: bool,
}

impl Selection {
    pub fn credentials(&self) -> Value {
        json!({
            "key": self.key,
            "did": self.did,
            "public_key": self.public_key,
            "created": self.created_key,
            "storage": "operating-system-credential-store",
            "scope": "one environment and one key",
            "written_to_disk": false,
            "gateway_alias": self.gateway_alias,
            "gateway_key_id": self.gateway_key_id,
            "gateway_scopes": self.gateway_scopes,
            "gateway_authorization": "LayerX-Key",
            "gateway_rotated": self.rotated_gateway_key,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GatewayEnvelope {
    ok: bool,
    key: GatewayKey,
}

#[derive(Deserialize)]
struct GatewayKey {
    id: String,
    secret: String,
    authorization_scheme: String,
    scopes: Vec<String>,
}

impl Drop for GatewayKey {
    fn drop(&mut self) {
        self.secret.zeroize();
    }
}

pub fn select(
    configuration: &mut Configuration,
    environment: Option<String>,
    key: Option<String>,
    fallback_key: &str,
    token_stdin: bool,
    component: &str,
    read_only: bool,
    rotate: bool,
) -> Result<Selection, String> {
    let environment = match environment {
        Some(name) => {
            Configuration::validate_environment_name(&name)?;
            name
        }
        None => configuration.current_environment.clone(),
    };
    let profile = configuration.environments.get(&environment).ok_or_else(|| {
        format!(
            "environment {environment} is not configured; run layerx environment use {environment} --endpoint <url> --network-id <id>"
        )
    })?;
    let endpoint = profile.endpoint.clone();
    let network_id = profile.network_id;
    if environment == "emulator" {
        return Err(
            "MCP and A2A installation require a configured hosted testnet or production gateway; the emulator does not expose self-service scoped keys or the production activity route"
                .into(),
        );
    }
    if token_stdin {
        credential::set_token(&environment)?;
    }
    let (name, created_key) = resolve_key(configuration, key, fallback_key)?;
    let metadata = configuration
        .keys
        .get(&name)
        .ok_or_else(|| format!("key {name} does not exist"))?;
    let mode = if read_only { "read" } else { "payment" };
    let gateway_alias = format!("{environment}:{component}:{mode}:{name}");
    let gateway_scopes = if read_only {
        vec!["receipt:read".to_owned()]
    } else {
        vec!["activity:write".to_owned(), "receipt:read".to_owned()]
    };
    let existing = credential::gateway(&gateway_alias)?;
    let existing_id = existing
        .as_ref()
        .and_then(|value| value.split_once(':').map(|(id, _)| id.to_owned()));
    let (gateway_key_id, rotated_gateway_key) = if let (Some(id), false) =
        (existing_id.clone(), rotate)
    {
        (id, false)
    } else {
        let token = credential::token(&environment)?.ok_or_else(|| {
                format!(
                    "no {environment} identity session is held in credential storage; pipe one in with --token-stdin or run layerx auth set --environment {environment}"
                )
            })?;
        let client = Client::new(&endpoint, Some(token))?;
        let idempotency = provisioning_idempotency(
            &environment,
            component,
            mode,
            &name,
            &metadata.public_key,
            existing_id.as_deref(),
        );
        let path = existing_id.as_ref().map_or_else(
            || "/v1/keys".to_owned(),
            |id| format!("/v1/keys/{id}/rotate"),
        );
        let body = if existing_id.is_some() {
            json!({})
        } else {
            json!({
                "signer_public_key": metadata.public_key,
                "scopes": gateway_scopes,
                "quota_requests": 3600,
                "quota_window_seconds": 3600,
            })
        };
        let (status, mut issued): (u16, GatewayEnvelope) =
            client.post_sensitive(&path, &body, &idempotency)?;
        if !issued.ok
            || issued.key.authorization_scheme != "LayerX-Key"
            || issued.key.scopes != gateway_scopes
        {
            if existing_id.is_none() {
                let _ = client.delete(&format!("/v1/keys/{}", issued.key.id));
            }
            return Err("gateway returned a key outside the requested installation scope".into());
        }
        let mut stored = Zeroizing::new(format!("{}:{}", issued.key.id, issued.key.secret));
        issued.key.secret.zeroize();
        if let Err(error) = credential::set_gateway(&gateway_alias, &mut stored) {
            if status == 201 {
                let _ = client.delete(&format!("/v1/keys/{}", issued.key.id));
            }
            if existing_id.is_some() {
                let _ = credential::delete_gateway(&gateway_alias);
            }
            return Err(error);
        }
        (issued.key.id.clone(), existing_id.is_some())
    };
    Ok(Selection {
        environment,
        endpoint,
        network_id,
        key: name.clone(),
        did: metadata.did.clone(),
        public_key: metadata.public_key.clone(),
        created_key,
        gateway_alias,
        gateway_key_id,
        gateway_scopes,
        rotated_gateway_key,
    })
}

fn provisioning_idempotency(
    environment: &str,
    component: &str,
    mode: &str,
    key: &str,
    public_key: &str,
    replacement: Option<&str>,
) -> String {
    let mut digest = Sha256::new();
    for value in [
        "layerx-install-gateway-key-v1",
        environment,
        component,
        mode,
        key,
        public_key,
        replacement.unwrap_or("new"),
    ] {
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value.as_bytes());
    }
    hex_encode(&digest.finalize())
}

fn resolve_key(
    configuration: &mut Configuration,
    key: Option<String>,
    fallback_key: &str,
) -> Result<(String, bool), String> {
    if let Some(name) = key {
        if !configuration.keys.contains_key(&name) {
            return Err(format!("key {name} does not exist"));
        }
        return Ok((name, false));
    }
    if let Some(name) = configuration.default_key.clone() {
        if configuration.keys.contains_key(&name) {
            return Ok((name, false));
        }
    }
    if configuration.keys.contains_key(fallback_key) {
        return Ok((fallback_key.to_owned(), false));
    }
    credential::create_key(configuration, fallback_key, None)?;
    Ok((fallback_key.to_owned(), true))
}

pub fn hosts(requested: &[String]) -> Result<Vec<Host>, String> {
    if requested.is_empty() {
        return detected();
    }
    let mut selected = Vec::with_capacity(requested.len());
    for value in requested {
        let host = Host::parse(value)?;
        if !selected.contains(&host) {
            selected.push(host);
        }
    }
    Ok(selected)
}

fn detected() -> Result<Vec<Host>, String> {
    let mut present = Vec::new();
    for host in HOSTS {
        if host == Host::Layerx || host.marker()?.exists() || host.path()?.exists() {
            present.push(host);
        }
    }
    Ok(present)
}

pub fn executable() -> Result<String, String> {
    let resolved = env::current_exe()
        .map_err(|error| format!("could not resolve the running executable: {error}"))?;
    let resolved = fs::canonicalize(&resolved).unwrap_or(resolved);
    resolved
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| "the running executable path is not valid UTF-8".to_string())
}

pub fn variables() -> Result<BTreeMap<String, String>, String> {
    let mut variables = BTreeMap::new();
    variables.insert(
        "LAYERX_CONFIG".to_owned(),
        config::path()?.display().to_string(),
    );
    Ok(variables)
}

pub fn apply(registration: &Registration) -> Result<Outcome, String> {
    reject_secret_material(&registration.entry)?;
    let mut document = match read_document(&registration.path)? {
        Some(Value::Object(map)) => map,
        Some(_) => {
            return Err(format!(
                "{} does not hold a JSON object",
                registration.path.display()
            ))
        }
        None => Map::new(),
    };
    let mut entries = match document.remove(registration.section) {
        Some(Value::Object(map)) => map,
        Some(_) => {
            return Err(format!(
                "{} does not hold {} as a JSON object",
                registration.path.display(),
                registration.section
            ))
        }
        None => Map::new(),
    };
    let identical = entries.get(&registration.name) == Some(&registration.entry);
    if identical && private(&registration.path)? {
        return Ok(Outcome {
            action: "unchanged",
            changed: false,
        });
    }
    let action = if entries.contains_key(&registration.name) {
        "updated"
    } else {
        "created"
    };
    entries.insert(registration.name.clone(), registration.entry.clone());
    document.insert(registration.section.to_owned(), Value::Object(entries));
    let encoded = serde_json::to_string_pretty(&Value::Object(document))
        .map_err(|error| format!("could not encode {}: {error}", registration.path.display()))?;
    write_private(&registration.path, &encoded)?;
    Ok(Outcome {
        action,
        changed: true,
    })
}

pub fn publish(path: &Path, document: &Value) -> Result<Outcome, String> {
    reject_secret_material(document)?;
    let current = read_document(path)?;
    if current.as_ref() == Some(document) && private(path)? {
        return Ok(Outcome {
            action: "unchanged",
            changed: false,
        });
    }
    let action = if current.is_some() {
        "updated"
    } else {
        "created"
    };
    let encoded = serde_json::to_string_pretty(document)
        .map_err(|error| format!("could not encode {}: {error}", path.display()))?;
    write_private(path, &encoded)?;
    Ok(Outcome {
        action,
        changed: true,
    })
}

pub fn report(path: &Path, section: &str, name: &str, outcome: &Outcome) -> Value {
    json!({
        "path": path.display().to_string(),
        "section": section,
        "name": name,
        "action": outcome.action,
        "changed": outcome.changed,
        "permissions": "owner-only",
    })
}

fn read_document(path: &Path) -> Result<Option<Value>, String> {
    match fs::read_to_string(path) {
        Ok(source) if source.trim().is_empty() => Ok(None),
        Ok(source) => serde_json::from_str(&source)
            .map(Some)
            .map_err(|error| format!("could not parse {}: {error}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("could not read {}: {error}", path.display())),
    }
}

fn reject_secret_material(value: &Value) -> Result<(), String> {
    match value {
        Value::Object(map) => {
            for (key, nested) in map {
                let lowered = key.to_ascii_lowercase();
                if SECRET_MARKERS.iter().any(|marker| lowered.contains(marker)) {
                    return Err(format!(
                        "installed configuration may not carry the field {key}; secrets stay in credential storage"
                    ));
                }
                reject_secret_material(nested)?;
            }
            Ok(())
        }
        Value::Array(items) => {
            for item in items {
                reject_secret_material(item)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn write_private(path: &Path, contents: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", path.display()))?;
    validate_existing_ancestors(path, false)?;
    ensure_directory(parent)?;
    #[cfg(unix)]
    fs::set_permissions(parent, std::os::unix::fs::PermissionsExt::from_mode(0o700))
        .map_err(|error| format!("could not protect {}: {error}", parent.display()))?;
    validate_existing_ancestors(path, true)?;
    let temporary = unique_temporary_path(path)?;
    let result = write_temporary(&temporary, path, contents);
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
        return result;
    }
    if private_file_metadata(path).is_ok() {
        Ok(())
    } else {
        Err(format!(
            "{} is readable beyond its owner after installation",
            path.display()
        ))
    }
}

fn write_temporary(temporary: &Path, path: &Path, contents: &str) -> Result<(), String> {
    let before = installation_leaf_metadata(path)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options
        .open(temporary)
        .map_err(|error| format!("could not create {}: {error}", temporary.display()))?;
    file.write_all(contents.as_bytes())
        .and_then(|()| file.write_all(b"\n"))
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("could not write {}: {error}", temporary.display()))?;
    let temporary_metadata = file
        .metadata()
        .map_err(|error| format!("could not inspect {}: {error}", temporary.display()))?;
    validate_existing_ancestors(path, true)?;
    let after = installation_leaf_metadata(path)?;
    if !same_optional_installation_metadata(before.as_ref(), after.as_ref()) {
        return Err(format!(
            "{} changed while publication was prepared",
            path.display()
        ));
    }
    fs::rename(temporary, path)
        .map_err(|error| format!("could not replace {}: {error}", path.display()))?;
    let published = fs::symlink_metadata(path)
        .map_err(|error| format!("could not inspect published {}: {error}", path.display()))?;
    if !stable_installation_metadata(&temporary_metadata, &published) {
        return Err(format!("{} changed during publication", path.display()));
    }
    sync_parent(path)
}

fn ensure_directory(path: &Path) -> Result<(), String> {
    let mut builder = DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }
    builder
        .create(path)
        .map_err(|error| format!("could not create {}: {error}", path.display()))
}

fn private(path: &Path) -> Result<bool, String> {
    match private_file_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.contains("does not exist") => Ok(false),
        Err(error) => Err(error),
    }
}

pub(super) fn private_file_metadata(path: &Path) -> Result<Metadata, String> {
    validate_existing_ancestors(path, true)?;
    match fs::symlink_metadata(path) {
        Ok(metadata)
            if metadata.is_file()
                && !metadata.file_type().is_symlink()
                && owner_only(&metadata)
                && owned_by_current_user(&metadata) =>
        {
            Ok(metadata)
        }
        Ok(_) => Err(format!(
            "{} must be a regular owner-only file owned by the current user",
            path.display()
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err(format!("{} does not exist", path.display()))
        }
        Err(error) => Err(format!("could not inspect {}: {error}", path.display())),
    }
}

pub(super) fn validate_existing_ancestors(path: &Path, private_parent: bool) -> Result<(), String> {
    if !path.is_absolute() {
        return Err(format!("{} must be an absolute path", path.display()));
    }
    let direct_parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", path.display()))?;
    let mut current = Some(direct_parent);
    while let Some(directory) = current {
        match fs::symlink_metadata(directory) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(format!(
                        "{} has a non-directory or symlink ancestor {}",
                        path.display(),
                        directory.display()
                    ));
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt as _;
                    if metadata.mode() & 0o022 != 0
                        && (metadata.mode() & 0o1000 == 0 || metadata.uid() != 0)
                    {
                        return Err(format!(
                            "{} has a group- or world-writable ancestor {}",
                            path.display(),
                            directory.display()
                        ));
                    }
                    if private_parent
                        && directory == direct_parent
                        && (!owned_by_current_user(&metadata) || metadata.mode() & 0o077 != 0)
                    {
                        return Err(format!(
                            "{} must have an owner-only parent directory owned by the current user",
                            path.display()
                        ));
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "could not inspect {}: {error}",
                    directory.display()
                ))
            }
        }
        current = directory.parent();
    }
    Ok(())
}

#[cfg(unix)]
fn owned_by_current_user(metadata: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    metadata.uid() == rustix::process::geteuid().as_raw()
}

#[cfg(not(unix))]
const fn owned_by_current_user(_metadata: &Metadata) -> bool {
    true
}

#[cfg(unix)]
pub(super) fn same_file(left: &Metadata, right: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(not(unix))]
pub(super) fn same_file(left: &Metadata, right: &Metadata) -> bool {
    left.len() == right.len()
        && left.modified().ok().is_some()
        && left.modified().ok() == right.modified().ok()
}

#[cfg(unix)]
#[allow(clippy::verbose_bit_mask)]
fn owner_only(metadata: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    metadata.mode() & 0o077 == 0
}

#[cfg(not(unix))]
const fn owner_only(_metadata: &Metadata) -> bool {
    true
}

pub fn layerx_directory() -> Result<PathBuf, String> {
    let path = config::path()?;
    path.parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "the CLI configuration path has no parent directory".to_string())
}

fn home() -> Result<PathBuf, String> {
    if let Some(root) = env::var_os("LAYERX_INSTALL_ROOT") {
        return Ok(PathBuf::from(root));
    }
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "HOME is required to locate agent runtime configuration".to_string())
}

fn xdg_config() -> Result<PathBuf, String> {
    if env::var_os("LAYERX_INSTALL_ROOT").is_some() {
        return Ok(home()?.join(".config"));
    }
    if let Some(base) = env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(base));
    }
    Ok(home()?.join(".config"))
}

fn application_support() -> Result<PathBuf, String> {
    Ok(home()?.join("Library").join("Application Support"))
}

fn claude_desktop_directory() -> Result<PathBuf, String> {
    if cfg!(target_os = "macos") {
        application_support().map(|base| base.join("Claude"))
    } else {
        xdg_config().map(|base| base.join("Claude"))
    }
}

fn code_directory() -> Result<PathBuf, String> {
    if cfg!(target_os = "macos") {
        application_support().map(|base| base.join("Code"))
    } else {
        xdg_config().map(|base| base.join("Code"))
    }
}

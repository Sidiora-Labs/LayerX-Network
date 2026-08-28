use std::collections::BTreeMap;
use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const CONFIG_VERSION: u16 = 1;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Environment {
    pub endpoint: String,
    pub network_id: u32,
    #[serde(default)]
    pub sequencer_trust_anchor: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KeyMetadata {
    pub did: String,
    pub public_key: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Configuration {
    version: u16,
    pub current_environment: String,
    pub default_key: Option<String>,
    pub environments: BTreeMap<String, Environment>,
    pub keys: BTreeMap<String, KeyMetadata>,
}

impl Default for Configuration {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            current_environment: "emulator".into(),
            default_key: None,
            environments: BTreeMap::from([(
                "emulator".into(),
                Environment {
                    endpoint: "http://127.0.0.1:9402".into(),
                    network_id: 402,
                    sequencer_trust_anchor: None,
                },
            )]),
            keys: BTreeMap::new(),
        }
    }
}

impl Configuration {
    pub fn load() -> Result<Self, String> {
        let path = path()?;
        let source = match fs::read_to_string(&path) {
            Ok(source) => source,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default())
            }
            Err(error) => return Err(format!("could not read {}: {error}", path.display())),
        };
        let parsed: Self = serde_json::from_str(&source)
            .map_err(|error| format!("could not parse {}: {error}", path.display()))?;
        if parsed.version != CONFIG_VERSION {
            return Err(format!(
                "unsupported CLI configuration version {}",
                parsed.version
            ));
        }
        parsed.active_environment()?;
        Ok(parsed)
    }

    pub fn save(&self) -> Result<(), String> {
        let path = path()?;
        let parent = path
            .parent()
            .ok_or_else(|| "configuration path has no parent directory".to_string())?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
        let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
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
            let encoded = serde_json::to_vec_pretty(self)
                .map_err(|error| format!("could not encode configuration: {error}"))?;
            file.write_all(&encoded)
                .and_then(|()| file.write_all(b"\n"))
                .and_then(|()| file.sync_all())
                .map_err(|error| format!("could not write {}: {error}", temporary.display()))?;
            fs::rename(&temporary, &path)
                .map_err(|error| format!("could not replace {}: {error}", path.display()))
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }

    pub fn active_environment(&self) -> Result<(&str, &Environment), String> {
        self.environments
            .get_key_value(&self.current_environment)
            .map(|(name, environment)| (name.as_str(), environment))
            .ok_or_else(|| {
                format!(
                    "current environment {} has no configuration",
                    self.current_environment
                )
            })
    }

    pub fn validate_environment_name(name: &str) -> Result<(), String> {
        match name {
            "emulator" | "testnet" | "production" => Ok(()),
            _ => Err("environment must be emulator, testnet, or production".into()),
        }
    }
}

pub fn path() -> Result<PathBuf, String> {
    if let Some(explicit) = env::var_os("LAYERX_CONFIG") {
        return absolute(PathBuf::from(explicit));
    }
    if let Some(base) = env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(base).join("layerx/config.json"));
    }
    let base = env::var_os("HOME").ok_or_else(|| {
        "HOME or XDG_CONFIG_HOME is required to locate CLI configuration".to_string()
    })?;
    Ok(PathBuf::from(base).join(".config/layerx/config.json"))
}

fn absolute(path: PathBuf) -> Result<PathBuf, String> {
    if path.is_absolute() {
        return Ok(path);
    }
    env::current_dir()
        .map(|current| current.join(path))
        .map_err(|error| format!("could not resolve configuration path: {error}"))
}

pub fn ensure_directory_empty(path: &Path) -> Result<(), String> {
    match fs::read_dir(path) {
        Ok(mut entries) => {
            if entries.next().is_none() {
                Ok(())
            } else {
                Err(format!(
                    "{} already exists and is not empty",
                    path.display()
                ))
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("could not inspect {}: {error}", path.display())),
    }
}

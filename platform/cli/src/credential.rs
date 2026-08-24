use std::io::{self, Read as _};
use std::sync::OnceLock;

use ed25519_dalek::SigningKey;
use keyring_core::Entry;
use zeroize::{Zeroize as _, Zeroizing};

use crate::config::{Configuration, KeyMetadata};
use crate::encoding::{fixed_hex, hex_encode};

const SERVICE: &str = "dev.layerx.cli";
const MAX_STDIN_SECRET_BYTES: u64 = 16 * 1024;

/// Environment variable that swaps the credential store for the in-memory
/// keyring-core mock store. It exists so the command suite can be driven end to
/// end on a headless machine without a live operating-system keychain; it is
/// never consulted in normal operation, where the native store is used.
const MOCK_STORE_VARIABLE: &str = "LAYERX_CREDENTIAL_STORE";
const MOCK_STORE_VALUE: &str = "mock";

/// Installs the process-wide credential store exactly once.
///
/// In normal operation this defers to the keyring v1 initialiser, which selects
/// and installs the operating-system credential store (Keychain Services on
/// macOS, Credential Manager on Windows, the Secret Service on other Unix
/// systems). When `LAYERX_CREDENTIAL_STORE=mock` is set, the non-persistent
/// keyring-core mock store is installed instead so tests never touch a real
/// keychain. Secrets are always held by the store, never written to config.
fn ensure_store() -> Result<(), String> {
    static STORE: OnceLock<Result<(), String>> = OnceLock::new();
    STORE.get_or_init(install_store).clone()
}

fn install_store() -> Result<(), String> {
    if matches!(
        std::env::var(MOCK_STORE_VARIABLE).as_deref(),
        Ok(MOCK_STORE_VALUE)
    ) {
        let store = keyring_core::mock::Store::new().map_err(|error| {
            format!("could not initialise the in-memory test credential store: {error}")
        })?;
        keyring_core::set_default_store(store);
        return Ok(());
    }
    if let Err(error) = keyring::Entry::store_status() {
        return Err(format!(
            "operating-system credential storage is unavailable: {error}"
        ));
    }
    Ok(())
}

fn entry(kind: &str, name: &str) -> Result<Entry, String> {
    ensure_store()?;
    Entry::new(SERVICE, &format!("{kind}:{name}"))
        .map_err(|error| format!("operating-system credential storage is unavailable: {error}"))
}

fn read_secret() -> Result<Zeroizing<String>, String> {
    let mut value = String::new();
    io::stdin()
        .take(MAX_STDIN_SECRET_BYTES)
        .read_to_string(&mut value)
        .map_err(|error| format!("could not read secret from standard input: {error}"))?;
    let trimmed = value.trim().to_owned();
    value.zeroize();
    if trimmed.is_empty() {
        return Err("standard input did not contain a secret".into());
    }
    Ok(Zeroizing::new(trimmed))
}

pub fn create_key(
    configuration: &mut Configuration,
    name: &str,
    did: Option<String>,
) -> Result<KeyMetadata, String> {
    validate_name(name)?;
    if configuration.keys.contains_key(name) {
        return Err(format!("key {name} already exists"));
    }
    let mut seed = Zeroizing::new([0_u8; 32]);
    getrandom::fill(seed.as_mut())
        .map_err(|error| format!("operating-system randomness failed: {error}"))?;
    import_seed(configuration, name, did, *seed)
}

pub fn import_key(
    configuration: &mut Configuration,
    name: &str,
    did: Option<String>,
) -> Result<KeyMetadata, String> {
    validate_name(name)?;
    if configuration.keys.contains_key(name) {
        return Err(format!("key {name} already exists"));
    }
    let mut source = read_secret()?;
    let seed = fixed_hex::<32>("private seed", &source)?;
    source.zeroize();
    import_seed(configuration, name, did, seed)
}

fn import_seed(
    configuration: &mut Configuration,
    name: &str,
    did: Option<String>,
    seed: [u8; 32],
) -> Result<KeyMetadata, String> {
    let signing = SigningKey::from_bytes(&seed);
    let public_key = signing.verifying_key().to_bytes();
    drop(signing);
    let did = did.unwrap_or_else(|| format!("did:layerx:{}", hex_encode(&public_key)));
    if did.is_empty() || did.len() > 255 {
        return Err("DID must contain between 1 and 255 bytes".into());
    }
    let mut encoded = Zeroizing::new(hex_encode(&seed));
    entry("key", name)?
        .set_password(&encoded)
        .map_err(|error| {
            format!("could not save key in operating-system credential storage: {error}")
        })?;
    encoded.zeroize();
    let metadata = KeyMetadata {
        did,
        public_key: hex_encode(&public_key),
    };
    configuration.keys.insert(name.to_owned(), metadata.clone());
    if configuration.default_key.is_none() {
        configuration.default_key = Some(name.to_owned());
    }
    if let Err(error) = configuration.save() {
        let _ = entry("key", name).and_then(|value| {
            value
                .delete_credential()
                .map_err(|failure| failure.to_string())
        });
        return Err(error);
    }
    Ok(metadata)
}

pub fn delete_key(configuration: &mut Configuration, name: &str) -> Result<(), String> {
    if !configuration.keys.contains_key(name) {
        return Err(format!("key {name} does not exist"));
    }
    entry("key", name)?
        .delete_credential()
        .map_err(|error| format!("could not delete key from operating-system storage: {error}"))?;
    configuration.keys.remove(name);
    if configuration.default_key.as_deref() == Some(name) {
        configuration.default_key = configuration.keys.keys().next().cloned();
    }
    configuration.save()
}

pub fn set_default_key(configuration: &mut Configuration, name: &str) -> Result<(), String> {
    if !configuration.keys.contains_key(name) {
        return Err(format!("key {name} does not exist"));
    }
    configuration.default_key = Some(name.to_owned());
    configuration.save()
}

pub fn set_token(environment: &str) -> Result<(), String> {
    Configuration::validate_environment_name(environment)?;
    let mut token = read_secret()?;
    entry("token", environment)?
        .set_password(&token)
        .map_err(|error| {
            format!("could not save token in operating-system credential storage: {error}")
        })?;
    token.zeroize();
    Ok(())
}

pub fn delete_token(environment: &str) -> Result<(), String> {
    Configuration::validate_environment_name(environment)?;
    entry("token", environment)?
        .delete_credential()
        .map_err(|error| format!("could not delete token from operating-system storage: {error}"))
}

pub fn token(environment: &str) -> Result<Option<Zeroizing<String>>, String> {
    match entry("token", environment)?.get_password() {
        Ok(value) => Ok(Some(Zeroizing::new(value))),
        Err(keyring_core::Error::NoEntry) => Ok(None),
        Err(error) => Err(format!(
            "could not read token from operating-system credential storage: {error}"
        )),
    }
}

pub fn key_seed(name: &str) -> Result<Zeroizing<[u8; 32]>, String> {
    validate_name(name)?;
    let encoded = entry("key", name)?.get_password().map_err(|error| {
        format!("could not read signing key from operating-system storage: {error}")
    })?;
    let encoded = Zeroizing::new(encoded);
    fixed_hex::<32>("private seed", &encoded).map(Zeroizing::new)
}

pub fn set_gateway(alias: &str, credential: &mut Zeroizing<String>) -> Result<(), String> {
    validate_alias(alias)?;
    validate_gateway_credential(credential)?;
    entry("gateway", alias)?
        .set_password(credential)
        .map_err(|error| {
            format!("could not save gateway key in operating-system credential storage: {error}")
        })
}

pub fn gateway(alias: &str) -> Result<Option<Zeroizing<String>>, String> {
    validate_alias(alias)?;
    match entry("gateway", alias)?.get_password() {
        Ok(value) => {
            let value = Zeroizing::new(value);
            validate_gateway_credential(&value)?;
            Ok(Some(value))
        }
        Err(keyring_core::Error::NoEntry) => Ok(None),
        Err(error) => Err(format!(
            "could not read gateway key from operating-system credential storage: {error}"
        )),
    }
}

pub fn delete_gateway(alias: &str) -> Result<(), String> {
    validate_alias(alias)?;
    match entry("gateway", alias)?.delete_credential() {
        Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
        Err(error) => Err(format!(
            "could not delete gateway key from operating-system credential storage: {error}"
        )),
    }
}

fn validate_gateway_credential(value: &str) -> Result<(), String> {
    let (id, secret) = value
        .split_once(':')
        .ok_or_else(|| "gateway credential is malformed".to_owned())?;
    if id.is_empty()
        || id.len() > 64
        || !id.bytes().all(|byte| byte.is_ascii_alphanumeric())
        || !secret.starts_with("lxp_live_")
        || secret.len() != 73
    {
        return Err("gateway credential is malformed".to_owned());
    }
    Ok(())
}

fn validate_alias(alias: &str) -> Result<(), String> {
    if alias.is_empty()
        || alias.len() > 192
        || !alias
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':'))
    {
        return Err(
            "credential alias must be 1-192 ASCII letters, digits, dashes, underscores, or colons"
                .into(),
        );
    }
    Ok(())
}

fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 128
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("key name must be 1-128 ASCII letters, digits, dashes, or underscores".into());
    }
    Ok(())
}

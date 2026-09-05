//! The Ed25519 signing boundary behind `kms.layerx-internal.svc`.
//!
//! Keys are generated inside the service from the operating-system entropy
//! source, sealed at rest under the operator's seal secret, and never leave
//! the process: the API hands out an opaque handle and the public key, and
//! signs messages on request. Key creation is idempotent per
//! `Idempotency-Key` and purpose so a retried registration reuses the key it
//! already created.

use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;
use zeroize::Zeroizing;

use crate::base64;
use crate::http::{json, ok, refusal, Request, Response};
use crate::journal::Journal;
use crate::seal::SealKey;
use crate::secret::{random_hex, sha256_hex, unix_seconds, valid_hex, valid_token};

/// The domain label under which the seal keys are derived.
pub const SEAL_LABEL: &[u8] = b"layerx-kms-ed25519-seed";
/// Prefix of every key identifier the webhooks scheme accepts.
pub const KEY_ID_PREFIX: &str = "whk_";
/// Prefix of every handle this boundary issues.
pub const HANDLE_PREFIX: &str = "kms-ed25519-";
/// Largest message the boundary signs.
pub const MAX_MESSAGE_BYTES: usize = 64 * 1024;
const MAX_KEYS: usize = 100_000;
const MAX_PURPOSE_BYTES: usize = 64;

#[derive(Serialize, Deserialize, Clone)]
struct KeyRecord {
    key_id: String,
    handle: String,
    purpose: String,
    scope: String,
    request_digest: String,
    public_key: String,
    sealed_seed: String,
    created_at: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateKeyRequest {
    algorithm: String,
    purpose: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SignRequest {
    key_handle: String,
    algorithm: String,
    message: String,
}

#[derive(Serialize)]
struct KeyResponse<'a> {
    key_id: &'a str,
    handle: &'a str,
    public_key: String,
}

#[derive(Serialize)]
struct SignatureResponse {
    signature: String,
}

#[derive(Serialize)]
struct Readiness {
    ready: bool,
    ed25519_non_exportable: bool,
}

/// The sealed key store.
pub struct KeyStore {
    journal: Journal,
    seal: SealKey,
    by_handle: BTreeMap<String, KeyRecord>,
    by_scope: BTreeMap<String, String>,
    by_key_id: BTreeMap<String, String>,
}

/// Outcome of a create request.
pub enum Created {
    /// A key was generated for a new scope.
    Fresh(KeyView),
    /// The scope had already created this key with the same request.
    Existing(KeyView),
    /// The scope already created a key with a different request body.
    Conflict,
}

/// The public view of a key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyView {
    pub key_id: String,
    pub handle: String,
    pub public_key: [u8; 32],
}

impl KeyStore {
    /// Opens the store under `state_dir`, replaying and authenticating every
    /// sealed key.
    ///
    /// # Errors
    /// Returns a description when the journal is unusable or a sealed key
    /// fails authentication under the configured seal secret.
    pub fn open(state_dir: &Path, seal: SealKey) -> Result<Self, String> {
        let mut records = Vec::new();
        let journal = Journal::open::<KeyRecord>(state_dir, |record| records.push(record))?;
        let mut store = Self {
            journal,
            seal,
            by_handle: BTreeMap::new(),
            by_scope: BTreeMap::new(),
            by_key_id: BTreeMap::new(),
        };
        for record in records {
            let signing = store.open_key(&record)?;
            if signing.verifying_key().to_bytes() != decode_public(&record.public_key)? {
                return Err(format!(
                    "journaled key {} does not match its sealed seed",
                    record.key_id
                ));
            }
            store.index(record);
        }
        Ok(store)
    }

    /// Number of keys held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_handle.len()
    }

    /// True when no key is held.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_handle.is_empty()
    }

    /// Proves the state directory accepts durable writes.
    ///
    /// # Errors
    /// Returns a description when the readiness marker cannot be synced.
    pub fn probe_writable(&self) -> Result<(), String> {
        self.journal.probe_writable()
    }

    fn index(&mut self, record: KeyRecord) {
        self.by_scope
            .insert(record.scope.clone(), record.handle.clone());
        self.by_key_id
            .insert(record.key_id.clone(), record.handle.clone());
        self.by_handle.insert(record.handle.clone(), record);
    }

    fn open_key(&self, record: &KeyRecord) -> Result<SigningKey, String> {
        let seed = self.seal.open(&record.sealed_seed)?;
        let seed: [u8; 32] = seed
            .as_slice()
            .try_into()
            .map_err(|_| "sealed seed has an invalid length".to_owned())?;
        let seed = Zeroizing::new(seed);
        Ok(SigningKey::from_bytes(&seed))
    }

    /// Creates, or returns the already created, Ed25519 key for `scope`.
    ///
    /// # Errors
    /// Returns a description when entropy, sealing or journaling fails.
    pub fn create(
        &mut self,
        scope: &str,
        purpose: &str,
        request_digest: &str,
    ) -> Result<Created, String> {
        if let Some(handle) = self.by_scope.get(scope) {
            let record = self
                .by_handle
                .get(handle)
                .ok_or_else(|| "scope index is inconsistent".to_owned())?;
            if record.request_digest == request_digest {
                return Ok(Created::Existing(view(record)?));
            }
            return Ok(Created::Conflict);
        }
        if self.by_handle.len() >= MAX_KEYS {
            return Err("key store is full".to_owned());
        }
        let mut seed = Zeroizing::new([0_u8; 32]);
        getrandom::fill(seed.as_mut()).map_err(|_| "entropy is unavailable".to_owned())?;
        let signing = SigningKey::from_bytes(&seed);
        let record = KeyRecord {
            key_id: format!("{KEY_ID_PREFIX}{}", random_hex(12)?.as_str()),
            handle: format!("{HANDLE_PREFIX}{}", random_hex(16)?.as_str()),
            purpose: purpose.to_owned(),
            scope: scope.to_owned(),
            request_digest: request_digest.to_owned(),
            public_key: crate::secret::hex(&signing.verifying_key().to_bytes()),
            sealed_seed: self.seal.seal(seed.as_slice())?,
            created_at: unix_seconds()?,
        };
        if self.by_key_id.contains_key(&record.key_id) || self.by_handle.contains_key(&record.handle)
        {
            return Err("generated identifiers collided".to_owned());
        }
        self.journal.append(&record)?;
        let created = view(&record)?;
        self.index(record);
        Ok(Created::Fresh(created))
    }

    /// Looks a key up by identifier.
    #[must_use]
    pub fn lookup(&self, key_id: &str) -> Option<KeyView> {
        let handle = self.by_key_id.get(key_id)?;
        self.by_handle.get(handle).and_then(|record| view(record).ok())
    }

    /// Signs `message` under the key behind `handle`.
    ///
    /// # Errors
    /// Returns `Ok(None)` for an unknown handle and a description when the
    /// sealed key cannot be opened or no longer matches its public key.
    pub fn sign(&self, handle: &str, message: &[u8]) -> Result<Option<[u8; 64]>, String> {
        let Some(record) = self.by_handle.get(handle) else {
            return Ok(None);
        };
        let signing = self.open_key(record)?;
        let public = VerifyingKey::from_bytes(&decode_public(&record.public_key)?)
            .map_err(|error| error.to_string())?;
        if signing.verifying_key() != public {
            return Err("sealed key does not match its public key".to_owned());
        }
        let signature = signing.sign(message);
        public
            .verify(message, &signature)
            .map_err(|error| format!("signature self-check failed: {error}"))?;
        Ok(Some(signature.to_bytes()))
    }
}

fn view(record: &KeyRecord) -> Result<KeyView, String> {
    Ok(KeyView {
        key_id: record.key_id.clone(),
        handle: record.handle.clone(),
        public_key: decode_public(&record.public_key)?,
    })
}

fn decode_public(value: &str) -> Result<[u8; 32], String> {
    if !valid_hex(value, 32) {
        return Err("journaled public key is not 32 hex bytes".to_owned());
    }
    crate::secret::unhex(value)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| "journaled public key is malformed".to_owned())
}

/// The service state a listener shares across connections.
pub struct Service {
    store: Mutex<KeyStore>,
    token: Zeroizing<String>,
    require_peer_certificate: bool,
}

impl Service {
    /// Wraps an open store with its bearer token and peer policy.
    #[must_use]
    pub fn new(store: KeyStore, token: Zeroizing<String>, require_peer_certificate: bool) -> Self {
        Self {
            store: Mutex::new(store),
            token,
            require_peer_certificate,
        }
    }

    fn authenticate(&self, request: &Request) -> Option<Response> {
        if self.require_peer_certificate && !request.peer_verified {
            return Some(refusal(401, "client_certificate_required", None));
        }
        if !request.bearer_matches(self.token.as_str()) {
            return Some(refusal(401, "unauthorized", None));
        }
        None
    }

    fn readiness(&self) -> Response {
        let ready = self
            .store
            .lock()
            .map_or(false, |store| store.probe_writable().is_ok());
        json(
            if ready { 200 } else { 503 },
            &Readiness {
                ready,
                ed25519_non_exportable: true,
            },
        )
    }

    fn create_key(&self, request: &Request) -> Response {
        if let Some(refused) = self.authenticate(request) {
            return refused;
        }
        let Some(idempotency) = request
            .headers
            .get("idempotency-key")
            .filter(|value| valid_token(value, 128))
        else {
            return refusal(400, "idempotency_key_required", None);
        };
        if !request.json_body() {
            return refusal(400, "invalid_request", None);
        }
        let Ok(body) = serde_json::from_slice::<CreateKeyRequest>(&request.body) else {
            return refusal(400, "invalid_request", None);
        };
        if body.algorithm != "ed25519" {
            return refusal(422, "unsupported_algorithm", None);
        }
        if !valid_token(&body.purpose, MAX_PURPOSE_BYTES) {
            return refusal(422, "invalid_purpose", None);
        }
        let scope = format!("{}:{idempotency}", body.purpose);
        let digest = sha256_hex(&request.body);
        let Ok(mut store) = self.store.lock() else {
            return refusal(503, "dependency_unavailable", Some(5));
        };
        match store.create(&scope, &body.purpose, &digest) {
            Ok(Created::Fresh(key)) => json(201, &key_response(&key)),
            Ok(Created::Existing(key)) => json(200, &key_response(&key)),
            Ok(Created::Conflict) => refusal(409, "idempotency_conflict", None),
            Err(error) => {
                eprintln!("layerx-kms create failed: {error}");
                refusal(503, "dependency_unavailable", Some(5))
            }
        }
    }

    fn get_key(&self, request: &Request, key_id: &str) -> Response {
        if let Some(refused) = self.authenticate(request) {
            return refused;
        }
        if !valid_token(key_id, 64) {
            return refusal(404, "unknown_key", None);
        }
        let Ok(store) = self.store.lock() else {
            return refusal(503, "dependency_unavailable", Some(5));
        };
        store.lookup(key_id).map_or_else(
            || refusal(404, "unknown_key", None),
            |key| json(200, &key_response(&key)),
        )
    }

    fn sign(&self, request: &Request) -> Response {
        if let Some(refused) = self.authenticate(request) {
            return refused;
        }
        if !request.json_body() {
            return refusal(400, "invalid_request", None);
        }
        let Ok(body) = serde_json::from_slice::<SignRequest>(&request.body) else {
            return refusal(400, "invalid_request", None);
        };
        if body.algorithm != "ed25519" {
            return refusal(422, "unsupported_algorithm", None);
        }
        if !valid_token(&body.key_handle, 512) {
            return refusal(404, "unknown_key", None);
        }
        let Some(message) = base64::decode(&body.message) else {
            return refusal(400, "invalid_message", None);
        };
        if message.len() > MAX_MESSAGE_BYTES {
            return refusal(413, "message_too_large", None);
        }
        let Ok(store) = self.store.lock() else {
            return refusal(503, "dependency_unavailable", Some(5));
        };
        match store.sign(&body.key_handle, &message) {
            Ok(Some(signature)) => ok(serde_json::json!(SignatureResponse {
                signature: base64::encode(&signature),
            })
            .to_string()),
            Ok(None) => refusal(404, "unknown_key", None),
            Err(error) => {
                eprintln!("layerx-kms sign failed: {error}");
                refusal(503, "dependency_unavailable", Some(5))
            }
        }
    }

    /// Routes one request.
    #[must_use]
    pub fn route(&self, request: &Request) -> Response {
        match (request.method.as_str(), request.path.as_str()) {
            ("GET", "/livez") => ok(r#"{"alive":true}"#.to_owned()),
            ("GET", "/readyz") => self.readiness(),
            ("POST", "/v1/signing-keys") => self.create_key(request),
            ("POST", "/v1/signatures") => self.sign(request),
            ("GET", path) => path.strip_prefix("/v1/signing-keys/").map_or_else(
                || refusal(404, "not_found", None),
                |key_id| self.get_key(request, key_id),
            ),
            (_, "/livez" | "/readyz" | "/v1/signing-keys" | "/v1/signatures") => {
                refusal(405, "method_not_allowed", None)
            }
            _ => refusal(404, "not_found", None),
        }
    }
}

fn key_response(key: &KeyView) -> KeyResponse<'_> {
    KeyResponse {
        key_id: &key.key_id,
        handle: &key.handle,
        public_key: base64::encode(&key.public_key),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn temporary_directory(name: &str) -> PathBuf {
        let directory = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join("kms")
            .join(format!("{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        directory
    }

    #[test]
    fn keys_are_idempotent_sealed_and_durable() {
        let directory = temporary_directory("durable");
        let seal = || SealKey::derive(SEAL_LABEL, b"seal secret");
        let mut store = KeyStore::open(&directory, seal()).unwrap_or_else(|error| panic!("{error}"));
        let Created::Fresh(first) = store
            .create("layerx-webhook-v1:register:abc", "layerx-webhook-v1", "digest-a")
            .unwrap_or_else(|error| panic!("{error}"))
        else {
            panic!("first create must be fresh");
        };
        assert!(first.key_id.starts_with(KEY_ID_PREFIX));
        assert!(first.handle.starts_with(HANDLE_PREFIX));
        let Created::Existing(again) = store
            .create("layerx-webhook-v1:register:abc", "layerx-webhook-v1", "digest-a")
            .unwrap_or_else(|error| panic!("{error}"))
        else {
            panic!("repeat must return the existing key");
        };
        assert_eq!(first, again);
        assert!(matches!(
            store.create("layerx-webhook-v1:register:abc", "layerx-webhook-v1", "digest-b"),
            Ok(Created::Conflict)
        ));
        let signature = store
            .sign(&first.handle, b"message")
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("handle must sign"));
        VerifyingKey::from_bytes(&first.public_key)
            .unwrap_or_else(|error| panic!("{error}"))
            .verify(b"message", &ed25519_dalek::Signature::from_bytes(&signature))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            store
                .sign("kms-ed25519-missing", b"message")
                .unwrap_or_else(|error| panic!("{error}")),
            None
        );
        let journal = fs::read_to_string(directory.join("journal.log"))
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(journal.contains(&crate::secret::hex(&first.public_key)));
        assert!(journal.contains("\"sealed_seed\":\""));
        assert!(!journal.contains("\"seed\":"));
        drop(store);
        let reopened = KeyStore::open(&directory, seal()).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(reopened.len(), 1);
        assert_eq!(reopened.lookup(&first.key_id), Some(first.clone()));
        assert!(KeyStore::open(&directory, SealKey::derive(SEAL_LABEL, b"other secret")).is_err());
        let _ = fs::remove_dir_all(&directory);
    }
}

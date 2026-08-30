//! Typed production authentication foundations. This module defines no JSON RPC.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::MetadataExt as _;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use getrandom::fill as random_fill;
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use subtle::ConstantTimeEq as _;
use zeroize::{Zeroize as _, Zeroizing};

use super::backend::{ApiFailure, PrincipalContext};
use super::schema::{AuthorizationClass, Operation};
use crate::auth::{
    AccessDecision, AuthError, AuthorizationRequest, OperationClass, OperationDigest, Passkeys,
    SessionContext, SessionGrant, StepUpEvidence,
};
use crate::security::{
    AuthenticatorMethod, AuthenticatorProvider, AuthenticatorSetupChallenge,
    AuthenticatorSetupResult, AuthenticatorStatus, BackupCodeSet, RecoveryEvidenceProvider,
    SecurityBoundaryError, TimedSecret,
};
use crate::store::{AgentTenantId, PrincipalId, PrincipalStore, StoreError};

const MAGIC: &[u8; 4] = b"LXAI";
const FILE: &str = "authentication.index";
const TEMP: &str = "authentication.index.tmp";
const DOMAIN: &[u8] = b"layerx-human-execution-capability/v1";
const PROVIDER_MAGIC: &[u8; 4] = b"LXSP";

pub struct IndexAuthenticationKey(Zeroizing<[u8; 32]>);
impl IndexAuthenticationKey {
    pub fn new(value: [u8; 32]) -> Result<Self, ProductionAuthError> {
        if bool::from(value.ct_eq(&[0; 32])) {
            Err(ProductionAuthError::InvalidConfiguration)
        } else {
            Ok(Self(Zeroizing::new(value)))
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Registration = 1,
    Assertion = 2,
    Session = 3,
    StepUp = 4,
}
#[derive(Clone)]
struct Discovery {
    kind: Kind,
    principal: PrincipalId,
    expires_at: u64,
}
#[derive(Clone)]
struct StoredCapability {
    digest: OperationDigest,
    issued_at: u64,
    expires_at: u64,
}
#[derive(Clone, Default)]
struct State {
    discoveries: BTreeMap<String, Discovery>,
    capabilities: BTreeMap<[u8; 32], StoredCapability>,
}

/// MAC-authenticated discovery contains identifiers, never bearer credentials.
pub struct AuthDiscoveryIndex {
    root: PathBuf,
    key: IndexAuthenticationKey,
    state: Mutex<State>,
}
impl AuthDiscoveryIndex {
    pub fn open(
        root: impl AsRef<Path>,
        key: IndexAuthenticationKey,
    ) -> Result<Self, ProductionAuthError> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root)?;
        let metadata = fs::symlink_metadata(&root)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() || metadata.mode() & 0o022 != 0 {
            return Err(ProductionAuthError::InvalidConfiguration);
        }
        reclaim_temp(&root)?;
        let state = load(&root, &key)?;
        Ok(Self {
            root,
            key,
            state: Mutex::new(state),
        })
    }
    pub fn bind_registration(
        &self,
        id: &str,
        p: &PrincipalId,
        e: u64,
    ) -> Result<(), ProductionAuthError> {
        self.bind(Kind::Registration, id, p, e)
    }
    pub fn bind_assertion(
        &self,
        id: &str,
        p: &PrincipalId,
        e: u64,
    ) -> Result<(), ProductionAuthError> {
        self.bind(Kind::Assertion, id, p, e)
    }
    pub fn bind_step_up(
        &self,
        id: &str,
        p: &PrincipalId,
        e: u64,
    ) -> Result<(), ProductionAuthError> {
        self.bind(Kind::StepUp, id, p, e)
    }
    pub fn bind_account(&self, email: &str, p: &PrincipalId) -> Result<(), ProductionAuthError> {
        self.bind(Kind::Assertion, &account_key(email)?, p, u64::MAX)
    }
    pub fn bind_session(
        &self,
        g: &SessionGrant,
        p: &PrincipalId,
    ) -> Result<(), ProductionAuthError> {
        self.bind(Kind::Session, g.session_id(), p, g.refresh_expires_at())
    }
    pub fn resolve_assertion(
        &self,
        id: &str,
        now: u64,
    ) -> Result<PrincipalId, ProductionAuthError> {
        self.resolve(Kind::Assertion, id, now)
    }
    pub fn resolve_registration(
        &self,
        id: &str,
        now: u64,
    ) -> Result<PrincipalId, ProductionAuthError> {
        self.resolve(Kind::Registration, id, now)
    }
    pub fn resolve_step_up(&self, id: &str, now: u64) -> Result<PrincipalId, ProductionAuthError> {
        self.resolve(Kind::StepUp, id, now)
    }
    pub fn resolve_account(&self, email: &str) -> Result<PrincipalId, ProductionAuthError> {
        self.resolve(Kind::Assertion, &account_key(email)?, 0)
    }
    pub(crate) fn resolve_access_token(
        &self,
        token: &str,
        now: u64,
    ) -> Result<PrincipalId, ProductionAuthError> {
        let id = token
            .split_once('.')
            .map(|v| v.0)
            .filter(|v| v.starts_with("ses_"))
            .ok_or(ProductionAuthError::Unauthenticated)?;
        self.resolve(Kind::Session, id, now)
    }
    pub(crate) fn active_principals(
        &self,
        now: u64,
    ) -> Result<Vec<PrincipalId>, ProductionAuthError> {
        let state = self
            .state
            .lock()
            .map_err(|_| ProductionAuthError::Unavailable)?;
        let mut principals = std::collections::BTreeSet::new();
        for discovery in state.discoveries.values() {
            if discovery.expires_at >= now {
                principals.insert(discovery.principal.clone());
            }
        }
        Ok(principals.into_iter().collect())
    }
    pub(crate) fn browser_session_seed(
        &self,
        principal: &PrincipalId,
        assertion_id: &str,
        idempotency_key: &str,
    ) -> Result<[u8; 32], ProductionAuthError> {
        valid_id(assertion_id)?;
        if idempotency_key.is_empty()
            || idempotency_key.len() > 255
            || idempotency_key.bytes().any(|byte| {
                !(byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
            })
        {
            return Err(ProductionAuthError::InvalidDisclosure);
        }
        let mut material = Vec::new();
        material.extend_from_slice(b"layerx-human/browser-session-recovery/v1\0");
        push_bytes(&mut material, principal.as_str().as_bytes())?;
        push_bytes(&mut material, assertion_id.as_bytes())?;
        push_bytes(&mut material, idempotency_key.as_bytes())?;
        let seed = hmac(&self.key.0, &material);
        material.zeroize();
        Ok(seed)
    }
    fn bind(
        &self,
        kind: Kind,
        id: &str,
        p: &PrincipalId,
        expires_at: u64,
    ) -> Result<(), ProductionAuthError> {
        valid_id(id)?;
        let mut s = self
            .state
            .lock()
            .map_err(|_| ProductionAuthError::Unavailable)?;
        if let Some(old) = s.discoveries.get(id) {
            if old.kind != kind || old.principal != *p {
                return Err(ProductionAuthError::Conflict);
            }
        }
        let mut next = s.clone();
        next.discoveries.insert(
            id.into(),
            Discovery {
                kind,
                principal: p.clone(),
                expires_at,
            },
        );
        persist(&self.root, &self.key, &next)?;
        *s = next;
        Ok(())
    }
    fn resolve(&self, kind: Kind, id: &str, now: u64) -> Result<PrincipalId, ProductionAuthError> {
        valid_id(id)?;
        let s = self
            .state
            .lock()
            .map_err(|_| ProductionAuthError::Unavailable)?;
        let v = s
            .discoveries
            .get(id)
            .ok_or(ProductionAuthError::Unauthenticated)?;
        if v.kind != kind || now > v.expires_at {
            return Err(ProductionAuthError::Unauthenticated);
        }
        Ok(v.principal.clone())
    }
    fn issue(
        &self,
        digest: OperationDigest,
        issued_at: u64,
        expires_at: u64,
    ) -> Result<[u8; 32], ProductionAuthError> {
        let mut nonce = [0; 32];
        random_fill(&mut nonce).map_err(|_| ProductionAuthError::Entropy)?;
        let mut s = self
            .state
            .lock()
            .map_err(|_| ProductionAuthError::Unavailable)?;
        if s.capabilities.contains_key(&nonce) {
            return Err(ProductionAuthError::Entropy);
        }
        let mut next = s.clone();
        next.capabilities.insert(
            nonce,
            StoredCapability {
                digest,
                issued_at,
                expires_at,
            },
        );
        persist(&self.root, &self.key, &next)?;
        *s = next;
        Ok(nonce)
    }
    fn consume(
        &self,
        nonce: [u8; 32],
        digest: OperationDigest,
        now: u64,
    ) -> Result<(), ProductionAuthError> {
        let mut s = self
            .state
            .lock()
            .map_err(|_| ProductionAuthError::Unavailable)?;
        let v = s
            .capabilities
            .get(&nonce)
            .ok_or(ProductionAuthError::CapabilitySpent)?;
        if v.digest != digest
            || now < v.issued_at
            || now > v.expires_at
            || now.saturating_sub(v.issued_at) > 60
        {
            return Err(ProductionAuthError::CapabilityRefused);
        }
        let mut next = s.clone();
        next.capabilities.remove(&nonce);
        persist(&self.root, &self.key, &next)?;
        *s = next;
        Ok(())
    }
}

pub struct AuthorizationDisclosure<'a> {
    pub operation: &'a Operation,
    pub destination: &'a str,
    pub path_parameters: &'a BTreeMap<String, String>,
    pub body: &'a Value,
    pub idempotency_key: Option<&'a str>,
    pub trace: &'a str,
}

/// Non-cloneable, consuming authority bound to one complete request disclosure.
pub struct ExecutionCapability {
    operation: String,
    principal: PrincipalId,
    tenant: AgentTenantId,
    session: SessionContext,
    destination: String,
    trace: String,
    nonce: [u8; 32],
    body: [u8; 32],
    request: [u8; 32],
    digest: OperationDigest,
    issued_at: u64,
    expires_at: u64,
}
impl ExecutionCapability {
    pub fn operation(&self) -> &str {
        &self.operation
    }
    pub fn principal(&self) -> &PrincipalId {
        &self.principal
    }
    pub fn tenant(&self) -> &AgentTenantId {
        &self.tenant
    }
    pub fn session(&self) -> &SessionContext {
        &self.session
    }
    pub fn destination(&self) -> &str {
        &self.destination
    }
    pub fn trace(&self) -> &str {
        &self.trace
    }
    pub const fn request_disclosure(&self) -> [u8; 32] {
        self.request
    }
    pub const fn body_disclosure(&self) -> [u8; 32] {
        self.body
    }
    pub const fn issued_at(&self) -> u64 {
        self.issued_at
    }
    pub const fn expires_at(&self) -> u64 {
        self.expires_at
    }
    pub fn into_context(self) -> Result<PrincipalContext, ApiFailure> {
        PrincipalContext::authorized(
            self.principal,
            self.tenant,
            self.session.session_id,
            URL_SAFE_NO_PAD.encode(self.nonce),
            self.request,
            self.body,
            self.operation,
            self.destination,
            self.trace,
            self.issued_at,
            self.expires_at,
        )
    }
}

pub fn authorize_execution(
    store: &mut PrincipalStore,
    passkeys: &Passkeys,
    index: &AuthDiscoveryIndex,
    access: &str,
    csrf: Option<&str>,
    step_up: Option<&StepUpEvidence>,
    d: AuthorizationDisclosure<'_>,
    now: u64,
    max_age: u64,
) -> Result<ExecutionCapability, ProductionAuthError> {
    if max_age == 0
        || max_age > 60
        || !d.destination.starts_with('/')
        || d.destination.starts_with("//")
        || d.trace.is_empty()
        || d.trace.len() > 255
    {
        return Err(ProductionAuthError::InvalidDisclosure);
    }
    let principal = index.resolve_access_token(access, now)?;
    let mut scope = store.principal(&principal)?;
    let tenant = scope.tenant().clone();
    let class = operation_class(d.operation)?;
    let body = body_hash(d.body)?;
    let request = request_hash(&d)?;
    let token_session = access
        .split_once('.')
        .map(|value| value.0)
        .ok_or(ProductionAuthError::Unauthenticated)?;
    let digest = capability_digest(
        &principal,
        &tenant,
        token_session,
        d.operation,
        d.destination,
        d.trace,
        request,
        body,
    );
    let step_up_digest = class
        .requires_step_up()
        .then(|| {
            step_up_digest(
                &principal,
                &tenant,
                d.operation,
                d.destination,
                d.path_parameters,
                d.body,
                d.idempotency_key,
            )
        })
        .transpose()?;
    let decision = passkeys.authorize(
        &mut scope,
        access,
        csrf,
        &AuthorizationRequest {
            operation: class,
            digest: step_up_digest,
            step_up,
            intended_destination: d.destination,
        },
        now,
    )?;
    let AccessDecision::Authorized(session) = decision else {
        return Err(ProductionAuthError::SessionExpired);
    };
    if session.session_id != token_session {
        return Err(ProductionAuthError::Unauthenticated);
    }
    let expires_at = now
        .checked_add(max_age)
        .ok_or(ProductionAuthError::InvalidConfiguration)?;
    let nonce = index.issue(digest, now, expires_at)?;
    Ok(ExecutionCapability {
        operation: d.operation.name.clone(),
        principal,
        tenant,
        session,
        destination: d.destination.into(),
        trace: d.trace.into(),
        nonce,
        body,
        request,
        digest,
        issued_at: now,
        expires_at,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn authorize_refresh_execution(
    store: &mut PrincipalStore,
    passkeys: &Passkeys,
    index: &AuthDiscoveryIndex,
    refresh: &str,
    csrf: &str,
    d: AuthorizationDisclosure<'_>,
    now: u64,
    max_age: u64,
) -> Result<PrincipalContext, ProductionAuthError> {
    if max_age == 0 || max_age > 60 {
        return Err(ProductionAuthError::InvalidConfiguration);
    }
    let principal = index.resolve_access_token(refresh, now)?;
    let scope = store.principal(&principal)?;
    let tenant = scope.tenant().clone();
    let session_id = passkeys.reserve_refresh(&scope, refresh, csrf, now)?;
    let body = body_hash(d.body)?;
    let request = request_hash(&d)?;
    let digest = capability_digest(
        &principal,
        &tenant,
        &session_id,
        d.operation,
        d.destination,
        d.trace,
        request,
        body,
    );
    let expires_at = now
        .checked_add(max_age)
        .ok_or(ProductionAuthError::InvalidConfiguration)?;
    let nonce = index.issue(digest, now, expires_at)?;
    PrincipalContext::authorized(
        principal,
        tenant,
        session_id,
        URL_SAFE_NO_PAD.encode(nonce),
        request,
        body,
        d.operation.name.clone(),
        d.destination.into(),
        d.trace.into(),
        now,
        expires_at,
    )
    .and_then(|context| context.with_refresh(refresh.to_owned(), csrf.to_owned()))
    .map_err(|_| ProductionAuthError::InvalidDisclosure)
}

/// Authorizes the exact request and body digests already computed by the
/// schema-aware HTTPS boundary. This is the production component-socket seam:
/// it never reconstructs or substitutes JSON after the router has decoded it.
#[allow(clippy::too_many_arguments)]
pub fn authorize_prehashed(
    store: &mut PrincipalStore,
    passkeys: &Passkeys,
    index: &AuthDiscoveryIndex,
    access: &str,
    csrf: Option<&str>,
    step_up: Option<&StepUpEvidence>,
    operation: &Operation,
    destination: &str,
    trace: &str,
    request: [u8; 32],
    body: [u8; 32],
    now: u64,
    max_age: u64,
) -> Result<ExecutionCapability, ProductionAuthError> {
    if max_age == 0
        || max_age > 60
        || !destination.starts_with('/')
        || destination.starts_with("//")
        || trace.is_empty()
        || trace.len() > 255
    {
        return Err(ProductionAuthError::InvalidDisclosure);
    }
    let principal = index.resolve_access_token(access, now)?;
    let mut scope = store.principal(&principal)?;
    let tenant = scope.tenant().clone();
    let class = operation_class(operation)?;
    if class.requires_step_up() {
        // A prehashed request cannot prove that its digest excluded the
        // evidence object itself. Sensitive operations must disclose their
        // decoded body to `authorize_execution` so the server derives the
        // non-circular step-up digest.
        return Err(ProductionAuthError::InvalidDisclosure);
    }
    let token_session = access
        .split_once('.')
        .map(|value| value.0)
        .ok_or(ProductionAuthError::Unauthenticated)?;
    let digest = capability_digest(
        &principal,
        &tenant,
        token_session,
        operation,
        destination,
        trace,
        request,
        body,
    );
    let decision = passkeys.authorize(
        &mut scope,
        access,
        csrf,
        &AuthorizationRequest {
            operation: class,
            digest: class.requires_step_up().then_some(digest),
            step_up,
            intended_destination: destination,
        },
        now,
    )?;
    let AccessDecision::Authorized(session) = decision else {
        return Err(ProductionAuthError::SessionExpired);
    };
    if session.session_id != token_session {
        return Err(ProductionAuthError::Unauthenticated);
    }
    let expires_at = now
        .checked_add(max_age)
        .ok_or(ProductionAuthError::InvalidConfiguration)?;
    let nonce = index.issue(digest, now, expires_at)?;
    Ok(ExecutionCapability {
        operation: operation.name.clone(),
        principal,
        tenant,
        session,
        destination: destination.into(),
        trace: trace.into(),
        nonce,
        body,
        request,
        digest,
        issued_at: now,
        expires_at,
    })
}

pub fn consume_execution(
    index: &AuthDiscoveryIndex,
    capability: ExecutionCapability,
    d: AuthorizationDisclosure<'_>,
    now: u64,
) -> Result<(), ProductionAuthError> {
    if capability.operation != d.operation.name
        || capability.destination != d.destination
        || capability.trace != d.trace
        || capability.body != body_hash(d.body)?
        || capability.request != request_hash(&d)?
        || now > capability.expires_at
    {
        return Err(ProductionAuthError::CapabilityRefused);
    }
    index.consume(capability.nonce, capability.digest, now)
}

/// Consumes the opaque capability after it crosses the authenticated component socket.
pub fn consume_context(
    index: &AuthDiscoveryIndex,
    context: &PrincipalContext,
    d: AuthorizationDisclosure<'_>,
    now: u64,
) -> Result<(), ProductionAuthError> {
    let nonce_bytes = URL_SAFE_NO_PAD
        .decode(context.capability())
        .map_err(|_| ProductionAuthError::CapabilityRefused)?;
    let nonce: [u8; 32] = nonce_bytes
        .try_into()
        .map_err(|_| ProductionAuthError::CapabilityRefused)?;
    let request = request_hash(&d)?;
    let body = body_hash(d.body)?;
    if context.operation() != d.operation.name
        || context.destination() != d.destination
        || context.trace() != d.trace
        || context.request_digest() != request
        || context.disclosure_digest() != body
        || context.issued_at() > now
        || context.expires_at() < now
        || context.expires_at().saturating_sub(context.issued_at()) > 60
    {
        return Err(ProductionAuthError::CapabilityRefused);
    }
    let digest = capability_digest(
        &context.principal,
        &context.tenant,
        &context.session_id,
        d.operation,
        d.destination,
        d.trace,
        request,
        body,
    );
    index.consume(nonce, digest, now)
}

fn operation_class(op: &Operation) -> Result<OperationClass, ProductionAuthError> {
    Ok(match op.authorization_class {
        AuthorizationClass::Read => OperationClass::Read,
        AuthorizationClass::MoneyMovement => OperationClass::MoneyMovement,
        AuthorizationClass::Approval => OperationClass::Approval,
        AuthorizationClass::Withdrawal => OperationClass::Withdrawal,
        AuthorizationClass::Exit => OperationClass::Exit,
        AuthorizationClass::SecuritySettings => OperationClass::SecuritySettings,
        AuthorizationClass::SecretReveal => OperationClass::SecretReveal,
        AuthorizationClass::WalletRebind => OperationClass::WalletRebind,
        AuthorizationClass::AgentArchive => OperationClass::AgentArchive,
    })
}

fn body_hash(body: &Value) -> Result<[u8; 32], ProductionAuthError> {
    let body = serde_json::to_vec(body).map_err(|_| ProductionAuthError::InvalidDisclosure)?;
    Ok(Sha256::digest(body).into())
}
fn step_up_digest(
    principal: &PrincipalId,
    tenant: &AgentTenantId,
    operation: &Operation,
    destination: &str,
    path_parameters: &BTreeMap<String, String>,
    body: &Value,
    idempotency_key: Option<&str>,
) -> Result<OperationDigest, ProductionAuthError> {
    let mut disclosed = body.clone();
    if let Some(object) = disclosed.as_object_mut() {
        object.remove("step_up");
        object.remove("step_up_evidence");
    }
    let body =
        serde_json::to_vec(&disclosed).map_err(|_| ProductionAuthError::InvalidDisclosure)?;
    let mut hash = Sha256::new();
    hash.update(b"layerx-human/step-up-operation/v1\0");
    put_text(&mut hash, principal.as_str());
    put_text(&mut hash, tenant.as_str());
    put_text(&mut hash, &operation.name);
    put_text(&mut hash, operation.authorization_class.as_str());
    put_text(&mut hash, &operation.method);
    put_text(&mut hash, destination);
    for (name, value) in path_parameters {
        put_text(&mut hash, name);
        put_text(&mut hash, value);
    }
    hash.update((body.len() as u64).to_be_bytes());
    hash.update(body);
    put_text(&mut hash, idempotency_key.unwrap_or(""));
    Ok(OperationDigest::new(hash.finalize().into()))
}
fn request_hash(d: &AuthorizationDisclosure<'_>) -> Result<[u8; 32], ProductionAuthError> {
    let body = serde_json::to_vec(d.body).map_err(|_| ProductionAuthError::InvalidDisclosure)?;
    let mut h = Sha256::new();
    h.update(b"layerx-human/authorized-operation/v1\0");
    put_text(&mut h, &d.operation.name);
    put_text(&mut h, &d.operation.method);
    put_text(&mut h, d.destination);
    for (k, v) in d.path_parameters {
        put_text(&mut h, k);
        put_text(&mut h, v)
    }
    h.update((body.len() as u64).to_be_bytes());
    h.update(body);
    put_text(&mut h, d.idempotency_key.unwrap_or(""));
    put_text(&mut h, d.trace);
    Ok(h.finalize().into())
}
fn capability_digest(
    p: &PrincipalId,
    t: &AgentTenantId,
    s: &str,
    o: &Operation,
    d: &str,
    tr: &str,
    r: [u8; 32],
    b: [u8; 32],
) -> OperationDigest {
    let mut h = Sha256::new();
    h.update(DOMAIN);
    put_text(&mut h, p.as_str());
    put_text(&mut h, t.as_str());
    put_text(&mut h, s);
    put_text(&mut h, &o.name);
    put_text(&mut h, o.authorization_class.as_str());
    put_text(&mut h, d);
    put_text(&mut h, tr);
    h.update(r);
    h.update(b);
    OperationDigest::new(h.finalize().into())
}
fn put_text(h: &mut Sha256, v: &str) {
    h.update((v.len() as u64).to_be_bytes());
    h.update(v.as_bytes())
}

/// Finite real provider boundary: versioned binary frames over a privileged UDS.
#[derive(Clone, Debug)]
pub struct SecurityProviderConfig {
    pub socket: PathBuf,
    pub deadline: Duration,
    pub maximum_frame_bytes: usize,
}
pub struct RemoteSecurityProvider {
    config: SecurityProviderConfig,
}
impl RemoteSecurityProvider {
    pub fn new(config: SecurityProviderConfig) -> Result<Self, ProductionAuthError> {
        if !config.socket.is_absolute()
            || config.deadline.is_zero()
            || !(64..=1_048_576).contains(&config.maximum_frame_bytes)
        {
            Err(ProductionAuthError::InvalidConfiguration)
        } else {
            Ok(Self { config })
        }
    }
    pub fn probe(&self) -> Result<(), SecurityBoundaryError> {
        let fields = self.call(0, &[])?;
        if fields.is_empty() {
            Ok(())
        } else {
            Err(SecurityBoundaryError::InvalidEvidence)
        }
    }
    fn call(&self, op: u8, fields: &[&[u8]]) -> Result<Vec<Vec<u8>>, SecurityBoundaryError> {
        let mut q = Vec::new();
        q.extend_from_slice(PROVIDER_MAGIC);
        q.push(1);
        q.push(op);
        push_u32(&mut q, fields.len()).map_err(|_| SecurityBoundaryError::Refused)?;
        for f in fields {
            push_bytes(&mut q, f).map_err(|_| SecurityBoundaryError::Refused)?
        }
        if q.len() > self.config.maximum_frame_bytes {
            return Err(SecurityBoundaryError::Refused);
        }
        let mut s = UnixStream::connect(&self.config.socket)
            .map_err(|_| SecurityBoundaryError::Unavailable)?;
        s.set_read_timeout(Some(self.config.deadline))
            .map_err(|_| SecurityBoundaryError::Unavailable)?;
        s.set_write_timeout(Some(self.config.deadline))
            .map_err(|_| SecurityBoundaryError::Unavailable)?;
        s.write_all(&(q.len() as u32).to_be_bytes())
            .map_err(|_| SecurityBoundaryError::Unavailable)?;
        let sent = s.write_all(&q);
        q.zeroize();
        sent.map_err(|_| SecurityBoundaryError::Unavailable)?;
        let mut p = [0; 4];
        s.read_exact(&mut p)
            .map_err(|_| SecurityBoundaryError::Unavailable)?;
        let n = u32::from_be_bytes(p) as usize;
        if n == 0 || n > self.config.maximum_frame_bytes {
            return Err(SecurityBoundaryError::InvalidEvidence);
        }
        let mut r = vec![0; n];
        s.read_exact(&mut r)
            .map_err(|_| SecurityBoundaryError::Unavailable)?;
        decode_response(&r)
    }
}

impl AuthenticatorProvider for RemoteSecurityProvider {
    fn status(&self, p: &PrincipalId) -> Result<AuthenticatorStatus, SecurityBoundaryError> {
        decode_status(self.call(1, &[p.as_str().as_bytes()])?)
    }
    fn begin_setup(
        &mut self,
        p: &PrincipalId,
        l: &str,
        now: u64,
    ) -> Result<AuthenticatorSetupChallenge, SecurityBoundaryError> {
        let n = now.to_be_bytes();
        let f = self.call(2, &[p.as_str().as_bytes(), l.as_bytes(), &n])?;
        if f.len() != 6 {
            return Err(SecurityBoundaryError::InvalidEvidence);
        }
        Ok(AuthenticatorSetupChallenge {
            setup_id: text(&f[0])?,
            secret: TimedSecret::new(text(&f[1])?, u64v(&f[2])?, true, now)?,
            otpauth_uri: TimedSecret::new(text(&f[3])?, u64v(&f[4])?, false, now)?,
            expires_at: u64v(&f[5])?,
        })
    }
    fn finish_setup(
        &mut self,
        p: &PrincipalId,
        id: &str,
        code: &str,
        now: u64,
    ) -> Result<AuthenticatorSetupResult, SecurityBoundaryError> {
        let n = now.to_be_bytes();
        let f = self.call(
            3,
            &[p.as_str().as_bytes(), id.as_bytes(), code.as_bytes(), &n],
        )?;
        if f.len() < 6 {
            return Err(SecurityBoundaryError::InvalidEvidence);
        }
        let method = AuthenticatorMethod {
            id: text(&f[0])?,
            label: text(&f[1])?,
            enabled_at: u64v(&f[2])?,
            last_used_at: opt_u64(&f[3])?,
        };
        let remask = u64v(&f[4])?;
        Ok(AuthenticatorSetupResult {
            method,
            backup_codes: BackupCodeSet::new(
                f[5..]
                    .iter()
                    .map(|x| text(x))
                    .collect::<Result<Vec<_>, _>>()?,
                remask,
                now,
            )?,
        })
    }
    fn disable(
        &mut self,
        p: &PrincipalId,
        id: &str,
        now: u64,
    ) -> Result<AuthenticatorStatus, SecurityBoundaryError> {
        decode_status(self.call(
            4,
            &[p.as_str().as_bytes(), id.as_bytes(), &now.to_be_bytes()],
        )?)
    }
    fn rotate_backup_codes(
        &mut self,
        p: &PrincipalId,
        now: u64,
    ) -> Result<BackupCodeSet, SecurityBoundaryError> {
        let f = self.call(5, &[p.as_str().as_bytes(), &now.to_be_bytes()])?;
        if f.len() < 2 {
            return Err(SecurityBoundaryError::InvalidEvidence);
        }
        BackupCodeSet::new(
            f[1..]
                .iter()
                .map(|x| text(x))
                .collect::<Result<Vec<_>, _>>()?,
            u64v(&f[0])?,
            now,
        )
    }
}
impl RecoveryEvidenceProvider for RemoteSecurityProvider {
    fn reveal_verified_receipt(
        &self,
        p: &PrincipalId,
        id: &str,
        now: u64,
    ) -> Result<TimedSecret, SecurityBoundaryError> {
        let f = self.call(
            6,
            &[p.as_str().as_bytes(), id.as_bytes(), &now.to_be_bytes()],
        )?;
        if f.len() != 3 || f[2] != [1] {
            return Err(SecurityBoundaryError::InvalidEvidence);
        }
        TimedSecret::new(text(&f[0])?, u64v(&f[1])?, true, now)
    }
}

fn decode_status(f: Vec<Vec<u8>>) -> Result<AuthenticatorStatus, SecurityBoundaryError> {
    if f.len() < 2 {
        return Err(SecurityBoundaryError::InvalidEvidence);
    }
    let remaining = u32v(&f[0])?;
    let count = u32v(&f[1])? as usize;
    if f.len() != 2 + count * 4 {
        return Err(SecurityBoundaryError::InvalidEvidence);
    }
    let mut methods = Vec::new();
    for x in f[2..].chunks_exact(4) {
        methods.push(AuthenticatorMethod {
            id: text(&x[0])?,
            label: text(&x[1])?,
            enabled_at: u64v(&x[2])?,
            last_used_at: opt_u64(&x[3])?,
        })
    }
    Ok(AuthenticatorStatus {
        methods,
        backup_codes_remaining: remaining,
    })
}
fn decode_response(b: &[u8]) -> Result<Vec<Vec<u8>>, SecurityBoundaryError> {
    let mut c = Cursor::new(b);
    if c.take(4)? != PROVIDER_MAGIC || c.byte()? != 1 {
        return Err(SecurityBoundaryError::InvalidEvidence);
    }
    match c.byte()? {
        0 => {}
        1 => return Err(SecurityBoundaryError::Refused),
        _ => return Err(SecurityBoundaryError::InvalidEvidence),
    }
    let n = c.u32()? as usize;
    let mut out = Vec::new();
    for _ in 0..n {
        out.push(c.bytes()?.to_vec())
    }
    if !c.rest().is_empty() {
        return Err(SecurityBoundaryError::InvalidEvidence);
    }
    Ok(out)
}
fn reclaim_temp(root: &Path) -> Result<(), ProductionAuthError> {
    let path = root.join(TEMP);
    if !path.exists() {
        return Ok(());
    }
    let root_metadata = fs::symlink_metadata(root)?;
    let metadata = fs::symlink_metadata(&path)?;
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata.uid() != root_metadata.uid()
        || metadata.mode() & 0o077 != 0
    {
        return Err(ProductionAuthError::InvalidConfiguration);
    }
    fs::remove_file(path)?;
    fs::File::open(root)?.sync_all()?;
    Ok(())
}

fn load(root: &Path, key: &IndexAuthenticationKey) -> Result<State, ProductionAuthError> {
    let p = root.join(FILE);
    if !p.exists() {
        return Ok(State::default());
    }
    let b = fs::read(p)?;
    if b.len() < 37 {
        return Err(ProductionAuthError::IndexAuthentication);
    }
    let cut = b.len() - 32;
    if !bool::from(hmac(&key.0, &b[..cut]).ct_eq(&b[cut..])) {
        return Err(ProductionAuthError::IndexAuthentication);
    }
    let mut c = Cursor::new(&b[..cut]);
    if c.take(4)
        .map_err(|_| ProductionAuthError::IndexAuthentication)?
        != MAGIC
        || c.byte()
            .map_err(|_| ProductionAuthError::IndexAuthentication)?
            != 1
    {
        return Err(ProductionAuthError::IndexAuthentication);
    }
    let mut s = State::default();
    for _ in 0..c
        .u32()
        .map_err(|_| ProductionAuthError::IndexAuthentication)?
    {
        let kind = match c
            .byte()
            .map_err(|_| ProductionAuthError::IndexAuthentication)?
        {
            1 => Kind::Registration,
            2 => Kind::Assertion,
            3 => Kind::Session,
            4 => Kind::StepUp,
            _ => return Err(ProductionAuthError::IndexAuthentication),
        };
        let id = text(
            c.bytes()
                .map_err(|_| ProductionAuthError::IndexAuthentication)?,
        )
        .map_err(|_| ProductionAuthError::IndexAuthentication)?;
        let p = PrincipalId::new(
            text(
                c.bytes()
                    .map_err(|_| ProductionAuthError::IndexAuthentication)?,
            )
            .map_err(|_| ProductionAuthError::IndexAuthentication)?,
        )
        .map_err(|_| ProductionAuthError::IndexAuthentication)?;
        let e = c
            .u64()
            .map_err(|_| ProductionAuthError::IndexAuthentication)?;
        s.discoveries.insert(
            id,
            Discovery {
                kind,
                principal: p,
                expires_at: e,
            },
        );
    }
    for _ in 0..c
        .u32()
        .map_err(|_| ProductionAuthError::IndexAuthentication)?
    {
        let mut n = [0; 32];
        n.copy_from_slice(
            c.take(32)
                .map_err(|_| ProductionAuthError::IndexAuthentication)?,
        );
        let mut d = [0; 32];
        d.copy_from_slice(
            c.take(32)
                .map_err(|_| ProductionAuthError::IndexAuthentication)?,
        );
        let i = c
            .u64()
            .map_err(|_| ProductionAuthError::IndexAuthentication)?;
        let e = c
            .u64()
            .map_err(|_| ProductionAuthError::IndexAuthentication)?;
        s.capabilities.insert(
            n,
            StoredCapability {
                digest: OperationDigest::new(d),
                issued_at: i,
                expires_at: e,
            },
        );
    }
    if !c.rest().is_empty() {
        return Err(ProductionAuthError::IndexAuthentication);
    }
    Ok(s)
}
fn persist(
    root: &Path,
    key: &IndexAuthenticationKey,
    s: &State,
) -> Result<(), ProductionAuthError> {
    let mut b = Vec::new();
    b.extend_from_slice(MAGIC);
    b.push(1);
    push_u32(&mut b, s.discoveries.len())?;
    for (id, v) in &s.discoveries {
        b.push(v.kind as u8);
        push_bytes(&mut b, id.as_bytes())?;
        push_bytes(&mut b, v.principal.as_str().as_bytes())?;
        b.extend_from_slice(&v.expires_at.to_be_bytes())
    }
    push_u32(&mut b, s.capabilities.len())?;
    for (n, v) in &s.capabilities {
        b.extend_from_slice(n);
        b.extend_from_slice(&v.digest.bytes());
        b.extend_from_slice(&v.issued_at.to_be_bytes());
        b.extend_from_slice(&v.expires_at.to_be_bytes())
    }
    b.extend_from_slice(&hmac(&key.0, &b));
    let tmp = root.join(TEMP);
    let result = (|| {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(&b)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, root.join(FILE))?;
        fs::File::open(root)?.sync_all()?;
        Ok::<(), ProductionAuthError>(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    b.zeroize();
    result
}
fn hmac(k: &[u8; 32], m: &[u8]) -> [u8; 32] {
    let mut i = [0x36; 64];
    let mut o = [0x5c; 64];
    for x in 0..32 {
        i[x] ^= k[x];
        o[x] ^= k[x]
    }
    let mut h = Sha256::new();
    h.update(i);
    h.update(m);
    let x = h.finalize();
    let mut h = Sha256::new();
    h.update(o);
    h.update(x);
    h.finalize().into()
}
fn valid_id(v: &str) -> Result<(), ProductionAuthError> {
    if v.is_empty()
        || v.len() > 128
        || !v
            .bytes()
            .all(|x| x.is_ascii_alphanumeric() || matches!(x, b'_' | b'-'))
    {
        Err(ProductionAuthError::InvalidDisclosure)
    } else {
        Ok(())
    }
}
fn account_key(email: &str) -> Result<String, ProductionAuthError> {
    let normalized = email.trim().to_ascii_lowercase();
    if normalized.len() < 3
        || normalized.len() > 254
        || normalized.matches('@').count() != 1
        || normalized.bytes().any(|b| b.is_ascii_control())
    {
        return Err(ProductionAuthError::InvalidDisclosure);
    }
    Ok(format!(
        "account_{}",
        URL_SAFE_NO_PAD.encode(Sha256::digest(normalized.as_bytes()))
    ))
}
fn push_u32(o: &mut Vec<u8>, v: usize) -> Result<(), ProductionAuthError> {
    o.extend_from_slice(
        &u32::try_from(v)
            .map_err(|_| ProductionAuthError::InvalidDisclosure)?
            .to_be_bytes(),
    );
    Ok(())
}
fn push_bytes(o: &mut Vec<u8>, v: &[u8]) -> Result<(), ProductionAuthError> {
    push_u32(o, v.len())?;
    o.extend_from_slice(v);
    Ok(())
}
fn text(v: &[u8]) -> Result<String, SecurityBoundaryError> {
    if v.is_empty() || v.len() > 4096 {
        return Err(SecurityBoundaryError::InvalidEvidence);
    }
    std::str::from_utf8(v)
        .map(str::to_owned)
        .map_err(|_| SecurityBoundaryError::InvalidEvidence)
}
fn u64v(v: &[u8]) -> Result<u64, SecurityBoundaryError> {
    v.try_into()
        .map(u64::from_be_bytes)
        .map_err(|_| SecurityBoundaryError::InvalidEvidence)
}
fn u32v(v: &[u8]) -> Result<u32, SecurityBoundaryError> {
    v.try_into()
        .map(u32::from_be_bytes)
        .map_err(|_| SecurityBoundaryError::InvalidEvidence)
}
fn opt_u64(v: &[u8]) -> Result<Option<u64>, SecurityBoundaryError> {
    if v.is_empty() {
        Ok(None)
    } else {
        u64v(v).map(Some)
    }
}
struct Cursor<'a> {
    b: &'a [u8],
    p: usize,
}
impl<'a> Cursor<'a> {
    fn new(b: &'a [u8]) -> Self {
        Self { b, p: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], SecurityBoundaryError> {
        let e = self
            .p
            .checked_add(n)
            .ok_or(SecurityBoundaryError::InvalidEvidence)?;
        let v = self
            .b
            .get(self.p..e)
            .ok_or(SecurityBoundaryError::InvalidEvidence)?;
        self.p = e;
        Ok(v)
    }
    fn byte(&mut self) -> Result<u8, SecurityBoundaryError> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32, SecurityBoundaryError> {
        let mut x = [0; 4];
        x.copy_from_slice(self.take(4)?);
        Ok(u32::from_be_bytes(x))
    }
    fn u64(&mut self) -> Result<u64, SecurityBoundaryError> {
        let mut x = [0; 8];
        x.copy_from_slice(self.take(8)?);
        Ok(u64::from_be_bytes(x))
    }
    fn bytes(&mut self) -> Result<&'a [u8], SecurityBoundaryError> {
        let n = self.u32()? as usize;
        self.take(n)
    }
    fn rest(&self) -> &'a [u8] {
        &self.b[self.p..]
    }
}

#[derive(Debug)]
pub enum ProductionAuthError {
    InvalidConfiguration,
    InvalidDisclosure,
    UnclassifiedOperation,
    IndexAuthentication,
    Unauthenticated,
    SessionExpired,
    CapabilitySpent,
    CapabilityRefused,
    Conflict,
    Entropy,
    Unavailable,
    Io(std::io::Error),
    Store(StoreError),
    Auth(AuthError),
}
impl From<std::io::Error> for ProductionAuthError {
    fn from(v: std::io::Error) -> Self {
        Self::Io(v)
    }
}
impl From<StoreError> for ProductionAuthError {
    fn from(v: StoreError) -> Self {
        Self::Store(v)
    }
}
impl From<AuthError> for ProductionAuthError {
    fn from(v: AuthError) -> Self {
        Self::Auth(v)
    }
}

//! Receipt-verifying authority boundary for the hosted gateway.

pub mod http;
pub mod store;

use layerx_crypto::disclosure::{bind as bind_disclosure, AmountRole, CounterpartyRole};
use layerx_crypto::{ed25519, SignatureMessage};
use layerx_proof::program::{
    verify_authorized_program_execution, verify_program_execution,
    AuthorizedProgramExecutionExpectation, ProgramExecutionExpectation, VerifiedProgramExecution,
};
use layerx_proof::receipt::{verify_outcome, AuthorizedBatch, ReceiptCheck};
use layerx_types::intent::{ProgramCallFailure, ProgramCallOutcome, ProgramLegacyValue};
pub use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_wire::activity::{decode_signed, encode_signed, encode_unsigned};
use layerx_wire::hash::{activity_id, Domain};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fmt::{Display, Formatter};
use subtle::ConstantTimeEq;
use zeroize::Zeroize;

use store::{KeyRecord, RedisStore};

const KEY_PREFIX: &str = "lxp_live_";
const KEY_BYTES: usize = 32;

/// Authentication failures shared by every hosted ingress using gateway API
/// keys. Persistence failure is deliberately distinct from a bad credential
/// so callers never turn an unavailable durable store into an authentication
/// refusal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccessError {
    Unauthenticated,
    PersistenceUnavailable,
}

/// Authenticates the exact `LayerX-Key` credential format issued by the hosted
/// gateway against its durable key store. Interop and human ingress call this
/// same boundary, so key revocation and principal scope cannot drift.
///
/// # Errors
/// Returns a typed authentication or persistence failure without exposing the
/// presented secret.
pub fn authenticate_gateway_key(
    store: &RedisStore,
    authorization: &str,
) -> Result<KeyRecord, AccessError> {
    let credentials = authorization
        .strip_prefix("LayerX-Key ")
        .ok_or(AccessError::Unauthenticated)?;
    let (id, secret) = credentials
        .split_once(':')
        .filter(|(id, secret)| {
            valid_key_identifier(id)
                && secret.starts_with(KEY_PREFIX)
                && secret.len() == KEY_PREFIX.len() + KEY_BYTES * 2
        })
        .ok_or(AccessError::Unauthenticated)?;
    let record = store
        .key(id)
        .map_err(|_| AccessError::PersistenceUnavailable)?
        .ok_or(AccessError::Unauthenticated)?;
    let candidate = gateway_digest(&[b"gateway-key-v1", record.salt.as_bytes(), secret.as_bytes()]);
    if record.disabled
        || record
            .secret_digest
            .as_bytes()
            .ct_eq(candidate.as_bytes())
            .unwrap_u8()
            != 1
    {
        return Err(AccessError::Unauthenticated);
    }
    Ok(record)
}

/// Domain-separated hexadecimal SHA-256 used by hosted ingress reservation
/// and audit identities.
#[must_use]
pub fn gateway_digest(parts: &[&[u8]]) -> String {
    let mut hash = Sha256::new();
    for part in parts {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part);
    }
    format!("{:x}", hash.finalize())
}

/// Builds one redacted audit commitment. Only the principal digest, action,
/// bounded subject and outcome participate; request payloads and credentials
/// never reach the durable audit stream.
#[must_use]
pub fn gateway_audit_event(
    principal_digest: &str,
    action: &str,
    subject: &str,
    outcome: &str,
    observed_at: u64,
) -> String {
    let event = gateway_digest(&[
        b"gateway-audit-v1",
        action.as_bytes(),
        subject.as_bytes(),
        outcome.as_bytes(),
        &observed_at.to_be_bytes(),
    ]);
    format!("{principal_digest}:{event}")
}

fn valid_key_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct PrincipalId(String);

impl PrincipalId {
    /// Creates a bounded stable principal identifier.
    ///
    /// # Errors
    /// Returns [`GatewayError::InvalidRequest`] for an invalid identifier.
    pub fn new(value: impl Into<String>) -> Result<Self, GatewayError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 128
            || !value.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'-' | b'_' | b'.' | b':')
            })
        {
            return Err(GatewayError::InvalidRequest);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn audit_digest(&self) -> [u8; 32] {
        Sha256::digest(self.0.as_bytes()).into()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Quota {
    pub requests: u64,
    pub window_seconds: u64,
}

impl Quota {
    /// Creates a finite, non-zero fixed-window quota.
    ///
    /// # Errors
    /// Returns [`GatewayError::InvalidRequest`] for a zero or excessive bound.
    pub const fn new(requests: u64, window_seconds: u64) -> Result<Self, GatewayError> {
        if requests == 0
            || requests > 1_000_000
            || window_seconds == 0
            || window_seconds > 2_592_000
        {
            return Err(GatewayError::InvalidRequest);
        }
        Ok(Self {
            requests,
            window_seconds,
        })
    }

    #[must_use]
    pub const fn requests(self) -> u64 {
        self.requests
    }

    #[must_use]
    pub const fn window_seconds(self) -> u64 {
        self.window_seconds
    }
}

pub struct IssuedKey {
    id: String,
    secret: String,
}

impl std::fmt::Debug for IssuedKey {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("IssuedKey")
            .field("id", &self.id)
            .field("secret", &"[REDACTED]")
            .finish()
    }
}

impl IssuedKey {
    /// Generates a gateway key from operating-system entropy.
    ///
    /// # Errors
    /// Returns [`GatewayError::Entropy`] when secure randomness is unavailable.
    pub fn generate() -> Result<Self, GatewayError> {
        let mut random = [0_u8; KEY_BYTES + 16];
        getrandom::fill(&mut random).map_err(|_| GatewayError::Entropy)?;
        let id = hex(&Sha256::digest(random)[..12]);
        let secret = format!("{KEY_PREFIX}{}", hex(&random[..KEY_BYTES]));
        random.zeroize();
        Ok(Self { id, secret })
    }

    /// Derives a replay-stable credential for an authenticated, durable
    /// issuance idempotency scope. The provisioning key is never retained.
    #[must_use]
    pub fn derive(provisioning_key: &[u8; 32], context: &[u8]) -> Self {
        let identifier = hmac_sha256(provisioning_key, b"layerx-gateway-key-id-v1", context);
        let mut secret_bytes =
            hmac_sha256(provisioning_key, b"layerx-gateway-key-secret-v1", context);
        let id = hex(&identifier[..12]);
        let secret = format!("{KEY_PREFIX}{}", hex(&secret_bytes));
        secret_bytes.zeroize();
        Self { id, secret }
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub fn secret(&self) -> &str {
        &self.secret
    }
}

impl Drop for IssuedKey {
    fn drop(&mut self) {
        self.secret.zeroize();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthorityFacts {
    batch_id: [u8; 32],
    asset: [u8; 32],
    previous_state_root: [u8; 32],
    resulting_state_root: [u8; 32],
    sequencer_public_key: [u8; 32],
}

impl AuthorityFacts {
    #[must_use]
    pub const fn new(
        batch_id: [u8; 32],
        asset: [u8; 32],
        previous_state_root: [u8; 32],
        resulting_state_root: [u8; 32],
        sequencer_public_key: [u8; 32],
    ) -> Self {
        Self {
            batch_id,
            asset,
            previous_state_root,
            resulting_state_root,
            sequencer_public_key,
        }
    }

    #[must_use]
    pub const fn sequencer_public_key(self) -> [u8; 32] {
        self.sequencer_public_key
    }

    fn authorized(self) -> AuthorizedBatch {
        AuthorizedBatch::new(
            self.batch_id,
            self.asset,
            self.previous_state_root,
            self.resulting_state_root,
            self.sequencer_public_key,
        )
    }
}

/// A response whose receipt passed canonical, invariant, root-chain and pinned
/// sequencer-signature verification. Its fields are private so a caller cannot
/// construct one from a status word and non-empty bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedOperation {
    response: Vec<u8>,
    receipt: Vec<u8>,
    receipt_digest: [u8; 32],
    activity_id: [u8; 32],
    result_code: i32,
    verification_rank: u8,
}

/// Canonical activity identity available only after module, network, wire and
/// signer verification under the key's pinned signer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VerifiedSubmission {
    activity_id: [u8; 32],
    idempotency_key: [u8; 32],
    transfer: Option<VerifiedTransfer>,
}

/// Exact 402LXP transfer facts decoded from a canonically signed submission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VerifiedTransfer {
    payer: [u8; 32],
    recipient: [u8; 32],
    asset: [u8; 32],
    amount: u128,
    not_before: u64,
    not_after: u64,
    expires_at: u64,
}

impl VerifiedTransfer {
    #[must_use]
    pub const fn payer(self) -> [u8; 32] {
        self.payer
    }

    #[must_use]
    pub const fn recipient(self) -> [u8; 32] {
        self.recipient
    }

    #[must_use]
    pub const fn asset(self) -> [u8; 32] {
        self.asset
    }

    #[must_use]
    pub const fn amount(self) -> u128 {
        self.amount
    }

    #[must_use]
    pub const fn not_before(self) -> u64 {
        self.not_before
    }

    #[must_use]
    pub const fn not_after(self) -> u64 {
        self.not_after
    }

    #[must_use]
    pub const fn expires_at(self) -> u64 {
        self.expires_at
    }
}

impl VerifiedSubmission {
    #[must_use]
    pub const fn activity_id(self) -> [u8; 32] {
        self.activity_id
    }

    #[must_use]
    pub const fn idempotency_key(self) -> [u8; 32] {
        self.idempotency_key
    }

    #[must_use]
    pub const fn transfer(self) -> Option<VerifiedTransfer> {
        self.transfer
    }
}

fn verified_transfer(
    unsigned: &[u8],
    registry: &ModuleRegistry,
    activity_type: ActivityType,
) -> Result<Option<VerifiedTransfer>, GatewayError> {
    if activity_type.module() != ModuleId::Asset || !matches!(activity_type.ordinal(), 5 | 6) {
        return Ok(None);
    }
    let disclosure =
        bind_disclosure(unsigned, registry).map_err(|_| GatewayError::InvalidRequest)?;
    let mut payer = None;
    let mut recipient = None;
    for counterparty in disclosure.counterparties {
        let target = match counterparty.role {
            CounterpartyRole::Payer => &mut payer,
            CounterpartyRole::Recipient => &mut recipient,
        };
        if target.replace(counterparty.account).is_some() {
            return Err(GatewayError::InvalidRequest);
        }
    }
    let mut amount = None;
    for disclosed in disclosure.amounts {
        if disclosed.role == AmountRole::Transfer && amount.replace(disclosed.value).is_some() {
            return Err(GatewayError::InvalidRequest);
        }
    }
    Ok(Some(VerifiedTransfer {
        payer: payer.ok_or(GatewayError::InvalidRequest)?,
        recipient: recipient.ok_or(GatewayError::InvalidRequest)?,
        asset: disclosure.asset,
        amount: amount.ok_or(GatewayError::InvalidRequest)?,
        not_before: disclosure.expiry.not_before,
        not_after: disclosure.expiry.not_after,
        expires_at: disclosure.expiry.payload_expires_at,
    }))
}

/// Refuses a submitted activity unless its bytes are canonical, its module was
/// provisioned from the core contract, its protocol scope is exact, and its
/// Ed25519 signature belongs to the signer bound to the authenticated key.
///
/// # Errors
/// Returns [`GatewayError::InvalidRequest`] without yielding partial identity.
pub fn verify_submission(
    bytes: &[u8],
    registry: &ModuleRegistry,
    protocol_version: u16,
    network_id: u32,
    signer_public_key: &[u8; 32],
) -> Result<VerifiedSubmission, GatewayError> {
    let activity = decode_signed(bytes, registry).map_err(|_| GatewayError::InvalidRequest)?;
    if activity.protocol_version() != protocol_version || activity.network_id() != network_id {
        return Err(GatewayError::InvalidRequest);
    }
    if encode_signed(&activity).map_err(|_| GatewayError::InvalidRequest)? != bytes {
        return Err(GatewayError::InvalidRequest);
    }
    let signature_bytes = activity.signature().ok_or(GatewayError::InvalidRequest)?;
    let signature: [u8; 64] = signature_bytes
        .try_into()
        .map_err(|_| GatewayError::InvalidRequest)?;
    let unsigned = encode_unsigned(&activity).map_err(|_| GatewayError::InvalidRequest)?;
    let message = SignatureMessage::new(
        Domain::SignaturePreimage,
        activity.protocol_version(),
        activity.network_id(),
        &unsigned,
    )
    .map_err(|_| GatewayError::InvalidRequest)?;
    ed25519::verify(signer_public_key, &signature, message).map_err(|_| GatewayError::Forbidden)?;
    let transfer = verified_transfer(&unsigned, registry, activity.activity_type())?;
    Ok(VerifiedSubmission {
        activity_id: activity_id(&activity).map_err(|_| GatewayError::InvalidRequest)?,
        idempotency_key: activity.idempotency_key(),
        transfer,
    })
}

impl VerifiedOperation {
    #[must_use]
    pub fn response(&self) -> &[u8] {
        &self.response
    }

    #[must_use]
    pub fn receipt(&self) -> &[u8] {
        &self.receipt
    }

    #[must_use]
    pub const fn receipt_digest(&self) -> [u8; 32] {
        self.receipt_digest
    }

    #[must_use]
    pub const fn activity_id(&self) -> [u8; 32] {
        self.activity_id
    }

    #[must_use]
    pub const fn result_code(&self) -> i32 {
        self.result_code
    }

    #[must_use]
    pub const fn verification_rank(&self) -> u8 {
        self.verification_rank
    }

    #[must_use]
    pub const fn verification_level(&self) -> &'static str {
        "receipt-verified"
    }
}

#[derive(Serialize)]
struct ActivityResult {
    activity_id: String,
    batch_id: String,
    global_sequence: u64,
    result_code: i32,
    state_root: String,
    receipt: String,
}

/// Verifies a component receipt against the independently provisioned
/// sequencer key and derives the public activity response only from verified
/// receipt fields.
///
/// # Errors
/// Returns a typed verification error without a partially verified value.
pub fn verify_activity_operation(
    receipt_bytes: &[u8],
    authority: AuthorityFacts,
    trusted_sequencer_key: &[u8; 32],
    expected_activity_id: Option<[u8; 32]>,
) -> Result<VerifiedOperation, GatewayError> {
    if authority
        .sequencer_public_key()
        .ct_eq(trusted_sequencer_key)
        .unwrap_u8()
        != 1
    {
        return Err(GatewayError::UntrustedSequencer);
    }
    let verified = verify_outcome(receipt_bytes, &authority.authorized())
        .map_err(|failure| GatewayError::Receipt(failure.check))?;
    let protocol = verified
        .receipt()
        .protocol()
        .ok_or(GatewayError::Receipt(ReceiptCheck::ReceiptShape))?;
    if expected_activity_id.is_some_and(|expected| protocol.activity_id() != expected) {
        return Err(GatewayError::ActivityMismatch);
    }
    let receipt_digest = verified
        .evidence()
        .receipt_digest()
        .ok_or(GatewayError::VerificationRequired)?;
    let response = serde_json::to_vec(&ActivityResult {
        activity_id: hex(&protocol.activity_id()),
        batch_id: hex(&protocol.batch_id()),
        global_sequence: protocol.global_sequence(),
        result_code: protocol.result_code(),
        state_root: hex(&protocol.resulting_state_root()),
        receipt: hex(verified.canonical_bytes()),
    })
    .map_err(|_| GatewayError::Encoding)?;
    Ok(VerifiedOperation {
        response,
        receipt: verified.canonical_bytes().to_vec(),
        receipt_digest,
        activity_id: protocol.activity_id(),
        result_code: protocol.result_code(),
        verification_rank: verified.level().wire_rank(),
    })
}

/// Verifies a committed Programs receipt together with its authenticated
/// terminal payload and call graph, then renders only locally derived fields.
///
/// # Errors
///
/// Refuses untrusted sequencers and every receipt, activity, program, ABI,
/// terminal, graph, occupancy, or transfer-authority mismatch.
pub fn verify_program_operation(
    receipt_bytes: &[u8],
    terminal_payload: &[u8],
    call_graph: &[u8],
    authority: AuthorityFacts,
    trusted_sequencer_key: &[u8; 32],
    expected_activity_id: [u8; 32],
    expected_program_id: [u8; 32],
    expected_guest_abi_version: u16,
) -> Result<VerifiedOperation, GatewayError> {
    if authority
        .sequencer_public_key()
        .ct_eq(trusted_sequencer_key)
        .unwrap_u8()
        != 1
    {
        return Err(GatewayError::UntrustedSequencer);
    }
    let verified = verify_authorized_program_execution(
        receipt_bytes,
        terminal_payload,
        call_graph,
        AuthorizedProgramExecutionExpectation {
            authority: authority.authorized(),
            activity_id: expected_activity_id,
            program_id: expected_program_id,
            guest_abi_version: expected_guest_abi_version,
        },
    )
    .map_err(|_| GatewayError::VerificationRequired)?;
    render_verified_program_operation(
        verified,
        terminal_payload,
        call_graph,
        expected_program_id,
        expected_guest_abi_version,
        *trusted_sequencer_key,
        "executed",
    )
}

/// Verifies a non-committing Programs simulation against a separately pinned
/// protocol head and sequencer key.
///
/// # Errors
///
/// Refuses every receipt or terminal mismatch without returning a partial
/// simulation result.
pub fn verify_program_simulation_operation(
    receipt_bytes: &[u8],
    terminal_payload: &[u8],
    call_graph: &[u8],
    trusted_previous_state_root: [u8; 32],
    trusted_sequencer_key: [u8; 32],
    expected_activity_id: [u8; 32],
    expected_program_id: [u8; 32],
    expected_guest_abi_version: u16,
) -> Result<VerifiedOperation, GatewayError> {
    let verified = verify_program_execution(
        receipt_bytes,
        terminal_payload,
        call_graph,
        ProgramExecutionExpectation {
            sequencer_public_key: trusted_sequencer_key,
            previous_state_root: trusted_previous_state_root,
            activity_id: expected_activity_id,
            program_id: expected_program_id,
            guest_abi_version: expected_guest_abi_version,
        },
    )
    .map_err(|_| GatewayError::VerificationRequired)?;
    render_verified_program_operation(
        verified,
        terminal_payload,
        call_graph,
        expected_program_id,
        expected_guest_abi_version,
        trusted_sequencer_key,
        "simulated",
    )
}

fn render_verified_program_operation(
    verified: VerifiedProgramExecution,
    terminal_payload: &[u8],
    call_graph: &[u8],
    expected_program_id: [u8; 32],
    expected_guest_abi_version: u16,
    sequencer_public_key: [u8; 32],
    state: &str,
) -> Result<VerifiedOperation, GatewayError> {
    let protocol = verified
        .receipt()
        .receipt()
        .protocol()
        .ok_or(GatewayError::VerificationRequired)?;
    let receipt_digest = verified
        .receipt()
        .evidence()
        .receipt_digest()
        .ok_or(GatewayError::VerificationRequired)?;
    let state = if state == "executed" && !verified.outcome().is_completed() {
        "refused"
    } else {
        state
    };
    let outcome = program_outcome_json(verified.outcome());
    let response = serde_json::to_vec(&serde_json::json!({
        "state": state,
        "activity_id": hex(&protocol.activity_id()),
        "program_id": hex(&expected_program_id),
        "guest_abi_version": expected_guest_abi_version,
        "module_version": protocol.module_version(),
        "batch_id": hex(&protocol.batch_id()),
        "global_sequence": protocol.global_sequence().to_string(),
        "result_code": verified.result_code(),
        "state_root": hex(&protocol.resulting_state_root()),
        "receipt": hex(verified.receipt().canonical_bytes()),
        "receipt_digest": hex(&receipt_digest),
        "terminal_payload": hex(terminal_payload),
        "call_graph": hex(call_graph),
        "authority": {
            "batch_id": hex(&protocol.batch_id()),
            "asset": hex(&protocol.asset()),
            "previous_state_root": hex(&protocol.previous_state_root()),
            "resulting_state_root": hex(&protocol.resulting_state_root()),
            "sequencer_public_key": hex(&sequencer_public_key),
        },
        "usage": {
            "cpu_fuel": verified.cpu_fuel().to_string(),
            "memory_bytes": verified.memory_bytes().to_string(),
            "storage_read_bytes": verified.storage_read_bytes().to_string(),
            "storage_write_bytes": verified.storage_write_bytes().to_string(),
            "output_values": verified.output_values(),
            "output_bytes": verified.output_bytes().to_string(),
            "fee_units": verified.fee_units().to_string(),
        },
        "outcome": outcome,
        "verification": "receipt-terminal-and-call-graph-verified",
    }))
    .map_err(|_| GatewayError::Encoding)?;
    Ok(VerifiedOperation {
        response,
        receipt: verified.receipt().canonical_bytes().to_vec(),
        receipt_digest,
        activity_id: protocol.activity_id(),
        result_code: verified.result_code(),
        verification_rank: verified.receipt().level().wire_rank(),
    })
}

fn program_outcome_json(outcome: &ProgramCallOutcome) -> serde_json::Value {
    match outcome {
        ProgramCallOutcome::Completed(response) => serde_json::json!({
            "kind": "completed",
            "code": response.code(),
            "response": hex(response.body()),
        }),
        ProgramCallOutcome::LegacyCompleted(response) => serde_json::json!({
            "kind": "legacy_completed",
            "code": response.code(),
            "values": response.values().iter().map(|value| match value {
                ProgramLegacyValue::I32(value) => serde_json::json!({"type":"i32","value":value}),
                ProgramLegacyValue::I64(value) => serde_json::json!({"type":"i64","value":value.to_string()}),
            }).collect::<Vec<_>>(),
        }),
        ProgramCallOutcome::Refused(failure) => serde_json::json!({
            "kind": "refused",
            "failure": program_failure_json(*failure),
        }),
    }
}

fn program_failure_json(failure: ProgramCallFailure) -> serde_json::Value {
    match failure {
        ProgramCallFailure::UnknownProgram => serde_json::json!({"kind":"unknown_program"}),
        ProgramCallFailure::Reentrancy => serde_json::json!({"kind":"reentrancy"}),
        ProgramCallFailure::DepthExceeded { limit, attempted } => {
            serde_json::json!({"kind":"depth_exceeded","limit":limit,"attempted":attempted})
        }
        ProgramCallFailure::FanoutExceeded { limit, attempted } => {
            serde_json::json!({"kind":"fanout_exceeded","limit":limit,"attempted":attempted})
        }
        ProgramCallFailure::GuestRefused { code } => {
            serde_json::json!({"kind":"guest_refused","code":code})
        }
        ProgramCallFailure::Authority => serde_json::json!({"kind":"authority"}),
        ProgramCallFailure::Resource => serde_json::json!({"kind":"resource"}),
        ProgramCallFailure::Response => serde_json::json!({"kind":"response"}),
        ProgramCallFailure::Fault => serde_json::json!({"kind":"fault"}),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProductionRoute<'a> {
    Activity,
    State,
    Receipt(&'a str),
    ProgramRegistry(&'a str),
    ProgramInterface(&'a str),
    ProgramSimulation,
    ProgramCall,
    ProgramActivity(&'a str),
    ProgramReceiptByIdempotency(&'a str),
}

/// Parses the exact production route set shared with the emulator. Emulator
/// administration paths are never accepted.
///
/// # Errors
/// Returns [`GatewayError::InvalidRoute`] for drift or unsafe identifiers.
pub fn production_route<'a>(
    method: &str,
    path: &'a str,
) -> Result<ProductionRoute<'a>, GatewayError> {
    match (method, path) {
        ("POST", "/v1/activities") => Ok(ProductionRoute::Activity),
        ("POST", "/v1/programs/simulate") => Ok(ProductionRoute::ProgramSimulation),
        ("POST", "/v1/programs/call") => Ok(ProductionRoute::ProgramCall),
        ("GET", "/v1/state") => Ok(ProductionRoute::State),
        ("GET", path) if path.starts_with("/v1/programs/activities/") => {
            let id = path
                .strip_prefix("/v1/programs/activities/")
                .ok_or(GatewayError::InvalidRoute)?;
            if id.len() == 64
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            {
                Ok(ProductionRoute::ProgramActivity(id))
            } else {
                Err(GatewayError::InvalidRoute)
            }
        }
        ("GET", path) if path.starts_with("/v1/programs/receipts/by-idempotency/") => {
            let id = path
                .strip_prefix("/v1/programs/receipts/by-idempotency/")
                .ok_or(GatewayError::InvalidRoute)?;
            if id.len() == 64
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            {
                Ok(ProductionRoute::ProgramReceiptByIdempotency(id))
            } else {
                Err(GatewayError::InvalidRoute)
            }
        }
        ("GET", path) if path.starts_with("/v1/programs/registry/") => {
            let remainder = path
                .strip_prefix("/v1/programs/registry/")
                .ok_or(GatewayError::InvalidRoute)?;
            let (id, interface) = remainder
                .strip_suffix("/interface")
                .map_or((remainder, false), |id| (id, true));
            if id.len() == 64
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            {
                if interface {
                    Ok(ProductionRoute::ProgramInterface(id))
                } else {
                    Ok(ProductionRoute::ProgramRegistry(id))
                }
            } else {
                Err(GatewayError::InvalidRoute)
            }
        }
        ("GET", path) => {
            let id = path
                .strip_prefix("/v1/receipts/")
                .ok_or(GatewayError::InvalidRoute)?;
            if id.len() == 64 && id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                Ok(ProductionRoute::Receipt(id))
            } else {
                Err(GatewayError::InvalidRoute)
            }
        }
        _ => Err(GatewayError::InvalidRoute),
    }
}

#[must_use]
pub fn platform_gateway() -> &'static str {
    "tls-receipt-verifying-multi-instance-hosted-gateway"
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(char::from(DIGITS[usize::from(byte >> 4)]));
        value.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    value
}

fn hmac_sha256(key: &[u8; 32], domain: &[u8], context: &[u8]) -> [u8; 32] {
    let mut inner_pad = [0x36_u8; 64];
    let mut outer_pad = [0x5c_u8; 64];
    for (index, byte) in key.iter().enumerate() {
        inner_pad[index] ^= byte;
        outer_pad[index] ^= byte;
    }
    let mut inner = Sha256::new();
    inner.update(inner_pad);
    inner.update((domain.len() as u64).to_be_bytes());
    inner.update(domain);
    inner.update((context.len() as u64).to_be_bytes());
    inner.update(context);
    let inner_digest = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(outer_pad);
    outer.update(inner_digest);
    inner_pad.zeroize();
    outer_pad.zeroize();
    outer.finalize().into()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GatewayError {
    Unauthenticated,
    Forbidden,
    InvalidRequest,
    InvalidRoute,
    IdempotencyConflict,
    VerificationRequired,
    UntrustedSequencer,
    ActivityMismatch,
    Receipt(ReceiptCheck),
    Encoding,
    Entropy,
    Unavailable,
}

impl Display for GatewayError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Unauthenticated => "gateway authentication refused",
            Self::Forbidden => "gateway principal is not authorized",
            Self::InvalidRequest => "gateway request is invalid",
            Self::InvalidRoute => "gateway route is not available",
            Self::IdempotencyConflict => "idempotency key conflicts with an earlier request",
            Self::VerificationRequired => "protocol response lacks verified receipt evidence",
            Self::UntrustedSequencer => "receipt sequencer is not an authorized gateway key",
            Self::ActivityMismatch => "receipt does not identify the requested activity",
            Self::Receipt(_) => "protocol receipt verification refused",
            Self::Encoding => "gateway response encoding failed",
            Self::Entropy => "secure key generation unavailable",
            Self::Unavailable => "gateway dependency unavailable",
        })
    }
}

impl std::error::Error for GatewayError {}

#[cfg(test)]
mod tests {
    use super::{production_route, IssuedKey, ProductionRoute};

    #[test]
    fn issued_key_debug_redacts_the_credential() {
        let key = IssuedKey::derive(&[7_u8; 32], b"debug-redaction");
        let output = format!("{key:?}");
        assert!(output.contains(key.id()));
        assert!(output.contains("[REDACTED]"));
        assert!(!output.contains(key.secret()));
    }

    #[test]
    fn production_program_routes_are_exact_and_bounded() {
        let identifier = "a".repeat(64);
        assert!(matches!(
            production_route("POST", "/v1/programs/call"),
            Ok(ProductionRoute::ProgramCall)
        ));
        assert!(matches!(
            production_route("POST", "/v1/programs/simulate"),
            Ok(ProductionRoute::ProgramSimulation)
        ));
        let discovery = format!("/v1/programs/registry/{identifier}");
        assert!(matches!(
            production_route("GET", &discovery),
            Ok(ProductionRoute::ProgramRegistry(value)) if value == identifier
        ));
        let interface = format!("/v1/programs/registry/{identifier}/interface");
        assert!(matches!(
            production_route("GET", &interface),
            Ok(ProductionRoute::ProgramInterface(value)) if value == identifier
        ));
        let activity = format!("/v1/programs/activities/{identifier}");
        assert!(matches!(
            production_route("GET", &activity),
            Ok(ProductionRoute::ProgramActivity(value)) if value == identifier
        ));
        let receipt = format!("/v1/programs/receipts/by-idempotency/{identifier}");
        assert!(matches!(
            production_route("GET", &receipt),
            Ok(ProductionRoute::ProgramReceiptByIdempotency(value)) if value == identifier
        ));
        assert!(production_route("GET", "/v1/programs/activities/aa").is_err());
        assert!(production_route("GET", "/v1/programs/activities/../state").is_err());
        assert!(production_route(
            "GET",
            &format!("/v1/programs/activities/{}", "A".repeat(64))
        )
        .is_err());
        assert!(production_route("GET", "/v1/programs/registry").is_err());
    }
}

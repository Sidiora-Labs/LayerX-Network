use std::fmt::{Debug, Formatter};
use std::sync::{Mutex, MutexGuard};

use layerx_crypto::disclosure::Disclosure;
use layerx_types::payload::ModuleRegistry;

use crate::audit::{
    AuditChain, AuditEvent, Decision, SigningOperation, StepUpEvidence as AuditStepUpEvidence,
};
use crate::store::{PrincipalId, PrincipalScope, PrincipalStore, RowKey, Table};
use crate::trace::TraceId;

use super::{CustodyError, KeyId, Keystore};

const RATE_KEY: &str = "custody-sign-rate";
const RATE_MAGIC: &[u8; 4] = b"LXRL";
const RATE_VERSION: u8 = 1;
const STEP_UP_KEY_PREFIX: &str = "custody-stepup-";
const STEP_UP_ID_LIMIT: usize = 96;

/// The operation class named in a custody signing decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation {
    ProtocolMutation,
    ApprovalDecision,
    SecuritySettings,
    SecretReveal,
    Withdrawal,
    EmergencyExit,
    WalletRebinding,
    AgentArchive,
}

impl Operation {
    /// Returns the stable audit label for this operation class.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::ProtocolMutation => "protocol-mutation",
            Self::ApprovalDecision => "approval-decision",
            Self::SecuritySettings => "security-settings",
            Self::SecretReveal => "secret-reveal",
            Self::Withdrawal => "withdrawal",
            Self::EmergencyExit => "emergency-exit",
            Self::WalletRebinding => "wallet-rebinding",
            Self::AgentArchive => "agent-archive",
        }
    }

    /// Returns whether this operation requires fresh step-up evidence.
    #[must_use]
    pub const fn requires_step_up(self) -> bool {
        !matches!(self, Self::ProtocolMutation)
    }
}

/// Fresh authentication evidence bound to one disclosure digest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StepUpEvidence {
    evidence_id: String,
    operation: Operation,
    disclosure_digest: [u8; 32],
    valid_from: u64,
    expires_at: u64,
}

impl StepUpEvidence {
    /// Constructs bounded, non-secret ceremony evidence.
    ///
    /// # Errors
    ///
    /// Refuses invalid identifiers and empty or inverted validity windows.
    pub fn new(
        evidence_id: impl Into<String>,
        operation: Operation,
        disclosure_digest: [u8; 32],
        valid_from: u64,
        expires_at: u64,
    ) -> Result<Self, CustodyError> {
        let evidence_id = evidence_id.into();
        if !super::valid_identifier(&evidence_id)
            || evidence_id.len() > STEP_UP_ID_LIMIT
            || disclosure_digest == [0; 32]
            || valid_from >= expires_at
        {
            return Err(CustodyError::InvalidEvidence);
        }
        Ok(Self {
            evidence_id,
            operation,
            disclosure_digest,
            valid_from,
            expires_at,
        })
    }

    /// Returns the non-secret ceremony reference.
    #[must_use]
    pub fn evidence_id(&self) -> &str {
        &self.evidence_id
    }

    /// Returns the exact operation this ceremony approved.
    #[must_use]
    pub const fn operation(&self) -> Operation {
        self.operation
    }

    /// Returns the exact disclosure digest this ceremony approved.
    #[must_use]
    pub const fn disclosure_digest(&self) -> [u8; 32] {
        self.disclosure_digest
    }

    /// Returns the beginning of the evidence validity window.
    #[must_use]
    pub const fn valid_from(&self) -> u64 {
        self.valid_from
    }

    /// Returns the exclusive end of the evidence validity window.
    #[must_use]
    pub const fn expires_at(&self) -> u64 {
        self.expires_at
    }
}

/// Declared per-principal signing throughput configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SigningLimits {
    maximum: u32,
    window: u64,
}

impl SigningLimits {
    /// Defines the maximum attempts admitted in one injected-time window.
    ///
    /// # Errors
    ///
    /// Refuses zero limits and zero-width windows.
    pub const fn new(maximum: u32, window: u64) -> Result<Self, CustodyError> {
        if maximum == 0 || window == 0 {
            return Err(CustodyError::InvalidLimits);
        }
        Ok(Self { maximum, window })
    }

    /// Returns the maximum admitted attempts per principal and window.
    #[must_use]
    pub const fn maximum(self) -> u32 {
        self.maximum
    }

    /// Returns the configured window width in protocol-time units.
    #[must_use]
    pub const fn window(self) -> u64 {
        self.window
    }
}

/// One exact, disclosure-bound request to the custody service.
#[derive(Clone, Copy, Debug)]
pub struct SignAuthorization<'a> {
    operation: Operation,
    step_up: Option<&'a StepUpEvidence>,
}

impl<'a> SignAuthorization<'a> {
    #[must_use]
    pub const fn new(operation: Operation, step_up: Option<&'a StepUpEvidence>) -> Self {
        Self { operation, step_up }
    }
}

/// One exact, disclosure-bound request to the custody service.
pub struct SignRequest<'a> {
    principal: &'a PrincipalId,
    key: &'a KeyId,
    trace: &'a TraceId,
    operation: Operation,
    canonical_bytes: &'a [u8],
    disclosure: &'a Disclosure,
    step_up: Option<&'a StepUpEvidence>,
    now: u64,
}

impl<'a> SignRequest<'a> {
    /// Couples the authenticated scope, held key and exact prepared bytes.
    #[must_use]
    pub const fn new(
        principal: &'a PrincipalId,
        key: &'a KeyId,
        trace: &'a TraceId,
        authorization: SignAuthorization<'a>,
        canonical_bytes: &'a [u8],
        disclosure: &'a Disclosure,
        now: u64,
    ) -> Self {
        Self {
            principal,
            key,
            trace,
            operation: authorization.operation,
            canonical_bytes,
            disclosure,
            step_up: authorization.step_up,
            now,
        }
    }
}

impl Debug for SignRequest<'_> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SignRequest")
            .field("principal", self.principal)
            .field("key", self.key)
            .field("trace", self.trace)
            .field("operation", &self.operation)
            .field("canonical_bytes", &"[redacted]")
            .field("disclosure", &"[validated at signing]")
            .field("step_up", &self.step_up.map(StepUpEvidence::evidence_id))
            .field("now", &self.now)
            .finish()
    }
}

/// Public material returned after one audited custody signing grant.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct SignatureGrant {
    signature: [u8; 64],
    signer_public_key: [u8; 32],
    disclosure_digest: [u8; 32],
}

impl SignatureGrant {
    /// Returns the Ed25519 signature bytes.
    #[must_use]
    pub const fn signature(&self) -> &[u8; 64] {
        &self.signature
    }

    /// Returns the public key that produced the signature.
    #[must_use]
    pub const fn signer_public_key(&self) -> [u8; 32] {
        self.signer_public_key
    }

    /// Returns the domain-separated disclosure digest audited for the grant.
    #[must_use]
    pub const fn disclosure_digest(&self) -> [u8; 32] {
        self.disclosure_digest
    }
}

impl Debug for SignatureGrant {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SignatureGrant")
            .field("signature", &"[public signature]")
            .field("signer_public_key", &self.signer_public_key)
            .field("disclosure_digest", &self.disclosure_digest)
            .finish()
    }
}

/// KMS-backed custody signer using the agent layer's disclosure-bound signer
/// contract and the principal store's durable rate and audit records.
pub struct CustodySigner {
    keystore: Keystore,
    store: Mutex<PrincipalStore>,
    registry: ModuleRegistry,
    limits: SigningLimits,
}

impl CustodySigner {
    /// Binds one keystore, principal store, negotiated module registry and
    /// declared throughput policy into a custody signing service.
    #[must_use]
    pub fn new(
        keystore: Keystore,
        store: PrincipalStore,
        registry: ModuleRegistry,
        limits: SigningLimits,
    ) -> Self {
        Self {
            keystore,
            store: Mutex::new(store),
            registry,
            limits,
        }
    }

    /// Returns the public descriptor for a principal's held key.
    ///
    /// # Errors
    ///
    /// Returns typed key-storage refusals without opening the KMS envelope.
    pub fn describe_key(
        &self,
        principal: &PrincipalId,
        key: &KeyId,
    ) -> Result<super::KeyDescriptor, CustodyError> {
        self.keystore.describe(principal, key)
    }

    /// Signs one exact disclosure-bound request and durably audits the grant
    /// or refusal before returning it.
    ///
    /// # Errors
    ///
    /// Refuses invalid disclosure bindings, missing or stale step-up evidence,
    /// exceeded throughput, unavailable KMS material and audit failures.
    pub async fn sign(&self, request: SignRequest<'_>) -> Result<SignatureGrant, CustodyError> {
        let Ok(disclosure_digest) = request.disclosure.audit_digest() else {
            let error = CustodyError::Sign(layerx_crypto::signer::SignError::InvalidDisclosure);
            self.append_decision(&request, None, Some(&error))?;
            return Err(error);
        };

        if let Err(error) = validate_step_up(&request, &disclosure_digest) {
            self.append_decision(&request, Some(disclosure_digest), Some(&error))?;
            return Err(error);
        }
        if let Some(evidence) = request
            .step_up
            .filter(|_| request.operation.requires_step_up())
        {
            if let Err(error) = self.consume_step_up(request.principal, evidence, request.now) {
                self.append_decision(&request, Some(disclosure_digest), Some(&error))?;
                return Err(error);
            }
        }
        if let Err(error) = self.consume_rate(request.principal, request.now) {
            self.append_decision(&request, Some(disclosure_digest), Some(&error))?;
            return Err(error);
        }

        let signer = match self.keystore.remote_signer(request.principal, request.key) {
            Ok(signer) => signer,
            Err(error) => {
                self.append_decision(&request, Some(disclosure_digest), Some(&error))?;
                return Err(error);
            }
        };
        let signer_public_key = signer.public_key();
        let signature = match signer
            .sign_disclosed(request.canonical_bytes, request.disclosure, &self.registry)
            .await
        {
            Ok(signature) => signature,
            Err(error) => {
                self.append_decision(&request, Some(disclosure_digest), Some(&error))?;
                return Err(error);
            }
        };
        self.append_decision(&request, Some(disclosure_digest), None)?;
        Ok(SignatureGrant {
            signature,
            signer_public_key,
            disclosure_digest,
        })
    }

    /// Signs through the same KMS and disclosure gate while recording rate,
    /// step-up and audit state in an already-open principal scope. Journey
    /// engines use this form so a subsequent journey write cannot overwrite a
    /// signing audit appended through a second stale store handle.
    ///
    /// # Errors
    ///
    /// Returns the same typed refusals as [`Self::sign`] and additionally
    /// refuses a request whose principal differs from the supplied scope.
    pub async fn sign_in_scope(
        &self,
        scope: &mut PrincipalScope<'_>,
        request: SignRequest<'_>,
    ) -> Result<SignatureGrant, CustodyError> {
        if scope.principal() != request.principal {
            return Err(CustodyError::InvalidEvidence);
        }
        let Ok(disclosure_digest) = request.disclosure.audit_digest() else {
            let error = CustodyError::Sign(layerx_crypto::signer::SignError::InvalidDisclosure);
            append_decision_to_scope(scope, &request, None, Some(&error))?;
            return Err(error);
        };
        if let Err(error) = validate_step_up(&request, &disclosure_digest) {
            append_decision_to_scope(scope, &request, Some(disclosure_digest), Some(&error))?;
            return Err(error);
        }
        if let Some(evidence) = request
            .step_up
            .filter(|_| request.operation.requires_step_up())
        {
            if let Err(error) = consume_step_up_in_scope(scope, evidence, request.now) {
                append_decision_to_scope(scope, &request, Some(disclosure_digest), Some(&error))?;
                return Err(error);
            }
        }
        if let Err(error) = consume_rate_in_scope(scope, self.limits, request.now) {
            append_decision_to_scope(scope, &request, Some(disclosure_digest), Some(&error))?;
            return Err(error);
        }

        let signer = match self.keystore.remote_signer(request.principal, request.key) {
            Ok(signer) => signer,
            Err(error) => {
                append_decision_to_scope(scope, &request, Some(disclosure_digest), Some(&error))?;
                return Err(error);
            }
        };
        let signer_public_key = signer.public_key();
        let signature = match signer
            .sign_disclosed(request.canonical_bytes, request.disclosure, &self.registry)
            .await
        {
            Ok(signature) => signature,
            Err(error) => {
                append_decision_to_scope(scope, &request, Some(disclosure_digest), Some(&error))?;
                return Err(error);
            }
        };
        append_decision_to_scope(scope, &request, Some(disclosure_digest), None)?;
        Ok(SignatureGrant {
            signature,
            signer_public_key,
            disclosure_digest,
        })
    }

    fn store(&self) -> Result<MutexGuard<'_, PrincipalStore>, CustodyError> {
        self.store
            .lock()
            .map_err(|_| CustodyError::CoordinationUnavailable)
    }

    fn consume_rate(&self, principal: &PrincipalId, now: u64) -> Result<(), CustodyError> {
        let mut store = self.store()?;
        let mut scope = store.principal(principal).map_err(CustodyError::Store)?;
        let key = RowKey::new(RATE_KEY).map_err(CustodyError::Store)?;
        let mut state = match scope.get(Table::Cache, &key) {
            Some(row) => RateState::decode(row.bytes())?,
            None => RateState {
                window_start: now,
                attempts: 0,
            },
        };
        if now < state.window_start {
            return Err(CustodyError::NonMonotonicTime);
        }
        let retry_at = state.window_start.saturating_add(self.limits.window);
        if now >= retry_at {
            state = RateState {
                window_start: now,
                attempts: 0,
            };
        } else if state.attempts >= self.limits.maximum {
            return Err(CustodyError::ThroughputExceeded { retry_at });
        }
        state.attempts = state
            .attempts
            .checked_add(1)
            .ok_or(CustodyError::CorruptState("signing attempt count overflow"))?;
        scope
            .put(Table::Cache, key, now, state.encode().to_vec())
            .map_err(CustodyError::Store)
    }

    fn consume_step_up(
        &self,
        principal: &PrincipalId,
        evidence: &StepUpEvidence,
        now: u64,
    ) -> Result<(), CustodyError> {
        let mut store = self.store()?;
        let mut scope = store.principal(principal).map_err(CustodyError::Store)?;
        let key = RowKey::new(format!("{STEP_UP_KEY_PREFIX}{}", evidence.evidence_id))
            .map_err(CustodyError::Store)?;
        if scope.get(Table::Cache, &key).is_some() {
            return Err(CustodyError::StepUpReplayed);
        }
        let bytes = format!(
            "version=1\noperation={}\ndisclosure_digest={}\nvalid_from={}\nexpires_at={}\n",
            evidence.operation.label(),
            hex(evidence.disclosure_digest),
            evidence.valid_from,
            evidence.expires_at,
        )
        .into_bytes();
        scope
            .put(Table::Cache, key, now, bytes)
            .map_err(CustodyError::Store)
    }

    fn append_decision(
        &self,
        request: &SignRequest<'_>,
        disclosure_digest: Option<[u8; 32]>,
        refusal: Option<&CustodyError>,
    ) -> Result<(), CustodyError> {
        let mut store = self.store()?;
        let mut scope = store
            .principal(request.principal)
            .map_err(|error| CustodyError::Audit(error.into()))?;
        let digest = disclosure_digest.unwrap_or([0; 32]);
        let step_up = match request.step_up {
            Some(evidence) => AuditStepUpEvidence::Fresh {
                ceremony_digest: ceremony_digest(evidence)?,
            },
            None if request.operation.requires_step_up() => AuditStepUpEvidence::Missing,
            None => AuditStepUpEvidence::NotRequired,
        };
        let event = AuditEvent::SigningDecision {
            operation: signing_operation(request),
            disclosure_digest: digest,
            step_up,
            outcome: if refusal.is_some() {
                Decision::Refused
            } else {
                Decision::Granted
            },
        };
        let mut chain = AuditChain::open(&scope).map_err(CustodyError::Audit)?;
        chain
            .append(&mut scope, request.now, request.trace, &event, &[])
            .map(|_| ())
            .map_err(CustodyError::Audit)
    }
}

fn consume_rate_in_scope(
    scope: &mut PrincipalScope<'_>,
    limits: SigningLimits,
    now: u64,
) -> Result<(), CustodyError> {
    let key = RowKey::new(RATE_KEY).map_err(CustodyError::Store)?;
    let mut state = match scope.get(Table::Cache, &key) {
        Some(row) => RateState::decode(row.bytes())?,
        None => RateState {
            window_start: now,
            attempts: 0,
        },
    };
    if now < state.window_start {
        return Err(CustodyError::NonMonotonicTime);
    }
    let retry_at = state.window_start.saturating_add(limits.window);
    if now >= retry_at {
        state = RateState {
            window_start: now,
            attempts: 0,
        };
    } else if state.attempts >= limits.maximum {
        return Err(CustodyError::ThroughputExceeded { retry_at });
    }
    state.attempts = state
        .attempts
        .checked_add(1)
        .ok_or(CustodyError::CorruptState("signing attempt count overflow"))?;
    scope
        .put(Table::Cache, key, now, state.encode().to_vec())
        .map_err(CustodyError::Store)
}

fn consume_step_up_in_scope(
    scope: &mut PrincipalScope<'_>,
    evidence: &StepUpEvidence,
    now: u64,
) -> Result<(), CustodyError> {
    let key = RowKey::new(format!("{STEP_UP_KEY_PREFIX}{}", evidence.evidence_id))
        .map_err(CustodyError::Store)?;
    if scope.get(Table::Cache, &key).is_some() {
        return Err(CustodyError::StepUpReplayed);
    }
    let bytes = format!(
        "version=1\noperation={}\ndisclosure_digest={}\nvalid_from={}\nexpires_at={}\n",
        evidence.operation.label(),
        hex(evidence.disclosure_digest),
        evidence.valid_from,
        evidence.expires_at,
    )
    .into_bytes();
    scope
        .put(Table::Cache, key, now, bytes)
        .map_err(CustodyError::Store)
}

fn append_decision_to_scope(
    scope: &mut PrincipalScope<'_>,
    request: &SignRequest<'_>,
    disclosure_digest: Option<[u8; 32]>,
    refusal: Option<&CustodyError>,
) -> Result<(), CustodyError> {
    let digest = disclosure_digest.unwrap_or([0; 32]);
    let step_up = match request.step_up {
        Some(evidence) => AuditStepUpEvidence::Fresh {
            ceremony_digest: ceremony_digest(evidence)?,
        },
        None if request.operation.requires_step_up() => AuditStepUpEvidence::Missing,
        None => AuditStepUpEvidence::NotRequired,
    };
    let event = AuditEvent::SigningDecision {
        operation: signing_operation(request),
        disclosure_digest: digest,
        step_up,
        outcome: if refusal.is_some() {
            Decision::Refused
        } else {
            Decision::Granted
        },
    };
    let mut chain = AuditChain::open(scope).map_err(CustodyError::Audit)?;
    chain
        .append(scope, request.now, request.trace, &event, &[])
        .map(|_| ())
        .map_err(CustodyError::Audit)
}

impl Debug for CustodySigner {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CustodySigner")
            .field("keystore", &"[KMS-backed]")
            .field("store", &"[principal-scoped]")
            .field("registry", &self.registry)
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

fn validate_step_up(
    request: &SignRequest<'_>,
    disclosure_digest: &[u8; 32],
) -> Result<(), CustodyError> {
    let evidence = match request.step_up {
        Some(evidence) => evidence,
        None if request.operation.requires_step_up() => return Err(CustodyError::StepUpRequired),
        None => return Ok(()),
    };
    if evidence.operation != request.operation {
        return Err(CustodyError::StepUpOperationMismatch);
    }
    if !layerx_crypto::ct::eq_fixed(&evidence.disclosure_digest, disclosure_digest) {
        return Err(CustodyError::StepUpMismatch);
    }
    if request.now < evidence.valid_from {
        return Err(CustodyError::StepUpNotYetValid);
    }
    if request.now >= evidence.expires_at {
        return Err(CustodyError::StepUpExpired);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RateState {
    window_start: u64,
    attempts: u32,
}

impl RateState {
    fn encode(self) -> [u8; 17] {
        let mut bytes = [0_u8; 17];
        bytes[..4].copy_from_slice(RATE_MAGIC);
        bytes[4] = RATE_VERSION;
        bytes[5..13].copy_from_slice(&self.window_start.to_be_bytes());
        bytes[13..17].copy_from_slice(&self.attempts.to_be_bytes());
        bytes
    }

    fn decode(bytes: &[u8]) -> Result<Self, CustodyError> {
        if bytes.len() != 17 || bytes.get(..4) != Some(RATE_MAGIC) || bytes[4] != RATE_VERSION {
            return Err(CustodyError::CorruptState("invalid signing rate record"));
        }
        let window_start = u64::from_be_bytes(
            bytes[5..13]
                .try_into()
                .map_err(|_| CustodyError::CorruptState("truncated signing rate record"))?,
        );
        let attempts = u32::from_be_bytes(
            bytes[13..17]
                .try_into()
                .map_err(|_| CustodyError::CorruptState("truncated signing rate record"))?,
        );
        Ok(Self {
            window_start,
            attempts,
        })
    }
}

fn signing_operation(request: &SignRequest<'_>) -> SigningOperation {
    match request.operation {
        Operation::ProtocolMutation => SigningOperation::ProtocolMutation,
        Operation::ApprovalDecision => SigningOperation::ApprovalDecision,
        Operation::SecuritySettings => SigningOperation::SecuritySettings,
        Operation::SecretReveal => SigningOperation::SecretReveal,
        Operation::Withdrawal => SigningOperation::BridgeWithdrawRequest,
        Operation::EmergencyExit => SigningOperation::EmergencyExit,
        Operation::WalletRebinding => SigningOperation::EvmPayoutBinding,
        Operation::AgentArchive => SigningOperation::AgentArchive,
    }
}

fn ceremony_digest(evidence: &StepUpEvidence) -> Result<[u8; 32], CustodyError> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"layerx-human-step-up-audit/v1");
    bytes.extend_from_slice(evidence.evidence_id.as_bytes());
    bytes.extend_from_slice(evidence.operation.label().as_bytes());
    bytes.extend_from_slice(&evidence.disclosure_digest);
    bytes.extend_from_slice(&evidence.valid_from.to_be_bytes());
    bytes.extend_from_slice(&evidence.expires_at.to_be_bytes());
    layerx_proof::merkle::leaf_hash(&bytes).map_err(|_| CustodyError::InvalidEvidence)
}

fn hex(bytes: [u8; 32]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

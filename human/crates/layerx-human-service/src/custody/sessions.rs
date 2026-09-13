//! Managed-agent protocol sessions and primary-key rotation orchestration.

use std::collections::BTreeMap;
use std::fmt::{Debug, Display, Formatter};

use layerx_crypto::local::LocalSigner;
use layerx_crypto::session::{
    issue_session_key, IssuedSessionKey, SessionIssueError, SessionKeyRequest,
};
use layerx_crypto::signer::Signer as _;
use layerx_intents::{
    compile, CompileError, DisclosureCheck, DisclosureCheckError, Intent, IntentError, IntentKind,
    KeyRotation,
};
use layerx_types::activity::TimestampBound;
use layerx_types::ids::Did;
use layerx_types::intent::{PublicKey, Sequence};
use layerx_types::payload::{ActivityType, ModuleRegistry};
use layerx_types::verify::VerificationLevel;
use sha2::{Digest as _, Sha256};
use zeroize::Zeroizing;

use crate::store::PrincipalId;

use super::{CustodyError, KeyClass, KeyId, Keystore};

const SESSION_RECEIPT_DOMAIN: &[u8] = b"layerx-human/agent-session-receipt/v1";
const SUSPENSION_RECEIPT_DOMAIN: &[u8] = b"layerx-human/agent-session-suspension/v1";
const REVOCATION_RECEIPT_DOMAIN: &[u8] = b"layerx-human/agent-session-revocation/v1";
const ROTATION_RECEIPT_DOMAIN: &[u8] = b"layerx-human/key-rotation-receipt/v1";

/// Declared policy for every managed-agent operating session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionPolicy {
    lifetime_seconds: u64,
    renewal_lead_seconds: u64,
    maximum_revocation_latency_seconds: u64,
    daemon_scopes: Vec<String>,
    policy_version: String,
}

impl SessionPolicy {
    /// Constructs a complete session policy without implicit permissions.
    ///
    /// # Errors
    ///
    /// Refuses zero or inverted timing, empty daemon scopes, duplicate scopes,
    /// and an unnamed daemon policy.
    pub fn new(
        lifetime_seconds: u64,
        renewal_lead_seconds: u64,
        maximum_revocation_latency_seconds: u64,
        mut daemon_scopes: Vec<String>,
        policy_version: impl Into<String>,
    ) -> Result<Self, SessionKeyError> {
        let policy_version = policy_version.into();
        if lifetime_seconds == 0
            || renewal_lead_seconds == 0
            || renewal_lead_seconds >= lifetime_seconds
            || maximum_revocation_latency_seconds == 0
            || daemon_scopes.is_empty()
            || daemon_scopes.iter().any(String::is_empty)
            || policy_version.is_empty()
        {
            return Err(SessionKeyError::InvalidPolicy);
        }
        daemon_scopes.sort();
        if daemon_scopes.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(SessionKeyError::InvalidPolicy);
        }
        Ok(Self {
            lifetime_seconds,
            renewal_lead_seconds,
            maximum_revocation_latency_seconds,
            daemon_scopes,
            policy_version,
        })
    }

    /// Returns the exact protocol validity duration.
    #[must_use]
    pub const fn lifetime_seconds(&self) -> u64 {
        self.lifetime_seconds
    }

    /// Returns how far ahead the unattended renewal scheduler acts.
    #[must_use]
    pub const fn renewal_lead_seconds(&self) -> u64 {
        self.renewal_lead_seconds
    }

    /// Returns the declared end-to-end pause/archive revocation target.
    #[must_use]
    pub const fn maximum_revocation_latency_seconds(&self) -> u64 {
        self.maximum_revocation_latency_seconds
    }
}

/// Current verified protocol identity facts required to issue authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolIdentitySnapshot {
    pub protocol_identity: [u8; 32],
    pub primary_public_key: [u8; 32],
    pub revocation_sequence: u64,
    pub protocol_time: u64,
    pub core_sequence: u64,
    pub verification_level: VerificationLevel,
}

/// A session seed that can only leave the Human service by being consumed by
/// the agent contract. It is never clonable or printable and zeroizes on drop.
pub struct AgentSessionSecret(Zeroizing<[u8; 32]>);

impl AgentSessionSecret {
    /// Consumes the transfer object at the trusted agent boundary.
    #[must_use]
    pub fn into_seed(self) -> [u8; 32] {
        *self.0
    }
}

impl Debug for AgentSessionSecret {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AgentSessionSecret([redacted])")
    }
}

/// Independently generated entropy for one operating session. The value is
/// injected from the configured entropy boundary and is zeroized if
/// provisioning is refused before the agent consumes it.
pub struct SessionKeyEntropy(Zeroizing<[u8; 32]>);

impl SessionKeyEntropy {
    /// Wraps one non-zero 256-bit seed.
    ///
    /// # Errors
    ///
    /// Refuses the reserved all-zero value.
    pub fn new(seed: [u8; 32]) -> Result<Self, SessionKeyError> {
        let seed = Zeroizing::new(seed);
        if seed.iter().all(|byte| *byte == 0) {
            Err(SessionKeyError::EntropyUnavailable)
        } else {
            Ok(Self(seed))
        }
    }

    fn into_secret(self) -> AgentSessionSecret {
        AgentSessionSecret(self.0)
    }
}

impl Debug for SessionKeyEntropy {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SessionKeyEntropy([redacted])")
    }
}

/// Configured entropy boundary used by initial provisioning and unattended
/// renewal. The Human orchestration layer never reaches ambient randomness.
pub trait SessionEntropySource {
    /// Returns fresh independently generated entropy for exactly one session.
    ///
    /// # Errors
    ///
    /// Returns an entropy-boundary refusal without providing partial material.
    fn next_session_entropy(&mut self) -> Result<SessionKeyEntropy, SessionKeyError>;
}

/// Exact protocol and daemon material sent to the agent contract. No primary
/// key or primary-key handle is representable here.
pub struct AgentSessionProvision {
    pub principal: PrincipalId,
    pub agent_did: Did,
    pub issued: IssuedSessionKey,
    pub daemon_scopes: Vec<String>,
    pub daemon_policy_version: String,
    secret: AgentSessionSecret,
}

impl AgentSessionProvision {
    /// Consumes the request into its public metadata and session-only secret.
    #[must_use]
    pub fn into_parts(
        self,
    ) -> (
        PrincipalId,
        Did,
        IssuedSessionKey,
        Vec<String>,
        String,
        AgentSessionSecret,
    ) {
        (
            self.principal,
            self.agent_did,
            self.issued,
            self.daemon_scopes,
            self.daemon_policy_version,
            self.secret,
        )
    }
}

impl Debug for AgentSessionProvision {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AgentSessionProvision")
            .field("principal", &self.principal)
            .field("agent_did", &self.agent_did)
            .field("grant_id", &self.issued.grant_id)
            .field("session_public_key", &self.issued.session_public_key)
            .field("expires_at", &self.issued.expires_at)
            .field("daemon_scopes", &self.daemon_scopes)
            .field("daemon_policy_version", &self.daemon_policy_version)
            .field("secret", &"[redacted]")
            .finish()
    }
}

/// Proof that the protocol grant and daemon permission session were installed
/// as one joined operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProvisionEvidence {
    pub grant_id: [u8; 32],
    pub session_public_key: [u8; 32],
    pub daemon_session_id: [u8; 32],
    pub protocol_sequence: u64,
    pub observed_at: u64,
    pub verification_level: VerificationLevel,
    pub receipt_digest: [u8; 32],
}

impl ProvisionEvidence {
    /// Computes the receipt digest the service independently verifies.
    #[must_use]
    pub fn expected_digest(&self) -> [u8; 32] {
        digest_fields(
            SESSION_RECEIPT_DOMAIN,
            &[
                &self.grant_id,
                &self.session_public_key,
                &self.daemon_session_id,
                &self.protocol_sequence.to_be_bytes(),
                &self.observed_at.to_be_bytes(),
                &[self.verification_level.wire_rank()],
            ],
        )
    }
}

/// Exact target for daemon suspension and protocol revocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionTarget {
    pub principal: PrincipalId,
    pub agent_did: Did,
    pub grant_id: [u8; 32],
    pub daemon_session_id: [u8; 32],
}

/// Why operating authority was removed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RevocationReason {
    Paused,
    Archived,
    Renewed,
    PrimaryKeyRotated,
}

impl RevocationReason {
    const fn code(self) -> u8 {
        match self {
            Self::Paused => 1,
            Self::Archived => 2,
            Self::Renewed => 3,
            Self::PrimaryKeyRotated => 4,
        }
    }
}

/// Daemon-side permission suspension evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SuspensionEvidence {
    pub grant_id: [u8; 32],
    pub daemon_session_id: [u8; 32],
    pub reason: RevocationReason,
    pub observed_at: u64,
    pub receipt_digest: [u8; 32],
}

impl SuspensionEvidence {
    #[must_use]
    pub fn expected_digest(&self) -> [u8; 32] {
        digest_fields(
            SUSPENSION_RECEIPT_DOMAIN,
            &[
                &self.grant_id,
                &self.daemon_session_id,
                &[self.reason.code()],
                &self.observed_at.to_be_bytes(),
            ],
        )
    }
}

/// Protocol-side authority revocation evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RevocationEvidence {
    pub grant_id: [u8; 32],
    pub reason: RevocationReason,
    pub observed_sequence: u64,
    pub observed_at: u64,
    pub verification_level: VerificationLevel,
    pub receipt_digest: [u8; 32],
}

impl RevocationEvidence {
    #[must_use]
    pub fn expected_digest(&self) -> [u8; 32] {
        digest_fields(
            REVOCATION_RECEIPT_DOMAIN,
            &[
                &self.grant_id,
                &[self.reason.code()],
                &self.observed_sequence.to_be_bytes(),
                &self.observed_at.to_be_bytes(),
                &[self.verification_level.wire_rank()],
            ],
        )
    }
}

/// Protocol key-rotation submission using only the Human intent authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RotationSubmission {
    pub principal: PrincipalId,
    pub subject: RotationSubject,
    pub did: Did,
    pub current_public_key: [u8; 32],
    pub pending_public_key: [u8; 32],
    pub effective_at: u64,
    pub lapse_at: u64,
    pub effective_sequence: u64,
    pub intent: Intent,
    pub compiled: layerx_intents::CompiledIntent,
    pub disclosure: DisclosureCheck,
}

/// Proof-bearing receipt for the key-rotation announcement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RotationEvidence {
    pub payload_hash: [u8; 32],
    pub pending_public_key: [u8; 32],
    pub effective_at: u64,
    pub lapse_at: u64,
    pub effective_sequence: u64,
    pub observed_sequence: u64,
    pub observed_at: u64,
    pub verification_level: VerificationLevel,
    pub receipt_digest: [u8; 32],
}

impl RotationEvidence {
    #[must_use]
    pub fn expected_digest(&self) -> [u8; 32] {
        digest_fields(
            ROTATION_RECEIPT_DOMAIN,
            &[
                &self.payload_hash,
                &self.pending_public_key,
                &self.effective_at.to_be_bytes(),
                &self.lapse_at.to_be_bytes(),
                &self.effective_sequence.to_be_bytes(),
                &self.observed_sequence.to_be_bytes(),
                &self.observed_at.to_be_bytes(),
                &[self.verification_level.wire_rank()],
            ],
        )
    }
}

/// State-proven identity rotation observation returned by the agent contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RotationObservation {
    pub primary_public_key: [u8; 32],
    pub pending_public_key: Option<[u8; 32]>,
    pub superseded_public_key: Option<[u8; 32]>,
    pub effective_at: u64,
    pub lapse_at: u64,
    pub effective_sequence: u64,
    pub observed_at: u64,
    pub observed_sequence: u64,
    pub verification_level: VerificationLevel,
}

/// Stable errors at the versioned agent boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentContractError {
    Unavailable,
    Refused(&'static str),
}

impl Display for AgentContractError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable => formatter.write_str("agent contract is unavailable"),
            Self::Refused(reason) => write!(formatter, "agent contract refused: {reason}"),
        }
    }
}

impl std::error::Error for AgentContractError {}

/// Versioned Human-to-agent contract. It deliberately exposes session-only
/// material and public rotation intents, never a primary private key.
pub trait AgentSessionContract {
    /// Reads state-proven identity and revocation facts.
    ///
    /// # Errors
    ///
    /// Returns a typed boundary failure or protocol refusal.
    fn identity_snapshot(
        &mut self,
        principal: &PrincipalId,
        did: &Did,
    ) -> Result<ProtocolIdentitySnapshot, AgentContractError>;

    /// Installs one protocol grant and its session-only signer.
    ///
    /// # Errors
    ///
    /// Returns a typed refusal without installing partial authority.
    fn provision_session(
        &mut self,
        request: AgentSessionProvision,
    ) -> Result<ProvisionEvidence, AgentContractError>;

    /// Suspends the exact daemon permission session.
    ///
    /// # Errors
    ///
    /// Returns a typed refusal when suspension cannot be proven.
    fn suspend_permissions(
        &mut self,
        target: &SessionTarget,
        reason: RevocationReason,
        requested_at: u64,
    ) -> Result<SuspensionEvidence, AgentContractError>;

    /// Revokes the exact protocol session authority.
    ///
    /// # Errors
    ///
    /// Returns a typed refusal when revocation cannot be proven.
    fn revoke_protocol_session(
        &mut self,
        target: &SessionTarget,
        reason: RevocationReason,
        requested_at: u64,
    ) -> Result<RevocationEvidence, AgentContractError>;

    /// Submits one byte-verified Human rotation intent.
    ///
    /// # Errors
    ///
    /// Returns a typed protocol or boundary refusal.
    fn announce_rotation(
        &mut self,
        submission: RotationSubmission,
    ) -> Result<RotationEvidence, AgentContractError>;

    /// Reads state-proven rotation state.
    ///
    /// # Errors
    ///
    /// Returns a typed refusal when current state cannot be proven.
    fn rotation_observation(
        &mut self,
        principal: &PrincipalId,
        did: &Did,
    ) -> Result<RotationObservation, AgentContractError>;
}

/// Human or managed-agent primary identity being rotated.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RotationSubject {
    Human,
    Agent,
}

impl RotationSubject {
    const fn key_class(self) -> KeyClass {
        match self {
            Self::Human => KeyClass::HumanPrimary,
            Self::Agent => KeyClass::AgentPrimary,
        }
    }
}

/// An exact duration rendered without protocol terminology.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlainTime {
    seconds: u64,
    label: String,
}

impl PlainTime {
    fn new(seconds: u64) -> Result<Self, SessionKeyError> {
        if seconds == 0 {
            return Err(SessionKeyError::InvalidRotationWindow);
        }
        let (value, unit) = if seconds.is_multiple_of(86_400) {
            (seconds / 86_400, "day")
        } else if seconds.is_multiple_of(3_600) {
            (seconds / 3_600, "hour")
        } else if seconds.is_multiple_of(60) {
            (seconds / 60, "minute")
        } else {
            (seconds, "second")
        };
        let suffix = if value == 1 { "" } else { "s" };
        Ok(Self {
            seconds,
            label: format!("{value} {unit}{suffix}"),
        })
    }

    #[must_use]
    pub const fn seconds(&self) -> u64 {
        self.seconds
    }
}

impl Display for PlainTime {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.label)
    }
}

/// Public, non-secret record of one live or retired agent session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionLease {
    pub principal: PrincipalId,
    pub agent_did: Did,
    pub session_public_key: [u8; 32],
    pub grant_id: [u8; 32],
    pub daemon_session_id: [u8; 32],
    pub permitted_activity_types: Vec<ActivityType>,
    pub issued_at: u64,
    pub expires_at: u64,
    pub revocation_sequence: u64,
    pub provision_receipt_digest: [u8; 32],
    pub state: SessionLeaseState,
}

/// Honest lifecycle state for one issued session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionLeaseState {
    Active,
    SuspendedAwaitingProtocolRevocation {
        reason: RevocationReason,
        suspended_at: u64,
        suspension_receipt_digest: [u8; 32],
    },
    Revoked {
        reason: RevocationReason,
        suspended_at: u64,
        revoked_at: u64,
        suspension_receipt_digest: [u8; 32],
        revocation_receipt_digest: [u8; 32],
    },
}

/// Current managed-agent lifecycle at the Human boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManagedAgentState {
    Active,
    Paused,
    Archived,
}

/// Completed authority-removal evidence including measured latency.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RevocationOutcome {
    pub reason: RevocationReason,
    pub requested_at: u64,
    pub suspended_at: u64,
    pub revoked_at: u64,
    pub latency_seconds: u64,
    pub within_declared_target: bool,
    pub suspension_receipt_digest: [u8; 32],
    pub revocation_receipt_digest: [u8; 32],
}

/// One automatic replacement, requiring no human ceremony.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenewalOutcome {
    pub previous_grant_id: [u8; 32],
    pub replacement: SessionLease,
    pub previous_revocation: RevocationOutcome,
}

/// Stored rotation journey with exact protocol windows and public key handles.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RotationJourney {
    pub principal: PrincipalId,
    pub subject: RotationSubject,
    pub did: Did,
    pub current_key_id: KeyId,
    pub pending_key_id: KeyId,
    pub current_public_key: [u8; 32],
    pub pending_public_key: [u8; 32],
    pub announced_at: u64,
    pub effective_at: u64,
    pub lapse_at: u64,
    pub effective_sequence: u64,
    pub challenge_delay: PlainTime,
    pub announcement_receipt_digest: [u8; 32],
    pub state: RotationJourneyState,
}

/// Honest rotation state, including the exact superseded-key sequence window.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RotationJourneyState {
    ChallengeOpen,
    ReadyToCommit,
    Effective {
        superseded_key_usable_before_sequence: u64,
        observed_sequence: u64,
    },
    Lapsed,
}

/// Refusals from provisioning, lifecycle propagation, and rotation.
#[derive(Debug)]
pub enum SessionKeyError {
    InvalidPolicy,
    InvalidIdentity,
    InvalidActivityScope,
    InvalidEvidence,
    InvalidRotationWindow,
    TimeOverflow,
    EntropyUnavailable,
    NotProvisioned,
    AlreadyProvisioned,
    Paused,
    Archived,
    RotationAlreadyOpen,
    RotationNotFound,
    KeyClassMismatch,
    SessionIssue(SessionIssueError),
    Intent(IntentError),
    Compile(CompileError),
    Disclosure(DisclosureCheckError),
    Custody(CustodyError),
    Agent(AgentContractError),
}

impl Display for SessionKeyError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPolicy => formatter.write_str("managed-agent session policy is invalid"),
            Self::InvalidIdentity => formatter.write_str("agent identity evidence is invalid"),
            Self::InvalidActivityScope => formatter.write_str("permitted activity set is empty"),
            Self::InvalidEvidence => {
                formatter.write_str("agent evidence did not match the request")
            }
            Self::InvalidRotationWindow => formatter.write_str("key rotation window is invalid"),
            Self::TimeOverflow => formatter.write_str("protocol time window overflowed"),
            Self::EntropyUnavailable => formatter.write_str("session-key entropy is unavailable"),
            Self::NotProvisioned => formatter.write_str("managed agent has no active authority"),
            Self::AlreadyProvisioned => {
                formatter.write_str("managed agent already has active authority")
            }
            Self::Paused => formatter.write_str("managed agent is paused"),
            Self::Archived => formatter.write_str("managed agent is archived"),
            Self::RotationAlreadyOpen => {
                formatter.write_str("identity already has an open rotation")
            }
            Self::RotationNotFound => formatter.write_str("identity has no tracked rotation"),
            Self::KeyClassMismatch => {
                formatter.write_str("custody key does not match the rotation subject")
            }
            Self::SessionIssue(error) => write!(formatter, "{error}"),
            Self::Intent(error) => write!(formatter, "intent refused: {error:?}"),
            Self::Compile(error) => write!(formatter, "intent compilation refused: {error:?}"),
            Self::Disclosure(error) => write!(formatter, "intent disclosure refused: {error:?}"),
            Self::Custody(error) => write!(formatter, "{error}"),
            Self::Agent(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for SessionKeyError {}

impl From<SessionIssueError> for SessionKeyError {
    fn from(value: SessionIssueError) -> Self {
        Self::SessionIssue(value)
    }
}

impl From<IntentError> for SessionKeyError {
    fn from(value: IntentError) -> Self {
        Self::Intent(value)
    }
}

impl From<CompileError> for SessionKeyError {
    fn from(value: CompileError) -> Self {
        Self::Compile(value)
    }
}

impl From<DisclosureCheckError> for SessionKeyError {
    fn from(value: DisclosureCheckError) -> Self {
        Self::Disclosure(value)
    }
}

impl From<CustodyError> for SessionKeyError {
    fn from(value: CustodyError) -> Self {
        Self::Custody(value)
    }
}

impl From<AgentContractError> for SessionKeyError {
    fn from(value: AgentContractError) -> Self {
        Self::Agent(value)
    }
}

struct ManagedAgent {
    state: ManagedAgentState,
    current: SessionLease,
    retired: Vec<SessionLease>,
}

/// Human control-plane orchestrator for bounded operating authority. Private
/// primary keys remain inside [`Keystore`]; only a freshly generated session
/// seed crosses the versioned agent contract.
pub struct SessionKeyProvisioner<C: AgentSessionContract, E: SessionEntropySource> {
    contract: C,
    entropy: E,
    policy: SessionPolicy,
    rotation_registry: ModuleRegistry,
    agents: BTreeMap<(PrincipalId, Did), ManagedAgent>,
    rotations: BTreeMap<(PrincipalId, Did), RotationJourney>,
}

impl<C: AgentSessionContract, E: SessionEntropySource> SessionKeyProvisioner<C, E> {
    /// Binds the versioned agent contract, declared authority policy, and
    /// core-negotiated registry used exclusively through `layerx-intents`.
    #[must_use]
    pub fn new(
        contract: C,
        entropy: E,
        policy: SessionPolicy,
        rotation_registry: ModuleRegistry,
    ) -> Self {
        Self {
            contract,
            entropy,
            policy,
            rotation_registry,
            agents: BTreeMap::new(),
            rotations: BTreeMap::new(),
        }
    }

    /// Borrows the contract for read-only qualification and status surfaces.
    #[must_use]
    pub const fn contract(&self) -> &C {
        &self.contract
    }

    /// Borrows the contract for boundary driving and integration qualification.
    #[must_use]
    pub const fn contract_mut(&mut self) -> &mut C {
        &mut self.contract
    }

    /// Returns the current public session record.
    #[must_use]
    pub fn session(&self, principal: &PrincipalId, did: &Did) -> Option<&SessionLease> {
        self.agents
            .get(&(principal.clone(), did.clone()))
            .map(|agent| &agent.current)
    }

    /// Provisions a fresh, exact-scope protocol session key and joins it to a
    /// real daemon permission session. Primary key material is never read.
    ///
    /// # Errors
    ///
    /// Refuses duplicate active authority, empty scope, unverified identity,
    /// unsafe grant bounds, entropy failure, or mismatched agent evidence.
    pub fn provision(
        &mut self,
        principal: &PrincipalId,
        did: &Did,
        permitted_activity_types: Vec<ActivityType>,
    ) -> Result<SessionLease, SessionKeyError> {
        let key = (principal.clone(), did.clone());
        if let Some(existing) = self.agents.get(&key) {
            return Err(match existing.state {
                ManagedAgentState::Active => SessionKeyError::AlreadyProvisioned,
                ManagedAgentState::Paused => SessionKeyError::Paused,
                ManagedAgentState::Archived => SessionKeyError::Archived,
            });
        }
        let lease = self.provision_new(principal, did, permitted_activity_types)?;
        self.agents.insert(
            key,
            ManagedAgent {
                state: ManagedAgentState::Active,
                current: lease.clone(),
                retired: Vec::new(),
            },
        );
        Ok(lease)
    }

    /// Immediately suspends daemon permissions, then proves targeted protocol
    /// revocation. A protocol refusal leaves an honest suspended state that can
    /// be retried instead of restoring permission.
    ///
    /// # Errors
    ///
    /// Refuses unknown agents and propagates exact daemon or protocol failures.
    pub fn pause(
        &mut self,
        principal: &PrincipalId,
        did: &Did,
        requested_at: u64,
    ) -> Result<RevocationOutcome, SessionKeyError> {
        self.stop_authority(principal, did, requested_at, RevocationReason::Paused)
    }

    /// Permanently suspends daemon permissions and retires protocol authority.
    ///
    /// # Errors
    ///
    /// Refuses unknown agents and propagates exact daemon or protocol failures.
    pub fn archive(
        &mut self,
        principal: &PrincipalId,
        did: &Did,
        requested_at: u64,
    ) -> Result<RevocationOutcome, SessionKeyError> {
        self.stop_authority(principal, did, requested_at, RevocationReason::Archived)
    }

    /// Restores a paused agent only through a fresh protocol grant and fresh
    /// daemon permission session. Archived agents can never resume.
    ///
    /// # Errors
    ///
    /// Refuses active, archived, unknown, or unprovable agent authority.
    pub fn resume(
        &mut self,
        principal: &PrincipalId,
        did: &Did,
    ) -> Result<SessionLease, SessionKeyError> {
        let key = (principal.clone(), did.clone());
        let (state, activities) = self
            .agents
            .get(&key)
            .map(|agent| (agent.state, agent.current.permitted_activity_types.clone()))
            .ok_or(SessionKeyError::NotProvisioned)?;
        match state {
            ManagedAgentState::Active => return Err(SessionKeyError::AlreadyProvisioned),
            ManagedAgentState::Archived => return Err(SessionKeyError::Archived),
            ManagedAgentState::Paused => {}
        }
        let replacement = self.provision_new(principal, did, activities)?;
        let agent = self
            .agents
            .get_mut(&key)
            .ok_or(SessionKeyError::NotProvisioned)?;
        let previous = std::mem::replace(&mut agent.current, replacement.clone());
        agent.retired.push(previous);
        agent.state = ManagedAgentState::Active;
        Ok(replacement)
    }

    /// Automatically renews every active session inside the declared lead
    /// window, installing its replacement before retiring the old grant.
    pub fn renew_expiring(&mut self, now: u64) -> Vec<Result<RenewalOutcome, SessionKeyError>> {
        let due = self
            .agents
            .iter()
            .filter(|(_, agent)| {
                agent.state == ManagedAgentState::Active
                    && agent.current.expires_at.saturating_sub(now)
                        <= self.policy.renewal_lead_seconds
            })
            .map(|((principal, did), _)| (principal.clone(), did.clone()))
            .collect::<Vec<_>>();
        due.into_iter()
            .map(|(principal, did)| {
                self.renew_one(&principal, &did, now, RevocationReason::Renewed)
            })
            .collect()
    }

    /// Announces human or agent primary-key rotation using a KMS-held pending
    /// key and the sole Human payload authority, `layerx-intents`.
    ///
    /// # Errors
    ///
    /// Refuses foreign key classes, unverified identity, malformed timing,
    /// competing rotation, compilation failure, or mismatched protocol evidence.
    #[allow(clippy::too_many_arguments)]
    pub fn announce_rotation(
        &mut self,
        keystore: &Keystore,
        principal: &PrincipalId,
        subject: RotationSubject,
        did: &Did,
        current_key_id: &KeyId,
        pending_key_id: &KeyId,
        challenge_delay_seconds: u64,
        effective_sequence: u64,
    ) -> Result<RotationJourney, SessionKeyError> {
        let key = (principal.clone(), did.clone());
        if self.rotations.contains_key(&key) {
            return Err(SessionKeyError::RotationAlreadyOpen);
        }
        let current = keystore.describe(principal, current_key_id)?;
        let pending = keystore.describe(principal, pending_key_id)?;
        if current.class != subject.key_class() || pending.class != subject.key_class() {
            return Err(SessionKeyError::KeyClassMismatch);
        }
        let identity = self.contract.identity_snapshot(principal, did)?;
        verify_identity(&identity)?;
        if identity.primary_public_key != current.public_key || effective_sequence == 0 {
            return Err(SessionKeyError::InvalidIdentity);
        }
        let effective_at = identity
            .protocol_time
            .checked_add(challenge_delay_seconds)
            .ok_or(SessionKeyError::TimeOverflow)?;
        let lapse_at = effective_at
            .checked_add(challenge_delay_seconds)
            .ok_or(SessionKeyError::TimeOverflow)?;
        let challenge_delay = PlainTime::new(challenge_delay_seconds)?;
        let window = TimestampBound::new(effective_at, lapse_at)
            .map_err(|_| SessionKeyError::InvalidRotationWindow)?;
        let rotation = KeyRotation::new(
            did.clone(),
            PublicKey::new(pending.public_key),
            window,
            Sequence::from_u64(effective_sequence),
        )?;
        let intent = Intent::v1(IntentKind::KeyRotation(rotation));
        let compiled = compile(&intent, &self.rotation_registry)?;
        let disclosure = DisclosureCheck::verify(&intent, &compiled)?;
        let expected_payload_hash = compiled.payload_hash();
        let evidence = self.contract.announce_rotation(RotationSubmission {
            principal: principal.clone(),
            subject,
            did: did.clone(),
            current_public_key: current.public_key,
            pending_public_key: pending.public_key,
            effective_at,
            lapse_at,
            effective_sequence,
            intent,
            compiled,
            disclosure,
        })?;
        if evidence.payload_hash != expected_payload_hash
            || evidence.pending_public_key != pending.public_key
            || evidence.effective_at != effective_at
            || evidence.lapse_at != lapse_at
            || evidence.effective_sequence != effective_sequence
            || evidence.observed_at < identity.protocol_time
            || evidence.verification_level < VerificationLevel::BATCH_INCLUDED
            || evidence.receipt_digest != evidence.expected_digest()
        {
            return Err(SessionKeyError::InvalidEvidence);
        }
        let journey = RotationJourney {
            principal: principal.clone(),
            subject,
            did: did.clone(),
            current_key_id: current_key_id.clone(),
            pending_key_id: pending_key_id.clone(),
            current_public_key: current.public_key,
            pending_public_key: pending.public_key,
            announced_at: identity.protocol_time,
            effective_at,
            lapse_at,
            effective_sequence,
            challenge_delay,
            announcement_receipt_digest: evidence.receipt_digest,
            state: RotationJourneyState::ChallengeOpen,
        };
        self.rotations.insert(key, journey.clone());
        Ok(journey)
    }

    /// Reconciles a tracked rotation from state-proven protocol facts. Agent
    /// rotation automatically restores operating continuity through a new
    /// scoped session after the old daemon sessions are invalidated.
    ///
    /// # Errors
    ///
    /// Refuses missing journeys, unverified or inconsistent observations, and
    /// any failed agent-session restoration.
    pub fn reconcile_rotation(
        &mut self,
        principal: &PrincipalId,
        did: &Did,
    ) -> Result<(RotationJourney, Option<SessionLease>), SessionKeyError> {
        let key = (principal.clone(), did.clone());
        let tracked = self
            .rotations
            .get(&key)
            .cloned()
            .ok_or(SessionKeyError::RotationNotFound)?;
        let observed = self.contract.rotation_observation(principal, did)?;
        if observed.verification_level < VerificationLevel::STATE_PROVEN
            || observed.effective_at != tracked.effective_at
            || observed.lapse_at != tracked.lapse_at
            || observed.effective_sequence != tracked.effective_sequence
            || observed.observed_at < tracked.announced_at
        {
            return Err(SessionKeyError::InvalidEvidence);
        }
        let mut replacement = None;
        let state = if observed.primary_public_key == tracked.current_public_key
            && observed.pending_public_key == Some(tracked.pending_public_key)
        {
            if observed.observed_at < tracked.effective_at {
                RotationJourneyState::ChallengeOpen
            } else if observed.observed_at <= tracked.lapse_at {
                RotationJourneyState::ReadyToCommit
            } else {
                RotationJourneyState::Lapsed
            }
        } else if observed.primary_public_key == tracked.pending_public_key
            && observed.pending_public_key.is_none()
            && observed.superseded_public_key == Some(tracked.current_public_key)
        {
            if tracked.subject == RotationSubject::Agent
                && !matches!(tracked.state, RotationJourneyState::Effective { .. })
            {
                if let Some(agent) = self.agents.get(&key) {
                    if agent.state == ManagedAgentState::Active {
                        let activities = agent.current.permitted_activity_types.clone();
                        let lease = self.provision_new(principal, did, activities)?;
                        let agent = self
                            .agents
                            .get_mut(&key)
                            .ok_or(SessionKeyError::NotProvisioned)?;
                        let previous = std::mem::replace(&mut agent.current, lease.clone());
                        agent.retired.push(previous);
                        replacement = Some(lease);
                    }
                }
            }
            RotationJourneyState::Effective {
                superseded_key_usable_before_sequence: tracked.effective_sequence,
                observed_sequence: observed.observed_sequence,
            }
        } else {
            return Err(SessionKeyError::InvalidEvidence);
        };
        let journey = self
            .rotations
            .get_mut(&key)
            .ok_or(SessionKeyError::RotationNotFound)?;
        journey.state = state;
        Ok((journey.clone(), replacement))
    }

    fn provision_new(
        &mut self,
        principal: &PrincipalId,
        did: &Did,
        permitted_activity_types: Vec<ActivityType>,
    ) -> Result<SessionLease, SessionKeyError> {
        if permitted_activity_types.is_empty() {
            return Err(SessionKeyError::InvalidActivityScope);
        }
        let identity = self.contract.identity_snapshot(principal, did)?;
        verify_identity(&identity)?;
        let expires_at = identity
            .protocol_time
            .checked_add(self.policy.lifetime_seconds)
            .ok_or(SessionKeyError::TimeOverflow)?;
        let entropy = self.entropy.next_session_entropy()?;
        let session_public_key = LocalSigner::new(*entropy.0).public_key();
        let issued = issue_session_key(&SessionKeyRequest {
            fee_budget: None,
            purpose: layerx_crypto::session::SessionPurpose::Activity,
            grantor: identity.protocol_identity,
            session_public_key,
            not_before: identity.protocol_time,
            expires_at: Some(expires_at),
            permitted_activity_types,
            revocation_sequence: Some(identity.revocation_sequence),
        })?;
        let request = AgentSessionProvision {
            principal: principal.clone(),
            agent_did: did.clone(),
            issued: issued.clone(),
            daemon_scopes: self.policy.daemon_scopes.clone(),
            daemon_policy_version: self.policy.policy_version.clone(),
            secret: entropy.into_secret(),
        };
        let evidence = self.contract.provision_session(request)?;
        if evidence.grant_id != issued.grant_id
            || evidence.session_public_key != issued.session_public_key
            || evidence.daemon_session_id == [0; 32]
            || evidence.protocol_sequence < identity.core_sequence
            || evidence.observed_at < identity.protocol_time
            || evidence.verification_level < VerificationLevel::BATCH_INCLUDED
            || evidence.receipt_digest != evidence.expected_digest()
        {
            return Err(SessionKeyError::InvalidEvidence);
        }
        Ok(SessionLease {
            principal: principal.clone(),
            agent_did: did.clone(),
            session_public_key,
            grant_id: issued.grant_id,
            daemon_session_id: evidence.daemon_session_id,
            permitted_activity_types: issued.permitted_activity_types,
            issued_at: identity.protocol_time,
            expires_at: issued.expires_at,
            revocation_sequence: issued.revocation_sequence,
            provision_receipt_digest: evidence.receipt_digest,
            state: SessionLeaseState::Active,
        })
    }

    fn stop_authority(
        &mut self,
        principal: &PrincipalId,
        did: &Did,
        requested_at: u64,
        reason: RevocationReason,
    ) -> Result<RevocationOutcome, SessionKeyError> {
        let key = (principal.clone(), did.clone());
        let lease = self
            .agents
            .get(&key)
            .map(|agent| agent.current.clone())
            .ok_or(SessionKeyError::NotProvisioned)?;
        if let SessionLeaseState::Revoked {
            reason: completed_reason,
            suspended_at,
            revoked_at,
            suspension_receipt_digest,
            revocation_receipt_digest,
        } = lease.state
        {
            if completed_reason != reason || requested_at > suspended_at {
                return Err(SessionKeyError::NotProvisioned);
            }
            let latency_seconds = revoked_at.saturating_sub(suspended_at);
            return Ok(RevocationOutcome {
                reason,
                requested_at,
                suspended_at,
                revoked_at,
                latency_seconds,
                within_declared_target: latency_seconds
                    <= self.policy.maximum_revocation_latency_seconds,
                suspension_receipt_digest,
                revocation_receipt_digest,
            });
        }
        let target = target(&lease);
        let suspension = match lease.state {
            SessionLeaseState::Active => {
                let evidence = self
                    .contract
                    .suspend_permissions(&target, reason, requested_at)?;
                validate_suspension(&evidence, &target, reason, requested_at)?;
                if let Some(agent) = self.agents.get_mut(&key) {
                    agent.current.state = SessionLeaseState::SuspendedAwaitingProtocolRevocation {
                        reason,
                        suspended_at: evidence.observed_at,
                        suspension_receipt_digest: evidence.receipt_digest,
                    };
                }
                evidence
            }
            SessionLeaseState::SuspendedAwaitingProtocolRevocation {
                reason: pending_reason,
                suspended_at,
                suspension_receipt_digest,
            } if pending_reason == reason => SuspensionEvidence {
                grant_id: target.grant_id,
                daemon_session_id: target.daemon_session_id,
                reason,
                observed_at: suspended_at,
                receipt_digest: suspension_receipt_digest,
            },
            SessionLeaseState::SuspendedAwaitingProtocolRevocation { .. }
            | SessionLeaseState::Revoked { .. } => return Err(SessionKeyError::NotProvisioned),
        };
        let revocation = self
            .contract
            .revoke_protocol_session(&target, reason, requested_at)?;
        validate_revocation(&revocation, &target, reason, suspension.observed_at)?;
        let outcome =
            revocation_outcome(&self.policy, reason, requested_at, &suspension, &revocation);
        let agent = self
            .agents
            .get_mut(&key)
            .ok_or(SessionKeyError::NotProvisioned)?;
        agent.current.state = SessionLeaseState::Revoked {
            reason,
            suspended_at: suspension.observed_at,
            revoked_at: revocation.observed_at,
            suspension_receipt_digest: suspension.receipt_digest,
            revocation_receipt_digest: revocation.receipt_digest,
        };
        agent.state = match reason {
            RevocationReason::Paused => ManagedAgentState::Paused,
            RevocationReason::Archived => ManagedAgentState::Archived,
            RevocationReason::Renewed | RevocationReason::PrimaryKeyRotated => agent.state,
        };
        Ok(outcome)
    }

    fn renew_one(
        &mut self,
        principal: &PrincipalId,
        did: &Did,
        requested_at: u64,
        reason: RevocationReason,
    ) -> Result<RenewalOutcome, SessionKeyError> {
        let key = (principal.clone(), did.clone());
        let previous = self
            .agents
            .get(&key)
            .filter(|agent| agent.state == ManagedAgentState::Active)
            .map(|agent| agent.current.clone())
            .ok_or(SessionKeyError::NotProvisioned)?;
        let replacement =
            self.provision_new(principal, did, previous.permitted_activity_types.clone())?;
        let target = target(&previous);
        let suspension = self
            .contract
            .suspend_permissions(&target, reason, requested_at)?;
        validate_suspension(&suspension, &target, reason, requested_at)?;
        let revocation = self
            .contract
            .revoke_protocol_session(&target, reason, requested_at)?;
        validate_revocation(&revocation, &target, reason, suspension.observed_at)?;
        let previous_revocation =
            revocation_outcome(&self.policy, reason, requested_at, &suspension, &revocation);
        let mut retired = previous.clone();
        retired.state = SessionLeaseState::Revoked {
            reason,
            suspended_at: suspension.observed_at,
            revoked_at: revocation.observed_at,
            suspension_receipt_digest: suspension.receipt_digest,
            revocation_receipt_digest: revocation.receipt_digest,
        };
        let agent = self
            .agents
            .get_mut(&key)
            .ok_or(SessionKeyError::NotProvisioned)?;
        agent.retired.push(retired);
        agent.current = replacement.clone();
        Ok(RenewalOutcome {
            previous_grant_id: previous.grant_id,
            replacement,
            previous_revocation,
        })
    }
}

fn target(lease: &SessionLease) -> SessionTarget {
    SessionTarget {
        principal: lease.principal.clone(),
        agent_did: lease.agent_did.clone(),
        grant_id: lease.grant_id,
        daemon_session_id: lease.daemon_session_id,
    }
}

fn verify_identity(identity: &ProtocolIdentitySnapshot) -> Result<(), SessionKeyError> {
    if identity.protocol_identity == [0; 32]
        || identity.primary_public_key == [0; 32]
        || identity.revocation_sequence == 0
        || identity.protocol_time == 0
        || identity.verification_level < VerificationLevel::STATE_PROVEN
    {
        Err(SessionKeyError::InvalidIdentity)
    } else {
        Ok(())
    }
}

fn validate_suspension(
    evidence: &SuspensionEvidence,
    target: &SessionTarget,
    reason: RevocationReason,
    requested_at: u64,
) -> Result<(), SessionKeyError> {
    if evidence.grant_id != target.grant_id
        || evidence.daemon_session_id != target.daemon_session_id
        || evidence.reason != reason
        || evidence.observed_at < requested_at
        || evidence.receipt_digest != evidence.expected_digest()
    {
        Err(SessionKeyError::InvalidEvidence)
    } else {
        Ok(())
    }
}

fn validate_revocation(
    evidence: &RevocationEvidence,
    target: &SessionTarget,
    reason: RevocationReason,
    suspended_at: u64,
) -> Result<(), SessionKeyError> {
    if evidence.grant_id != target.grant_id
        || evidence.reason != reason
        || evidence.observed_at < suspended_at
        || evidence.verification_level < VerificationLevel::BATCH_INCLUDED
        || evidence.receipt_digest != evidence.expected_digest()
    {
        Err(SessionKeyError::InvalidEvidence)
    } else {
        Ok(())
    }
}

fn revocation_outcome(
    policy: &SessionPolicy,
    reason: RevocationReason,
    requested_at: u64,
    suspension: &SuspensionEvidence,
    revocation: &RevocationEvidence,
) -> RevocationOutcome {
    let latency_seconds = revocation.observed_at.saturating_sub(requested_at);
    RevocationOutcome {
        reason,
        requested_at,
        suspended_at: suspension.observed_at,
        revoked_at: revocation.observed_at,
        latency_seconds,
        within_declared_target: latency_seconds <= policy.maximum_revocation_latency_seconds,
        suspension_receipt_digest: suspension.receipt_digest,
        revocation_receipt_digest: revocation.receipt_digest,
    }
}

fn digest_fields(domain: &[u8], fields: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    for field in fields {
        hasher.update(field.len().to_be_bytes());
        hasher.update(field);
    }
    hasher.finalize().into()
}

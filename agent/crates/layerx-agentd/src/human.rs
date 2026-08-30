//! Authenticated bounded peer for the Human-plane agent runtime.

use layerx_client::lni::transport::{FrameTransport, TransportError};
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;
use zeroize::Zeroizing;

const MAGIC: &[u8; 8] = b"LXHAGT01";
const PREPARE: u8 = 1;
const SUBMIT: u8 = 2;
const TRACK: u8 = 3;
const RECEIPT_LOOKUP: u8 = 4;
const REGISTRY: u8 = 5;
const APPROVAL_LIST: u8 = 9;
const APPROVAL_GET: u8 = 10;
const APPROVAL_APPROVE: u8 = 11;
const APPROVAL_REJECT: u8 = 12;
// Reserved lifecycle range. These values are part of the Human peer wire
// contract and must not be reused by ordinary activity operations.
const IDENTITY_RESOLVE: u8 = 20;
const LEASE_MAP: u8 = 21;
const OWNER_VALIDATE: u8 = 22;
const OWNER_INSTALL: u8 = 23;
const AGENT_LIST: u8 = 24;
const AGENT_GET: u8 = 25;
const AGENT_CONTROL: u8 = 26;
const AGENT_LIMIT: u8 = 27;
const AGENT_JOURNEY: u8 = 28;
const AGENT_ARCHIVE: u8 = 29;
const CAPABILITY_INSTALL: u8 = 30;
const AGENT_CONTEXT: u8 = 31;
const AGENT_BUDGET_STATE: u8 = 32;
const AGENT_KEY_POLICY: u8 = 33;
const AGENT_SESSION_SNAPSHOT: u8 = 34;
const AGENT_SESSION_SUSPEND: u8 = 35;
const AGENT_SESSION_BIND: u8 = 36;
const AGENT_LIFECYCLE_PUBLISH: u8 = 37;
const ACCOUNT_SEQUENCE: u8 = 13;
const BALANCE: u8 = 6;
const HEAD: u8 = 7;
const EVIDENCE: u8 = 8;
const MAX_TEXT: usize = 255;
const MAX_BYTES: usize = 1_048_576;

/// Identity established by the Unix listener before request bytes are read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HumanPeer {
    pub uid: u32,
    pub principal: String,
    pub tenant: String,
}

impl HumanPeer {
    fn validate(&self) -> Result<(), HumanProtocolError> {
        if self.principal.is_empty()
            || self.tenant.is_empty()
            || self.principal.len() > MAX_TEXT
            || self.tenant.len() > MAX_TEXT
        {
            return Err(HumanProtocolError::Unauthenticated);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MutationEnvelope<T> {
    pub request_id: u64,
    pub key: [u8; 32],
    pub body_digest: [u8; 32],
    pub operation: T,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HumanPrepare {
    pub activity_type: u32,
    pub actor: String,
    pub authority: String,
    pub account_sequence: u64,
    pub not_before: u64,
    pub not_after: u64,
    pub idempotency_key: String,
    pub fee_limit: u128,
    pub payload: Vec<u8>,
    pub payload_hash: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HumanSubmit {
    pub preparation_ref: String,
    pub signature: Vec<u8>,
    pub signer_public_key: [u8; 32],
    pub approval_release_ref: Option<[u8; 32]>,
}

pub struct HumanAgentSessionSecret(Zeroizing<[u8; 32]>);

impl HumanAgentSessionSecret {
    fn from_wire(mut seed: [u8; 32]) -> Result<Self, HumanProtocolError> {
        if seed == [0; 32] {
            return Err(HumanProtocolError::Malformed);
        }
        let secret = Self(Zeroizing::new(seed));
        seed.fill(0);
        Ok(secret)
    }

    pub(crate) fn as_seed(&self) -> &[u8; 32] {
        &self.0
    }
}

impl std::fmt::Debug for HumanAgentSessionSecret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("HumanAgentSessionSecret([redacted])")
    }
}

#[derive(Debug)]
pub struct HumanOwnerInstall {
    pub agent: String,
    pub authority_kind: u8,
    pub authority_id: [u8; 32],
    pub session_id: [u8; 32],
    pub token_id: [u8; 32],
    pub session_public_key: [u8; 32],
    pub registration_payload: Vec<u8>,
    pub grantor: [u8; 32],
    pub grant_not_before: u64,
    pub grant_expires_at: u64,
    pub grant_revocation_sequence: u64,
    pub session_secret: Option<HumanAgentSessionSecret>,
    pub permitted_activity_types: Vec<u16>,
    pub scopes: Vec<String>,
    pub lease_not_before_unix_ms: u64,
    pub lease_not_after_unix_ms: u64,
    pub opening_client: String,
    pub policy_version: String,
    pub lifecycle: Option<HumanAgentLifecycleSeed>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HumanAgentLifecycleSeed {
    pub agent_id: String,
    pub name: String,
    pub purpose: String,
    pub currency: String,
    pub monthly_limit: u128,
    pub period_start: u64,
    pub period_end: u64,
    pub created_at: u64,
    pub updated_at: u64,
    pub verified_evidence: Vec<[u8; 32]>,
    pub actor: String,
    pub primary_authority: String,
    pub custody_key: String,
    pub custody_public_key: [u8; 32],
    pub owner_account: String,
    pub budget_account: String,
    pub budget_asset: [u8; 32],
    pub purpose_hash: [u8; 32],
    pub recovery_root: [u8; 32],
    pub recovery_threshold: u16,
    pub capability_id: [u8; 32],
    pub activity_types: Vec<u32>,
    pub counterparties: Vec<[u8; 32]>,
    pub assets: Vec<[u8; 32]>,
    pub amount_ceiling: u128,
    pub rate_maximum_uses: u64,
    pub rate_window_sequences: u64,
    pub purposes: Vec<String>,
    pub capability_expiry_sequence: u64,
    pub session_scopes: Vec<String>,
    pub session_expiry_unix_seconds: u64,
    pub protocol_grant_id: [u8; 32],
    pub budget_period_seconds: u64,
    pub budget_expiry_seconds: u64,
    pub initial_funding: u128,
    pub network_id: u32,
    pub creation_receipt_roots: Vec<[u8; 32]>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HumanCapabilityInstall {
    pub action_key: [u8; 32],
    pub agent: String,
    pub authority_id: [u8; 32],
    pub capability_id: [u8; 32],
    pub activity_types: Vec<u16>,
    pub counterparties: Vec<[u8; 32]>,
    pub assets: Vec<[u8; 32]>,
    pub amount_ceiling: u128,
    pub rate_maximum_uses: u64,
    pub rate_window_sequences: u64,
    pub purposes: Vec<String>,
    pub expiry_sequence: u64,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HumanFinalizationEvidence {
    pub action_key: [u8; 32],
    pub activity_id: [u8; 32],
    pub receipt_digest: [u8; 32],
    pub observed_sequence: u64,
    pub verification: u8,
    pub finalized_at: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HumanAgentJourneyKind {
    Reclaim {
        amount: u128,
        currency: String,
    },
    Rotate {
        challenge_delay_seconds: u64,
        ready_at: u64,
    },
    Recover {
        challenge_delay_seconds: u64,
        ready_at: u64,
    },
}

#[derive(Debug)]
pub enum HumanRequest {
    Prepare(MutationEnvelope<HumanPrepare>),
    Submit(MutationEnvelope<HumanSubmit>),
    Track {
        submission_ref: String,
    },
    ReceiptLookup {
        idempotency_key: [u8; 32],
        expected_activity_id: [u8; 32],
    },
    Registry,
    ApprovalList {
        current_sequence: u64,
        cursor: Option<[u8; 32]>,
        limit: u8,
    },
    ApprovalGet {
        approval_id: [u8; 32],
        current_sequence: u64,
    },
    ApprovalApprove {
        approval_id: [u8; 32],
        held_digest: [u8; 32],
        idempotency_key: String,
        current_sequence: u64,
    },
    ApprovalReject {
        approval_id: [u8; 32],
        held_digest: [u8; 32],
        idempotency_key: String,
        current_sequence: u64,
    },
    Balance,
    Head,
    Evidence {
        idempotency_key: [u8; 32],
        expected_activity_id: [u8; 32],
    },
    IdentityResolve {
        agent: String,
    },
    LeaseMap {
        not_before_unix_ms: u64,
        not_after_unix_ms: u64,
    },
    OwnerValidate(HumanOwnerInstall),
    OwnerInstall(MutationEnvelope<HumanOwnerInstall>),
    AccountSequence {
        actor: String,
        authority: String,
    },
    AgentList {
        cursor: Option<[u8; 32]>,
        limit: u8,
    },
    AgentGet {
        agent_id: String,
    },
    AgentControl {
        agent_id: String,
        resume: bool,
        session_observation: [u8; 32],
        evidence: HumanFinalizationEvidence,
    },
    AgentLimit {
        agent_id: String,
        monthly_limit: u128,
        currency: String,
        replacement_budget_id: [u8; 32],
        evidence: HumanFinalizationEvidence,
    },
    AgentJourney {
        agent_id: String,
        kind: HumanAgentJourneyKind,
        pre_observation: [u8; 32],
        post_observation: [u8; 32],
        evidence: HumanFinalizationEvidence,
    },
    AgentArchive {
        agent_id: String,
        confirm_name: String,
        pre_observation: [u8; 32],
        post_observation: [u8; 32],
        session_observation: [u8; 32],
        evidence: HumanFinalizationEvidence,
    },
    CapabilityInstall(HumanCapabilityInstall),
    AgentContext {
        agent_id: String,
    },
    AgentLifecyclePublish(MutationEnvelope<HumanAgentLifecycleSeed>),
    AgentBudgetState {
        active_budget_id: [u8; 32],
    },
    AgentKeyPolicy {
        agent_did: String,
        recovery: bool,
    },
    AgentSessionSnapshot {
        agent_id: String,
    },
    AgentSessionSuspend {
        agent_id: String,
        action_key: [u8; 32],
    },
    AgentSessionBind {
        agent_id: String,
        session_id: [u8; 32],
        token_id: [u8; 32],
        action_key: [u8; 32],
    },
}

/// Response bytes are the canonical typed payload following the shared magic
/// and success byte. Implementations build them from the daemon's existing
/// prepare, outbox, approval and receipt response objects.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HumanResponse(Vec<u8>);

impl HumanResponse {
    pub fn new(bytes: Vec<u8>) -> Result<Self, HumanProtocolError> {
        if bytes.is_empty() || bytes.len() > MAX_BYTES {
            return Err(HumanProtocolError::Malformed);
        }
        Ok(Self(bytes))
    }

    pub fn bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Narrow adapter over the existing daemon operation owners. It deliberately
/// has no sign method: Human custody supplies the public signature to submit.
pub trait HumanOperations {
    /// Returns the exact core-negotiated module registry encoded as count,
    /// module id, activity count and packed activity ids.
    fn registry(&self, peer: &HumanPeer) -> Result<HumanResponse, HumanOperationError>;

    fn prepare(
        &mut self,
        peer: &HumanPeer,
        request: MutationEnvelope<HumanPrepare>,
    ) -> Result<HumanResponse, HumanOperationError>;

    fn submit_external(
        &mut self,
        peer: &HumanPeer,
        request: MutationEnvelope<HumanSubmit>,
    ) -> Result<HumanResponse, HumanOperationError>;

    fn track(
        &mut self,
        peer: &HumanPeer,
        submission_ref: &str,
    ) -> Result<HumanResponse, HumanOperationError>;

    fn receipt_by_idempotency_key(
        &mut self,
        peer: &HumanPeer,
        idempotency_key: [u8; 32],
        expected_activity_id: [u8; 32],
    ) -> Result<HumanResponse, HumanOperationError>;

    fn approval_list(
        &mut self,
        peer: &HumanPeer,
        current_sequence: u64,
        cursor: Option<[u8; 32]>,
        limit: u8,
    ) -> Result<HumanResponse, HumanOperationError>;
    fn approval_get(
        &mut self,
        peer: &HumanPeer,
        approval_id: [u8; 32],
        current_sequence: u64,
    ) -> Result<HumanResponse, HumanOperationError>;
    fn approval_approve(
        &mut self,
        peer: &HumanPeer,
        approval_id: [u8; 32],
        held_digest: [u8; 32],
        idempotency_key: &str,
        current_sequence: u64,
    ) -> Result<HumanResponse, HumanOperationError>;
    fn approval_reject(
        &mut self,
        peer: &HumanPeer,
        approval_id: [u8; 32],
        held_digest: [u8; 32],
        idempotency_key: &str,
        current_sequence: u64,
    ) -> Result<HumanResponse, HumanOperationError>;

    fn balance(&mut self, peer: &HumanPeer) -> Result<HumanResponse, HumanOperationError>;
    fn head(&self, peer: &HumanPeer) -> Result<HumanResponse, HumanOperationError>;
    fn evidence(
        &mut self,
        peer: &HumanPeer,
        idempotency_key: [u8; 32],
        expected_activity_id: [u8; 32],
    ) -> Result<HumanResponse, HumanOperationError>;
    fn identity_resolve(
        &mut self,
        peer: &HumanPeer,
        agent: &str,
    ) -> Result<HumanResponse, HumanOperationError>;
    fn lease_map(
        &mut self,
        peer: &HumanPeer,
        not_before_unix_ms: u64,
        not_after_unix_ms: u64,
    ) -> Result<HumanResponse, HumanOperationError>;
    fn owner_validate(
        &mut self,
        peer: &HumanPeer,
        request: HumanOwnerInstall,
    ) -> Result<HumanResponse, HumanOperationError>;
    fn owner_install(
        &mut self,
        peer: &HumanPeer,
        request: MutationEnvelope<HumanOwnerInstall>,
    ) -> Result<HumanResponse, HumanOperationError>;
    fn account_sequence(
        &mut self,
        peer: &HumanPeer,
        actor: &str,
        authority: &str,
    ) -> Result<HumanResponse, HumanOperationError>;
    fn agent_list(
        &mut self,
        peer: &HumanPeer,
        cursor: Option<[u8; 32]>,
        limit: u8,
    ) -> Result<HumanResponse, HumanOperationError>;
    fn agent_get(
        &mut self,
        peer: &HumanPeer,
        agent_id: &str,
    ) -> Result<HumanResponse, HumanOperationError>;
    fn agent_control(
        &mut self,
        peer: &HumanPeer,
        agent_id: &str,
        resume: bool,
        session_observation: [u8; 32],
        evidence: HumanFinalizationEvidence,
    ) -> Result<HumanResponse, HumanOperationError>;
    fn agent_limit(
        &mut self,
        peer: &HumanPeer,
        agent_id: &str,
        monthly_limit: u128,
        currency: &str,
        replacement_budget_id: [u8; 32],
        evidence: HumanFinalizationEvidence,
    ) -> Result<HumanResponse, HumanOperationError>;
    fn agent_journey(
        &mut self,
        peer: &HumanPeer,
        agent_id: &str,
        kind: HumanAgentJourneyKind,
        pre_observation: [u8; 32],
        post_observation: [u8; 32],
        evidence: HumanFinalizationEvidence,
    ) -> Result<HumanResponse, HumanOperationError>;
    fn agent_archive(
        &mut self,
        peer: &HumanPeer,
        agent_id: &str,
        confirm_name: &str,
        pre_observation: [u8; 32],
        post_observation: [u8; 32],
        session_observation: [u8; 32],
        evidence: HumanFinalizationEvidence,
    ) -> Result<HumanResponse, HumanOperationError>;
    fn capability_install(
        &mut self,
        peer: &HumanPeer,
        request: HumanCapabilityInstall,
    ) -> Result<HumanResponse, HumanOperationError>;
    fn agent_lifecycle_publish(
        &mut self,
        peer: &HumanPeer,
        request: MutationEnvelope<HumanAgentLifecycleSeed>,
    ) -> Result<HumanResponse, HumanOperationError>;
    fn agent_context(
        &mut self,
        peer: &HumanPeer,
        agent_id: &str,
    ) -> Result<HumanResponse, HumanOperationError>;
    fn agent_budget_state(
        &mut self,
        peer: &HumanPeer,
        active_budget_id: [u8; 32],
    ) -> Result<HumanResponse, HumanOperationError>;
    fn agent_key_policy(
        &mut self,
        peer: &HumanPeer,
        agent_did: &str,
        recovery: bool,
    ) -> Result<HumanResponse, HumanOperationError>;
    fn agent_session_snapshot(
        &mut self,
        peer: &HumanPeer,
        agent_id: &str,
    ) -> Result<HumanResponse, HumanOperationError>;
    fn agent_session_suspend(
        &mut self,
        peer: &HumanPeer,
        agent_id: &str,
        action_key: [u8; 32],
    ) -> Result<HumanResponse, HumanOperationError>;
    fn agent_session_bind(
        &mut self,
        peer: &HumanPeer,
        agent_id: &str,
        session_id: [u8; 32],
        token_id: [u8; 32],
        action_key: [u8; 32],
    ) -> Result<HumanResponse, HumanOperationError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HumanOperationError {
    Refused,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HumanProtocolError {
    Unauthenticated,
    Malformed,
    Transport(TransportError),
}

/// Mandatory listener configuration. Each accepted kernel uid has one
/// immutable principal and tenant; an unmapped uid is refused before framing.
pub struct HumanListenerConfig {
    pub endpoint: PathBuf,
    pub owner_uid: u32,
    pub owner_gid: u32,
    pub mode: u32,
    pub maximum_frame_bytes: usize,
    pub deadline: Duration,
    pub peers: BTreeMap<u32, (String, String)>,
}

pub struct HumanUnixServer<O> {
    listener: UnixListener,
    config: HumanListenerConfig,
    operations: O,
}

impl<O: HumanOperations> HumanUnixServer<O> {
    pub fn bind(config: HumanListenerConfig, operations: O) -> Result<Self, HumanProtocolError> {
        if !config.endpoint.is_absolute()
            || config.maximum_frame_bytes == 0
            || config.maximum_frame_bytes > MAX_BYTES
            || config.deadline.is_zero()
            || config.peers.is_empty()
            || config.mode & !0o770 != 0
        {
            return Err(HumanProtocolError::Unauthenticated);
        }
        let parent = config
            .endpoint
            .parent()
            .ok_or(HumanProtocolError::Unauthenticated)?;
        validate_path(parent, config.owner_uid, config.owner_gid, true)?;
        if config.endpoint.exists() {
            return Err(HumanProtocolError::Unauthenticated);
        }
        let listener = UnixListener::bind(&config.endpoint)
            .map_err(|error| HumanProtocolError::Transport(io_error(&error)))?;
        fs::set_permissions(&config.endpoint, fs::Permissions::from_mode(config.mode))
            .map_err(|error| HumanProtocolError::Transport(io_error(&error)))?;
        validate_path(&config.endpoint, config.owner_uid, config.owner_gid, false)?;
        if fs::symlink_metadata(&config.endpoint)
            .map_err(|_| HumanProtocolError::Unauthenticated)?
            .mode()
            & 0o777
            != config.mode
        {
            return Err(HumanProtocolError::Unauthenticated);
        }
        Ok(Self {
            listener,
            config,
            operations,
        })
    }

    pub fn serve(mut self) -> Result<(), HumanProtocolError> {
        loop {
            let (stream, _) = self
                .listener
                .accept()
                .map_err(|error| HumanProtocolError::Transport(io_error(&error)))?;
            let credentials = rustix::net::sockopt::socket_peercred(&stream)
                .map_err(|_| HumanProtocolError::Unauthenticated)?;
            let uid = credentials.uid.as_raw();
            let Some((principal, tenant)) = self.config.peers.get(&uid) else {
                continue;
            };
            let peer = HumanPeer {
                uid,
                principal: principal.clone(),
                tenant: tenant.clone(),
            };
            let mut transport = AcceptedTransport::new(
                stream,
                self.config.maximum_frame_bytes,
                self.config.deadline,
            )?;
            let _ = serve_one(&mut transport, &peer, &mut self.operations);
        }
    }
}

struct AcceptedTransport {
    stream: UnixStream,
    maximum: usize,
}
impl AcceptedTransport {
    fn new(
        stream: UnixStream,
        maximum: usize,
        deadline: Duration,
    ) -> Result<Self, HumanProtocolError> {
        stream
            .set_read_timeout(Some(deadline))
            .map_err(|error| HumanProtocolError::Transport(io_error(&error)))?;
        stream
            .set_write_timeout(Some(deadline))
            .map_err(|error| HumanProtocolError::Transport(io_error(&error)))?;
        Ok(Self { stream, maximum })
    }
}
impl FrameTransport for AcceptedTransport {
    fn send(&mut self, bytes: &[u8]) -> Result<(), TransportError> {
        layerx_client::lni::framing::write_frame(&mut self.stream, bytes, self.maximum)
    }
    fn receive(&mut self) -> Result<Vec<u8>, TransportError> {
        layerx_client::lni::framing::read_frame(&mut self.stream, self.maximum)
    }
}

fn validate_path(
    path: &Path,
    uid: u32,
    gid: u32,
    directory: bool,
) -> Result<(), HumanProtocolError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| HumanProtocolError::Unauthenticated)?;
    if metadata.file_type().is_symlink()
        || (directory && !metadata.is_dir())
        || (!directory && !metadata.file_type().is_socket())
        || metadata.uid() != uid
        || metadata.gid() != gid
        || metadata.mode() & 0o007 != 0
    {
        return Err(HumanProtocolError::Unauthenticated);
    }
    Ok(())
}
fn io_error(error: &std::io::Error) -> TransportError {
    match error.kind() {
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => TransportError::Deadline,
        kind => TransportError::ConnectionFailure(kind),
    }
}

/// Serves one already peer-authenticated request and emits exactly one bounded
/// response. Authentication is completed before this function by the listener.
pub fn serve_one<T: FrameTransport, O: HumanOperations>(
    transport: &mut T,
    peer: &HumanPeer,
    operations: &mut O,
) -> Result<(), HumanProtocolError> {
    peer.validate()?;
    let frame = transport.receive().map_err(HumanProtocolError::Transport)?;
    if frame.len() > MAX_BYTES {
        return Err(HumanProtocolError::Malformed);
    }
    let request = decode_request(frame)?;
    let result = match request {
        HumanRequest::Prepare(request) => operations.prepare(peer, request),
        HumanRequest::Submit(request) => operations.submit_external(peer, request),
        HumanRequest::Track { submission_ref } => operations.track(peer, &submission_ref),
        HumanRequest::ReceiptLookup {
            idempotency_key,
            expected_activity_id,
        } => operations.receipt_by_idempotency_key(peer, idempotency_key, expected_activity_id),
        HumanRequest::Registry => operations.registry(peer),
        HumanRequest::ApprovalList {
            current_sequence,
            cursor,
            limit,
        } => operations.approval_list(peer, current_sequence, cursor, limit),
        HumanRequest::ApprovalGet {
            approval_id,
            current_sequence,
        } => operations.approval_get(peer, approval_id, current_sequence),
        HumanRequest::ApprovalApprove {
            approval_id,
            held_digest,
            idempotency_key,
            current_sequence,
        } => operations.approval_approve(
            peer,
            approval_id,
            held_digest,
            &idempotency_key,
            current_sequence,
        ),
        HumanRequest::ApprovalReject {
            approval_id,
            held_digest,
            idempotency_key,
            current_sequence,
        } => operations.approval_reject(
            peer,
            approval_id,
            held_digest,
            &idempotency_key,
            current_sequence,
        ),
        HumanRequest::Balance => operations.balance(peer),
        HumanRequest::Head => operations.head(peer),
        HumanRequest::Evidence {
            idempotency_key,
            expected_activity_id,
        } => operations.evidence(peer, idempotency_key, expected_activity_id),
        HumanRequest::IdentityResolve { agent } => operations.identity_resolve(peer, &agent),
        HumanRequest::LeaseMap {
            not_before_unix_ms,
            not_after_unix_ms,
        } => operations.lease_map(peer, not_before_unix_ms, not_after_unix_ms),
        HumanRequest::OwnerValidate(request) => operations.owner_validate(peer, request),
        HumanRequest::OwnerInstall(request) => operations.owner_install(peer, request),
        HumanRequest::AccountSequence { actor, authority } => {
            operations.account_sequence(peer, &actor, &authority)
        }
        HumanRequest::AgentList { cursor, limit } => operations.agent_list(peer, cursor, limit),
        HumanRequest::AgentGet { agent_id } => operations.agent_get(peer, &agent_id),
        HumanRequest::AgentControl {
            agent_id,
            resume,
            session_observation,
            evidence,
        } => operations.agent_control(peer, &agent_id, resume, session_observation, evidence),
        HumanRequest::AgentLimit {
            agent_id,
            monthly_limit,
            currency,
            replacement_budget_id,
            evidence,
        } => operations.agent_limit(
            peer,
            &agent_id,
            monthly_limit,
            &currency,
            replacement_budget_id,
            evidence,
        ),
        HumanRequest::AgentJourney {
            agent_id,
            kind,
            pre_observation,
            post_observation,
            evidence,
        } => operations.agent_journey(
            peer,
            &agent_id,
            kind,
            pre_observation,
            post_observation,
            evidence,
        ),
        HumanRequest::AgentArchive {
            agent_id,
            confirm_name,
            pre_observation,
            post_observation,
            session_observation,
            evidence,
        } => operations.agent_archive(
            peer,
            &agent_id,
            &confirm_name,
            pre_observation,
            post_observation,
            session_observation,
            evidence,
        ),
        HumanRequest::CapabilityInstall(request) => operations.capability_install(peer, request),
        HumanRequest::AgentContext { agent_id } => operations.agent_context(peer, &agent_id),
        HumanRequest::AgentLifecyclePublish(request) => {
            operations.agent_lifecycle_publish(peer, request)
        }
        HumanRequest::AgentBudgetState { active_budget_id } => {
            operations.agent_budget_state(peer, active_budget_id)
        }
        HumanRequest::AgentKeyPolicy {
            agent_did,
            recovery,
        } => operations.agent_key_policy(peer, &agent_did, recovery),
        HumanRequest::AgentSessionSnapshot { agent_id } => {
            operations.agent_session_snapshot(peer, &agent_id)
        }
        HumanRequest::AgentSessionSuspend {
            agent_id,
            action_key,
        } => operations.agent_session_suspend(peer, &agent_id, action_key),
        HumanRequest::AgentSessionBind {
            agent_id,
            session_id,
            token_id,
            action_key,
        } => operations.agent_session_bind(peer, &agent_id, session_id, token_id, action_key),
    };
    let mut response = Vec::with_capacity(64);
    response.extend_from_slice(MAGIC);
    match result {
        Ok(payload) => {
            response.push(0);
            response.extend_from_slice(payload.bytes());
        }
        Err(HumanOperationError::Refused) => response.push(1),
        Err(HumanOperationError::Unavailable) => response.push(2),
    }
    transport
        .send(&response)
        .map_err(HumanProtocolError::Transport)
}

fn decode_request(bytes: Vec<u8>) -> Result<HumanRequest, HumanProtocolError> {
    let mut reader = Reader::new(bytes);
    if reader.fixed::<8>()? != *MAGIC {
        return Err(HumanProtocolError::Malformed);
    }
    let operation = reader.u8()?;
    let request = match operation {
        PREPARE => {
            let envelope = mutation_header(&mut reader)?;
            HumanRequest::Prepare(MutationEnvelope {
                request_id: envelope.0,
                key: envelope.1,
                body_digest: envelope.2,
                operation: HumanPrepare {
                    activity_type: reader.u32()?,
                    actor: reader.text()?,
                    authority: reader.text()?,
                    account_sequence: reader.u64()?,
                    not_before: reader.u64()?,
                    not_after: reader.u64()?,
                    idempotency_key: reader.text()?,
                    fee_limit: reader.u128()?,
                    payload: reader.bytes()?,
                    payload_hash: reader.fixed()?,
                },
            })
        }
        SUBMIT => {
            let envelope = mutation_header(&mut reader)?;
            HumanRequest::Submit(MutationEnvelope {
                request_id: envelope.0,
                key: envelope.1,
                body_digest: envelope.2,
                operation: HumanSubmit {
                    preparation_ref: reader.text()?,
                    signature: reader.bytes()?,
                    signer_public_key: reader.fixed()?,
                    approval_release_ref: match reader.u8()? {
                        0 => None,
                        1 => Some(reader.fixed()?),
                        _ => return Err(HumanProtocolError::Malformed),
                    },
                },
            })
        }
        TRACK => HumanRequest::Track {
            submission_ref: reader.text()?,
        },
        RECEIPT_LOOKUP => HumanRequest::ReceiptLookup {
            idempotency_key: reader.fixed()?,
            expected_activity_id: reader.fixed()?,
        },
        REGISTRY => HumanRequest::Registry,
        APPROVAL_LIST => HumanRequest::ApprovalList {
            current_sequence: reader.u64()?,
            cursor: match reader.u8()? {
                0 => None,
                1 => Some(reader.fixed()?),
                _ => return Err(HumanProtocolError::Malformed),
            },
            limit: reader.u8()?,
        },
        APPROVAL_GET => HumanRequest::ApprovalGet {
            approval_id: reader.fixed()?,
            current_sequence: reader.u64()?,
        },
        APPROVAL_APPROVE | APPROVAL_REJECT => {
            let approval_id = reader.fixed()?;
            let held_digest = reader.fixed()?;
            let idempotency_key = reader.text()?;
            let current_sequence = reader.u64()?;
            if operation == APPROVAL_APPROVE {
                HumanRequest::ApprovalApprove {
                    approval_id,
                    held_digest,
                    idempotency_key,
                    current_sequence,
                }
            } else {
                HumanRequest::ApprovalReject {
                    approval_id,
                    held_digest,
                    idempotency_key,
                    current_sequence,
                }
            }
        }
        BALANCE => HumanRequest::Balance,
        HEAD => HumanRequest::Head,
        EVIDENCE => HumanRequest::Evidence {
            idempotency_key: reader.fixed()?,
            expected_activity_id: reader.fixed()?,
        },
        IDENTITY_RESOLVE => HumanRequest::IdentityResolve {
            agent: reader.text()?,
        },
        LEASE_MAP => HumanRequest::LeaseMap {
            not_before_unix_ms: reader.u64()?,
            not_after_unix_ms: reader.u64()?,
        },
        OWNER_VALIDATE => HumanRequest::OwnerValidate(owner_install(&mut reader)?),
        OWNER_INSTALL => {
            let envelope = mutation_header(&mut reader)?;
            HumanRequest::OwnerInstall(MutationEnvelope {
                request_id: envelope.0,
                key: envelope.1,
                body_digest: envelope.2,
                operation: owner_install(&mut reader)?,
            })
        }
        ACCOUNT_SEQUENCE => HumanRequest::AccountSequence {
            actor: reader.text()?,
            authority: reader.text()?,
        },
        AGENT_LIST => HumanRequest::AgentList {
            cursor: match reader.u8()? {
                0 => None,
                1 => Some(reader.fixed()?),
                _ => return Err(HumanProtocolError::Malformed),
            },
            limit: reader.u8()?,
        },
        AGENT_GET => HumanRequest::AgentGet {
            agent_id: reader.text()?,
        },
        AGENT_CONTROL => HumanRequest::AgentControl {
            agent_id: reader.text()?,
            resume: match reader.u8()? {
                0 => false,
                1 => true,
                _ => return Err(HumanProtocolError::Malformed),
            },
            session_observation: reader.fixed()?,
            evidence: finalization(&mut reader)?,
        },
        AGENT_LIMIT => HumanRequest::AgentLimit {
            agent_id: reader.text()?,
            monthly_limit: reader.u128()?,
            currency: reader.text()?,
            replacement_budget_id: reader.fixed()?,
            evidence: finalization(&mut reader)?,
        },
        AGENT_JOURNEY => {
            let tag = reader.u8()?;
            let agent_id = reader.text()?;
            let amount = reader.u128()?;
            let currency = reader.text()?;
            let challenge_delay_seconds = reader.u64()?;
            let ready_at = reader.u64()?;
            let pre_observation = reader.fixed()?;
            let post_observation = reader.fixed()?;
            let evidence = finalization(&mut reader)?;
            let kind = match tag {
                0 if amount > 0
                    && !currency.is_empty()
                    && challenge_delay_seconds == 0
                    && ready_at == 0 =>
                {
                    HumanAgentJourneyKind::Reclaim { amount, currency }
                }
                1 if amount == 0
                    && currency.is_empty()
                    && challenge_delay_seconds > 0
                    && ready_at > evidence.finalized_at =>
                {
                    HumanAgentJourneyKind::Rotate {
                        challenge_delay_seconds,
                        ready_at,
                    }
                }
                2 if amount == 0
                    && currency.is_empty()
                    && challenge_delay_seconds > 0
                    && ready_at > evidence.finalized_at =>
                {
                    HumanAgentJourneyKind::Recover {
                        challenge_delay_seconds,
                        ready_at,
                    }
                }
                _ => return Err(HumanProtocolError::Malformed),
            };
            HumanRequest::AgentJourney {
                agent_id,
                kind,
                pre_observation,
                post_observation,
                evidence,
            }
        }
        AGENT_ARCHIVE => HumanRequest::AgentArchive {
            agent_id: reader.text()?,
            confirm_name: reader.text()?,
            pre_observation: reader.fixed()?,
            post_observation: reader.fixed()?,
            session_observation: reader.fixed()?,
            evidence: finalization(&mut reader)?,
        },
        CAPABILITY_INSTALL => {
            let action_key = reader.fixed()?;
            let agent = reader.text()?;
            let authority_id = reader.fixed()?;
            let capability_id = reader.fixed()?;
            let activity_count = usize::from(reader.u16()?);
            if action_key == [0; 32] || activity_count == 0 || activity_count > 256 {
                return Err(HumanProtocolError::Malformed);
            }
            let mut activity_types = Vec::with_capacity(activity_count);
            for _ in 0..activity_count {
                activity_types.push(reader.u16()?);
            }
            let counterparty_count = usize::from(reader.u16()?);
            if counterparty_count == 0 || counterparty_count > 256 {
                return Err(HumanProtocolError::Malformed);
            }
            let mut counterparties = Vec::with_capacity(counterparty_count);
            for _ in 0..counterparty_count {
                counterparties.push(reader.fixed()?);
            }
            let asset_count = usize::from(reader.u16()?);
            if asset_count == 0 || asset_count > 256 {
                return Err(HumanProtocolError::Malformed);
            }
            let mut assets = Vec::with_capacity(asset_count);
            for _ in 0..asset_count {
                assets.push(reader.fixed()?);
            }
            let amount_ceiling = reader.u128()?;
            let rate_maximum_uses = reader.u64()?;
            let rate_window_sequences = reader.u64()?;
            let purpose_count = usize::from(reader.u16()?);
            if purpose_count == 0 || purpose_count > 256 {
                return Err(HumanProtocolError::Malformed);
            }
            let mut purposes = Vec::with_capacity(purpose_count);
            for _ in 0..purpose_count {
                purposes.push(reader.text()?);
            }
            let expiry_sequence = reader.u64()?;
            if amount_ceiling == 0
                || rate_maximum_uses == 0
                || rate_window_sequences == 0
                || expiry_sequence == 0
            {
                return Err(HumanProtocolError::Malformed);
            }
            HumanRequest::CapabilityInstall(HumanCapabilityInstall {
                action_key,
                agent,
                authority_id,
                capability_id,
                activity_types,
                counterparties,
                assets,
                amount_ceiling,
                rate_maximum_uses,
                rate_window_sequences,
                purposes,
                expiry_sequence,
            })
        }
        AGENT_CONTEXT => HumanRequest::AgentContext {
            agent_id: reader.text()?,
        },
        AGENT_LIFECYCLE_PUBLISH => {
            let (request_id, key, body_digest) = mutation_header(&mut reader)?;
            HumanRequest::AgentLifecyclePublish(MutationEnvelope {
                request_id,
                key,
                body_digest,
                operation: lifecycle_seed(&mut reader)?,
            })
        }
        AGENT_BUDGET_STATE => HumanRequest::AgentBudgetState {
            active_budget_id: reader.fixed()?,
        },
        AGENT_KEY_POLICY => HumanRequest::AgentKeyPolicy {
            agent_did: reader.text()?,
            recovery: match reader.u8()? {
                0 => false,
                1 => true,
                _ => return Err(HumanProtocolError::Malformed),
            },
        },
        AGENT_SESSION_SNAPSHOT => HumanRequest::AgentSessionSnapshot {
            agent_id: reader.text()?,
        },
        AGENT_SESSION_SUSPEND => HumanRequest::AgentSessionSuspend {
            agent_id: reader.text()?,
            action_key: reader.fixed()?,
        },
        AGENT_SESSION_BIND => HumanRequest::AgentSessionBind {
            agent_id: reader.text()?,
            session_id: reader.fixed()?,
            token_id: reader.fixed()?,
            action_key: reader.fixed()?,
        },
        _ => return Err(HumanProtocolError::Malformed),
    };
    reader.finish()?;
    Ok(request)
}

fn owner_install(reader: &mut Reader) -> Result<HumanOwnerInstall, HumanProtocolError> {
    let agent = reader.text()?;
    let authority_kind = reader.u8()?;
    let authority_id = reader.fixed()?;
    let session_id = reader.fixed()?;
    let token_id = reader.fixed()?;
    let session_public_key = reader.fixed()?;
    let registration_payload = reader.bytes()?;
    if registration_payload.is_empty() || registration_payload.len() > 1024 {
        return Err(HumanProtocolError::Malformed);
    }
    let grantor = reader.fixed()?;
    let grant_not_before = reader.u64()?;
    let grant_expires_at = reader.u64()?;
    let grant_revocation_sequence = reader.u64()?;
    let session_seed = reader.fixed()?;
    let session_secret = if session_seed == [0; 32] {
        None
    } else {
        Some(HumanAgentSessionSecret::from_wire(session_seed)?)
    };
    let activity_count = usize::from(reader.u16()?);
    if activity_count == 0 || activity_count > 256 {
        return Err(HumanProtocolError::Malformed);
    }
    let mut permitted_activity_types = Vec::with_capacity(activity_count);
    for _ in 0..activity_count {
        permitted_activity_types.push(reader.u16()?);
    }
    let scope_count = usize::from(reader.u8()?);
    if scope_count == 0 || scope_count > 32 {
        return Err(HumanProtocolError::Malformed);
    }
    let mut scopes = Vec::with_capacity(scope_count);
    for _ in 0..scope_count {
        scopes.push(reader.text()?);
    }
    let lease_not_before_unix_ms = reader.u64()?;
    let lease_not_after_unix_ms = reader.u64()?;
    let opening_client = reader.text()?;
    let policy_version = reader.text()?;
    let lifecycle = match reader.u8()? {
        0 => None,
        1 => {
            let agent_id = reader.text()?;
            let name = reader.text()?;
            let purpose = reader.text()?;
            let currency = reader.text()?;
            let monthly_limit = reader.u128()?;
            let period_start = reader.u64()?;
            let period_end = reader.u64()?;
            let created_at = reader.u64()?;
            let updated_at = reader.u64()?;
            let verified_evidence = fixed_list(reader, 64)?;
            let actor = reader.text()?;
            let primary_authority = reader.text()?;
            let custody_key = reader.text()?;
            let custody_public_key = reader.fixed()?;
            let owner_account = reader.text()?;
            let budget_account = reader.text()?;
            let budget_asset = reader.fixed()?;
            let purpose_hash = reader.fixed()?;
            let recovery_root = reader.fixed()?;
            let recovery_threshold = reader.u16()?;
            let capability_id = reader.fixed()?;
            let activity_types = u32_list(reader, 256)?;
            let counterparties = fixed_list(reader, 256)?;
            let assets = fixed_list(reader, 256)?;
            let amount_ceiling = reader.u128()?;
            let rate_maximum_uses = reader.u64()?;
            let rate_window_sequences = reader.u64()?;
            let purposes = text_list(reader, 64)?;
            let capability_expiry_sequence = reader.u64()?;
            let session_scopes = text_list(reader, 64)?;
            let session_expiry_unix_seconds = reader.u64()?;
            let protocol_grant_id = reader.fixed()?;
            let budget_period_seconds = reader.u64()?;
            let budget_expiry_seconds = reader.u64()?;
            let initial_funding = reader.u128()?;
            let network_id = reader.u32()?;
            let creation_receipt_roots = fixed_list(reader, 64)?;
            let value = HumanAgentLifecycleSeed {
                agent_id,
                name,
                purpose,
                currency,
                monthly_limit,
                period_start,
                period_end,
                created_at,
                updated_at,
                verified_evidence,
                actor,
                primary_authority,
                custody_key,
                custody_public_key,
                owner_account,
                budget_account,
                budget_asset,
                purpose_hash,
                recovery_root,
                recovery_threshold,
                capability_id,
                activity_types,
                counterparties,
                assets,
                amount_ceiling,
                rate_maximum_uses,
                rate_window_sequences,
                purposes,
                capability_expiry_sequence,
                session_scopes,
                session_expiry_unix_seconds,
                protocol_grant_id,
                budget_period_seconds,
                budget_expiry_seconds,
                initial_funding,
                network_id,
                creation_receipt_roots,
            };
            Some(value)
        }
        _ => return Err(HumanProtocolError::Malformed),
    };
    Ok(HumanOwnerInstall {
        agent,
        authority_kind,
        authority_id,
        session_id,
        token_id,
        session_public_key,
        registration_payload,
        grantor,
        grant_not_before,
        grant_expires_at,
        grant_revocation_sequence,
        session_secret,
        permitted_activity_types,
        scopes,
        lease_not_before_unix_ms,
        lease_not_after_unix_ms,
        opening_client,
        policy_version,
        lifecycle,
    })
}

fn fixed_list(reader: &mut Reader, maximum: usize) -> Result<Vec<[u8; 32]>, HumanProtocolError> {
    let count = usize::from(reader.u16()?);
    if count == 0 || count > maximum {
        return Err(HumanProtocolError::Malformed);
    }
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        values.push(reader.fixed()?)
    }
    Ok(values)
}
fn lifecycle_seed(reader: &mut Reader) -> Result<HumanAgentLifecycleSeed, HumanProtocolError> {
    let agent_id = reader.text()?;
    let name = reader.text()?;
    let purpose = reader.text()?;
    let currency = reader.text()?;
    let monthly_limit = reader.u128()?;
    let period_start = reader.u64()?;
    let period_end = reader.u64()?;
    let created_at = reader.u64()?;
    let updated_at = reader.u64()?;
    let verified_evidence = fixed_list(reader, 64)?;
    let actor = reader.text()?;
    let primary_authority = reader.text()?;
    let custody_key = reader.text()?;
    let custody_public_key = reader.fixed()?;
    let owner_account = reader.text()?;
    let budget_account = reader.text()?;
    let budget_asset = reader.fixed()?;
    let purpose_hash = reader.fixed()?;
    let recovery_root = reader.fixed()?;
    let recovery_threshold = reader.u16()?;
    let capability_id = reader.fixed()?;
    let activity_types = u32_list(reader, 256)?;
    let counterparties = fixed_list(reader, 256)?;
    let assets = fixed_list(reader, 256)?;
    let amount_ceiling = reader.u128()?;
    let rate_maximum_uses = reader.u64()?;
    let rate_window_sequences = reader.u64()?;
    let purposes = text_list(reader, 64)?;
    let capability_expiry_sequence = reader.u64()?;
    let session_scopes = text_list(reader, 64)?;
    let session_expiry_unix_seconds = reader.u64()?;
    let protocol_grant_id = reader.fixed()?;
    let budget_period_seconds = reader.u64()?;
    let budget_expiry_seconds = reader.u64()?;
    let initial_funding = reader.u128()?;
    let network_id = reader.u32()?;
    let creation_receipt_roots = fixed_list(reader, 64)?;
    Ok(HumanAgentLifecycleSeed {
        agent_id,
        name,
        purpose,
        currency,
        monthly_limit,
        period_start,
        period_end,
        created_at,
        updated_at,
        verified_evidence,
        actor,
        primary_authority,
        custody_key,
        custody_public_key,
        owner_account,
        budget_account,
        budget_asset,
        purpose_hash,
        recovery_root,
        recovery_threshold,
        capability_id,
        activity_types,
        counterparties,
        assets,
        amount_ceiling,
        rate_maximum_uses,
        rate_window_sequences,
        purposes,
        capability_expiry_sequence,
        session_scopes,
        session_expiry_unix_seconds,
        protocol_grant_id,
        budget_period_seconds,
        budget_expiry_seconds,
        initial_funding,
        network_id,
        creation_receipt_roots,
    })
}
fn u32_list(reader: &mut Reader, maximum: usize) -> Result<Vec<u32>, HumanProtocolError> {
    let count = usize::from(reader.u16()?);
    if count == 0 || count > maximum {
        return Err(HumanProtocolError::Malformed);
    }
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        values.push(reader.u32()?)
    }
    Ok(values)
}
fn text_list(reader: &mut Reader, maximum: usize) -> Result<Vec<String>, HumanProtocolError> {
    let count = usize::from(reader.u16()?);
    if count == 0 || count > maximum {
        return Err(HumanProtocolError::Malformed);
    }
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        values.push(reader.text()?)
    }
    Ok(values)
}

fn finalization(reader: &mut Reader) -> Result<HumanFinalizationEvidence, HumanProtocolError> {
    let value = HumanFinalizationEvidence {
        action_key: reader.fixed()?,
        activity_id: reader.fixed()?,
        receipt_digest: reader.fixed()?,
        observed_sequence: reader.u64()?,
        verification: reader.u8()?,
        finalized_at: reader.u64()?,
    };
    if value.action_key == [0; 32]
        || value.activity_id == [0; 32]
        || value.receipt_digest == [0; 32]
        || value.observed_sequence == 0
        || value.verification < 4
        || value.verification > 5
        || value.finalized_at == 0
    {
        return Err(HumanProtocolError::Malformed);
    }
    Ok(value)
}

fn mutation_header(reader: &mut Reader) -> Result<(u64, [u8; 32], [u8; 32]), HumanProtocolError> {
    let request_id = reader.u64()?;
    let key = reader.fixed()?;
    let body_digest = reader.fixed()?;
    if key == [0; 32] || body_digest == [0; 32] {
        return Err(HumanProtocolError::Malformed);
    }
    Ok((request_id, key, body_digest))
}

struct Reader {
    bytes: Vec<u8>,
    offset: usize,
}

impl Drop for Reader {
    fn drop(&mut self) {
        self.bytes.fill(0);
    }
}

impl Reader {
    fn new(bytes: Vec<u8>) -> Self {
        Self { bytes, offset: 0 }
    }
    fn take(&mut self, length: usize) -> Result<&[u8], HumanProtocolError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(HumanProtocolError::Malformed)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(HumanProtocolError::Malformed)?;
        self.offset = end;
        Ok(value)
    }
    fn u8(&mut self) -> Result<u8, HumanProtocolError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, HumanProtocolError> {
        Ok(u16::from_be_bytes(self.fixed()?))
    }
    fn u32(&mut self) -> Result<u32, HumanProtocolError> {
        Ok(u32::from_be_bytes(self.fixed()?))
    }
    fn u64(&mut self) -> Result<u64, HumanProtocolError> {
        Ok(u64::from_be_bytes(self.fixed()?))
    }
    fn u128(&mut self) -> Result<u128, HumanProtocolError> {
        Ok(u128::from_be_bytes(self.fixed()?))
    }
    fn fixed<const N: usize>(&mut self) -> Result<[u8; N], HumanProtocolError> {
        self.take(N)?
            .try_into()
            .map_err(|_| HumanProtocolError::Malformed)
    }
    fn bytes(&mut self) -> Result<Vec<u8>, HumanProtocolError> {
        let length = usize::try_from(self.u32()?).map_err(|_| HumanProtocolError::Malformed)?;
        if length == 0 || length > MAX_BYTES {
            return Err(HumanProtocolError::Malformed);
        }
        Ok(self.take(length)?.to_vec())
    }
    fn text(&mut self) -> Result<String, HumanProtocolError> {
        let bytes = self.bytes()?;
        if bytes.len() > MAX_TEXT {
            return Err(HumanProtocolError::Malformed);
        }
        String::from_utf8(bytes).map_err(|_| HumanProtocolError::Malformed)
    }
    fn finish(&self) -> Result<(), HumanProtocolError> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(HumanProtocolError::Malformed)
        }
    }
}

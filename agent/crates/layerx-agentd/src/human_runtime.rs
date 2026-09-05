//! Production operation owner for the authenticated Human-to-agent boundary.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, RwLock};

use layerx_client::evidence::{CheckpointSelector, EvidenceError, ProofBundleSelector};
use layerx_client::receipt::{Lookup, ReceiptSelector};
use layerx_client::submit::Submission;
use layerx_client::Client;
use layerx_proof::inclusion::SequencerAuthorization;
use layerx_proof::receipt::AuthorizedBatch;
use layerx_types::activity::{Authority, TimestampBound};
use layerx_types::amount::Amount;
use layerx_types::ids::{Did, IdempotencyKey};
use layerx_types::payload::{ActivityType, ModuleRegistry};
use layerx_types::result::Retriability;
use layerx_types::verify::VerificationLevel;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::time::Duration;

use crate::approval::{
    ApprovalDecision, ApprovalExpiry, ApprovalOutcome, ApprovalRecord, ApprovalService,
    ApprovalSubmissionQueue, DecisionKey, DecisionRequest,
};
use crate::budget::{BudgetLimiter, LimitConfig};
use crate::capability::{
    assert_narrowing, Capability, CapabilityDimensions, CapabilityId, RateCeiling,
};
use crate::degraded::Controller;
use crate::human::{
    HumanCapabilityInstall, HumanFinalizationEvidence, HumanOperationError, HumanOperations,
    HumanOwnerInstall, HumanPeer, HumanPrepare, HumanResponse, HumanSubmit, MutationEnvelope,
};
use crate::identity::{self, CoreIdentity, IdentityError, IdentityResolver, ProtocolAuthority};
use crate::managed_agent::{self, ManagedAgent};
use crate::outbox::{Outbox, SubmissionState, SubmissionStatus};
use crate::policy::approval::{ApprovalRegistry, ApprovalState, ApproverId};
use crate::prepare::PreparationLifecycle;
use crate::prepare::{
    prepare_activity_for_protocol, CoreStateError, PreparationDefaults, PrepareRequest, Prepared,
    ProductionCorePreparationBoundary,
};
use crate::session::{self, OpenRequest, SessionId, SessionRegistry};
use crate::session_control::SessionControl;
use crate::session_keys::SessionKeyRegistry;
use crate::sign::{
    attach_external_signature, validate_issued_session, verify_before_submit, ProvisionedSessionKey,
};
use crate::store::{key, ObjectKind, StorageClass, Store, TenantId, TenantKey};

const MAX_RESPONSE: usize = 1_048_576;

/// Independently authenticated authority data. Implementations must obtain
/// these values from the configured authority peer; no local/static authority
/// is accepted by this operation owner.
pub trait HumanAuthorityBoundary {
    fn registry(&self, peer: &HumanPeer) -> Result<ModuleRegistry, CoreStateError>;
    fn authorized_batch(
        &mut self,
        peer: &HumanPeer,
        expected_activity: [u8; 32],
    ) -> Result<AuthorizedBatch, HumanOperationError>;
    fn balance_context(
        &mut self,
        peer: &HumanPeer,
    ) -> Result<
        (
            [u8; 32],
            [u8; 32],
            String,
            String,
            u64,
            u64,
            SequencerAuthorization,
        ),
        HumanOperationError,
    >;
    fn core_identity(
        &mut self,
        peer: &HumanPeer,
        agent: &Did,
    ) -> Result<CoreIdentity, IdentityError>;
    fn lease_attestation(
        &mut self,
        peer: &HumanPeer,
    ) -> Result<CoreLeaseAttestation, HumanOperationError>;
    fn capability_scope(
        &mut self,
        peer: &HumanPeer,
        agent: &Did,
        authority_id: [u8; 32],
        action_key: [u8; 32],
        capability_id: [u8; 32],
    ) -> Result<CoreCapabilityScope, HumanOperationError>;
    fn budget_state(
        &mut self,
        peer: &HumanPeer,
        active_budget_id: [u8; 32],
    ) -> Result<CoreBudgetState, HumanOperationError>;
    fn key_rotation_policy(
        &mut self,
        peer: &HumanPeer,
        did: &Did,
        recovery: bool,
    ) -> Result<CoreKeyPolicy, HumanOperationError>;
}
pub struct CoreCapabilityScope {
    pub scope: crate::capability::ProtocolScope,
    pub observed_sequence: u64,
    pub verification: u8,
    pub evidence_digest: [u8; 32],
}
pub struct CoreBudgetState {
    pub revocation_sequence: u64,
    pub observed_head_sequence: u64,
    pub verification: u8,
    pub evidence_digest: [u8; 32],
    pub receipt_digest: [u8; 32],
    pub checkpoint_digest: [u8; 32],
    pub age_sequences: u64,
    pub maximum_age_sequences: u64,
    pub remaining: u128,
    pub asset: [u8; 32],
}
pub struct CoreKeyPolicy {
    pub policy_revision: u64,
    pub required_delay_seconds: u64,
    pub maximum_delay_seconds: u64,
    pub effective_sequence: u64,
    pub observed_head_sequence: u64,
    pub verification: u8,
    pub evidence_digest: [u8; 32],
    pub checkpoint_digest: [u8; 32],
    pub age_sequences: u64,
    pub maximum_age_sequences: u64,
}

/// Authenticated authority observation relating wall clock to the core clock.
/// Both anchors are required so conversion never assumes seconds equal blocks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreLeaseAttestation {
    pub lower_unix_ms: u64,
    pub lower_sequence: u64,
    pub upper_unix_ms: u64,
    pub upper_sequence: u64,
    pub observed_head_sequence: u64,
    pub canonical_bytes: Vec<u8>,
}

impl CoreLeaseAttestation {
    fn map(&self, not_before: u64, not_after: u64) -> Result<(u64, u64), HumanOperationError> {
        let wall_span = self
            .upper_unix_ms
            .checked_sub(self.lower_unix_ms)
            .ok_or(HumanOperationError::Refused)?;
        let sequence_span = self
            .upper_sequence
            .checked_sub(self.lower_sequence)
            .ok_or(HumanOperationError::Refused)?;
        if wall_span == 0
            || sequence_span == 0
            || self.canonical_bytes.is_empty()
            || self.lower_sequence == 0
            || self.observed_head_sequence < self.lower_sequence
            || self.observed_head_sequence > self.upper_sequence
            || not_before < self.lower_unix_ms
            || not_after <= not_before
            || not_after > self.upper_unix_ms
        {
            return Err(HumanOperationError::Refused);
        }
        let lower_delta = not_before - self.lower_unix_ms;
        let upper_delta = not_after - self.lower_unix_ms;
        let first = self
            .lower_sequence
            .checked_add(
                lower_delta
                    .checked_mul(sequence_span)
                    .ok_or(HumanOperationError::Refused)?
                    / wall_span,
            )
            .ok_or(HumanOperationError::Refused)?;
        let upper_product = upper_delta
            .checked_mul(sequence_span)
            .ok_or(HumanOperationError::Refused)?;
        let last = self
            .lower_sequence
            .checked_add(
                upper_product
                    .checked_add(wall_span - 1)
                    .ok_or(HumanOperationError::Refused)?
                    / wall_span,
            )
            .ok_or(HumanOperationError::Refused)?;
        if last <= self.observed_head_sequence || last <= first {
            return Err(HumanOperationError::Refused);
        }
        Ok((first, last))
    }
}

/// TLS-authenticated independent authority replica. The endpoint is distinct
/// from the node LNI socket and every response is decoded into closed protocol
/// types before it can influence preparation or receipt verification.
pub struct RemoteHumanAuthority {
    agent: ureq::Agent,
    endpoint: String,
    bearer: String,
    maximum_response_bytes: usize,
}

impl RemoteHumanAuthority {
    pub fn connect(
        endpoint: &str,
        bearer: String,
        deadline: Duration,
        maximum_response_bytes: usize,
    ) -> Result<Self, HumanOperationError> {
        let endpoint = endpoint.trim_end_matches('/');
        if !endpoint.starts_with("https://")
            || bearer.len() < 32
            || deadline.is_zero()
            || maximum_response_bytes == 0
            || maximum_response_bytes > MAX_RESPONSE
        {
            return Err(HumanOperationError::Refused);
        }
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(deadline))
            .http_status_as_error(false)
            .build();
        Ok(Self {
            agent: config.into(),
            endpoint: endpoint.to_owned(),
            bearer,
            maximum_response_bytes,
        })
    }

    fn get(&self, path: &str) -> Result<Value, HumanOperationError> {
        let mut response = self
            .agent
            .get(format!("{}{path}", self.endpoint))
            .header("Authorization", &format!("Bearer {}", self.bearer))
            .call()
            .map_err(|_| HumanOperationError::Unavailable)?;
        if !response.status().is_success() {
            return Err(HumanOperationError::Refused);
        }
        if response
            .body()
            .content_length()
            .is_some_and(|length| length > self.maximum_response_bytes as u64)
        {
            return Err(HumanOperationError::Refused);
        }
        let body = response
            .body_mut()
            .with_config()
            .limit(self.maximum_response_bytes.saturating_add(1) as u64)
            .read_to_string()
            .map_err(|_| HumanOperationError::Unavailable)?;
        if body.len() > self.maximum_response_bytes {
            return Err(HumanOperationError::Refused);
        }
        serde_json::from_str(&body).map_err(|_| HumanOperationError::Refused)
    }

    fn registry_from(value: &Value) -> Result<ModuleRegistry, HumanOperationError> {
        let modules = value
            .get("modules")
            .and_then(Value::as_array)
            .ok_or(HumanOperationError::Refused)?;
        if modules.is_empty() || modules.len() > 32 {
            return Err(HumanOperationError::Refused);
        }
        let mut registrations = Vec::with_capacity(modules.len());
        for module in modules {
            let id = u16::try_from(
                module
                    .get("module_id")
                    .and_then(Value::as_u64)
                    .ok_or(HumanOperationError::Refused)?,
            )
            .map_err(|_| HumanOperationError::Refused)?;
            let module_id = layerx_types::payload::ModuleId::from_u16(id)
                .map_err(|_| HumanOperationError::Refused)?;
            let values = module
                .get("activity_types")
                .and_then(Value::as_array)
                .ok_or(HumanOperationError::Refused)?;
            let mut activities = Vec::with_capacity(values.len());
            for value in values {
                activities.push(
                    ActivityType::from_u32(
                        u32::try_from(value.as_u64().ok_or(HumanOperationError::Refused)?)
                            .map_err(|_| HumanOperationError::Refused)?,
                    )
                    .map_err(|_| HumanOperationError::Refused)?,
                );
            }
            registrations.push(
                layerx_types::payload::ModuleRegistration::new(module_id, &activities)
                    .map_err(|_| HumanOperationError::Refused)?,
            );
        }
        ModuleRegistry::new(&registrations).map_err(|_| HumanOperationError::Refused)
    }
}

impl HumanAuthorityBoundary for RemoteHumanAuthority {
    fn registry(&self, peer: &HumanPeer) -> Result<ModuleRegistry, CoreStateError> {
        self.get(&format!(
            "/v1/agent/registry?tenant={}&principal={}",
            query(&peer.tenant),
            query(&peer.principal)
        ))
        .and_then(|value| Self::registry_from(&value))
        .map_err(|error| match error {
            HumanOperationError::Unavailable => CoreStateError::Unavailable,
            HumanOperationError::Refused => CoreStateError::Unverified,
        })
    }
    fn authorized_batch(
        &mut self,
        peer: &HumanPeer,
        expected_activity: [u8; 32],
    ) -> Result<AuthorizedBatch, HumanOperationError> {
        let value = self.get(&format!(
            "/v1/agent/authorized-batch?tenant={}&principal={}&activity_id={}",
            query(&peer.tenant),
            query(&peer.principal),
            hex(&expected_activity)
        ))?;
        Ok(AuthorizedBatch::new(
            hex_field(&value, "batch_id")?,
            hex_field(&value, "asset")?,
            hex_field(&value, "previous_state_root")?,
            hex_field(&value, "resulting_state_root")?,
            hex_field(&value, "sequencer_public_key")?,
        ))
    }
    fn balance_context(
        &mut self,
        peer: &HumanPeer,
    ) -> Result<
        (
            [u8; 32],
            [u8; 32],
            String,
            String,
            u64,
            u64,
            SequencerAuthorization,
        ),
        HumanOperationError,
    > {
        let value = self.get(&format!(
            "/v1/agent/balance-context?tenant={}&principal={}",
            query(&peer.tenant),
            query(&peer.principal)
        ))?;
        let currency = value
            .get("currency")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty() && value.len() <= 32)
            .ok_or(HumanOperationError::Refused)?
            .to_owned();
        let observed_at = value
            .get("observed_at")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty() && value.len() <= 64)
            .ok_or(HumanOperationError::Refused)?
            .to_owned();
        Ok((
            hex_field(&value, "account_id")?,
            hex_field(&value, "asset_id")?,
            currency,
            observed_at,
            u64_field(&value, "age_seconds")?,
            u64_field(&value, "maximum_age_seconds")?,
            SequencerAuthorization::new(
                hex_field(&value, "sequencer_id")?,
                hex_field(&value, "sequencer_public_key")?,
                u64_field(&value, "first_batch_number")?,
                u64_field(&value, "last_batch_number")?,
            ),
        ))
    }
    fn core_identity(
        &mut self,
        peer: &HumanPeer,
        agent: &Did,
    ) -> Result<CoreIdentity, IdentityError> {
        let did = std::str::from_utf8(agent.as_bytes()).map_err(|_| IdentityError::Unverified)?;
        let value = self
            .get(&format!(
                "/v1/agent/identity?tenant={}&principal={}&did={}",
                query(&peer.tenant),
                query(&peer.principal),
                query(did)
            ))
            .map_err(map_identity)?;
        let authorities = value
            .get("authorities")
            .and_then(Value::as_array)
            .ok_or(IdentityError::Unverified)?
            .iter()
            .map(|entry| {
                let id = entry
                    .get("id")
                    .and_then(Value::as_str)
                    .and_then(digest_from_hex)
                    .ok_or(IdentityError::Unverified)?;
                match entry.get("kind").and_then(Value::as_str) {
                    Some("primary_key") => Ok(ProtocolAuthority::PrimaryKey(id)),
                    Some("session_key") => Ok(ProtocolAuthority::SessionKey(id)),
                    Some("capability_grant") => Ok(ProtocolAuthority::CapabilityGrant(id)),
                    _ => Err(IdentityError::Unverified),
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        if authorities.is_empty() {
            return Err(IdentityError::Unverified);
        }
        let canonical = value
            .get("canonical_core_bytes")
            .and_then(Value::as_str)
            .and_then(decode_hex)
            .ok_or(IdentityError::Unverified)?;
        Ok(CoreIdentity {
            canonical_bytes: canonical,
            head_sequence: u64_field(&value, "head_sequence")
                .map_err(|_| IdentityError::Unverified)?,
            revocation_sequence: u64_field(&value, "revocation_sequence")
                .map_err(|_| IdentityError::Unverified)?,
            verification_level: verification_level(&value)?,
            frozen: value
                .get("frozen")
                .and_then(Value::as_bool)
                .ok_or(IdentityError::Unverified)?,
            authorities,
        })
    }
    fn lease_attestation(
        &mut self,
        peer: &HumanPeer,
    ) -> Result<CoreLeaseAttestation, HumanOperationError> {
        let value = self.get(&format!(
            "/v1/agent/core-clock?tenant={}&principal={}",
            query(&peer.tenant),
            query(&peer.principal)
        ))?;
        Ok(CoreLeaseAttestation {
            lower_unix_ms: u64_field(&value, "lower_unix_ms")?,
            lower_sequence: u64_field(&value, "lower_sequence")?,
            upper_unix_ms: u64_field(&value, "upper_unix_ms")?,
            upper_sequence: u64_field(&value, "upper_sequence")?,
            observed_head_sequence: u64_field(&value, "observed_head_sequence")?,
            canonical_bytes: value
                .get("canonical_attestation")
                .and_then(Value::as_str)
                .and_then(decode_hex)
                .filter(|v| !v.is_empty())
                .ok_or(HumanOperationError::Refused)?,
        })
    }
    fn capability_scope(
        &mut self,
        peer: &HumanPeer,
        agent: &Did,
        authority_id: [u8; 32],
        action_key: [u8; 32],
        capability_id: [u8; 32],
    ) -> Result<CoreCapabilityScope, HumanOperationError> {
        let did =
            std::str::from_utf8(agent.as_bytes()).map_err(|_| HumanOperationError::Refused)?;
        let value = self.get(&format!("/v1/agent/capability-scope?tenant={}&principal={}&did={}&authority={}&action_key={}&capability_id={}", query(&peer.tenant), query(&peer.principal), query(did), hex(&authority_id), hex(&action_key), hex(&capability_id)))?;
        let set16 = |name| -> Result<std::collections::BTreeSet<u16>, HumanOperationError> {
            value
                .get(name)
                .and_then(Value::as_array)
                .ok_or(HumanOperationError::Refused)?
                .iter()
                .map(|v| {
                    u16::try_from(v.as_u64().ok_or(HumanOperationError::Refused)?)
                        .map_err(|_| HumanOperationError::Refused)
                })
                .collect()
        };
        let set32 = |name| -> Result<std::collections::BTreeSet<[u8; 32]>, HumanOperationError> {
            value
                .get(name)
                .and_then(Value::as_array)
                .ok_or(HumanOperationError::Refused)?
                .iter()
                .map(|v| {
                    v.as_str()
                        .and_then(digest_from_hex)
                        .ok_or(HumanOperationError::Refused)
                })
                .collect()
        };
        let enforceable_dimensions = value
            .get("enforceable_dimensions")
            .and_then(Value::as_array)
            .ok_or(HumanOperationError::Refused)?
            .iter()
            .map(|v| match v.as_str() {
                Some("activity_type") => Ok(crate::capability::Dimension::ActivityType),
                Some("counterparty") => Ok(crate::capability::Dimension::Counterparty),
                Some("asset") => Ok(crate::capability::Dimension::Asset),
                Some("amount") => Ok(crate::capability::Dimension::Amount),
                Some("rate") => Ok(crate::capability::Dimension::Rate),
                Some("purpose") => Ok(crate::capability::Dimension::Purpose),
                Some("expiry") => Ok(crate::capability::Dimension::Expiry),
                _ => Err(HumanOperationError::Refused),
            })
            .collect::<Result<_, _>>()?;
        Ok(CoreCapabilityScope {
            scope: crate::capability::ProtocolScope {
                activity_types: set16("activity_types")?,
                counterparties: set32("counterparties")?,
                assets: set32("assets")?,
                amount_ceiling: value
                    .get("amount_ceiling")
                    .and_then(Value::as_str)
                    .ok_or(HumanOperationError::Refused)?
                    .parse()
                    .map_err(|_| HumanOperationError::Refused)?,
                expires_at_sequence: u64_field(&value, "expiry_sequence")?,
                enforceable_dimensions,
            },
            observed_sequence: u64_field(&value, "observed_sequence")?,
            verification: u8::try_from(u64_field(&value, "verification")?)
                .map_err(|_| HumanOperationError::Refused)?,
            evidence_digest: hex_field(&value, "evidence_digest")?,
        })
    }
    fn budget_state(
        &mut self,
        peer: &HumanPeer,
        active_budget_id: [u8; 32],
    ) -> Result<CoreBudgetState, HumanOperationError> {
        let value = self.get(&format!(
            "/v1/agent/budget-state?tenant={}&principal={}&budget_id={}",
            query(&peer.tenant),
            query(&peer.principal),
            hex(&active_budget_id)
        ))?;
        let state = CoreBudgetState {
            revocation_sequence: u64_field(&value, "revocation_sequence")?,
            observed_head_sequence: u64_field(&value, "observed_head_sequence")?,
            verification: u8::try_from(u64_field(&value, "verification")?)
                .map_err(|_| HumanOperationError::Refused)?,
            evidence_digest: hex_field(&value, "evidence_digest")?,
            receipt_digest: hex_field(&value, "receipt_digest")?,
            checkpoint_digest: hex_field(&value, "checkpoint_digest")?,
            age_sequences: u64_field(&value, "age_sequences")?,
            maximum_age_sequences: u64_field(&value, "maximum_age_sequences")?,
            remaining: value
                .get("remaining")
                .and_then(Value::as_str)
                .ok_or(HumanOperationError::Refused)?
                .parse()
                .map_err(|_| HumanOperationError::Refused)?,
            asset: hex_field(&value, "asset")?,
        };
        if state.revocation_sequence == 0
            || state.observed_head_sequence < state.revocation_sequence
            || state.verification < 4
            || state.verification > 5
            || state.evidence_digest == [0; 32]
            || state.receipt_digest == [0; 32]
            || state.checkpoint_digest == [0; 32]
            || state.maximum_age_sequences == 0
            || state.age_sequences > state.maximum_age_sequences
            || state.asset == [0; 32]
        {
            return Err(HumanOperationError::Refused);
        }
        Ok(state)
    }
    fn key_rotation_policy(
        &mut self,
        peer: &HumanPeer,
        did: &Did,
        recovery: bool,
    ) -> Result<CoreKeyPolicy, HumanOperationError> {
        let did = std::str::from_utf8(did.as_bytes()).map_err(|_| HumanOperationError::Refused)?;
        let value = self.get(&format!(
            "/v1/agent/key-policy?tenant={}&principal={}&did={}&recovery={}",
            query(&peer.tenant),
            query(&peer.principal),
            query(did),
            recovery
        ))?;
        let state = CoreKeyPolicy {
            policy_revision: u64_field(&value, "policy_revision")?,
            required_delay_seconds: u64_field(&value, "required_delay_seconds")?,
            maximum_delay_seconds: u64_field(&value, "maximum_delay_seconds")?,
            effective_sequence: u64_field(&value, "effective_sequence")?,
            observed_head_sequence: u64_field(&value, "observed_head_sequence")?,
            verification: u8::try_from(u64_field(&value, "verification")?)
                .map_err(|_| HumanOperationError::Refused)?,
            evidence_digest: hex_field(&value, "evidence_digest")?,
            checkpoint_digest: hex_field(&value, "checkpoint_digest")?,
            age_sequences: u64_field(&value, "age_sequences")?,
            maximum_age_sequences: u64_field(&value, "maximum_age_sequences")?,
        };
        if state.policy_revision == 0
            || state.required_delay_seconds == 0
            || state.maximum_delay_seconds < state.required_delay_seconds
            || state.effective_sequence == 0
            || state.observed_head_sequence < state.effective_sequence
            || state.verification < 4
            || state.verification > 5
            || state.evidence_digest == [0; 32]
            || state.checkpoint_digest == [0; 32]
            || state.maximum_age_sequences == 0
            || state.age_sequences > state.maximum_age_sequences
        {
            return Err(HumanOperationError::Refused);
        }
        Ok(state)
    }
}

/// Concrete production path. Prepared bytes remain key-free; externally
/// supplied signatures are attached, reverified, durably queued, and only then
/// submitted through the sole frozen LNI client.
pub struct ProductionHumanOperations<A> {
    authority: A,
    node: Client,
    store: Arc<Mutex<Store>>,
    outbox: Outbox,
    prepared: BTreeMap<(String, String, String), CachedPreparation>,
    submissions: BTreeMap<(String, String, String), [u8; 32]>,
    maximum_payload_bytes: usize,
    timestamp_span: u64,
    last_verified_receipt: Option<([u8; 32], i32, u64)>,
    unified_owner_active: bool,
}

#[derive(Clone)]
struct CachedPreparation {
    prepared: Prepared,
    registry: ModuleRegistry,
}

/// One daemon-owned composition for every mutable agent authority. Services
/// construct short-lived adapters borrowing these owners; none opens a second
/// store or maintains an independent approval/budget/session universe.
pub struct UnifiedAgentOwner<A> {
    operations: Arc<Mutex<ProductionHumanOperations<A>>>,
    store: Arc<Mutex<Store>>,
    pub approvals: Arc<ApprovalRegistry>,
    pub approval_queue: Arc<ApprovalSubmissionQueue>,
    pub approval_expiry: Arc<ApprovalExpiry>,
    pub budgets: Arc<BudgetLimiter>,
    pub preparation_lifecycle: Arc<PreparationLifecycle>,
    pub sessions: Arc<RwLock<SessionRegistry>>,
    pub session_control: SessionControl,
    pub session_keys: SessionKeyRegistry,
    pub degraded: Controller,
}

impl<A: HumanAuthorityBoundary> UnifiedAgentOwner<A> {
    pub fn new(
        mut operations: ProductionHumanOperations<A>,
        shared_store: Arc<Mutex<Store>>,
        peers: &BTreeMap<u32, (String, String)>,
        verified_limits: Vec<LimitConfig>,
        session_keys: SessionKeyRegistry,
    ) -> Result<Self, HumanOperationError> {
        if verified_limits.is_empty() {
            return Err(HumanOperationError::Refused);
        }
        let approvals = Arc::new(ApprovalRegistry::with_store(Arc::clone(&shared_store)));
        let budgets = Arc::new(
            BudgetLimiter::new(verified_limits).map_err(|_| HumanOperationError::Refused)?,
        );
        let approval_queue = Arc::new(ApprovalSubmissionQueue::default());
        let mut replayed = std::collections::BTreeSet::new();
        for (uid, (principal, tenant)) in peers {
            if replayed.insert(tenant.clone()) {
                let tenant_id =
                    TenantId::new(tenant.clone()).map_err(|_| HumanOperationError::Refused)?;
                let registry = operations
                    .authority
                    .registry(&HumanPeer {
                        uid: *uid,
                        principal: principal.clone(),
                        tenant: tenant.clone(),
                    })
                    .map_err(map_core)?;
                let released = approvals
                    .replay_released(&tenant_id, &budgets, &registry)
                    .map_err(|_| HumanOperationError::Refused)?;
                approval_queue
                    .restore(released)
                    .map_err(|_| HumanOperationError::Refused)?;
                approvals
                    .replay_tenant(&tenant_id, &budgets)
                    .map_err(|_| HumanOperationError::Refused)?;
                approvals
                    .validate_registry(&tenant_id, &registry)
                    .map_err(|_| HumanOperationError::Refused)?;
                {
                    let store = shared_store
                        .lock()
                        .map_err(|_| HumanOperationError::Unavailable)?;
                    managed_agent::validate_tenant(&store, &tenant_id)?;
                }
            }
        }
        for id in approvals
            .hold_ids()
            .map_err(|_| HumanOperationError::Refused)?
        {
            if !budgets
                .has_reservation(id)
                .map_err(|_| HumanOperationError::Refused)?
            {
                return Err(HumanOperationError::Refused);
            }
        }
        operations.unified_owner_active = true;
        {
            let store = shared_store
                .lock()
                .map_err(|_| HumanOperationError::Unavailable)?;
            for tenant in store.tenant_ids_for_kind(ObjectKind::Session) {
                replayed.insert(tenant.as_str().to_owned());
            }
        }
        let mut sessions = SessionRegistry::default();
        {
            let store = shared_store
                .lock()
                .map_err(|_| HumanOperationError::Unavailable)?;
            for tenant in &replayed {
                let tenant_id =
                    TenantId::new(tenant.clone()).map_err(|_| HumanOperationError::Refused)?;
                sessions
                    .restore_tenant(&store, &tenant_id)
                    .map_err(|_| HumanOperationError::Refused)?;
                managed_agent::validate_session_coordinates(&store, &sessions, &tenant_id)?;
            }
        }
        session_keys
            .probe()
            .map_err(|_| HumanOperationError::Unavailable)?;
        let preparation_lifecycle = Arc::new(PreparationLifecycle::default());
        let session_control = SessionControl::new(
            Arc::clone(&shared_store),
            sessions,
            Arc::clone(&preparation_lifecycle),
            Arc::clone(&budgets),
        );
        let sessions = session_control.registry();
        Ok(Self {
            operations: Arc::new(Mutex::new(operations)),
            store: Arc::clone(&shared_store),
            approvals,
            approval_queue,
            approval_expiry: Arc::new(ApprovalExpiry::from_shared_store(shared_store)),
            budgets,
            preparation_lifecycle,
            sessions,
            session_control,
            session_keys,
            degraded: Controller::default(),
        })
    }

    fn lock_operations(
        &self,
    ) -> Result<MutexGuard<'_, ProductionHumanOperations<A>>, HumanOperationError> {
        self.operations
            .lock()
            .map_err(|_| HumanOperationError::Unavailable)
    }
}

impl<A: HumanAuthorityBoundary> HumanOperations for UnifiedAgentOwner<A> {
    fn registry(&self, peer: &HumanPeer) -> Result<HumanResponse, HumanOperationError> {
        self.lock_operations()?.registry(peer)
    }
    fn prepare(
        &mut self,
        peer: &HumanPeer,
        request: MutationEnvelope<HumanPrepare>,
    ) -> Result<HumanResponse, HumanOperationError> {
        self.lock_operations()?.prepare(peer, request)
    }
    fn submit_external(
        &mut self,
        peer: &HumanPeer,
        request: MutationEnvelope<HumanSubmit>,
    ) -> Result<HumanResponse, HumanOperationError> {
        let prepared_key = (
            peer.tenant.clone(),
            peer.principal.clone(),
            request.operation.preparation_ref.clone(),
        );
        let prepared = self
            .lock_operations()?
            .prepared
            .get(&prepared_key)
            .cloned()
            .ok_or(HumanOperationError::Refused)?;
        let tenant =
            TenantId::new(peer.tenant.clone()).map_err(|_| HumanOperationError::Refused)?;
        self.approval_queue
            .authorize_submit(
                &tenant,
                &request.operation.preparation_ref,
                &prepared.prepared.canonical_bytes,
                request.operation.approval_release_ref,
            )
            .map_err(|_| HumanOperationError::Refused)?;
        self.lock_operations()?.submit_external(peer, request)
    }
    fn track(
        &mut self,
        peer: &HumanPeer,
        submission_ref: &str,
    ) -> Result<HumanResponse, HumanOperationError> {
        self.lock_operations()?.track(peer, submission_ref)
    }
    fn receipt_by_idempotency_key(
        &mut self,
        peer: &HumanPeer,
        idempotency_key: [u8; 32],
        expected_activity_id: [u8; 32],
    ) -> Result<HumanResponse, HumanOperationError> {
        let mut operations = self.lock_operations()?;
        let response =
            operations.receipt_by_idempotency_key(peer, idempotency_key, expected_activity_id)?;
        if let Some((key, result_code, sequence)) = operations.last_verified_receipt.take() {
            let tenant =
                TenantId::new(peer.tenant.clone()).map_err(|_| HumanOperationError::Refused)?;
            let mut store = self
                .store
                .lock()
                .map_err(|_| HumanOperationError::Unavailable)?;
            self.approval_queue
                .settle_verified(
                    &tenant,
                    key,
                    result_code,
                    sequence,
                    &mut store,
                    &self.budgets,
                )
                .map_err(|_| HumanOperationError::Unavailable)?;
        }
        Ok(response)
    }
    fn approval_list(
        &mut self,
        peer: &HumanPeer,
        current_sequence: u64,
        cursor: Option<[u8; 32]>,
        limit: u8,
    ) -> Result<HumanResponse, HumanOperationError> {
        let tenant =
            TenantId::new(peer.tenant.clone()).map_err(|_| HumanOperationError::Refused)?;
        let page = ApprovalService::new(&self.approvals, &self.budgets, &self.approval_expiry)
            .list(&tenant, cursor, usize::from(limit), current_sequence)
            .map_err(|_| HumanOperationError::Refused)?;
        let mut out = Encoder::new();
        out.u8(u8::try_from(page.approvals.len()).map_err(|_| HumanOperationError::Refused)?);
        for record in &page.approvals {
            encode_approval(&mut out, record)?;
        }
        match page.next_cursor {
            Some(value) => {
                out.u8(1);
                out.fixed(&value);
            }
            None => out.u8(0),
        }
        out.finish()
    }
    fn approval_get(
        &mut self,
        peer: &HumanPeer,
        approval_id: [u8; 32],
        current_sequence: u64,
    ) -> Result<HumanResponse, HumanOperationError> {
        let tenant =
            TenantId::new(peer.tenant.clone()).map_err(|_| HumanOperationError::Refused)?;
        let record = ApprovalService::new(&self.approvals, &self.budgets, &self.approval_expiry)
            .get(&tenant, approval_id, current_sequence)
            .map_err(|_| HumanOperationError::Refused)?;
        let mut out = Encoder::new();
        encode_approval(&mut out, &record)?;
        out.finish()
    }
    fn approval_approve(
        &mut self,
        peer: &HumanPeer,
        approval_id: [u8; 32],
        held_digest: [u8; 32],
        idempotency_key: &str,
        current_sequence: u64,
    ) -> Result<HumanResponse, HumanOperationError> {
        let tenant =
            TenantId::new(peer.tenant.clone()).map_err(|_| HumanOperationError::Refused)?;
        let key = DecisionKey::new(idempotency_key).map_err(|_| HumanOperationError::Refused)?;
        if let Some(decision) = self
            .approval_expiry
            .repeated(&tenant, approval_id, &key)
            .map_err(|_| HumanOperationError::Unavailable)?
        {
            if decision.outcome == ApprovalOutcome::Granted {
                let reference = decision
                    .submission_ref
                    .ok_or(HumanOperationError::Refused)?;
                if !self
                    .approval_queue
                    .matches_released_decision(&tenant, approval_id, reference, held_digest)
                    .map_err(|_| HumanOperationError::Unavailable)?
                {
                    return Err(HumanOperationError::Refused);
                }
            }
            return encode_decision(&decision);
        }
        let snapshot = self
            .approvals
            .get_scoped(&tenant, approval_id, current_sequence)
            .map_err(|_| HumanOperationError::Refused)?;
        if snapshot.prepared.disclosure.canonical_digest != held_digest {
            return Err(HumanOperationError::Refused);
        }
        let decision = ApprovalService::new(&self.approvals, &self.budgets, &self.approval_expiry)
            .approve(
                DecisionRequest {
                    tenant: &tenant,
                    approval_id,
                    idempotency_key: &key,
                    approver: ApproverId::new(peer.principal.clone())
                        .map_err(|_| HumanOperationError::Refused)?,
                    current_sequence,
                },
                &snapshot.prepared,
                &self.approval_queue,
            )
            .map_err(|_| HumanOperationError::Unavailable)?;
        encode_decision(&decision)
    }
    fn approval_reject(
        &mut self,
        peer: &HumanPeer,
        approval_id: [u8; 32],
        held_digest: [u8; 32],
        idempotency_key: &str,
        current_sequence: u64,
    ) -> Result<HumanResponse, HumanOperationError> {
        let tenant =
            TenantId::new(peer.tenant.clone()).map_err(|_| HumanOperationError::Refused)?;
        let snapshot = self
            .approvals
            .get_scoped(&tenant, approval_id, current_sequence)
            .map_err(|_| HumanOperationError::Refused)?;
        if snapshot.prepared.disclosure.canonical_digest != held_digest {
            return Err(HumanOperationError::Refused);
        }
        let key = DecisionKey::new(idempotency_key).map_err(|_| HumanOperationError::Refused)?;
        let decision = ApprovalService::new(&self.approvals, &self.budgets, &self.approval_expiry)
            .reject(DecisionRequest {
                tenant: &tenant,
                approval_id,
                idempotency_key: &key,
                approver: ApproverId::new(peer.principal.clone())
                    .map_err(|_| HumanOperationError::Refused)?,
                current_sequence,
            })
            .map_err(|_| HumanOperationError::Unavailable)?;
        encode_decision(&decision)
    }
    fn balance(&mut self, peer: &HumanPeer) -> Result<HumanResponse, HumanOperationError> {
        self.lock_operations()?.balance(peer)
    }
    fn head(&self, peer: &HumanPeer) -> Result<HumanResponse, HumanOperationError> {
        self.lock_operations()?.head(peer)
    }
    fn evidence(
        &mut self,
        peer: &HumanPeer,
        idempotency_key: [u8; 32],
        expected_activity_id: [u8; 32],
    ) -> Result<HumanResponse, HumanOperationError> {
        self.lock_operations()?
            .evidence(peer, idempotency_key, expected_activity_id)
    }
    fn account_sequence(
        &mut self,
        peer: &HumanPeer,
        actor: &str,
        authority: &str,
    ) -> Result<HumanResponse, HumanOperationError> {
        self.lock_operations()?
            .account_sequence(peer, actor, authority)
    }
    fn identity_resolve(
        &mut self,
        peer: &HumanPeer,
        agent: &str,
    ) -> Result<HumanResponse, HumanOperationError> {
        let did = Did::new(agent.as_bytes()).map_err(|_| HumanOperationError::Refused)?;
        let identity = self
            .lock_operations()?
            .authority
            .core_identity(peer, &did)
            .map_err(map_identity_operation)?;
        encode_identity(&identity)
    }
    fn lease_map(
        &mut self,
        peer: &HumanPeer,
        not_before_unix_ms: u64,
        not_after_unix_ms: u64,
    ) -> Result<HumanResponse, HumanOperationError> {
        let attestation = self.lock_operations()?.authority.lease_attestation(peer)?;
        let (not_before_sequence, expiry_sequence) =
            attestation.map(not_before_unix_ms, not_after_unix_ms)?;
        let mut out = Encoder::new();
        out.u64(not_before_sequence);
        out.u64(expiry_sequence);
        out.u64(attestation.observed_head_sequence);
        out.bytes(&attestation.canonical_bytes)?;
        out.finish()
    }
    fn owner_validate(
        &mut self,
        peer: &HumanPeer,
        request: HumanOwnerInstall,
    ) -> Result<HumanResponse, HumanOperationError> {
        let validated = self.validate_owner(peer, &request)?;
        encode_owner_validation(&validated)
    }
    fn owner_install(
        &mut self,
        peer: &HumanPeer,
        request: MutationEnvelope<HumanOwnerInstall>,
    ) -> Result<HumanResponse, HumanOperationError> {
        if owner_digest(&request.operation) != request.body_digest {
            return Err(HumanOperationError::Refused);
        }
        let tenant =
            TenantId::new(peer.tenant.clone()).map_err(|_| HumanOperationError::Refused)?;
        let action = owner_action_key(&tenant, request.key)?;
        let replay = {
            let mut store = self
                .store
                .lock()
                .map_err(|_| HumanOperationError::Unavailable)?;
            match store.get(&action) {
                Some(value) => Some(
                    if pending_owner_action(value.bytes(), request.body_digest) {
                        OwnerAction::Pending
                    } else {
                        decode_owner_action(value.bytes(), request.body_digest)?
                    },
                ),
                None => {
                    let mut pending = Vec::with_capacity(34);
                    pending.push(1);
                    pending.push(0);
                    pending.extend_from_slice(&request.body_digest);
                    store
                        .put_local(action.clone(), pending)
                        .map_err(|_| HumanOperationError::Unavailable)?;
                    None
                }
            }
        };
        let (validated, allocated_token_id) = match replay {
            Some(OwnerAction::Completed(response)) => return Ok(response),
            Some(OwnerAction::Validated(value, Some(token_id))) => (value, token_id),
            Some(OwnerAction::Validated(value, None)) => match self.persist_owner_validation(
                &tenant,
                &action,
                request.body_digest,
                request.operation.session_id,
                value,
            )? {
                OwnerAction::Completed(response) => return Ok(response),
                OwnerAction::Validated(value, Some(token_id)) => (value, token_id),
                _ => return Err(HumanOperationError::Unavailable),
            },
            Some(OwnerAction::Pending) | None => {
                let value = self.validate_owner(peer, &request.operation)?;
                match self.persist_owner_validation(
                    &tenant,
                    &action,
                    request.body_digest,
                    request.operation.session_id,
                    value,
                )? {
                    OwnerAction::Completed(response) => return Ok(response),
                    OwnerAction::Validated(value, Some(token_id)) => (value, token_id),
                    _ => return Err(HumanOperationError::Unavailable),
                }
            }
        };
        let did = Did::new(request.operation.agent.as_bytes())
            .map_err(|_| HumanOperationError::Refused)?;
        let installed_agent = request.operation.agent.clone();
        let lifecycle = request.operation.lifecycle.clone();
        let mut resolver = FixedIdentity(validated.0.clone());
        let mut sessions = self
            .sessions
            .write()
            .map_err(|_| HumanOperationError::Unavailable)?;
        let mut store = self
            .store
            .lock()
            .map_err(|_| HumanOperationError::Unavailable)?;
        let identity = identity::register(&mut store, tenant.clone(), did.clone(), &mut resolver)
            .map_err(map_identity_operation)?;
        let issued = validate_issued_session(
            &request.operation.registration_payload,
            request.operation.grantor,
            request.operation.session_public_key,
            request.operation.grant_not_before,
            request.operation.grant_expires_at,
            request.operation.grant_revocation_sequence,
            &request.operation.permitted_activity_types,
        )
        .map_err(|_| HumanOperationError::Refused)?;
        match self.session_keys.provision(
            request.operation.authority_id,
            request
                .operation
                .session_secret
                .as_ref()
                .ok_or(HumanOperationError::Refused)?
                .as_seed(),
            issued.clone(),
        ) {
            Ok(()) => {}
            Err(crate::session_keys::SessionKeyRegistryError::Exists) => {
                let signer = self
                    .session_keys
                    .load(request.operation.authority_id, issued)
                    .map_err(|_| HumanOperationError::Refused)?;
                drop(signer)
            }
            Err(_) => return Err(HumanOperationError::Unavailable),
        }
        let open_request = OpenRequest {
            session_id: SessionId(request.operation.session_id),
            token_id: allocated_token_id,
            tenant: tenant.clone(),
            agent: did,
            authority: owner_authority(&request.operation)?,
            permitted_activity_types: request
                .operation
                .permitted_activity_types
                .iter()
                .copied()
                .collect(),
            scopes: request.operation.scopes.iter().cloned().collect(),
            expiry_sequence: validated.1,
            opening_client: request.operation.opening_client,
            policy_version: request.operation.policy_version,
        };
        if let Some(existing) = sessions.get(&tenant, open_request.session_id) {
            if !existing.open || existing.request != open_request {
                return Err(HumanOperationError::Refused);
            }
        } else {
            session::open(
                &mut store,
                &mut sessions,
                &identity,
                open_request,
                validated.2,
            )
            .map_err(|_| HumanOperationError::Refused)?;
        }
        let installed_generation = sessions
            .generation(&tenant, SessionId(request.operation.session_id))
            .ok_or(HumanOperationError::Unavailable)?;
        if let Some(seed) = lifecycle.as_ref() {
            managed_agent::publish_creation(
                &mut store,
                &tenant,
                ManagedAgent::from_creation(
                    seed,
                    &installed_agent,
                    request.operation.session_id,
                    allocated_token_id,
                    installed_generation,
                )?,
            )?;
        }
        let mut out = Encoder::new();
        out.fixed(&allocated_token_id);
        out.fixed(&request.operation.session_id);
        out.u64(installed_generation);
        out.u64(validated.1);
        out.u64(validated.2);
        let response = out.finish()?;
        let mut completed = Vec::with_capacity(34 + response.bytes().len());
        completed.push(1);
        completed.push(2);
        completed.extend_from_slice(&request.body_digest);
        completed.extend_from_slice(response.bytes());
        store
            .put_local(action, completed)
            .map_err(|_| HumanOperationError::Unavailable)?;
        Ok(response)
    }
    fn agent_list(
        &mut self,
        peer: &HumanPeer,
        cursor: Option<[u8; 32]>,
        limit: u8,
    ) -> Result<HumanResponse, HumanOperationError> {
        let tenant =
            TenantId::new(peer.tenant.clone()).map_err(|_| HumanOperationError::Refused)?;
        let store = self
            .store
            .lock()
            .map_err(|_| HumanOperationError::Unavailable)?;
        managed_agent::list(&store, &tenant, cursor, limit)
    }
    fn agent_get(
        &mut self,
        peer: &HumanPeer,
        agent_id: &str,
    ) -> Result<HumanResponse, HumanOperationError> {
        let tenant =
            TenantId::new(peer.tenant.clone()).map_err(|_| HumanOperationError::Refused)?;
        let store = self
            .store
            .lock()
            .map_err(|_| HumanOperationError::Unavailable)?;
        managed_agent::get(&store, &tenant, agent_id)
    }
    fn agent_control(
        &mut self,
        peer: &HumanPeer,
        agent_id: &str,
        resume: bool,
        session_observation: [u8; 32],
        evidence: HumanFinalizationEvidence,
    ) -> Result<HumanResponse, HumanOperationError> {
        let tenant =
            TenantId::new(peer.tenant.clone()).map_err(|_| HumanOperationError::Refused)?;
        let mut store = self
            .store
            .lock()
            .map_err(|_| HumanOperationError::Unavailable)?;
        managed_agent::finalize_control(
            &mut store,
            &tenant,
            agent_id,
            resume,
            session_observation,
            evidence.into(),
        )
    }
    fn agent_limit(
        &mut self,
        peer: &HumanPeer,
        agent_id: &str,
        monthly_limit: u128,
        currency: &str,
        replacement_budget_id: [u8; 32],
        evidence: HumanFinalizationEvidence,
    ) -> Result<HumanResponse, HumanOperationError> {
        let tenant =
            TenantId::new(peer.tenant.clone()).map_err(|_| HumanOperationError::Refused)?;
        let mut store = self
            .store
            .lock()
            .map_err(|_| HumanOperationError::Unavailable)?;
        managed_agent::finalize_limit(
            &mut store,
            &tenant,
            agent_id,
            monthly_limit,
            currency,
            replacement_budget_id,
            evidence.into(),
        )
    }
    fn agent_journey(
        &mut self,
        peer: &HumanPeer,
        agent_id: &str,
        kind: crate::human::HumanAgentJourneyKind,
        pre_observation: [u8; 32],
        post_observation: [u8; 32],
        evidence: HumanFinalizationEvidence,
    ) -> Result<HumanResponse, HumanOperationError> {
        let tenant =
            TenantId::new(peer.tenant.clone()).map_err(|_| HumanOperationError::Refused)?;
        let (kind, amount, currency, delay, ready) = match kind {
            crate::human::HumanAgentJourneyKind::Reclaim { amount, currency } => {
                (0, amount, currency, 0, 0)
            }
            crate::human::HumanAgentJourneyKind::Rotate {
                challenge_delay_seconds,
                ready_at,
            } => (1, 0, String::new(), challenge_delay_seconds, ready_at),
            crate::human::HumanAgentJourneyKind::Recover {
                challenge_delay_seconds,
                ready_at,
            } => (2, 0, String::new(), challenge_delay_seconds, ready_at),
        };
        let evidence: managed_agent::FinalizationEvidence = evidence.into();
        if matches!(kind, 1 | 2) {
            let finalized = {
                let store = self
                    .store
                    .lock()
                    .map_err(|_| HumanOperationError::Unavailable)?;
                managed_agent::validate_authority_revocation(
                    &store,
                    &tenant,
                    agent_id,
                    kind,
                    delay,
                    ready,
                    pre_observation,
                    post_observation,
                    evidence,
                )?
            };
            self.session_control
                .invalidate_finalized(&finalized)
                .map_err(|error| match error {
                    crate::session_control::SessionControlError::Unavailable => {
                        HumanOperationError::Unavailable
                    }
                    _ => HumanOperationError::Refused,
                })?;
        }
        let mut store = self
            .store
            .lock()
            .map_err(|_| HumanOperationError::Unavailable)?;
        managed_agent::finalize_journey(
            &mut store,
            &tenant,
            agent_id,
            kind,
            amount,
            &currency,
            delay,
            ready,
            pre_observation,
            post_observation,
            evidence,
        )
    }
    fn agent_archive(
        &mut self,
        peer: &HumanPeer,
        agent_id: &str,
        confirm_name: &str,
        pre_observation: [u8; 32],
        post_observation: [u8; 32],
        session_observation: [u8; 32],
        evidence: HumanFinalizationEvidence,
    ) -> Result<HumanResponse, HumanOperationError> {
        let tenant =
            TenantId::new(peer.tenant.clone()).map_err(|_| HumanOperationError::Refused)?;
        let mut store = self
            .store
            .lock()
            .map_err(|_| HumanOperationError::Unavailable)?;
        managed_agent::finalize_archive(
            &mut store,
            &tenant,
            agent_id,
            confirm_name,
            pre_observation,
            post_observation,
            session_observation,
            evidence.into(),
        )
    }
    fn capability_install(
        &mut self,
        peer: &HumanPeer,
        request: HumanCapabilityInstall,
    ) -> Result<HumanResponse, HumanOperationError> {
        install_capability(
            &mut self.lock_operations()?.authority,
            &self.store,
            peer,
            request,
        )
    }
    fn agent_lifecycle_publish(
        &mut self,
        peer: &HumanPeer,
        request: MutationEnvelope<crate::human::HumanAgentLifecycleSeed>,
    ) -> Result<HumanResponse, HumanOperationError> {
        let tenant =
            TenantId::new(peer.tenant.clone()).map_err(|_| HumanOperationError::Refused)?;
        if managed_agent::lifecycle_publish_digest(&request.operation)
            .map_err(|_| HumanOperationError::Refused)?
            != request.body_digest
        {
            return Err(HumanOperationError::Refused);
        }
        let record = self
            .sessions
            .read()
            .map_err(|_| HumanOperationError::Unavailable)?
            .get(&tenant, SessionId(request.key))
            .cloned()
            .ok_or(HumanOperationError::Refused)?;
        if !record.open
            || record.request.tenant != tenant
            || record.request.authority
                != ProtocolAuthority::SessionKey(request.operation.protocol_grant_id)
        {
            return Err(HumanOperationError::Refused);
        }
        let agent = ManagedAgent::from_creation(
            &request.operation,
            std::str::from_utf8(record.request.agent.as_bytes())
                .map_err(|_| HumanOperationError::Refused)?,
            request.key,
            record.request.token_id,
            record.generation,
        )?;
        let companion = lifecycle_action_key(&tenant, request.key)?;
        let mut completed = Vec::with_capacity(34);
        completed.push(2);
        completed.extend(request.body_digest);
        let mut store = self
            .store
            .lock()
            .map_err(|_| HumanOperationError::Unavailable)?;
        if let Some(existing) = store.get(&companion) {
            if existing.class() != StorageClass::LocalOnly || existing.bytes() != completed {
                return Err(HumanOperationError::Refused);
            }
            return HumanResponse::new(vec![1]).map_err(|_| HumanOperationError::Refused);
        }
        managed_agent::publish_creation_with_companion(
            &mut store, &tenant, agent, companion, completed,
        )?;
        HumanResponse::new(vec![1]).map_err(|_| HumanOperationError::Refused)
    }
    fn agent_context(
        &mut self,
        peer: &HumanPeer,
        agent_id: &str,
    ) -> Result<HumanResponse, HumanOperationError> {
        let tenant =
            TenantId::new(peer.tenant.clone()).map_err(|_| HumanOperationError::Refused)?;
        let store = self
            .store
            .lock()
            .map_err(|_| HumanOperationError::Unavailable)?;
        managed_agent::context(&store, &tenant, agent_id)
    }
    fn agent_budget_state(
        &mut self,
        peer: &HumanPeer,
        active_budget_id: [u8; 32],
    ) -> Result<HumanResponse, HumanOperationError> {
        if active_budget_id == [0; 32] {
            return Err(HumanOperationError::Refused);
        }
        let state = self
            .lock_operations()?
            .authority
            .budget_state(peer, active_budget_id)?;
        let mut out = Encoder::new();
        out.fixed(&active_budget_id);
        out.u64(state.revocation_sequence);
        out.u64(state.observed_head_sequence);
        out.u8(state.verification);
        out.fixed(&state.evidence_digest);
        out.fixed(&state.receipt_digest);
        out.fixed(&state.checkpoint_digest);
        out.u64(state.age_sequences);
        out.u64(state.maximum_age_sequences);
        out.u128(state.remaining);
        out.fixed(&state.asset);
        let response = out.finish()?;
        let tenant =
            TenantId::new(peer.tenant.clone()).map_err(|_| HumanOperationError::Refused)?;
        let mut store = self
            .store
            .lock()
            .map_err(|_| HumanOperationError::Unavailable)?;
        managed_agent::record_observation(
            &mut store,
            &tenant,
            1,
            state.evidence_digest,
            response.bytes().to_vec(),
        )?;
        Ok(response)
    }
    fn agent_key_policy(
        &mut self,
        peer: &HumanPeer,
        agent_did: &str,
        recovery: bool,
    ) -> Result<HumanResponse, HumanOperationError> {
        let did = Did::new(agent_did.as_bytes()).map_err(|_| HumanOperationError::Refused)?;
        let state = self
            .lock_operations()?
            .authority
            .key_rotation_policy(peer, &did, recovery)?;
        let mut out = Encoder::new();
        out.text(agent_did)?;
        out.u8(u8::from(recovery));
        out.u64(state.policy_revision);
        out.u64(state.required_delay_seconds);
        out.u64(state.maximum_delay_seconds);
        out.u64(state.effective_sequence);
        out.u64(state.observed_head_sequence);
        out.u8(state.verification);
        out.fixed(&state.evidence_digest);
        out.fixed(&state.checkpoint_digest);
        out.u64(state.age_sequences);
        out.u64(state.maximum_age_sequences);
        let response = out.finish()?;
        let tenant =
            TenantId::new(peer.tenant.clone()).map_err(|_| HumanOperationError::Refused)?;
        let mut store = self
            .store
            .lock()
            .map_err(|_| HumanOperationError::Unavailable)?;
        managed_agent::record_observation(
            &mut store,
            &tenant,
            2,
            state.evidence_digest,
            response.bytes().to_vec(),
        )?;
        Ok(response)
    }
    fn agent_session_snapshot(
        &mut self,
        peer: &HumanPeer,
        agent_id: &str,
    ) -> Result<HumanResponse, HumanOperationError> {
        let tenant =
            TenantId::new(peer.tenant.clone()).map_err(|_| HumanOperationError::Refused)?;
        let sessions = self
            .sessions
            .read()
            .map_err(|_| HumanOperationError::Unavailable)?;
        let store = self
            .store
            .lock()
            .map_err(|_| HumanOperationError::Unavailable)?;
        let (did, session_id, token, generation) =
            managed_agent::session_coordinates(&store, &tenant, agent_id)?;
        let record = sessions
            .get(&tenant, SessionId(session_id))
            .ok_or(HumanOperationError::Refused)?;
        if record.request.tenant != tenant
            || record.request.token_id != token
            || record.generation != generation
        {
            return Err(HumanOperationError::Refused);
        }
        let mut out = Encoder::new();
        out.text(agent_id)?;
        out.text(&did)?;
        out.fixed(&session_id);
        out.fixed(&token);
        out.u64(generation);
        out.u8(u8::from(record.open));
        out.u64(record.request.expiry_sequence);
        out.u64(record.sequence);
        out.finish()
    }
    fn agent_session_suspend(
        &mut self,
        peer: &HumanPeer,
        agent_id: &str,
        action_key: [u8; 32],
    ) -> Result<HumanResponse, HumanOperationError> {
        let tenant =
            TenantId::new(peer.tenant.clone()).map_err(|_| HumanOperationError::Refused)?;
        let mut sessions = self
            .sessions
            .write()
            .map_err(|_| HumanOperationError::Unavailable)?;
        let mut store = self
            .store
            .lock()
            .map_err(|_| HumanOperationError::Unavailable)?;
        let (_, session_id, _, _) = managed_agent::session_coordinates(&store, &tenant, agent_id)?;
        let grant = managed_agent::protocol_grant(&store, &tenant, agent_id)?;
        let was_open = sessions
            .get(&tenant, SessionId(session_id))
            .map(|record| record.open)
            .ok_or(HumanOperationError::Refused)?;
        if !was_open {
            return managed_agent::record_session_observation(
                &mut store, &tenant, agent_id, action_key, false, false,
            );
        }
        let (response, key, bytes) = managed_agent::prepare_session_observation(
            &store, &tenant, agent_id, action_key, false,
        )?;
        session::close_with_companion(
            &mut store,
            &mut sessions,
            &tenant,
            SessionId(session_id),
            key,
            bytes,
        )
        .map_err(|_| HumanOperationError::Unavailable)?;
        drop(store);
        drop(sessions);
        self.session_keys
            .revoke(grant)
            .map_err(|_| HumanOperationError::Unavailable)?;
        Ok(response)
    }
    fn agent_session_bind(
        &mut self,
        peer: &HumanPeer,
        agent_id: &str,
        session_id: [u8; 32],
        token_id: [u8; 32],
        action_key: [u8; 32],
    ) -> Result<HumanResponse, HumanOperationError> {
        let tenant =
            TenantId::new(peer.tenant.clone()).map_err(|_| HumanOperationError::Refused)?;
        let sessions = self
            .sessions
            .read()
            .map_err(|_| HumanOperationError::Unavailable)?;
        let mut store = self
            .store
            .lock()
            .map_err(|_| HumanOperationError::Unavailable)?;
        let (did, _, _, _) = managed_agent::session_coordinates(&store, &tenant, agent_id)?;
        let agent = Did::new(did.as_bytes()).map_err(|_| HumanOperationError::Refused)?;
        let token = sessions
            .authenticate_bearer(&tenant, SessionId(session_id), token_id)
            .map_err(|_| HumanOperationError::Refused)?;
        token
            .boundary(&sessions)
            .map_err(|_| HumanOperationError::Refused)?;
        if token.tenant() != &tenant || token.agent() != &agent {
            return Err(HumanOperationError::Refused);
        }
        let authority = match sessions
            .get(&tenant, SessionId(session_id))
            .map(|record| record.request.authority.clone())
        {
            Some(ProtocolAuthority::SessionKey(id)) => id,
            _ => return Err(HumanOperationError::Refused),
        };
        managed_agent::bind_session(
            &mut store,
            &tenant,
            agent_id,
            session_id,
            token_id,
            token.generation(),
            authority,
            action_key,
        )
    }
    fn agent_session_restrict(
        &mut self,
        peer: &HumanPeer,
        agent_id: &str,
        current_sequence: u64,
        action_key: [u8; 32],
        permitted_activity_types: Vec<u16>,
        scopes: Vec<String>,
    ) -> Result<HumanResponse, HumanOperationError> {
        let tenant =
            TenantId::new(peer.tenant.clone()).map_err(|_| HumanOperationError::Refused)?;
        self.session_control
            .restrict_managed_agent(
                &tenant,
                agent_id,
                scopes.into_iter().collect(),
                permitted_activity_types.into_iter().collect(),
                current_sequence,
                action_key,
            )
            .map(|(_, _, response)| response)
            .map_err(|error| match error {
                crate::session_control::SessionControlError::Unavailable => {
                    HumanOperationError::Unavailable
                }
                _ => HumanOperationError::Refused,
            })
    }
}

struct FixedIdentity(CoreIdentity);
impl IdentityResolver for FixedIdentity {
    fn resolve(&mut self, _did: &Did) -> Result<Option<CoreIdentity>, IdentityError> {
        Ok(Some(self.0.clone()))
    }
}
enum OwnerAction {
    Pending,
    Validated((CoreIdentity, u64, u64), Option<[u8; 32]>),
    Completed(HumanResponse),
}
fn pending_owner_action(bytes: &[u8], body: [u8; 32]) -> bool {
    bytes.len() == 34 && bytes[0] == 1 && bytes[1] == 0 && bytes[2..] == body
}
fn owner_action_key(
    tenant: &TenantId,
    action: [u8; 32],
) -> Result<crate::store::TenantKey, HumanOperationError> {
    let mut id = b"human-owner-action-v1:".to_vec();
    id.extend_from_slice(&action);
    key(tenant.clone(), ObjectKind::Idempotency, id).map_err(|_| HumanOperationError::Refused)
}
fn lifecycle_action_key(
    tenant: &TenantId,
    action: [u8; 32],
) -> Result<crate::store::TenantKey, HumanOperationError> {
    let mut id = b"human-lifecycle-publish-v1:".to_vec();
    id.extend_from_slice(&action);
    key(tenant.clone(), ObjectKind::Idempotency, id).map_err(|_| HumanOperationError::Refused)
}
fn encode_owner_validated(
    body: [u8; 32],
    value: &(CoreIdentity, u64, u64),
    token_id: [u8; 32],
) -> Result<Vec<u8>, HumanOperationError> {
    if token_id == [0; 32] {
        return Err(HumanOperationError::Refused);
    }
    let mut out = Encoder::new();
    out.u8(1);
    out.u8(1);
    out.fixed(&body);
    out.u64(value.0.head_sequence);
    out.u64(value.0.revocation_sequence);
    out.u8(value.0.verification_level.wire_rank());
    out.u8(u8::from(value.0.frozen));
    out.u16(value.0.authorities.len())?;
    for authority in &value.0.authorities {
        match authority {
            ProtocolAuthority::PrimaryKey(id) => {
                out.u8(1);
                out.fixed(id)
            }
            ProtocolAuthority::SessionKey(id) => {
                out.u8(2);
                out.fixed(id)
            }
            ProtocolAuthority::CapabilityGrant(id) => {
                out.u8(3);
                out.fixed(id)
            }
        }
    }
    out.bytes(&value.0.canonical_bytes)?;
    out.u64(value.1);
    out.u64(value.2);
    out.fixed(&token_id);
    Ok(out.0)
}
fn decode_owner_action(bytes: &[u8], body: [u8; 32]) -> Result<OwnerAction, HumanOperationError> {
    let mut input = OwnerInput { bytes, at: 0 };
    if input.u8()? != 1 {
        return Err(HumanOperationError::Refused);
    }
    let state = input.u8()?;
    if input.fixed() != Some(body) {
        return Err(HumanOperationError::Refused);
    }
    if state == 2 {
        return Ok(OwnerAction::Completed(
            HumanResponse::new(input.remaining().to_vec())
                .map_err(|_| HumanOperationError::Refused)?,
        ));
    }
    if state != 1 {
        return Err(HumanOperationError::Refused);
    }
    let head_sequence = input.u64()?;
    let revocation_sequence = input.u64()?;
    let verification_level = rank(input.u8()?)?;
    let frozen = match input.u8()? {
        0 => false,
        1 => true,
        _ => return Err(HumanOperationError::Refused),
    };
    let count = usize::from(input.u16()?);
    if count == 0 || count > 256 {
        return Err(HumanOperationError::Refused);
    }
    let mut authorities = Vec::with_capacity(count);
    for _ in 0..count {
        let tag = input.u8()?;
        let id = input.fixed().ok_or(HumanOperationError::Refused)?;
        authorities.push(match tag {
            1 => ProtocolAuthority::PrimaryKey(id),
            2 => ProtocolAuthority::SessionKey(id),
            3 => ProtocolAuthority::CapabilityGrant(id),
            _ => return Err(HumanOperationError::Refused),
        })
    }
    let canonical_bytes = input.bytes()?;
    let expiry = input.u64()?;
    let observed = input.u64()?;
    let allocated_token_id = if input.remaining().is_empty() {
        None
    } else {
        let token_id = input.fixed().ok_or(HumanOperationError::Refused)?;
        if token_id == [0; 32] || !input.remaining().is_empty() {
            return Err(HumanOperationError::Refused);
        }
        Some(token_id)
    };
    Ok(OwnerAction::Validated(
        (
            CoreIdentity {
                canonical_bytes,
                head_sequence,
                revocation_sequence,
                verification_level,
                frozen,
                authorities,
            },
            expiry,
            observed,
        ),
        allocated_token_id,
    ))
}
struct OwnerInput<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl OwnerInput<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], HumanOperationError> {
        let end = self.at.checked_add(n).ok_or(HumanOperationError::Refused)?;
        let out = self
            .bytes
            .get(self.at..end)
            .ok_or(HumanOperationError::Refused)?;
        self.at = end;
        Ok(out)
    }
    fn u8(&mut self) -> Result<u8, HumanOperationError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, HumanOperationError> {
        Ok(u16::from_be_bytes(
            self.take(2)?
                .try_into()
                .map_err(|_| HumanOperationError::Refused)?,
        ))
    }
    fn u64(&mut self) -> Result<u64, HumanOperationError> {
        Ok(u64::from_be_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| HumanOperationError::Refused)?,
        ))
    }
    fn fixed(&mut self) -> Option<[u8; 32]> {
        self.take(32).ok()?.try_into().ok()
    }
    fn bytes(&mut self) -> Result<Vec<u8>, HumanOperationError> {
        let n = u32::from_be_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| HumanOperationError::Refused)?,
        ) as usize;
        Ok(self.take(n)?.to_vec())
    }
    fn remaining(&self) -> &[u8] {
        &self.bytes[self.at..]
    }
}
fn rank(value: u8) -> Result<VerificationLevel, HumanOperationError> {
    match value {
        1 => Ok(VerificationLevel::SEQUENCER_SIGNED),
        2 => Ok(VerificationLevel::BATCH_INCLUDED),
        3 => Ok(VerificationLevel::STATE_PROVEN),
        4 => Ok(VerificationLevel::CHECKPOINT_FINALISED),
        5 => Ok(VerificationLevel::SETTLEMENT_ANCHORED),
        _ => Err(HumanOperationError::Refused),
    }
}

impl<A: HumanAuthorityBoundary> UnifiedAgentOwner<A> {
    fn persist_owner_validation(
        &self,
        tenant: &TenantId,
        action: &TenantKey,
        body_digest: [u8; 32],
        session_id: [u8; 32],
        validated: (CoreIdentity, u64, u64),
    ) -> Result<OwnerAction, HumanOperationError> {
        let sessions = self
            .sessions
            .read()
            .map_err(|_| HumanOperationError::Unavailable)?;
        let mut store = self
            .store
            .lock()
            .map_err(|_| HumanOperationError::Unavailable)?;
        let current = store.get(action).ok_or(HumanOperationError::Unavailable)?;
        let decoded = if pending_owner_action(current.bytes(), body_digest) {
            OwnerAction::Pending
        } else {
            decode_owner_action(current.bytes(), body_digest)?
        };
        match decoded {
            OwnerAction::Completed(response) => return Ok(OwnerAction::Completed(response)),
            OwnerAction::Validated(value, Some(token_id)) => {
                return Ok(OwnerAction::Validated(value, Some(token_id)))
            }
            OwnerAction::Pending | OwnerAction::Validated(_, None) => {}
        }
        let token_id = match sessions.get(tenant, SessionId(session_id)) {
            Some(existing) if existing.request.token_id != [0; 32] => existing.request.token_id,
            Some(_) => return Err(HumanOperationError::Refused),
            None => {
                let mut generated = None;
                for _ in 0..8 {
                    let mut token_id = [0_u8; 32];
                    getrandom::fill(&mut token_id).map_err(|_| HumanOperationError::Unavailable)?;
                    if token_id != [0; 32] {
                        generated = Some(token_id);
                        break;
                    }
                }
                generated.ok_or(HumanOperationError::Unavailable)?
            }
        };
        store
            .put_local(
                action.clone(),
                encode_owner_validated(body_digest, &validated, token_id)?,
            )
            .map_err(|_| HumanOperationError::Unavailable)?;
        Ok(OwnerAction::Validated(validated, Some(token_id)))
    }

    fn validate_owner(
        &mut self,
        peer: &HumanPeer,
        request: &HumanOwnerInstall,
    ) -> Result<(CoreIdentity, u64, u64), HumanOperationError> {
        if request.authority_kind != 2
            || request.session_id == [0; 32]
            || request.token_id != [0; 32]
            || request.session_public_key == [0; 32]
            || request.grantor == [0; 32]
            || request.registration_payload.is_empty()
            || request.registration_payload.len() > 1024
            || request.grant_not_before == 0
            || request.grant_expires_at <= request.grant_not_before
            || request.grant_revocation_sequence == 0
            || request.session_secret.is_none()
            || request.permitted_activity_types.is_empty()
            || request.scopes.is_empty()
            || request.opening_client.is_empty()
            || request.policy_version.is_empty()
        {
            return Err(HumanOperationError::Refused);
        }
        let (authenticated_account, _, _, _, account_age, maximum_account_age, _) =
            self.lock_operations()?.authority.balance_context(peer)?;
        if request.grantor != authenticated_account
            || maximum_account_age == 0
            || account_age > maximum_account_age
        {
            return Err(HumanOperationError::Refused);
        }
        if request
            .lifecycle
            .as_ref()
            .is_some_and(|lifecycle| lifecycle.protocol_grant_id != request.authority_id)
        {
            return Err(HumanOperationError::Refused);
        }
        let issued = validate_issued_session(
            &request.registration_payload,
            request.grantor,
            request.session_public_key,
            request.grant_not_before,
            request.grant_expires_at,
            request.grant_revocation_sequence,
            &request.permitted_activity_types,
        )
        .map_err(|_| HumanOperationError::Refused)?;
        if issued.grant_id != request.authority_id {
            return Err(HumanOperationError::Refused);
        }
        let provisioned = ProvisionedSessionKey::from_seed(
            request
                .session_secret
                .as_ref()
                .ok_or(HumanOperationError::Refused)?
                .as_seed(),
            issued,
        )
        .map_err(|_| HumanOperationError::Refused)?;
        drop(provisioned);
        let did = Did::new(request.agent.as_bytes()).map_err(|_| HumanOperationError::Refused)?;
        let identity = self
            .lock_operations()?
            .authority
            .core_identity(peer, &did)
            .map_err(map_identity_operation)?;
        if identity.frozen
            || identity.head_sequence == 0
            || identity.revocation_sequence != request.grant_revocation_sequence
            || identity.canonical_bytes.is_empty()
            || identity.verification_level < VerificationLevel::CHECKPOINT_FINALISED
            || !identity.authorities.contains(&owner_authority(request)?)
        {
            return Err(HumanOperationError::Refused);
        }
        let attestation = self.lock_operations()?.authority.lease_attestation(peer)?;
        let (not_before, expiry) = attestation.map(
            request.lease_not_before_unix_ms,
            request.lease_not_after_unix_ms,
        )?;
        if not_before != request.grant_not_before || expiry != request.grant_expires_at {
            return Err(HumanOperationError::Refused);
        }
        Ok((identity, expiry, attestation.observed_head_sequence))
    }
}

fn install_capability<A: HumanAuthorityBoundary>(
    authority: &mut A,
    shared_store: &Arc<Mutex<Store>>,
    peer: &HumanPeer,
    request: HumanCapabilityInstall,
) -> Result<HumanResponse, HumanOperationError> {
    if request.action_key == [0; 32]
        || request.capability_id == [0; 32]
        || request.authority_id == [0; 32]
        || request.activity_types.is_empty()
        || request.counterparties.is_empty()
        || request.assets.is_empty()
        || request.purposes.is_empty()
        || request.amount_ceiling == 0
        || request.rate_maximum_uses == 0
        || request.rate_window_sequences == 0
        || request.expiry_sequence == 0
    {
        return Err(HumanOperationError::Refused);
    }
    let tenant = TenantId::new(peer.tenant.clone()).map_err(|_| HumanOperationError::Refused)?;
    let did = Did::new(request.agent.as_bytes()).map_err(|_| HumanOperationError::Refused)?;
    let replay_key = capability_action_key(&tenant, request.action_key)?;
    let request_digest = capability_request_digest(&request);
    {
        let store = shared_store
            .lock()
            .map_err(|_| HumanOperationError::Unavailable)?;
        if let Some(existing) = store.get(&replay_key) {
            if existing.class() != StorageClass::LocalOnly {
                return Err(HumanOperationError::Refused);
            }
            return decode_capability_action(existing.bytes(), request_digest);
        }
    }
    {
        let mut store = shared_store
            .lock()
            .map_err(|_| HumanOperationError::Unavailable)?;
        let mut pending = Vec::with_capacity(34);
        pending.push(1);
        pending.extend_from_slice(&request_digest);
        pending.push(0);
        store
            .put_local(replay_key.clone(), pending)
            .map_err(|_| HumanOperationError::Unavailable)?;
    }
    let identity = authority
        .core_identity(peer, &did)
        .map_err(map_identity_operation)?;
    if identity.frozen
        || identity.verification_level == VerificationLevel::UNVERIFIED
        || !identity
            .authorities
            .contains(&ProtocolAuthority::PrimaryKey(request.authority_id))
    {
        return Err(HumanOperationError::Refused);
    }
    let observed = authority.capability_scope(
        peer,
        &did,
        request.authority_id,
        request.action_key,
        request.capability_id,
    )?;
    if observed.observed_sequence == 0
        || observed.verification < 4
        || observed.verification > 5
        || observed.evidence_digest == [0; 32]
        || request.expiry_sequence <= observed.observed_sequence
    {
        return Err(HumanOperationError::Refused);
    }
    let capability = Capability::new(
        CapabilityId(request.capability_id),
        tenant.clone(),
        CapabilityDimensions {
            activity_types: request.activity_types.iter().copied().collect(),
            counterparties: request.counterparties.iter().copied().collect(),
            assets: request.assets.iter().copied().collect(),
            amount_ceiling: request.amount_ceiling,
            rate_ceiling: RateCeiling {
                maximum_uses: request.rate_maximum_uses,
                window_sequences: request.rate_window_sequences,
            },
            purposes: request.purposes.iter().cloned().collect(),
            expiry_sequence: request.expiry_sequence,
        },
    )
    .map_err(|_| HumanOperationError::Refused)?;
    assert_narrowing(
        &capability,
        ProtocolAuthority::PrimaryKey(request.authority_id),
        &observed.scope,
    )
    .map_err(|_| HumanOperationError::Refused)?;
    let expected = agent_evidence_digest(
        request.action_key,
        request.capability_id,
        observed.observed_sequence,
        observed.verification,
    );
    if observed.evidence_digest != expected {
        return Err(HumanOperationError::Refused);
    }
    let mut store = shared_store
        .lock()
        .map_err(|_| HumanOperationError::Unavailable)?;
    let existing_capability =
        Capability::restore(&store, tenant.clone(), CapabilityId(request.capability_id))
            .map_err(|_| HumanOperationError::Unavailable)?;
    if let Some(existing) = existing_capability.as_ref() {
        if *existing != capability {
            return Err(HumanOperationError::Refused);
        }
    }
    let mut out = Encoder::new();
    out.fixed(&request.capability_id);
    out.u64(observed.observed_sequence);
    out.u8(observed.verification);
    out.fixed(&observed.evidence_digest);
    let response = out.finish()?;
    let mut completed = Vec::with_capacity(34 + response.bytes().len());
    completed.push(1);
    completed.extend_from_slice(&request_digest);
    completed.push(1);
    completed.extend_from_slice(response.bytes());
    if existing_capability.is_some() {
        store
            .put_local(replay_key, completed)
            .map_err(|_| HumanOperationError::Unavailable)?;
    } else {
        let capability_key = key(
            tenant,
            ObjectKind::Capability,
            request.capability_id.to_vec(),
        )
        .map_err(|_| HumanOperationError::Refused)?;
        let capability_bytes =
            crate::capability::encode(&capability).map_err(|_| HumanOperationError::Refused)?;
        store
            .update_local_with_companion(replay_key, completed, capability_key, capability_bytes)
            .map_err(|_| HumanOperationError::Unavailable)?;
    }
    Ok(response)
}

fn capability_action_key(
    tenant: &TenantId,
    action_key: [u8; 32],
) -> Result<crate::store::TenantKey, HumanOperationError> {
    let mut id = b"human-capability-action-v1:".to_vec();
    id.extend_from_slice(&action_key);
    key(tenant.clone(), ObjectKind::Idempotency, id).map_err(|_| HumanOperationError::Refused)
}

fn capability_request_digest(request: &HumanCapabilityInstall) -> [u8; 32] {
    fn field(digest: &mut Sha256, value: &[u8]) {
        digest.update(u32::try_from(value.len()).unwrap_or(u32::MAX).to_be_bytes());
        digest.update(value);
    }
    let mut digest = Sha256::new();
    digest.update(b"layerx-agentd/human-capability-action/v1\0");
    digest.update(request.action_key);
    field(&mut digest, request.agent.as_bytes());
    digest.update(request.authority_id);
    digest.update(request.capability_id);
    digest.update(
        u32::try_from(request.activity_types.len())
            .unwrap_or(u32::MAX)
            .to_be_bytes(),
    );
    for value in &request.activity_types {
        digest.update(value.to_be_bytes());
    }
    digest.update(
        u32::try_from(request.counterparties.len())
            .unwrap_or(u32::MAX)
            .to_be_bytes(),
    );
    for value in &request.counterparties {
        digest.update(value);
    }
    digest.update(
        u32::try_from(request.assets.len())
            .unwrap_or(u32::MAX)
            .to_be_bytes(),
    );
    for value in &request.assets {
        digest.update(value);
    }
    digest.update(request.amount_ceiling.to_be_bytes());
    digest.update(request.rate_maximum_uses.to_be_bytes());
    digest.update(request.rate_window_sequences.to_be_bytes());
    digest.update(
        u32::try_from(request.purposes.len())
            .unwrap_or(u32::MAX)
            .to_be_bytes(),
    );
    for value in &request.purposes {
        field(&mut digest, value.as_bytes());
    }
    digest.update(request.expiry_sequence.to_be_bytes());
    digest.finalize().into()
}

fn decode_capability_action(
    bytes: &[u8],
    expected: [u8; 32],
) -> Result<HumanResponse, HumanOperationError> {
    if bytes.len() < 34 || bytes[0] != 1 || bytes[1..33] != expected {
        return Err(HumanOperationError::Refused);
    }
    match bytes[33] {
        0 => Err(HumanOperationError::Unavailable),
        1 => HumanResponse::new(bytes[34..].to_vec()).map_err(|_| HumanOperationError::Refused),
        _ => Err(HumanOperationError::Refused),
    }
}

fn agent_evidence_digest(
    action_key: [u8; 32],
    object_id: [u8; 32],
    observed_sequence: u64,
    verification: u8,
) -> [u8; 32] {
    let mut digest = Sha256::new();
    let stage = [5_u8];
    let sequence = observed_sequence.to_be_bytes();
    let rank = [verification];
    for part in [
        b"layerx-human/agent-create/agent-evidence/v1".as_slice(),
        stage.as_slice(),
        action_key.as_slice(),
        object_id.as_slice(),
        sequence.as_slice(),
        rank.as_slice(),
    ] {
        digest.update(u32::try_from(part.len()).unwrap_or(u32::MAX).to_be_bytes());
        digest.update(part);
    }
    digest.finalize().into()
}

impl<A: HumanAuthorityBoundary> ProductionHumanOperations<A> {
    pub fn new(
        authority: A,
        node: Client,
        store: Arc<Mutex<Store>>,
        peers: &BTreeMap<u32, (String, String)>,
        maximum_payload_bytes: usize,
        timestamp_span: u64,
    ) -> Result<Self, HumanOperationError> {
        if maximum_payload_bytes == 0 || timestamp_span == 0 {
            return Err(HumanOperationError::Refused);
        }
        let mut outbox = Outbox::default();
        let mut submissions = BTreeMap::new();
        let mut tenant_principals = BTreeMap::<String, String>::new();
        for (principal, tenant) in peers.values() {
            if tenant_principals
                .insert(tenant.clone(), principal.clone())
                .is_some_and(|previous| previous != *principal)
            {
                return Err(HumanOperationError::Refused);
            }
        }
        let durable = store.lock().map_err(|_| HumanOperationError::Unavailable)?;
        for (tenant, principal) in tenant_principals {
            let tenant_id =
                TenantId::new(tenant.clone()).map_err(|_| HumanOperationError::Refused)?;
            for object in durable.list_object_ids(&tenant_id, ObjectKind::Outbox) {
                if object.starts_with(b"approval-released-v1:") {
                    continue;
                }
                let id: [u8; 32] = object
                    .try_into()
                    .map_err(|_| HumanOperationError::Refused)?;
                outbox
                    .restore(&durable, tenant_id.clone(), id)
                    .map_err(|_| HumanOperationError::Refused)?;
                submissions.insert((tenant.clone(), principal.clone(), hex(&id)), id);
            }
        }
        drop(durable);
        Ok(Self {
            authority,
            node,
            store,
            outbox,
            prepared: BTreeMap::new(),
            submissions,
            maximum_payload_bytes,
            timestamp_span,
            last_verified_receipt: None,
            unified_owner_active: false,
        })
    }

    fn registry_response(registry: &ModuleRegistry) -> Result<HumanResponse, HumanOperationError> {
        let mut out = Encoder::new();
        out.u16(registry.registrations().len())?;
        for registration in registry.registrations() {
            out.u16(usize::from(registration.module() as u16))?;
            out.u16(registration.activity_types().len())?;
            for activity in registration.activity_types() {
                out.u32(activity.value());
            }
        }
        out.finish()
    }

    fn observation(status: &SubmissionStatus) -> Result<HumanResponse, HumanOperationError> {
        let mut out = Encoder::new();
        out.fixed(&status.activity_id);
        out.text(&hex(&status.submission_id))?;
        out.u8(state_code(status.state));
        if status.state == SubmissionState::Executed {
            out.text(
                &status
                    .evidence
                    .map(|value| hex(&value.receipt_ref()))
                    .ok_or(HumanOperationError::Refused)?,
            )?;
        }
        out.u8(0); // verification remains unverified until a receipt is returned
        out.u8(0); // evidence
                   // The current durable outbox schema has no transition timestamp. Do
                   // not fabricate one for the Human API; the state itself is durable.
        out.u8(0);
        out.u8(0); // no receipt
        out.finish()
    }

    fn balance(&mut self, peer: &HumanPeer) -> Result<HumanResponse, HumanOperationError> {
        let (
            account,
            asset,
            currency,
            observed_at,
            age_seconds,
            maximum_age_seconds,
            authorization,
        ) = self.authority.balance_context(peer)?;
        if maximum_age_seconds == 0 || age_seconds > maximum_age_seconds {
            return Err(HumanOperationError::Unavailable);
        }
        let identity = Sha256::digest([peer.tenant.as_bytes(), peer.principal.as_bytes()].concat());
        let correlation = u64::from_be_bytes(
            identity[..8]
                .try_into()
                .map_err(|_| HumanOperationError::Refused)?,
        );
        let balance = self
            .node
            .balance(
                account,
                asset,
                VerificationLevel::CHECKPOINT_FINALISED,
                correlation,
                authorization,
            )
            .map_err(|_| HumanOperationError::Unavailable)?;
        let freshness = balance.freshness();
        let mut out = Encoder::new();
        out.fixed(&balance.account);
        out.fixed(&balance.asset);
        out.text(&currency)?;
        out.text(&observed_at)?;
        out.u64(age_seconds);
        out.u128(balance.amount.value());
        out.u8(verification_code(balance.achieved()));
        out.u64(freshness.global_sequence);
        out.u64(freshness.batch_number);
        out.u64(freshness.observed_head_sequence);
        out.fixed(&freshness.observed_checkpoint);
        out.bytes(balance.canonical_bytes())?;
        out.bytes(balance.proof_material())?;
        out.finish()
    }

    fn head(&self, _peer: &HumanPeer) -> Result<HumanResponse, HumanOperationError> {
        let head = self.node.head();
        let mut out = Encoder::new();
        out.u64(head.chain_sequence);
        out.u64(head.sealed_batch);
        out.fixed(&head.finalised_checkpoint);
        out.finish()
    }

    fn evidence(
        &mut self,
        peer: &HumanPeer,
        idempotency_key: [u8; 32],
        expected_activity_id: [u8; 32],
    ) -> Result<HumanResponse, HumanOperationError> {
        self.receipt_by_idempotency_key(peer, idempotency_key, expected_activity_id)
    }
}

impl<A: HumanAuthorityBoundary> HumanOperations for ProductionHumanOperations<A> {
    fn registry(&self, _peer: &HumanPeer) -> Result<HumanResponse, HumanOperationError> {
        // Mutable authenticated authority access is intentionally required; the
        // listener calls prepare first in normal operation. A readiness owner
        // must probe authority before accepting the socket.
        Self::registry_response(&self.authority.registry(_peer).map_err(map_core)?)
    }

    fn account_sequence(
        &mut self,
        peer: &HumanPeer,
        actor: &str,
        authority: &str,
    ) -> Result<HumanResponse, HumanOperationError> {
        if actor.is_empty() || authority.is_empty() {
            return Err(HumanOperationError::Refused);
        }
        let actor = Did::new(actor.as_bytes()).map_err(|_| HumanOperationError::Refused)?;
        let correlation = boundary_correlation(peer, actor.as_bytes(), b"account-sequence");
        let mut boundary = ProductionCorePreparationBoundary::new(&mut self.node, correlation)
            .map_err(map_core)?;
        let state =
            crate::prepare::CorePreparationBoundary::preparation_state(&mut boundary, &actor)
                .map_err(map_core)?;
        let mut out = Encoder::new();
        out.u64(state.account_sequence);
        out.finish()
    }

    fn prepare(
        &mut self,
        peer: &HumanPeer,
        request: MutationEnvelope<HumanPrepare>,
    ) -> Result<HumanResponse, HumanOperationError> {
        if !self.unified_owner_active {
            return Err(HumanOperationError::Unavailable);
        }
        if prepare_digest(&request.operation) != request.body_digest {
            return Err(HumanOperationError::Refused);
        }
        let activity_type = ActivityType::from_u32(request.operation.activity_type)
            .map_err(|_| HumanOperationError::Refused)?;
        let actor = Did::new(request.operation.actor.as_bytes())
            .map_err(|_| HumanOperationError::Refused)?;
        let authority = Authority::owner(request.operation.authority.as_bytes())
            .map_err(|_| HumanOperationError::Refused)?;
        let timestamp =
            TimestampBound::new(request.operation.not_before, request.operation.not_after)
                .map_err(|_| HumanOperationError::Refused)?;
        let protocol_version = self.node.handshake().node().protocol_version;
        let mut boundary =
            ProductionCorePreparationBoundary::new(&mut self.node, request.request_id)
                .map_err(map_core)?;
        let prepared = prepare_activity_for_protocol(
            &mut boundary,
            PreparationDefaults {
                timestamp_span: self.timestamp_span,
                fee_limit: Amount::from_u128(request.operation.fee_limit),
                maximum_payload_bytes: self.maximum_payload_bytes,
            },
            PrepareRequest {
                actor,
                authority,
                activity_type,
                expected_account_sequence: Some(request.operation.account_sequence),
                timestamp_bound: Some(timestamp),
                fee_limit: Some(Amount::from_u128(request.operation.fee_limit)),
                idempotency_key: IdempotencyKey::new(
                    digest_from_hex(&request.operation.idempotency_key)
                        .ok_or(HumanOperationError::Refused)?,
                ),
                payload: request.operation.payload,
                declared_payload_limit: self.maximum_payload_bytes,
            },
            protocol_version,
        )
        .map_err(|_| HumanOperationError::Refused)?;
        let registry = boundary
            .last_state()
            .ok_or(HumanOperationError::Unavailable)?
            .module_registry
            .clone();
        if prepared.envelope.payload_hash() != request.operation.payload_hash {
            return Err(HumanOperationError::Refused);
        }
        let reference = hex(&Sha256::digest(&prepared.canonical_bytes));
        if self
            .prepared
            .insert(
                (
                    peer.tenant.clone(),
                    peer.principal.clone(),
                    reference.clone(),
                ),
                CachedPreparation {
                    prepared: prepared.clone(),
                    registry,
                },
            )
            .is_some()
        {
            return Err(HumanOperationError::Refused);
        }
        let mut out = Encoder::new();
        out.text(&reference)?;
        out.bytes(&prepared.canonical_bytes)?;
        out.bytes(&prepared.signing_preimage)?;
        out.u32(prepared.envelope.activity_type().value());
        out.text(
            std::str::from_utf8(prepared.envelope.actor_did().as_bytes())
                .map_err(|_| HumanOperationError::Refused)?,
        )?;
        out.text(
            std::str::from_utf8(prepared.envelope.authority().as_bytes())
                .map_err(|_| HumanOperationError::Refused)?,
        )?;
        out.u64(prepared.envelope.account_sequence());
        out.u64(prepared.envelope.timestamp_bound().not_before());
        out.u64(prepared.envelope.timestamp_bound().not_after());
        out.u128(prepared.envelope.fee_limit().value());
        out.bytes(prepared.envelope.payload().as_bytes())?;
        out.fixed(&prepared.envelope.payload_hash());
        out.fixed(&prepared.envelope.idempotency_key().bytes());
        out.finish()
    }

    fn submit_external(
        &mut self,
        peer: &HumanPeer,
        request: MutationEnvelope<HumanSubmit>,
    ) -> Result<HumanResponse, HumanOperationError> {
        if !self.unified_owner_active {
            return Err(HumanOperationError::Unavailable);
        }
        if submit_digest(&request.operation) != request.body_digest {
            return Err(HumanOperationError::Refused);
        }
        let prepared_key = (
            peer.tenant.clone(),
            peer.principal.clone(),
            request.operation.preparation_ref.clone(),
        );
        let cached = self
            .prepared
            .get(&prepared_key)
            .cloned()
            .ok_or(HumanOperationError::Refused)?;
        let prepared = cached.prepared;
        let signature: [u8; 64] = request
            .operation
            .signature
            .as_slice()
            .try_into()
            .map_err(|_| HumanOperationError::Refused)?;
        let signed = attach_external_signature(&prepared, signature)
            .map_err(|_| HumanOperationError::Refused)?;
        let verified = verify_before_submit(
            &signed,
            &prepared,
            &request.operation.signer_public_key,
            &cached.registry,
        )
        .map_err(|_| HumanOperationError::Refused)?;
        let submission_id = prepared.envelope.idempotency_key().bytes();
        let tenant =
            TenantId::new(peer.tenant.clone()).map_err(|_| HumanOperationError::Refused)?;
        let mut store = self
            .store
            .lock()
            .map_err(|_| HumanOperationError::Unavailable)?;
        self.outbox
            .enqueue(&mut store, tenant, submission_id, verified)
            .map_err(|_| HumanOperationError::Unavailable)?;
        self.prepared.remove(&prepared_key);
        self.submissions.insert(
            (
                peer.tenant.clone(),
                peer.principal.clone(),
                hex(&submission_id),
            ),
            submission_id,
        );
        self.outbox
            .transition(
                &mut store,
                submission_id,
                SubmissionState::Submitted,
                "transmission started",
                None,
            )
            .map_err(|_| HumanOperationError::Unavailable)?;
        let bytes = self
            .outbox
            .bytes_for_transmission(submission_id)
            .map_err(|_| HumanOperationError::Unavailable)?
            .to_vec();
        match self.node.submit_signed(
            &cached.registry,
            request.operation.signer_public_key,
            request.request_id,
            0,
            &bytes,
        ) {
            Ok(Submission::Acknowledged(_)) => {
                self.outbox
                    .transition(
                        &mut store,
                        submission_id,
                        SubmissionState::Acknowledged,
                        "core admission acknowledged",
                        None,
                    )
                    .map_err(|_| HumanOperationError::Unavailable)?;
            }
            Ok(Submission::Unknown(_)) => {
                self.outbox
                    .transition(
                        &mut store,
                        submission_id,
                        SubmissionState::Unknown,
                        "submission outcome indeterminate",
                        None,
                    )
                    .map_err(|_| HumanOperationError::Unavailable)?;
            }
            Err(_) => {
                self.outbox
                    .transition(
                        &mut store,
                        submission_id,
                        SubmissionState::Unknown,
                        "node submission boundary unavailable after durable dispatch",
                        None,
                    )
                    .map_err(|_| HumanOperationError::Unavailable)?;
            }
        }
        Self::observation(
            self.outbox
                .status(submission_id)
                .ok_or(HumanOperationError::Unavailable)?,
        )
    }

    fn track(
        &mut self,
        peer: &HumanPeer,
        submission_ref: &str,
    ) -> Result<HumanResponse, HumanOperationError> {
        let id = *self
            .submissions
            .get(&(
                peer.tenant.clone(),
                peer.principal.clone(),
                submission_ref.to_owned(),
            ))
            .ok_or(HumanOperationError::Refused)?;
        Self::observation(self.outbox.status(id).ok_or(HumanOperationError::Refused)?)
    }

    fn receipt_by_idempotency_key(
        &mut self,
        peer: &HumanPeer,
        idempotency_key: [u8; 32],
        expected_activity_id: [u8; 32],
    ) -> Result<HumanResponse, HumanOperationError> {
        self.last_verified_receipt = None;
        if !self
            .submissions
            .values()
            .any(|value| *value == idempotency_key)
            || !self.submissions.contains_key(&(
                peer.tenant.clone(),
                peer.principal.clone(),
                hex(&idempotency_key),
            ))
        {
            return Err(HumanOperationError::Refused);
        }
        let authority = self
            .authority
            .authorized_batch(peer, expected_activity_id)?;
        let lookup = self
            .node
            .lookup_receipt(
                ReceiptSelector::IdempotencyKey {
                    idempotency_key,
                    expected_activity_id,
                },
                u64::from_be_bytes(
                    idempotency_key[..8]
                        .try_into()
                        .map_err(|_| HumanOperationError::Refused)?,
                ),
                authority,
            )
            .map_err(|_| HumanOperationError::Unavailable)?;
        let mut out = Encoder::new();
        match lookup {
            Lookup::Absent => out.u8(0),
            Lookup::Verified(receipt) => {
                let protocol = receipt
                    .receipt()
                    .protocol()
                    .ok_or(HumanOperationError::Refused)?;
                let tenant =
                    TenantId::new(peer.tenant.clone()).map_err(|_| HumanOperationError::Refused)?;
                let mut served = {
                    let mut store = self
                        .store
                        .lock()
                        .map_err(|_| HumanOperationError::Unavailable)?;
                    crate::receipt::store_verified_if_absent(
                        &mut store,
                        tenant.clone(),
                        idempotency_key,
                        receipt.canonical_bytes(),
                        &authority,
                    )
                    .map_err(|error| match error {
                        crate::receipt::ReceiptStoreError::Missing
                        | crate::receipt::ReceiptStoreError::Store(_) => {
                            HumanOperationError::Unavailable
                        }
                        _ => HumanOperationError::Refused,
                    })?
                };
                if served.canonical_bytes != receipt.canonical_bytes()
                    || served.metadata.idempotency_key != idempotency_key
                    || served.metadata.activity_id != expected_activity_id
                    || served.metadata.activity_id != protocol.activity_id()
                    || served.metadata.global_sequence != protocol.global_sequence()
                    || served.metadata.result.code.raw() != protocol.result_code()
                    || served.metadata.verification_level < receipt.level()
                {
                    return Err(HumanOperationError::Refused);
                }
                let registry = self.authority.registry(peer).map_err(map_core)?;
                let correlation = u64::from_be_bytes(
                    idempotency_key[..8]
                        .try_into()
                        .map_err(|_| HumanOperationError::Refused)?,
                ) | 1;
                let activity_evidence = self.node.proof_bundle(
                    ProofBundleSelector::Activity(expected_activity_id),
                    correlation,
                    &registry,
                );
                let receipt_evidence = self.node.proof_bundle(
                    ProofBundleSelector::Receipt(expected_activity_id),
                    correlation
                        .checked_add(1)
                        .ok_or(HumanOperationError::Refused)?,
                    &registry,
                );
                match (activity_evidence, receipt_evidence) {
                    (Ok(activity_evidence), Ok(receipt_evidence)) => {
                        if activity_evidence.canonical_bytes()
                            != self
                                .outbox
                                .exact_signed_bytes(idempotency_key)
                                .map_err(|_| HumanOperationError::Refused)?
                            || receipt_evidence.canonical_bytes() != receipt.canonical_bytes()
                        {
                            return Err(HumanOperationError::Refused);
                        }
                        let evidence_batch = activity_evidence
                            .signed_header()
                            .batch_number()
                            .map_err(|_| HumanOperationError::Refused)?;
                        let checkpoint = match self.node.checkpoint_evidence(
                            CheckpointSelector::Batch(evidence_batch),
                            correlation
                                .checked_add(2)
                                .ok_or(HumanOperationError::Refused)?,
                        ) {
                            Ok(checkpoint) => Some(checkpoint),
                            Err(error) if evidence_unavailable(&error) => None,
                            Err(_) => return Err(HumanOperationError::Refused),
                        };
                        {
                            let mut store = self
                                .store
                                .lock()
                                .map_err(|_| HumanOperationError::Unavailable)?;
                            crate::finality::augment_verified(
                                &mut store,
                                tenant.clone(),
                                idempotency_key,
                                &activity_evidence,
                                &receipt_evidence,
                                checkpoint.as_ref(),
                            )
                            .map_err(|_| HumanOperationError::Refused)?;
                            served = crate::receipt::serve(
                                &store,
                                tenant,
                                crate::receipt::ReceiptLookupKey::Idempotency(idempotency_key),
                            )
                            .map_err(|_| HumanOperationError::Unavailable)?;
                        }
                    }
                    (Err(error), _) | (_, Err(error)) if evidence_unavailable(&error) => {}
                    (Err(_), _) | (_, Err(_)) => return Err(HumanOperationError::Refused),
                }
                self.last_verified_receipt = Some((
                    idempotency_key,
                    served.metadata.result.code.raw(),
                    served.metadata.global_sequence,
                ));
                out.u8(1);
                out.bytes(receipt.canonical_bytes())?;
                out.fixed(&authority.batch_id());
                out.fixed(&authority.asset());
                out.fixed(&authority.previous_state_root());
                out.fixed(&authority.resulting_state_root());
                out.fixed(&authority.sequencer_public_key());
                out.u8(served.metadata.verification_level.wire_rank());
            }
        }
        out.finish()
    }
    fn balance(&mut self, peer: &HumanPeer) -> Result<HumanResponse, HumanOperationError> {
        ProductionHumanOperations::balance(self, peer)
    }
    fn head(&self, peer: &HumanPeer) -> Result<HumanResponse, HumanOperationError> {
        ProductionHumanOperations::head(self, peer)
    }
    fn evidence(
        &mut self,
        peer: &HumanPeer,
        idempotency_key: [u8; 32],
        expected_activity_id: [u8; 32],
    ) -> Result<HumanResponse, HumanOperationError> {
        ProductionHumanOperations::evidence(self, peer, idempotency_key, expected_activity_id)
    }
    fn approval_list(
        &mut self,
        _: &HumanPeer,
        _: u64,
        _: Option<[u8; 32]>,
        _: u8,
    ) -> Result<HumanResponse, HumanOperationError> {
        Err(HumanOperationError::Unavailable)
    }
    fn approval_get(
        &mut self,
        _: &HumanPeer,
        _: [u8; 32],
        _: u64,
    ) -> Result<HumanResponse, HumanOperationError> {
        Err(HumanOperationError::Unavailable)
    }
    fn approval_approve(
        &mut self,
        _: &HumanPeer,
        _: [u8; 32],
        _: [u8; 32],
        _: &str,
        _: u64,
    ) -> Result<HumanResponse, HumanOperationError> {
        Err(HumanOperationError::Unavailable)
    }
    fn approval_reject(
        &mut self,
        _: &HumanPeer,
        _: [u8; 32],
        _: [u8; 32],
        _: &str,
        _: u64,
    ) -> Result<HumanResponse, HumanOperationError> {
        Err(HumanOperationError::Unavailable)
    }
    fn identity_resolve(
        &mut self,
        _: &HumanPeer,
        _: &str,
    ) -> Result<HumanResponse, HumanOperationError> {
        Err(HumanOperationError::Unavailable)
    }
    fn lease_map(
        &mut self,
        _: &HumanPeer,
        _: u64,
        _: u64,
    ) -> Result<HumanResponse, HumanOperationError> {
        Err(HumanOperationError::Unavailable)
    }
    fn owner_validate(
        &mut self,
        _: &HumanPeer,
        _: HumanOwnerInstall,
    ) -> Result<HumanResponse, HumanOperationError> {
        Err(HumanOperationError::Unavailable)
    }
    fn owner_install(
        &mut self,
        _: &HumanPeer,
        _: MutationEnvelope<HumanOwnerInstall>,
    ) -> Result<HumanResponse, HumanOperationError> {
        Err(HumanOperationError::Unavailable)
    }
    fn capability_install(
        &mut self,
        peer: &HumanPeer,
        request: HumanCapabilityInstall,
    ) -> Result<HumanResponse, HumanOperationError> {
        install_capability(&mut self.authority, &self.store, peer, request)
    }
    fn agent_lifecycle_publish(
        &mut self,
        _: &HumanPeer,
        _: MutationEnvelope<crate::human::HumanAgentLifecycleSeed>,
    ) -> Result<HumanResponse, HumanOperationError> {
        Err(HumanOperationError::Unavailable)
    }
    fn agent_list(
        &mut self,
        _: &HumanPeer,
        _: Option<[u8; 32]>,
        _: u8,
    ) -> Result<HumanResponse, HumanOperationError> {
        Err(HumanOperationError::Unavailable)
    }
    fn agent_get(&mut self, _: &HumanPeer, _: &str) -> Result<HumanResponse, HumanOperationError> {
        Err(HumanOperationError::Unavailable)
    }
    fn agent_control(
        &mut self,
        _: &HumanPeer,
        _: &str,
        _: bool,
        _: [u8; 32],
        _: HumanFinalizationEvidence,
    ) -> Result<HumanResponse, HumanOperationError> {
        Err(HumanOperationError::Unavailable)
    }
    fn agent_limit(
        &mut self,
        _: &HumanPeer,
        _: &str,
        _: u128,
        _: &str,
        _: [u8; 32],
        _: HumanFinalizationEvidence,
    ) -> Result<HumanResponse, HumanOperationError> {
        Err(HumanOperationError::Unavailable)
    }
    fn agent_journey(
        &mut self,
        _: &HumanPeer,
        _: &str,
        _: crate::human::HumanAgentJourneyKind,
        _: [u8; 32],
        _: [u8; 32],
        _: HumanFinalizationEvidence,
    ) -> Result<HumanResponse, HumanOperationError> {
        Err(HumanOperationError::Unavailable)
    }
    fn agent_archive(
        &mut self,
        _: &HumanPeer,
        _: &str,
        _: &str,
        _: [u8; 32],
        _: [u8; 32],
        _: [u8; 32],
        _: HumanFinalizationEvidence,
    ) -> Result<HumanResponse, HumanOperationError> {
        Err(HumanOperationError::Unavailable)
    }
    fn agent_context(
        &mut self,
        _: &HumanPeer,
        _: &str,
    ) -> Result<HumanResponse, HumanOperationError> {
        Err(HumanOperationError::Unavailable)
    }
    fn agent_budget_state(
        &mut self,
        _: &HumanPeer,
        _: [u8; 32],
    ) -> Result<HumanResponse, HumanOperationError> {
        Err(HumanOperationError::Unavailable)
    }
    fn agent_key_policy(
        &mut self,
        _: &HumanPeer,
        _: &str,
        _: bool,
    ) -> Result<HumanResponse, HumanOperationError> {
        Err(HumanOperationError::Unavailable)
    }
    fn agent_session_snapshot(
        &mut self,
        _: &HumanPeer,
        _: &str,
    ) -> Result<HumanResponse, HumanOperationError> {
        Err(HumanOperationError::Unavailable)
    }
    fn agent_session_suspend(
        &mut self,
        _: &HumanPeer,
        _: &str,
        _: [u8; 32],
    ) -> Result<HumanResponse, HumanOperationError> {
        Err(HumanOperationError::Unavailable)
    }
    fn agent_session_bind(
        &mut self,
        _: &HumanPeer,
        _: &str,
        _: [u8; 32],
        _: [u8; 32],
        _: [u8; 32],
    ) -> Result<HumanResponse, HumanOperationError> {
        Err(HumanOperationError::Unavailable)
    }
}

fn evidence_unavailable(error: &EvidenceError) -> bool {
    match error {
        EvidenceError::Unavailable | EvidenceError::Transport(_) => true,
        EvidenceError::CoreRefusal { result, .. } => {
            result.retriability() == Retriability::Retriable
        }
        _ => false,
    }
}

fn owner_authority(request: &HumanOwnerInstall) -> Result<ProtocolAuthority, HumanOperationError> {
    match request.authority_kind {
        1 => Ok(ProtocolAuthority::PrimaryKey(request.authority_id)),
        2 => Ok(ProtocolAuthority::SessionKey(request.authority_id)),
        3 => Ok(ProtocolAuthority::CapabilityGrant(request.authority_id)),
        _ => Err(HumanOperationError::Refused),
    }
}
fn encode_identity(identity: &CoreIdentity) -> Result<HumanResponse, HumanOperationError> {
    let mut out = Encoder::new();
    out.u64(identity.head_sequence);
    out.u64(identity.revocation_sequence);
    out.u8(verification_code(identity.verification_level));
    out.u8(u8::from(identity.frozen));
    out.u16(identity.authorities.len())?;
    for authority in &identity.authorities {
        let (kind, id) = match authority {
            ProtocolAuthority::PrimaryKey(id) => (1, id),
            ProtocolAuthority::SessionKey(id) => (2, id),
            ProtocolAuthority::CapabilityGrant(id) => (3, id),
        };
        out.u8(kind);
        out.fixed(id);
    }
    out.bytes(&identity.canonical_bytes)?;
    out.finish()
}
fn encode_owner_validation(
    validated: &(CoreIdentity, u64, u64),
) -> Result<HumanResponse, HumanOperationError> {
    let mut out = Encoder::new();
    out.u64(validated.0.head_sequence);
    out.u64(validated.1);
    out.u64(validated.2);
    out.bytes(&validated.0.canonical_bytes)?;
    out.finish()
}
fn owner_digest(request: &HumanOwnerInstall) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"layerx-human-owner-install/v2");
    hash_text(&mut digest, request.agent.as_bytes());
    digest.update([request.authority_kind]);
    digest.update(request.authority_id);
    digest.update(request.session_id);
    digest.update(request.token_id);
    digest.update(request.session_public_key);
    hash_text(&mut digest, &request.registration_payload);
    digest.update(request.grantor);
    digest.update(request.grant_not_before.to_be_bytes());
    digest.update(request.grant_expires_at.to_be_bytes());
    digest.update(request.grant_revocation_sequence.to_be_bytes());
    count(&mut digest, request.permitted_activity_types.len());
    for x in &request.permitted_activity_types {
        digest.update(x.to_be_bytes())
    }
    count(&mut digest, request.scopes.len());
    for x in &request.scopes {
        hash_text(&mut digest, x.as_bytes())
    }
    digest.update(request.lease_not_before_unix_ms.to_be_bytes());
    digest.update(request.lease_not_after_unix_ms.to_be_bytes());
    hash_text(&mut digest, request.opening_client.as_bytes());
    hash_text(&mut digest, request.policy_version.as_bytes());
    match &request.lifecycle {
        None => digest.update([0]),
        Some(v) => {
            digest.update([1]);
            hash_text(&mut digest, v.agent_id.as_bytes());
            hash_text(&mut digest, v.name.as_bytes());
            hash_text(&mut digest, v.purpose.as_bytes());
            hash_text(&mut digest, v.currency.as_bytes());
            digest.update(v.monthly_limit.to_be_bytes());
            digest.update(v.period_start.to_be_bytes());
            digest.update(v.period_end.to_be_bytes());
            digest.update(v.created_at.to_be_bytes());
            digest.update(v.updated_at.to_be_bytes());
            count(&mut digest, v.verified_evidence.len());
            for x in &v.verified_evidence {
                digest.update(x)
            }
            hash_text(&mut digest, v.actor.as_bytes());
            hash_text(&mut digest, v.primary_authority.as_bytes());
            hash_text(&mut digest, v.custody_key.as_bytes());
            digest.update(v.custody_public_key);
            hash_text(&mut digest, v.owner_account.as_bytes());
            hash_text(&mut digest, v.budget_account.as_bytes());
            digest.update(v.budget_asset);
            digest.update(v.purpose_hash);
            digest.update(v.recovery_root);
            digest.update(v.recovery_threshold.to_be_bytes());
            digest.update(v.capability_id);
            count(&mut digest, v.activity_types.len());
            for x in &v.activity_types {
                digest.update(x.to_be_bytes())
            }
            count(&mut digest, v.counterparties.len());
            for x in &v.counterparties {
                digest.update(x)
            }
            count(&mut digest, v.assets.len());
            for x in &v.assets {
                digest.update(x)
            }
            digest.update(v.amount_ceiling.to_be_bytes());
            digest.update(v.rate_maximum_uses.to_be_bytes());
            digest.update(v.rate_window_sequences.to_be_bytes());
            count(&mut digest, v.purposes.len());
            for x in &v.purposes {
                hash_text(&mut digest, x.as_bytes())
            }
            digest.update(v.capability_expiry_sequence.to_be_bytes());
            count(&mut digest, v.session_scopes.len());
            for x in &v.session_scopes {
                hash_text(&mut digest, x.as_bytes())
            }
            digest.update(v.session_expiry_unix_seconds.to_be_bytes());
            digest.update(v.protocol_grant_id);
            digest.update(v.budget_period_seconds.to_be_bytes());
            digest.update(v.budget_expiry_seconds.to_be_bytes());
            digest.update(v.initial_funding.to_be_bytes());
            digest.update(v.network_id.to_be_bytes());
            count(&mut digest, v.creation_receipt_roots.len());
            for x in &v.creation_receipt_roots {
                digest.update(x)
            }
        }
    }
    digest.finalize().into()
}
fn count(digest: &mut Sha256, value: usize) {
    digest.update(u16::try_from(value).unwrap_or(u16::MAX).to_be_bytes())
}
fn map_identity(error: HumanOperationError) -> IdentityError {
    match error {
        HumanOperationError::Unavailable => IdentityError::BoundaryUnavailable,
        HumanOperationError::Refused => IdentityError::Unverified,
    }
}
fn map_identity_operation(error: IdentityError) -> HumanOperationError {
    match error {
        IdentityError::BoundaryUnavailable => HumanOperationError::Unavailable,
        _ => HumanOperationError::Refused,
    }
}
fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if value.is_empty() || value.len() % 2 != 0 || value.len() > MAX_RESPONSE.saturating_mul(2) {
        return None;
    }
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).ok())
        .collect()
}
fn verification_level(value: &Value) -> Result<VerificationLevel, IdentityError> {
    match value.get("verification_level").and_then(Value::as_str) {
        Some("sequencer_signed") => Ok(VerificationLevel::SEQUENCER_SIGNED),
        Some("batch_included") => Ok(VerificationLevel::BATCH_INCLUDED),
        Some("state_proven") => Ok(VerificationLevel::STATE_PROVEN),
        Some("checkpoint_finalised") => Ok(VerificationLevel::CHECKPOINT_FINALISED),
        Some("settlement_anchored") => Ok(VerificationLevel::SETTLEMENT_ANCHORED),
        _ => Err(IdentityError::Unverified),
    }
}

fn encode_approval(out: &mut Encoder, record: &ApprovalRecord) -> Result<(), HumanOperationError> {
    out.fixed(&record.approval_id);
    encode_disclosure(out, &record.held_activity)?;
    out.fixed(&record.canonical_bytes_digest);
    out.text(record.hold_reason.code)?;
    out.text(record.hold_reason.message)?;
    out.u64(record.created_at_sequence);
    out.u64(record.expires_at_sequence);
    out.u8(match record.state {
        ApprovalState::AwaitingApproval => 0,
        ApprovalState::Approved => 1,
        ApprovalState::Rejected => 2,
        ApprovalState::Expired => 3,
        ApprovalState::Defective => 4,
    });
    if record.state == ApprovalState::Approved {
        out.fixed(
            &record
                .submission_ref
                .ok_or(HumanOperationError::Unavailable)?,
        );
    }
    Ok(())
}
fn encode_disclosure(
    out: &mut Encoder,
    value: &layerx_agent_api::prepare::Disclosure,
) -> Result<(), HumanOperationError> {
    out.fixed(&value.canonical_digest);
    out.u16(usize::from(value.activity_type.0))?;
    out.text(value.actor.as_str())?;
    out.text(value.authority.as_str())?;
    out.u16(value.counterparties.values().len())?;
    for item in value.counterparties.values() {
        out.text(item.as_str())?;
    }
    out.u16(value.amounts.values().len())?;
    for item in value.amounts.values() {
        out.text(item.counterparty.as_str())?;
        out.u128(item.amount.0);
    }
    out.text(value.asset.as_str())?;
    out.u128(value.fee_limit.0);
    out.u64(value.expiry.0);
    out.text(value.idempotency_key.as_str())?;
    Ok(())
}
fn encode_decision(decision: &ApprovalDecision) -> Result<HumanResponse, HumanOperationError> {
    let mut out = Encoder::new();
    out.u8(outcome_code(decision.outcome));
    match decision.submission_ref {
        Some(value) => {
            out.u8(1);
            out.fixed(&value);
        }
        None => out.u8(0),
    }
    match decision.winning_outcome {
        Some(value) => {
            out.u8(1);
            out.u8(outcome_code(value));
        }
        None => out.u8(0),
    }
    out.finish()
}
fn outcome_code(value: ApprovalOutcome) -> u8 {
    match value {
        ApprovalOutcome::Granted => 0,
        ApprovalOutcome::Rejected => 1,
        ApprovalOutcome::Expired => 2,
        ApprovalOutcome::Defective => 3,
        ApprovalOutcome::AlreadyDecided => 4,
        ApprovalOutcome::Conflict => 5,
    }
}
fn map_core(error: CoreStateError) -> HumanOperationError {
    match error {
        CoreStateError::Unavailable => HumanOperationError::Unavailable,
        CoreStateError::Unverified | CoreStateError::Refused { .. } => HumanOperationError::Refused,
    }
}
fn boundary_correlation(peer: &HumanPeer, actor: &[u8], purpose: &[u8]) -> u64 {
    let mut digest = Sha256::new();
    digest.update(b"layerx-agentd/human-lni-correlation/v1\0");
    hash_text(&mut digest, peer.tenant.as_bytes());
    hash_text(&mut digest, peer.principal.as_bytes());
    hash_text(&mut digest, actor);
    hash_text(&mut digest, purpose);
    let bytes = digest.finalize();
    let mut correlation = [0_u8; 8];
    correlation.copy_from_slice(&bytes[..8]);
    let value = u64::from_be_bytes(correlation);
    if value == 0 {
        1
    } else {
        value
    }
}
fn state_code(state: SubmissionState) -> u8 {
    match state {
        SubmissionState::Prepared => 0,
        SubmissionState::Signed => 1,
        SubmissionState::Queued => 2,
        SubmissionState::Submitted => 3,
        SubmissionState::Acknowledged => 4,
        SubmissionState::Unknown => 5,
        SubmissionState::Executed => 6,
        SubmissionState::Failed => 7,
        SubmissionState::Expired | SubmissionState::Superseded => 8,
    }
}
fn verification_code(level: VerificationLevel) -> u8 {
    if level == VerificationLevel::UNVERIFIED {
        0
    } else if level == VerificationLevel::SEQUENCER_SIGNED {
        1
    } else if level == VerificationLevel::BATCH_INCLUDED {
        2
    } else if level == VerificationLevel::STATE_PROVEN {
        3
    } else if level == VerificationLevel::CHECKPOINT_FINALISED {
        4
    } else {
        5
    }
}
fn hex(bytes: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|b| [H[(b >> 4) as usize] as char, H[(b & 15) as usize] as char])
        .collect()
}
fn digest_from_hex(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    let mut out = [0; 32];
    for (index, slot) in out.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(out)
}
fn u64_field(value: &Value, field: &str) -> Result<u64, HumanOperationError> {
    value
        .get(field)
        .and_then(Value::as_u64)
        .ok_or(HumanOperationError::Refused)
}
fn hex_field(value: &Value, field: &str) -> Result<[u8; 32], HumanOperationError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .and_then(digest_from_hex)
        .ok_or(HumanOperationError::Refused)
}
fn query(value: &str) -> String {
    value
        .bytes()
        .flat_map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
                vec![char::from(byte)]
            } else {
                format!("%{byte:02X}").chars().collect()
            }
        })
        .collect()
}
fn prepare_digest(request: &HumanPrepare) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"layerx-human-journey-prepare/v1");
    digest.update(request.activity_type.to_be_bytes());
    hash_text(&mut digest, request.actor.as_bytes());
    hash_text(&mut digest, request.authority.as_bytes());
    digest.update(request.account_sequence.to_be_bytes());
    digest.update(request.not_before.to_be_bytes());
    digest.update(request.not_after.to_be_bytes());
    hash_text(&mut digest, request.idempotency_key.as_bytes());
    digest.update(request.fee_limit.to_be_bytes());
    digest.update(request.payload_hash);
    digest.update(Sha256::digest(&request.payload));
    digest.finalize().into()
}
fn submit_digest(request: &HumanSubmit) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"layerx-human-journey-submit/v1");
    hash_text(&mut digest, request.preparation_ref.as_bytes());
    digest.update(Sha256::digest(&request.signature));
    digest.update(request.signer_public_key);
    match request.approval_release_ref {
        Some(reference) => {
            digest.update([1]);
            digest.update(reference);
        }
        None => digest.update([0]),
    }
    digest.finalize().into()
}
fn hash_text(digest: &mut Sha256, value: &[u8]) {
    digest.update(u32::try_from(value.len()).unwrap_or(u32::MAX).to_be_bytes());
    digest.update(value);
}

struct Encoder(Vec<u8>);
impl Encoder {
    fn new() -> Self {
        Self(Vec::with_capacity(256))
    }
    fn u8(&mut self, value: u8) {
        self.0.push(value);
    }
    fn u16(&mut self, value: usize) -> Result<(), HumanOperationError> {
        self.0.extend_from_slice(
            &u16::try_from(value)
                .map_err(|_| HumanOperationError::Refused)?
                .to_be_bytes(),
        );
        Ok(())
    }
    fn u32(&mut self, value: u32) {
        self.0.extend_from_slice(&value.to_be_bytes());
    }
    fn u64(&mut self, value: u64) {
        self.0.extend_from_slice(&value.to_be_bytes());
    }
    fn u128(&mut self, value: u128) {
        self.0.extend_from_slice(&value.to_be_bytes());
    }
    fn fixed(&mut self, value: &[u8]) {
        self.0.extend_from_slice(value);
    }
    fn bytes(&mut self, value: &[u8]) -> Result<(), HumanOperationError> {
        if value.is_empty() || value.len() > MAX_RESPONSE {
            return Err(HumanOperationError::Refused);
        }
        self.u32(u32::try_from(value.len()).map_err(|_| HumanOperationError::Refused)?);
        self.fixed(value);
        Ok(())
    }
    fn text(&mut self, value: &str) -> Result<(), HumanOperationError> {
        self.bytes(value.as_bytes())
    }
    fn finish(self) -> Result<HumanResponse, HumanOperationError> {
        HumanResponse::new(self.0).map_err(|_| HumanOperationError::Refused)
    }
}

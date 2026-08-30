//! Bounded, typed production transport for the Human journey engine.

use std::path::{Path, PathBuf};

use layerx_agent_api::idempotency::IdempotentMutation;
use layerx_agent_api::prepare::{PreparationRef, PrepareRequest};
use layerx_agent_api::submit::SubmitRequest;
use layerx_agent_api::track::{
    EvidenceRef, ReceiptRef, SubmissionRef, SubmissionState, TrackRequest, TrackedSubmission,
    Transition,
};
use layerx_agent_api::verify::Level;
use layerx_agent_api::TimestampSeconds;
use layerx_client::lni::transport::{ConnectionGate, FrameTransport, Limits, Uds};
use layerx_crypto::disclosure;
use layerx_proof::receipt::AuthorizedBatch;
use layerx_sdk::Call;
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_types::result::ResultCode;
use layerx_types::verify::VerificationLevel;
use sha2::Digest as _;
use zeroize::Zeroizing;

use crate::journeys::DepositAgentBoundary;
use crate::journeys::{
    AgentBoundary, AgentBoundaryError, AgentObservation, AgentPreparation, ReceiptLookup,
    ReceiptMaterial,
};

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
const BALANCE: u8 = 6;
const HEAD: u8 = 7;
const EVIDENCE: u8 = 8;
const ACCOUNT_SEQUENCE: u8 = 13;
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
const MAX_TEXT: usize = 255;
const MAX_BYTES: usize = 1_048_576;
const MAX_EVIDENCE: usize = 64;
const MAX_TRANSITIONS: usize = 64;

/// Production journey adapter. Every call opens one deadline-bounded framed
/// UDS exchange; no socket, request buffer, or untrusted response is retained.
pub struct AgentRuntime {
    endpoint: PathBuf,
    gate: ConnectionGate,
    limits: Limits,
    registry: ModuleRegistry,
}

pub struct AgentApprovalPage {
    pub approvals: Vec<crate::approvals::AgentApprovalRecord>,
    pub next_cursor: Option<[u8; 32]>,
}

pub struct VerifiedBalance {
    pub account: [u8; 32],
    pub asset: [u8; 32],
    pub currency: String,
    pub observed_at: String,
    pub age_seconds: u64,
    pub amount: u128,
    pub verification: u8,
    pub global_sequence: u64,
    pub batch_number: u64,
    pub observed_head_sequence: u64,
    pub observed_checkpoint: [u8; 32],
    pub canonical_bytes: Vec<u8>,
    pub proof_material: Vec<u8>,
}

pub struct AgentHead {
    pub chain_sequence: u64,
    pub sealed_batch: u64,
    pub finalised_checkpoint: [u8; 32],
}

pub struct AgentCoreIdentity {
    pub head_sequence: u64,
    pub revocation_sequence: u64,
    pub verification: u8,
    pub frozen: bool,
    pub authorities: Vec<(u8, [u8; 32])>,
    pub canonical_bytes: Vec<u8>,
}
pub struct AgentLease {
    pub not_before_sequence: u64,
    pub expiry_sequence: u64,
    pub observed_head_sequence: u64,
    pub canonical_attestation: Vec<u8>,
}
pub struct AgentOwnerValidation {
    pub identity_head_sequence: u64,
    pub expiry_sequence: u64,
    pub observed_head_sequence: u64,
    pub canonical_identity: Vec<u8>,
}
pub struct AgentOwnerInstalled {
    pub token_id: [u8; 32],
    pub session_id: [u8; 32],
    pub expiry_sequence: u64,
    pub observed_head_sequence: u64,
}
pub struct AgentSessionSeed(Zeroizing<[u8; 32]>);
impl AgentSessionSeed {
    pub fn new(seed: [u8; 32]) -> Result<Self, AgentBoundaryError> {
        if seed == [0; 32] {
            Err(AgentBoundaryError::Refused)
        } else {
            Ok(Self(Zeroizing::new(seed)))
        }
    }
    fn expose(&self) -> &[u8; 32] {
        &self.0
    }
}
impl std::fmt::Debug for AgentSessionSeed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AgentSessionSeed([redacted])")
    }
}
pub struct AgentOwnerInstall {
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
    pub session_seed: Option<AgentSessionSeed>,
    pub permitted_activity_types: Vec<u16>,
    pub scopes: Vec<String>,
    pub lease_not_before_unix_ms: u64,
    pub lease_not_after_unix_ms: u64,
    pub opening_client: String,
    pub policy_version: String,
    pub lifecycle: Option<AgentLifecycleSeed>,
}
pub struct AgentLifecycleSeed {
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
pub struct AgentCapabilityInstall {
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

pub struct ManagedAgentEvidence {
    pub evidence_id: String,
    pub class: String,
    pub verification: u8,
}
pub struct ManagedAgentView {
    pub agent_id: String,
    pub name: String,
    pub purpose: String,
    pub state: u8,
    pub monthly_limit: u128,
    pub currency: String,
    pub limit_enforcement: u8,
    pub period_start: String,
    pub period_end: String,
    pub spent: u128,
    pub remaining: u128,
    pub spend_verification: u8,
    pub created_at: String,
    pub updated_at: String,
    pub evidence: Vec<ManagedAgentEvidence>,
}
pub struct ManagedAgentPage {
    pub agents: Vec<ManagedAgentView>,
    pub next_cursor: Option<[u8; 32]>,
}
pub struct ManagedAgentJourneyStage {
    pub stage_id: String,
    pub copy_key: String,
    pub state: u8,
    pub evidence: Vec<ManagedAgentEvidence>,
}
pub struct ManagedAgentJourney {
    pub journey_id: String,
    pub kind: String,
    pub state: u8,
    pub stages: Vec<ManagedAgentJourneyStage>,
    pub started_at: String,
    pub updated_at: String,
    pub evidence: Vec<ManagedAgentEvidence>,
}
pub struct ManagedAgentChallenge {
    pub agent_id: String,
    pub kind: u8,
    pub delay_seconds: u64,
    pub ready_at: String,
    pub evidence: Vec<ManagedAgentEvidence>,
}
#[derive(Clone, Copy)]
pub struct AgentFinalizationEvidence {
    pub action_key: [u8; 32],
    pub activity_id: [u8; 32],
    pub receipt_digest: [u8; 32],
    pub observed_sequence: u64,
    pub verification: u8,
    pub finalized_at: u64,
}
pub struct AgentLifecycleContext {
    pub seed: AgentLifecycleSeed,
    pub agent_did: String,
    pub session_id: [u8; 32],
    pub session_token_id: [u8; 32],
    pub protocol_grant_id: [u8; 32],
    pub active_budget_id: [u8; 32],
    pub state: u8,
    pub current_monthly_limit: u128,
    pub spent: u128,
    pub updated_at: u64,
}
pub struct AgentBudgetState {
    pub active_budget_id: [u8; 32],
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
pub struct AgentKeyPolicy {
    pub agent_did: String,
    pub recovery: bool,
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
pub struct AgentSessionSnapshot {
    pub agent_id: String,
    pub agent_did: String,
    pub session_id: [u8; 32],
    pub token_id: [u8; 32],
    pub open: bool,
    pub expiry_sequence: u64,
    pub sequence: u64,
}
pub struct AgentSessionObservation {
    pub agent_id: String,
    pub agent_did: String,
    pub session_id: [u8; 32],
    pub token_id: [u8; 32],
    pub open: bool,
    pub action_key: [u8; 32],
    pub evidence_digest: [u8; 32],
}

impl AgentOwnerInstall {
    pub fn body_digest(&self) -> Result<[u8; 32], AgentBoundaryError> {
        let mut digest = sha2::Sha256::new();
        digest.update(b"layerx-human-owner-install/v2");
        digest_text(&mut digest, self.agent.as_bytes())?;
        digest.update([self.authority_kind]);
        digest.update(self.authority_id);
        digest.update(self.session_id);
        digest.update(self.token_id);
        digest.update(self.session_public_key);
        digest_text(&mut digest, &self.registration_payload)?;
        digest.update(self.grantor);
        digest.update(self.grant_not_before.to_be_bytes());
        digest.update(self.grant_expires_at.to_be_bytes());
        digest.update(self.grant_revocation_sequence.to_be_bytes());
        digest.update(
            u16::try_from(self.permitted_activity_types.len())
                .map_err(|_| AgentBoundaryError::Refused)?
                .to_be_bytes(),
        );
        for value in &self.permitted_activity_types {
            digest.update(value.to_be_bytes());
        }
        digest.update(
            u16::try_from(self.scopes.len())
                .map_err(|_| AgentBoundaryError::Refused)?
                .to_be_bytes(),
        );
        for value in &self.scopes {
            digest_text(&mut digest, value.as_bytes())?;
        }
        digest.update(self.lease_not_before_unix_ms.to_be_bytes());
        digest.update(self.lease_not_after_unix_ms.to_be_bytes());
        digest_text(&mut digest, self.opening_client.as_bytes())?;
        digest_text(&mut digest, self.policy_version.as_bytes())?;
        match &self.lifecycle {
            None => digest.update([0]),
            Some(value) => {
                digest.update([1]);
                digest_lifecycle(&mut digest, value)?;
            }
        }
        Ok(digest.finalize().into())
    }
}
impl AgentLifecycleSeed {
    pub fn body_digest(&self) -> Result<[u8; 32], AgentBoundaryError> {
        let mut wire = Writer::new(0);
        encode_lifecycle(&mut wire, self)?;
        let mut digest = sha2::Sha256::new();
        digest.update(b"layerx-human-agent-lifecycle-publish/v1");
        digest.update(&wire.0[10..]);
        Ok(digest.finalize().into())
    }
}

fn digest_text(digest: &mut sha2::Sha256, value: &[u8]) -> Result<(), AgentBoundaryError> {
    digest.update(
        u32::try_from(value.len())
            .map_err(|_| AgentBoundaryError::Refused)?
            .to_be_bytes(),
    );
    digest.update(value);
    Ok(())
}

fn digest_lifecycle(
    digest: &mut sha2::Sha256,
    value: &AgentLifecycleSeed,
) -> Result<(), AgentBoundaryError> {
    for text in [
        &value.agent_id,
        &value.name,
        &value.purpose,
        &value.currency,
    ] {
        digest_text(digest, text.as_bytes())?;
    }
    digest.update(value.monthly_limit.to_be_bytes());
    digest.update(value.period_start.to_be_bytes());
    digest.update(value.period_end.to_be_bytes());
    digest.update(value.created_at.to_be_bytes());
    digest.update(value.updated_at.to_be_bytes());
    digest.update(
        u16::try_from(value.verified_evidence.len())
            .map_err(|_| AgentBoundaryError::Refused)?
            .to_be_bytes(),
    );
    for evidence in &value.verified_evidence {
        digest.update(evidence);
    }
    for text in [&value.actor, &value.primary_authority, &value.custody_key] {
        digest_text(digest, text.as_bytes())?;
    }
    digest.update(value.custody_public_key);
    digest_text(digest, value.owner_account.as_bytes())?;
    digest_text(digest, value.budget_account.as_bytes())?;
    digest.update(value.budget_asset);
    digest.update(value.purpose_hash);
    digest.update(value.recovery_root);
    digest.update(value.recovery_threshold.to_be_bytes());
    digest.update(value.capability_id);
    digest.update(
        u16::try_from(value.activity_types.len())
            .map_err(|_| AgentBoundaryError::Refused)?
            .to_be_bytes(),
    );
    for item in &value.activity_types {
        digest.update(item.to_be_bytes());
    }
    for values in [&value.counterparties, &value.assets] {
        digest.update(
            u16::try_from(values.len())
                .map_err(|_| AgentBoundaryError::Refused)?
                .to_be_bytes(),
        );
        for item in values {
            digest.update(item);
        }
    }
    digest.update(value.amount_ceiling.to_be_bytes());
    digest.update(value.rate_maximum_uses.to_be_bytes());
    digest.update(value.rate_window_sequences.to_be_bytes());
    digest.update(
        u16::try_from(value.purposes.len())
            .map_err(|_| AgentBoundaryError::Refused)?
            .to_be_bytes(),
    );
    for item in &value.purposes {
        digest_text(digest, item.as_bytes())?;
    }
    digest.update(value.capability_expiry_sequence.to_be_bytes());
    digest.update(
        u16::try_from(value.session_scopes.len())
            .map_err(|_| AgentBoundaryError::Refused)?
            .to_be_bytes(),
    );
    for item in &value.session_scopes {
        digest_text(digest, item.as_bytes())?;
    }
    digest.update(value.session_expiry_unix_seconds.to_be_bytes());
    digest.update(value.protocol_grant_id);
    digest.update(value.budget_period_seconds.to_be_bytes());
    digest.update(value.budget_expiry_seconds.to_be_bytes());
    digest.update(value.initial_funding.to_be_bytes());
    digest.update(value.network_id.to_be_bytes());
    digest.update(
        u16::try_from(value.creation_receipt_roots.len())
            .map_err(|_| AgentBoundaryError::Refused)?
            .to_be_bytes(),
    );
    for item in &value.creation_receipt_roots {
        digest.update(item);
    }
    Ok(())
}

impl AgentRuntime {
    pub fn publish_lifecycle(
        &mut self,
        request_id: u64,
        key: [u8; 32],
        seed: &AgentLifecycleSeed,
    ) -> Result<(), AgentBoundaryError> {
        let mut writer = Writer::new(AGENT_LIFECYCLE_PUBLISH);
        writer.u64(request_id);
        writer.fixed(&key);
        writer.fixed(&seed.body_digest()?);
        let tag = writer.0.len();
        encode_lifecycle(&mut writer, seed)?;
        writer.0.remove(tag);
        let mut reader = self.exchange(writer.finish())?;
        if reader.u8()? != 1 {
            return Err(AgentBoundaryError::CorruptResponse);
        }
        reader.finish()
    }
    pub fn capability_install(
        &mut self,
        request: &AgentCapabilityInstall,
    ) -> Result<([u8; 32], u64, u8, [u8; 32]), AgentBoundaryError> {
        if request.action_key == [0; 32]
            || request.activity_types.is_empty()
            || request.counterparties.is_empty()
            || request.assets.is_empty()
            || request.purposes.is_empty()
            || request.amount_ceiling == 0
            || request.rate_maximum_uses == 0
            || request.rate_window_sequences == 0
            || request.expiry_sequence == 0
        {
            return Err(AgentBoundaryError::Refused);
        }
        let mut writer = Writer::new(CAPABILITY_INSTALL);
        writer.fixed(&request.action_key);
        writer.text(&request.agent)?;
        writer.fixed(&request.authority_id);
        writer.fixed(&request.capability_id);
        writer.u16(
            u16::try_from(request.activity_types.len()).map_err(|_| AgentBoundaryError::Refused)?,
        );
        for value in &request.activity_types {
            writer.u16(*value);
        }
        writer.u16(
            u16::try_from(request.counterparties.len()).map_err(|_| AgentBoundaryError::Refused)?,
        );
        for value in &request.counterparties {
            writer.fixed(value);
        }
        writer.u16(u16::try_from(request.assets.len()).map_err(|_| AgentBoundaryError::Refused)?);
        for value in &request.assets {
            writer.fixed(value);
        }
        writer.u128(request.amount_ceiling);
        writer.u64(request.rate_maximum_uses);
        writer.u64(request.rate_window_sequences);
        writer.u16(u16::try_from(request.purposes.len()).map_err(|_| AgentBoundaryError::Refused)?);
        for value in &request.purposes {
            writer.text(value)?;
        }
        writer.u64(request.expiry_sequence);
        let mut reader = self.exchange(writer.finish())?;
        let object_id = reader.fixed()?;
        let observed_sequence = reader.u64()?;
        let verification = reader.u8()?;
        let receipt_digest = reader.fixed()?;
        if object_id != request.capability_id
            || observed_sequence == 0
            || verification < 2
            || verification > 5
            || receipt_digest == [0; 32]
        {
            return Err(AgentBoundaryError::CorruptResponse);
        }
        reader.finish()?;
        Ok((object_id, observed_sequence, verification, receipt_digest))
    }
    pub fn agent_list(
        &mut self,
        cursor: Option<[u8; 32]>,
        limit: u8,
    ) -> Result<ManagedAgentPage, AgentBoundaryError> {
        if limit == 0 || limit > 100 {
            return Err(AgentBoundaryError::Refused);
        }
        let mut writer = Writer::new(AGENT_LIST);
        match cursor {
            Some(value) => {
                writer.u8(1);
                writer.fixed(&value);
            }
            None => writer.u8(0),
        }
        writer.u8(limit);
        let mut reader = self.exchange(writer.finish())?;
        let count = usize::from(reader.u8()?);
        if count > usize::from(limit) {
            return Err(AgentBoundaryError::CorruptResponse);
        }
        let mut agents = Vec::with_capacity(count);
        for _ in 0..count {
            agents.push(decode_managed_agent(&mut reader)?);
        }
        let next_cursor = match reader.u8()? {
            0 => None,
            1 => Some(reader.fixed()?),
            _ => return Err(AgentBoundaryError::CorruptResponse),
        };
        reader.finish()?;
        Ok(ManagedAgentPage {
            agents,
            next_cursor,
        })
    }
    pub fn agent_get(&mut self, agent_id: &str) -> Result<ManagedAgentView, AgentBoundaryError> {
        let mut writer = Writer::new(AGENT_GET);
        writer.text(agent_id)?;
        let mut reader = self.exchange(writer.finish())?;
        let value = decode_managed_agent(&mut reader)?;
        reader.finish()?;
        Ok(value)
    }
    pub fn agent_context(
        &mut self,
        agent_id: &str,
    ) -> Result<AgentLifecycleContext, AgentBoundaryError> {
        let mut writer = Writer::new(AGENT_CONTEXT);
        writer.text(agent_id)?;
        let mut reader = self.exchange(writer.finish())?;
        let seed = decode_lifecycle(&mut reader)?;
        let value = AgentLifecycleContext {
            seed,
            agent_did: reader.text()?,
            session_id: reader.fixed()?,
            session_token_id: reader.fixed()?,
            protocol_grant_id: reader.fixed()?,
            active_budget_id: reader.fixed()?,
            state: reader.u8()?,
            current_monthly_limit: reader.u128()?,
            spent: reader.u128()?,
            updated_at: reader.u64()?,
        };
        if value.agent_did.is_empty()
            || value.session_id == [0; 32]
            || value.session_token_id == [0; 32]
            || value.protocol_grant_id == [0; 32]
            || value.active_budget_id == [0; 32]
            || value.state > 4
            || value.current_monthly_limit == 0
            || value.spent > value.current_monthly_limit
        {
            return Err(AgentBoundaryError::CorruptResponse);
        }
        reader.finish()?;
        Ok(value)
    }
    pub fn agent_budget_state(
        &mut self,
        active_budget_id: [u8; 32],
    ) -> Result<AgentBudgetState, AgentBoundaryError> {
        if active_budget_id == [0; 32] {
            return Err(AgentBoundaryError::Refused);
        }
        let mut writer = Writer::new(AGENT_BUDGET_STATE);
        writer.fixed(&active_budget_id);
        let mut reader = self.exchange(writer.finish())?;
        let value = AgentBudgetState {
            active_budget_id: reader.fixed()?,
            revocation_sequence: reader.u64()?,
            observed_head_sequence: reader.u64()?,
            verification: reader.u8()?,
            evidence_digest: reader.fixed()?,
            receipt_digest: reader.fixed()?,
            checkpoint_digest: reader.fixed()?,
            age_sequences: reader.u64()?,
            maximum_age_sequences: reader.u64()?,
            remaining: reader.u128()?,
            asset: reader.fixed()?,
        };
        if value.active_budget_id != active_budget_id
            || value.revocation_sequence == 0
            || value.observed_head_sequence < value.revocation_sequence
            || !(4..=5).contains(&value.verification)
            || value.evidence_digest == [0; 32]
            || value.receipt_digest == [0; 32]
            || value.checkpoint_digest == [0; 32]
            || value.maximum_age_sequences == 0
            || value.age_sequences > value.maximum_age_sequences
            || value.asset == [0; 32]
        {
            return Err(AgentBoundaryError::CorruptResponse);
        }
        reader.finish()?;
        Ok(value)
    }
    pub fn agent_key_policy(
        &mut self,
        agent_did: &str,
        recovery: bool,
    ) -> Result<AgentKeyPolicy, AgentBoundaryError> {
        let mut writer = Writer::new(AGENT_KEY_POLICY);
        writer.text(agent_did)?;
        writer.u8(u8::from(recovery));
        let mut reader = self.exchange(writer.finish())?;
        let value = AgentKeyPolicy {
            agent_did: reader.text()?,
            recovery: match reader.u8()? {
                0 => false,
                1 => true,
                _ => return Err(AgentBoundaryError::CorruptResponse),
            },
            policy_revision: reader.u64()?,
            required_delay_seconds: reader.u64()?,
            maximum_delay_seconds: reader.u64()?,
            effective_sequence: reader.u64()?,
            observed_head_sequence: reader.u64()?,
            verification: reader.u8()?,
            evidence_digest: reader.fixed()?,
            checkpoint_digest: reader.fixed()?,
            age_sequences: reader.u64()?,
            maximum_age_sequences: reader.u64()?,
        };
        if value.agent_did != agent_did
            || value.recovery != recovery
            || value.policy_revision == 0
            || value.required_delay_seconds == 0
            || value.required_delay_seconds > value.maximum_delay_seconds
            || value.effective_sequence == 0
            || value.observed_head_sequence < value.effective_sequence
            || !(4..=5).contains(&value.verification)
            || value.evidence_digest == [0; 32]
            || value.checkpoint_digest == [0; 32]
            || value.maximum_age_sequences == 0
            || value.age_sequences > value.maximum_age_sequences
        {
            return Err(AgentBoundaryError::CorruptResponse);
        }
        reader.finish()?;
        Ok(value)
    }
    pub fn agent_session_snapshot(
        &mut self,
        agent_id: &str,
    ) -> Result<AgentSessionSnapshot, AgentBoundaryError> {
        let mut writer = Writer::new(AGENT_SESSION_SNAPSHOT);
        writer.text(agent_id)?;
        let mut reader = self.exchange(writer.finish())?;
        let value = AgentSessionSnapshot {
            agent_id: reader.text()?,
            agent_did: reader.text()?,
            session_id: reader.fixed()?,
            token_id: reader.fixed()?,
            open: match reader.u8()? {
                0 => false,
                1 => true,
                _ => return Err(AgentBoundaryError::CorruptResponse),
            },
            expiry_sequence: reader.u64()?,
            sequence: reader.u64()?,
        };
        if value.agent_id != agent_id
            || value.agent_did.is_empty()
            || value.session_id == [0; 32]
            || value.token_id == [0; 32]
            || value.expiry_sequence == 0
            || value.sequence == 0
        {
            return Err(AgentBoundaryError::CorruptResponse);
        }
        reader.finish()?;
        Ok(value)
    }
    pub fn agent_session_suspend(
        &mut self,
        agent_id: &str,
        action_key: [u8; 32],
    ) -> Result<AgentSessionObservation, AgentBoundaryError> {
        let mut writer = Writer::new(AGENT_SESSION_SUSPEND);
        writer.text(agent_id)?;
        writer.fixed(&action_key);
        self.session_observation(writer, agent_id, action_key, false)
    }
    pub fn agent_session_bind(
        &mut self,
        agent_id: &str,
        session_id: [u8; 32],
        token_id: [u8; 32],
        action_key: [u8; 32],
    ) -> Result<AgentSessionObservation, AgentBoundaryError> {
        let mut writer = Writer::new(AGENT_SESSION_BIND);
        writer.text(agent_id)?;
        writer.fixed(&session_id);
        writer.fixed(&token_id);
        writer.fixed(&action_key);
        let value = self.session_observation(writer, agent_id, action_key, true)?;
        if value.session_id != session_id || value.token_id != token_id {
            return Err(AgentBoundaryError::CorruptResponse);
        }
        Ok(value)
    }
    fn session_observation(
        &mut self,
        writer: Writer,
        agent_id: &str,
        action_key: [u8; 32],
        open: bool,
    ) -> Result<AgentSessionObservation, AgentBoundaryError> {
        if action_key == [0; 32] {
            return Err(AgentBoundaryError::Refused);
        }
        let mut reader = self.exchange(writer.finish())?;
        let value = AgentSessionObservation {
            agent_id: reader.text()?,
            agent_did: reader.text()?,
            session_id: reader.fixed()?,
            token_id: reader.fixed()?,
            open: match reader.u8()? {
                0 => false,
                1 => true,
                _ => return Err(AgentBoundaryError::CorruptResponse),
            },
            action_key: reader.fixed()?,
            evidence_digest: reader.fixed()?,
        };
        if value.agent_id != agent_id
            || value.agent_did.is_empty()
            || value.open != open
            || value.action_key != action_key
            || value.session_id == [0; 32]
            || value.token_id == [0; 32]
            || value.evidence_digest == [0; 32]
        {
            return Err(AgentBoundaryError::CorruptResponse);
        }
        reader.finish()?;
        Ok(value)
    }
    pub fn agent_control(
        &mut self,
        agent_id: &str,
        resume: bool,
        session_observation: [u8; 32],
        evidence: AgentFinalizationEvidence,
    ) -> Result<ManagedAgentView, AgentBoundaryError> {
        if session_observation == [0; 32] {
            return Err(AgentBoundaryError::Refused);
        }
        let mut writer = Writer::new(AGENT_CONTROL);
        writer.text(agent_id)?;
        writer.u8(u8::from(resume));
        writer.fixed(&session_observation);
        encode_finalization(&mut writer, evidence)?;
        let mut reader = self.exchange(writer.finish())?;
        let value = decode_managed_agent(&mut reader)?;
        reader.finish()?;
        Ok(value)
    }
    pub fn agent_limit(
        &mut self,
        agent_id: &str,
        monthly_limit: u128,
        currency: &str,
        replacement_budget_id: [u8; 32],
        evidence: AgentFinalizationEvidence,
    ) -> Result<ManagedAgentView, AgentBoundaryError> {
        if monthly_limit == 0 || replacement_budget_id == [0; 32] {
            return Err(AgentBoundaryError::Refused);
        }
        let mut writer = Writer::new(AGENT_LIMIT);
        writer.text(agent_id)?;
        writer.u128(monthly_limit);
        writer.text(currency)?;
        writer.fixed(&replacement_budget_id);
        encode_finalization(&mut writer, evidence)?;
        let mut reader = self.exchange(writer.finish())?;
        let value = decode_managed_agent(&mut reader)?;
        reader.finish()?;
        Ok(value)
    }
    pub fn agent_reclaim(
        &mut self,
        agent_id: &str,
        amount: u128,
        currency: &str,
        pre_observation: [u8; 32],
        post_observation: [u8; 32],
        evidence: AgentFinalizationEvidence,
    ) -> Result<ManagedAgentJourney, AgentBoundaryError> {
        self.agent_journey(
            0,
            agent_id,
            amount,
            currency,
            0,
            0,
            pre_observation,
            post_observation,
            evidence,
        )
    }
    pub fn agent_key_change(
        &mut self,
        agent_id: &str,
        recover: bool,
        challenge_delay_seconds: u64,
        ready_at: u64,
        evidence: AgentFinalizationEvidence,
    ) -> Result<ManagedAgentChallenge, AgentBoundaryError> {
        let mut writer = Writer::new(AGENT_JOURNEY);
        writer.u8(if recover { 2 } else { 1 });
        writer.text(agent_id)?;
        writer.u128(0);
        writer.text("")?;
        writer.u64(challenge_delay_seconds);
        writer.u64(ready_at);
        writer.fixed(&[0; 32]);
        writer.fixed(&[0; 32]);
        encode_finalization(&mut writer, evidence)?;
        let mut reader = self.exchange(writer.finish())?;
        let value = decode_managed_challenge(&mut reader)?;
        reader.finish()?;
        Ok(value)
    }
    pub fn agent_archive(
        &mut self,
        agent_id: &str,
        confirm_name: &str,
        pre_observation: [u8; 32],
        post_observation: [u8; 32],
        session_observation: [u8; 32],
        evidence: AgentFinalizationEvidence,
    ) -> Result<ManagedAgentJourney, AgentBoundaryError> {
        if session_observation == [0; 32] {
            return Err(AgentBoundaryError::Refused);
        }
        let mut writer = Writer::new(AGENT_ARCHIVE);
        writer.text(agent_id)?;
        writer.text(confirm_name)?;
        writer.fixed(&pre_observation);
        writer.fixed(&post_observation);
        writer.fixed(&session_observation);
        encode_finalization(&mut writer, evidence)?;
        let mut reader = self.exchange(writer.finish())?;
        let value = decode_managed_journey(&mut reader)?;
        reader.finish()?;
        Ok(value)
    }
    fn agent_journey(
        &mut self,
        kind: u8,
        agent_id: &str,
        amount: u128,
        currency: &str,
        delay: u64,
        ready_at: u64,
        pre_observation: [u8; 32],
        post_observation: [u8; 32],
        evidence: AgentFinalizationEvidence,
    ) -> Result<ManagedAgentJourney, AgentBoundaryError> {
        let mut writer = Writer::new(AGENT_JOURNEY);
        writer.u8(kind);
        writer.text(agent_id)?;
        writer.u128(amount);
        writer.text(currency)?;
        writer.u64(delay);
        writer.u64(ready_at);
        writer.fixed(&pre_observation);
        writer.fixed(&post_observation);
        encode_finalization(&mut writer, evidence)?;
        let mut reader = self.exchange(writer.finish())?;
        let value = decode_managed_journey(&mut reader)?;
        reader.finish()?;
        Ok(value)
    }
    pub fn identity_resolve(
        &mut self,
        agent: &str,
    ) -> Result<AgentCoreIdentity, AgentBoundaryError> {
        let mut writer = Writer::new(IDENTITY_RESOLVE);
        writer.text(agent)?;
        let mut reader = self.exchange(writer.finish())?;
        let head_sequence = reader.u64()?;
        let revocation_sequence = reader.u64()?;
        let verification = reader.u8()?;
        let frozen = match reader.u8()? {
            0 => false,
            1 => true,
            _ => return Err(AgentBoundaryError::CorruptResponse),
        };
        let count = usize::from(reader.u16()?);
        if count == 0 || count > 256 {
            return Err(AgentBoundaryError::CorruptResponse);
        }
        let mut authorities = Vec::with_capacity(count);
        for _ in 0..count {
            let kind = reader.u8()?;
            if !(1..=3).contains(&kind) {
                return Err(AgentBoundaryError::CorruptResponse);
            }
            authorities.push((kind, reader.fixed()?));
        }
        let canonical_bytes = reader.bytes()?;
        if revocation_sequence == 0 {
            return Err(AgentBoundaryError::CorruptResponse);
        }
        reader.finish()?;
        Ok(AgentCoreIdentity {
            head_sequence,
            revocation_sequence,
            verification,
            frozen,
            authorities,
            canonical_bytes,
        })
    }
    pub fn lease_map(
        &mut self,
        not_before_unix_ms: u64,
        not_after_unix_ms: u64,
    ) -> Result<AgentLease, AgentBoundaryError> {
        let mut writer = Writer::new(LEASE_MAP);
        writer.u64(not_before_unix_ms);
        writer.u64(not_after_unix_ms);
        let mut reader = self.exchange(writer.finish())?;
        let value = AgentLease {
            not_before_sequence: reader.u64()?,
            expiry_sequence: reader.u64()?,
            observed_head_sequence: reader.u64()?,
            canonical_attestation: reader.bytes()?,
        };
        if value.expiry_sequence <= value.not_before_sequence
            || value.expiry_sequence <= value.observed_head_sequence
        {
            return Err(AgentBoundaryError::CorruptResponse);
        }
        reader.finish()?;
        Ok(value)
    }
    pub fn owner_validate(
        &mut self,
        request: &AgentOwnerInstall,
    ) -> Result<AgentOwnerValidation, AgentBoundaryError> {
        self.owner_exchange(OWNER_VALIDATE, None, request)
    }
    pub fn owner_install(
        &mut self,
        request_id: u64,
        key: [u8; 32],
        body_digest: [u8; 32],
        request: &AgentOwnerInstall,
    ) -> Result<AgentOwnerInstalled, AgentBoundaryError> {
        let mut writer = Writer::new(OWNER_INSTALL);
        writer.u64(request_id);
        writer.fixed(&key);
        writer.fixed(&body_digest);
        encode_owner(&mut writer, request)?;
        let mut reader = self.exchange_secret(writer.finish_secret())?;
        let value = AgentOwnerInstalled {
            token_id: reader.fixed()?,
            session_id: reader.fixed()?,
            expiry_sequence: reader.u64()?,
            observed_head_sequence: reader.u64()?,
        };
        reader.finish()?;
        Ok(value)
    }
    fn owner_exchange(
        &mut self,
        operation: u8,
        mutation: Option<(u64, [u8; 32], [u8; 32])>,
        request: &AgentOwnerInstall,
    ) -> Result<AgentOwnerValidation, AgentBoundaryError> {
        let mut writer = Writer::new(operation);
        if let Some((id, key, digest)) = mutation {
            writer.u64(id);
            writer.fixed(&key);
            writer.fixed(&digest);
        }
        encode_owner(&mut writer, request)?;
        let mut reader = self.exchange(writer.finish())?;
        let value = AgentOwnerValidation {
            identity_head_sequence: reader.u64()?,
            expiry_sequence: reader.u64()?,
            observed_head_sequence: reader.u64()?,
            canonical_identity: reader.bytes()?,
        };
        reader.finish()?;
        Ok(value)
    }
    pub fn account_sequence(
        &mut self,
        actor: &layerx_agent_api::identity::AgentDid,
        authority: &layerx_agent_api::identity::AuthorityRef,
    ) -> Result<u64, AgentBoundaryError> {
        let mut writer = Writer::new(ACCOUNT_SEQUENCE);
        writer.text(actor.as_str())?;
        writer.text(authority.as_str())?;
        let mut reader = self.exchange(writer.finish())?;
        let sequence = reader.u64()?;
        reader.finish()?;
        Ok(sequence)
    }
    pub fn balance(&mut self) -> Result<VerifiedBalance, AgentBoundaryError> {
        let mut reader = self.exchange(Writer::new(BALANCE).finish())?;
        let value = VerifiedBalance {
            account: reader.fixed()?,
            asset: reader.fixed()?,
            currency: reader.text()?,
            observed_at: reader.text()?,
            age_seconds: reader.u64()?,
            amount: reader.u128()?,
            verification: reader.u8()?,
            global_sequence: reader.u64()?,
            batch_number: reader.u64()?,
            observed_head_sequence: reader.u64()?,
            observed_checkpoint: reader.fixed()?,
            canonical_bytes: reader.bytes()?,
            proof_material: reader.bytes()?,
        };
        if value.verification < 4
            || value.account == [0; 32]
            || value.asset == [0; 32]
            || value.currency.is_empty()
            || value.observed_at.is_empty()
            || value.canonical_bytes.is_empty()
            || value.proof_material.is_empty()
        {
            return Err(AgentBoundaryError::CorruptResponse);
        }
        reader.finish()?;
        Ok(value)
    }

    pub fn head(&mut self) -> Result<AgentHead, AgentBoundaryError> {
        let mut reader = self.exchange(Writer::new(HEAD).finish())?;
        let value = AgentHead {
            chain_sequence: reader.u64()?,
            sealed_batch: reader.u64()?,
            finalised_checkpoint: reader.fixed()?,
        };
        reader.finish()?;
        Ok(value)
    }

    pub fn evidence(
        &mut self,
        idempotency_key: [u8; 32],
        expected_activity_id: [u8; 32],
    ) -> Result<ReceiptLookup, AgentBoundaryError> {
        let mut writer = Writer::new(EVIDENCE);
        writer.fixed(&idempotency_key);
        writer.fixed(&expected_activity_id);
        let mut reader = self.exchange(writer.finish())?;
        let found = reader.u8()?;
        let value = match found {
            0 => ReceiptLookup::Absent,
            1 => ReceiptLookup::Found(decode_receipt(&mut reader)?),
            _ => return Err(AgentBoundaryError::CorruptResponse),
        };
        reader.finish()?;
        Ok(value)
    }
    /// Repeats the authenticated registry negotiation as a live readiness
    /// probe; a lockable adapter is not itself evidence that agentd is alive.
    pub fn probe(&self) -> Result<(), AgentBoundaryError> {
        let mut runtime = Self::connect(&self.endpoint, self.limits)?;
        let head = runtime.head()?;
        if head.finalised_checkpoint == [0; 32] {
            return Err(AgentBoundaryError::CorruptResponse);
        }
        Ok(())
    }

    /// Connects to the authenticated agent peer and adopts only its
    /// core-negotiated module registry.
    pub fn connect(endpoint: impl AsRef<Path>, limits: Limits) -> Result<Self, AgentBoundaryError> {
        let empty = ModuleRegistry::new(&[]).map_err(|_| AgentBoundaryError::Refused)?;
        let mut runtime = Self::new(endpoint, limits, empty)?;
        let mut reader = runtime.exchange(Writer::new(REGISTRY).finish())?;
        let module_count = usize::from(reader.u16()?);
        if module_count == 0 || module_count > 32 {
            return Err(AgentBoundaryError::CorruptResponse);
        }
        let mut registrations = Vec::with_capacity(module_count);
        for _ in 0..module_count {
            let module = ModuleId::from_u16(reader.u16()?)
                .map_err(|_| AgentBoundaryError::CorruptResponse)?;
            let activity_count = usize::from(reader.u16()?);
            if activity_count == 0 || activity_count > 256 {
                return Err(AgentBoundaryError::CorruptResponse);
            }
            let mut activities = Vec::with_capacity(activity_count);
            for _ in 0..activity_count {
                let activity = ActivityType::from_u32(reader.u32()?)
                    .map_err(|_| AgentBoundaryError::CorruptResponse)?;
                activities.push(activity);
            }
            registrations.push(
                ModuleRegistration::new(module, &activities)
                    .map_err(|_| AgentBoundaryError::CorruptResponse)?,
            );
        }
        reader.finish()?;
        runtime.registry =
            ModuleRegistry::new(&registrations).map_err(|_| AgentBoundaryError::CorruptResponse)?;
        Ok(runtime)
    }

    #[must_use]
    pub fn registry(&self) -> &ModuleRegistry {
        &self.registry
    }
    /// Creates a runtime for an absolute agentd endpoint and a core-negotiated
    /// registry. The registry is used to derive disclosure from returned bytes.
    pub fn new(
        endpoint: impl AsRef<Path>,
        limits: Limits,
        registry: ModuleRegistry,
    ) -> Result<Self, AgentBoundaryError> {
        let endpoint = endpoint.as_ref();
        if !endpoint.is_absolute() || endpoint.as_os_str().is_empty() {
            return Err(AgentBoundaryError::Refused);
        }
        let limits = limits.validate().map_err(|_| AgentBoundaryError::Refused)?;
        Ok(Self {
            endpoint: endpoint.to_path_buf(),
            gate: ConnectionGate::new(limits.maximum_connections),
            limits,
            registry,
        })
    }

    fn exchange(&self, request: Vec<u8>) -> Result<Reader, AgentBoundaryError> {
        let mut transport = Uds::connect(&self.endpoint, &self.gate, self.limits)
            .map_err(|_| AgentBoundaryError::Unavailable)?;
        transport
            .send(&request)
            .map_err(|_| AgentBoundaryError::Unavailable)?;
        let response = transport
            .receive()
            .map_err(|_| AgentBoundaryError::Unavailable)?;
        let mut reader = Reader::new(response);
        if reader.fixed::<8>()? != *MAGIC {
            return Err(AgentBoundaryError::CorruptResponse);
        }
        match reader.u8()? {
            0 => Ok(reader),
            1 => Err(AgentBoundaryError::Refused),
            2 => Err(AgentBoundaryError::Unavailable),
            _ => Err(AgentBoundaryError::CorruptResponse),
        }
    }

    fn exchange_secret(&self, request: Zeroizing<Vec<u8>>) -> Result<Reader, AgentBoundaryError> {
        let mut transport = Uds::connect(&self.endpoint, &self.gate, self.limits)
            .map_err(|_| AgentBoundaryError::Unavailable)?;
        transport
            .send(&request)
            .map_err(|_| AgentBoundaryError::Unavailable)?;
        let response = transport
            .receive()
            .map_err(|_| AgentBoundaryError::Unavailable)?;
        let mut reader = Reader::new(response);
        if reader.fixed::<8>()? != *MAGIC {
            return Err(AgentBoundaryError::CorruptResponse);
        }
        match reader.u8()? {
            0 => Ok(reader),
            1 => Err(AgentBoundaryError::Refused),
            2 => Err(AgentBoundaryError::Unavailable),
            _ => Err(AgentBoundaryError::CorruptResponse),
        }
    }

    fn encode_mutation_header<T>(operation: u8, mutation: &IdempotentMutation<T>) -> Writer {
        let mut writer = Writer::new(operation);
        writer.u64(mutation.request_id.0);
        writer.fixed(&mutation.key.bytes());
        writer.fixed(&mutation.body_digest.0);
        writer
    }

    fn decode_observation(
        &self,
        reader: &mut Reader,
    ) -> Result<AgentObservation, AgentBoundaryError> {
        let activity_id = reader.fixed::<32>()?;
        if activity_id == [0; 32] {
            return Err(AgentBoundaryError::CorruptResponse);
        }
        let submission = decode_tracked(reader)?;
        let receipt = decode_optional_receipt(reader)?;
        reader.finish()?;
        let executed = matches!(submission.state, SubmissionState::Executed { .. });
        if executed != receipt.is_some() {
            return Err(AgentBoundaryError::CorruptResponse);
        }
        Ok(AgentObservation {
            submission,
            activity_id,
            receipt,
        })
    }

    pub fn approval_list(
        &mut self,
        current_sequence: u64,
        cursor: Option<[u8; 32]>,
        limit: u8,
    ) -> Result<AgentApprovalPage, AgentBoundaryError> {
        let mut writer = Writer::new(APPROVAL_LIST);
        writer.u64(current_sequence);
        match cursor {
            Some(value) => {
                writer.u8(1);
                writer.fixed(&value);
            }
            None => writer.u8(0),
        }
        writer.u8(limit);
        let mut reader = self.exchange(writer.finish())?;
        let count = usize::from(reader.u8()?);
        if count > 100 {
            return Err(AgentBoundaryError::CorruptResponse);
        }
        let mut approvals = Vec::with_capacity(count);
        for _ in 0..count {
            approvals.push(decode_approval(&mut reader)?);
        }
        let next_cursor = match reader.u8()? {
            0 => None,
            1 => Some(reader.fixed()?),
            _ => return Err(AgentBoundaryError::CorruptResponse),
        };
        reader.finish()?;
        Ok(AgentApprovalPage {
            approvals,
            next_cursor,
        })
    }
    pub fn approval_get(
        &mut self,
        approval_id: [u8; 32],
        current_sequence: u64,
    ) -> Result<crate::approvals::AgentApprovalRecord, AgentBoundaryError> {
        let mut writer = Writer::new(APPROVAL_GET);
        writer.fixed(&approval_id);
        writer.u64(current_sequence);
        let mut reader = self.exchange(writer.finish())?;
        let value = decode_approval(&mut reader)?;
        reader.finish()?;
        Ok(value)
    }
    pub fn approval_decide(
        &mut self,
        approve: bool,
        approval_id: [u8; 32],
        held_digest: [u8; 32],
        idempotency_key: &str,
        current_sequence: u64,
    ) -> Result<crate::approvals::AgentDecision, AgentBoundaryError> {
        let mut writer = Writer::new(if approve {
            APPROVAL_APPROVE
        } else {
            APPROVAL_REJECT
        });
        writer.fixed(&approval_id);
        writer.fixed(&held_digest);
        writer.text(idempotency_key)?;
        writer.u64(current_sequence);
        let mut reader = self.exchange(writer.finish())?;
        let outcome = reader.u8()?;
        let submission_ref = match reader.u8()? {
            0 => None,
            1 => Some(reader.fixed()?),
            _ => return Err(AgentBoundaryError::CorruptResponse),
        };
        let winning = match reader.u8()? {
            0 => None,
            1 => Some(reader.u8()?),
            _ => return Err(AgentBoundaryError::CorruptResponse),
        };
        reader.finish()?;
        decision(outcome, submission_ref, winning)
    }
}

fn encode_owner(
    writer: &mut Writer,
    request: &AgentOwnerInstall,
) -> Result<(), AgentBoundaryError> {
    if request.permitted_activity_types.is_empty()
        || request.permitted_activity_types.len() > 256
        || request.scopes.is_empty()
        || request.scopes.len() > 32
    {
        return Err(AgentBoundaryError::Refused);
    }
    writer.text(&request.agent)?;
    writer.u8(request.authority_kind);
    writer.fixed(&request.authority_id);
    writer.fixed(&request.session_id);
    writer.fixed(&request.token_id);
    writer.fixed(&request.session_public_key);
    writer.bytes(&request.registration_payload)?;
    writer.fixed(&request.grantor);
    writer.u64(request.grant_not_before);
    writer.u64(request.grant_expires_at);
    writer.u64(request.grant_revocation_sequence);
    match &request.session_seed {
        Some(seed) => writer.fixed(seed.expose()),
        None => writer.fixed(&[0; 32]),
    };
    writer.u16(
        u16::try_from(request.permitted_activity_types.len())
            .map_err(|_| AgentBoundaryError::Refused)?,
    );
    for value in &request.permitted_activity_types {
        writer.u16(*value);
    }
    writer.u8(u8::try_from(request.scopes.len()).map_err(|_| AgentBoundaryError::Refused)?);
    for scope in &request.scopes {
        writer.text(scope)?;
    }
    writer.u64(request.lease_not_before_unix_ms);
    writer.u64(request.lease_not_after_unix_ms);
    writer.text(&request.opening_client)?;
    writer.text(&request.policy_version)?;
    match &request.lifecycle {
        None => writer.u8(0),
        Some(value) => {
            encode_lifecycle(writer, value)?;
        }
    }
    Ok(())
}

fn encode_finalization(
    writer: &mut Writer,
    value: AgentFinalizationEvidence,
) -> Result<(), AgentBoundaryError> {
    if value.action_key == [0; 32]
        || value.activity_id == [0; 32]
        || value.receipt_digest == [0; 32]
        || value.observed_sequence == 0
        || !(4..=5).contains(&value.verification)
        || value.finalized_at == 0
    {
        return Err(AgentBoundaryError::Refused);
    }
    writer.fixed(&value.action_key);
    writer.fixed(&value.activity_id);
    writer.fixed(&value.receipt_digest);
    writer.u64(value.observed_sequence);
    writer.u8(value.verification);
    writer.u64(value.finalized_at);
    Ok(())
}

fn encode_lifecycle(
    writer: &mut Writer,
    value: &AgentLifecycleSeed,
) -> Result<(), AgentBoundaryError> {
    if value.monthly_limit == 0
        || value.period_end <= value.period_start
        || value.verified_evidence.is_empty()
        || value.verified_evidence.len() > MAX_EVIDENCE
        || value.activity_types.is_empty()
        || value.counterparties.is_empty()
        || value.assets.is_empty()
        || value.purposes.is_empty()
        || value.session_scopes.is_empty()
        || value.creation_receipt_roots.is_empty()
        || value.custody_public_key == [0; 32]
        || value.budget_asset == [0; 32]
        || value.purpose_hash == [0; 32]
        || value.recovery_root == [0; 32]
        || value.recovery_threshold == 0
        || value.capability_id == [0; 32]
        || value.protocol_grant_id == [0; 32]
        || value.network_id == 0
    {
        return Err(AgentBoundaryError::Refused);
    }
    writer.u8(1);
    for text in [
        &value.agent_id,
        &value.name,
        &value.purpose,
        &value.currency,
    ] {
        writer.text(text)?;
    }
    writer.u128(value.monthly_limit);
    writer.u64(value.period_start);
    writer.u64(value.period_end);
    writer.u64(value.created_at);
    writer.u64(value.updated_at);
    writer.u16(
        u16::try_from(value.verified_evidence.len()).map_err(|_| AgentBoundaryError::Refused)?,
    );
    for item in &value.verified_evidence {
        writer.fixed(item);
    }
    for text in [&value.actor, &value.primary_authority, &value.custody_key] {
        writer.text(text)?;
    }
    writer.fixed(&value.custody_public_key);
    writer.text(&value.owner_account)?;
    writer.text(&value.budget_account)?;
    writer.fixed(&value.budget_asset);
    writer.fixed(&value.purpose_hash);
    writer.fixed(&value.recovery_root);
    writer.u16(value.recovery_threshold);
    writer.fixed(&value.capability_id);
    writer.u16(u16::try_from(value.activity_types.len()).map_err(|_| AgentBoundaryError::Refused)?);
    for item in &value.activity_types {
        writer.u32(*item);
    }
    for values in [&value.counterparties, &value.assets] {
        writer.u16(u16::try_from(values.len()).map_err(|_| AgentBoundaryError::Refused)?);
        for item in values {
            writer.fixed(item);
        }
    }
    writer.u128(value.amount_ceiling);
    writer.u64(value.rate_maximum_uses);
    writer.u64(value.rate_window_sequences);
    writer.u16(u16::try_from(value.purposes.len()).map_err(|_| AgentBoundaryError::Refused)?);
    for item in &value.purposes {
        writer.text(item)?;
    }
    writer.u64(value.capability_expiry_sequence);
    writer.u16(u16::try_from(value.session_scopes.len()).map_err(|_| AgentBoundaryError::Refused)?);
    for item in &value.session_scopes {
        writer.text(item)?;
    }
    writer.u64(value.session_expiry_unix_seconds);
    writer.fixed(&value.protocol_grant_id);
    writer.u64(value.budget_period_seconds);
    writer.u64(value.budget_expiry_seconds);
    writer.u128(value.initial_funding);
    writer.u32(value.network_id);
    writer.u16(
        u16::try_from(value.creation_receipt_roots.len())
            .map_err(|_| AgentBoundaryError::Refused)?,
    );
    for item in &value.creation_receipt_roots {
        writer.fixed(item);
    }
    Ok(())
}

fn decode_lifecycle(reader: &mut Reader) -> Result<AgentLifecycleSeed, AgentBoundaryError> {
    let agent_id = reader.text()?;
    let name = reader.text()?;
    let purpose = reader.text()?;
    let currency = reader.text()?;
    let monthly_limit = reader.u128()?;
    let period_start = reader.u64()?;
    let period_end = reader.u64()?;
    let created_at = reader.u64()?;
    let updated_at = reader.u64()?;
    let verified_evidence = read_fixed_list(reader, 64)?;
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
    let activity_types = read_u32_list(reader, 256)?;
    let counterparties = read_fixed_list(reader, 256)?;
    let assets = read_fixed_list(reader, 256)?;
    let amount_ceiling = reader.u128()?;
    let rate_maximum_uses = reader.u64()?;
    let rate_window_sequences = reader.u64()?;
    let purposes = read_text_list(reader, 64)?;
    let capability_expiry_sequence = reader.u64()?;
    let session_scopes = read_text_list(reader, 64)?;
    let session_expiry_unix_seconds = reader.u64()?;
    let protocol_grant_id = reader.fixed()?;
    let budget_period_seconds = reader.u64()?;
    let budget_expiry_seconds = reader.u64()?;
    let initial_funding = reader.u128()?;
    let network_id = reader.u32()?;
    let creation_receipt_roots = read_fixed_list(reader, 64)?;
    let value = AgentLifecycleSeed {
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
    if value.monthly_limit == 0
        || value.period_end <= value.period_start
        || value.custody_public_key == [0; 32]
        || value.budget_asset == [0; 32]
        || value.purpose_hash == [0; 32]
        || value.recovery_root == [0; 32]
        || value.recovery_threshold == 0
        || value.capability_id == [0; 32]
        || value.protocol_grant_id == [0; 32]
        || value.network_id == 0
    {
        return Err(AgentBoundaryError::CorruptResponse);
    }
    Ok(value)
}
fn read_fixed_list(reader: &mut Reader, max: usize) -> Result<Vec<[u8; 32]>, AgentBoundaryError> {
    let n = usize::from(reader.u16()?);
    if n == 0 || n > max {
        return Err(AgentBoundaryError::CorruptResponse);
    }
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        v.push(reader.fixed()?)
    }
    Ok(v)
}
fn read_u16_list(reader: &mut Reader, max: usize) -> Result<Vec<u16>, AgentBoundaryError> {
    let n = usize::from(reader.u16()?);
    if n == 0 || n > max {
        return Err(AgentBoundaryError::CorruptResponse);
    }
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        v.push(reader.u16()?)
    }
    Ok(v)
}
fn read_u32_list(reader: &mut Reader, max: usize) -> Result<Vec<u32>, AgentBoundaryError> {
    let n = usize::from(reader.u16()?);
    if n == 0 || n > max {
        return Err(AgentBoundaryError::CorruptResponse);
    }
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        v.push(reader.u32()?)
    }
    Ok(v)
}
fn read_text_list(reader: &mut Reader, max: usize) -> Result<Vec<String>, AgentBoundaryError> {
    let n = usize::from(reader.u16()?);
    if n == 0 || n > max {
        return Err(AgentBoundaryError::CorruptResponse);
    }
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        v.push(reader.text()?)
    }
    Ok(v)
}

fn decode_managed_evidence(
    reader: &mut Reader,
) -> Result<Vec<ManagedAgentEvidence>, AgentBoundaryError> {
    let count = usize::from(reader.u8()?);
    if count > MAX_EVIDENCE {
        return Err(AgentBoundaryError::CorruptResponse);
    }
    let mut evidence = Vec::with_capacity(count);
    for _ in 0..count {
        let value = ManagedAgentEvidence {
            evidence_id: reader.text()?,
            class: reader.text()?,
            verification: reader.u8()?,
        };
        if value.evidence_id.is_empty() || value.class.is_empty() || value.verification > 5 {
            return Err(AgentBoundaryError::CorruptResponse);
        }
        evidence.push(value);
    }
    Ok(evidence)
}
fn decode_managed_agent(reader: &mut Reader) -> Result<ManagedAgentView, AgentBoundaryError> {
    let value = ManagedAgentView {
        agent_id: reader.text()?,
        name: reader.text()?,
        purpose: reader.text()?,
        state: reader.u8()?,
        monthly_limit: reader.u128()?,
        currency: reader.text()?,
        limit_enforcement: reader.u8()?,
        period_start: reader.text()?,
        period_end: reader.text()?,
        spent: reader.u128()?,
        remaining: reader.u128()?,
        spend_verification: reader.u8()?,
        created_at: reader.text()?,
        updated_at: reader.text()?,
        evidence: decode_managed_evidence(reader)?,
    };
    if value.agent_id.is_empty()
        || value.name.is_empty()
        || value.purpose.is_empty()
        || value.state > 4
        || value.monthly_limit == 0
        || value.currency.is_empty()
        || value.limit_enforcement > 1
        || value.period_start.is_empty()
        || value.period_end.is_empty()
        || value.spend_verification > 5
        || value.created_at.is_empty()
        || value.updated_at.is_empty()
        || value.spent.checked_add(value.remaining).is_none()
    {
        return Err(AgentBoundaryError::CorruptResponse);
    }
    Ok(value)
}
fn decode_managed_journey(reader: &mut Reader) -> Result<ManagedAgentJourney, AgentBoundaryError> {
    let journey_id = reader.text()?;
    let kind = reader.text()?;
    let state = reader.u8()?;
    let count = usize::from(reader.u8()?);
    if count == 0 || count > 16 {
        return Err(AgentBoundaryError::CorruptResponse);
    }
    let mut stages = Vec::with_capacity(count);
    for _ in 0..count {
        let stage = ManagedAgentJourneyStage {
            stage_id: reader.text()?,
            copy_key: reader.text()?,
            state: reader.u8()?,
            evidence: decode_managed_evidence(reader)?,
        };
        if stage.stage_id.is_empty() || stage.copy_key.is_empty() || stage.state > 3 {
            return Err(AgentBoundaryError::CorruptResponse);
        }
        stages.push(stage);
    }
    let value = ManagedAgentJourney {
        journey_id,
        kind,
        state,
        stages,
        started_at: reader.text()?,
        updated_at: reader.text()?,
        evidence: decode_managed_evidence(reader)?,
    };
    if value.journey_id.is_empty()
        || value.kind.is_empty()
        || value.state > 3
        || value.started_at.is_empty()
        || value.updated_at.is_empty()
    {
        return Err(AgentBoundaryError::CorruptResponse);
    }
    Ok(value)
}
fn decode_managed_challenge(
    reader: &mut Reader,
) -> Result<ManagedAgentChallenge, AgentBoundaryError> {
    let value = ManagedAgentChallenge {
        agent_id: reader.text()?,
        kind: reader.u8()?,
        delay_seconds: reader.u64()?,
        ready_at: reader.text()?,
        evidence: decode_managed_evidence(reader)?,
    };
    if value.agent_id.is_empty()
        || value.kind > 1
        || value.delay_seconds == 0
        || value.ready_at.is_empty()
    {
        return Err(AgentBoundaryError::CorruptResponse);
    }
    Ok(value)
}

fn decode_approval(
    reader: &mut Reader,
) -> Result<crate::approvals::AgentApprovalRecord, AgentBoundaryError> {
    use layerx_agent_api::identity::{
        ActivityType as ApiActivityType, AgentDid, Asset, AuthorityRef, ExplicitSet,
    };
    use layerx_agent_api::prepare::{DisclosedAmount, Disclosure, IdempotencyRef};
    let approval_id = reader.fixed()?;
    let canonical_digest = reader.fixed()?;
    let activity_type = ApiActivityType(reader.u16()?);
    let actor = AgentDid::new(reader.text()?).map_err(|_| AgentBoundaryError::CorruptResponse)?;
    let authority =
        AuthorityRef::new(reader.text()?).map_err(|_| AgentBoundaryError::CorruptResponse)?;
    let counterpart_count = usize::from(reader.u16()?);
    if counterpart_count > 64 {
        return Err(AgentBoundaryError::CorruptResponse);
    }
    let mut counterparties = Vec::with_capacity(counterpart_count);
    for _ in 0..counterpart_count {
        counterparties
            .push(AgentDid::new(reader.text()?).map_err(|_| AgentBoundaryError::CorruptResponse)?);
    }
    let amount_count = usize::from(reader.u16()?);
    if amount_count > 64 {
        return Err(AgentBoundaryError::CorruptResponse);
    }
    let mut amounts = Vec::with_capacity(amount_count);
    for _ in 0..amount_count {
        amounts.push(DisclosedAmount {
            counterparty: AgentDid::new(reader.text()?)
                .map_err(|_| AgentBoundaryError::CorruptResponse)?,
            amount: layerx_agent_api::Amount(reader.u128()?),
        });
    }
    let held_activity = Disclosure {
        canonical_digest,
        activity_type,
        actor,
        authority,
        counterparties: ExplicitSet::allow(counterparties),
        amounts: ExplicitSet::allow(amounts),
        asset: Asset::new(reader.text()?).map_err(|_| AgentBoundaryError::CorruptResponse)?,
        fee_limit: layerx_agent_api::Amount(reader.u128()?),
        expiry: TimestampSeconds(reader.u64()?),
        idempotency_key: IdempotencyRef::new(reader.text()?)
            .map_err(|_| AgentBoundaryError::CorruptResponse)?,
    };
    let canonical_bytes_digest = reader.fixed()?;
    let hold_reason_code = reader.text()?;
    let hold_reason = reader.text()?;
    let created_at_sequence = reader.u64()?;
    let expires_at_sequence = reader.u64()?;
    let state = match reader.u8()? {
        0 => crate::approvals::AgentApprovalState::AwaitingApproval,
        1 => crate::approvals::AgentApprovalState::Approved {
            submission_ref: reader.fixed()?,
        },
        2 => crate::approvals::AgentApprovalState::Rejected,
        3 => crate::approvals::AgentApprovalState::Expired,
        4 => crate::approvals::AgentApprovalState::Defective,
        _ => return Err(AgentBoundaryError::CorruptResponse),
    };
    Ok(crate::approvals::AgentApprovalRecord {
        approval_id,
        held_activity,
        canonical_bytes_digest,
        hold_reason_code,
        hold_reason,
        created_at_sequence,
        expires_at_sequence,
        state,
    })
}
fn decision(
    outcome: u8,
    submission_ref: Option<[u8; 32]>,
    winning: Option<u8>,
) -> Result<crate::approvals::AgentDecision, AgentBoundaryError> {
    use crate::approvals::{AgentDecision, AgentDecisionResolution, AgentDecisionStatus};
    let effective = if matches!(outcome, 4 | 5) {
        winning.ok_or(AgentBoundaryError::CorruptResponse)?
    } else {
        outcome
    };
    let status = match effective {
        0 => AgentDecisionStatus::Approved { submission_ref },
        1 => AgentDecisionStatus::Rejected,
        2 => AgentDecisionStatus::Expired,
        3 => AgentDecisionStatus::Defective,
        _ => return Err(AgentBoundaryError::CorruptResponse),
    };
    Ok(AgentDecision {
        status,
        resolution: if matches!(outcome, 4 | 5) {
            AgentDecisionResolution::AlreadyDecided
        } else {
            AgentDecisionResolution::Applied
        },
    })
}

impl AgentBoundary for AgentRuntime {
    fn prepare(
        &mut self,
        call: &Call<IdempotentMutation<PrepareRequest>>,
    ) -> Result<AgentPreparation, AgentBoundaryError> {
        let mutation = call.request();
        let request = &mutation.operation;
        let mut writer = Self::encode_mutation_header(PREPARE, mutation);
        writer.u32(request.protocol_activity_type);
        writer.text(request.actor.as_str())?;
        writer.text(request.authority.as_str())?;
        writer.u64(request.account_sequence.0);
        writer.u64(request.timestamp_bound.not_before.0);
        writer.u64(request.timestamp_bound.not_after.0);
        writer.text(request.idempotency_key.as_str())?;
        writer.u128(request.fee_limit.0);
        writer.bytes(request.payload.as_bytes())?;
        writer.fixed(&request.payload_hash);

        let mut reader = self.exchange(writer.finish())?;
        let preparation_ref =
            PreparationRef::new(reader.text()?).map_err(|_| AgentBoundaryError::CorruptResponse)?;
        let unsigned_canonical_bytes = reader.bytes()?;
        let signing_preimage = reader.bytes()?;
        let activity_type = ActivityType::from_u32(reader.u32()?)
            .map_err(|_| AgentBoundaryError::CorruptResponse)?;
        let actor = layerx_agent_api::identity::AgentDid::new(reader.text()?)
            .map_err(|_| AgentBoundaryError::CorruptResponse)?;
        let authority = layerx_agent_api::identity::AuthorityRef::new(reader.text()?)
            .map_err(|_| AgentBoundaryError::CorruptResponse)?;
        let account_sequence = reader.u64()?;
        let not_before = reader.u64()?;
        let not_after = reader.u64()?;
        let fee_limit = reader.u128()?;
        let payload = reader.bytes()?;
        let payload_hash = reader.fixed::<32>()?;
        let idempotency_key = reader.fixed::<32>()?;
        reader.finish()?;

        let disclosure = disclosure::bind(&unsigned_canonical_bytes, &self.registry)
            .map_err(|_| AgentBoundaryError::CorruptResponse)?;
        if disclosure
            .reencode()
            .map_err(|_| AgentBoundaryError::CorruptResponse)?
            != unsigned_canonical_bytes
        {
            return Err(AgentBoundaryError::CorruptResponse);
        }
        Ok(AgentPreparation {
            preparation_ref,
            unsigned_canonical_bytes,
            signing_preimage,
            disclosure,
            actor,
            authority,
            account_sequence,
            not_before,
            not_after,
            fee_limit,
            activity_type,
            payload,
            payload_hash,
            idempotency_key,
        })
    }

    fn submit(
        &mut self,
        call: &Call<IdempotentMutation<SubmitRequest>>,
        signer_public_key: [u8; 32],
    ) -> Result<AgentObservation, AgentBoundaryError> {
        let mutation = call.request();
        let mut writer = Self::encode_mutation_header(SUBMIT, mutation);
        writer.text(mutation.operation.preparation_ref.as_str())?;
        writer.bytes(mutation.operation.signature.as_bytes())?;
        writer.fixed(&signer_public_key);
        match mutation.operation.approval_release_ref {
            Some(reference) => {
                writer.u8(1);
                writer.fixed(&reference);
            }
            None => writer.u8(0),
        }
        let mut reader = self.exchange(writer.finish())?;
        self.decode_observation(&mut reader)
    }

    fn track(&mut self, call: &Call<TrackRequest>) -> Result<AgentObservation, AgentBoundaryError> {
        let mut writer = Writer::new(TRACK);
        writer.text(call.request().submission_ref.as_str())?;
        let mut reader = self.exchange(writer.finish())?;
        self.decode_observation(&mut reader)
    }

    fn receipt_by_idempotency_key(
        &mut self,
        idempotency_key: [u8; 32],
        expected_activity_id: [u8; 32],
    ) -> Result<ReceiptLookup, AgentBoundaryError> {
        let mut writer = Writer::new(RECEIPT_LOOKUP);
        writer.fixed(&idempotency_key);
        writer.fixed(&expected_activity_id);
        let mut reader = self.exchange(writer.finish())?;
        let found = reader.u8()?;
        let result = match found {
            0 => ReceiptLookup::Absent,
            1 => ReceiptLookup::Found(decode_receipt(&mut reader)?),
            _ => return Err(AgentBoundaryError::CorruptResponse),
        };
        reader.finish()?;
        Ok(result)
    }
}

impl DepositAgentBoundary for AgentRuntime {
    fn credit_receipt(
        &mut self,
        action_key: [u8; 32],
        activity_id: [u8; 32],
    ) -> Result<ReceiptMaterial, AgentBoundaryError> {
        match self.receipt_by_idempotency_key(action_key, activity_id)? {
            ReceiptLookup::Found(material) => Ok(material),
            ReceiptLookup::Absent => Err(AgentBoundaryError::Unavailable),
        }
    }
}

impl crate::approvals::ApprovalBoundary for AgentRuntime {
    fn approval(
        &mut self,
        approval_id: [u8; 32],
        at_sequence: u64,
    ) -> Result<crate::approvals::AgentApprovalRecord, crate::approvals::ApprovalBoundaryError>
    {
        self.approval_get(approval_id, at_sequence)
            .map_err(map_approval_error)
    }
    fn verified_budget_after(
        &mut self,
        hold: &crate::approvals::AgentApprovalRecord,
        at_sequence: u64,
    ) -> Result<crate::approvals::VerifiedBudgetAfter, crate::approvals::ApprovalBoundaryError>
    {
        let balance = self.balance().map_err(map_approval_error)?;
        if balance.currency != hold.held_activity.asset.as_str()
            || balance.global_sequence < at_sequence
        {
            return Err(crate::approvals::ApprovalBoundaryError::VerificationFailed);
        }
        let level = match balance.verification {
            0 => Level::Unverified,
            1 => Level::SequencerSigned,
            2 => Level::BatchIncluded,
            3 => Level::StateProven,
            4 => Level::CheckpointFinalised,
            5 => Level::SettlementAnchored,
            _ => return Err(crate::approvals::ApprovalBoundaryError::Corrupt),
        };
        let mut digest = sha2::Sha256::new();
        digest.update(&balance.canonical_bytes);
        digest.update(&balance.proof_material);
        Ok(crate::approvals::VerifiedBudgetAfter {
            remaining: balance.amount,
            level,
            evidence_digest: digest.finalize().into(),
            observed_at_sequence: balance.global_sequence,
        })
    }
    fn track_released(
        &mut self,
        submission_ref: [u8; 32],
    ) -> Result<TrackedSubmission, crate::approvals::ApprovalBoundaryError> {
        let mut writer = Writer::new(TRACK);
        writer
            .text(&hex(&submission_ref))
            .map_err(map_approval_error)?;
        let mut reader = self.exchange(writer.finish()).map_err(map_approval_error)?;
        self.decode_observation(&mut reader)
            .map(|value| value.submission)
            .map_err(map_approval_error)
    }
}
impl crate::approvals::AgentDecisionBoundary for AgentRuntime {
    fn approve(
        &mut self,
        approval_id: [u8; 32],
        held_digest: [u8; 32],
        idempotency_key: &str,
        current_sequence: u64,
    ) -> Result<crate::approvals::AgentDecision, crate::approvals::ApprovalBoundaryError> {
        self.approval_decide(
            true,
            approval_id,
            held_digest,
            idempotency_key,
            current_sequence,
        )
        .map_err(map_approval_error)
    }
    fn reject(
        &mut self,
        approval_id: [u8; 32],
        held_digest: [u8; 32],
        idempotency_key: &str,
        current_sequence: u64,
    ) -> Result<crate::approvals::AgentDecision, crate::approvals::ApprovalBoundaryError> {
        self.approval_decide(
            false,
            approval_id,
            held_digest,
            idempotency_key,
            current_sequence,
        )
        .map_err(map_approval_error)
    }
}
fn map_approval_error(error: AgentBoundaryError) -> crate::approvals::ApprovalBoundaryError {
    match error {
        AgentBoundaryError::Unavailable => crate::approvals::ApprovalBoundaryError::Unavailable,
        AgentBoundaryError::Refused => crate::approvals::ApprovalBoundaryError::NotFound,
        AgentBoundaryError::CorruptResponse => crate::approvals::ApprovalBoundaryError::Corrupt,
    }
}
fn hex(bytes: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|byte| {
            [
                H[(byte >> 4) as usize] as char,
                H[(byte & 15) as usize] as char,
            ]
        })
        .collect()
}

fn decode_tracked(reader: &mut Reader) -> Result<TrackedSubmission, AgentBoundaryError> {
    let submission_ref =
        SubmissionRef::new(reader.text()?).map_err(|_| AgentBoundaryError::CorruptResponse)?;
    let state = match reader.u8()? {
        0 => SubmissionState::Prepared,
        1 => SubmissionState::Signed,
        2 => SubmissionState::Queued,
        3 => SubmissionState::Submitted,
        4 => SubmissionState::Acknowledged,
        5 => SubmissionState::Unknown,
        6 => SubmissionState::Executed {
            receipt_ref: ReceiptRef::new(reader.text()?)
                .map_err(|_| AgentBoundaryError::CorruptResponse)?,
        },
        7 => SubmissionState::Failed {
            result: ResultCode::from_raw(reader.i32()?),
        },
        8 => SubmissionState::Expired,
        _ => return Err(AgentBoundaryError::CorruptResponse),
    };
    let verification_level = decode_level(reader.u8()?)?;
    let evidence_count = usize::from(reader.u8()?);
    if evidence_count > MAX_EVIDENCE {
        return Err(AgentBoundaryError::CorruptResponse);
    }
    let mut evidence = Vec::with_capacity(evidence_count);
    for _ in 0..evidence_count {
        evidence.push(EvidenceRef {
            kind: reader.text()?,
            digest: reader.fixed::<32>()?,
        });
    }
    let transition_count = usize::from(reader.u8()?);
    if transition_count > MAX_TRANSITIONS {
        return Err(AgentBoundaryError::CorruptResponse);
    }
    let mut transitions = Vec::with_capacity(transition_count);
    for _ in 0..transition_count {
        let from = decode_state_without_data(reader.u8()?)?;
        let to = decode_state_without_data(reader.u8()?)?;
        transitions.push(
            Transition {
                from,
                to,
                cause: reader.text()?,
                at: TimestampSeconds(reader.u64()?),
            }
            .validate()
            .map_err(|_| AgentBoundaryError::CorruptResponse)?,
        );
    }
    Ok(TrackedSubmission {
        submission_ref,
        state,
        evidence,
        verification_level,
        transitions,
    })
}

fn decode_state_without_data(value: u8) -> Result<SubmissionState, AgentBoundaryError> {
    match value {
        0 => Ok(SubmissionState::Prepared),
        1 => Ok(SubmissionState::Signed),
        2 => Ok(SubmissionState::Queued),
        3 => Ok(SubmissionState::Submitted),
        4 => Ok(SubmissionState::Acknowledged),
        5 => Ok(SubmissionState::Unknown),
        8 => Ok(SubmissionState::Expired),
        _ => Err(AgentBoundaryError::CorruptResponse),
    }
}

fn decode_level(value: u8) -> Result<Level, AgentBoundaryError> {
    match value {
        0 => Ok(Level::Unverified),
        1 => Ok(Level::SequencerSigned),
        2 => Ok(Level::BatchIncluded),
        3 => Ok(Level::StateProven),
        4 => Ok(Level::CheckpointFinalised),
        5 => Ok(Level::SettlementAnchored),
        _ => Err(AgentBoundaryError::CorruptResponse),
    }
}

fn decode_optional_receipt(
    reader: &mut Reader,
) -> Result<Option<ReceiptMaterial>, AgentBoundaryError> {
    match reader.u8()? {
        0 => Ok(None),
        1 => decode_receipt(reader).map(Some),
        _ => Err(AgentBoundaryError::CorruptResponse),
    }
}

fn decode_receipt(reader: &mut Reader) -> Result<ReceiptMaterial, AgentBoundaryError> {
    let canonical_bytes = reader.bytes()?;
    let authorised_batch = AuthorizedBatch::new(
        reader.fixed::<32>()?,
        reader.fixed::<32>()?,
        reader.fixed::<32>()?,
        reader.fixed::<32>()?,
        reader.fixed::<32>()?,
    );
    let verification_level = match reader.u8()? {
        1 => VerificationLevel::SEQUENCER_SIGNED,
        2 => VerificationLevel::BATCH_INCLUDED,
        3 => VerificationLevel::STATE_PROVEN,
        4 => VerificationLevel::CHECKPOINT_FINALISED,
        5 => VerificationLevel::SETTLEMENT_ANCHORED,
        _ => return Err(AgentBoundaryError::CorruptResponse),
    };
    Ok(ReceiptMaterial {
        canonical_bytes,
        authorised_batch,
        verification_level,
    })
}

struct Writer(Zeroizing<Vec<u8>>);

impl Writer {
    fn new(operation: u8) -> Self {
        let mut bytes = Vec::with_capacity(256);
        bytes.extend_from_slice(MAGIC);
        bytes.push(operation);
        Self(Zeroizing::new(bytes))
    }
    fn u8(&mut self, value: u8) {
        self.0.push(value);
    }
    fn u16(&mut self, value: u16) {
        self.0.extend_from_slice(&value.to_be_bytes());
    }
    fn u64(&mut self, value: u64) {
        self.0.extend_from_slice(&value.to_be_bytes());
    }
    fn u32(&mut self, value: u32) {
        self.0.extend_from_slice(&value.to_be_bytes());
    }
    fn u128(&mut self, value: u128) {
        self.0.extend_from_slice(&value.to_be_bytes());
    }
    fn fixed(&mut self, value: &[u8]) {
        self.0.extend_from_slice(value);
    }
    fn text(&mut self, value: &str) -> Result<(), AgentBoundaryError> {
        if value.is_empty() || value.len() > MAX_TEXT {
            return Err(AgentBoundaryError::Refused);
        }
        self.bytes(value.as_bytes())
    }
    fn bytes(&mut self, value: &[u8]) -> Result<(), AgentBoundaryError> {
        let length = u32::try_from(value.len()).map_err(|_| AgentBoundaryError::Refused)?;
        if value.is_empty() || value.len() > MAX_BYTES {
            return Err(AgentBoundaryError::Refused);
        }
        self.u32(length);
        self.fixed(value);
        Ok(())
    }
    fn finish(mut self) -> Vec<u8> {
        std::mem::take(&mut *self.0)
    }
    fn finish_secret(mut self) -> Zeroizing<Vec<u8>> {
        Zeroizing::new(std::mem::take(&mut *self.0))
    }
}

struct Reader {
    bytes: Vec<u8>,
    offset: usize,
}

impl Reader {
    fn new(bytes: Vec<u8>) -> Self {
        Self { bytes, offset: 0 }
    }
    fn take(&mut self, length: usize) -> Result<&[u8], AgentBoundaryError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(AgentBoundaryError::CorruptResponse)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(AgentBoundaryError::CorruptResponse)?;
        self.offset = end;
        Ok(value)
    }
    fn u8(&mut self) -> Result<u8, AgentBoundaryError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, AgentBoundaryError> {
        Ok(u16::from_be_bytes(self.fixed()?))
    }
    fn u32(&mut self) -> Result<u32, AgentBoundaryError> {
        Ok(u32::from_be_bytes(self.fixed()?))
    }
    fn i32(&mut self) -> Result<i32, AgentBoundaryError> {
        Ok(i32::from_be_bytes(self.fixed()?))
    }
    fn u64(&mut self) -> Result<u64, AgentBoundaryError> {
        Ok(u64::from_be_bytes(self.fixed()?))
    }
    fn u128(&mut self) -> Result<u128, AgentBoundaryError> {
        Ok(u128::from_be_bytes(self.fixed()?))
    }
    fn fixed<const N: usize>(&mut self) -> Result<[u8; N], AgentBoundaryError> {
        self.take(N)?
            .try_into()
            .map_err(|_| AgentBoundaryError::CorruptResponse)
    }
    fn bytes(&mut self) -> Result<Vec<u8>, AgentBoundaryError> {
        let length =
            usize::try_from(self.u32()?).map_err(|_| AgentBoundaryError::CorruptResponse)?;
        if length == 0 || length > MAX_BYTES {
            return Err(AgentBoundaryError::CorruptResponse);
        }
        Ok(self.take(length)?.to_vec())
    }
    fn text(&mut self) -> Result<String, AgentBoundaryError> {
        let bytes = self.bytes()?;
        if bytes.len() > MAX_TEXT {
            return Err(AgentBoundaryError::CorruptResponse);
        }
        String::from_utf8(bytes).map_err(|_| AgentBoundaryError::CorruptResponse)
    }
    fn finish(&self) -> Result<(), AgentBoundaryError> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(AgentBoundaryError::CorruptResponse)
        }
    }
}

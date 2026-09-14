//! Production managed-agent creation adapter.

use layerx_agent_api::idempotency::{BodyDigest, IdempotentMutation, Key};
use layerx_agent_api::identity::{AgentDid, AuthorityRef};
use layerx_agent_api::prepare::{IdempotencyRef, PayloadBytes, PrepareRequest, TimestampBound};
use layerx_agent_api::submit::{SignatureBytes, SubmitRequest};
use layerx_agent_api::track::TrackRequest;
use layerx_agent_api::{Amount, Sequence, TimestampSeconds};
use layerx_crypto::local::LocalSigner;
use layerx_crypto::session::{issue_session_key, SessionKeyRequest};
use layerx_crypto::signer::Signer as _;
use layerx_intents::{Intent, IntentKind, SessionGrant};
use layerx_sdk::Client;
use layerx_types::payload::ModuleRegistry;
use layerx_types::verify::VerificationLevel;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::agents::{
    AgentCreationContract, AgentEvidence, AgentFailure, CapabilityProvision, CreationProjection,
    CreationStage, ProtocolAction, ProtocolEvidence, ScopedAgentCreationContract, SessionProvision,
};
use crate::custody::{CustodySigner, Operation, SignAuthorization, SignRequest};
use crate::journeys::{AgentBoundary, AgentBoundaryError, ReceiptLookup};
use crate::store::PrincipalScope;
use crate::trace::TraceId;

use super::agent_runtime::{
    AgentCapabilityInstall, AgentLifecycleSeed, AgentOwnerInstall, AgentRuntime,
};
use super::poll_once_ready;

const PREPARE_DOMAIN: &[u8] = b"layerx-human-journey-prepare/v1";
const SUBMIT_DOMAIN: &[u8] = b"layerx-human-journey-submit/v1";

#[derive(Clone, Copy)]
pub struct CreationBounds {
    pub timestamp_span: u64,
    pub fee_limit: u128,
}

#[derive(Serialize, Deserialize)]
struct SessionPreparation {
    version: u8,
    request_binding: [u8; 32],
    started_at: u64,
    lease_expires_at: u64,
    native_not_before: u64,
    native_expires_at: u64,
    revocation_sequence: u64,
    expiry_sequence: u64,
    fee: Option<crate::agents::NativeFeeConsent>,
    replacement: Option<([u8; 32], [u8; 32])>,
}

pub struct ProductionAgentCreation<'a> {
    runtime: &'a mut AgentRuntime,
    client: &'a Client,
    custody: &'a CustodySigner,
    trace: &'a TraceId,
    actor: AgentDid,
    authority: AuthorityRef,
    timestamp_span: u64,
    fee_limit: u128,
    owner_primary_key: Option<[u8; 32]>,
    latest_session_credential: Option<(zeroize::Zeroizing<[u8; 32]>, u64)>,
}

impl<'a> ProductionAgentCreation<'a> {
    ///
    /// # Errors
    ///
    /// Refuses invalid protocol evidence, custody authorization, or unavailable agent state.
    pub fn new(
        runtime: &'a mut AgentRuntime,
        client: &'a Client,
        custody: &'a CustodySigner,
        trace: &'a TraceId,
        actor: AgentDid,
        authority: AuthorityRef,
        bounds: CreationBounds,
    ) -> Result<Self, AgentFailure> {
        let CreationBounds {
            timestamp_span,
            fee_limit,
        } = bounds;
        if timestamp_span == 0 {
            return Err(AgentFailure::Refused("invalid creation preparation bounds"));
        }
        Ok(Self {
            runtime,
            client,
            custody,
            trace,
            actor,
            authority,
            timestamp_span,
            fee_limit,
            owner_primary_key: None,
            latest_session_credential: None,
        })
    }

    ///
    /// # Errors
    ///
    /// Refuses invalid protocol evidence, custody authorization, or unavailable agent state.
    pub fn take_latest_session_credential(&mut self) -> Result<([u8; 32], u64), AgentFailure> {
        self.latest_session_credential
            .take()
            .map(|(token, generation)| (*token, generation))
            .ok_or(AgentFailure::Refused("session token was not provisioned"))
    }

    fn prepare_action(
        &mut self,
        action: &ProtocolAction,
    ) -> Result<crate::journeys::AgentPreparation, AgentFailure> {
        let account_sequence = self
            .runtime
            .account_sequence(&self.actor, &self.authority)
            .map_err(map_boundary)?;
        let not_before = action
            .started_at
            .checked_mul(1_000)
            .ok_or(AgentFailure::Refused("creation preparation bound overflow"))?;
        let not_after = action
            .started_at
            .checked_add(self.timestamp_span)
            .and_then(|value| value.checked_mul(1_000))
            .ok_or(AgentFailure::Refused("creation preparation bound overflow"))?;
        let request = PrepareRequest {
            protocol_activity_type: action.compiled.activity_type().value(),
            actor: self.actor.clone(),
            authority: self.authority.clone(),
            account_sequence: Sequence(account_sequence),
            timestamp_bound: TimestampBound {
                not_before: TimestampSeconds(not_before),
                not_after: TimestampSeconds(not_after),
            }
            .validate()
            .map_err(|_| AgentFailure::Refused("invalid creation preparation bounds"))?,
            idempotency_key: IdempotencyRef::new(hex(&action.action_key))
                .map_err(|_| AgentFailure::Refused("invalid creation action key"))?,
            fee_limit: Amount(self.fee_limit),
            payload: PayloadBytes::new(action.compiled.payload().as_bytes().to_vec())
                .map_err(|_| AgentFailure::Refused("invalid compiled creation payload"))?,
            payload_hash: action.compiled.payload_hash(),
        };
        let prepared = self
            .runtime
            .prepare(&self.client.prepare(mutation(
                action.action_key,
                prepare_digest(&request),
                request,
            )?))
            .map_err(map_boundary)?;
        if prepared.activity_type != action.compiled.activity_type()
            || prepared.actor != self.actor
            || prepared.authority != self.authority
            || prepared.account_sequence != account_sequence
            || prepared.not_before != not_before
            || prepared.not_after != not_after
            || prepared.fee_limit != self.fee_limit
            || prepared.payload.as_slice() != action.compiled.payload().as_bytes()
            || prepared.payload_hash != action.compiled.payload_hash()
            || prepared.idempotency_key != action.action_key
            || prepared
                .disclosure
                .reencode()
                .map_err(|_| AgentFailure::Refused("invalid prepared disclosure"))?
                != prepared.unsigned_canonical_bytes
        {
            return Err(AgentFailure::Refused(
                "agent preparation differs from creation intent",
            ));
        }
        Ok(prepared)
    }

    fn submit_scoped(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        action: &ProtocolAction,
    ) -> Result<ProtocolEvidence, AgentFailure> {
        let prepared = self.prepare_action(action)?;
        let principal = scope.principal().clone();
        let descriptor = self
            .custody
            .describe_key(&principal, &action.custody_key)
            .map_err(|_| AgentFailure::Refused("agent custody key is unavailable"))?;
        let expected_class = if action.custody_key.as_str() == "human-primary" {
            crate::custody::KeyClass::HumanPrimary
        } else {
            crate::custody::KeyClass::AgentPrimary
        };
        if descriptor.class != expected_class {
            return Err(AgentFailure::Refused(
                "agent custody key has the wrong class",
            ));
        }
        self.owner_primary_key = Some(descriptor.public_key);
        let grant = poll_once_ready(self.custody.sign_in_scope(
            scope,
            SignRequest::new(
                &principal,
                &action.custody_key,
                self.trace,
                SignAuthorization::new(Operation::ProtocolMutation, None),
                &prepared.unsigned_canonical_bytes,
                &prepared.disclosure,
                action.started_at,
            ),
        ))
        .map_err(|_| AgentFailure::Unavailable)?
        .map_err(|_| AgentFailure::Refused("custody refused creation signature"))?;
        let submit = SubmitRequest {
            preparation_ref: prepared.preparation_ref,
            signature: SignatureBytes::new(grant.signature().to_vec())
                .map_err(|_| AgentFailure::Refused("invalid custody signature"))?,
            approval_release_ref: None,
        };
        let mut observation = self
            .runtime
            .submit(
                &self.client.submit(mutation(
                    action.action_key,
                    submit_digest(&submit, grant.signer_public_key()),
                    submit,
                )?),
                grant.signer_public_key(),
            )
            .map_err(map_boundary)?;
        if observation.activity_id != action.action_key {
            return Err(AgentFailure::Refused(
                "agent returned a different activity identity",
            ));
        }
        if observation.receipt.is_none() {
            let tracked = self
                .runtime
                .track(&self.client.track(TrackRequest {
                    submission_ref: observation.submission.submission_ref.clone(),
                }))
                .map_err(map_boundary)?;
            if tracked.activity_id != action.action_key {
                return Err(AgentFailure::Refused(
                    "tracked creation activity identity changed",
                ));
            }
            observation = tracked;
        }
        let receipt = match observation.receipt {
            Some(receipt) => receipt,
            None => match self
                .runtime
                .receipt_by_idempotency_key(action.action_key, action.action_key)
                .map_err(map_boundary)?
            {
                ReceiptLookup::Found(receipt) => receipt,
                ReceiptLookup::Absent => return Err(AgentFailure::Unavailable),
            },
        };
        Ok(ProtocolEvidence {
            action_key: action.action_key,
            activity_id: observation.activity_id,
            receipt_bytes: receipt.canonical_bytes,
            authorized_batch: receipt.authorised_batch,
            verification_level: receipt.verification_level,
        })
    }

    ///
    /// # Errors
    ///
    /// Refuses invalid protocol evidence, custody authorization, or unavailable agent state.
    pub fn submit_lifecycle_intent(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        registry: &ModuleRegistry,
        intent: layerx_intents::Intent,
        action_key: [u8; 32],
        custody_key: crate::custody::KeyId,
        started_at: u64,
    ) -> Result<ProtocolEvidence, AgentFailure> {
        let compiled = layerx_intents::compile(&intent, registry)
            .map_err(|_| AgentFailure::Refused("lifecycle intent did not compile"))?;
        let disclosure = layerx_intents::DisclosureCheck::verify(&intent, &compiled)
            .map_err(|_| AgentFailure::Refused("lifecycle disclosure did not match"))?;
        self.submit_scoped(
            scope,
            &ProtocolAction {
                stage: CreationStage::BudgetCreation,
                action_key,
                intent,
                compiled,
                disclosure,
                custody_key,
                started_at,
            },
        )
    }

    ///
    /// # Errors
    ///
    /// Refuses invalid protocol evidence, custody authorization, or unavailable agent state.
    pub fn finalization_evidence(
        evidence: &ProtocolEvidence,
        expected_module: layerx_types::payload::ModuleId,
        expected_operation: u8,
        finalized_at: u64,
    ) -> Result<super::agent_runtime::AgentFinalizationEvidence, AgentFailure> {
        let verified =
            layerx_proof::receipt::verify(&evidence.receipt_bytes, &evidence.authorized_batch)
                .map_err(|_| AgentFailure::Refused("lifecycle receipt verification failed"))?;
        if evidence.verification_level
            < layerx_types::verify::VerificationLevel::CHECKPOINT_FINALISED
            || evidence.verification_level < verified.level()
        {
            return Err(AgentFailure::Refused(
                "lifecycle receipt is not checkpoint-finalized",
            ));
        }
        let protocol = verified.receipt().protocol().ok_or(AgentFailure::Refused(
            "lifecycle receipt is not protocol material",
        ))?;
        if evidence.activity_id != evidence.action_key
            || protocol.activity_id() != evidence.activity_id
            || protocol.module_id() != expected_module as u16
            || protocol.operation() != expected_operation
            || protocol.result_code() != 0
        {
            return Err(AgentFailure::Refused(
                "lifecycle receipt does not prove successful action",
            ));
        }
        Ok(super::agent_runtime::AgentFinalizationEvidence {
            action_key: evidence.action_key,
            activity_id: protocol.activity_id(),
            receipt_digest: sha2::Sha256::digest(verified.canonical_bytes()).into(),
            observed_sequence: protocol.global_sequence(),
            verification: evidence.verification_level.wire_rank(),
            finalized_at,
        })
    }

    ///
    /// # Errors
    ///
    /// Refuses invalid protocol evidence, custody authorization, or unavailable agent state.
    pub fn publish_creation(
        &mut self,
        projection: &CreationProjection,
    ) -> Result<(), AgentFailure> {
        if !matches!(
            projection.status.state,
            crate::agents::CreationState::Active
        ) || projection.verified_evidence.is_empty()
            || projection.protocol_grant_id == [0; 32]
        {
            return Err(AgentFailure::Refused("creation is not receipt complete"));
        }
        let primary = projection
            .primary_public_key
            .ok_or(AgentFailure::Refused("creation primary key is missing"))?;
        let period_end = projection
            .started_at
            .checked_add(projection.period_seconds)
            .ok_or(AgentFailure::Refused("agent period overflow"))?;
        let action_key = digest(&[
            b"layerx-human/agent-create/action/v1",
            &projection.agent_id,
            &[4_u8],
        ]);
        let seed = AgentLifecycleSeed {
            agent_id: format!("agt_{}", hex(&projection.agent_id)),
            name: projection.name.clone(),
            purpose: projection.purpose.clone(),
            currency: projection.currency.clone(),
            monthly_limit: projection.monthly_limit,
            period_start: projection.started_at,
            period_end,
            created_at: projection.started_at,
            updated_at: projection.started_at,
            verified_evidence: projection.verified_evidence.clone(),
            actor: self.actor.as_str().to_owned(),
            primary_authority: self.authority.as_str().to_owned(),
            custody_key: format!("agent-{}", &hex(&projection.agent_id)[..32]),
            custody_public_key: primary,
            owner_account: projection.owner_account.clone(),
            budget_account: projection.budget_account.clone(),
            budget_asset: projection.budget_asset,
            purpose_hash: projection.purpose_hash,
            recovery_root: projection.recovery_root,
            recovery_threshold: projection.recovery_threshold,
            capability_id: digest(&[b"layerx-human/agent-capability/v1", &projection.agent_id]),
            activity_types: projection.activity_types.clone(),
            counterparties: projection.counterparties.clone(),
            assets: projection.assets.clone(),
            amount_ceiling: projection.amount_ceiling,
            rate_maximum_uses: projection.rate_maximum_uses,
            rate_window_sequences: projection.rate_window_sequences,
            purposes: projection.purposes.clone(),
            capability_expiry_sequence: projection.capability_expiry_sequence,
            session_scopes: projection.daemon_scopes.clone(),
            session_expiry_unix_seconds: projection.session_expires_at,
            protocol_grant_id: projection.protocol_grant_id,
            budget_period_seconds: projection.period_seconds,
            budget_expiry_seconds: projection.budget_expiry_seconds,
            initial_funding: projection.initial_funding,
            network_id: projection.network_id,
            creation_receipt_roots: projection.verified_evidence.clone(),
        };
        let mut request_id = [0; 8];
        request_id.copy_from_slice(&action_key[..8]);
        self.runtime
            .publish_lifecycle(u64::from_be_bytes(request_id), action_key, &seed)
            .map_err(map_boundary)
    }
}

impl ProductionAgentCreation<'_> {
    fn install_session_owner(
        &mut self,
        install: &AgentOwnerInstall,
        action_key: [u8; 32],
        grant_id: [u8; 32],
        finalization: &super::agent_runtime::AgentFinalizationEvidence,
    ) -> Result<AgentEvidence, AgentFailure> {
        let body = install.body_digest().map_err(map_boundary)?;
        let mut request_id = [0_u8; 8];
        request_id.copy_from_slice(&action_key[..8]);
        let installed = self
            .runtime
            .owner_install(u64::from_be_bytes(request_id), action_key, body, install)
            .map_err(map_boundary)?;
        if installed.session_id != install.session_id
            || installed.observed_head_sequence < finalization.observed_sequence
        {
            return Err(AgentFailure::Refused(
                "owner installation evidence differs from protocol grant",
            ));
        }
        self.latest_session_credential = Some((
            zeroize::Zeroizing::new(installed.token_id.expose()),
            installed.generation,
        ));
        Ok(AgentEvidence {
            action_key,
            object_id: grant_id,
            observed_sequence: installed.observed_head_sequence,
            verification_level: VerificationLevel::CHECKPOINT_FINALISED,
            receipt_digest: finalization.receipt_digest,
        })
    }
}

impl AgentCreationContract for ProductionAgentCreation<'_> {
    fn submit_protocol(
        &mut self,
        _action: ProtocolAction,
    ) -> Result<ProtocolEvidence, AgentFailure> {
        Err(AgentFailure::Refused(
            "creation protocol submission requires principal scope",
        ))
    }

    fn provision_session(
        &mut self,
        _request: SessionProvision,
    ) -> Result<AgentEvidence, AgentFailure> {
        Err(AgentFailure::Refused(
            "session provisioning requires principal scope",
        ))
    }

    fn narrow_capability(
        &mut self,
        request: CapabilityProvision,
    ) -> Result<AgentEvidence, AgentFailure> {
        let agent = std::str::from_utf8(request.did.as_bytes())
            .map_err(|_| AgentFailure::Refused("agent DID is not textual"))?
            .to_owned();
        let install = AgentCapabilityInstall {
            action_key: request.action_key,
            agent,
            authority_id: request.primary_authority,
            capability_id: request.capability_id,
            activity_types: request.activity_types.into_iter().collect(),
            counterparties: request.counterparties.into_iter().collect(),
            assets: request.assets.into_iter().collect(),
            amount_ceiling: request.amount_ceiling,
            rate_maximum_uses: request.rate_maximum_uses,
            rate_window_sequences: request.rate_window_sequences,
            purposes: request.purposes.into_iter().collect(),
            expiry_sequence: request.expiry_sequence,
        };
        let (object_id, observed_sequence, rank, receipt_digest) = self
            .runtime
            .capability_install(&install)
            .map_err(map_boundary)?;
        let verification_level = match rank {
            0 => VerificationLevel::UNVERIFIED,
            1 => VerificationLevel::SEQUENCER_SIGNED,
            2 => VerificationLevel::BATCH_INCLUDED,
            3 => VerificationLevel::STATE_PROVEN,
            4 => VerificationLevel::CHECKPOINT_FINALISED,
            5 => VerificationLevel::SETTLEMENT_ANCHORED,
            _ => {
                return Err(AgentFailure::Refused(
                    "capability verification level is invalid",
                ));
            }
        };
        let evidence = AgentEvidence {
            action_key: request.action_key,
            object_id,
            observed_sequence,
            verification_level,
            receipt_digest,
        };
        if evidence.object_id != request.capability_id
            || evidence.receipt_digest
                != evidence.expected_digest(CreationStage::CapabilityNarrowing)
        {
            return Err(AgentFailure::Refused(
                "capability evidence differs from request",
            ));
        }
        Ok(evidence)
    }
}

impl ProductionAgentCreation<'_> {
    fn session_preparation(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        request: &SessionProvision,
    ) -> Result<SessionPreparation, AgentFailure> {
        let fee = request
            .native_fee_budget
            .map(|budget| crate::agents::NativeFeeConsent {
                asset_id: budget.asset,
                maximum_per_activity: budget.maximum_per_activity,
                maximum_total: budget.maximum_total,
                period_length_ms: budget.period_length,
                maximum_per_period: budget.maximum_per_period,
            });
        let replacement = request
            .replacement
            .as_ref()
            .map(|state| (state.grant.grant_id, state.charge_commitment));
        let activities: Vec<_> = request
            .activity_types
            .iter()
            .map(|value| value.value())
            .collect();
        let lifetime = request
            .expires_at
            .checked_sub(request.not_before)
            .ok_or(AgentFailure::Refused("session expiry precedes start"))?;
        let canonical_request = serde_json::to_vec(&(
            request.did.as_bytes(),
            activities,
            &request.daemon_scopes,
            request.custody_key.as_str(),
            request.primary_authority,
            lifetime,
            &fee,
            replacement,
        ))
        .map_err(|_| AgentFailure::Refused("invalid session request"))?;
        let request_binding: [u8; 32] = Sha256::digest(canonical_request).into();
        let key = crate::store::RowKey::new(format!("session-issue-{}", hex(&request.action_key)))
            .map_err(|_| AgentFailure::Refused("invalid session action"))?;
        if let Some(row) = scope.get(crate::store::Table::Journeys, &key) {
            let stored: SessionPreparation = serde_json::from_slice(row.bytes())
                .map_err(|_| AgentFailure::Refused("corrupt session preparation"))?;
            if stored.version != 1 || stored.request_binding != request_binding {
                return Err(AgentFailure::Refused(
                    "session preparation changed on retry",
                ));
            }
            return Ok(stored);
        }
        let policy = self.runtime.native_fee_policy().map_err(map_boundary)?;
        if (policy.version == 2 && fee.is_none())
            || fee
                .as_ref()
                .is_some_and(|value| value.asset_id != policy.asset_id)
        {
            return Err(AgentFailure::Refused(
                "explicit native fee budget is required",
            ));
        }
        let actor = std::str::from_utf8(request.did.as_bytes())
            .map_err(|_| AgentFailure::Refused("invalid agent DID"))?;
        let identity = self.runtime.identity_resolve(actor).map_err(map_boundary)?;
        let native_not_before = session_period_anchor(request)?;
        let plan = SessionPreparation {
            version: 1,
            request_binding,
            started_at: request.not_before,
            lease_expires_at: request.expires_at,
            native_not_before,
            native_expires_at: request
                .expires_at
                .checked_mul(1_000)
                .ok_or(AgentFailure::Refused("session time overflow"))?,
            revocation_sequence: identity.revocation_sequence,
            expiry_sequence: self
                .runtime
                .head()
                .map_err(map_boundary)?
                .chain_sequence
                .checked_add(1024)
                .ok_or(AgentFailure::Refused("session issuance sequence overflow"))?,
            fee,
            replacement,
        };
        scope
            .put(
                crate::store::Table::Journeys,
                key,
                request.not_before,
                serde_json::to_vec(&plan)
                    .map_err(|_| AgentFailure::Refused("invalid session preparation"))?,
            )
            .map_err(|_| AgentFailure::Unavailable)?;
        Ok(plan)
    }

    fn submit_session_intent(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        registry: &ModuleRegistry,
        request: &SessionProvision,
        intent: Intent,
        started_at: u64,
    ) -> Result<ProtocolEvidence, AgentFailure> {
        let actor = AgentDid::new(
            std::str::from_utf8(request.did.as_bytes())
                .map_err(|_| AgentFailure::Refused("invalid session DID"))?
                .to_owned(),
        )
        .map_err(|_| AgentFailure::Refused("invalid session DID"))?;
        let authority = AuthorityRef::new(hex(&request.primary_authority))
            .map_err(|_| AgentFailure::Refused("invalid session owner"))?;
        let previous_actor = std::mem::replace(&mut self.actor, actor);
        let previous_authority = std::mem::replace(&mut self.authority, authority);
        let result = self.submit_lifecycle_intent(
            scope,
            registry,
            intent,
            request.action_key,
            request.custody_key.clone(),
            started_at,
        );
        self.actor = previous_actor;
        self.authority = previous_authority;
        result
    }
}

impl ScopedAgentCreationContract for ProductionAgentCreation<'_> {
    fn submit_protocol_scoped(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        action: ProtocolAction,
    ) -> Result<ProtocolEvidence, AgentFailure> {
        self.submit_scoped(scope, &action)
    }

    fn provision_session_scoped(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        registry: &ModuleRegistry,
        request: SessionProvision,
    ) -> Result<AgentEvidence, AgentFailure> {
        let agent = std::str::from_utf8(request.did.as_bytes())
            .map_err(|_| AgentFailure::Refused("agent DID is not textual"))?
            .to_owned();
        let plan = self.session_preparation(scope, &request)?;
        let plan_bytes = serde_json::to_vec(&plan)
            .map_err(|_| AgentFailure::Refused("invalid session preparation"))?;
        let session_seed = self
            .runtime
            .prepare_session_seed(
                &agent,
                request.action_key,
                Sha256::digest(plan_bytes).into(),
            )
            .map_err(map_boundary)?;
        let session_public_key = LocalSigner::new(*session_seed.expose()).public_key();
        let grantor = layerx_wire::hash::did_id_for_protocol(&request.did, 3)
            .map_err(|_| AgentFailure::Refused("invalid agent identity"))?;
        let issued = issue_session_key(&SessionKeyRequest {
            fee_budget: plan
                .fee
                .as_ref()
                .map(|value| value.budget(plan.native_not_before))
                .transpose()
                .map_err(|_| AgentFailure::Refused("invalid native fee budget"))?,
            purpose: layerx_crypto::session::SessionPurpose::Activity,
            grantor,
            session_public_key,
            not_before: plan.native_not_before,
            expires_at: Some(plan.native_expires_at),
            permitted_activity_types: request.activity_types.clone(),
            revocation_sequence: Some(plan.revocation_sequence),
        })
        .map_err(|_| AgentFailure::Refused("protocol session grant is invalid"))?;
        let mut grant_intent = SessionGrant::new(
            issued.registration_payload.clone(),
            plan.expiry_sequence,
            request.action_key,
        )
        .map_err(|_| AgentFailure::Refused("protocol session grant is invalid"))?;
        if let Some((predecessor, commitment)) = plan.replacement {
            grant_intent = grant_intent
                .replacing(predecessor, commitment)
                .map_err(|_| AgentFailure::Refused("invalid session replacement"))?;
        }
        let protocol = self.submit_session_intent(
            scope,
            registry,
            &request,
            Intent::v1(IntentKind::SessionGrant(grant_intent)),
            plan.started_at,
        )?;
        let finalization = Self::finalization_evidence(
            &protocol,
            layerx_types::payload::ModuleId::Governance,
            5,
            plan.started_at,
        )?;
        let install = AgentOwnerInstall {
            agent,
            authority_kind: 2,
            authority_id: issued.grant_id,
            session_id: request.action_key,
            token_id: [0; 32],
            session_public_key: issued.session_public_key,
            registration_payload: issued.registration_payload.clone(),
            grantor,
            grant_not_before: plan.native_not_before,
            grant_expires_at: plan.native_expires_at,
            grant_revocation_sequence: plan.revocation_sequence,
            session_seed: Some(session_seed),
            permitted_activity_types: request
                .activity_types
                .iter()
                .map(|value| value.ordinal())
                .collect(),
            scopes: request.daemon_scopes,
            lease_not_before_unix_ms: plan
                .started_at
                .checked_mul(1_000)
                .ok_or(AgentFailure::Refused("session time overflow"))?,
            lease_not_after_unix_ms: plan
                .lease_expires_at
                .checked_mul(1_000)
                .ok_or(AgentFailure::Refused("session time overflow"))?,
            opening_client: self.actor.as_str().to_owned(),
            policy_version: self.authority.as_str().to_owned(),
            lifecycle: None,
        };
        self.install_session_owner(&install, request.action_key, issued.grant_id, &finalization)
    }
}

fn mutation<T>(
    key: [u8; 32],
    body_digest: [u8; 32],
    operation: T,
) -> Result<IdempotentMutation<T>, AgentFailure> {
    let mut request_id = [0_u8; 8];
    request_id.copy_from_slice(&key[..8]);
    Ok(IdempotentMutation {
        request_id: layerx_agent_api::error::RequestId(u64::from_be_bytes(request_id)),
        key: Key::new(key).map_err(|_| AgentFailure::Refused("invalid creation action key"))?,
        body_digest: BodyDigest(body_digest),
        operation,
    })
}

fn prepare_digest(request: &PrepareRequest) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(PREPARE_DOMAIN);
    digest.update(request.protocol_activity_type.to_be_bytes());
    hash_text(&mut digest, request.actor.as_str());
    hash_text(&mut digest, request.authority.as_str());
    digest.update(request.account_sequence.0.to_be_bytes());
    digest.update(request.timestamp_bound.not_before.0.to_be_bytes());
    digest.update(request.timestamp_bound.not_after.0.to_be_bytes());
    hash_text(&mut digest, request.idempotency_key.as_str());
    digest.update(request.fee_limit.0.to_be_bytes());
    digest.update(request.payload_hash);
    digest.update(Sha256::digest(request.payload.as_bytes()));
    digest.finalize().into()
}

fn submit_digest(request: &SubmitRequest, public_key: [u8; 32]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(SUBMIT_DOMAIN);
    hash_text(&mut digest, request.preparation_ref.as_str());
    digest.update(Sha256::digest(request.signature.as_bytes()));
    digest.update(public_key);
    match request.approval_release_ref {
        Some(reference) => {
            digest.update([1]);
            digest.update(reference);
        }
        None => digest.update([0]),
    }
    digest.finalize().into()
}

fn hash_text(digest: &mut Sha256, value: &str) {
    digest.update(u32::try_from(value.len()).unwrap_or(u32::MAX).to_be_bytes());
    digest.update(value.as_bytes());
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|byte| {
            [
                DIGITS[(byte >> 4) as usize] as char,
                DIGITS[(byte & 15) as usize] as char,
            ]
        })
        .collect()
}

fn map_boundary(error: AgentBoundaryError) -> AgentFailure {
    match error {
        AgentBoundaryError::Unavailable => AgentFailure::Unavailable,
        AgentBoundaryError::Refused | AgentBoundaryError::CorruptResponse => {
            AgentFailure::Refused("agent refused or corrupted creation operation")
        }
    }
}

fn digest(parts: &[&[u8]]) -> [u8; 32] {
    let mut digest = Sha256::new();
    for part in parts {
        digest.update(u32::try_from(part.len()).unwrap_or(u32::MAX).to_be_bytes());
        digest.update(part);
    }
    digest.finalize().into()
}

fn session_period_anchor(request: &SessionProvision) -> Result<u64, AgentFailure> {
    match &request.replacement {
        Some(prior) => {
            if prior.revoked_at_sequence == 0
                || prior.successor != [0; 32]
                || prior.grant.fee_budget != request.native_fee_budget
                || prior.grant.permitted_activity_types != request.activity_types
                || prior.grant.grantor
                    != layerx_wire::hash::did_id_for_protocol(&request.did, 3)
                        .map_err(|_| AgentFailure::Refused("invalid session DID"))?
            {
                return Err(AgentFailure::Refused("session replacement is not bound"));
            }
            Ok(prior.grant.not_before)
        }
        None => request
            .not_before
            .checked_mul(1_000)
            .ok_or(AgentFailure::Refused("session time overflow")),
    }
}

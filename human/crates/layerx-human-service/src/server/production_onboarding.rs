use super::*;
use crate::agents::{CreationStage, NativeAgentCreationContract, NativeFundingRequest};
use crate::custody::{Operation as CustodyOperation, SignAuthorization, SignRequest};
use crate::onboarding::{NativePlan, NativeSponsor, OnboardingStatus};
use crate::store::{PrincipalId, PrincipalScope};
use layerx_intents::owner_activity::OwnerEnvelopeContext;

impl ProductionComponents {
    pub(super) fn advance_native_onboarding(
        &self, principal: &PrincipalId, trace: &str, observed_at: u64,
    ) -> Result<OnboardingStatus, ApiFailure> {
        let trace = TraceId::parse(trace).map_err(|_| ApiFailure::invalid_request(None))?;
        let mut store = self.store.lock().map_err(|_| ApiFailure::unavailable())?;
        let mut journey = {
            let mut scope = store.principal(principal).map_err(|_| ApiFailure::unavailable())?;
            let mut journey = OnboardingJourney::load(&scope)
                .map_err(|_| ApiFailure::upstream_degraded())?.ok_or_else(ApiFailure::not_found)?;
            self.custody.resume_onboarding_local(&mut journey, &mut scope, observed_at)
                .map_err(|_| ApiFailure::upstream_degraded())?;
            if matches!(journey.status().state(), crate::onboarding::OnboardingState::Complete | crate::onboarding::OnboardingState::Refused) {
                return Ok(journey.status());
            }
            journey
        };
        let (sponsor, mut agent) = {
            let scope = store.principal(&self.onboarding_sponsor_principal)
                .map_err(|_| ApiFailure::upstream_degraded())?;
            let mut agent = self.principal_agent(&scope)?;
            (self.native_onboarding_sponsor(&scope, &mut agent)?, agent)
        };
        let registry = agent.registry().clone();
        let (plan, consent) = {
            let mut scope = store.principal(principal).map_err(|_| ApiFailure::unavailable())?;
            let plan = journey.native_plan(&mut scope, &sponsor, observed_at)
                .map_err(|_| ApiFailure::upstream_degraded())?;
            let consent = self.native_onboarding_consent(&mut scope, &plan, &registry, &trace)?;
            (plan, consent)
        };
        let registration = {
            let mut scope = store.principal(&self.onboarding_sponsor_principal)
                .map_err(|_| ApiFailure::upstream_degraded())?;
            let mut adapter = self.onboarding_adapter(&mut agent, &trace,
                self.agent_actor.clone(), self.agent_authority.clone())?;
            adapter.submit_lifecycle_intent(&mut scope, &registry,
                Intent::v3(IntentKind::NativeOnboarding(consent)), plan.registration_action,
                primary_key()?, plan.started_at).map_err(|_| ApiFailure::upstream_degraded())?
        };
        {
            let mut scope = store.principal(principal).map_err(|_| ApiFailure::unavailable())?;
            journey.accept_native_registration(&mut scope, &plan, &registration, &trace, observed_at)
                .map_err(|_| ApiFailure::upstream_degraded())?;
            if journey.status().state() == crate::onboarding::OnboardingState::Refused {
                return Ok(journey.status());
            }
        }
        let funded = {
            let mut scope = store.principal(&self.onboarding_sponsor_principal)
                .map_err(|_| ApiFailure::upstream_degraded())?;
            let mut adapter = self.onboarding_adapter(&mut agent, &trace,
                self.agent_actor.clone(), self.agent_authority.clone())?;
            adapter.fund_scoped(&mut scope, &NativeFundingRequest {
                stage: CreationStage::MainFunding,
                source: plan.source_account().map_err(|_| ApiFailure::upstream_degraded())?,
                destination: plan.target_account().map_err(|_| ApiFailure::upstream_degraded())?,
                asset: plan.asset, amount: plan.initial_funding, action_key: plan.funding_action,
                network_id: plan.network_id, started_at: plan.started_at,
            }).map_err(|_| ApiFailure::upstream_degraded())?
        };
        let mut scope = store.principal(principal).map_err(|_| ApiFailure::unavailable())?;
        journey.accept_native_funding(&mut scope, &plan, &funded, &registry, observed_at)
            .map_err(|_| ApiFailure::upstream_degraded())?;
        if journey.status().state() == crate::onboarding::OnboardingState::Refused {
            return Ok(journey.status());
        }
        let mut agent = self.principal_agent(&scope)?;
        let owner = owner::resolve_principal_owner(self, &scope, &mut agent)?;
        let mut adapter = self.onboarding_adapter(&mut agent, &trace, owner.actor, owner.authority)?;
        let recovery = adapter.submit_lifecycle_intent(&mut scope, &registry,
            plan.recovery_intent().map_err(|_| ApiFailure::upstream_degraded())?,
            plan.recovery_action, primary_key()?, plan.started_at)
            .map_err(|_| ApiFailure::upstream_degraded())?;
        journey.accept_native_recovery(&mut scope, &plan, &recovery, &registry, &trace, observed_at)
            .map_err(|_| ApiFailure::upstream_degraded())?;
        Ok(journey.status())
    }

    fn native_onboarding_sponsor(
        &self, scope: &PrincipalScope<'_>, agent: &mut AgentRuntime,
    ) -> Result<NativeSponsor, ApiFailure> {
        if scope.principal() != &self.onboarding_sponsor_principal {
            return Err(ApiFailure::forbidden());
        }
        let owner = owner::resolve_principal_owner(self, scope, agent)?;
        if owner.actor != self.agent_actor || owner.authority != self.agent_authority
            || owner.account.canonical() != self.agent_owner_account
        {
            return Err(ApiFailure::upstream_degraded());
        }
        if owner.recovery_policy()? != (self.agent_recovery_root, self.agent_recovery_threshold) {
            return Err(ApiFailure::upstream_degraded());
        }
        let key = self.custody.describe_key(scope.principal(), &primary_key()?)
            .map_err(|_| ApiFailure::upstream_degraded())?;
        let asset = agent.native_fee_policy().map_err(agent_failure)?.asset_id;
        Ok(NativeSponsor {
            principal: scope.principal().as_str().to_owned(),
            did: Did::new(owner.actor.as_str().as_bytes()).map_err(|_| ApiFailure::upstream_degraded())?,
            public_key: key.public_key, account: owner.account, asset,
            network_id: self.network_id, initial_funding: self.onboarding_initial_funding,
            timestamp_span: self.agent_timestamp_span_seconds,
        })
    }

    fn onboarding_adapter<'a>(
        &'a self, agent: &'a mut AgentRuntime, trace: &'a TraceId,
        actor: AgentDid, authority: AuthorityRef,
    ) -> Result<ProductionAgentCreation<'a>, ApiFailure> {
        ProductionAgentCreation::new(agent, &self.agent_contract, &self.custody, trace, actor, authority,
            super::super::agent_creation::CreationBounds {
                timestamp_span: self.agent_timestamp_span_seconds, fee_limit: self.agent_fee_limit,
            }).map_err(|_| ApiFailure::upstream_degraded())
    }

    fn native_onboarding_consent(
        &self, scope: &mut PrincipalScope<'_>, plan: &NativePlan,
        registry: &layerx_types::payload::ModuleRegistry, trace: &TraceId,
    ) -> Result<layerx_crypto::onboarding::SponsoredRegistration, ApiFailure> {
        let row = RowKey::new("onboarding-native-target-consent")
            .map_err(|_| ApiFailure::upstream_degraded())?;
        if let Some(value) = scope.get(Table::Journeys, &row) {
            return plan.signed_consent(value.bytes()).map_err(|_| ApiFailure::upstream_degraded());
        }
        let key = primary_key()?;
        let descriptor = self.custody.describe_key(scope.principal(), &key)
            .map_err(|_| ApiFailure::upstream_degraded())?;
        if descriptor.class != KeyClass::HumanPrimary || descriptor.public_key != plan.target_key {
            return Err(ApiFailure::upstream_degraded());
        }
        let intent = Intent::v3(IntentKind::NativeOnboardingConsent(
            plan.consent().map_err(|_| ApiFailure::upstream_degraded())?));
        let compiled = layerx_intents::compile(&intent, registry)
            .map_err(|_| ApiFailure::upstream_degraded())?;
        let context = OwnerEnvelopeContext {
            actor: plan.target().map_err(|_| ApiFailure::upstream_degraded())?,
            owner_public_key: descriptor.public_key, network_id: plan.network_id,
            account_sequence: 0, not_before_ms: plan.started_at.checked_mul(1_000)
                .ok_or_else(ApiFailure::upstream_degraded)?,
            not_after_ms: plan.expires_at, action_key: plan.registration_action, fee_limit: 0,
        };
        let (unsigned, disclosure) = layerx_intents::owner_activity::unsigned_native(&compiled, &context, registry)
            .map_err(|_| ApiFailure::upstream_degraded())?;
        let principal = scope.principal().clone();
        let signature = super::super::poll_once_ready(self.custody.sign_in_scope(scope,
            SignRequest::new(&principal, &key, trace,
                SignAuthorization::new(CustodyOperation::ProtocolMutation, None),
                &unsigned, &disclosure, plan.started_at)))
            .map_err(|_| ApiFailure::upstream_degraded())?
            .map_err(|_| ApiFailure::upstream_degraded())?;
        let signed = layerx_intents::owner_activity::attach_signature(&unsigned,
            *signature.signature(), signature.signer_public_key(), registry)
            .map_err(|_| ApiFailure::upstream_degraded())?;
        let registration = plan.signed_consent(&signed).map_err(|_| ApiFailure::upstream_degraded())?;
        scope.put(Table::Journeys, row, plan.started_at, signed)
            .map_err(|_| ApiFailure::upstream_degraded())?;
        Ok(registration)
    }
}

fn primary_key() -> Result<KeyId, ApiFailure> {
    KeyId::new("human-primary").map_err(|_| ApiFailure::upstream_degraded())
}

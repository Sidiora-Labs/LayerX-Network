use super::*;
use crate::agents::{NativeFundingEvidence, ProtocolEvidence};
use layerx_crypto::onboarding::{OnboardingConsent, SponsoredRegistration};
use layerx_types::account::AccountId;
use layerx_types::payload::{ActivityType, ModuleId};
use layerx_types::verify::VerificationLevel;

const PLAN_ROW: &str = "onboarding-native-plan";
const FUNDING_ROW: &str = "onboarding-native-funding-receipt";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeSponsor {
    pub principal: String,
    pub did: Did,
    pub public_key: [u8; 32],
    pub account: AccountId,
    pub asset: [u8; 32],
    pub network_id: u32,
    pub initial_funding: u128,
    pub timestamp_span: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct NativePlan {
    version: u8,
    pub sponsor_principal: String,
    sponsor_did: Vec<u8>,
    pub sponsor_key: [u8; 32],
    sponsor_account: String,
    target_did: Vec<u8>,
    pub target_key: [u8; 32],
    pub asset: [u8; 32],
    pub network_id: u32,
    pub initial_funding: u128,
    pub started_at: u64,
    pub expires_at: u64,
    pub registration_action: [u8; 32],
    pub funding_action: [u8; 32],
    pub recovery_action: [u8; 32],
    recovery_root: [u8; 32],
    recovery_threshold: u16,
}

impl NativePlan {
    pub fn consent(&self) -> Result<OnboardingConsent, OnboardingError> {
        Ok(OnboardingConsent {
            sponsor: layerx_intents::canonical::did_id_for_protocol(&self.sponsor()?, 3)
                .map_err(|_| OnboardingError::EvidenceConflict)?,
            target: self.target()?,
            target_public_key: self.target_key,
            native_asset: self.asset,
            action_key: self.registration_action,
            expires_at: self.expires_at,
        })
    }
    pub fn target(&self) -> Result<Did, OnboardingError> {
        Did::new(&self.target_did).map_err(|_| OnboardingError::EvidenceConflict)
    }
    pub fn sponsor(&self) -> Result<Did, OnboardingError> {
        Did::new(&self.sponsor_did).map_err(|_| OnboardingError::EvidenceConflict)
    }
    pub fn source_account(&self) -> Result<AccountId, OnboardingError> {
        AccountId::parse(&self.sponsor_account).map_err(|_| OnboardingError::EvidenceConflict)
    }
    pub fn target_account(&self) -> Result<AccountId, OnboardingError> {
        let did = std::str::from_utf8(&self.target_did).map_err(|_| OnboardingError::EvidenceConflict)?;
        AccountId::parse(&format!("agent:{did}:main")).map_err(|_| OnboardingError::EvidenceConflict)
    }
    pub fn recovery_intent(&self) -> Result<Intent, OnboardingError> {
        Ok(Intent::v3(IntentKind::RecoveryRegistration(RecoveryRegistration::new(
            self.target()?, RecoveryRoot::new(self.recovery_root),
            ApprovalThreshold::new(self.recovery_threshold)
                .map_err(|_| OnboardingError::EvidenceConflict)?,
        )?)))
    }
    pub fn signed_consent(&self, bytes: &[u8]) -> Result<SponsoredRegistration, OnboardingError> {
        let value = SponsoredRegistration::from_signed_consent(bytes)
            .map_err(|_| OnboardingError::EvidenceConflict)?;
        if value.consent != self.consent()? || value.network_id != self.network_id {
            return Err(OnboardingError::EvidenceConflict);
        }
        Ok(value)
    }

    pub fn accept_funding(
        &self, scope: &mut PrincipalScope<'_>, registry: &ModuleRegistry,
        funded: &NativeFundingEvidence, now: u64,
    ) -> Result<(), OnboardingError> {
        let (source, destination, asset, amount, _, key, _, _, _, network, protocol) =
            funded.intent.to_wire_parts();
        if source != &self.source_account()? || destination != &self.target_account()?
            || asset.bytes() != self.asset || amount.value() != self.initial_funding
            || key.bytes() != self.funding_action || network.value() != self.network_id
            || protocol.value() != 3 || funded.evidence.actor != self.sponsor_did
            || funded.evidence.owner_public_key != self.sponsor_key
        {
            return Err(OnboardingError::EvidenceConflict);
        }
        let compiled = compile(&Intent::v1(IntentKind::LxpSend(funded.intent.clone())), registry)?;
        let kind = compiled.activity_type();
        self.verify_evidence(&funded.evidence, kind, self.funding_action)?;
        if funded.evidence.bound_activity(kind).map_err(|_| OnboardingError::EvidenceConflict)?.payload()
            != compiled.payload().as_bytes()
        {
            return Err(OnboardingError::EvidenceConflict);
        }
        put_exact(scope, RowKey::new(FUNDING_ROW)?, now, funded.evidence.receipt_bytes.clone())?;
        put_exact(scope, RowKey::new("onboarding-native-funding-activity")?, now,
            funded.evidence.signed_activity.clone())
    }

    fn verify_evidence(
        &self, evidence: &ProtocolEvidence, kind: ActivityType, action: [u8; 32],
    ) -> Result<(), OnboardingError> {
        if evidence.action_key != action || evidence.network_id != self.network_id
            || evidence.verification_level != VerificationLevel::CHECKPOINT_FINALISED
        {
            return Err(OnboardingError::EvidenceConflict);
        }
        let verified = evidence.verify_outcome(kind).map_err(|_| OnboardingError::EvidenceConflict)?;
        let protocol = verified.receipt().protocol().ok_or(OnboardingError::ReceiptShape)?;
        if protocol.result_code() != 0 || protocol.activity_id() != evidence.activity_id
            || protocol.global_sequence() == 0
        {
            return Err(OnboardingError::EvidenceConflict);
        }
        Ok(())
    }
}

impl OnboardingJourney {
    pub(crate) fn native_plan(
        &self, scope: &mut PrincipalScope<'_>, sponsor: &NativeSponsor, now: u64,
    ) -> Result<NativePlan, OnboardingError> {
        let key = RowKey::new(PLAN_ROW)?;
        let retained = scope.get(Table::Journeys, &key)
            .map(|row| serde_json::from_slice::<NativePlan>(row.bytes()))
            .transpose().map_err(|_| OnboardingError::EvidenceConflict)?;
        let started_at = retained.as_ref().map_or(now, |plan| plan.started_at);
        let mut digest = Sha256::new();
        digest.update(b"LXP/human/onboarding-native-funding/v1\0");
        digest.update(self.record.idempotency_key);
        let plan = NativePlan {
            version: 1,
            sponsor_principal: sponsor.principal.clone(),
            sponsor_did: sponsor.did.as_bytes().to_vec(),
            sponsor_key: sponsor.public_key,
            sponsor_account: sponsor.account.canonical().to_owned(),
            target_did: self.record.did.clone(),
            target_key: self.record.public_key.ok_or(OnboardingError::ReceiptRequired)?,
            asset: sponsor.asset, network_id: sponsor.network_id,
            initial_funding: sponsor.initial_funding, started_at,
            expires_at: started_at.checked_add(sponsor.timestamp_span)
                .and_then(|value| value.checked_mul(1_000))
                .ok_or(OnboardingError::InvalidAgentContext)?,
            registration_action: self.action_key(ProtocolStage::DidRegistration),
            funding_action: digest.finalize().into(),
            recovery_action: self.action_key(ProtocolStage::RecoveryRegistration),
            recovery_root: self.record.recovery_root,
            recovery_threshold: self.record.recovery_threshold,
        };
        if plan.initial_funding == 0 || sponsor.timestamp_span == 0 || plan.network_id == 0
            || sponsor.principal == scope.principal().as_str()
            || sponsor.did.as_bytes() == plan.target_did || sponsor.public_key == plan.target_key
            || plan.consent()?.payload().is_err()
            || retained.as_ref().is_some_and(|old| old != &plan)
        {
            return Err(OnboardingError::EvidenceConflict);
        }
        put_exact(scope, key, now, serde_json::to_vec(&plan)
            .map_err(|_| OnboardingError::EvidenceConflict)?)?;
        Ok(plan)
    }

    pub(crate) fn accept_native_registration(
        &mut self, scope: &mut PrincipalScope<'_>, plan: &NativePlan,
        evidence: &ProtocolEvidence, trace: &TraceId, now: u64,
    ) -> Result<(), OnboardingError> {
        let kind = ActivityType::new(ModuleId::Governance, 1)
            .map_err(|_| OnboardingError::EvidenceConflict)?;
        plan.verify_evidence(evidence, kind, plan.registration_action)?;
        let activity = evidence.bound_activity(kind).map_err(|_| OnboardingError::EvidenceConflict)?;
        let registration = SponsoredRegistration::decode(activity.payload())
            .map_err(|_| OnboardingError::EvidenceConflict)?;
        if registration.consent != plan.consent()? || registration.validate_outer(&activity).is_err()
            || evidence.actor != plan.sponsor_did || evidence.owner_public_key != plan.sponsor_key
        {
            return Err(OnboardingError::EvidenceConflict);
        }
        self.accept_native_stage(scope, ProtocolStage::DidRegistration, evidence, trace, now)
    }

    pub(crate) fn accept_native_recovery(
        &mut self, scope: &mut PrincipalScope<'_>, plan: &NativePlan,
        evidence: &ProtocolEvidence, registry: &ModuleRegistry, trace: &TraceId, now: u64,
    ) -> Result<(), OnboardingError> {
        let compiled = compile(&plan.recovery_intent()?, registry)?;
        let kind = compiled.activity_type();
        plan.verify_evidence(evidence, kind, plan.recovery_action)?;
        let activity = evidence.bound_activity(kind).map_err(|_| OnboardingError::EvidenceConflict)?;
        if activity.payload() != compiled.payload().as_bytes() || evidence.actor != plan.target_did
            || evidence.owner_public_key != plan.target_key || !self.did_verified()
            || scope.get(Table::Journeys, &RowKey::new(FUNDING_ROW)?).is_none()
        {
            return Err(OnboardingError::EvidenceConflict);
        }
        self.accept_native_stage(scope, ProtocolStage::RecoveryRegistration, evidence, trace, now)
    }

    fn accept_native_stage(
        &mut self, scope: &mut PrincipalScope<'_>, stage: ProtocolStage,
        evidence: &ProtocolEvidence, trace: &TraceId, now: u64,
    ) -> Result<(), OnboardingError> {
        let digest: [u8; 32] = Sha256::digest(&evidence.receipt_bytes).into();
        let progress = self.stage(stage);
        if evidence.action_key != self.action_key(stage)
            || progress.receipt_digest.is_some_and(|old| old != digest)
            || progress.activity_id.is_some_and(|old| old != evidence.activity_id)
        {
            return Err(OnboardingError::EvidenceConflict);
        }
        put_exact(scope, receipt_row(stage)?, now, evidence.receipt_bytes.clone())?;
        put_exact(scope, RowKey::new(format!("onboarding-native-activity-{}", stage.code()))?, now,
            evidence.signed_activity.clone())?;
        let progress = self.stage_mut(stage);
        progress.state = ProtocolState::Verified;
        progress.unavailable = None;
        progress.receipt_digest = Some(digest);
        progress.activity_id = Some(evidence.activity_id);
        progress.submission_ref = Some(hex(&evidence.activity_id));
        progress.refusal_code = None;
        self.persist(scope, now)?;
        let mut audit = AuditChain::open(scope)?;
        self.ensure_activation_audit(scope, &mut audit, trace, stage, now)
    }
}

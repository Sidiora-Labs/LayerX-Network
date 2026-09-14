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
        let did =
            std::str::from_utf8(&self.target_did).map_err(|_| OnboardingError::EvidenceConflict)?;
        AccountId::parse(&format!("agent:{did}:main"))
            .map_err(|_| OnboardingError::EvidenceConflict)
    }
    pub fn recovery_intent(&self) -> Result<Intent, OnboardingError> {
        Ok(Intent::v3(IntentKind::RecoveryRegistration(
            RecoveryRegistration::new(
                self.target()?,
                RecoveryRoot::new(self.recovery_root),
                ApprovalThreshold::new(self.recovery_threshold)
                    .map_err(|_| OnboardingError::EvidenceConflict)?,
            )?,
        )))
    }
    pub fn signed_consent(&self, bytes: &[u8]) -> Result<SponsoredRegistration, OnboardingError> {
        let value = SponsoredRegistration::from_signed_consent(bytes)
            .map_err(|_| OnboardingError::EvidenceConflict)?;
        if value.consent != self.consent()? || value.network_id != self.network_id {
            return Err(OnboardingError::EvidenceConflict);
        }
        Ok(value)
    }

    fn verify_funding(
        &self,
        registry: &ModuleRegistry,
        funded: &NativeFundingEvidence,
    ) -> Result<i32, OnboardingError> {
        let (source, destination, asset, amount, sequence, key, _, _, _, network, protocol) =
            funded.intent.to_wire_parts();
        if source != &self.source_account()?
            || destination != &self.target_account()?
            || asset.bytes() != self.asset
            || amount.value() != self.initial_funding
            || key.bytes() != self.funding_action
            || network.value() != self.network_id
            || protocol.value() != 3
            || funded.evidence.actor != self.sponsor_did
            || funded.evidence.owner_public_key != self.sponsor_key
        {
            return Err(OnboardingError::EvidenceConflict);
        }
        let compiled = compile(
            &Intent::v1(IntentKind::LxpSend(funded.intent.clone())),
            registry,
        )?;
        let kind = compiled.activity_type();
        let verified = self.verify_evidence(&funded.evidence, kind, self.funding_action)?;
        let activity = funded
            .evidence
            .bound_activity(kind)
            .map_err(|_| OnboardingError::EvidenceConflict)?;
        if activity.payload() != compiled.payload().as_bytes() {
            return Err(OnboardingError::EvidenceConflict);
        }
        let receipt = verified
            .receipt()
            .protocol()
            .ok_or(OnboardingError::ReceiptShape)?;
        if receipt.result_code() == 0 {
            let from = layerx_intents::canonical::account_id_for_protocol(source, 3)
                .map_err(|_| OnboardingError::EvidenceConflict)?;
            let to = layerx_intents::canonical::account_id_for_protocol(destination, 3)
                .map_err(|_| OnboardingError::EvidenceConflict)?;
            if receipt.from() != from
                || receipt.to() != to
                || receipt.asset() != self.asset
                || receipt.amount() != self.initial_funding
                || receipt.debit_sequence() != sequence.value()
                || receipt
                    .debit_balance_before()
                    .checked_sub(self.initial_funding)
                    != Some(receipt.debit_balance_after())
                || receipt
                    .credit_balance_before()
                    .checked_add(self.initial_funding)
                    != Some(receipt.credit_balance_after())
                || receipt.fee_charged() > activity.fee_limit()
            {
                return Err(OnboardingError::EvidenceConflict);
            }
        }
        Ok(receipt.result_code())
    }

    fn verify_evidence(
        &self,
        evidence: &ProtocolEvidence,
        kind: ActivityType,
        action: [u8; 32],
    ) -> Result<layerx_proof::receipt::VerifiedReceipt, OnboardingError> {
        if evidence.action_key != action
            || evidence.network_id != self.network_id
            || evidence.verification_level < VerificationLevel::CHECKPOINT_FINALISED
        {
            return Err(OnboardingError::EvidenceConflict);
        }
        let verified = evidence
            .verify_outcome(kind)
            .map_err(|_| OnboardingError::EvidenceConflict)?;
        let protocol = verified
            .receipt()
            .protocol()
            .ok_or(OnboardingError::ReceiptShape)?;
        if evidence.verification_level < verified.level()
            || protocol.activity_id() != evidence.activity_id
            || protocol.global_sequence() == 0
        {
            return Err(OnboardingError::EvidenceConflict);
        }
        Ok(verified)
    }
}

impl OnboardingJourney {
    pub(crate) fn bootstrap_action(&self, stage: ProtocolStage) -> [u8; 32] {
        self.action_key(stage)
    }

    pub(crate) fn accept_bootstrap_owner(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        stage: ProtocolStage,
        evidence: &ProtocolEvidence,
        registry: &ModuleRegistry,
        trace: &TraceId,
        now: u64,
    ) -> Result<(), OnboardingError> {
        let (did, public_key, policy) = self.bootstrap_identity()?;
        let request = match stage {
            ProtocolStage::DidRegistration => layerx_intents::NativeOwnerBootstrap::Identity {
                did,
                primary_key: layerx_types::intent::PublicKey::new(public_key),
            },
            ProtocolStage::RecoveryRegistration => {
                layerx_intents::NativeOwnerBootstrap::RecoveryPolicy {
                    did,
                    root: policy.root(),
                    threshold: policy.threshold(),
                    minimum_delay: policy.challenge_delay_secs(),
                    maximum_delay: policy.challenge_delay_secs(),
                }
            }
        };
        let compiled = request.compile(registry)?;
        if evidence.actor != self.record.did
            || evidence.owner_public_key != public_key
            || evidence.verification_level < VerificationLevel::CHECKPOINT_FINALISED
            || stage == ProtocolStage::RecoveryRegistration && !self.did_verified()
        {
            return Err(OnboardingError::EvidenceConflict);
        }
        let activity = evidence
            .bound_activity(compiled.activity_type())
            .map_err(|_| OnboardingError::EvidenceConflict)?;
        if activity.payload() != compiled.payload().as_bytes() {
            return Err(OnboardingError::EvidenceConflict);
        }
        let verified = evidence
            .verify_outcome(compiled.activity_type())
            .map_err(|_| OnboardingError::EvidenceConflict)?;
        if evidence.verification_level < verified.level() {
            return Err(OnboardingError::EvidenceConflict);
        }
        let result = verified
            .receipt()
            .protocol()
            .ok_or(OnboardingError::ReceiptShape)?
            .result_code();
        self.accept_native_stage(scope, (stage, result), evidence, trace, now)
    }

    pub(crate) fn bootstrap_identity(
        &self,
    ) -> Result<(Did, [u8; 32], RecoveryPolicy), OnboardingError> {
        Ok((
            self.did()?,
            self.record
                .public_key
                .ok_or(OnboardingError::CustodyKeyRequired)?,
            RecoveryPolicy::new(
                RecoveryRoot::new(self.record.recovery_root),
                ApprovalThreshold::new(self.record.recovery_threshold)
                    .map_err(|_| OnboardingError::EvidenceConflict)?,
                self.record.recovery_challenge_delay_secs,
            )?,
        ))
    }

    pub(crate) fn native_plan(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        sponsor: &NativeSponsor,
        now: u64,
    ) -> Result<NativePlan, OnboardingError> {
        let key = RowKey::new(PLAN_ROW)?;
        let retained = scope
            .get(Table::Journeys, &key)
            .map(|row| serde_json::from_slice::<NativePlan>(row.bytes()))
            .transpose()
            .map_err(|_| OnboardingError::EvidenceConflict)?;
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
            target_key: self
                .record
                .public_key
                .ok_or(OnboardingError::ReceiptRequired)?,
            asset: sponsor.asset,
            network_id: sponsor.network_id,
            initial_funding: sponsor.initial_funding,
            started_at,
            expires_at: started_at
                .checked_add(sponsor.timestamp_span)
                .and_then(|value| value.checked_mul(1_000))
                .ok_or(OnboardingError::InvalidAgentContext)?,
            registration_action: self.action_key(ProtocolStage::DidRegistration),
            funding_action: digest.finalize().into(),
            recovery_action: self.action_key(ProtocolStage::RecoveryRegistration),
            recovery_root: self.record.recovery_root,
            recovery_threshold: self.record.recovery_threshold,
        };
        if plan.initial_funding == 0
            || sponsor.timestamp_span == 0
            || plan.network_id == 0
            || sponsor.principal == scope.principal().as_str()
            || sponsor.did.as_bytes() == plan.target_did
            || sponsor.public_key == plan.target_key
            || plan.consent()?.payload().is_err()
            || retained.as_ref().is_some_and(|old| old != &plan)
        {
            return Err(OnboardingError::EvidenceConflict);
        }
        put_exact(
            scope,
            key,
            now,
            serde_json::to_vec(&plan).map_err(|_| OnboardingError::EvidenceConflict)?,
        )?;
        if self.record.native_funding.is_none() {
            self.record.native_funding = Some(ProtocolRecord::queued());
            self.persist(scope, now)?;
        }
        Ok(plan)
    }

    fn require_native_plan(&self, plan: &NativePlan) -> Result<(), OnboardingError> {
        if plan.target_did != self.record.did
            || Some(plan.target_key) != self.record.public_key
            || plan.registration_action != self.action_key(ProtocolStage::DidRegistration)
            || plan.recovery_action != self.action_key(ProtocolStage::RecoveryRegistration)
            || plan.recovery_root != self.record.recovery_root
            || plan.recovery_threshold != self.record.recovery_threshold
            || self.record.native_funding.is_none()
        {
            return Err(OnboardingError::EvidenceConflict);
        }
        Ok(())
    }

    pub(crate) fn accept_native_funding(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        plan: &NativePlan,
        funded: &NativeFundingEvidence,
        registry: &ModuleRegistry,
        now: u64,
    ) -> Result<(), OnboardingError> {
        self.require_native_plan(plan)?;
        if !self.did_verified() {
            return Err(OnboardingError::StageNotEligible);
        }
        let result = plan.verify_funding(registry, funded)?;
        let record = self
            .record
            .native_funding
            .as_mut()
            .ok_or(OnboardingError::StageNotEligible)?;
        retain_native_outcome(scope, record, funding_row(), &funded.evidence, result, now)?;
        self.persist(scope, now)
    }

    pub(crate) fn accept_native_registration(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        plan: &NativePlan,
        evidence: &ProtocolEvidence,
        trace: &TraceId,
        now: u64,
    ) -> Result<(), OnboardingError> {
        let kind = ActivityType::new(ModuleId::Governance, 1)
            .map_err(|_| OnboardingError::EvidenceConflict)?;
        self.require_native_plan(plan)?;
        let verified = plan.verify_evidence(evidence, kind, plan.registration_action)?;
        let result = verified
            .receipt()
            .protocol()
            .ok_or(OnboardingError::ReceiptShape)?
            .result_code();
        let activity = evidence
            .bound_activity(kind)
            .map_err(|_| OnboardingError::EvidenceConflict)?;
        let registration = SponsoredRegistration::decode(activity.payload())
            .map_err(|_| OnboardingError::EvidenceConflict)?;
        if registration.consent != plan.consent()?
            || registration.validate_outer(&activity).is_err()
            || evidence.actor != plan.sponsor_did
            || evidence.owner_public_key != plan.sponsor_key
        {
            return Err(OnboardingError::EvidenceConflict);
        }
        self.accept_native_stage(
            scope,
            (ProtocolStage::DidRegistration, result),
            evidence,
            trace,
            now,
        )
    }

    pub(crate) fn accept_native_recovery(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        plan: &NativePlan,
        evidence: &ProtocolEvidence,
        registry: &ModuleRegistry,
        trace: &TraceId,
        now: u64,
    ) -> Result<(), OnboardingError> {
        let compiled = compile(&plan.recovery_intent()?, registry)?;
        let kind = compiled.activity_type();
        self.require_native_plan(plan)?;
        let verified = plan.verify_evidence(evidence, kind, plan.recovery_action)?;
        let result = verified
            .receipt()
            .protocol()
            .ok_or(OnboardingError::ReceiptShape)?
            .result_code();
        let activity = evidence
            .bound_activity(kind)
            .map_err(|_| OnboardingError::EvidenceConflict)?;
        if activity.payload() != compiled.payload().as_bytes()
            || evidence.actor != plan.target_did
            || evidence.owner_public_key != plan.target_key
            || !self.did_verified()
            || !self
                .record
                .native_funding
                .as_ref()
                .is_some_and(|record| record.state == ProtocolState::Verified)
        {
            return Err(OnboardingError::EvidenceConflict);
        }
        self.accept_native_stage(
            scope,
            (ProtocolStage::RecoveryRegistration, result),
            evidence,
            trace,
            now,
        )
    }

    fn accept_native_stage(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        outcome: (ProtocolStage, i32),
        evidence: &ProtocolEvidence,
        trace: &TraceId,
        now: u64,
    ) -> Result<(), OnboardingError> {
        let (stage, result) = outcome;
        if evidence.action_key != self.action_key(stage) {
            return Err(OnboardingError::EvidenceConflict);
        }
        retain_native_outcome(
            scope,
            self.stage_mut(stage),
            receipt_row(stage)?,
            evidence,
            result,
            now,
        )?;
        self.persist(scope, now)?;
        if result == 0 {
            let mut audit = AuditChain::open(scope)?;
            self.ensure_activation_audit(scope, &mut audit, trace, stage, now)?;
        }
        Ok(())
    }
}

pub(super) fn funding_row() -> RowKey {
    RowKey::new(FUNDING_ROW).unwrap_or_else(|_| unreachable!("static onboarding funding row"))
}

fn retain_native_outcome(
    scope: &mut PrincipalScope<'_>,
    record: &mut ProtocolRecord,
    row: RowKey,
    evidence: &ProtocolEvidence,
    result: i32,
    now: u64,
) -> Result<(), OnboardingError> {
    let digest: [u8; 32] = Sha256::digest(&evidence.receipt_bytes).into();
    if record.receipt_digest.is_some_and(|old| old != digest)
        || record
            .activity_id
            .is_some_and(|old| old != evidence.activity_id)
        || record.state == ProtocolState::Refused && record.refusal_code != Some(result)
    {
        return Err(OnboardingError::EvidenceConflict);
    }
    put_exact(
        scope,
        RowKey::new(format!("{}-activity", row.as_str()))?,
        now,
        evidence.signed_activity.clone(),
    )?;
    put_exact(scope, row, now, evidence.receipt_bytes.clone())?;
    record.unavailable = None;
    record.submission_ref = Some(hex(&evidence.activity_id));
    if result == 0 {
        record.state = ProtocolState::Verified;
        record.receipt_digest = Some(digest);
        record.activity_id = Some(evidence.activity_id);
        record.refusal_code = None;
    } else {
        record.state = ProtocolState::Refused;
        record.refusal_code = Some(result);
    }
    Ok(())
}

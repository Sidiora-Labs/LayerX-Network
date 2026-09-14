use super::*;
use layerx_crypto::onboarding::{OnboardingConsent, SponsoredRegistration};
use layerx_intents::{LxpSend, NativeBudgetCreate};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(super) struct NativeCreation {
    pub native_asset: [u8; 32],
    pub timestamp_span: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeOnboardingRequest {
    pub consent: OnboardingConsent,
    pub network_id: u32,
    pub started_at: u64,
    pub custody_key: KeyId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeFundingRequest {
    pub stage: CreationStage,
    pub source: AccountId,
    pub destination: AccountId,
    pub asset: [u8; 32],
    pub amount: u128,
    pub action_key: [u8; 32],
    pub network_id: u32,
    pub started_at: u64,
}

pub struct NativeFundingEvidence {
    pub evidence: ProtocolEvidence,
    pub intent: LxpSend,
}

pub struct NativeIdentityRevisionRequest {
    pub did: Did,
    pub public_key: [u8; 32],
    pub minimum_sequence: u64,
    pub action_key: [u8; 32],
    pub started_at: u64,
}

pub trait NativeAgentCreationContract: ScopedAgentCreationContract {
    /// # Errors
    /// Refuses an unfinalised identity, mismatched owner key or conflicting retained revision.
    fn identity_revision_scoped(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        request: &NativeIdentityRevisionRequest,
    ) -> Result<u64, AgentFailure>;
    /// # Errors
    /// Refuses an invalid target consent, sponsor or durable registration outcome.
    fn onboard_scoped(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        request: NativeOnboardingRequest,
    ) -> Result<ProtocolEvidence, AgentFailure>;
    /// # Errors
    /// Refuses an unproven source account, custody authorization or transfer outcome.
    fn fund_scoped(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        request: &NativeFundingRequest,
    ) -> Result<NativeFundingEvidence, AgentFailure>;
    /// # Errors
    /// Refuses an unavailable account proof or conflicting retained source sequence.
    fn source_sequence_scoped(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        account: &AccountId,
        action_key: [u8; 32],
        started_at: u64,
    ) -> Result<u64, AgentFailure>;
}

pub(super) fn stages(
    mode: &NativeCreation,
    fee: Option<&NativeFeeConsent>,
    asset: [u8; 32],
) -> Vec<CreationStage> {
    let mut result = vec![CreationStage::Custody, CreationStage::DidRegistration];
    if asset == mode.native_asset || fee.is_some() {
        result.push(CreationStage::MainFunding);
    }
    if asset != mode.native_asset {
        result.extend([
            CreationStage::AssetAccountOpening,
            CreationStage::InitialFunding,
        ]);
    }
    result.extend([
        CreationStage::RecoveryRegistration,
        CreationStage::BudgetCreation,
        CreationStage::SessionProvision,
        CreationStage::CapabilityNarrowing,
    ]);
    result
}

impl CreationJourney {
    fn native_recovery_sequence(
        &self,
        scope: &PrincipalScope<'_>,
    ) -> Result<u64, AgentCreationError> {
        let stage = self.stage(CreationStage::RecoveryRegistration);
        if stage.state != StageState::ReceiptVerified {
            return Err(AgentCreationError::EvidenceConflict);
        }
        let row = scope
            .get(
                Table::Journeys,
                &evidence_row(self.record.agent_id, CreationStage::RecoveryRegistration)?,
            )
            .ok_or(AgentCreationError::EvidenceConflict)?;
        let digest: [u8; 32] = Sha256::digest(row.bytes()).into();
        let receipt = layerx_intents::canonical::decode_receipt(row.bytes())
            .map_err(|_| AgentCreationError::EvidenceConflict)?;
        if Some(digest) != stage.evidence_digest
            || Some(receipt.activity_id()) != stage.object_id
            || receipt.protocol_version() != 3
            || receipt.result_code() != 0
            || receipt.global_sequence() == 0
        {
            return Err(AgentCreationError::EvidenceConflict);
        }
        Ok(receipt.global_sequence())
    }
    /// # Errors
    /// Refuses an invalid native fee asset, lifetime or conflicting creation retry.
    pub fn start_native(
        scope: &mut PrincipalScope<'_>,
        request: &CreateAgentRequest,
        context: &CreationContext,
        catalog: &PurposePresetCatalog,
        now: u64,
        native_asset: [u8; 32],
        timestamp_span: u64,
    ) -> Result<Self, AgentCreationError> {
        if native_asset == [0; 32]
            || timestamp_span == 0
            || request
                .native_fee_budget
                .as_ref()
                .is_some_and(|fee| fee.asset_id != native_asset)
            || context
                .protocol_time
                .checked_add(timestamp_span)
                .and_then(|value| value.checked_mul(1_000))
                .is_none()
        {
            return Err(AgentCreationError::InvalidContext);
        }
        Self::start_with(
            scope,
            request,
            context,
            catalog,
            now,
            Some(NativeCreation {
                native_asset,
                timestamp_span,
            }),
        )
    }

    /// # Errors
    /// Refuses incomplete custody, unsigned effects, changed retries or insufficient finality.
    pub fn resume_native<C: NativeAgentCreationContract>(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        keystore: &Keystore,
        registry: &ModuleRegistry,
        agent: &mut C,
        now: u64,
    ) -> Result<CreationStatus, AgentCreationError> {
        if self.record.version == RECORD_VERSION {
            return self.resume(scope, keystore, registry, agent, now);
        }
        if self.record.version != 2 || self.record.native.is_none() {
            return Err(AgentCreationError::CorruptJourney);
        }
        self.ensure_custody(scope, keystore, now)?;
        let stages: Vec<_> = self
            .record
            .stages
            .iter()
            .map(|record| record.stage)
            .collect();
        for stage in stages.into_iter().skip(1) {
            if self.stage(stage).state == StageState::ReceiptVerified {
                continue;
            }
            let result = match stage {
                CreationStage::DidRegistration => self.native_onboard(scope, registry, agent, now),
                CreationStage::MainFunding | CreationStage::InitialFunding => {
                    self.native_fund(scope, registry, agent, stage, now)
                }
                CreationStage::RecoveryRegistration
                | CreationStage::AssetAccountOpening
                | CreationStage::BudgetCreation => {
                    self.native_protocol(scope, registry, agent, stage, now)
                }
                CreationStage::SessionProvision => self.run_session(scope, registry, agent, now),
                CreationStage::CapabilityNarrowing => self.run_capability(scope, agent, now),
                CreationStage::Custody | CreationStage::BudgetFunding => {
                    Err(AgentCreationError::InvalidStage)
                }
            };
            match result {
                Ok(()) => {}
                Err(AgentCreationError::Agent(failure)) => {
                    self.stage_mut(stage).state = match failure {
                        AgentFailure::Unavailable => StageState::Unavailable,
                        AgentFailure::Refused(_) => StageState::Refused,
                    };
                    self.persist(scope, now)?;
                    return Ok(self.status());
                }
                Err(error) => return Err(error),
            }
        }
        Ok(self.status())
    }

    fn native_mode(&self) -> Result<&NativeCreation, AgentCreationError> {
        self.record
            .native
            .as_ref()
            .ok_or(AgentCreationError::CorruptJourney)
    }

    fn native_account(&self, did: &Did, asset: [u8; 32]) -> Result<AccountId, AgentCreationError> {
        AccountId::for_asset(
            std::str::from_utf8(did.as_bytes()).map_err(|_| AgentCreationError::InvalidContext)?,
            asset,
            self.native_mode()?.native_asset,
        )
        .map_err(|_| AgentCreationError::InvalidContext)
    }

    fn sponsor(&self) -> Result<Did, AgentCreationError> {
        let did = self
            .record
            .owner_account
            .strip_prefix("agent:")
            .and_then(|name| name.strip_suffix(":main"))
            .ok_or(AgentCreationError::InvalidContext)?;
        Did::new(did.as_bytes()).map_err(|_| AgentCreationError::InvalidContext)
    }

    fn native_onboard<C: NativeAgentCreationContract>(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        registry: &ModuleRegistry,
        agent: &mut C,
        now: u64,
    ) -> Result<(), AgentCreationError> {
        let stage = CreationStage::DidRegistration;
        let consent = OnboardingConsent {
            sponsor: layerx_intents::canonical::did_id_for_protocol(&self.sponsor()?, 3)
                .map_err(|_| AgentCreationError::InvalidContext)?,
            target: self.did()?,
            target_public_key: self
                .record
                .public_key
                .ok_or(AgentCreationError::CustodyRequired)?,
            native_asset: self.native_mode()?.native_asset,
            action_key: self.stage(stage).action_key,
            expires_at: self
                .record
                .started_at
                .checked_add(self.native_mode()?.timestamp_span)
                .and_then(|value| value.checked_mul(1_000))
                .ok_or(AgentCreationError::InvalidContext)?,
        };
        let evidence = agent.onboard_scoped(
            scope,
            NativeOnboardingRequest {
                consent: consent.clone(),
                network_id: self.record.network_id,
                started_at: self.record.started_at,
                custody_key: KeyId::new(format!("agent-{}", short_hex(&self.record.agent_id)))?,
            },
        )?;
        let kind = ActivityType::new(layerx_types::payload::ModuleId::Governance, 1)
            .map_err(|_| AgentCreationError::InvalidStage)?;
        let activity = evidence.bound_activity(kind)?;
        let registration = SponsoredRegistration::decode(activity.payload())
            .map_err(|_| AgentCreationError::EvidenceConflict)?;
        if registration.consent != consent
            || registration.validate_outer(&activity).is_err()
            || activity.actor_did() != self.sponsor()?.as_bytes()
        {
            return Err(AgentCreationError::EvidenceConflict);
        }
        let intent = Intent::v3(IntentKind::NativeOnboarding(registration));
        let compiled = compile(&intent, registry)?;
        if activity.payload() != compiled.payload().as_bytes() {
            return Err(AgentCreationError::EvidenceConflict);
        }
        self.accept_native(scope, stage, kind, &evidence, now)
    }

    fn native_fund<C: NativeAgentCreationContract>(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        registry: &ModuleRegistry,
        agent: &mut C,
        stage: CreationStage,
        now: u64,
    ) -> Result<(), AgentCreationError> {
        let native = self.native_mode()?.native_asset;
        let asset = if stage == CreationStage::MainFunding {
            native
        } else {
            self.record.preset.budget_asset
        };
        let initial = if asset == self.record.preset.budget_asset {
            self.record.preset.initial_funding
        } else {
            0
        };
        let fees = if stage == CreationStage::MainFunding {
            self.record
                .native_fee_budget
                .as_ref()
                .map_or(0, |fee| fee.maximum_total)
        } else {
            0
        };
        let amount = initial
            .checked_add(fees)
            .filter(|value| *value != 0)
            .ok_or(AgentCreationError::InvalidRequest)?;
        let request = NativeFundingRequest {
            stage,
            source: self.native_account(&self.sponsor()?, asset)?,
            destination: self.native_account(&self.did()?, asset)?,
            asset,
            amount,
            action_key: self.stage(stage).action_key,
            network_id: self.record.network_id,
            started_at: self.record.started_at,
        };
        let funded = agent.fund_scoped(scope, &request)?;
        let (from, to, actual_asset, actual_amount, _, key, _, _, _, network, protocol) =
            funded.intent.to_wire_parts();
        if from != &request.source
            || to != &request.destination
            || actual_asset.bytes() != asset
            || actual_amount.value() != amount
            || key.bytes() != request.action_key
            || network.value() != request.network_id
            || protocol.value() != 3
            || funded.evidence.actor != self.sponsor()?.as_bytes()
        {
            return Err(AgentCreationError::EvidenceConflict);
        }
        let intent = Intent::v1(IntentKind::LxpSend(funded.intent));
        let compiled = compile(&intent, registry)?;
        if funded
            .evidence
            .bound_activity(compiled.activity_type())?
            .payload()
            != compiled.payload().as_bytes()
        {
            return Err(AgentCreationError::EvidenceConflict);
        }
        self.accept_native(
            scope,
            stage,
            compiled.activity_type(),
            &funded.evidence,
            now,
        )
    }

    fn native_protocol<C: NativeAgentCreationContract>(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        registry: &ModuleRegistry,
        agent: &mut C,
        stage: CreationStage,
        now: u64,
    ) -> Result<(), AgentCreationError> {
        let intent = match stage {
            CreationStage::RecoveryRegistration => {
                Intent::v3(IntentKind::RecoveryRegistration(RecoveryRegistration::new(
                    self.did()?,
                    RecoveryRoot::new(self.record.recovery_root),
                    ApprovalThreshold::new(self.record.recovery_threshold)
                        .map_err(|_| AgentCreationError::InvalidContext)?,
                )?))
            }
            CreationStage::AssetAccountOpening => Intent::v3(IntentKind::NativeAssetAccountOpen(
                AssetId::new(self.record.preset.budget_asset),
            )),
            CreationStage::BudgetCreation => {
                let source = self.native_account(&self.did()?, self.record.preset.budget_asset)?;
                let revision = NativeIdentityRevisionRequest {
                    did: self.did()?,
                    public_key: self
                        .record
                        .public_key
                        .ok_or(AgentCreationError::EvidenceConflict)?,
                    minimum_sequence: self.native_recovery_sequence(scope)?,
                    action_key: self.stage(stage).action_key,
                    started_at: self.record.started_at,
                };
                let revocation_sequence = agent.identity_revision_scoped(scope, &revision)?;
                let sequence = agent.source_sequence_scoped(
                    scope,
                    &source,
                    self.stage(stage).action_key,
                    self.record.started_at,
                )?;
                Intent::v3(IntentKind::NativeBudgetCreate(NativeBudgetCreate {
                    budget_id: self.record.agent_id,
                    budget_account: self.budget_account()?,
                    asset: self.record.preset.budget_asset,
                    purpose: self.record.preset.config_digest,
                    per_period_limit: self.record.monthly_spend,
                    carry_cap: 0,
                    initial_amount: self.record.preset.initial_funding,
                    period_length_ms: self
                        .record
                        .preset
                        .budget_period_seconds
                        .checked_mul(1_000)
                        .ok_or(AgentCreationError::InvalidPresetConfig)?,
                    period_start_ms: self
                        .record
                        .started_at
                        .checked_mul(1_000)
                        .ok_or(AgentCreationError::InvalidContext)?,
                    expiry_ms: self
                        .record
                        .started_at
                        .checked_add(self.record.preset.budget_expiry_seconds)
                        .and_then(|value| value.checked_mul(1_000))
                        .ok_or(AgentCreationError::InvalidPresetConfig)?,
                    revocation_sequence,
                    rollover: 1,
                    source_account: source,
                    source_sequence: sequence,
                }))
            }
            _ => return Err(AgentCreationError::InvalidStage),
        };
        let compiled = compile(&intent, registry)?;
        let disclosure = DisclosureCheck::verify(&intent, &compiled)?;
        let action = ProtocolAction {
            actor: Some(self.did()?),
            stage,
            action_key: self.stage(stage).action_key,
            intent,
            compiled,
            disclosure,
            custody_key: KeyId::new(format!("agent-{}", short_hex(&self.record.agent_id)))?,
            started_at: self.record.started_at,
        };
        let kind = action.compiled.activity_type();
        let payload = action.compiled.payload().as_bytes().to_vec();
        let evidence = agent.submit_protocol_scoped(scope, action)?;
        if evidence.actor != self.record.did
            || evidence.bound_activity(kind)?.payload() != payload
            || Some(evidence.owner_public_key) != self.record.public_key
        {
            return Err(AgentCreationError::EvidenceConflict);
        }
        self.accept_native(scope, stage, kind, &evidence, now)
    }

    fn accept_native(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        stage: CreationStage,
        kind: ActivityType,
        evidence: &ProtocolEvidence,
        now: u64,
    ) -> Result<(), AgentCreationError> {
        if evidence.action_key != self.stage(stage).action_key
            || evidence.network_id != self.record.network_id
        {
            return Err(AgentCreationError::EvidenceConflict);
        }
        if evidence.verification_level != VerificationLevel::CHECKPOINT_FINALISED {
            return Err(AgentFailure::Unavailable.into());
        }
        let verified = evidence.verify_outcome(kind)?;
        let protocol = verified
            .receipt()
            .protocol()
            .ok_or(AgentCreationError::EvidenceConflict)?;
        if protocol.activity_id() != evidence.activity_id
            || protocol.protocol_version() != 3
            || (kind.module() == layerx_types::payload::ModuleId::Asset
                && (protocol.module_id() != 1 || u16::from(protocol.operation()) != kind.ordinal()))
            || Some(u8::try_from(kind.ordinal()).map_err(|_| AgentCreationError::EvidenceConflict)?)
                != stage.operation()
        {
            return Err(AgentCreationError::EvidenceConflict);
        }
        let receipt_digest = Sha256::digest(verified.canonical_bytes()).into();
        put_exact(
            scope,
            evidence_row(self.record.agent_id, stage)?,
            now,
            verified.canonical_bytes().to_vec(),
        )?;
        put_exact(
            scope,
            RowKey::new(format!(
                "native-create-activity-{}-{}",
                full_hex(&self.record.agent_id),
                stage.code()
            ))?,
            now,
            evidence.signed_activity.clone(),
        )?;
        let progress = self.stage_mut(stage);
        if progress
            .evidence_digest
            .is_some_and(|old| old != receipt_digest)
        {
            return Err(AgentCreationError::EvidenceConflict);
        }
        progress.state = if protocol.result_code() == 0 {
            StageState::ReceiptVerified
        } else {
            StageState::Refused
        };
        progress.evidence_digest = Some(receipt_digest);
        progress.object_id = Some(evidence.activity_id);
        if stage == CreationStage::DidRegistration && protocol.result_code() == 0 {
            self.stage_mut(CreationStage::Custody).state = StageState::ReceiptVerified;
            self.stage_mut(CreationStage::Custody).evidence_digest = Some(receipt_digest);
        }
        self.persist(scope, now)?;
        if protocol.result_code() != 0 {
            return Err(AgentFailure::Refused("native creation operation was refused").into());
        }
        Ok(())
    }
}

pub(super) fn full_hex(bytes: &[u8; 32]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(64);
    for byte in bytes {
        text.push(char::from(DIGITS[usize::from(byte >> 4)]));
        text.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    text
}

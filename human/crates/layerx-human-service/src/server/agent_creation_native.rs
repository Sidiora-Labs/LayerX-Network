use super::*;
use crate::agents::{
    NativeAgentCreationContract, NativeFundingEvidence, NativeFundingRequest,
    NativeOnboardingRequest,
};
use crate::custody::{KeyClass, KeyId, SendPlanAuthorization};
use crate::store::{RowKey, Table};
use layerx_intents::{compile, DisclosureCheck, LxpSend};
use layerx_types::account::AccountId;
use layerx_types::ids::{AssetId, IdempotencyKey};
use layerx_types::intent::{
    AuthorizationSignature, ContextHash, NetworkId, ProtocolVersion, PublicKey, SendAuthorization,
    SendAuthorizationKind,
};

#[derive(Serialize, Deserialize)]
struct SourceSequence {
    account: [u8; 32],
    sequence: u64,
}

impl NativeAgentCreationContract for ProductionAgentCreation<'_> {
    fn source_sequence_scoped(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        account: &AccountId,
        action_key: [u8; 32],
        started_at: u64,
    ) -> Result<u64, AgentFailure> {
        let account_id = layerx_intents::canonical::account_id_for_protocol(account, 3)
            .map_err(|_| AgentFailure::Refused("invalid native source account"))?;
        let key = RowKey::new(format!("native-source-sequence-{}", hex(&action_key)))
            .map_err(|_| AgentFailure::Refused("invalid native source key"))?;
        let retained = if let Some(row) = scope.get(Table::Journeys, &key) {
            serde_json::from_slice::<SourceSequence>(row.bytes())
                .map_err(|_| AgentFailure::Refused("invalid retained source sequence"))?
        } else {
            let state = self
                .runtime
                .account_state(account_id)
                .map_err(map_boundary)?;
            if state.name != account.canonical().as_bytes()
                || state.account_id != account_id
                || state.frozen
                || state.authority_key.is_none()
            {
                return Err(AgentFailure::Refused("native source proof differs"));
            }
            let retained = SourceSequence {
                account: account_id,
                sequence: state.next_sequence,
            };
            scope
                .put(
                    Table::Journeys,
                    key,
                    started_at,
                    serde_json::to_vec(&retained)
                        .map_err(|_| AgentFailure::Refused("invalid native source preparation"))?,
                )
                .map_err(|_| AgentFailure::Unavailable)?;
            retained
        };
        if retained.account != account_id || retained.sequence == u64::MAX {
            return Err(AgentFailure::Refused("native source sequence changed"));
        }
        Ok(retained.sequence)
    }

    fn onboard_scoped(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        request: NativeOnboardingRequest,
    ) -> Result<ProtocolEvidence, AgentFailure> {
        let consent = self.sign_onboarding_consent(scope, &request)?;
        let registration =
            layerx_crypto::onboarding::SponsoredRegistration::from_signed_consent(&consent)
                .map_err(|_| AgentFailure::Refused("invalid signed target consent"))?;
        if registration.consent != request.consent || registration.network_id != request.network_id
        {
            return Err(AgentFailure::Refused("retained target consent differs"));
        }
        let sponsor = layerx_types::ids::Did::new(self.actor.as_str().as_bytes())
            .map_err(|_| AgentFailure::Refused("invalid registration sponsor"))?;
        if layerx_intents::canonical::did_id_for_protocol(&sponsor, 3)
            .map_err(|_| AgentFailure::Refused("invalid sponsor identity"))?
            != request.consent.sponsor
        {
            return Err(AgentFailure::Refused("registration sponsor differs"));
        }
        let action = self.native_action(
            Intent::v3(IntentKind::NativeOnboarding(registration)),
            CreationStage::DidRegistration,
            request.consent.action_key,
            request.started_at,
            KeyId::new("human-primary")
                .map_err(|_| AgentFailure::Refused("invalid sponsor key"))?,
        )?;
        self.submit_scoped(scope, &action)
    }

    fn fund_scoped(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        request: &NativeFundingRequest,
    ) -> Result<NativeFundingEvidence, AgentFailure> {
        let key = KeyId::new("human-primary")
            .map_err(|_| AgentFailure::Refused("invalid funding key"))?;
        let descriptor = self
            .custody
            .describe_key(scope.principal(), &key)
            .map_err(|_| AgentFailure::Refused("funding custody is unavailable"))?;
        let binding = self
            .custody
            .evm_binding(scope.principal(), &key)
            .map_err(|_| AgentFailure::Refused("funding custody binding is unavailable"))?;
        if descriptor.class != KeyClass::HumanPrimary
            || binding.class() != KeyClass::HumanPrimary
            || binding.network_id() != request.network_id
        {
            return Err(AgentFailure::Refused("funding custody binding differs"));
        }
        let native_asset = self
            .runtime
            .native_fee_policy()
            .map_err(map_boundary)?
            .asset_id;
        request
            .source
            .matches_asset(self.actor.as_str(), request.asset, native_asset)
            .map_err(|_| AgentFailure::Refused("funding source owner differs"))?;
        let from = layerx_intents::canonical::account_id_for_protocol(&request.source, 3)
            .map_err(|_| AgentFailure::Refused("invalid funding source"))?;
        let to = layerx_intents::canonical::account_id_for_protocol(&request.destination, 3)
            .map_err(|_| AgentFailure::Refused("invalid funding destination"))?;
        let sequence = self.source_sequence_scoped(
            scope,
            &request.source,
            request.action_key,
            request.started_at,
        )?;
        let not_after = request
            .started_at
            .checked_add(self.timestamp_span)
            .ok_or(AgentFailure::Refused("funding expiry overflow"))?;
        let expires_at = not_after
            .checked_mul(1_000)
            .ok_or(AgentFailure::Refused("funding expiry overflow"))?;
        let context = layerx_crypto::send::send_context_hash(
            &from,
            &to,
            &request.asset,
            request.amount,
            &request.action_key,
        );
        let authorization = SendPlanAuthorization {
            plan_id: request.action_key,
            action_key: request.action_key,
            principal: scope.principal().as_str().to_owned(),
            tenant: scope.tenant().as_str().to_owned(),
            binding_digest: binding.digest(),
            from,
            to,
            asset: request.asset,
            amount: request.amount,
            sequence,
            idempotency_key: request.action_key,
            expires_at,
            context,
            network: request.network_id,
            protocol: 3,
            not_before: request.started_at,
            not_after,
        };
        let signature_key = RowKey::new(format!(
            "native-funding-signature-{}",
            hex(&request.action_key)
        ))
        .map_err(|_| AgentFailure::Refused("invalid funding signature key"))?;
        let signature = if let Some(row) = scope.get(Table::Journeys, &signature_key) {
            <[u8; 64]>::try_from(row.bytes())
                .map_err(|_| AgentFailure::Refused("invalid retained funding signature"))?
        } else {
            let signature = self
                .custody
                .authorize_send(scope.principal(), &key, &authorization)
                .map_err(|_| AgentFailure::Refused("custody refused native funding"))?;
            scope
                .put(
                    Table::Journeys,
                    signature_key,
                    request.started_at,
                    signature.to_vec(),
                )
                .map_err(|_| AgentFailure::Unavailable)?;
            signature
        };
        let debit = layerx_crypto::send::SendDebit {
            from,
            to,
            asset: request.asset,
            amount: request.amount,
            source_sequence: sequence,
            idempotency_key: request.action_key,
            expires_at,
            context_hash: context,
            conditions: Vec::new(),
            authorization_kind: 1,
            network_id: request.network_id,
            protocol_version: 3,
        };
        layerx_intents::canonical::signed_send_payload(&debit, descriptor.public_key, signature)
            .map_err(|_| AgentFailure::Refused("retained funding signature differs"))?;
        let intent = LxpSend::new(
            request.source.clone(),
            request.destination.clone(),
            AssetId::new(request.asset),
            layerx_types::amount::Amount::from_u128(request.amount),
            layerx_types::intent::Sequence::from_u64(sequence),
            IdempotencyKey::new(request.action_key),
            layerx_types::intent::TimestampSeconds::from_u64(expires_at),
            ContextHash::new(context),
            SendAuthorization::new(
                SendAuthorizationKind::Owner,
                PublicKey::new(descriptor.public_key),
                AuthorizationSignature::new(signature),
            ),
            NetworkId::new(request.network_id)
                .map_err(|_| AgentFailure::Refused("invalid funding network"))?,
            ProtocolVersion::new(3)
                .map_err(|_| AgentFailure::Refused("invalid funding protocol"))?,
        )
        .map_err(|_| AgentFailure::Refused("invalid native funding intent"))?;
        let action = self.native_action(
            Intent::v1(IntentKind::LxpSend(intent.clone())),
            request.stage,
            request.action_key,
            request.started_at,
            key,
        )?;
        let evidence = self.submit_scoped(scope, &action)?;
        Ok(NativeFundingEvidence { evidence, intent })
    }
}

impl ProductionAgentCreation<'_> {
    fn retained_protocol_evidence(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        action: &ProtocolAction,
    ) -> Result<Option<ProtocolEvidence>, AgentFailure> {
        let key = RowKey::new(format!("protocol-signed-{}", hex(&action.action_key)))
            .map_err(|_| AgentFailure::Refused("invalid protocol action key"))?;
        let Some(row) = scope.get(Table::Journeys, &key) else {
            return Ok(None);
        };
        let bytes = row.bytes().to_vec();
        let activity = layerx_intents::owner_activity::verify(&bytes, self.runtime.registry())
            .map_err(|_| AgentFailure::Refused("invalid retained owner activity"))?;
        let descriptor = self.signing_descriptor(scope.principal(), action)?;
        if activity.actor_did() != self.actor.as_str().as_bytes()
            || activity.authority() != descriptor.public_key
            || activity.payload() != action.compiled.payload().as_bytes()
            || activity.payload_hash() != action.compiled.payload_hash()
            || activity.activity_type() != action.compiled.activity_type()
            || activity.idempotency_key() != action.action_key
        {
            return Err(AgentFailure::Refused("retained owner request differs"));
        }
        let activity_id = layerx_intents::canonical::activity_id(&activity)
            .map_err(|_| AgentFailure::Refused("invalid retained owner identity"))?;
        let material = match self
            .runtime
            .receipt_by_idempotency_key(action.action_key, activity_id)
            .map_err(map_boundary)?
        {
            ReceiptLookup::Absent => return Ok(None),
            ReceiptLookup::Found(value) => value,
        };
        Ok(Some(ProtocolEvidence {
            actor: activity.actor_did().to_vec(),
            owner_public_key: descriptor.public_key,
            network_id: activity.network_id(),
            signed_activity: bytes,
            action_key: action.action_key,
            activity_id,
            receipt_bytes: material.canonical_bytes,
            authorized_batch: material.authorised_batch,
            verification_level: material.verification_level,
        }))
    }

    fn native_action(
        &self,
        intent: Intent,
        stage: CreationStage,
        action_key: [u8; 32],
        started_at: u64,
        custody_key: KeyId,
    ) -> Result<ProtocolAction, AgentFailure> {
        let compiled = compile(&intent, self.runtime.registry())
            .map_err(|_| AgentFailure::Refused("native creation compilation failed"))?;
        let disclosure = DisclosureCheck::verify(&intent, &compiled)
            .map_err(|_| AgentFailure::Refused("native creation disclosure differs"))?;
        Ok(ProtocolAction {
            actor: None,
            stage,
            action_key,
            intent,
            compiled,
            disclosure,
            custody_key,
            started_at,
        })
    }

    fn sign_onboarding_consent(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        request: &NativeOnboardingRequest,
    ) -> Result<Vec<u8>, AgentFailure> {
        let key = RowKey::new(format!(
            "native-target-consent-{}",
            hex(&request.consent.action_key)
        ))
        .map_err(|_| AgentFailure::Refused("invalid target consent key"))?;
        if let Some(row) = scope.get(Table::Journeys, &key) {
            return Ok(row.bytes().to_vec());
        }
        let descriptor = self
            .custody
            .describe_key(scope.principal(), &request.custody_key)
            .map_err(|_| AgentFailure::Refused("target custody is unavailable"))?;
        if descriptor.class != KeyClass::AgentPrimary
            || descriptor.public_key != request.consent.target_public_key
        {
            return Err(AgentFailure::Refused("target custody differs"));
        }
        let compiled = compile(
            &Intent::v3(IntentKind::NativeOnboardingConsent(request.consent.clone())),
            self.runtime.registry(),
        )
        .map_err(|_| AgentFailure::Refused("invalid target consent intent"))?;
        let context = layerx_intents::owner_activity::OwnerEnvelopeContext {
            actor: request.consent.target.clone(),
            owner_public_key: descriptor.public_key,
            network_id: request.network_id,
            account_sequence: 0,
            not_before_ms: request
                .started_at
                .checked_mul(1_000)
                .ok_or(AgentFailure::Refused("target consent expiry overflow"))?,
            not_after_ms: request.consent.expires_at,
            action_key: request.consent.action_key,
            fee_limit: 0,
        };
        let (unsigned, disclosure) = layerx_intents::owner_activity::unsigned_native(
            &compiled,
            &context,
            self.runtime.registry(),
        )
        .map_err(|_| AgentFailure::Refused("target consent disclosure failed"))?;
        let principal = scope.principal().clone();
        let grant = poll_once_ready(self.custody.sign_in_scope(
            scope,
            SignRequest::new(
                &principal,
                &request.custody_key,
                self.trace,
                SignAuthorization::new(Operation::ProtocolMutation, None),
                &unsigned,
                &disclosure,
                request.started_at,
            ),
        ))
        .map_err(|_| AgentFailure::Unavailable)?
        .map_err(|_| AgentFailure::Refused("custody refused target consent"))?;
        let signed = layerx_intents::owner_activity::attach_signature(
            &unsigned,
            *grant.signature(),
            grant.signer_public_key(),
            self.runtime.registry(),
        )
        .map_err(|_| AgentFailure::Refused("target consent signature differs"))?;
        scope
            .put(Table::Journeys, key, request.started_at, signed.clone())
            .map_err(|_| AgentFailure::Unavailable)?;
        Ok(signed)
    }
}

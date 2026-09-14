use super::{
    consume_rate_in_scope, AuditChain, AuditEvent, AuditStepUpEvidence, CustodyError,
    CustodySigner, Decision, KeyId, PrincipalScope, SigningOperation, TraceId,
};
use layerx_crypto::settlement_recipient::RecipientAuthorization;
use sha2::{Digest as _, Sha256};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettlementRecipientRequest {
    pub checkpoint: [u8; 32],
    pub asset: [u8; 32],
    pub recipient: [u8; 20],
}

impl CustodySigner {
    /// # Errors
    /// Refuses absent principal onboarding, wrong custody or recipient bindings and audit failures.
    pub fn settlement_recipient_in_scope(
        &self,
        scope: &mut PrincipalScope<'_>,
        key: &KeyId,
        request: SettlementRecipientRequest,
        trace: &TraceId,
        now: u64,
    ) -> Result<[u8; 64], CustodyError> {
        let (authorization, reference) = self.recipient_authorization(scope, key, request)?;
        let encoded = authorization
            .encode()
            .map_err(|_| CustodyError::InvalidEvidence)?;
        let digest = Sha256::digest(&encoded).into();
        let result = consume_rate_in_scope(scope, self.limits, now).and_then(|()| {
            let binding = self.keystore.evm_binding(scope.principal(), key)?;
            let bytes = self
                .keystore
                .provider
                .evm_operation(13, &binding, &reference, &encoded)
                .map_err(CustodyError::Kms)?;
            let signature: [u8; 64] = bytes
                .try_into()
                .map_err(|_| CustodyError::InvalidEvidence)?;
            authorization
                .verify_signature(&signature)
                .map_err(|_| CustodyError::InvalidEvidence)?;
            Ok(signature)
        });
        let event = AuditEvent::SigningDecision {
            operation: SigningOperation::EvmPayoutBinding,
            disclosure_digest: digest,
            step_up: AuditStepUpEvidence::NotRequired,
            outcome: if result.is_ok() {
                Decision::Granted
            } else {
                Decision::Refused
            },
        };
        AuditChain::open(scope)
            .map_err(CustodyError::Audit)?
            .append(scope, now, trace, &event, &[])
            .map_err(CustodyError::Audit)?;
        result
    }

    fn recipient_authorization(
        &self,
        scope: &PrincipalScope<'_>,
        key: &KeyId,
        request: SettlementRecipientRequest,
    ) -> Result<(RecipientAuthorization, super::super::ProviderKeyReference), CustodyError> {
        let journey = crate::onboarding::OnboardingJourney::load(scope)
            .map_err(|_| CustodyError::InvalidEvidence)?
            .ok_or(CustodyError::InvalidEvidence)?;
        let did = journey.did().map_err(|_| CustodyError::InvalidEvidence)?;
        let descriptor = self.keystore.describe(scope.principal(), key)?;
        let binding = self.keystore.evm_binding(scope.principal(), key)?;
        if key.as_str() != "human-primary"
            || descriptor.class != super::super::KeyClass::HumanPrimary
            || binding.class() != super::super::KeyClass::HumanPrimary
            || self.keystore.evm_wallet(scope.principal(), key)? != request.recipient
        {
            return Err(CustodyError::InvalidEvidence);
        }
        Ok((
            RecipientAuthorization {
                network_id: binding.network_id(),
                binding_digest: binding.digest(),
                did,
                public_key: descriptor.public_key,
                asset: request.asset,
                checkpoint: request.checkpoint,
                recipient: request.recipient,
            },
            self.keystore
                .evm_provider_reference(scope.principal(), key)?,
        ))
    }
}

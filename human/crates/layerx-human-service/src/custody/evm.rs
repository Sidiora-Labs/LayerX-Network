use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvmTransaction {
    pub chain_id: u64,
    pub nonce: u64,
    pub max_priority_fee_per_gas: u64,
    pub max_fee_per_gas: u64,
    pub gas_limit: u64,
    pub to: [u8; 20],
    pub value: [u8; 32],
    pub calldata: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvmPlanAuthorization {
    pub plan_id: [u8; 32],
    pub action_key: [u8; 32],
    pub tenant: String,
    pub principal: String,
    pub binding_digest: [u8; 32],
    pub wallet: [u8; 20],
    pub not_before: u64,
    pub not_after: u64,
    pub transaction: EvmTransaction,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvmAction {
    pub authorization: EvmPlanAuthorization,
    pub raw_transaction: Vec<u8>,
    pub transaction_hash: Option<[u8; 32]>,
    pub acknowledged: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvmAcknowledgement {
    pub action_key: [u8; 32],
    pub transaction_hash: [u8; 32],
}

use super::{CustodyError, KeyId, Keystore, KmsError};
use crate::store::PrincipalId;

impl Keystore {
    /// Returns the existing opaque provider handle after binding validation.
    /// # Errors
    /// Refuses missing or mismatched custody records.
    pub fn evm_provider_reference(
        &self,
        principal: &PrincipalId,
        key: &KeyId,
    ) -> Result<super::ProviderKeyReference, CustodyError> {
        let record = self.read_record(principal, key)?;
        Self::require_record_binding(&self.binding(principal, key, record.class)?, &record)?;
        Ok(record.provider_reference)
    }

    /// Resolves the EVM wallet attached to an existing principal custody key.
    /// # Errors
    /// Refuses absent, destroyed or mismatched custody records.
    pub fn evm_wallet(
        &self,
        principal: &PrincipalId,
        key: &KeyId,
    ) -> Result<[u8; 20], CustodyError> {
        self.evm_call(principal, key, 6, &[])?
            .try_into()
            .map_err(|_| CustodyError::Kms(KmsError::InvalidResponse))
    }
    /// Returns the custody binding required by an exact authorized plan.
    /// # Errors
    /// Refuses missing or mismatched custody records.
    pub fn evm_binding(
        &self,
        principal: &PrincipalId,
        key: &KeyId,
    ) -> Result<super::PrincipalKeyBinding, CustodyError> {
        let record = self.read_record(principal, key)?;
        let binding = self.binding(principal, key, record.class)?;
        Self::require_record_binding(&binding, &record)?;
        Ok(binding)
    }
    /// Persists an authorized exact transaction and reserves its nonce.
    /// # Errors
    /// Refuses expired plans, wrong bindings or conflicting action keys/nonces.
    pub fn authorize_evm_plan(
        &self,
        principal: &PrincipalId,
        key: &KeyId,
        authorization: &EvmPlanAuthorization,
    ) -> Result<EvmAction, CustodyError> {
        if authorization.principal != principal.as_str() {
            return Err(CustodyError::Kms(KmsError::Refused));
        }
        self.evm_json(principal, key, 7, authorization)
    }
    /// Signs a previously authorized transaction, returning the durable result.
    /// # Errors
    /// Refuses unregistered or expired actions.
    pub fn sign_evm_action(
        &self,
        principal: &PrincipalId,
        key: &KeyId,
        action: &[u8; 32],
    ) -> Result<EvmAction, CustodyError> {
        self.evm_json(principal, key, 8, action)
    }
    /// Records submission only for the exact signed transaction hash.
    /// # Errors
    /// Refuses unregistered actions or mismatched hashes.
    pub fn acknowledge_evm_action(
        &self,
        principal: &PrincipalId,
        key: &KeyId,
        ack: &EvmAcknowledgement,
    ) -> Result<EvmAction, CustodyError> {
        self.evm_json(principal, key, 9, ack)
    }
    /// Recovers the authorization, signed bytes and submission journal state.
    /// # Errors
    /// Refuses absent actions or mismatched principal custody keys.
    pub fn recover_evm_action(
        &self,
        principal: &PrincipalId,
        key: &KeyId,
        action: &[u8; 32],
    ) -> Result<EvmAction, CustodyError> {
        self.evm_json(principal, key, 10, action)
    }
    fn evm_json<T: Serialize>(
        &self,
        principal: &PrincipalId,
        key: &KeyId,
        operation: u8,
        value: &T,
    ) -> Result<EvmAction, CustodyError> {
        let payload = serde_json::to_vec(value)
            .map_err(|_| CustodyError::Kms(KmsError::InvalidConfiguration))?;
        serde_json::from_slice(&self.evm_call(principal, key, operation, &payload)?)
            .map_err(|_| CustodyError::Kms(KmsError::InvalidResponse))
    }
    fn evm_call(
        &self,
        principal: &PrincipalId,
        key: &KeyId,
        operation: u8,
        payload: &[u8],
    ) -> Result<Vec<u8>, CustodyError> {
        let record = self.read_record(principal, key)?;
        let binding = self.binding(principal, key, record.class)?;
        Self::require_record_binding(&binding, &record)?;
        self.provider
            .evm_operation(operation, &binding, &record.provider_reference, payload)
            .map_err(CustodyError::Kms)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SendPlanAuthorization {
    pub plan_id: [u8; 32],
    pub action_key: [u8; 32],
    pub principal: String,
    pub tenant: String,
    pub binding_digest: [u8; 32],
    pub from: [u8; 32],
    pub to: [u8; 32],
    pub asset: [u8; 32],
    pub amount: u128,
    pub sequence: u64,
    pub idempotency_key: [u8; 32],
    pub expires_at: u64,
    pub context: [u8; 32],
    pub network: u32,
    pub protocol: u16,
    pub not_before: u64,
    pub not_after: u64,
}
impl Keystore {
    /// Authorizes a canonical owner SEND message for an approved plan.
    /// # Errors
    /// Refuses wrong principals, expiry, protocol scope and action conflicts.
    pub fn authorize_send(
        &self,
        principal: &PrincipalId,
        key: &KeyId,
        authorization: &SendPlanAuthorization,
    ) -> Result<[u8; 64], CustodyError> {
        if authorization.principal != principal.as_str() {
            return Err(CustodyError::Kms(KmsError::Refused));
        }
        let payload = serde_json::to_vec(authorization)
            .map_err(|_| CustodyError::Kms(KmsError::InvalidConfiguration))?;
        self.evm_call(principal, key, 11, &payload)?
            .try_into()
            .map_err(|_| CustodyError::Kms(KmsError::InvalidResponse))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvmExternalSignature {
    pub action_key: [u8; 32],
    pub signature: Vec<u8>,
}

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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvmExternalSignature {
    pub action_key: [u8; 32],
    pub signature: Vec<u8>,
}

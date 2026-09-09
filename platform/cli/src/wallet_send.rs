use layerx_types::payload::{ActivityType, ModuleId};
use serde_json::{json, Value};

use crate::wallet_signing::{PreparedPayment, SigningFacts};

#[derive(Clone, Debug)]
pub struct Send {
    pub from: [u8; 32],
    pub to: [u8; 32],
    pub asset: [u8; 32],
    pub amount: u128,
    pub source_next_sequence: u64,
    pub idempotency_key: [u8; 32],
    pub expires_at: u64,
    pub context_hash: [u8; 32],
    pub conditions: Vec<(u8, u64)>,
    pub authorization: Authorization,
}

#[derive(Clone, Debug)]
pub struct Authorization {
    pub kind: u8,
    pub controller: [u8; 32],
    pub public_key: [u8; 32],
    pub signature: [u8; 64],
    pub signed_context_hash: [u8; 32],
    pub network_id: u32,
    pub protocol_version: u16,
}

impl Send {
    /// # Errors
    /// Refuses invalid bounds, conditions, amounts, or authorization bindings.
    pub fn encode(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        self.debit()
            .encode_signed(self.authorization.public_key, self.authorization.signature)
            .map_err(|e| format!("{e:?}"))
    }

    /// # Errors
    /// Refuses invalid source and context bindings before using the shared encoder.
    pub fn authorization_message(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        self.debit()
            .authorization_message()
            .map_err(|e| format!("{e:?}"))
    }

    fn debit(&self) -> layerx_crypto::send::SendDebit {
        layerx_crypto::send::SendDebit {
            from: self.from,
            to: self.to,
            asset: self.asset,
            amount: self.amount,
            source_sequence: self.source_next_sequence,
            idempotency_key: self.idempotency_key,
            expires_at: self.expires_at,
            context_hash: self.context_hash,
            conditions: self
                .conditions
                .iter()
                .map(|&(kind, timestamp)| layerx_crypto::send::SendCondition { kind, timestamp })
                .collect(),
            authorization_kind: self.authorization.kind,
            network_id: self.authorization.network_id,
            protocol_version: self.authorization.protocol_version,
        }
    }

    fn validate(&self) -> Result<(), String> {
        if self.amount == 0 || self.from == self.to {
            return Err("Send requires a positive amount and different accounts".into());
        }
        if self.conditions.len() > 8
            || self
                .conditions
                .iter()
                .any(|(kind, _)| !matches!(kind, 1 | 2))
        {
            return Err("Send permits at most eight not-before/not-after conditions".into());
        }
        if !(1..=6).contains(&self.authorization.kind)
            || self.authorization.controller != self.from
            || self.authorization.signed_context_hash != self.context_hash
        {
            return Err("Send authorization does not bind the source and context".into());
        }
        Ok(())
    }

    /// # Errors
    /// Requires the payload authorization to match the independent envelope facts.
    pub fn prepare(&self, facts: &SigningFacts<'_>) -> Result<PreparedPayment, String> {
        if self.authorization.protocol_version != 3
            || self.authorization.network_id != facts.network_id
            || self.authorization.public_key != facts.public_key
            || self.idempotency_key != facts.idempotency_key
            || self.expires_at != facts.expires_at_ms
        {
            return Err("Send authorization and envelope facts differ".into());
        }
        let kind = ActivityType::new(ModuleId::Asset, 5).map_err(|e| format!("{e:?}"))?;
        PreparedPayment::from_encoded(kind, &self.encode()?, facts, Some(self.disclosure()))
    }

    #[must_use]
    pub fn disclosure(&self) -> Value {
        json!({
            "from": hex(&self.from), "to": hex(&self.to), "asset": hex(&self.asset),
            "amount": self.amount.to_string(),
            "payload_sequence": self.source_next_sequence.to_string(),
            "idempotency_key": hex(&self.idempotency_key),
            "expires_at": self.expires_at.to_string(), "context_hash": hex(&self.context_hash),
            "conditions": self.conditions.iter().map(|(kind, timestamp)| json!({"kind":kind,"timestamp":timestamp.to_string()})).collect::<Vec<_>>(),
            "authorization": {
                "kind":self.authorization.kind, "controller":hex(&self.authorization.controller),
                "public_key":hex(&self.authorization.public_key), "signature":hex(&self.authorization.signature),
                "signed_context_hash":hex(&self.authorization.signed_context_hash),
                "network_id":self.authorization.network_id, "protocol_version":self.authorization.protocol_version
            }
        })
    }
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(char::from(DIGITS[usize::from(byte >> 4)]));
        text.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    text
}

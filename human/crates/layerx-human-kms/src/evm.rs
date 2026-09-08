use crate::evm_types::{EvmAction, EvmPlanAuthorization, EvmTransaction};
use crate::wire::{Error, Result};
use k256::ecdsa::SigningKey;
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};
use std::collections::BTreeMap;
use zeroize::Zeroize;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Wallet {
    pub seed: [u8; 32],
    pub address: [u8; 20],
    pub next_nonce: BTreeMap<u64, u64>,
    pub actions: BTreeMap<String, EvmAction>,
}
impl Drop for Wallet {
    fn drop(&mut self) {
        self.seed.zeroize();
    }
}
impl Wallet {
    pub fn create() -> Result<Self> {
        let mut seed = zeroize::Zeroizing::new([0; 32]);
        getrandom::fill(&mut *seed).map_err(|_| Error::Unavailable)?;
        let key = SigningKey::from_bytes((&*seed).into()).map_err(|_| Error::Unavailable)?;
        let encoded = key.verifying_key().to_encoded_point(false);
        let digest = Keccak256::digest(&encoded.as_bytes()[1..]);
        let address = digest[12..].try_into().map_err(|_| Error::Integrity)?;
        Ok(Self {
            seed: *seed,
            address,
            next_nonce: BTreeMap::new(),
            actions: BTreeMap::new(),
        })
    }
    pub fn validate(&self, binding: [u8; 32]) -> Result<()> {
        let key = SigningKey::from_bytes((&self.seed).into()).map_err(|_| Error::Integrity)?;
        let encoded = key.verifying_key().to_encoded_point(false);
        if Keccak256::digest(&encoded.as_bytes()[1..])[12..] != self.address
            || self.actions.len() > 4096
        {
            return Err(Error::Integrity);
        }
        for (id, action) in &self.actions {
            let authorization = &action.authorization;
            if id != &crate::store::hex(&authorization.action_key)
                || authorization.wallet != self.address
                || authorization.binding_digest != binding
                || self
                    .next_nonce
                    .get(&authorization.transaction.chain_id)
                    .is_none_or(|next| *next <= authorization.transaction.nonce)
                || action.raw_transaction.is_empty() != action.transaction_hash.is_none()
                || (action.acknowledged && action.transaction_hash.is_none())
            {
                return Err(Error::Integrity);
            }
            if !action.raw_transaction.is_empty()
                && action.transaction_hash
                    != Some(Keccak256::digest(&action.raw_transaction).into())
            {
                return Err(Error::Integrity);
            }
        }
        Ok(())
    }
    pub fn authorize(
        &mut self,
        authorization: EvmPlanAuthorization,
        now: u64,
    ) -> Result<EvmAction> {
        let key = crate::store::hex(&authorization.action_key);
        if let Some(previous) = self.actions.get(&key) {
            return if previous.authorization == authorization {
                Ok(previous.clone())
            } else {
                Err(Error::Conflict)
            };
        }
        let transaction = &authorization.transaction;
        if authorization.plan_id == [0; 32]
            || authorization.action_key == [0; 32]
            || authorization.tenant.is_empty()
            || authorization.tenant.len() > 128
            || authorization.principal.is_empty()
            || authorization.principal.len() > 128
            || authorization.wallet != self.address
            || authorization.not_before > now
            || authorization.not_after < now
            || authorization.not_after <= authorization.not_before
            || transaction.chain_id == 0
            || transaction.to == [0; 20]
            || transaction.gas_limit < 21_000
            || transaction.max_fee_per_gas == 0
            || transaction.max_priority_fee_per_gas > transaction.max_fee_per_gas
            || transaction.calldata.len() > 524_288
            || self.actions.len() >= 4096
        {
            return Err(Error::Refused);
        }
        if self
            .next_nonce
            .get(&transaction.chain_id)
            .is_some_and(|next| transaction.nonce < *next)
        {
            return Err(Error::Conflict);
        }
        let next = transaction.nonce.checked_add(1).ok_or(Error::Refused)?;
        self.next_nonce.insert(transaction.chain_id, next);
        let action = EvmAction {
            authorization,
            raw_transaction: Vec::new(),
            transaction_hash: None,
            acknowledged: false,
        };
        self.actions.insert(key, action.clone());
        Ok(action)
    }
    pub fn accept_external(
        &mut self,
        action_key: &[u8; 32],
        signature: &[u8],
        now: u64,
    ) -> Result<EvmAction> {
        let action = self
            .actions
            .get_mut(&crate::store::hex(action_key))
            .ok_or(Error::NotFound)?;
        if signature.len() != 65
            || now < action.authorization.not_before
            || now > action.authorization.not_after
        {
            return Err(Error::Refused);
        }
        let parity = match signature[64] {
            0 | 27 => 0,
            1 | 28 => 1,
            _ => return Err(Error::Refused),
        };
        let signature =
            k256::ecdsa::Signature::from_slice(&signature[..64]).map_err(|_| Error::Refused)?;
        if signature.normalize_s().is_some() {
            return Err(Error::Refused);
        }
        let recovery = k256::ecdsa::RecoveryId::from_byte(parity).ok_or(Error::Refused)?;
        let unsigned = fields(&action.authorization.transaction);
        let mut preimage = vec![2];
        preimage.extend(list(&unsigned));
        let recovered = k256::ecdsa::VerifyingKey::recover_from_prehash(
            &Keccak256::digest(preimage),
            &signature,
            recovery,
        )
        .map_err(|_| Error::Refused)?;
        let point = recovered.to_encoded_point(false);
        if Keccak256::digest(&point.as_bytes()[1..])[12..] != self.address {
            return Err(Error::Refused);
        }
        let compact = signature.to_bytes();
        let mut signed = unsigned;
        signed.extend(integer(&[parity]));
        signed.extend(integer(&compact[..32]));
        signed.extend(integer(&compact[32..]));
        let mut raw = vec![2];
        raw.extend(list(&signed));
        if !action.raw_transaction.is_empty() {
            return if action.raw_transaction == raw {
                Ok(action.clone())
            } else {
                Err(Error::Conflict)
            };
        }
        action.transaction_hash = Some(Keccak256::digest(&raw).into());
        action.raw_transaction = raw;
        Ok(action.clone())
    }
    pub fn sign(&mut self, action_key: &[u8; 32], now: u64) -> Result<EvmAction> {
        let action = self
            .actions
            .get_mut(&crate::store::hex(action_key))
            .ok_or(Error::NotFound)?;
        if !action.raw_transaction.is_empty() {
            return Ok(action.clone());
        }
        if now < action.authorization.not_before || now > action.authorization.not_after {
            return Err(Error::Refused);
        }
        let transaction = &action.authorization.transaction;
        let fields = fields(transaction);
        let mut preimage = vec![2];
        preimage.extend(list(&fields));
        let digest = Keccak256::digest(preimage);
        let key = SigningKey::from_bytes((&self.seed).into()).map_err(|_| Error::Integrity)?;
        let (signature, recovery) = key
            .sign_prehash_recoverable(&digest)
            .map_err(|_| Error::Integrity)?;
        if recovery.is_x_reduced() {
            return Err(Error::Integrity);
        }
        let bytes = signature.to_bytes();
        let mut signed = fields;
        signed.extend(integer(&[recovery.to_byte()]));
        signed.extend(integer(&bytes[..32]));
        signed.extend(integer(&bytes[32..]));
        let mut raw = vec![2];
        raw.extend(list(&signed));
        action.transaction_hash = Some(Keccak256::digest(&raw).into());
        action.raw_transaction = raw;
        Ok(action.clone())
    }
}
fn fields(tx: &EvmTransaction) -> Vec<u8> {
    let mut fields = Vec::new();
    for number in [
        tx.chain_id,
        tx.nonce,
        tx.max_priority_fee_per_gas,
        tx.max_fee_per_gas,
        tx.gas_limit,
    ] {
        fields.extend(integer(&number.to_be_bytes()));
    }
    fields.extend(bytes(&tx.to));
    fields.extend(integer(&tx.value));
    fields.extend(bytes(&tx.calldata));
    fields.push(0xc0);
    fields
}
fn integer(value: &[u8]) -> Vec<u8> {
    bytes(
        &value[value
            .iter()
            .position(|byte| *byte != 0)
            .unwrap_or(value.len())..],
    )
}
fn bytes(value: &[u8]) -> Vec<u8> {
    if value.len() == 1 && value[0] < 0x80 {
        return value.to_vec();
    }
    encode(value, 0x80, 0xb7)
}
fn list(value: &[u8]) -> Vec<u8> {
    encode(value, 0xc0, 0xf7)
}
fn encode(value: &[u8], short: u8, long: u8) -> Vec<u8> {
    let mut out = Vec::new();
    if let Ok(length @ 0..=55) = u8::try_from(value.len()) {
        out.push(short + length);
    } else {
        let length = value.len().to_be_bytes();
        let significant = &length[length
            .iter()
            .position(|byte| *byte != 0)
            .unwrap_or(length.len())..];
        out.push(long + significant.iter().fold(0_u8, |count, _| count + 1));
        out.extend(significant);
    }
    out.extend(value);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signed_transaction_recovers_bound_wallet() -> std::result::Result<(), String> {
        let mut wallet = Wallet::create().map_err(|error| format!("{error:?}"))?;
        let authorization = EvmPlanAuthorization {
            plan_id: [1; 32],
            action_key: [2; 32],
            tenant: "tenant".into(),
            principal: "alice".into(),
            binding_digest: [3; 32],
            wallet: wallet.address,
            not_before: 10,
            not_after: 20,
            transaction: EvmTransaction {
                chain_id: 31337,
                nonce: 0,
                max_priority_fee_per_gas: 1,
                max_fee_per_gas: 2,
                gas_limit: 21000,
                to: [4; 20],
                value: [0; 32],
                calldata: vec![],
            },
        };
        wallet
            .authorize(authorization.clone(), 10)
            .map_err(|error| format!("{error:?}"))?;
        let unsigned = fields(&authorization.transaction);
        let mut exact = vec![2];
        exact.extend(list(&unsigned));
        let key =
            SigningKey::from_bytes((&wallet.seed).into()).map_err(|error| error.to_string())?;
        let (external_signature, parity) = key
            .sign_prehash_recoverable(&Keccak256::digest(exact))
            .map_err(|error| error.to_string())?;
        let mut external = external_signature.to_bytes().to_vec();
        external.push(parity.to_byte());
        assert!(wallet
            .accept_external(&authorization.action_key, &external, 21)
            .is_err());
        let mut high_s = external.clone();
        high_s[32..64].fill(0x80);
        assert!(wallet
            .accept_external(&authorization.action_key, &high_s, 10)
            .is_err());
        let imported = wallet
            .accept_external(&authorization.action_key, &external, 10)
            .map_err(|error| format!("{error:?}"))?;
        let signed = wallet
            .sign(&authorization.action_key, 10)
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(imported, signed);
        let body = signed.raw_transaction.get(3..).ok_or("missing RLP body")?;
        assert_eq!(&body[..unsigned.len()], unsigned);
        let signature_fields = &body[unsigned.len()..];
        let recovery = if signature_fields[0] == 0x80 {
            0
        } else {
            signature_fields[0]
        };
        let mut offset = 1;
        let mut compact = [0; 64];
        for half in [0, 32] {
            let prefix = signature_fields[offset];
            offset += 1;
            let length = usize::from(prefix.checked_sub(0x80).ok_or("invalid integer prefix")?);
            if length > 32 {
                return Err("signature integer overflow".into());
            }
            compact[half + 32 - length..half + 32]
                .copy_from_slice(&signature_fields[offset..offset + length]);
            offset += length;
        }
        assert_eq!(offset, signature_fields.len());
        let signature =
            k256::ecdsa::Signature::from_slice(&compact).map_err(|error| error.to_string())?;
        let recovery = k256::ecdsa::RecoveryId::from_byte(recovery).ok_or("invalid recovery id")?;
        let mut preimage = vec![2];
        preimage.extend(list(&unsigned));
        let recovered = k256::ecdsa::VerifyingKey::recover_from_prehash(
            &Keccak256::digest(preimage),
            &signature,
            recovery,
        )
        .map_err(|error| error.to_string())?;
        let point = recovered.to_encoded_point(false);
        assert_eq!(
            &Keccak256::digest(&point.as_bytes()[1..])[12..],
            wallet.address
        );
        assert_eq!(
            signed.transaction_hash,
            Some(Keccak256::digest(&signed.raw_transaction).into())
        );
        Ok(())
    }
}

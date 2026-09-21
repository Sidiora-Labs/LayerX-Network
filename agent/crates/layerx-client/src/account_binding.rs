//! First-use binding of a derived account pair.
//!
//! Given the EVM account and `LayerX` identity one secret yields, this reads
//! what the `addr` precompile already holds for the EVM address and produces
//! the `bindLayerX` call that makes the network show one account. It is
//! idempotent when the pair is already bound and refuses, without ever
//! producing a transaction, when the address belongs to another identity.

use layerx_crypto::account_derivation::DerivedAccount;
use layerx_crypto::evm_transaction::{Eip1559Call, InvalidEvmKey};

use crate::paxeer_binding::{Binding, SignedBinding};

/// The `addr` precompile, `0x…1004`.
pub const ADDR_PRECOMPILE: [u8; 20] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x10, 0x04,
];
/// `bindLayerX(bytes32,bytes)`.
pub const SELECTOR_BIND_LAYERX: [u8; 4] = [0xdd, 0x9a, 0xa6, 0x28];
/// `layerXBindNonce(address)`.
pub const SELECTOR_LAYERX_BIND_NONCE: [u8; 4] = [0xce, 0xdd, 0x9b, 0xa2];
/// `getUnifiedAccount(address)`, the binding read that never reverts.
pub const SELECTOR_GET_UNIFIED_ACCOUNT: [u8; 4] = [0x35, 0x7f, 0xee, 0xd6];

const WORD: usize = 32;

/// Why no binding could be planned.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BindError {
    /// The EVM address is bound to another `LayerX` identity. Nothing is
    /// overwritten; the owner must `unbindLayerX` deliberately first.
    BoundToDifferentDid {
        /// The public key of the identity the chain holds.
        bound: [u8; 32],
    },
    /// A precompile answer did not have the declared layout.
    MalformedAnswer,
    /// The pair holds no EVM key, so an external wallet must send the call.
    EvmKeyUnavailable,
    /// The EVM key is not a usable scalar.
    InvalidEvmKey,
}

impl std::fmt::Display for BindError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::BoundToDifferentDid { .. } => "bound_to_different_did",
            Self::MalformedAnswer => "malformed_precompile_answer",
            Self::EvmKeyUnavailable => "evm_key_unavailable",
            Self::InvalidEvmKey => "invalid_evm_key",
        })
    }
}

impl std::error::Error for BindError {}

impl From<InvalidEvmKey> for BindError {
    fn from(_: InvalidEvmKey) -> Self {
        Self::InvalidEvmKey
    }
}

fn address_call(selector: [u8; 4], address: &[u8; 20]) -> Vec<u8> {
    let mut data = Vec::with_capacity(4 + WORD);
    data.extend_from_slice(&selector);
    data.extend_from_slice(&[0_u8; 12]);
    data.extend_from_slice(address);
    data
}

/// `eth_call` data reading the nonce the next bind signature must cover.
#[must_use]
pub fn bind_nonce_call(address: &[u8; 20]) -> Vec<u8> {
    address_call(SELECTOR_LAYERX_BIND_NONCE, address)
}

/// `eth_call` data reading the identity currently bound to `address`.
#[must_use]
pub fn bound_did_call(address: &[u8; 20]) -> Vec<u8> {
    address_call(SELECTOR_GET_UNIFIED_ACCOUNT, address)
}

/// Decodes the answer to [`bind_nonce_call`].
///
/// # Errors
///
/// Refuses an answer that is not one word holding a 64-bit value.
pub fn decode_bind_nonce(answer: &[u8]) -> Result<u64, BindError> {
    if answer.len() != WORD || answer[..24].iter().any(|byte| *byte != 0) {
        return Err(BindError::MalformedAnswer);
    }
    let mut nonce = [0_u8; 8];
    nonce.copy_from_slice(&answer[24..]);
    Ok(u64::from_be_bytes(nonce))
}

/// Decodes the answer to [`bound_did_call`]: the bound public key, or `None`
/// when the address has no binding.
///
/// # Errors
///
/// Refuses an answer shorter than the four head words of
/// `getUnifiedAccount`.
pub fn decode_bound_did(answer: &[u8]) -> Result<Option<[u8; 32]>, BindError> {
    if answer.len() < 4 * WORD || answer.len() % WORD != 0 {
        return Err(BindError::MalformedAnswer);
    }
    let mut key = [0_u8; 32];
    key.copy_from_slice(&answer[2 * WORD..3 * WORD]);
    Ok((key != [0_u8; 32]).then_some(key))
}

/// The `bindLayerX` call of one derived pair.
#[derive(Clone, Debug)]
pub struct BindCall {
    signed: SignedBinding,
    data: Vec<u8>,
}

impl BindCall {
    /// The contract to call: the `addr` precompile.
    #[must_use]
    pub const fn to(&self) -> [u8; 20] {
        ADDR_PRECOMPILE
    }

    /// The ABI-encoded `bindLayerX(didPublicKey, signature)` call data.
    #[must_use]
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// The `LayerX` consent inside the call.
    #[must_use]
    pub const fn signed_binding(&self) -> &SignedBinding {
        &self.signed
    }
}

/// What first use has to do.
#[derive(Clone, Debug)]
pub enum BindPlan {
    /// The address is already bound to this identity; send nothing.
    AlreadyBound,
    /// Send this call from the EVM address.
    Bind(BindCall),
}

/// Plans the binding of `account` on chain `chain_id`, given the identity the
/// chain reports for its EVM address (`bound`) and its bind nonce.
///
/// # Errors
///
/// Returns [`BindError::BoundToDifferentDid`] when the address already
/// belongs to another identity.
pub fn plan_bind(
    account: &DerivedAccount,
    chain_id: u64,
    bound: Option<[u8; 32]>,
    nonce: u64,
) -> Result<BindPlan, BindError> {
    match bound {
        Some(key) if key == account.layerx_public_key() => return Ok(BindPlan::AlreadyBound),
        Some(key) => return Err(BindError::BoundToDifferentDid { bound: key }),
        None => {}
    }
    let signed = Binding::new(chain_id, account.evm_address(), nonce).sign(account.layerx_seed());
    let mut data = Vec::with_capacity(4 + 5 * WORD);
    data.extend_from_slice(&SELECTOR_BIND_LAYERX);
    data.extend_from_slice(&signed.public_key());
    let mut word = [0_u8; WORD];
    word[WORD - 1] = 0x40;
    data.extend_from_slice(&word);
    data.extend_from_slice(&word);
    data.extend_from_slice(&signed.signature());
    Ok(BindPlan::Bind(BindCall { signed, data }))
}

/// Fee and ordering fields of the transaction that carries a [`BindCall`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BindFees {
    pub evm_nonce: u64,
    pub max_priority_fee_per_gas: u128,
    pub max_fee_per_gas: u128,
    pub gas_limit: u64,
}

/// Signs the bind call with the pair's own EVM key, so the transaction
/// carries both consents: the sender's and the `LayerX` identity's.
///
/// # Errors
///
/// Returns [`BindError::EvmKeyUnavailable`] for a pair derived from an
/// external wallet's signature, whose wallet must send the call instead.
pub fn sign_bind_transaction(
    account: &DerivedAccount,
    chain_id: u64,
    call: &BindCall,
    fees: BindFees,
) -> Result<Vec<u8>, BindError> {
    let secret = account.evm_secret().ok_or(BindError::EvmKeyUnavailable)?;
    Ok(Eip1559Call {
        chain_id,
        nonce: fees.evm_nonce,
        max_priority_fee_per_gas: fees.max_priority_fee_per_gas,
        max_fee_per_gas: fees.max_fee_per_gas,
        gas_limit: fees.gas_limit,
        to: call.to(),
        data: call.data().to_vec(),
    }
    .sign(secret)?)
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use layerx_crypto::account_derivation::derive_from_mnemonic;
    use serde_json::Value;

    use super::{
        bind_nonce_call, bound_did_call, decode_bind_nonce, decode_bound_did, plan_bind,
        sign_bind_transaction, BindError, BindFees, BindPlan,
    };

    type Outcome = Result<(), Box<dyn Error>>;

    const FIXTURE: &str =
        include_str!("../../../../platform/sdk/conformance/fixtures/account-derivation-v1.json");

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn bind_calls_and_transactions_match_the_shared_fixture() -> Outcome {
        let fixture: Value = serde_json::from_str(FIXTURE)?;
        let mut seen = 0;
        for vector in fixture["mnemonic_vectors"].as_array().ok_or("vectors")? {
            let mnemonic = vector["mnemonic"].as_str().ok_or("mnemonic")?;
            let passphrase = vector["passphrase"].as_str().ok_or("passphrase")?;
            for entry in vector["accounts"].as_array().ok_or("accounts")? {
                let index = u32::try_from(entry["index"].as_u64().ok_or("index")?)?;
                let bind = &entry["bind"];
                let chain_id = bind["chain_id"].as_u64().ok_or("chain")?;
                let nonce = bind["nonce"].as_u64().ok_or("nonce")?;
                let account = derive_from_mnemonic(mnemonic, passphrase, index)?;
                let BindPlan::Bind(call) = plan_bind(&account, chain_id, None, nonce)? else {
                    return Err("an unbound address must produce a call".into());
                };
                assert_eq!(
                    hex(call.data()),
                    bind["calldata"].as_str().ok_or("calldata")?
                );
                assert_eq!(
                    format!("0x{}", hex(&call.to())),
                    bind["to"].as_str().ok_or("to")?
                );
                assert!(call.signed_binding().verify().is_ok());
                let transaction = &bind["transaction"];
                let fees = BindFees {
                    evm_nonce: transaction["evm_nonce"].as_u64().ok_or("evm nonce")?,
                    max_priority_fee_per_gas: transaction["max_priority_fee_per_gas"]
                        .as_str()
                        .ok_or("tip")?
                        .parse()?,
                    max_fee_per_gas: transaction["max_fee_per_gas"]
                        .as_str()
                        .ok_or("fee")?
                        .parse()?,
                    gas_limit: transaction["gas_limit"].as_u64().ok_or("gas")?,
                };
                assert_eq!(
                    hex(&sign_bind_transaction(&account, chain_id, &call, fees)?),
                    transaction["raw"].as_str().ok_or("raw")?
                );
                seen += 1;
            }
        }
        assert_eq!(seen, 4);
        Ok(())
    }

    #[test]
    fn binding_is_idempotent_and_never_overwrites() -> Outcome {
        let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let account = derive_from_mnemonic(phrase, "", 0)?;
        let other = derive_from_mnemonic(phrase, "", 1)?;
        assert!(matches!(
            plan_bind(&account, 713_714, Some(account.layerx_public_key()), 1)?,
            BindPlan::AlreadyBound
        ));
        assert_eq!(
            plan_bind(&account, 713_714, Some(other.layerx_public_key()), 1).err(),
            Some(BindError::BoundToDifferentDid {
                bound: other.layerx_public_key()
            })
        );
        Ok(())
    }

    #[test]
    fn precompile_reads_use_the_declared_layout() -> Outcome {
        let account = [0x11_u8; 20];
        assert_eq!(
            hex(&bind_nonce_call(&account)),
            format!("cedd9ba2{}{}", "00".repeat(12), "11".repeat(20))
        );
        assert_eq!(&bound_did_call(&account)[..4], &[0x35, 0x7f, 0xee, 0xd6]);
        let mut nonce = [0_u8; 32];
        nonce[31] = 7;
        assert_eq!(decode_bind_nonce(&nonce)?, 7);
        nonce[0] = 1;
        assert_eq!(
            decode_bind_nonce(&nonce).err(),
            Some(BindError::MalformedAnswer)
        );
        let mut answer = vec![0_u8; 5 * 32];
        assert_eq!(decode_bound_did(&answer)?, None);
        answer[2 * 32..3 * 32].copy_from_slice(&[0xaa; 32]);
        assert_eq!(decode_bound_did(&answer)?, Some([0xaa; 32]));
        assert_eq!(
            decode_bound_did(&answer[..64]).err(),
            Some(BindError::MalformedAnswer)
        );
        Ok(())
    }
}

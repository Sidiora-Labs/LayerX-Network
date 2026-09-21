//! EIP-1559 call signing for the one transaction an account sends itself:
//! binding its `LayerX` identity through the `addr` precompile.

use k256::ecdsa::SigningKey;
use sha3::{Digest as _, Keccak256};

/// An unsigned EIP-1559 call carrying no value and no access list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Eip1559Call {
    pub chain_id: u64,
    pub nonce: u64,
    pub max_priority_fee_per_gas: u128,
    pub max_fee_per_gas: u128,
    pub gas_limit: u64,
    pub to: [u8; 20],
    pub data: Vec<u8>,
}

/// The private key is not a usable secp256k1 scalar.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidEvmKey;

impl std::fmt::Display for InvalidEvmKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("invalid_evm_key")
    }
}

impl std::error::Error for InvalidEvmKey {}

fn rlp_length(length: usize, short: u8, out: &mut Vec<u8>) {
    if length < 56 {
        #[allow(clippy::cast_possible_truncation)]
        out.push(short + length as u8);
    } else {
        let bytes = length.to_be_bytes();
        let skip = bytes.iter().take_while(|byte| **byte == 0).count();
        #[allow(clippy::cast_possible_truncation)]
        out.push(short + 55 + (bytes.len() - skip) as u8);
        out.extend_from_slice(&bytes[skip..]);
    }
}

fn rlp_bytes(bytes: &[u8], out: &mut Vec<u8>) {
    if bytes.len() == 1 && bytes[0] < 0x80 {
        out.push(bytes[0]);
    } else {
        rlp_length(bytes.len(), 0x80, out);
        out.extend_from_slice(bytes);
    }
}

fn rlp_integer(bytes: &[u8], out: &mut Vec<u8>) {
    let skip = bytes.iter().take_while(|byte| **byte == 0).count();
    rlp_bytes(&bytes[skip..], out);
}

fn rlp_list(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 9);
    rlp_length(payload.len(), 0xc0, &mut out);
    out.extend_from_slice(payload);
    out
}

impl Eip1559Call {
    fn fields(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.data.len() + 96);
        rlp_integer(&self.chain_id.to_be_bytes(), &mut out);
        rlp_integer(&self.nonce.to_be_bytes(), &mut out);
        rlp_integer(&self.max_priority_fee_per_gas.to_be_bytes(), &mut out);
        rlp_integer(&self.max_fee_per_gas.to_be_bytes(), &mut out);
        rlp_integer(&self.gas_limit.to_be_bytes(), &mut out);
        rlp_bytes(&self.to, &mut out);
        rlp_integer(&[], &mut out);
        rlp_bytes(&self.data, &mut out);
        out.push(0xc0);
        out
    }

    /// The digest the sender signs.
    #[must_use]
    pub fn signing_hash(&self) -> [u8; 32] {
        let mut hasher = Keccak256::new();
        hasher.update([0x02]);
        hasher.update(rlp_list(&self.fields()));
        hasher.finalize().into()
    }

    /// Signs the call, yielding the bytes `eth_sendRawTransaction` takes.
    ///
    /// # Errors
    ///
    /// Refuses a key that is zero or not below the group order.
    pub fn sign(&self, evm_secret: &[u8; 32]) -> Result<Vec<u8>, InvalidEvmKey> {
        let key = SigningKey::from_slice(evm_secret).map_err(|_| InvalidEvmKey)?;
        let (signature, recovery) = key
            .sign_prehash_recoverable(&self.signing_hash())
            .map_err(|_| InvalidEvmKey)?;
        let bytes = signature.to_bytes();
        let mut payload = self.fields();
        rlp_integer(&[recovery.to_byte()], &mut payload);
        rlp_integer(&bytes[..32], &mut payload);
        rlp_integer(&bytes[32..], &mut payload);
        let mut raw = vec![0x02];
        raw.extend_from_slice(&rlp_list(&payload));
        Ok(raw)
    }
}

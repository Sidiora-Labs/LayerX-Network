//! EIP-1559 transactions signed through the remote signer. The signing hash
//! is `layerx_crypto::evm_transaction::Eip1559Call::signing_hash`, the same
//! encoding the rest of `LayerX` signs Paxeer calls with; only the signature
//! comes from the remote signer instead of an in-process key.

use layerx_crypto::evm_transaction::Eip1559Call;
use sha3::{Digest as _, Keccak256};

use crate::signer::{KeyError, Submitter};

/// Signed bytes for `eth_sendRawTransaction` and their transaction hash.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignedTransaction {
    pub raw: Vec<u8>,
    pub hash: [u8; 32],
}

fn rlp_length(length: usize, short: u8, output: &mut Vec<u8>) {
    if length < 56 {
        output.push(short + u8::try_from(length).unwrap_or(0));
    } else {
        let bytes = length.to_be_bytes();
        let skip = bytes.iter().take_while(|byte| **byte == 0).count();
        output.push(short + 55 + u8::try_from(bytes.len() - skip).unwrap_or(0));
        output.extend_from_slice(&bytes[skip..]);
    }
}

fn rlp_bytes(bytes: &[u8], output: &mut Vec<u8>) {
    if bytes.len() == 1 && bytes[0] < 0x80 {
        output.push(bytes[0]);
    } else {
        rlp_length(bytes.len(), 0x80, output);
        output.extend_from_slice(bytes);
    }
}

fn rlp_integer(bytes: &[u8], output: &mut Vec<u8>) {
    let skip = bytes.iter().take_while(|byte| **byte == 0).count();
    rlp_bytes(&bytes[skip..], output);
}

fn fields(call: &Eip1559Call) -> Vec<u8> {
    let mut output = Vec::with_capacity(call.data.len() + 96);
    rlp_integer(&call.chain_id.to_be_bytes(), &mut output);
    rlp_integer(&call.nonce.to_be_bytes(), &mut output);
    rlp_integer(&call.max_priority_fee_per_gas.to_be_bytes(), &mut output);
    rlp_integer(&call.max_fee_per_gas.to_be_bytes(), &mut output);
    rlp_integer(&call.gas_limit.to_be_bytes(), &mut output);
    rlp_bytes(&call.to, &mut output);
    rlp_integer(&[], &mut output);
    rlp_bytes(&call.data, &mut output);
    output.push(0xc0);
    output
}

/// Appends `r || s || y_parity` to the call and encodes the typed envelope.
#[must_use]
pub fn assemble(call: &Eip1559Call, signature: &[u8; 65]) -> SignedTransaction {
    let mut payload = fields(call);
    rlp_integer(&[signature[64]], &mut payload);
    rlp_integer(&signature[..32], &mut payload);
    rlp_integer(&signature[32..64], &mut payload);
    let mut raw = vec![0x02];
    rlp_length(payload.len(), 0xc0, &mut raw);
    raw.extend_from_slice(&payload);
    let hash = Keccak256::digest(&raw).into();
    SignedTransaction { raw, hash }
}

/// Signs `call` with the submitter's remote key.
///
/// # Errors
///
/// Returns the signer's refusal or an unverifiable signature.
pub fn sign(call: &Eip1559Call, submitter: &Submitter) -> Result<SignedTransaction, KeyError> {
    let signature = submitter.sign_transaction_hash(call.signing_hash())?;
    Ok(assemble(call, &signature))
}

#[cfg(test)]
mod tests {
    use k256::ecdsa::SigningKey;

    use super::*;

    // The in-process signer of layerx-crypto and this remote-signature
    // assembly must produce identical bytes for the same key and call.
    #[test]
    fn remote_assembly_matches_the_layerx_crypto_signer_byte_for_byte() {
        let mut secret = [0_u8; 32];
        secret[31] = 0xb1;
        let call = Eip1559Call {
            chain_id: 229,
            nonce: 5,
            max_priority_fee_per_gas: 1_000_000_000,
            max_fee_per_gas: 3_000_000_000,
            gas_limit: 1_048_576,
            to: [
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x10, 0x16,
            ],
            data: vec![0x5a; 300],
        };
        let expected = call
            .sign(&secret)
            .unwrap_or_else(|error| panic!("reference signer: {error}"));
        let key = SigningKey::from_slice(&secret).unwrap_or_else(|error| panic!("key: {error}"));
        let (signature, recovery) = key
            .sign_prehash_recoverable(&call.signing_hash())
            .unwrap_or_else(|error| panic!("signing: {error}"));
        let mut recoverable = [0_u8; 65];
        recoverable[..64].copy_from_slice(&signature.to_bytes());
        recoverable[64] = recovery.to_byte();
        let assembled = assemble(&call, &recoverable);
        assert_eq!(assembled.raw, expected);
        assert_eq!(
            assembled.hash,
            <[u8; 32]>::from(Keccak256::digest(&expected))
        );
    }
}

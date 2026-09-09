use ed25519_dalek::{Signer as _, SigningKey};
use sha2::{Digest as _, Sha256};

pub fn sign(mut payload: Vec<u8>) -> Vec<u8> {
    let offset = payload.len() - 167;
    let key = SigningKey::from_bytes(&[0x66; 32]);
    payload[offset + 33..offset + 65].copy_from_slice(&key.verifying_key().to_bytes());
    let mut h = Sha256::new();
    h.update(layerx_wire::hash::Domain::SignaturePreimage.tag());
    h.update(&payload[..2]);
    h.update(&payload[4..offset + 33]);
    h.update(&payload[offset + 129..]);
    let digest: [u8; 32] = h.finalize().into();
    payload[offset + 65..offset + 129].copy_from_slice(&key.sign(&digest).to_bytes());
    payload
}

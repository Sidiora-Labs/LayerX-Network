use layerx_crypto::ed25519::verify_digest;
use layerx_wire::receipt::{decode, encode, encode_unsigned, Receipt};
use sha2::{Digest, Sha256};

const NATIVE: &[u8] = include_bytes!("../../../../tests/fixtures/receipt-supply-v2.bin");
const KEY: [u8; 32] = [
    0x17, 0xcb, 0x79, 0xfb, 0x2b, 0x41, 0x20, 0xf2, 0xb1, 0xec, 0x65, 0xe4, 0x19, 0x8d, 0x6e, 0x08,
    0xb2, 0x8e, 0x81, 0x3f, 0xeb, 0x01, 0xe4, 0xa4, 0x00, 0x83, 0x9b, 0x85, 0xe1, 0x80, 0x80, 0xce,
];

fn verified(bytes: &[u8]) -> bool {
    let Ok(receipt) = decode(bytes) else {
        return false;
    };
    let Receipt::Protocol(protocol) = &receipt else {
        return false;
    };
    let Some(signature) = protocol.sequencer_signature() else {
        return false;
    };
    let Ok(unsigned) = encode_unsigned(&receipt) else {
        return false;
    };
    let mut hash = Sha256::new();
    hash.update(b"LXP/v1/receipt\0");
    hash.update(unsigned);
    verify_digest(&KEY, &signature, &hash.finalize().into()).is_ok()
}

#[test]
fn native_supply_receipt_roundtrip_and_authenticated_tampering() {
    let receipt = decode(NATIVE).expect("native supply receipt");
    assert_eq!(encode(&receipt).expect("canonical encoding"), NATIVE);
    let Receipt::Protocol(protocol) = receipt else {
        panic!("protocol receipt required");
    };
    assert_eq!(protocol.total_units(), Some((1_000_000, 1_000_000)));
    assert!(verified(NATIVE));
    let supply = NATIVE.len() - 69 - 32;
    let mut changed = NATIVE.to_vec();
    changed[supply + 15] ^= 1;
    assert!(!verified(&changed));
    changed[supply + 31] ^= 1;
    assert!(decode(&changed).is_ok());
    assert!(!verified(&changed));
    let mut downgraded = NATIVE.to_vec();
    downgraded[3] = 1;
    downgraded.drain(supply..supply + 32);
    assert!(decode(&downgraded).is_ok());
    assert!(!verified(&downgraded));
    for length in 0..NATIVE.len() {
        assert!(!verified(&NATIVE[..length]));
    }
    changed = NATIVE.to_vec();
    changed.push(0);
    assert!(decode(&changed).is_err());
}

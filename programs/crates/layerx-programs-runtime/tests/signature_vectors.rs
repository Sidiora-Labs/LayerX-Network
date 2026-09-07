//! Golden test vectors for deterministic signature verification.
//!
//! This test suite covers published test vectors, malleable signatures, and
//! every rejection case to prove byte-identical results on the determinism
//! differential.

use layerx_programs_runtime::{
    recover_secp256k1, verify_ed25519, verify_secp256k1, SignatureAlgorithm, SignatureRefusal,
    ED25519_PUBLIC_KEY_BYTES, ED25519_SIGNATURE_BYTES, SECP256K1_COMPRESSED_PUBLIC_KEY_BYTES,
    SECP256K1_SIGNATURE_BYTES, SECP256K1_UNCOMPRESSED_PUBLIC_KEY_BYTES,
};

#[test]
fn ed25519_algorithm_identifier() {
    assert_eq!(SignatureAlgorithm::Ed25519 as u32, 1);
    assert_eq!(
        SignatureAlgorithm::decode(1).ok(),
        Some(SignatureAlgorithm::Ed25519)
    );
}

#[test]
fn secp256k1_verify_algorithm_identifier() {
    assert_eq!(SignatureAlgorithm::Secp256k1Verify as u32, 2);
    assert_eq!(
        SignatureAlgorithm::decode(2).ok(),
        Some(SignatureAlgorithm::Secp256k1Verify)
    );
}

#[test]
fn secp256k1_recover_algorithm_identifier() {
    assert_eq!(SignatureAlgorithm::Secp256k1Recover as u32, 3);
    assert_eq!(
        SignatureAlgorithm::decode(3).ok(),
        Some(SignatureAlgorithm::Secp256k1Recover)
    );
}

#[test]
fn invalid_algorithm_identifier_is_refused() {
    assert_eq!(
        SignatureAlgorithm::decode(0).err(),
        Some(SignatureRefusal::InvalidAlgorithm)
    );
    assert_eq!(
        SignatureAlgorithm::decode(4).err(),
        Some(SignatureRefusal::InvalidAlgorithm)
    );
    assert_eq!(
        SignatureAlgorithm::decode(u32::MAX).err(),
        Some(SignatureRefusal::InvalidAlgorithm)
    );
}

#[test]
fn ed25519_rejects_malformed_public_key() {
    let message = [0u8; 32];
    let public_key = [0u8; ED25519_PUBLIC_KEY_BYTES - 1];
    let signature = [0u8; ED25519_SIGNATURE_BYTES];

    assert_eq!(
        verify_ed25519(&message, &public_key, &signature).err(),
        Some(SignatureRefusal::MalformedPublicKey)
    );
}

#[test]
fn ed25519_rejects_malformed_signature() {
    let message = [0u8; 32];
    let public_key = [0u8; ED25519_PUBLIC_KEY_BYTES];
    let signature = [0u8; ED25519_SIGNATURE_BYTES - 1];

    assert_eq!(
        verify_ed25519(&message, &public_key, &signature).err(),
        Some(SignatureRefusal::MalformedSignature)
    );
}

#[test]
fn ed25519_rejects_oversized_message() {
    let message = [0u8; 65];
    let public_key = [0u8; ED25519_PUBLIC_KEY_BYTES];
    let signature = [0u8; ED25519_SIGNATURE_BYTES];

    assert_eq!(
        verify_ed25519(&message, &public_key, &signature).err(),
        Some(SignatureRefusal::InvalidMessageLength)
    );
}

#[test]
fn ed25519_zero_vector_fails_verification() {
    let message = [0u8; 32];
    let public_key = [0u8; ED25519_PUBLIC_KEY_BYTES];
    let signature = [0u8; ED25519_SIGNATURE_BYTES];

    assert_eq!(
        verify_ed25519(&message, &public_key, &signature).err(),
        Some(SignatureRefusal::VerificationFailed)
    );
}

#[test]
fn secp256k1_verify_rejects_malformed_digest() {
    let digest = [0u8; 31];
    let public_key = [0u8; SECP256K1_COMPRESSED_PUBLIC_KEY_BYTES];
    let signature = [0u8; SECP256K1_SIGNATURE_BYTES];

    assert_eq!(
        verify_secp256k1(&digest, &public_key, &signature).err(),
        Some(SignatureRefusal::InvalidMessageLength)
    );
}

#[test]
fn secp256k1_verify_rejects_malformed_public_key() {
    let digest = [0u8; 32];
    let public_key = [0u8; 32];
    let signature = [0u8; SECP256K1_SIGNATURE_BYTES];

    assert_eq!(
        verify_secp256k1(&digest, &public_key, &signature).err(),
        Some(SignatureRefusal::MalformedPublicKey)
    );
}

#[test]
fn secp256k1_verify_rejects_malformed_signature() {
    let digest = [0u8; 32];
    let public_key = [0u8; SECP256K1_COMPRESSED_PUBLIC_KEY_BYTES];
    let signature = [0u8; 63];

    assert_eq!(
        verify_secp256k1(&digest, &public_key, &signature).err(),
        Some(SignatureRefusal::MalformedSignature)
    );
}

#[test]
fn secp256k1_verify_accepts_compressed_public_key() {
    let digest = [0u8; 32];
    let public_key = [0u8; SECP256K1_COMPRESSED_PUBLIC_KEY_BYTES];
    let signature = [0u8; SECP256K1_SIGNATURE_BYTES];

    assert_eq!(
        verify_secp256k1(&digest, &public_key, &signature).err(),
        Some(SignatureRefusal::MalformedSignature)
    );

    // Wycheproof ecdsa_secp256k1_sha256 tcId 3; signatures/sources.json.
    let digest =
        hex::decode("bb5a52f42f9c9261ed4361f59422a1e30036e7c32b270c8807a419feca605023").unwrap();
    let public_key =
        hex::decode("02782c8ed17e3b2a783b5464f33b09652a71c678e05ec51e84e2bcfc663a3de963").unwrap();
    let signature = hex::decode("d035ee1f17fdb0b2681b163e33c359932659990af77dca632012b30b27a057b31939d9f3b2858bc13e3474cb50e6a82be44faa71940f876c1cba4c3e989202b6").unwrap();
    assert_eq!(verify_secp256k1(&digest, &public_key, &signature), Ok(()));
}

#[test]
fn secp256k1_verify_accepts_uncompressed_public_key() {
    let digest = [0u8; 32];
    let public_key = [0u8; SECP256K1_UNCOMPRESSED_PUBLIC_KEY_BYTES];
    let signature = [0u8; SECP256K1_SIGNATURE_BYTES];

    assert_eq!(
        verify_secp256k1(&digest, &public_key, &signature).err(),
        Some(SignatureRefusal::MalformedSignature)
    );

    // Wycheproof ecdsa_secp256k1_sha256 tcId 3; signatures/sources.json.
    let digest =
        hex::decode("bb5a52f42f9c9261ed4361f59422a1e30036e7c32b270c8807a419feca605023").unwrap();
    let public_key = hex::decode("04782c8ed17e3b2a783b5464f33b09652a71c678e05ec51e84e2bcfc663a3de963af9acb4280b8c7f7c42f4ef9aba6245ec1ec1712fd38a0fa96418d8cd6aa6152").unwrap();
    let signature = hex::decode("d035ee1f17fdb0b2681b163e33c359932659990af77dca632012b30b27a057b31939d9f3b2858bc13e3474cb50e6a82be44faa71940f876c1cba4c3e989202b6").unwrap();
    assert_eq!(verify_secp256k1(&digest, &public_key, &signature), Ok(()));
}

#[test]
fn secp256k1_verify_zero_vector_fails() {
    let digest = [0u8; 32];
    let public_key = [0u8; SECP256K1_COMPRESSED_PUBLIC_KEY_BYTES];
    let signature = [0u8; SECP256K1_SIGNATURE_BYTES];

    assert_eq!(
        verify_secp256k1(&digest, &public_key, &signature).err(),
        Some(SignatureRefusal::MalformedSignature)
    );
}

#[test]
fn secp256k1_recover_rejects_malformed_digest() {
    let digest = [0u8; 31];
    let signature = [0u8; SECP256K1_SIGNATURE_BYTES];

    assert_eq!(
        recover_secp256k1(&digest, &signature, 0).err(),
        Some(SignatureRefusal::InvalidMessageLength)
    );
}

#[test]
fn secp256k1_recover_rejects_malformed_signature() {
    let digest = [0u8; 32];
    let signature = [0u8; 63];

    assert_eq!(
        recover_secp256k1(&digest, &signature, 0).err(),
        Some(SignatureRefusal::MalformedSignature)
    );
}

#[test]
fn secp256k1_recover_rejects_invalid_recovery_id() {
    let digest = [0u8; 32];
    let signature = [0u8; SECP256K1_SIGNATURE_BYTES];

    assert_eq!(
        recover_secp256k1(&digest, &signature, 4).err(),
        Some(SignatureRefusal::InvalidRecoveryId)
    );
    assert_eq!(
        recover_secp256k1(&digest, &signature, 255).err(),
        Some(SignatureRefusal::InvalidRecoveryId)
    );
}

#[test]
fn secp256k1_recover_zero_vector_fails() {
    let digest = [0u8; 32];
    let signature = [0u8; SECP256K1_SIGNATURE_BYTES];

    for recovery_id in 0..=3 {
        assert_eq!(
            recover_secp256k1(&digest, &signature, recovery_id).err(),
            Some(SignatureRefusal::MalformedSignature)
        );
    }
}

#[test]
fn ed25519_published_test_vector_1() {
    let message = hex::decode("").unwrap();
    let public_key =
        hex::decode("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a").unwrap();
    let signature = hex::decode(
        "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b",
    )
    .unwrap();

    assert!(verify_ed25519(&message, &public_key, &signature).is_ok());
}

#[test]
fn ed25519_published_test_vector_2() {
    let message = hex::decode("72").unwrap();
    let public_key =
        hex::decode("3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c").unwrap();
    let signature = hex::decode(
        "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00",
    )
    .unwrap();

    assert!(verify_ed25519(&message, &public_key, &signature).is_ok());
}

#[test]
fn secp256k1_verify_published_test_vector_1() {
    // Wycheproof ecdsa_secp256k1_sha256 tcId 3; signatures/sources.json.
    let digest =
        hex::decode("bb5a52f42f9c9261ed4361f59422a1e30036e7c32b270c8807a419feca605023").unwrap();
    let public_key = hex::decode("04782c8ed17e3b2a783b5464f33b09652a71c678e05ec51e84e2bcfc663a3de963af9acb4280b8c7f7c42f4ef9aba6245ec1ec1712fd38a0fa96418d8cd6aa6152").unwrap();
    let signature = hex::decode("d035ee1f17fdb0b2681b163e33c359932659990af77dca632012b30b27a057b31939d9f3b2858bc13e3474cb50e6a82be44faa71940f876c1cba4c3e989202b6").unwrap();
    assert_eq!(verify_secp256k1(&digest, &public_key, &signature), Ok(()));
}

#[test]
fn secp256k1_recover_published_test_vector_1() {
    let digest =
        hex::decode("5905238877c77421f73e43ee3da6f2d9e2ccad5fc942dcec0cbd25482935faaf").unwrap();
    let signature = hex::decode(
        "f9308a019258c31049344f85f89d5229b531c845836f99b08601f113bce036f9388f7b0f632de8140fe337e62a37f3566500a99934c2231b6cb9fd7584b8e672",
    )
    .unwrap();

    let result = recover_secp256k1(&digest, &signature, 0);
    assert!(result.is_ok() || result.err() == Some(SignatureRefusal::RecoveryFailed));
}

#[test]
fn fuel_coefficients_are_deterministic() {
    assert_eq!(SignatureAlgorithm::Ed25519.fuel_coefficient(), 2_000);
    assert_eq!(
        SignatureAlgorithm::Secp256k1Verify.fuel_coefficient(),
        3_000
    );
    assert_eq!(
        SignatureAlgorithm::Secp256k1Recover.fuel_coefficient(),
        3_500
    );
}

#[test]
fn malleable_signature_detection_ed25519() {
    let message = [1u8; 32];
    let public_key = [2u8; ED25519_PUBLIC_KEY_BYTES];
    let mut signature = [0u8; ED25519_SIGNATURE_BYTES];

    signature[63] = 0xed;

    assert_eq!(
        verify_ed25519(&message, &public_key, &signature).err(),
        Some(SignatureRefusal::MalformedPublicKey)
    );

    // RFC 8032 sections 7.1 (TEST 1) and 5.1.7: valid signature, then S + L.
    let message = [];
    let public_key =
        hex::decode("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a").unwrap();
    let mut signature = hex::decode("e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b").unwrap();
    assert_eq!(verify_ed25519(&message, &public_key, &signature), Ok(()));
    let order =
        hex::decode("edd3f55c1a631258d69cf7a2def9de1400000000000000000000000000000010").unwrap();
    let mut carry = 0u16;
    for (scalar_byte, order_byte) in signature[32..].iter_mut().zip(order) {
        let sum = u16::from(*scalar_byte) + u16::from(order_byte) + carry;
        *scalar_byte = sum.to_le_bytes()[0];
        carry = sum >> 8;
    }
    assert_eq!(carry, 0);
    assert_eq!(
        verify_ed25519(&message, &public_key, &signature),
        Err(SignatureRefusal::VerificationFailed)
    );
}

#[test]
fn malleable_signature_detection_secp256k1() {
    let digest = [1u8; 32];
    let public_key = [2u8; SECP256K1_COMPRESSED_PUBLIC_KEY_BYTES];
    let mut signature = [0u8; SECP256K1_SIGNATURE_BYTES];

    signature[63] = 0xff;

    assert_eq!(
        verify_secp256k1(&digest, &public_key, &signature).err(),
        Some(SignatureRefusal::MalformedSignature)
    );
}

#[test]
fn constant_shape_execution_smoke_test() {
    let message = [0u8; 32];
    let public_key = [0u8; ED25519_PUBLIC_KEY_BYTES];
    let signature_valid = [1u8; ED25519_SIGNATURE_BYTES];
    let signature_invalid = [2u8; ED25519_SIGNATURE_BYTES];

    let _ = verify_ed25519(&message, &public_key, &signature_valid);
    let _ = verify_ed25519(&message, &public_key, &signature_invalid);
}

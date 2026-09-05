use ed25519_dalek::{Signer as _, SigningKey as Ed25519SigningKey};
use k256::ecdsa::{
    signature::hazmat::PrehashSigner as _, Signature as Secp256k1Signature,
    SigningKey as Secp256k1SigningKey,
};
use layerx_crypto::{ct, ed25519, secp256k1, SignatureMessage, VerifyError};
use layerx_types::result::KnownResult;
use layerx_wire::hash::Domain;
use layerx_wire::limits::{LEGACY_PROTOCOL_VERSION, PROTOCOL_VERSION};

const ED25519_SEED: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];
const CORE_MESSAGE: &[u8] = &[0, 0, 0, 17, b'L', b'a', b'y', b'e', b'r', b'X'];
const OTHER_NETWORK_MESSAGE: &[u8] = &[0, 0, 0, 18, b'L', b'a', b'y', b'e', b'r', b'X'];

fn message(domain: Domain, network_id: u32) -> SignatureMessage<'static> {
    let Ok(value) = SignatureMessage::new(domain, PROTOCOL_VERSION, network_id, CORE_MESSAGE)
    else {
        panic!("valid signature scope rejected");
    };
    value
}

fn ed25519_vector() -> ([u8; 32], [u8; 64]) {
    let key = Ed25519SigningKey::from_bytes(&ED25519_SEED);
    let signature = key.sign(&message(Domain::SignaturePreimage, 17).digest());
    (key.verifying_key().to_bytes(), signature.to_bytes())
}

#[test]
fn core_ed25519_vector_and_negative_forms_match() {
    let (public_key, signature) = ed25519_vector();
    assert_eq!(
        public_key,
        [
            0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64,
            0x07, 0x3a, 0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68,
            0xf7, 0x07, 0x51, 0x1a,
        ]
    );
    assert_eq!(
        ed25519::verify(
            &public_key,
            &signature,
            message(Domain::SignaturePreimage, 17)
        ),
        Ok(())
    );
    assert_eq!(
        ed25519::verify(&public_key, &signature, message(Domain::ActivityId, 17)),
        Err(VerifyError::BadSignature)
    );
    let mut non_reduced = signature;
    non_reduced[32..].fill(0xff);
    assert_eq!(
        ed25519::verify(
            &public_key,
            &non_reduced,
            message(Domain::SignaturePreimage, 17)
        ),
        Err(VerifyError::BadSignature)
    );
    assert_eq!(
        ed25519::verify(
            &[
                1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0
            ],
            &signature,
            message(Domain::SignaturePreimage, 17)
        ),
        Err(VerifyError::BadSignature)
    );
}

#[test]
fn core_secp256k1_vector_and_malleable_forms_match() {
    let mut private_key = [0_u8; 32];
    private_key[31] = 1;
    let Ok(signing_key) = Secp256k1SigningKey::from_bytes((&private_key).into()) else {
        panic!("core private key rejected");
    };
    let scoped = message(Domain::CheckpointCertificate, 17);
    let digest = scoped.digest();
    let Ok(signature): Result<Secp256k1Signature, _> = signing_key.sign_prehash(&digest) else {
        panic!("core secp256k1 vector signing failed");
    };
    let public_key = signing_key.verifying_key().to_encoded_point(true);
    assert_eq!(
        public_key.as_bytes(),
        &[
            0x02, 0x79, 0xbe, 0x66, 0x7e, 0xf9, 0xdc, 0xbb, 0xac, 0x55, 0xa0, 0x62, 0x95, 0xce,
            0x87, 0x0b, 0x07, 0x02, 0x9b, 0xfc, 0xdb, 0x2d, 0xce, 0x28, 0xd9, 0x59, 0xf2, 0x81,
            0x5b, 0x16, 0xf8, 0x17, 0x98,
        ]
    );
    let signature_bytes: [u8; 64] = signature.to_bytes().into();
    let (recoverable_signature, recovery_id) = signing_key
        .sign_prehash_recoverable(&digest)
        .unwrap_or_else(|error| panic!("recoverable secp256k1 signing failed: {error}"));
    let recoverable_bytes: [u8; 64] = recoverable_signature.to_bytes().into();
    assert_eq!(recoverable_bytes, signature_bytes);
    assert_eq!(
        secp256k1::evm_address(public_key.as_bytes()),
        Ok([
            0x7e, 0x5f, 0x45, 0x52, 0x09, 0x1a, 0x69, 0x12, 0x5d, 0x5d, 0xfc, 0xb7, 0xb8, 0xc2,
            0x65, 0x90, 0x29, 0x39, 0x5b, 0xdf,
        ])
    );
    assert_eq!(
        secp256k1::verify_recoverable_digest(
            public_key.as_bytes(),
            &signature_bytes,
            27 + u8::from(recovery_id),
            &digest,
        ),
        Ok(())
    );
    assert_eq!(
        secp256k1::verify_recoverable_digest(public_key.as_bytes(), &signature_bytes, 29, &digest,),
        Err(VerifyError::BadSignature)
    );
    assert_eq!(
        secp256k1::verify(public_key.as_bytes(), &signature_bytes, scoped),
        Ok(())
    );
    let mut high_s = [0_u8; 64];
    high_s[31] = 1;
    high_s[32..].copy_from_slice(&[
        0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0x5d, 0x57, 0x6e, 0x73, 0x57, 0xa4, 0x50, 0x1d, 0xdf, 0xe9, 0x2f, 0x46, 0x68, 0x1b,
        0x20, 0xa1,
    ]);
    assert_eq!(
        secp256k1::verify(public_key.as_bytes(), &high_s, scoped),
        Err(VerifyError::BadSignature)
    );
    assert_eq!(
        secp256k1::verify(public_key.as_bytes(), &[0xff; 64], scoped),
        Err(VerifyError::BadSignature)
    );
}

#[test]
fn protocol_and_canonical_network_bytes_cannot_reuse_signatures() {
    let (public_key, signature) = ed25519_vector();
    let other_network = SignatureMessage::new(
        Domain::SignaturePreimage,
        PROTOCOL_VERSION,
        18,
        OTHER_NETWORK_MESSAGE,
    )
    .unwrap_or_else(|error| panic!("network-scoped bytes rejected: {error:?}"));
    assert_eq!(
        ed25519::verify(&public_key, &signature, other_network),
        Err(VerifyError::BadSignature)
    );
    assert!(SignatureMessage::new(
        Domain::SignaturePreimage,
        LEGACY_PROTOCOL_VERSION,
        17,
        CORE_MESSAGE,
    )
    .is_ok());
    assert_eq!(
        SignatureMessage::new(Domain::SignaturePreimage, 3, 17, CORE_MESSAGE).err(),
        Some(VerifyError::VersionUnsupported)
    );
    assert_eq!(
        SignatureMessage::new(Domain::SignaturePreimage, PROTOCOL_VERSION, 0, CORE_MESSAGE).err(),
        Some(VerifyError::WrongNetwork)
    );
    assert_eq!(
        VerifyError::BadSignature.result_code().known(),
        Some(KnownResult::BadSignature)
    );
}

#[test]
fn comparison_is_exact_for_digests_and_secret_bytes() {
    assert!(ct::eq(&[7; 32], &[7; 32]));
    assert!(!ct::eq(&[7; 32], &[7; 31]));
    assert!(!ct::eq_fixed(&[7; 32], &[6; 32]));
}

#[test]
fn state_commitment_signatures_bind_exact_canonical_bytes() {
    let key = Ed25519SigningKey::from_bytes(&ED25519_SEED);
    let version = layerx_wire::limits::STATE_COMMITMENT_PROTOCOL_VERSION;
    let mut canonical = version.to_be_bytes().to_vec();
    canonical.extend_from_slice(CORE_MESSAGE);
    let message = SignatureMessage::new(Domain::SignaturePreimage, version, 17, &canonical)
        .expect("explicit version three signature message");
    let signature = key.sign(&message.digest()).to_bytes();
    assert_eq!(
        ed25519::verify(&key.verifying_key().to_bytes(), &signature, message),
        Ok(())
    );
    canonical[1] = 2;
    let altered = SignatureMessage::new(Domain::SignaturePreimage, 2, 17, &canonical)
        .expect("legacy version remains supported");
    assert_eq!(
        ed25519::verify(&key.verifying_key().to_bytes(), &signature, altered),
        Err(VerifyError::BadSignature)
    );
}

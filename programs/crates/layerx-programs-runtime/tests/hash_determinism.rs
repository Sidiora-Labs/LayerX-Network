//! Determinism differential for cryptographic hash primitives.

use layerx_programs_runtime::{hash_bytes, HashAlgorithm};

#[test]
fn sha256_is_deterministic_across_invocations() {
    let inputs = [
        b"" as &[u8],
        b"a",
        b"abc",
        b"message digest",
        b"abcdefghijklmnopqrstuvwxyz",
        &[0u8; 1024],
        &[0xffu8; 4096],
    ];
    for input in inputs {
        let first = hash_bytes(HashAlgorithm::Sha256, input).unwrap();
        let second = hash_bytes(HashAlgorithm::Sha256, input).unwrap();
        assert_eq!(
            first, second,
            "sha256 diverged on input of length {}",
            input.len()
        );
    }
}

#[test]
fn keccak256_is_deterministic_across_invocations() {
    let inputs = [
        b"" as &[u8],
        b"a",
        b"abc",
        b"message digest",
        b"abcdefghijklmnopqrstuvwxyz",
        &[0u8; 1024],
        &[0xffu8; 4096],
    ];
    for input in inputs {
        let first = hash_bytes(HashAlgorithm::Keccak256, input).unwrap();
        let second = hash_bytes(HashAlgorithm::Keccak256, input).unwrap();
        assert_eq!(
            first, second,
            "keccak256 diverged on input of length {}",
            input.len()
        );
    }
}

#[test]
fn blake3_is_deterministic_across_invocations() {
    let inputs = [
        b"" as &[u8],
        b"a",
        b"abc",
        b"message digest",
        b"abcdefghijklmnopqrstuvwxyz",
        &[0u8; 1024],
        &[0xffu8; 4096],
    ];
    for input in inputs {
        let first = hash_bytes(HashAlgorithm::Blake3, input).unwrap();
        let second = hash_bytes(HashAlgorithm::Blake3, input).unwrap();
        assert_eq!(
            first, second,
            "blake3 diverged on input of length {}",
            input.len()
        );
    }
}

#[test]
fn sha256_golden_vectors() {
    assert_eq!(
        hash_bytes(HashAlgorithm::Sha256, b"").unwrap(),
        [
            0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f,
            0xb9, 0x24, 0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b,
            0x78, 0x52, 0xb8, 0x55
        ]
    );
    assert_eq!(
        hash_bytes(HashAlgorithm::Sha256, b"abc").unwrap(),
        [
            0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
            0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
            0xf2, 0x00, 0x15, 0xad
        ]
    );
    assert_eq!(
        hash_bytes(
            HashAlgorithm::Sha256,
            b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
        )
        .unwrap(),
        [
            0x24, 0x8d, 0x6a, 0x61, 0xd2, 0x06, 0x38, 0xb8, 0xe5, 0xc0, 0x26, 0x93, 0x0c, 0x3e,
            0x60, 0x39, 0xa3, 0x3c, 0xe4, 0x59, 0x64, 0xff, 0x21, 0x67, 0xf6, 0xec, 0xed, 0xd4,
            0x19, 0xdb, 0x06, 0xc1
        ]
    );
}

#[test]
fn keccak256_golden_vectors() {
    assert_eq!(
        hash_bytes(HashAlgorithm::Keccak256, b"").unwrap(),
        [
            0xc5, 0xd2, 0x46, 0x01, 0x86, 0xf7, 0x23, 0x3c, 0x92, 0x7e, 0x7d, 0xb2, 0xdc, 0xc7,
            0x03, 0xc0, 0xe5, 0x00, 0xb6, 0x53, 0xca, 0x82, 0x27, 0x3b, 0x7b, 0xfa, 0xd8, 0x04,
            0x5d, 0x85, 0xa4, 0x70
        ]
    );
    assert_eq!(
        hash_bytes(HashAlgorithm::Keccak256, b"abc").unwrap(),
        [
            0x4e, 0x03, 0x65, 0x7a, 0xea, 0x45, 0xa9, 0x4f, 0xc7, 0xd4, 0x7b, 0xa8, 0x26, 0xc8,
            0xd6, 0x67, 0xc0, 0xd1, 0xe6, 0xe3, 0x3a, 0x64, 0xa0, 0x36, 0xec, 0x44, 0xf5, 0x8f,
            0xa1, 0x2d, 0x6c, 0x45
        ]
    );
    assert_eq!(
        hash_bytes(
            HashAlgorithm::Keccak256,
            b"The quick brown fox jumps over the lazy dog"
        )
        .unwrap(),
        [
            0x4d, 0x74, 0x1b, 0x6f, 0x1e, 0xb2, 0x9c, 0xb2, 0xa9, 0xb9, 0x91, 0x1c, 0x82, 0xf5,
            0x6f, 0xa8, 0xd7, 0x3b, 0x04, 0x95, 0x9d, 0x3d, 0x9d, 0x22, 0x28, 0x95, 0xdf, 0x6c,
            0x0b, 0x28, 0xaa, 0x15
        ]
    );
}

#[test]
fn blake3_golden_vectors() {
    assert_eq!(
        hash_bytes(HashAlgorithm::Blake3, b"").unwrap(),
        [
            0xaf, 0x13, 0x49, 0xb9, 0xf5, 0xf9, 0xa1, 0xa6, 0xa0, 0x40, 0x4d, 0xea, 0x36, 0xdc,
            0xc9, 0x49, 0x9b, 0xcb, 0x25, 0xc9, 0xad, 0xc1, 0x12, 0xb7, 0xcc, 0x9a, 0x93, 0xca,
            0xe4, 0x1f, 0x32, 0x62
        ]
    );
    assert_eq!(
        hash_bytes(HashAlgorithm::Blake3, b"abc").unwrap(),
        [
            0x6d, 0xd3, 0xce, 0xca, 0xd2, 0x65, 0xd5, 0x6f, 0x92, 0x04, 0x93, 0x4b, 0x11, 0xa5,
            0xea, 0xe7, 0x1b, 0xa2, 0xd1, 0x1d, 0xb4, 0xd4, 0xd8, 0xa5, 0x66, 0xbd, 0x48, 0x8a,
            0xba, 0x02, 0xbe, 0x48
        ]
    );
    assert_eq!(
        hash_bytes(HashAlgorithm::Blake3, b"hello world").unwrap(),
        [
            0xd7, 0x4e, 0x1a, 0x63, 0xf1, 0x0c, 0x5c, 0xe8, 0x38, 0x6a, 0x72, 0x0c, 0x55, 0x3d,
            0xd4, 0xbe, 0x51, 0x13, 0x1e, 0x7d, 0xc5, 0x57, 0x53, 0x12, 0xd0, 0xb3, 0x79, 0x5c,
            0x5c, 0xe8, 0x76, 0x48
        ]
    );
}

#[test]
fn all_algorithms_produce_32_byte_digests() {
    let input = b"test input";
    for algorithm in [
        HashAlgorithm::Sha256,
        HashAlgorithm::Keccak256,
        HashAlgorithm::Blake3,
    ] {
        let digest = hash_bytes(algorithm, input).unwrap();
        assert_eq!(digest.len(), 32, "{algorithm} produced wrong output length");
    }
}

#[test]
fn different_algorithms_produce_different_digests() {
    let input = b"test input";
    let sha256 = hash_bytes(HashAlgorithm::Sha256, input).unwrap();
    let keccak256 = hash_bytes(HashAlgorithm::Keccak256, input).unwrap();
    let blake3 = hash_bytes(HashAlgorithm::Blake3, input).unwrap();
    assert_ne!(sha256, keccak256);
    assert_ne!(sha256, blake3);
    assert_ne!(keccak256, blake3);
}

#[test]
fn cross_platform_byte_identity_sha256() {
    let test_vectors = [
        (b"" as &[u8], "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"),
        (b"abc", "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
        (&[0u8; 64], "f5a5fd42d16a20302798ef6ed309979b43003d2320d9f0e8ea9831a92759fb4b"),
    ];
    for (input, expected_hex) in test_vectors {
        let digest = hash_bytes(HashAlgorithm::Sha256, input).unwrap();
        let hex = format_hex(&digest);
        assert_eq!(hex, expected_hex, "sha256 diverged for input length {}", input.len());
    }
}

#[test]
fn cross_platform_byte_identity_keccak256() {
    let test_vectors = [
        (b"" as &[u8], "c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470"),
        (b"abc", "4e03657aea45a94fc7d47ba826c8d667c0d1e6e33a64a036ec44f58fa12d6c45"),
        (b"The quick brown fox jumps over the lazy dog", "4d741b6f1eb29cb2a9b9911c82f56fa8d73b04959d3d9d222895df6c0b28aa15"),
    ];
    for (input, expected_hex) in test_vectors {
        let digest = hash_bytes(HashAlgorithm::Keccak256, input).unwrap();
        let hex = format_hex(&digest);
        assert_eq!(hex, expected_hex, "keccak256 diverged for input length {}", input.len());
    }
}

#[test]
fn cross_platform_byte_identity_blake3() {
    let test_vectors = [
        (b"" as &[u8], "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"),
        (b"abc", "6dd3cecad265d56f9204934b11a5eae71ba2d11db4d4d8a566bd488aba02be48"),
        (b"hello world", "d74e1a63f10c5ce8386a720c553dd4be51131e7dc5575312d0b3795c5ce87648"),
    ];
    for (input, expected_hex) in test_vectors {
        let digest = hash_bytes(HashAlgorithm::Blake3, input).unwrap();
        let hex = format_hex(&digest);
        assert_eq!(hex, expected_hex, "blake3 diverged for input length {}", input.len());
    }
}

fn format_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

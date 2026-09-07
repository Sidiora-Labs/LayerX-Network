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
        let first = hash_bytes(HashAlgorithm::Sha256, input)
            .unwrap_or_else(|error| panic!("{}: {error:?}", "required value"));
        let second = hash_bytes(HashAlgorithm::Sha256, input)
            .unwrap_or_else(|error| panic!("{}: {error:?}", "required value"));
        assert_eq!(
            first,
            second,
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
        let first = hash_bytes(HashAlgorithm::Keccak256, input)
            .unwrap_or_else(|error| panic!("{}: {error:?}", "required value"));
        let second = hash_bytes(HashAlgorithm::Keccak256, input)
            .unwrap_or_else(|error| panic!("{}: {error:?}", "required value"));
        assert_eq!(
            first,
            second,
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
        let first = hash_bytes(HashAlgorithm::Blake3, input)
            .unwrap_or_else(|error| panic!("{}: {error:?}", "required value"));
        let second = hash_bytes(HashAlgorithm::Blake3, input)
            .unwrap_or_else(|error| panic!("{}: {error:?}", "required value"));
        assert_eq!(
            first,
            second,
            "blake3 diverged on input of length {}",
            input.len()
        );
    }
}

#[test]
fn sha256_golden_vectors() {
    assert_eq!(
        hash_bytes(HashAlgorithm::Sha256, b"")
            .unwrap_or_else(|error| panic!("{}: {error:?}", "required value")),
        [
            0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f,
            0xb9, 0x24, 0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b,
            0x78, 0x52, 0xb8, 0x55
        ]
    );
    assert_eq!(
        hash_bytes(HashAlgorithm::Sha256, b"abc")
            .unwrap_or_else(|error| panic!("{}: {error:?}", "required value")),
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
        .unwrap_or_else(|error| panic!("{}: {error:?}", "required value")),
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
        hash_bytes(HashAlgorithm::Keccak256, b"")
            .unwrap_or_else(|error| panic!("{}: {error:?}", "required value")),
        [
            0xc5, 0xd2, 0x46, 0x01, 0x86, 0xf7, 0x23, 0x3c, 0x92, 0x7e, 0x7d, 0xb2, 0xdc, 0xc7,
            0x03, 0xc0, 0xe5, 0x00, 0xb6, 0x53, 0xca, 0x82, 0x27, 0x3b, 0x7b, 0xfa, 0xd8, 0x04,
            0x5d, 0x85, 0xa4, 0x70
        ]
    );
    assert_eq!(
        hash_bytes(HashAlgorithm::Keccak256, b"abc")
            .unwrap_or_else(|error| panic!("{}: {error:?}", "required value")),
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
        .unwrap_or_else(|error| panic!("{}: {error:?}", "required value")),
        [
            0x4d, 0x74, 0x1b, 0x6f, 0x1e, 0xb2, 0x9c, 0xb2, 0xa9, 0xb9, 0x91, 0x1c, 0x82, 0xf5,
            0x6f, 0xa8, 0xd7, 0x3b, 0x04, 0x95, 0x9d, 0x3d, 0x9d, 0x22, 0x28, 0x95, 0xdf, 0x6c,
            0x0b, 0x28, 0xaa, 0x15
        ]
    );
}

#[test]
fn blake3_golden_vectors() {
    // Text digests independently confirmed by the official 1.5.0 reference below.
    assert_eq!(
        hash_bytes(HashAlgorithm::Blake3, b"")
            .unwrap_or_else(|error| panic!("{}: {error:?}", "required value")),
        [
            0xaf, 0x13, 0x49, 0xb9, 0xf5, 0xf9, 0xa1, 0xa6, 0xa0, 0x40, 0x4d, 0xea, 0x36, 0xdc,
            0xc9, 0x49, 0x9b, 0xcb, 0x25, 0xc9, 0xad, 0xc1, 0x12, 0xb7, 0xcc, 0x9a, 0x93, 0xca,
            0xe4, 0x1f, 0x32, 0x62
        ]
    );
    assert_eq!(
        hash_bytes(HashAlgorithm::Blake3, b"abc")
            .unwrap_or_else(|error| panic!("{}: {error:?}", "required value")),
        [
            0x64, 0x37, 0xb3, 0xac, 0x38, 0x46, 0x51, 0x33, 0xff, 0xb6, 0x3b, 0x75, 0x27, 0x3a,
            0x8d, 0xb5, 0x48, 0xc5, 0x58, 0x46, 0x5d, 0x79, 0xdb, 0x03, 0xfd, 0x35, 0x9c, 0x6c,
            0xd5, 0xbd, 0x9d, 0x85
        ]
    );
    assert_eq!(
        hash_bytes(HashAlgorithm::Blake3, b"hello world")
            .unwrap_or_else(|error| panic!("{}: {error:?}", "required value")),
        [
            0xd7, 0x49, 0x81, 0xef, 0xa7, 0x0a, 0x0c, 0x88, 0x0b, 0x8d, 0x8c, 0x19, 0x85, 0xd0,
            0x75, 0xdb, 0xcb, 0xf6, 0x79, 0xb9, 0x9a, 0x5f, 0x99, 0x14, 0xe5, 0xaa, 0xf9, 0x6b,
            0x83, 0x1a, 0x9e, 0x24
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
        let digest = hash_bytes(algorithm, input)
            .unwrap_or_else(|error| panic!("{}: {error:?}", "required value"));
        assert_eq!(digest.len(), 32, "{algorithm} produced wrong output length");
    }
}

#[test]
fn different_algorithms_produce_different_digests() {
    let input = b"test input";
    let sha256 = hash_bytes(HashAlgorithm::Sha256, input)
        .unwrap_or_else(|error| panic!("{}: {error:?}", "required value"));
    let keccak256 = hash_bytes(HashAlgorithm::Keccak256, input)
        .unwrap_or_else(|error| panic!("{}: {error:?}", "required value"));
    let blake3 = hash_bytes(HashAlgorithm::Blake3, input)
        .unwrap_or_else(|error| panic!("{}: {error:?}", "required value"));
    assert_ne!(sha256, keccak256);
    assert_ne!(sha256, blake3);
    assert_ne!(keccak256, blake3);
}

#[test]
fn cross_platform_byte_identity_sha256() {
    let test_vectors = [
        (
            b"" as &[u8],
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        ),
        (
            b"abc",
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        ),
        (
            &[0u8; 64],
            "f5a5fd42d16a20302798ef6ed309979b43003d2320d9f0e8ea9831a92759fb4b",
        ),
    ];
    for (input, expected_hex) in test_vectors {
        let digest = hash_bytes(HashAlgorithm::Sha256, input)
            .unwrap_or_else(|error| panic!("{}: {error:?}", "required value"));
        let hex = format_hex(&digest);
        assert_eq!(
            hex,
            expected_hex,
            "sha256 diverged for input length {}",
            input.len()
        );
    }
}

#[test]
fn cross_platform_byte_identity_keccak256() {
    let test_vectors = [
        (
            b"" as &[u8],
            "c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470",
        ),
        (
            b"abc",
            "4e03657aea45a94fc7d47ba826c8d667c0d1e6e33a64a036ec44f58fa12d6c45",
        ),
        (
            b"The quick brown fox jumps over the lazy dog",
            "4d741b6f1eb29cb2a9b9911c82f56fa8d73b04959d3d9d222895df6c0b28aa15",
        ),
    ];
    for (input, expected_hex) in test_vectors {
        let digest = hash_bytes(HashAlgorithm::Keccak256, input)
            .unwrap_or_else(|error| panic!("{}: {error:?}", "required value"));
        let hex = format_hex(&digest);
        assert_eq!(
            hex,
            expected_hex,
            "keccak256 diverged for input length {}",
            input.len()
        );
    }
}

#[test]
fn cross_platform_byte_identity_blake3() {
    let test_vectors = [
        (
            b"" as &[u8],
            "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262",
        ),
        (
            b"abc",
            "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85",
        ),
        (
            b"hello world",
            "d74981efa70a0c880b8d8c1985d075dbcbf679b99a5f9914e5aaf96b831a9e24",
        ),
    ];
    for (input, expected_hex) in test_vectors {
        let digest = hash_bytes(HashAlgorithm::Blake3, input)
            .unwrap_or_else(|error| panic!("{}: {error:?}", "required value"));
        let hex = format_hex(&digest);
        assert_eq!(
            hex,
            expected_hex,
            "blake3 diverged for input length {}",
            input.len()
        );
    }
}

fn format_hex(bytes: &[u8]) -> String {
    hex::encode(bytes)
}

// Authorities: https://github.com/BLAKE3-team/BLAKE3/blob/1.5.0/test_vectors/test_vectors.json
// SHA256: dcb91ea8accc77e6d6e632af7cdc1a99a9f3ae78cf648da595c7d064db32f624.
// Reference: https://github.com/BLAKE3-team/BLAKE3/blob/1.5.0/reference_impl/reference_impl.rs.
// SHA256: 2fae31faabb79094ebb8c8377e5f1d804ae46902f8cc2a8b31e14d2632948ca0.
#[path = "support/blake3_reference.rs"]
mod blake3_reference;

// Every input_len and the first 32 bytes of hash from the official 1.5.0 file.
const BLAKE3_OFFICIAL_VECTORS: [(usize, &str); 35] = [
    (
        0,
        "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262",
    ),
    (
        1,
        "2d3adedff11b61f14c886e35afa036736dcd87a74d27b5c1510225d0f592e213",
    ),
    (
        2,
        "7b7015bb92cf0b318037702a6cdd81dee41224f734684c2c122cd6359cb1ee63",
    ),
    (
        3,
        "e1be4d7a8ab5560aa4199eea339849ba8e293d55ca0a81006726d184519e647f",
    ),
    (
        4,
        "f30f5ab28fe047904037f77b6da4fea1e27241c5d132638d8bedce9d40494f32",
    ),
    (
        5,
        "b40b44dfd97e7a84a996a91af8b85188c66c126940ba7aad2e7ae6b385402aa2",
    ),
    (
        6,
        "06c4e8ffb6872fad96f9aaca5eee1553eb62aed0ad7198cef42e87f6a616c844",
    ),
    (
        7,
        "3f8770f387faad08faa9d8414e9f449ac68e6ff0417f673f602a646a891419fe",
    ),
    (
        8,
        "2351207d04fc16ade43ccab08600939c7c1fa70a5c0aaca76063d04c3228eaeb",
    ),
    (
        63,
        "e9bc37a594daad83be9470df7f7b3798297c3d834ce80ba85d6e207627b7db7b",
    ),
    (
        64,
        "4eed7141ea4a5cd4b788606bd23f46e212af9cacebacdc7d1f4c6dc7f2511b98",
    ),
    (
        65,
        "de1e5fa0be70df6d2be8fffd0e99ceaa8eb6e8c93a63f2d8d1c30ecb6b263dee",
    ),
    (
        127,
        "d81293fda863f008c09e92fc382a81f5a0b4a1251cba1634016a0f86a6bd640d",
    ),
    (
        128,
        "f17e570564b26578c33bb7f44643f539624b05df1a76c81f30acd548c44b45ef",
    ),
    (
        129,
        "683aaae9f3c5ba37eaaf072aed0f9e30bac0865137bae68b1fde4ca2aebdcb12",
    ),
    (
        1023,
        "10108970eeda3eb932baac1428c7a2163b0e924c9a9e25b35bba72b28f70bd11",
    ),
    (
        1024,
        "42214739f095a406f3fc83deb889744ac00df831c10daa55189b5d121c855af7",
    ),
    (
        1025,
        "d00278ae47eb27b34faecf67b4fe263f82d5412916c1ffd97c8cb7fb814b8444",
    ),
    (
        2048,
        "e776b6028c7cd22a4d0ba182a8bf62205d2ef576467e838ed6f2529b85fba24a",
    ),
    (
        2049,
        "5f4d72f40d7a5f82b15ca2b2e44b1de3c2ef86c426c95c1af0b6879522563030",
    ),
    (
        3072,
        "b98cb0ff3623be03326b373de6b9095218513e64f1ee2edd2525c7ad1e5cffd2",
    ),
    (
        3073,
        "7124b49501012f81cc7f11ca069ec9226cecb8a2c850cfe644e327d22d3e1cd3",
    ),
    (
        4096,
        "015094013f57a5277b59d8475c0501042c0b642e531b0a1c8f58d2163229e969",
    ),
    (
        4097,
        "9b4052b38f1c5fc8b1f9ff7ac7b27cd242487b3d890d15c96a1c25b8aa0fb995",
    ),
    (
        5120,
        "9cadc15fed8b5d854562b26a9536d9707cadeda9b143978f319ab34230535833",
    ),
    (
        5121,
        "628bd2cb2004694adaab7bbd778a25df25c47b9d4155a55f8fbd79f2fe154cff",
    ),
    (
        6144,
        "3e2e5b74e048f3add6d21faab3f83aa44d3b2278afb83b80b3c35164ebeca205",
    ),
    (
        6145,
        "f1323a8631446cc50536a9f705ee5cb619424d46887f3c376c695b70e0f0507f",
    ),
    (
        7168,
        "61da957ec2499a95d6b8023e2b0e604ec7f6b50e80a9678b89d2628e99ada77a",
    ),
    (
        7169,
        "a003fc7a51754a9b3c7fae0367ab3d782dccf28855a03d435f8cfe74605e7817",
    ),
    (
        8192,
        "aae792484c8efe4f19e2ca7d371d8c467ffb10748d8a5a1ae579948f718a2a63",
    ),
    (
        8193,
        "bab6c09cb8ce8cf459261398d2e7aef35700bf488116ceb94a36d0f5f1b7bc3b",
    ),
    (
        16384,
        "f875d6646de28985646f34ee13be9a576fd515f76b5b0a26bb324735041ddde4",
    ),
    (
        31744,
        "62b6960e1a44bcc1eb1a611a8d6235b6b4b78f32e7abc4fb4c6cdcce94895c47",
    ),
    (
        102_400,
        "bc3e3d41a1146b069abffad3c0d44860cf664390afce4d9661f7902e7943e085",
    ),
];

#[test]
fn blake3_official_vectors() -> Result<(), Box<dyn std::error::Error>> {
    let mut mismatches = Vec::new();
    for (input_len, expected) in BLAKE3_OFFICIAL_VECTORS {
        let input: Vec<u8> = (0_u8..=250).cycle().take(input_len).collect();
        let actual = hash_bytes(HashAlgorithm::Blake3, &input)?;
        let mut reference = blake3_reference::Hasher::new();
        reference.update(&input);
        let mut independent = [0; 32];
        reference.finalize(&mut independent);
        if format_hex(&actual) != expected || format_hex(&independent) != expected {
            mismatches.push((input_len, format_hex(&actual), format_hex(&independent)));
        }
    }
    assert!(
        mismatches.is_empty(),
        "official vector mismatches: {mismatches:?}"
    );
    Ok(())
}

#[test]
fn blake3_reference_confirms_text_inputs() -> Result<(), Box<dyn std::error::Error>> {
    for input in [b"abc".as_slice(), b"hello world".as_slice()] {
        let mut reference = blake3_reference::Hasher::new();
        reference.update(input);
        let mut independent = [0; 32];
        reference.finalize(&mut independent);
        let actual = hash_bytes(HashAlgorithm::Blake3, input)?;
        assert_eq!(actual, independent);
        println!("{input:?}: {}", format_hex(&independent));
    }
    Ok(())
}

#[test]
fn blake3_reference_modes_match_official_empty_vector() {
    let modes = [
        (
            blake3_reference::Hasher::new_keyed(b"whats the Elvish word for friend"),
            "92b2b75604ed3c761f9d6f62392c8a9227ad0ea3f09573e783f1498a4ed60d26",
        ),
        (
            blake3_reference::Hasher::new_derive_key(
                "BLAKE3 2019-12-27 16:29:52 test vectors context",
            ),
            "2cc39783c223154fea8dfb7c1b1660f2ac2dcbd1c1de8277b0b0dd39b7e50d7d",
        ),
    ];
    for (reference, expected) in modes {
        let mut digest = [0; 32];
        reference.finalize(&mut digest);
        assert_eq!(format_hex(&digest), expected);
    }
}

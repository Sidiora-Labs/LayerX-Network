// The reference signer is exercised through the publisher's own remote signer
// client over a real Unix domain socket: layerx_mirror::signer verifies every
// returned signature against the independently configured public key, so a
// successful round trip proves the daemon speaks signer-protocol.md and signs
// with the key material the mirror secret carries.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use ed25519_dalek::SigningKey as Ed25519SigningKey;
use k256::ecdsa::SigningKey as Secp256k1SigningKey;
use layerx_mirror::signer::{
    ChainSignature, RemoteChainSigner, RemoteSignerConfig, SignerEndpoint, SignerError,
    SigningAlgorithm,
};
use layerx_mirror_signer::{Options, SignerListener, ETHEREUM_POLICY_DOMAIN, SOLANA_POLICY_DOMAIN};

const ETHEREUM_KEY_HEX: &str = "4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f362318";
const SOLANA_SEED: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];

fn work_directory(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "layerx-mirror-signer-{}-{name}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory)
        .unwrap_or_else(|error| panic!("work directory {}: {error}", directory.display()));
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
        .unwrap_or_else(|error| panic!("work directory permissions: {error}"));
    directory
}

fn ethereum_signing_key() -> Secp256k1SigningKey {
    let mut raw = [0_u8; 32];
    for (index, slot) in raw.iter_mut().enumerate() {
        let pair = ETHEREUM_KEY_HEX
            .get(index * 2..index * 2 + 2)
            .unwrap_or_else(|| panic!("the fixture key is 32 hexadecimal bytes"));
        *slot = u8::from_str_radix(pair, 16)
            .unwrap_or_else(|error| panic!("the fixture key is hexadecimal: {error}"));
    }
    Secp256k1SigningKey::from_slice(&raw)
        .unwrap_or_else(|error| panic!("the fixture key is a secp256k1 scalar: {error}"))
}

fn write_key_material(directory: &Path) -> Options {
    let ethereum = directory.join("ethereum.key");
    fs::write(&ethereum, ETHEREUM_KEY_HEX)
        .unwrap_or_else(|error| panic!("ethereum key file: {error}"));
    fs::set_permissions(&ethereum, fs::Permissions::from_mode(0o600))
        .unwrap_or_else(|error| panic!("ethereum key permissions: {error}"));

    let key = Ed25519SigningKey::from_bytes(&SOLANA_SEED);
    let mut keypair = Vec::with_capacity(64);
    keypair.extend_from_slice(&SOLANA_SEED);
    keypair.extend_from_slice(&key.verifying_key().to_bytes());
    let values: Vec<String> = keypair.iter().map(ToString::to_string).collect();
    let solana = directory.join("solana.json");
    fs::write(&solana, format!("[{}]", values.join(",")))
        .unwrap_or_else(|error| panic!("solana keypair file: {error}"));
    fs::set_permissions(&solana, fs::Permissions::from_mode(0o600))
        .unwrap_or_else(|error| panic!("solana keypair permissions: {error}"));

    Options {
        socket: directory.join("signer.sock"),
        ethereum_key_file: ethereum,
        ethereum_key_handle: "mirror/ethereum/beta".to_owned(),
        solana_keypair_file: solana,
        solana_key_handle: "mirror/solana/beta".to_owned(),
    }
}

fn serve(options: &Options) {
    let listener = SignerListener::bind(options)
        .unwrap_or_else(|error| panic!("the signer refused to start: {error}"));
    let mode = fs::metadata(listener.socket())
        .unwrap_or_else(|error| panic!("the signer socket is missing: {error}"))
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(
        mode, 0o660,
        "the signer socket must be reachable by its owner and group only"
    );
    thread::spawn(move || listener.serve());
}

fn client(options: &Options, algorithm: SigningAlgorithm, handle: &str) -> RemoteChainSigner {
    let public_key = match algorithm {
        SigningAlgorithm::Secp256k1Recoverable => ethereum_signing_key()
            .verifying_key()
            .to_encoded_point(true)
            .as_bytes()
            .to_vec(),
        SigningAlgorithm::Ed25519 => Ed25519SigningKey::from_bytes(&SOLANA_SEED)
            .verifying_key()
            .to_bytes()
            .to_vec(),
    };
    RemoteChainSigner::new(RemoteSignerConfig {
        endpoint: SignerEndpoint::Uds {
            socket: options.socket.clone(),
        },
        algorithm,
        key_handle: handle.to_owned(),
        public_key,
        timeout: Duration::from_secs(5),
    })
    .unwrap_or_else(|error| panic!("the publisher signer client refused the fixture: {error:?}"))
}

#[test]
fn both_publisher_keys_round_trip_through_the_publisher_signer_client() {
    let directory = work_directory("round-trip");
    let options = write_key_material(&directory);
    serve(&options);

    let ethereum = client(
        &options,
        SigningAlgorithm::Secp256k1Recoverable,
        &options.ethereum_key_handle,
    );
    let digest = [0x3a_u8; 32];
    match ethereum.sign_digest(ETHEREUM_POLICY_DOMAIN, digest) {
        Ok(ChainSignature::Secp256k1(signature)) => {
            assert!(
                signature[64] <= 1,
                "the recovery identifier must be 0 or 1, got {}",
                signature[64]
            );
        }
        other => panic!("the Ethereum publisher key did not sign: {other:?}"),
    }
    let address = ethereum
        .ethereum_address()
        .unwrap_or_else(|error| panic!("the publisher address is underivable: {error:?}"));
    assert_ne!(address, [0_u8; 20], "the publisher address must be real");

    let solana = client(
        &options,
        SigningAlgorithm::Ed25519,
        &options.solana_key_handle,
    );
    let message = b"LayerX mirror archive transaction message";
    match solana.sign_message(SOLANA_POLICY_DOMAIN, message) {
        Ok(ChainSignature::Ed25519(signature)) => {
            assert_ne!(signature, [0_u8; 64], "the Solana signature must be real");
        }
        other => panic!("the Solana publisher key did not sign: {other:?}"),
    }

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn an_unknown_handle_and_a_foreign_policy_domain_are_refused() {
    let directory = work_directory("refusals");
    let options = write_key_material(&directory);
    serve(&options);

    let unknown = client(
        &options,
        SigningAlgorithm::Secp256k1Recoverable,
        "mirror/ethereum/not-in-this-signer",
    );
    assert_eq!(
        unknown.sign_digest(ETHEREUM_POLICY_DOMAIN, [0x11_u8; 32]),
        Err(SignerError::Refused),
        "an unknown handle must be refused"
    );

    let ethereum = client(
        &options,
        SigningAlgorithm::Secp256k1Recoverable,
        &options.ethereum_key_handle,
    );
    assert_eq!(
        ethereum.sign_digest(SOLANA_POLICY_DOMAIN, [0x11_u8; 32]),
        Err(SignerError::Refused),
        "the Ethereum key must refuse the Solana policy domain"
    );

    let solana = client(
        &options,
        SigningAlgorithm::Ed25519,
        &options.solana_key_handle,
    );
    assert_eq!(
        solana.sign_message(ETHEREUM_POLICY_DOMAIN, b"archive"),
        Err(SignerError::Refused),
        "the Solana key must refuse the Ethereum policy domain"
    );

    let _ = fs::remove_dir_all(&directory);
}

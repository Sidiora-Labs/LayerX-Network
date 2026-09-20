//! Emits the JSON fixture consumed by the Paxeer account-binding tests
//! (`modules/evm/keeper` and `precompiles/addr`). A LayerX DID key consents to
//! a binding with an EVM address by signing
//! `"LX:PAXEER-BIND:v1" || chain id (u256 BE) || EVM address || nonce (u64 BE)`.
//! Every message is assembled here, signed with the Ed25519 key type the
//! LayerX local signer uses, and judged by the strict LayerX verifier; the
//! recorded `valid` flag is that verifier's own answer. The main account
//! identifier is derived by the real wire crate.
//!
//! Usage: `cargo run -p layerx-client --example paxeer_bind_vectors > paxeer_bind_vectors.json`

use std::fmt::Write as _;

use ed25519_dalek::{Signer as _, SigningKey};
use layerx_crypto::ed25519::verify_message;
use layerx_types::account::AccountId;
use layerx_wire::hash::account_id_for_protocol;

type Failure = Box<dyn std::error::Error>;

const DOMAIN: &[u8] = b"LX:PAXEER-BIND:v1";
const PROTOCOL: u16 = 3;
const EVM_ADDRESS: [u8; 20] = [
    0x10, 0x21, 0x32, 0x43, 0x54, 0x65, 0x76, 0x87, 0x98, 0xa9, 0xba, 0xcb, 0xdc, 0xed, 0xfe, 0x0f,
    0x1e, 0x2d, 0x3c, 0x4b,
];

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

fn bind_message(chain_id: u64, evm_address: &[u8; 20], nonce: u64) -> Vec<u8> {
    let mut message = Vec::with_capacity(DOMAIN.len() + 32 + 20 + 8);
    message.extend_from_slice(DOMAIN);
    message.extend_from_slice(&[0_u8; 24]);
    message.extend_from_slice(&chain_id.to_be_bytes());
    message.extend_from_slice(evm_address);
    message.extend_from_slice(&nonce.to_be_bytes());
    message
}

struct Case {
    name: &'static str,
    chain_id: u64,
    nonce: u64,
    signed_chain_id: u64,
    signed_nonce: u64,
    expected: bool,
}

fn main() -> Result<(), Failure> {
    let key = SigningKey::from_bytes(&[0x61; 32]);
    let public = key.verifying_key().to_bytes();
    let did = format!("did:layerx:{}", hex(&public));
    let account_name = format!("agent:{did}:main");
    let account = AccountId::parse(&account_name).map_err(|error| format!("{error:?}"))?;
    let account_id =
        account_id_for_protocol(&account, PROTOCOL).map_err(|error| format!("{error:?}"))?;

    let cases = [
        Case {
            name: "default-chain-nonce-0",
            chain_id: 713_714,
            nonce: 0,
            signed_chain_id: 713_714,
            signed_nonce: 0,
            expected: true,
        },
        Case {
            name: "paxeer-mainnet-nonce-2",
            chain_id: 125,
            nonce: 2,
            signed_chain_id: 125,
            signed_nonce: 2,
            expected: true,
        },
        Case {
            name: "signed-for-another-chain",
            chain_id: 713_714,
            nonce: 0,
            signed_chain_id: 125,
            signed_nonce: 0,
            expected: false,
        },
        Case {
            name: "signed-for-another-nonce",
            chain_id: 713_714,
            nonce: 0,
            signed_chain_id: 713_714,
            signed_nonce: 1,
            expected: false,
        },
    ];

    let mut entries = Vec::new();
    for case in cases {
        let message = bind_message(case.chain_id, &EVM_ADDRESS, case.nonce);
        let signed = bind_message(case.signed_chain_id, &EVM_ADDRESS, case.signed_nonce);
        let signature = key.sign(&signed).to_bytes();
        let valid = verify_message(&public, &signature, &message).is_ok();
        if valid != case.expected {
            return Err(format!("{}: verifier answered {valid}", case.name).into());
        }
        entries.push(format!(
            "    {{\n      \"name\": \"{}\",\n      \"chain_id\": \"{}\",\n      \"evm_address\": \"{}\",\n      \"nonce\": \"{}\",\n      \"message\": \"{}\",\n      \"signature\": \"{}\",\n      \"valid\": {}\n    }}",
            case.name,
            case.chain_id,
            hex(&EVM_ADDRESS),
            case.nonce,
            hex(&message),
            hex(&signature),
            valid
        ));
    }
    println!(
        "{{\n  \"generator\": \"agent/crates/layerx-client/examples/paxeer_bind_vectors.rs\",\n  \"public_key\": \"{}\",\n  \"did\": \"{}\",\n  \"main_account_name\": \"{}\",\n  \"main_account_id\": \"{}\",\n  \"binds\": [\n{}\n  ]\n}}",
        hex(&public),
        did,
        account_name,
        hex(&account_id),
        entries.join(",\n")
    );
    Ok(())
}

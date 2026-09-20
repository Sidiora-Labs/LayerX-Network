//! Emits the JSON fixture consumed by the Paxeer account-binding tests
//! (`modules/evm/keeper` and `precompiles/addr`). A LayerX DID key consents to
//! a binding with an EVM address by signing
//! `"LX:PAXEER-BIND:v1" || chain id (u256 BE) || EVM address || nonce (u64 BE)`.
//! Every message is assembled and signed by the crate's own `paxeer_binding`
//! helper — the same code path a client calling `bindLayerX` takes — and judged
//! by the strict LayerX verifier; the recorded `valid` flag is that verifier's
//! own answer. The main account identifier is derived by the real wire crate.
//!
//! Usage: `cargo run -p layerx-client --example paxeer_bind_vectors > paxeer_bind_vectors.json`

use std::fmt::Write as _;

use layerx_client::paxeer_binding::Binding;
use layerx_types::account::AccountId;
use layerx_wire::hash::account_id_for_protocol;

type Failure = Box<dyn std::error::Error>;

const PROTOCOL: u16 = 3;
const SEED: [u8; 32] = [0x61; 32];
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

struct Case {
    name: &'static str,
    chain_id: u64,
    nonce: u64,
    signed_chain_id: u64,
    signed_nonce: u64,
    expected: bool,
}

fn main() -> Result<(), Failure> {
    let identity = Binding::new(0, EVM_ADDRESS, 0).sign(&SEED);
    let public = identity.public_key();
    let did = identity.did();
    let account_name = identity.main_account_name();
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
        let claimed = Binding::new(case.chain_id, EVM_ADDRESS, case.nonce);
        let consent =
            Binding::new(case.signed_chain_id, EVM_ADDRESS, case.signed_nonce).sign(&SEED);
        let signature = consent.signature();
        let valid = claimed.verify(&public, &signature).is_ok();
        if valid != case.expected {
            return Err(format!("{}: verifier answered {valid}", case.name).into());
        }
        entries.push(format!(
            "    {{\n      \"name\": \"{}\",\n      \"chain_id\": \"{}\",\n      \"evm_address\": \"{}\",\n      \"nonce\": \"{}\",\n      \"message\": \"{}\",\n      \"signature\": \"{}\",\n      \"valid\": {}\n    }}",
            case.name,
            case.chain_id,
            hex(&EVM_ADDRESS),
            case.nonce,
            hex(&claimed.message()),
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

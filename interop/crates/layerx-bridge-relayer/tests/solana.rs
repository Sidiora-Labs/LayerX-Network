//! The Solana inbound loop driven by recorded Solana and Paxeer JSON-RPC
//! exchanges (`tests/fixtures/solana_inbound.json`), a real signer socket and
//! a real journal on disk, in the format `tests/loops.rs` uses.

mod support;

use std::path::PathBuf;

use layerx_bridge_relayer::abi::{encode_bridge_in, LAYERX_BRIDGE_PRECOMPILE};
use layerx_bridge_relayer::attestation::{
    to_attestor_signature, uint256_from_u64, InboundAttestation,
};
use layerx_bridge_relayer::config::RelayerConfig;
use layerx_bridge_relayer::hex;
use layerx_bridge_relayer::journal::{inbound_key, Completion, Journal};
use layerx_bridge_relayer::relayer::{
    inbound_stream, ChainLink, ChainSettings, GasPolicy, PaxeerLink, PaxeerSettings, Relayer,
    RelayerAssembly, RelayerError, RelayerParts, SolanaLink, StepReport, OUTBOUND_STREAM,
};
use layerx_bridge_relayer::rpc::{RpcFault, SendOutcome};
use layerx_bridge_relayer::signer::{
    Attestor, Submitter, ATTEST_INBOUND_DOMAIN, ETHEREUM_TRANSACTION_DOMAIN,
    PAXEER_TRANSACTION_DOMAIN,
};
use layerx_bridge_relayer::solana::observe::{
    observe_deposits, Finding, SolanaSettings, DISAGREEMENT,
};
use layerx_bridge_relayer::solana::rpc::{Commitment, Confirmation, SolanaRpc};
use layerx_bridge_relayer::solana::{base58_fixed, inbound_tx_hash, SOLANA_CHAIN_ID};
use layerx_crypto::evm_transaction::Eip1559Call;
use sha3::{Digest as _, Keccak256};
use support::{key, secret, work_directory, Recording, SignerServer};

const FIXTURE: &str = "solana_inbound.json";
const ATTESTOR: u8 = 0xa1;
const PAXEER_FEES: u8 = 0xb1;
const ETHEREUM_FEES: u8 = 0xb2;

const PROGRAM_ID: &str = "A7SZbByPYuHpunZ9pyMDMrhMYvK44ANT1AVqb8U1FpM9";
const SIGNATURE_A: &str =
    "4okVsRAC31AiwNv6HUY1upyAF5hqTn6VownC1j7CKXcau7KovRyFXnnEkpUibUm1E2LQU5zxKwrGp8XVCZEBegsB";
const SIGNATURE_B: &str =
    "31913H78zHzGo2idNn9Ym9kHXdJWJQwZo9aQWMvKujYX7eSsqSr8Qegm5PrhnxVXLjv6v7mgJoer6jhsFHd1FcAB";
const SIGNATURE_C: &str =
    "2KPXJi3cjmvBZtMz1P5B4aXzDFyaA1kGshjGCosSQcqveLqKiz9u3mqtbwohXMumzKPN1kPjbwmWtk3xj7M667bn";
const RECENT_BLOCKHASH: &str = "FrLFSoxDaT2FroN4k7ZE7uettrVRLzn3DQN8p6hBcjCT";

/// The pinned inbound vector of `bridge/vectors` and
/// `bridge/ATTESTATION-SOLANA.md`.
const VECTOR_TX_HASH: &str = "0x4219bc1e7d357618e0c662c494981e70d02920d8f7c3f850dd25712ef87e48d3";
const VECTOR_RECIPIENT: &str = "0x000000000000000000000000b65aa00b0baa8fe2abc2d188312fb0a6d00e3ed5";
const VECTOR_DIGEST: &str = "0x3333b122e2a4e61ad324d5c875a8a9c72cf98a2724da26c211235e30a8d294ef";
const VAULT_HANDLE: &str = "0x334121a65b47bd45c3f6381537d9180e98e445bc";
const SIDIORA_ASSET: &str = "0x21f7b20a555199fa73a238b1a91fd0f549068fee";

const PAXEER: PaxeerSettings = PaxeerSettings {
    chain_id: 229,
    finality_depth: 2,
    start_block: 25,
    max_block_range: 10,
    gas: GasPolicy {
        gas_limit: 1_000_000,
        max_fee_per_gas: 100_000_000_000,
        max_priority_fee_per_gas: 2_000_000_000,
    },
};

const CHAIN: ChainSettings = ChainSettings {
    chain_id: 1,
    vault: [0x11; 20],
    finality_depth: 12,
    start_block: 95,
    max_block_range: 10,
    gas: GasPolicy {
        gas_limit: 400_000,
        max_fee_per_gas: 200_000_000_000,
        max_priority_fee_per_gas: 3_000_000_000,
    },
};

fn fixed<const N: usize>(text: &str) -> [u8; N] {
    hex::fixed::<N>(text).unwrap_or_else(|error| panic!("{text}: {error}"))
}

fn base58<const N: usize>(text: &str) -> [u8; N] {
    base58_fixed::<N>(text).unwrap_or_else(|error| panic!("{text}: {error}"))
}

fn settings() -> SolanaSettings {
    SolanaSettings {
        chain_id: SOLANA_CHAIN_ID,
        vault: fixed(VAULT_HANDLE),
        program_id: base58(PROGRAM_ID),
        finality_depth: 32,
        start_slot: 1000,
        max_slot_range: 500,
        commitment: Commitment::Finalized,
    }
}

fn signer(name: &str) -> SignerServer {
    SignerServer::start(
        name,
        vec![
            ("attestor-1", key(ATTESTOR), vec![ATTEST_INBOUND_DOMAIN]),
            (
                "paxeer-fees-1",
                key(PAXEER_FEES),
                vec![PAXEER_TRANSACTION_DOMAIN],
            ),
            (
                "ethereum-fees",
                key(ETHEREUM_FEES),
                vec![ETHEREUM_TRANSACTION_DOMAIN],
            ),
        ],
    )
}

fn parts(recording: &Recording, signer: &SignerServer, journal: &PathBuf) -> RelayerParts {
    RelayerParts {
        attestor: Attestor::new(signer.remote("attestor-1", &key(ATTESTOR)))
            .unwrap_or_else(|error| panic!("attestor: {error:?}")),
        paxeer: PaxeerLink {
            settings: PAXEER,
            rpc: recording.endpoint("paxeer"),
            submitter: Submitter::paxeer(signer.remote("paxeer-fees-1", &key(PAXEER_FEES)))
                .unwrap_or_else(|error| panic!("paxeer submitter: {error:?}")),
        },
        chains: vec![ChainLink {
            settings: CHAIN,
            rpc: recording.endpoint("ethereum-1"),
            submitter: Submitter::ethereum(signer.remote("ethereum-fees", &key(ETHEREUM_FEES)))
                .unwrap_or_else(|error| panic!("ethereum submitter: {error:?}")),
        }],
        journal: Journal::open(journal).unwrap_or_else(|error| panic!("journal: {error}")),
        cosign: None,
        max_submissions: 3,
    }
}

fn start(
    recording: &Recording,
    signer: &SignerServer,
    journal: &PathBuf,
    solana: SolanaSettings,
) -> Result<Relayer, RelayerError> {
    Relayer::new(RelayerAssembly {
        parts: parts(recording, signer, journal),
        solana: Some(SolanaLink {
            settings: solana,
            rpc: SolanaRpc::new(recording.endpoint("solana")),
        }),
    })
}

fn solana_rpc(recording: &Recording) -> SolanaRpc {
    SolanaRpc::new(recording.endpoint("solana"))
}

fn padded(address: u8) -> [u8; 32] {
    let mut word = [0_u8; 32];
    word[12..].fill(address);
    word
}

fn vector_deposit() -> InboundAttestation {
    InboundAttestation {
        chain_id: SOLANA_CHAIN_ID,
        vault: fixed(VAULT_HANDLE),
        tx_hash: fixed(VECTOR_TX_HASH),
        log_index: 7,
        recipient: fixed(VECTOR_RECIPIENT),
        asset: fixed(SIDIORA_ASSET),
        amount: uint256_from_u64(12_345_678),
    }
}

fn later_deposit() -> InboundAttestation {
    InboundAttestation {
        chain_id: SOLANA_CHAIN_ID,
        vault: fixed(VAULT_HANDLE),
        tx_hash: inbound_tx_hash(&base58(SIGNATURE_C)),
        log_index: 9,
        recipient: padded(0x77),
        asset: fixed(SIDIORA_ASSET),
        amount: uint256_from_u64(4_200_000),
    }
}

fn attest(digest: &[u8; 32]) -> [u8; 65] {
    let (signature, recovery) = key(ATTESTOR)
        .sign_prehash_recoverable(digest)
        .unwrap_or_else(|error| panic!("attest: {error}"));
    let mut recoverable = [0_u8; 65];
    recoverable[..64].copy_from_slice(&signature.to_bytes());
    recoverable[64] = recovery.to_byte();
    to_attestor_signature(recoverable).unwrap_or_else(|error| panic!("attest: {error}"))
}

/// Paxeer fees in the recording: priority 1 gwei, base 2 gwei, so
/// `max_fee = 2 * 2 + 1` gwei.
fn bridge_in(nonce: u64, attestation: &InboundAttestation) -> Vec<u8> {
    Eip1559Call {
        chain_id: PAXEER.chain_id,
        nonce,
        max_priority_fee_per_gas: 1_000_000_000,
        max_fee_per_gas: 5_000_000_000,
        gas_limit: PAXEER.gas.gas_limit,
        to: LAYERX_BRIDGE_PRECOMPILE,
        data: encode_bridge_in(attestation, &[attest(&attestation.digest())]),
    }
    .sign(&secret(PAXEER_FEES))
    .unwrap_or_else(|error| panic!("reference transaction: {error}"))
}

fn attestor_requests(signer: &SignerServer) -> usize {
    signer
        .requests()
        .iter()
        .filter(|(name, _, _)| name == "attestor-1")
        .count()
}

const CONFIG: &str = r#"{
    "journal_path": "journal.jsonl",
    "poll_interval_ms": 4000,
    "max_submissions": 3,
    "signer": {"endpoint": {"transport": "uds", "socket": "signer.sock"}, "timeout_ms": 2000},
    "attestor": {"handle": "bridge-attestor-1", "public_key": "0x02aa"},
    "paxeer": {
        "chain_id": 229,
        "endpoints": [{"url": "http://127.0.0.1:8545", "local_emulator": true, "request_timeout_ms": 3000}],
        "finality_depth": 2, "start_block": 0, "max_block_range": 500,
        "submitter": {"handle": "bridge-paxeer-fees", "public_key": "0x02bb"},
        "gas": {"gas_limit": 1000000, "max_fee_per_gas": 100000000000, "max_priority_fee_per_gas": 2000000000}
    },
    "chains": [{
        "chain_id": 1,
        "vault": "0x1111111111111111111111111111111111111111",
        "finality_depth": 64, "start_block": 0, "max_block_range": 1000,
        "rpc": {"endpoints": [], "quorum": 2, "connect_timeout_ms": 1000, "request_timeout_ms": 3000, "maximum_response_bytes": 1048576},
        "submitter": {"handle": "bridge-ethereum-fees", "public_key": "0x02cc"},
        "gas": {"gas_limit": 400000, "max_fee_per_gas": 200000000000, "max_priority_fee_per_gas": 3000000000}
    }],
    "solana": {
        "chain_id": 91600046870081,
        "vault": "0x334121a65b47bd45c3f6381537d9180e98e445bc",
        "finality_depth": 32, "start_slot": 1000, "max_slot_range": 500,
        "rpc": {"endpoints": [], "quorum": 2, "connect_timeout_ms": 1000, "request_timeout_ms": 3000, "maximum_response_bytes": 1048576},
        "fee_payer": {"handle": "bridge-solana-fees", "public_key": "0x3333333333333333333333333333333333333333333333333333333333333333"},
        "program_id": "A7SZbByPYuHpunZ9pyMDMrhMYvK44ANT1AVqb8U1FpM9",
        "commitment": "finalized"
    }
}"#;

fn parsed(text: &str) -> Result<RelayerConfig, String> {
    let config: RelayerConfig = serde_json::from_str(text).map_err(|error| error.to_string())?;
    config.validate().map_err(|error| error.to_string())?;
    Ok(config)
}

#[test]
fn the_configuration_accepts_a_solana_entry_and_refuses_each_inconsistency() {
    let config = parsed(CONFIG).unwrap_or_else(|error| panic!("solana config: {error}"));
    let solana = config
        .solana
        .as_ref()
        .unwrap_or_else(|| panic!("solana entry"));
    assert_eq!(
        solana
            .settings()
            .unwrap_or_else(|error| panic!("settings: {error}")),
        settings()
    );
    assert_eq!(solana.fee_payer.public_key, vec![0x33; 32]);

    let without: String = {
        let start = CONFIG
            .find(",\n    \"solana\"")
            .unwrap_or_else(|| panic!("solana entry in the example"));
        format!("{}\n}}", &CONFIG[..start])
    };
    let plain = parsed(&without).unwrap_or_else(|error| panic!("plain config: {error}"));
    assert_eq!(plain.solana, None);

    let refusals = [
        (
            r#""commitment": "finalized""#,
            r#""commitment": "finalized", "rpc_url": "x""#,
        ),
        (
            r#""handle": "bridge-solana-fees","#,
            r#""handle": "bridge-solana-fees", "private_key": "0x01","#,
        ),
        (r#""bridge-solana-fees""#, r#""bridge-attestor-1""#),
        (r#""chain_id": 91600046870081"#, r#""chain_id": 101"#),
        (r#""chain_id": 1,"#, r#""chain_id": 91600046870081,"#),
        (PROGRAM_ID, "A7SZbByPYuHpunZ9pyMDMrhMYvK44ANT1AVqb8U1FpM0"),
        (PROGRAM_ID, "11111111111111111111111111111111"),
        (
            r#""commitment": "finalized""#,
            r#""commitment": "processed""#,
        ),
        (
            r#""finality_depth": 32, "start_slot""#,
            r#""finality_depth": 0, "start_slot""#,
        ),
        (r#""max_slot_range": 500"#, r#""max_slot_range": 0"#),
        (
            "0x3333333333333333333333333333333333333333333333333333333333333333",
            "0x02dd",
        ),
        (VAULT_HANDLE, "0x0000000000000000000000000000000000000000"),
    ];
    for (from, to) in refusals {
        assert!(CONFIG.contains(from), "the example carries {from}");
        let text = CONFIG.replacen(from, to, 1);
        assert!(parsed(&text).is_err(), "{to} must be refused");
    }
}

#[test]
fn a_relayer_refuses_a_solana_entry_paxeer_has_not_registered_as_configured() {
    let recording = Recording::load(FIXTURE);
    let signer = signer("solana-registration");
    let directory = work_directory("solana-registration");
    let journal = directory.join("relayer.jsonl");
    let mut shallow = settings();
    shallow.finality_depth = 16;
    let mut other_vault = settings();
    other_vault.vault = [0x22; 20];
    let mut wrong_id = settings();
    wrong_id.chain_id = 1;
    let mut empty_range = settings();
    empty_range.max_slot_range = 0;
    for refused in [shallow, other_vault, wrong_id, empty_range] {
        assert!(matches!(
            start(&recording, &signer, &journal, refused),
            Err(RelayerError::Configuration(_))
        ));
    }
    assert!(start(&recording, &signer, &journal, settings()).is_ok());
}

#[test]
fn a_relayer_without_a_solana_entry_runs_the_ethereum_streams_only() {
    let recording = Recording::load(FIXTURE);
    let signer = signer("solana-absent");
    let directory = work_directory("solana-absent");
    let journal = directory.join("relayer.jsonl");
    let mut relayer = Relayer::new(parts(&recording, &signer, &journal))
        .unwrap_or_else(|error| panic!("relayer startup: {error}"));
    assert!(matches!(
        relayer.solana_step(),
        Err(RelayerError::Configuration(_))
    ));
    let streams: Vec<String> = relayer
        .tick()
        .into_iter()
        .map(|(stream, _)| stream)
        .collect();
    assert_eq!(streams, vec![inbound_stream(1), OUTBOUND_STREAM.to_owned()]);
    assert_eq!(recording.count("solana", "getSlot"), 0);
    assert!(relayer.journal().state().items.is_empty());
}

#[test]
fn the_solana_seam_returns_the_existing_typed_faults() {
    let recording = Recording::load(FIXTURE);
    let rpc = solana_rpc(&recording);
    recording.set_phase("rejected");
    assert_eq!(
        rpc.get_slot(Commitment::Finalized),
        Err(RpcFault::Rejected {
            code: -32005,
            message: "node is behind".to_owned(),
        })
    );
    recording.set_phase("unavailable");
    assert_eq!(
        rpc.get_slot(Commitment::Finalized),
        Err(RpcFault::Unavailable)
    );
    recording.set_phase("malformed");
    assert_eq!(
        rpc.get_slot(Commitment::Finalized),
        Err(RpcFault::Malformed)
    );
    recording.set_phase("scan");
    assert_eq!(rpc.get_slot(Commitment::Finalized), Ok(1100));
    assert_eq!(
        rpc.get_signatures_for_address(&base58(PROGRAM_ID), None, 0, Commitment::Finalized),
        Err(RpcFault::Configuration)
    );
    assert_eq!(
        rpc.get_signatures_for_address(&base58(PROGRAM_ID), None, 1001, Commitment::Finalized),
        Err(RpcFault::Configuration)
    );
    let page = rpc
        .get_signatures_for_address(&base58(PROGRAM_ID), None, 1000, Commitment::Finalized)
        .unwrap_or_else(|error| panic!("signatures: {error}"));
    let slots: Vec<(u64, bool)> = page.iter().map(|info| (info.slot, info.failed)).collect();
    assert_eq!(
        slots,
        vec![(1090, false), (1060, false), (1055, true), (1050, false)]
    );
    assert_eq!(page[3].signature, base58(SIGNATURE_A));

    recording.set_phase("seam");
    let blockhash = rpc
        .get_latest_blockhash(Commitment::Confirmed)
        .unwrap_or_else(|error| panic!("blockhash: {error}"));
    assert_eq!(blockhash.blockhash, base58(RECENT_BLOCKHASH));
    assert_eq!(blockhash.last_valid_block_height, 1250);
    let signature_a = base58::<64>(SIGNATURE_A);
    assert_eq!(
        rpc.send_transaction(b"PAXEERX_BRIDGE_FIXTURE_WIRE_ACCEPTED", &signature_a),
        Ok(SendOutcome::Accepted)
    );
    assert_eq!(
        rpc.send_transaction(b"PAXEERX_BRIDGE_FIXTURE_WIRE_OTHER", &signature_a),
        Err(RpcFault::Malformed)
    );
    assert!(matches!(
        rpc.send_transaction(b"PAXEERX_BRIDGE_FIXTURE_WIRE_REJECTED", &signature_a),
        Err(RpcFault::Rejected { code: -32002, .. })
    ));
    assert_eq!(
        rpc.send_transaction(b"PAXEERX_BRIDGE_FIXTURE_WIRE_UNKNOWN", &signature_a),
        Ok(SendOutcome::Unknown)
    );
    assert_eq!(
        rpc.send_transaction(&[], &signature_a),
        Err(RpcFault::Configuration)
    );
    let statuses = rpc
        .get_signature_statuses(&[signature_a, base58(SIGNATURE_B)])
        .unwrap_or_else(|error| panic!("statuses: {error}"));
    assert_eq!(statuses.len(), 2);
    let first = statuses[0].unwrap_or_else(|| panic!("status of the first signature"));
    assert_eq!(first.slot, 1050);
    assert!(!first.failed);
    assert_eq!(first.confirmation, Confirmation::Finalized);
    assert!(first.confirmation.reaches(Commitment::Finalized));
    assert_eq!(statuses[1], None);
    assert_eq!(
        rpc.get_signature_statuses(&[]),
        Err(RpcFault::Configuration)
    );
    assert_eq!(recording.unmatched(), Vec::<String>::new());
}

#[test]
fn a_recorded_deposit_attests_to_the_pinned_vector_and_a_disagreeing_one_is_refused() {
    let recording = Recording::load(FIXTURE);
    let rpc = solana_rpc(&recording);
    let findings = observe_deposits(&rpc, &settings(), 1000, 1068)
        .unwrap_or_else(|error| panic!("observe: {error}"));
    assert_eq!(findings.len(), 2);
    let Finding::Deposit(observation) = &findings[0] else {
        panic!(
            "the vector deposit is observed, not refused: {:?}",
            findings[0]
        );
    };
    let attestation = observation
        .inbound_attestation()
        .unwrap_or_else(|| panic!("an inbound observation"));
    assert_eq!(attestation, vector_deposit());
    assert_eq!(hex::prefixed(&attestation.digest()), VECTOR_DIGEST);
    assert_eq!(
        hex::prefixed(&inbound_tx_hash(&base58(SIGNATURE_A))),
        VECTOR_TX_HASH
    );
    assert_eq!(
        observation.key(),
        inbound_key(SOLANA_CHAIN_ID, &fixed(VECTOR_TX_HASH), 7)
    );

    let Finding::Refused {
        observation,
        reason,
    } = &findings[1]
    else {
        panic!("the disagreeing deposit is refused: {:?}", findings[1]);
    };
    assert_eq!(reason, DISAGREEMENT);
    assert_eq!(
        observation.key(),
        inbound_key(SOLANA_CHAIN_ID, &inbound_tx_hash(&base58(SIGNATURE_B)), 8)
    );
    // The failed transaction in slot 1055 is never fetched, and the deposit in
    // slot 1090 is above the window.
    assert_eq!(recording.count("solana", "getTransaction"), 2);
    assert_eq!(recording.unmatched(), Vec::<String>::new());
}

#[test]
fn deposits_above_the_slot_depth_are_not_observed_until_it_is_reached() {
    let recording = Recording::load(FIXTURE);
    let rpc = solana_rpc(&recording);
    let above = observe_deposits(&rpc, &settings(), 1069, 1089)
        .unwrap_or_else(|error| panic!("observe: {error}"));
    assert_eq!(above, Vec::new());
    assert_eq!(recording.count("solana", "getTransaction"), 0);
    let reached = observe_deposits(&rpc, &settings(), 1069, 1098)
        .unwrap_or_else(|error| panic!("observe: {error}"));
    assert_eq!(reached.len(), 1);
    let Finding::Deposit(observation) = &reached[0] else {
        panic!("the later deposit is observed: {:?}", reached[0]);
    };
    assert_eq!(observation.inbound_attestation(), Some(later_deposit()));
    assert_eq!(recording.count("solana", "getTransaction"), 1);
}

#[test]
fn an_unreadable_transaction_stops_the_scan_without_moving_the_cursor() {
    let recording = Recording::load(FIXTURE);
    let signer = signer("solana-unreadable");
    let directory = work_directory("solana-unreadable");
    let journal = directory.join("relayer.jsonl");
    let stream = inbound_stream(SOLANA_CHAIN_ID);
    let mut relayer = start(&recording, &signer, &journal, settings())
        .unwrap_or_else(|error| panic!("relayer startup: {error}"));

    recording.set_phase("missing");
    assert_eq!(
        relayer.solana_step(),
        Err(RelayerError::Rpc(RpcFault::Unavailable))
    );
    recording.set_phase("truncated");
    assert_eq!(
        relayer.solana_step(),
        Err(RelayerError::Rpc(RpcFault::Malformed))
    );
    assert_eq!(relayer.journal().state().cursors.get(&stream), None);
    assert!(relayer.journal().state().items.is_empty());
    assert_eq!(attestor_requests(&signer), 0);
    assert!(recording.sent().is_empty());
}

#[test]
fn solana_deposits_are_observed_and_submitted_as_bridge_in_exactly_once() {
    let recording = Recording::load(FIXTURE);
    let signer = signer("solana-cycle");
    let directory = work_directory("solana-cycle");
    let journal = directory.join("relayer.jsonl");
    let stream = inbound_stream(SOLANA_CHAIN_ID);
    let vector_key = inbound_key(SOLANA_CHAIN_ID, &fixed(VECTOR_TX_HASH), 7);
    let refused_key = inbound_key(SOLANA_CHAIN_ID, &inbound_tx_hash(&base58(SIGNATURE_B)), 8);
    let later_key = inbound_key(SOLANA_CHAIN_ID, &later_deposit().tx_hash, 9);
    assert_eq!(stream, format!("in:{SOLANA_CHAIN_ID}"));

    // Head 1100 at depth 32: slots up to 1068 are observed, the deposit in
    // slot 1090 is not yet.
    recording.set_phase("scan");
    let mut relayer = start(&recording, &signer, &journal, settings())
        .unwrap_or_else(|error| panic!("relayer startup: {error}"));
    let report = relayer
        .solana_step()
        .unwrap_or_else(|error| panic!("scan: {error}"));
    assert_eq!(
        report,
        StepReport {
            observed: 2,
            submitted: 1,
            completed: 0,
            waiting: 0,
            refused: 1,
        }
    );
    assert_eq!(recording.unmatched(), Vec::<String>::new());
    let sent = recording.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, "paxeer");
    assert_eq!(sent[0].1, bridge_in(5, &vector_deposit()));
    let state = relayer.journal().state();
    assert_eq!(state.cursors.get(&stream), Some(&1069));
    let vector_item = state
        .items
        .get(&vector_key)
        .unwrap_or_else(|| panic!("the vector deposit is journaled"));
    assert_eq!(
        vector_item.observation.inbound_attestation(),
        Some(vector_deposit())
    );
    assert_eq!(vector_item.signature, Some(attest(&fixed(VECTOR_DIGEST))));
    assert_eq!(
        state
            .items
            .get(&refused_key)
            .and_then(|item| item.refusal.clone()),
        Some(DISAGREEMENT.to_owned())
    );
    assert!(!state.items.contains_key(&later_key));
    assert_eq!(attestor_requests(&signer), 1);
    drop(relayer);

    // Restart on the same journal: nothing new is final, the vector deposit's
    // transaction is included and nothing is signed again.
    recording.set_phase("restart");
    let mut relayer = start(&recording, &signer, &journal, settings())
        .unwrap_or_else(|error| panic!("relayer restart: {error}"));
    let report = relayer
        .solana_step()
        .unwrap_or_else(|error| panic!("restart: {error}"));
    assert_eq!(
        report,
        StepReport {
            observed: 0,
            submitted: 0,
            completed: 1,
            waiting: 0,
            refused: 0,
        }
    );
    assert_eq!(recording.unmatched(), Vec::<String>::new());
    assert_eq!(recording.sent().len(), 1);
    assert_eq!(recording.count("solana", "getSignaturesForAddress"), 1);
    assert_eq!(
        relayer
            .journal()
            .state()
            .items
            .get(&vector_key)
            .and_then(|item| item.completion),
        Some(Completion::Included {
            tx_hash: Keccak256::digest(&sent[0].1).into(),
            block_number: 0x10,
        })
    );
    assert_eq!(attestor_requests(&signer), 1);
    drop(relayer);

    // Head 1130: the deposit in slot 1090 is now at depth and is submitted
    // with the next nonce; the refused and completed deposits stay as they are.
    recording.set_phase("later");
    let mut relayer = start(&recording, &signer, &journal, settings())
        .unwrap_or_else(|error| panic!("relayer later: {error}"));
    let results = relayer.tick();
    let solana = results
        .iter()
        .find(|(name, _)| *name == stream)
        .map(|(_, result)| result.clone())
        .unwrap_or_else(|| panic!("the tick runs the solana stream"));
    assert_eq!(
        solana,
        Ok(StepReport {
            observed: 1,
            submitted: 1,
            completed: 0,
            waiting: 0,
            refused: 0,
        })
    );
    let sent = recording.sent();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[1].1, bridge_in(6, &later_deposit()));
    assert_eq!(relayer.journal().state().cursors.get(&stream), Some(&1099));
    assert_eq!(attestor_requests(&signer), 2);
    assert_eq!(recording.count("solana", "getTransaction"), 3);

    // The journal on disk replays into the same state.
    let replayed = Journal::open(&journal).unwrap_or_else(|error| panic!("replay: {error}"));
    assert_eq!(replayed.state(), relayer.journal().state());
}

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU16, Ordering};
use std::thread;
use std::time::Duration;

use layerx_paxeer_client::{
    raw_call, ChainSignal, ClientConfigError, ConfirmationProgress, EndpointConfig, EndpointFault,
    EndpointSignal, EndpointTransport, ExecutionOutcome, FinalityReport, FinalityStage,
    FinalityTracker, Json, PaxeerClient, TrackerConfig, TrackerConfigError, TransactionHash,
    TransactionInclusion,
};
use layerx_types::intent::EvmAddress;

const FUNDED: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
const RECIPIENT: &str = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8";
const REVERTING_CREATION: &str = "0x6460006000fd6000526005601bf3";

static NEXT_PORT: AtomicU16 = AtomicU16::new(0);

fn next_port() -> u16 {
    let offset = NEXT_PORT.fetch_add(1, Ordering::Relaxed);
    let pid_lane = u16::try_from(std::process::id() % 9000).unwrap_or(0);
    20000_u16
        .saturating_add(pid_lane)
        .saturating_add(offset.saturating_mul(7))
}

fn anvil_binary() -> PathBuf {
    let foundry = PathBuf::from("/root/.foundry/bin/anvil");
    if foundry.exists() {
        foundry
    } else {
        PathBuf::from("anvil")
    }
}

struct Anvil {
    child: Child,
    endpoint: EndpointConfig,
}

impl Anvil {
    fn launch(extra: &[&str]) -> Self {
        for _ in 0..8 {
            let port = next_port();
            let endpoint = EndpointConfig {
                url: format!("http://127.0.0.1:{port}"),
                request_timeout: Duration::from_secs(5),
                transport: EndpointTransport::LocalEmulator,
                expected_chain_id: 31_337,
            };
            let child = Command::new(anvil_binary())
                .arg("--port")
                .arg(port.to_string())
                .arg("--silent")
                .args(extra)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap_or_else(|error| panic!("spawn anvil: {error}"));
            let mut anvil = Self { child, endpoint };
            if anvil.ready() {
                return anvil;
            }
            anvil.halt();
        }
        panic!("no free port for anvil");
    }

    fn ready(&self) -> bool {
        for _ in 0..100 {
            if raw_call(&self.endpoint, "eth_blockNumber", &[]).is_ok() {
                return true;
            }
            thread::sleep(Duration::from_millis(50));
        }
        false
    }

    fn call(&self, method: &str, params: &[Json]) -> Json {
        raw_call(&self.endpoint, method, params)
            .unwrap_or_else(|failure| panic!("{method}: {failure:?}"))
    }

    fn text_result(&self, method: &str, params: &[Json]) -> String {
        self.call(method, params)
            .as_text()
            .unwrap_or_else(|| panic!("{method}: expected text result"))
            .to_owned()
    }

    fn mine(&self, blocks: u32) {
        for _ in 0..blocks {
            self.call("evm_mine", &[]);
        }
    }

    fn send(&self, params: Json) -> TransactionHash {
        let hash = self.text_result("eth_sendTransaction", &[params]);
        TransactionHash::from_hex(&hash)
            .unwrap_or_else(|error| panic!("transaction hash: {error:?}"))
    }

    fn transfer(&self) -> TransactionHash {
        self.send(transfer_params())
    }

    fn halt(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Anvil {
    fn drop(&mut self) {
        self.halt();
    }
}

fn text_member(name: &str, value: &str) -> (String, Json) {
    (name.to_owned(), Json::Text(value.to_owned()))
}

fn transfer_params() -> Json {
    Json::Object(vec![
        text_member("from", FUNDED),
        text_member("to", RECIPIENT),
        text_member("value", "0x1"),
        text_member("nonce", "0x0"),
        text_member("gas", "0x5208"),
        text_member("maxFeePerGas", "0x77359400"),
        text_member("maxPriorityFeePerGas", "0x0"),
    ])
}

fn tracker_config(anvil: &Anvil, required: u64, delayed_after: u64) -> TrackerConfig {
    TrackerConfig {
        endpoints: vec![anvil.endpoint.clone()],
        minimum_endpoint_agreement: 1,
        required_confirmations: required,
        poll_cadence: Duration::from_millis(100),
        delayed_after_polls: delayed_after,
    }
}

fn tracker(
    anvil: &Anvil,
    required: u64,
    delayed_after: u64,
    transaction: TransactionHash,
) -> FinalityTracker {
    FinalityTracker::new(tracker_config(anvil, required, delayed_after), transaction)
        .unwrap_or_else(|error| panic!("tracker: {error:?}"))
}

fn client(anvil: &Anvil) -> PaxeerClient {
    PaxeerClient::new(vec![anvil.endpoint.clone()])
        .unwrap_or_else(|error| panic!("client: {error:?}"))
}

fn wait_included(tracked: &mut FinalityTracker) -> FinalityReport {
    for _ in 0..200 {
        let report = tracked.poll();
        if !matches!(
            report.stage(),
            FinalityStage::Pooled { .. } | FinalityStage::Missing { .. }
        ) {
            return report;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("transaction was not included in time")
}

fn wait_receipt(reader: &PaxeerClient, transaction: TransactionHash) -> TransactionInclusion {
    for _ in 0..200 {
        let receipt = reader
            .transaction_receipt(transaction)
            .unwrap_or_else(|error| panic!("receipt: {error:?}"));
        if let Some(inclusion) = receipt {
            return inclusion;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("transaction was not included in time")
}

fn progress(confirmed: u64, required: u64) -> ConfirmationProgress {
    ConfirmationProgress {
        confirmed,
        required,
    }
}

fn address_hex(address: EvmAddress) -> String {
    let digits: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::from("0x");
    for byte in address.bytes() {
        text.push(char::from(digits[usize::from(byte >> 4)]));
        text.push(char::from(digits[usize::from(byte & 0x0f)]));
    }
    text
}

#[test]
fn staged_progression_reaches_finality_with_confirmation_counts() {
    let anvil = Anvil::launch(&["--no-mining"]);

    let absent = TransactionHash::new([0xee; 32]);
    let mut absent_tracker = tracker(&anvil, 3, 100, absent);
    assert_eq!(absent_tracker.latest().stage(), FinalityStage::Announced);
    assert_eq!(absent_tracker.latest().progress(), progress(0, 3));
    assert_eq!(absent_tracker.latest().polls(), 0);
    let report = absent_tracker.poll();
    assert_eq!(report.stage(), FinalityStage::Missing { head: 0 });
    assert_eq!(report.signal(), ChainSignal::Progressing);
    assert_eq!(report.endpoint(), EndpointSignal::Serving);
    assert_eq!(report.progress(), progress(0, 3));
    assert_eq!(report.displacements(), 0);

    let transaction = anvil.transfer();
    let mut tracked = tracker(&anvil, 3, 100, transaction);
    let report = tracked.poll();
    assert_eq!(report.stage(), FinalityStage::Pooled { head: 0 });
    assert_eq!(report.progress(), progress(0, 3));

    anvil.mine(1);
    let report = tracked.poll();
    let FinalityStage::Confirming {
        inclusion,
        confirmations,
        required,
    } = report.stage()
    else {
        panic!("expected confirming, got {:?}", report.stage())
    };
    assert_eq!(confirmations, 1);
    assert_eq!(required, 3);
    assert_eq!(inclusion.block.number, 1);
    assert_eq!(inclusion.execution, ExecutionOutcome::Succeeded);
    assert_eq!(inclusion.deployed_contract, None);
    assert_eq!(report.signal(), ChainSignal::Progressing);
    assert_eq!(report.endpoint(), EndpointSignal::Serving);
    assert_eq!(report.progress(), progress(1, 3));

    anvil.mine(1);
    let report = tracked.poll();
    let FinalityStage::Confirming { confirmations, .. } = report.stage() else {
        panic!("expected confirming, got {:?}", report.stage())
    };
    assert_eq!(confirmations, 2);
    assert_eq!(report.progress(), progress(2, 3));

    anvil.mine(1);
    let report = tracked.poll();
    let FinalityStage::Final {
        inclusion,
        confirmations,
        required,
    } = report.stage()
    else {
        panic!("expected final, got {:?}", report.stage())
    };
    assert_eq!(confirmations, 3);
    assert_eq!(required, 3);
    assert_eq!(inclusion.block.number, 1);
    assert_eq!(report.progress(), progress(3, 3));

    anvil.mine(1);
    let report = tracked.poll();
    let FinalityStage::Final { confirmations, .. } = report.stage() else {
        panic!("expected final, got {:?}", report.stage())
    };
    assert_eq!(confirmations, 4);
    assert_eq!(report.signal(), ChainSignal::Progressing);
    assert_eq!(report.endpoint(), EndpointSignal::Serving);
    assert_eq!(report.progress(), progress(4, 3));
    assert_eq!(report.displacements(), 0);
    assert_eq!(report.polls(), 5);
}

#[test]
fn reorg_displacement_is_reported_distinctly() {
    let anvil = Anvil::launch(&[]);
    let transaction = anvil.transfer();
    let mut tracked = tracker(&anvil, 10, 100, transaction);

    let report = wait_included(&mut tracked);
    let FinalityStage::Confirming { inclusion, .. } = report.stage() else {
        panic!("expected confirming, got {:?}", report.stage())
    };
    assert_eq!(inclusion.block.number, 1);
    let original_hash = inclusion.block.hash;

    anvil.mine(3);
    let report = tracked.poll();
    let FinalityStage::Confirming { confirmations, .. } = report.stage() else {
        panic!("expected confirming, got {:?}", report.stage())
    };
    assert_eq!(confirmations, 4);

    anvil.call(
        "anvil_reorg",
        &[Json::Number("4".to_owned()), Json::Array(Vec::new())],
    );
    let report = tracked.poll();
    let FinalityStage::Displaced {
        lost,
        head,
        requeued,
    } = report.stage()
    else {
        panic!("expected displaced, got {:?}", report.stage())
    };
    assert_eq!(lost.block.number, 1);
    assert_eq!(lost.block.hash, original_hash);
    assert_eq!(head, 4);
    assert!(!requeued);
    assert_eq!(report.displacements(), 1);
    assert_eq!(report.signal(), ChainSignal::Progressing);
    assert_eq!(report.endpoint(), EndpointSignal::Serving);
    assert_eq!(report.progress(), progress(0, 10));

    let replaced = client(&anvil)
        .block_by_number(1)
        .unwrap_or_else(|error| panic!("block 1: {error:?}"))
        .unwrap_or_else(|| panic!("block 1 missing after reorg"));
    assert_ne!(replaced.hash, original_hash);

    let report = tracked.poll();
    assert!(matches!(
        report.stage(),
        FinalityStage::Displaced {
            requeued: false,
            ..
        }
    ));
    assert_eq!(report.displacements(), 1);
}

#[test]
fn displaced_transaction_requeues_and_reincludes_honestly() {
    let anvil = Anvil::launch(&[]);
    let snapshot = anvil.text_result("evm_snapshot", &[]);
    let transaction = anvil.transfer();
    let mut tracked = tracker(&anvil, 5, 100, transaction);

    let report = wait_included(&mut tracked);
    let FinalityStage::Confirming { confirmations, .. } = report.stage() else {
        panic!("expected confirming, got {:?}", report.stage())
    };
    assert_eq!(confirmations, 1);

    anvil.call("anvil_setAutomine", &[Json::Bool(false)]);
    assert_eq!(
        anvil.call("evm_revert", &[Json::Text(snapshot)]),
        Json::Bool(true)
    );
    let report = tracked.poll();
    assert!(matches!(
        report.stage(),
        FinalityStage::Displaced {
            requeued: false,
            ..
        }
    ));
    assert_eq!(report.displacements(), 1);

    let resent = anvil.transfer();
    assert_eq!(resent, transaction);
    let report = tracked.poll();
    let FinalityStage::Displaced {
        lost, requeued, ..
    } = report.stage()
    else {
        panic!("expected displaced, got {:?}", report.stage())
    };
    assert!(requeued);
    assert_eq!(lost.block.number, 1);
    assert_eq!(report.displacements(), 1);
    assert_eq!(report.progress(), progress(0, 5));

    anvil.mine(1);
    let report = tracked.poll();
    let FinalityStage::Confirming {
        inclusion,
        confirmations,
        ..
    } = report.stage()
    else {
        panic!("expected confirming, got {:?}", report.stage())
    };
    assert_eq!(confirmations, 1);
    assert_eq!(inclusion.block.number, 1);
    assert_eq!(report.displacements(), 1);
    assert_eq!(report.progress(), progress(1, 5));
}

#[test]
fn finality_stall_reads_delayed_not_unreachable() {
    let anvil = Anvil::launch(&["--no-mining"]);
    let transaction = anvil.transfer();
    let mut tracked = tracker(&anvil, 5, 3, transaction);
    anvil.mine(1);

    let report = tracked.poll();
    assert!(matches!(
        report.stage(),
        FinalityStage::Confirming {
            confirmations: 1,
            ..
        }
    ));
    assert_eq!(report.signal(), ChainSignal::Progressing);

    let mut report = tracked.poll();
    for _ in 0..2 {
        thread::sleep(tracked.poll_cadence());
        report = tracked.poll();
    }
    assert_eq!(
        report.signal(),
        ChainSignal::Delayed {
            stalled_polls: 3,
            threshold: 3,
            stalled_for: tracked.poll_cadence().saturating_mul(3),
            delayed_after: tracked.poll_cadence().saturating_mul(3),
        }
    );
    assert!(matches!(
        report.stage(),
        FinalityStage::Confirming {
            confirmations: 1,
            ..
        }
    ));
    assert!(!matches!(report.signal(), ChainSignal::Unreachable { .. }));
    assert_eq!(report.endpoint(), EndpointSignal::Serving);
    assert_eq!(report.progress(), progress(1, 5));

    anvil.mine(1);
    let report = tracked.poll();
    assert!(matches!(
        report.stage(),
        FinalityStage::Confirming {
            confirmations: 2,
            ..
        }
    ));
    assert_eq!(report.signal(), ChainSignal::Progressing);
}

#[test]
fn endpoint_loss_reads_unreachable_with_last_known_stage() {
    let mut anvil = Anvil::launch(&["--no-mining"]);
    let transaction = anvil.transfer();
    anvil.mine(1);
    let mut tracked = tracker(&anvil, 5, 50, transaction);

    let report = tracked.poll();
    assert!(matches!(
        report.stage(),
        FinalityStage::Confirming {
            confirmations: 1,
            ..
        }
    ));

    let url = anvil.endpoint.url.clone();
    anvil.halt();

    let report = tracked.poll();
    let ChainSignal::Unreachable { error } = report.signal() else {
        panic!("expected unreachable, got {:?}", report.signal())
    };
    let failure = error
        .failures
        .first()
        .unwrap_or_else(|| panic!("expected at least one endpoint failure"));
    assert_eq!(failure.url, url);
    assert!(matches!(failure.fault, EndpointFault::Connect { .. }));
    assert_eq!(report.endpoint(), EndpointSignal::Unreachable { error });
    assert!(matches!(
        report.stage(),
        FinalityStage::Confirming {
            confirmations: 1,
            ..
        }
    ));
    assert_eq!(report.progress(), progress(1, 5));
    assert_eq!(report.displacements(), 0);

    let report = tracked.poll();
    assert!(matches!(report.signal(), ChainSignal::Unreachable { .. }));
    assert!(matches!(report.endpoint(), EndpointSignal::Unreachable { .. }));
    assert!(matches!(report.stage(), FinalityStage::Confirming { .. }));
    assert_eq!(report.progress(), progress(1, 5));
}

#[test]
fn unreachable_endpoint_fails_over_to_healthy_one() {
    let anvil = Anvil::launch(&[]);
    let dead = EndpointConfig {
        url: format!("http://127.0.0.1:{}", next_port()),
        request_timeout: Duration::from_secs(2),
        transport: EndpointTransport::LocalEmulator,
        expected_chain_id: 31_337,
    };
    let dead_url = dead.url.clone();
    let transaction = anvil.transfer();
    let mut tracked = FinalityTracker::new(
        TrackerConfig {
            endpoints: vec![dead, anvil.endpoint.clone()],
            minimum_endpoint_agreement: 1,
            required_confirmations: 1,
            poll_cadence: Duration::from_millis(100),
            delayed_after_polls: 100,
        },
        transaction,
    )
    .unwrap_or_else(|error| panic!("tracker: {error:?}"));

    let report = wait_included(&mut tracked);
    assert_eq!(report.signal(), ChainSignal::Progressing);
    assert!(matches!(
        report.stage(),
        FinalityStage::Final {
            confirmations: 1,
            required: 1,
            ..
        }
    ));
    assert_eq!(report.progress(), progress(1, 1));
    let EndpointSignal::Degraded { failovers } = report.endpoint() else {
        panic!("expected degraded, got {:?}", report.endpoint())
    };
    assert!(!failovers.is_empty());
    assert!(failovers.iter().all(|failover| failover.url == dead_url
        && matches!(failover.fault, EndpointFault::Connect { .. })));
}

#[test]
fn pooled_transaction_reads_chain_delayed_while_endpoint_serves() {
    let anvil = Anvil::launch(&["--no-mining"]);
    let transaction = anvil.transfer();
    let mut tracked = tracker(&anvil, 3, 2, transaction);

    let report = tracked.poll();
    assert_eq!(report.stage(), FinalityStage::Pooled { head: 0 });
    assert_eq!(report.signal(), ChainSignal::Progressing);
    assert_eq!(report.endpoint(), EndpointSignal::Serving);
    assert_eq!(report.progress(), progress(0, 3));

    thread::sleep(tracked.poll_cadence());
    tracked.poll();
    thread::sleep(tracked.poll_cadence());
    let report = tracked.poll();
    assert_eq!(report.stage(), FinalityStage::Pooled { head: 0 });
    assert_eq!(
        report.signal(),
        ChainSignal::Delayed {
            stalled_polls: 2,
            threshold: 2,
            stalled_for: tracked.poll_cadence().saturating_mul(2),
            delayed_after: tracked.poll_cadence().saturating_mul(2),
        }
    );
    assert_eq!(report.endpoint(), EndpointSignal::Serving);
    assert_eq!(report.progress(), progress(0, 3));
}

#[test]
fn displaced_stage_survives_endpoint_loss_with_both_dimensions() {
    let mut anvil = Anvil::launch(&[]);
    let transaction = anvil.transfer();
    let mut tracked = tracker(&anvil, 10, 100, transaction);

    let report = wait_included(&mut tracked);
    let FinalityStage::Confirming { inclusion, .. } = report.stage() else {
        panic!("expected confirming, got {:?}", report.stage())
    };
    assert_eq!(inclusion.block.number, 1);

    anvil.mine(1);
    tracked.poll();
    anvil.call(
        "anvil_reorg",
        &[Json::Number("2".to_owned()), Json::Array(Vec::new())],
    );
    let report = tracked.poll();
    let FinalityStage::Displaced { lost, requeued, .. } = report.stage() else {
        panic!("expected displaced, got {:?}", report.stage())
    };
    assert!(!requeued);
    assert_eq!(lost.block.number, 1);
    assert_eq!(report.displacements(), 1);
    assert_eq!(report.endpoint(), EndpointSignal::Serving);
    assert_eq!(report.progress(), progress(0, 10));

    let url = anvil.endpoint.url.clone();
    anvil.halt();
    let report = tracked.poll();
    let FinalityStage::Displaced { lost, requeued, .. } = report.stage() else {
        panic!("expected displaced, got {:?}", report.stage())
    };
    assert!(!requeued);
    assert_eq!(lost.block.number, 1);
    let EndpointSignal::Unreachable { error } = report.endpoint() else {
        panic!("expected unreachable, got {:?}", report.endpoint())
    };
    let failure = error
        .failures
        .first()
        .unwrap_or_else(|| panic!("expected at least one endpoint failure"));
    assert_eq!(failure.url, url);
    assert!(matches!(failure.fault, EndpointFault::Connect { .. }));
    assert!(matches!(report.signal(), ChainSignal::Unreachable { .. }));
    assert_eq!(report.displacements(), 1);
    assert_eq!(report.progress(), progress(0, 10));
}

#[test]
fn reverted_custody_transaction_reports_execution_outcome() {
    let anvil = Anvil::launch(&[]);
    let deployment = anvil.send(Json::Object(vec![
        text_member("from", FUNDED),
        text_member("data", REVERTING_CREATION),
    ]));
    let inclusion = wait_receipt(&client(&anvil), deployment);
    assert_eq!(inclusion.execution, ExecutionOutcome::Succeeded);
    let contract = inclusion
        .deployed_contract
        .unwrap_or_else(|| panic!("deployment receipt without contract address"));

    let transaction = anvil.send(Json::Object(vec![
        text_member("from", FUNDED),
        text_member("to", &address_hex(contract)),
        text_member("value", "0x1"),
        text_member("gas", "0x186a0"),
    ]));
    let mut tracked = tracker(&anvil, 1, 100, transaction);
    let report = wait_included(&mut tracked);
    let FinalityStage::Final { inclusion, .. } = report.stage() else {
        panic!("expected final, got {:?}", report.stage())
    };
    assert_eq!(inclusion.execution, ExecutionOutcome::Reverted);
    assert_eq!(inclusion.deployed_contract, None);
}

#[test]
fn declared_configuration_is_validated() {
    let endpoint = EndpointConfig {
        url: "http://127.0.0.1:1".to_owned(),
        request_timeout: Duration::from_secs(1),
        transport: EndpointTransport::LocalEmulator,
        expected_chain_id: 31_337,
    };
    let transaction = TransactionHash::new([1; 32]);
    let base = TrackerConfig {
        endpoints: vec![endpoint.clone()],
        minimum_endpoint_agreement: 1,
        required_confirmations: 3,
        poll_cadence: Duration::from_millis(100),
        delayed_after_polls: 5,
    };

    let no_endpoints = TrackerConfig {
        endpoints: Vec::new(),
        ..base.clone()
    };
    assert!(matches!(
        FinalityTracker::new(no_endpoints, transaction).err(),
        Some(TrackerConfigError::Endpoints(ClientConfigError::NoEndpoints))
    ));

    let zero_required = TrackerConfig {
        required_confirmations: 0,
        ..base.clone()
    };
    assert!(matches!(
        FinalityTracker::new(zero_required, transaction).err(),
        Some(TrackerConfigError::ZeroRequiredConfirmations)
    ));

    let zero_cadence = TrackerConfig {
        poll_cadence: Duration::ZERO,
        ..base.clone()
    };
    assert!(matches!(
        FinalityTracker::new(zero_cadence, transaction).err(),
        Some(TrackerConfigError::ZeroPollCadence)
    ));

    let zero_delay = TrackerConfig {
        delayed_after_polls: 0,
        ..base.clone()
    };
    assert!(matches!(
        FinalityTracker::new(zero_delay, transaction).err(),
        Some(TrackerConfigError::ZeroDelayedAfterPolls)
    ));

    let unsupported_scheme = TrackerConfig {
        endpoints: vec![EndpointConfig {
            url: "ws://127.0.0.1:1".to_owned(),
            request_timeout: Duration::from_secs(1),
            transport: EndpointTransport::LocalEmulator,
            expected_chain_id: 31_337,
        }],
        ..base.clone()
    };
    assert!(matches!(
        FinalityTracker::new(unsupported_scheme, transaction).err(),
        Some(TrackerConfigError::Endpoints(
            ClientConfigError::InvalidEndpoint { .. }
        ))
    ));

    let zero_timeout = TrackerConfig {
        endpoints: vec![EndpointConfig {
            url: endpoint.url,
            request_timeout: Duration::ZERO,
            transport: EndpointTransport::LocalEmulator,
            expected_chain_id: 31_337,
        }],
        ..base
    };
    assert!(matches!(
        FinalityTracker::new(zero_timeout, transaction).err(),
        Some(TrackerConfigError::Endpoints(
            ClientConfigError::ZeroRequestTimeout { .. }
        ))
    ));
}

fn one_response(response: Vec<u8>) -> EndpointConfig {
    let listener = TcpListener::bind("127.0.0.1:0")
        .unwrap_or_else(|error| panic!("bind response fixture: {error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("response fixture address: {error}"));
    thread::spawn(move || {
        let (mut stream, _) = listener
            .accept()
            .unwrap_or_else(|error| panic!("response fixture accept: {error}"));
        let mut request = [0_u8; 8_192];
        let _ = stream.read(&mut request);
        stream
            .write_all(&response)
            .unwrap_or_else(|error| panic!("response fixture write: {error}"));
    });
    EndpointConfig {
        url: format!("http://{address}"),
        request_timeout: Duration::from_secs(1),
        transport: EndpointTransport::LocalEmulator,
        expected_chain_id: 31_337,
    }
}

#[test]
fn plaintext_requires_explicit_loopback_emulator_mode() {
    let public_plaintext = EndpointConfig {
        url: "http://rpc.paxeer.example".to_owned(),
        request_timeout: Duration::from_secs(1),
        transport: EndpointTransport::LocalEmulator,
        expected_chain_id: 1,
    };
    assert!(matches!(
        PaxeerClient::new(vec![public_plaintext]).err(),
        Some(ClientConfigError::InvalidEndpoint {
            fault: EndpointFault::InsecureTransport,
            ..
        })
    ));

    let production_plaintext = EndpointConfig {
        url: "http://127.0.0.1:8545".to_owned(),
        request_timeout: Duration::from_secs(1),
        transport: EndpointTransport::PinnedTls {
            trust_anchor_der: vec![1],
        },
        expected_chain_id: 1,
    };
    assert!(matches!(
        PaxeerClient::new(vec![production_plaintext]).err(),
        Some(ClientConfigError::InvalidEndpoint {
            fault: EndpointFault::InsecureTransport,
            ..
        })
    ));
}

#[test]
fn canonical_duplicate_endpoint_identities_are_refused() {
    let first = EndpointConfig {
        url: "http://LOCALHOST".to_owned(),
        request_timeout: Duration::from_secs(1),
        transport: EndpointTransport::LocalEmulator,
        expected_chain_id: 31_337,
    };
    let duplicate = EndpointConfig {
        url: "http://localhost:80/".to_owned(),
        ..first.clone()
    };

    assert!(matches!(
        PaxeerClient::new(vec![first, duplicate]).err(),
        Some(ClientConfigError::DuplicateEndpointIdentity { .. })
    ));
}

#[test]
fn response_bounds_and_ambiguous_framing_fail_closed() {
    let oversized = one_response(
        b"HTTP/1.1 200 OK\r\nContent-Length: 2097153\r\nConnection: close\r\n\r\n".to_vec(),
    );
    assert!(matches!(
        raw_call(&oversized, "eth_chainId", &[]).err().map(|failure| failure.fault),
        Some(EndpointFault::ResponseTooLarge)
    ));

    let body = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":\"0x7a69\"}";
    let mut response = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len(),
        body.len()
    )
    .into_bytes();
    response.extend_from_slice(body);
    let ambiguous = one_response(response);
    assert!(matches!(
        raw_call(&ambiguous, "eth_chainId", &[])
            .err()
            .map(|failure| failure.fault),
        Some(EndpointFault::AmbiguousFraming)
    ));

    let conflicting = one_response(
        b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n"
            .to_vec(),
    );
    assert!(matches!(
        raw_call(&conflicting, "eth_chainId", &[])
            .err()
            .map(|failure| failure.fault),
        Some(EndpointFault::AmbiguousFraming)
    ));

    let oversized_chunk = one_response(
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n200001\r\n"
            .to_vec(),
    );
    assert!(matches!(
        raw_call(&oversized_chunk, "eth_chainId", &[])
            .err()
            .map(|failure| failure.fault),
        Some(EndpointFault::ResponseTooLarge)
    ));
}

#[test]
fn every_read_is_bound_to_the_configured_chain() {
    let anvil = Anvil::launch(&[]);
    let mut wrong_chain = anvil.endpoint.clone();
    wrong_chain.expected_chain_id = 1;
    let client = PaxeerClient::new(vec![wrong_chain])
        .unwrap_or_else(|error| panic!("client configuration: {error:?}"));
    let error = client
        .head_number()
        .err()
        .unwrap_or_else(|| panic!("chain mismatch was accepted"));
    assert!(error.failures.iter().any(|failure| matches!(
        failure.fault,
        EndpointFault::ChainMismatch {
            expected: 1,
            actual: 31_337
        }
    )));
}

#[test]
fn finality_refuses_split_endpoint_observations() {
    let first = Anvil::launch(&[]);
    let second = Anvil::launch(&[]);
    let transaction = first.transfer();
    let mut tracker = FinalityTracker::new(
        TrackerConfig {
            endpoints: vec![first.endpoint.clone(), second.endpoint.clone()],
            minimum_endpoint_agreement: 2,
            required_confirmations: 1,
            poll_cadence: Duration::from_millis(10),
            delayed_after_polls: 10,
        },
        transaction,
    )
    .unwrap_or_else(|error| panic!("tracker: {error:?}"));
    let report = tracker.poll();
    let EndpointSignal::Unreachable { error } = report.endpoint() else {
        panic!("split observations were accepted: {:?}", report.endpoint())
    };
    assert!(error
        .failures
        .iter()
        .all(|failure| matches!(failure.fault, EndpointFault::InconsistentObservation)));
}

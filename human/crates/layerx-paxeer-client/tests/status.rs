use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU16, Ordering};
use std::thread;
use std::time::Duration;

use layerx_paxeer_client::{
    raw_call, BoundaryHealth, ChainStatus, ContractStatus, DelayExpectation, EndpointConfig,
    EndpointFault, EndpointStatus, EndpointTransport, FinalityStage, FinalityTracker, Json,
    TrackerConfig, TransactionHash,
};

const FUNDED: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
const RECIPIENT: &str = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8";
const REVERTING_CREATION: &str = "0x6460006000fd6000526005601bf3";

static NEXT_PORT: AtomicU16 = AtomicU16::new(0);

fn next_port() -> u16 {
    let offset = NEXT_PORT.fetch_add(1, Ordering::Relaxed);
    let pid_lane = u16::try_from(std::process::id() % 9000).unwrap_or(0);
    31000_u16
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

    fn send(&self, params: Json) -> TransactionHash {
        let hash = self.text_result("eth_sendTransaction", &[params]);
        TransactionHash::from_hex(&hash)
            .unwrap_or_else(|error| panic!("transaction hash: {error:?}"))
    }

    fn mine(&self) {
        self.call("evm_mine", &[]);
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

fn tracker(
    endpoints: Vec<EndpointConfig>,
    transaction: TransactionHash,
    required_confirmations: u64,
    delayed_after_polls: u64,
) -> FinalityTracker {
    FinalityTracker::new(
        TrackerConfig {
            endpoints,
            minimum_endpoint_agreement: 1,
            required_confirmations,
            poll_cadence: Duration::from_millis(25),
            delayed_after_polls,
        },
        transaction,
    )
    .unwrap_or_else(|error| panic!("tracker: {error:?}"))
}

fn transfer(anvil: &Anvil) -> TransactionHash {
    anvil.send(Json::Object(vec![
        text_member("from", FUNDED),
        text_member("to", RECIPIENT),
        text_member("value", "0x1"),
        text_member("nonce", "0x0"),
        text_member("gas", "0x5208"),
        text_member("maxFeePerGas", "0x77359400"),
        text_member("maxPriorityFeePerGas", "0x0"),
    ]))
}

fn wait_final(tracked: &mut FinalityTracker) {
    for _ in 0..200 {
        if matches!(tracked.poll().stage(), FinalityStage::Final { .. }) {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("transaction was not final in time");
}

#[test]
fn endpoint_failure_is_unavailable_without_becoming_a_delay() {
    let mut anvil = Anvil::launch(&[]);
    let transaction = transfer(&anvil);
    let mut tracked = tracker(vec![anvil.endpoint.clone()], transaction, 1, 2);
    wait_final(&mut tracked);
    anvil.halt();

    tracked.poll();
    let status = tracked.boundary_status();
    let EndpointStatus::Failed { error } = status.endpoint else {
        panic!("expected endpoint failure, got {:?}", status.endpoint)
    };
    assert!(error
        .failures
        .iter()
        .all(|failure| matches!(failure.fault, EndpointFault::Connect { .. })));
    assert_eq!(status.chain, ChainStatus::Progressing);
    assert_eq!(status.health, BoundaryHealth::Unavailable);
}

#[test]
fn a_stalled_pool_is_congestion_with_declared_timing() {
    let anvil = Anvil::launch(&["--no-mining"]);
    let transaction = transfer(&anvil);
    let mut tracked = tracker(vec![anvil.endpoint.clone()], transaction, 3, 2);

    tracked.poll();
    tracked.poll();
    tracked.poll();
    let status = tracked.boundary_status();
    assert_eq!(status.endpoint, EndpointStatus::Serving);
    assert_eq!(
        status.chain,
        ChainStatus::Congested {
            expectation: DelayExpectation {
                poll_cadence: Duration::from_millis(25),
                delayed_after: Duration::from_millis(50),
                stalled_for: Duration::from_millis(50),
                next_observation_within: Duration::from_millis(25),
            }
        }
    );
    assert_eq!(status.contract, ContractStatus::NotObserved);
    assert_eq!(status.health, BoundaryHealth::Degraded);
}

#[test]
fn a_stalled_confirmation_is_a_finality_delay_not_congestion() {
    let anvil = Anvil::launch(&["--no-mining"]);
    let transaction = transfer(&anvil);
    anvil.mine();
    let mut tracked = tracker(vec![anvil.endpoint.clone()], transaction, 4, 1);

    tracked.poll();
    tracked.poll();
    let status = tracked.boundary_status();
    assert!(matches!(status.stage, FinalityStage::Confirming { .. }));
    assert!(matches!(status.chain, ChainStatus::FinalityDelayed { .. }));
    assert_eq!(status.endpoint, EndpointStatus::Serving);
    assert_eq!(status.contract, ContractStatus::Accepted);
    assert_eq!(status.health, BoundaryHealth::Degraded);
}

#[test]
fn a_real_contract_revert_is_refused_while_the_boundary_stays_ready() {
    let anvil = Anvil::launch(&[]);
    let deployment = anvil.send(Json::Object(vec![
        text_member("from", FUNDED),
        text_member("data", REVERTING_CREATION),
    ]));
    let mut deployment_tracker = tracker(vec![anvil.endpoint.clone()], deployment, 1, 20);
    wait_final(&mut deployment_tracker);
    let FinalityStage::Final { inclusion, .. } = deployment_tracker.latest().stage() else {
        panic!("deployment was not final")
    };
    let contract = inclusion
        .deployed_contract
        .unwrap_or_else(|| panic!("deployment did not return a contract"));
    let address = format!("0x{}", hex(contract.bytes()));

    let transaction = anvil.send(Json::Object(vec![
        text_member("from", FUNDED),
        text_member("to", &address),
        text_member("value", "0x1"),
        text_member("gas", "0x186a0"),
    ]));
    let mut tracked = tracker(vec![anvil.endpoint.clone()], transaction, 1, 20);
    wait_final(&mut tracked);
    let status = tracked.boundary_status();
    assert_eq!(status.contract, ContractStatus::Refused);
    assert_eq!(status.endpoint, EndpointStatus::Serving);
    assert_eq!(status.chain, ChainStatus::Progressing);
    assert_eq!(status.health, BoundaryHealth::Ready);
}

#[test]
fn failover_degradation_remains_distinct_from_chain_delay() {
    let anvil = Anvil::launch(&[]);
    let dead = EndpointConfig {
        url: format!("http://127.0.0.1:{}", next_port()),
        request_timeout: Duration::from_millis(100),
        transport: EndpointTransport::LocalEmulator,
        expected_chain_id: 31_337,
    };
    let dead_url = dead.url.clone();
    let transaction = transfer(&anvil);
    let mut tracked = tracker(vec![dead, anvil.endpoint.clone()], transaction, 1, 20);
    wait_final(&mut tracked);

    let status = tracked.boundary_status();
    let EndpointStatus::Degraded { failovers } = status.endpoint else {
        panic!("expected failover degradation, got {:?}", status.endpoint)
    };
    assert!(!failovers.is_empty());
    assert!(failovers.iter().all(|failure| failure.url == dead_url));
    assert_eq!(status.chain, ChainStatus::Progressing);
    assert_eq!(status.health, BoundaryHealth::Degraded);
}

fn hex(bytes: [u8; 20]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::new();
    for byte in bytes {
        text.push(char::from(DIGITS[usize::from(byte >> 4)]));
        text.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    text
}

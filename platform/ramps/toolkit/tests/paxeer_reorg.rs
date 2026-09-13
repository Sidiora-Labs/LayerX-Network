use std::fs;
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use layerx_paxeer_client::{
    raw_call, EndpointConfig, EndpointTransport, FinalityStage, FinalityTracker, Json,
    TrackerConfig, TransactionHash,
};
use layerx_ramp_toolkit::journal::{Journal, PaxeerObservation};

struct Chain {
    child: Child,
    endpoint: EndpointConfig,
}

impl Chain {
    fn start() -> Self {
        let listener =
            TcpListener::bind("127.0.0.1:0").unwrap_or_else(|error| panic!("port: {error}"));
        let port = listener
            .local_addr()
            .unwrap_or_else(|error| panic!("address: {error}"))
            .port();
        drop(listener);
        let child = Command::new("anvil")
            .args(["--silent", "--port", &port.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap_or_else(|error| panic!("actual chain: {error}"));
        let chain = Self {
            child,
            endpoint: EndpointConfig {
                url: format!("http://127.0.0.1:{port}"),
                request_timeout: Duration::from_secs(5),
                transport: EndpointTransport::LocalEmulator,
                expected_chain_id: 31_337,
            },
        };
        for _ in 0..100 {
            if raw_call(&chain.endpoint, "eth_blockNumber", &[]).is_ok() {
                return chain;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("actual chain did not start");
    }

    fn call(&self, method: &str, params: &[Json]) -> Json {
        raw_call(&self.endpoint, method, params)
            .unwrap_or_else(|error| panic!("{method}: {error:?}"))
    }

    fn transfer(&self) -> TransactionHash {
        let fields = [
            ("from", "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"),
            ("to", "0x70997970C51812dc3A010C7d01b50e0d17dc79C8"),
            ("value", "0x1"),
            ("nonce", "0x0"),
            ("gas", "0x5208"),
            ("maxFeePerGas", "0x77359400"),
            ("maxPriorityFeePerGas", "0x0"),
        ];
        let value = self.call(
            "eth_sendTransaction",
            &[Json::Object(
                fields
                    .into_iter()
                    .map(|(key, value)| (key.to_owned(), Json::Text(value.to_owned())))
                    .collect(),
            )],
        );
        TransactionHash::from_hex(
            value
                .as_text()
                .unwrap_or_else(|| panic!("transaction hash")),
        )
        .unwrap_or_else(|error| panic!("transaction: {error:?}"))
    }
}

impl Drop for Chain {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn open(path: &std::path::Path) -> Journal {
    Journal::open(path).unwrap_or_else(|error| panic!("journal: {error:?}"))
}

#[test]
fn actual_reorg_preserves_lost_inclusion_and_restarts_on_new_inclusion() {
    let chain = Chain::start();
    let snapshot = chain.call("evm_snapshot", &[]);
    let transaction = chain.transfer();
    let mut tracker = FinalityTracker::new(
        TrackerConfig {
            endpoints: vec![chain.endpoint.clone()],
            minimum_endpoint_agreement: 1,
            required_confirmations: 5,
            poll_cadence: Duration::from_millis(100),
            delayed_after_polls: 100,
        },
        transaction,
    )
    .unwrap_or_else(|error| panic!("tracker: {error:?}"));
    let path = std::env::temp_dir().join(format!(
        "layerx-ramp-real-reorg-{}.jsonl",
        std::process::id()
    ));
    let mut journal = open(&path);
    journal
        .plan_paxeer([5; 32], [1; 32], 1, 1)
        .unwrap_or_else(|error| panic!("plan: {error:?}"));
    let included = tracker.poll();
    let FinalityStage::Confirming { inclusion, .. } = included.stage() else {
        panic!("actual inclusion: {:?}", included.stage());
    };
    journal
        .observe_paxeer(
            [5; 32],
            PaxeerObservation::from_finality("actual-transfer", &included),
            2,
        )
        .unwrap_or_else(|error| panic!("include: {error:?}"));
    chain.call("anvil_setAutomine", &[Json::Bool(false)]);
    assert_eq!(chain.call("evm_revert", &[snapshot]), Json::Bool(true));
    let displaced = tracker.poll();
    assert!(
        matches!(displaced.stage(), FinalityStage::Displaced { lost, .. } if lost == inclusion)
    );
    journal
        .observe_paxeer(
            [5; 32],
            PaxeerObservation::from_finality("actual-transfer", &displaced),
            3,
        )
        .unwrap_or_else(|error| panic!("displace: {error:?}"));
    drop(journal);
    let mut journal = open(&path);
    let retained = journal
        .paxeer(&[5; 32])
        .unwrap_or_else(|| panic!("retained displacement"));
    assert_eq!(retained.stage, "displaced");
    assert_eq!(retained.block_hash, Some(inclusion.block.hash));
    assert_eq!(retained.confirmations, 0);
    assert_eq!(chain.transfer(), transaction);
    chain.call(
        "evm_setNextBlockTimestamp",
        &[Json::Number("2000000000".to_owned())],
    );
    chain.call("evm_mine", &[]);
    let reincluded = tracker.poll();
    let FinalityStage::Confirming {
        inclusion: replacement,
        ..
    } = reincluded.stage()
    else {
        panic!("new inclusion: {:?}", reincluded.stage());
    };
    assert_ne!(replacement.block.hash, inclusion.block.hash);
    journal
        .observe_paxeer(
            [5; 32],
            PaxeerObservation::from_finality("actual-transfer", &reincluded),
            4,
        )
        .unwrap_or_else(|error| panic!("reinclude: {error:?}"));
    drop(journal);
    let journal = open(&path);
    let retained = journal
        .paxeer(&[5; 32])
        .unwrap_or_else(|| panic!("retained new inclusion"));
    assert_eq!(retained.stage, "confirming");
    assert_eq!(retained.block_hash, Some(replacement.block.hash));
    assert_eq!(retained.confirmations, 1);
    assert_eq!(retained.transaction_hash, Some(transaction.bytes()));
    drop(journal);
    assert!(fs::metadata(&path).is_ok());
}

//! Ordinary withdrawal against a real `LayerX` node and the native custody
//! precompile on a real Paxeer node.
//!
//! The debit half runs against an actual `layerxd` node driven through
//! `layerx-agentd`: the withdrawal activity is prepared, signed, submitted and
//! its receipt proven by the node itself. The Paxeer half runs against a real
//! disposable `paxd` started from genesis produced by
//! `platform/hosted/paxeer/custody-genesis.py` and
//! `platform/hosted/paxeer/anchor-genesis.py` and merged by
//! `platform/hosted/paxeer/init-chain.sh`, so `layerxCustody` (`0x…1013`) is the
//! native `layerxcustody` module and `layerxAnchor` (`0x…1014`) is the native
//! `layerxanchor` module. Nothing about the withdrawal is invented: the request
//! and finalise calldata is the byte-for-byte material the node's own receipt
//! inclusion produced, the roots the anchor reports final come from the
//! sequencer-signed batch header a genesis guarantor attested to, the nullifier
//! is the receipt's context hash and the claim identifier is derived with the
//! published custody helper.

use layerx_human_test_support as support;

#[path = "support/withdraw_native.rs"]
mod withdraw_native;

// ---------------------------------------------------------------------------
// The real Paxeer node behind both precompiles
// ---------------------------------------------------------------------------

mod paxd {
    use std::collections::BTreeMap;
    use std::io::{BufRead as _, BufReader, Write as _};
    use std::path::{Path, PathBuf};
    use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
    use std::sync::{Arc, Mutex, MutexGuard};
    use std::time::{Duration, Instant};

    use serde_json::{json, Value};

    use layerx_human_service::journeys::WithdrawalTransactionRequest;
    use layerx_intents::canonical::{decode_batch_header, decode_receipt};
    use layerx_paxeer_client::custody::{get_asset_calldata, withdrawal_claim_id};
    use layerx_paxeer_client::{
        raw_call, DebitExpectation, EndpointConfig, EndpointTransport, Json, TransactionHash,
        WithdrawalBoundary, WithdrawalConfig, WithdrawalMaterial, CUSTODY_PRECOMPILE,
        WEI_PER_BASE_UNIT,
    };
    use layerx_proof::merkle::Proof;
    use sha3::Digest as _;

    /// `hyperpax_125-1` is the only cosmos chain id `paxd` maps to an EVM chain
    /// id, so the disposable chain is always 125.
    pub(super) const CHAIN_ID: u64 = 125;
    const WORD: usize = 32;
    const REQUIRED_CONFIRMATIONS: u64 = 2;
    /// Base units the custody module holds for the withdrawing account.
    pub(super) const VAULT_BALANCE: u128 = 100;
    /// `layerxcustody` `withdrawal_delay_seconds` in the disposable genesis: the
    /// real queue-to-finalise window the journey waits out in wall-clock time.
    pub(super) const CHALLENGE_WINDOW: u64 = 5;
    /// The guarantor bond recorded in the anchor genesis, in bond units.
    const GUARANTOR_BOND: u64 = 1_000_000;

    fn lock<T>(value: &Mutex<T>) -> MutexGuard<'_, T> {
        value
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .unwrap_or_else(|| panic!("repository root absent"))
            .to_path_buf()
    }

    fn hex(bytes: &[u8]) -> String {
        let mut text = String::from("0x");
        for byte in bytes {
            use std::fmt::Write as _;
            let _ = write!(text, "{byte:02x}");
        }
        text
    }

    fn hex_bytes(text: &str) -> Vec<u8> {
        let digits = text
            .trim()
            .strip_prefix("0x")
            .unwrap_or_else(|| panic!("hex prefix absent in {text}"));
        assert_eq!(digits.len() % 2, 0, "odd hex length");
        digits
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                std::str::from_utf8(pair)
                    .ok()
                    .and_then(|value| u8::from_str_radix(value, 16).ok())
                    .unwrap_or_else(|| panic!("non-hex digit"))
            })
            .collect()
    }

    /// Everything the custody precompile needs about one settled `LayerX`
    /// withdrawal, derived only from the node's own proven receipt inclusion.
    #[derive(Clone, Debug)]
    pub(super) struct Settlement {
        pub(super) material: WithdrawalMaterial,
        pub(super) batch_number: u64,
        pub(super) header: Vec<u8>,
        pub(super) header_signature: [u8; 64],
        pub(super) nullifier: [u8; 32],
    }

    impl Settlement {
        pub(super) fn from_inclusion(
            receipt: Vec<u8>,
            proof: &Proof,
            header: Vec<u8>,
            header_signature: [u8; 64],
        ) -> Self {
            let batch_number = decode_batch_header(&header)
                .unwrap_or_else(|error| panic!("settled batch header: {error:?}"))
                .batch_number();
            let nullifier = {
                let decoded = decode_receipt(&receipt)
                    .unwrap_or_else(|error| panic!("withdrawal receipt: {error:?}"));
                let protocol = decoded
                    .protocol()
                    .unwrap_or_else(|| panic!("withdrawal protocol receipt absent"));
                assert!(
                    protocol.effects().len() > 1,
                    "withdrawal effect absent from the node's receipt"
                );
                protocol.context_hash()
            };
            let material = WithdrawalMaterial::from_inclusion(
                receipt,
                proof,
                header.clone(),
                header_signature,
            )
            .unwrap_or_else(|error| panic!("withdrawal material: {error:?}"));
            Self {
                material,
                batch_number,
                header,
                header_signature,
                nullifier,
            }
        }
    }

    /// The disposable `paxd` node, owned by one journey fixture.
    pub(super) struct PaxdNode {
        child: Mutex<Child>,
        input: Mutex<ChildStdin>,
        output: Mutex<BufReader<ChildStdout>>,
        endpoint: Mutex<Option<EndpointConfig>>,
        /// Sequencer-signed batch headers the node proved, by batch number.
        headers: Mutex<BTreeMap<u64, (Vec<u8>, [u8; 64])>>,
        finalized: Mutex<u64>,
        work: PathBuf,
    }

    impl PaxdNode {
        pub(super) fn launch(generated: &Value, beneficiary: [u8; 32]) -> Arc<Self> {
            let work = super::directory("paxd");
            std::fs::create_dir_all(&work).unwrap_or_else(|error| panic!("paxd work: {error}"));
            let script =
                Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/withdraw-paxd.py");
            let mut child = Command::new(
                std::env::var("LAYERX_TEST_PYTHON").unwrap_or_else(|_| "python3".to_owned()),
            )
            .arg(script)
            .current_dir(repo_root())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap_or_else(|error| panic!("spawn disposable paxd harness: {error}"));
            let input = child
                .stdin
                .take()
                .unwrap_or_else(|| panic!("paxd harness input"));
            let output = child
                .stdout
                .take()
                .unwrap_or_else(|| panic!("paxd harness output"));
            let node = Arc::new(Self {
                child: Mutex::new(child),
                input: Mutex::new(input),
                output: Mutex::new(BufReader::new(output)),
                endpoint: Mutex::new(None),
                headers: Mutex::new(BTreeMap::new()),
                finalized: Mutex::new(0),
                work,
            });
            let sequencer = generated["sequencer_public_key"]
                .as_str()
                .unwrap_or_else(|| panic!("fixture sequencer public key absent"))
                .to_owned();
            let started = node.call(json!({
                "command": "start",
                "work": node.work.to_string_lossy(),
                "network_id": super::NETWORK_ID,
                "asset": hex(&super::ASSET),
                "sequencer_public_key": sequencer,
                "withdrawal_delay_seconds": CHALLENGE_WINDOW,
                "guarantor_bond": GUARANTOR_BOND,
                "vault_base_units": VAULT_BALANCE,
                "beneficiary": hex(&beneficiary),
            }));
            let url = started["url"]
                .as_str()
                .unwrap_or_else(|| panic!("paxd endpoint absent"))
                .to_owned();
            *lock(&node.endpoint) = Some(EndpointConfig {
                url,
                request_timeout: Duration::from_secs(30),
                transport: EndpointTransport::LocalEmulator,
                expected_chain_id: CHAIN_ID,
            });
            node
        }

        fn call(&self, request: Value) -> Value {
            let mut input = lock(&self.input);
            writeln!(input, "{request}").unwrap_or_else(|error| panic!("paxd request: {error}"));
            input
                .flush()
                .unwrap_or_else(|error| panic!("paxd request flush: {error}"));
            drop(input);
            let mut line = String::new();
            let read = lock(&self.output)
                .read_line(&mut line)
                .unwrap_or_else(|error| panic!("paxd response: {error}"));
            assert!(read > 0, "disposable paxd harness closed its output");
            let answer: Value = serde_json::from_str(&line)
                .unwrap_or_else(|error| panic!("paxd response json: {error}"));
            assert!(
                answer["ok"].as_bool().unwrap_or_default(),
                "disposable paxd refused {}: {}",
                request["command"],
                answer["error"]
            );
            answer["result"].clone()
        }

        pub(super) fn endpoint(&self) -> EndpointConfig {
            lock(&self.endpoint)
                .clone()
                .unwrap_or_else(|| panic!("disposable paxd is not started"))
        }

        fn rpc(&self, method: &str, params: &[Json]) -> Json {
            raw_call(&self.endpoint(), method, params)
                .unwrap_or_else(|failure| panic!("{method} on the disposable paxd: {failure:?}"))
        }

        fn quantity(&self, method: &str, params: &[Json]) -> u128 {
            let answer = self.rpc(method, params);
            let text = answer
                .as_text()
                .unwrap_or_else(|| panic!("{method}: expected a quantity"));
            let digits = text
                .strip_prefix("0x")
                .unwrap_or_else(|| panic!("{method}: expected a hex quantity"));
            u128::from_str_radix(digits, 16)
                .unwrap_or_else(|error| panic!("{method} quantity: {error}"))
        }

        pub(super) fn block_number(&self) -> u64 {
            u64::try_from(self.quantity("eth_blockNumber", &[])).unwrap_or_default()
        }

        pub(super) fn balance(&self, address: [u8; 20]) -> u128 {
            self.quantity(
                "eth_getBalance",
                &[Json::Text(hex(&address)), Json::Text("latest".to_owned())],
            )
            .saturating_div(WEI_PER_BASE_UNIT)
        }

        pub(super) fn eth_call(&self, to: [u8; 20], data: &[u8]) -> Vec<u8> {
            let answer = self.rpc(
                "eth_call",
                &[
                    Json::Object(vec![
                        ("to".to_owned(), Json::Text(hex(&to))),
                        ("data".to_owned(), Json::Text(hex(data))),
                    ]),
                    Json::Text("latest".to_owned()),
                ],
            );
            hex_bytes(
                answer
                    .as_text()
                    .unwrap_or_else(|| panic!("eth_call: expected data")),
            )
        }

        /// Records one sequencer-signed header so the anchor can be walked
        /// forward to it: `layerxanchor` only finalizes contiguous batches.
        pub(super) fn register_header(&self, header: &[u8], signature: [u8; 64]) {
            let Ok(decoded) = decode_batch_header(header) else {
                return;
            };
            lock(&self.headers).insert(decoded.batch_number(), (header.to_vec(), signature));
        }

        /// Finalizes every recorded batch through `batch` on the anchor module.
        pub(super) fn finalize_through(&self, batch: u64) {
            let pending: Vec<(u64, Vec<u8>, [u8; 64])> = lock(&self.headers)
                .iter()
                .filter(|(number, _)| **number <= batch && **number > *lock(&self.finalized))
                .map(|(number, (header, signature))| (*number, header.clone(), *signature))
                .collect();
            for (number, header, signature) in pending {
                let result = self.call(json!({
                    "command": "checkpoint",
                    "header": hex(&header),
                    "header_signature": hex(&signature),
                }));
                assert!(
                    result["final"].as_bool().unwrap_or_default(),
                    "the anchor module did not finalize batch {number}: {result}"
                );
                *lock(&self.finalized) = number;
            }
            assert_eq!(
                *lock(&self.finalized),
                batch,
                "the anchor module has no finalized checkpoint for batch {batch}"
            );
        }

        pub(super) fn send(&self, calldata: &[u8]) -> TransactionHash {
            let result = self.call(json!({ "command": "send", "calldata": hex(calldata) }));
            let digest = result["transaction"]
                .as_str()
                .unwrap_or_else(|| panic!("custody transaction hash absent"));
            TransactionHash::from_hex(digest)
                .unwrap_or_else(|error| panic!("custody transaction hash: {error:?}"))
        }

        pub(super) fn cancel_claim(&self, claim_id: [u8; 32]) {
            let result = self.call(json!({ "command": "cancel", "claim_id": hex(&claim_id) }));
            assert_eq!(
                result["exit_code"].as_i64(),
                Some(0),
                "the custody authority could not cancel the claim: {result}"
            );
        }

        pub(super) fn wait_block(&self) {
            let head = self.block_number();
            let deadline = Instant::now() + Duration::from_secs(30);
            while self.block_number() <= head {
                assert!(
                    Instant::now() < deadline,
                    "disposable paxd stopped producing blocks"
                );
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }

    impl Drop for PaxdNode {
        fn drop(&mut self) {
            let _ = lock(&self.input).write_all(b"{\"command\":\"stop\"}\n");
            let mut child = lock(&self.child);
            let _ = child.kill();
            let _ = child.wait();
            if std::thread::panicking() {
                eprintln!("disposable paxd evidence: {}", self.work.display());
            } else {
                let _ = std::fs::remove_dir_all(&self.work);
            }
        }
    }

    /// The real settlement chain the native node fixture registers against.
    pub(super) struct PaxdChain {
        node: Arc<PaxdNode>,
        pub(super) configuration: serde_json::Value,
    }

    impl PaxdChain {
        pub(super) fn new(generated: &serde_json::Value, beneficiary: [u8; 32]) -> Self {
            let node = PaxdNode::launch(generated, beneficiary);
            let anchor = hex(&layerx_paxeer_client::ANCHOR_PRECOMPILE.bytes());
            let state_root = generated["receipt"]
                .as_str()
                .unwrap_or_else(|| panic!("genesis receipt digest absent"))
                .to_owned();
            let selector = sha3::Keccak256::digest(b"threshold()");
            let configuration = serde_json::json!({
                "url": node.endpoint().url,
                "chain_id": CHAIN_ID,
                "bond": anchor,
                "registry": anchor,
                "root_call": hex(selector.get(..4).unwrap_or_default()),
                "threshold": 1,
                "genesis_state_root": state_root,
            });
            Self {
                node,
                configuration,
            }
        }

        pub(super) fn node(&self) -> Arc<PaxdNode> {
            Arc::clone(&self.node)
        }
    }

    /// The Paxeer half of one withdrawal journey, on the real node.
    pub(super) struct JourneyChain {
        node: Arc<PaxdNode>,
        expectation: DebitExpectation,
        settlement: Mutex<Option<Settlement>>,
        boundary: WithdrawalBoundary,
    }

    impl JourneyChain {
        pub(super) fn new(node: Arc<PaxdNode>, expectation: DebitExpectation) -> Self {
            let boundary = WithdrawalBoundary::new_for_protocol(
                WithdrawalConfig {
                    endpoints: vec![node.endpoint()],
                    minimum_endpoint_agreement: 1,
                    required_confirmations: REQUIRED_CONFIRMATIONS,
                    poll_cadence: Duration::from_millis(200),
                    delayed_after_polls: 100,
                },
                3,
            )
            .unwrap_or_else(|error| panic!("withdrawal boundary: {error:?}"));
            Self {
                node,
                expectation,
                settlement: Mutex::new(None),
                boundary,
            }
        }

        pub(super) fn boundary(&self) -> &WithdrawalBoundary {
            &self.boundary
        }

        /// Walks the anchor module forward to the batch that carries the node's
        /// settled withdrawal, so its roots are the roots a real finalized
        /// checkpoint reports.
        pub(super) fn settle(&self, settlement: &Settlement) {
            self.node
                .register_header(&settlement.header, settlement.header_signature);
            self.node.finalize_through(settlement.batch_number);
            *lock(&self.settlement) = Some(settlement.clone());
        }

        pub(super) fn send(&self, request: &WithdrawalTransactionRequest) -> TransactionHash {
            assert_eq!(
                request.target, CUSTODY_PRECOMPILE,
                "withdrawal transactions target the custody precompile"
            );
            self.node.send(&request.calldata)
        }

        pub(super) fn mine(&self) {
            self.node.wait_block();
        }

        /// The custody module's payout delay is real chain time, so the journey
        /// waits it out instead of moving a simulated clock.
        pub(super) fn advance(&self, seconds: u64) {
            let deadline = Instant::now() + Duration::from_secs(seconds.saturating_add(60));
            let until = Instant::now() + Duration::from_secs(seconds);
            while Instant::now() < until {
                std::thread::sleep(Duration::from_millis(200));
            }
            self.node.wait_block();
            assert!(Instant::now() < deadline, "challenge window wait overran");
        }

        /// The custody authority's cancellation of a pending claim. It is a
        /// module message, so it moves claim and nullifier state with no EVM
        /// transaction and releases nothing.
        pub(super) fn cancel(&self) {
            let settlement = lock(&self.settlement)
                .clone()
                .unwrap_or_else(|| panic!("no settled withdrawal to cancel"));
            let claim_id =
                withdrawal_claim_id(CHAIN_ID, settlement.nullifier, self.expectation.recipient);
            self.node.cancel_claim(claim_id);
        }

        pub(super) fn recipient_balance(&self) -> u128 {
            self.node.balance(self.expectation.recipient.bytes())
        }

        /// The custody module holds deposits in its module account, not at the
        /// precompile address, so the held float is what `getAsset` reports as
        /// custodied: the module lowers it by every amount it pays out.
        pub(super) fn vault_balance(&self) -> u128 {
            let answer = self.node.eth_call(
                CUSTODY_PRECOMPILE.bytes(),
                &get_asset_calldata(self.expectation.asset_id),
            );
            let word = |index: usize| -> u128 {
                let start = WORD.saturating_mul(index).saturating_add(WORD);
                let bytes = answer
                    .get(start.saturating_add(16)..start.saturating_add(WORD))
                    .unwrap_or_else(|| panic!("getAsset tuple is short"));
                let mut value = [0_u8; 16];
                value.copy_from_slice(bytes);
                u128::from_be_bytes(value)
            };
            // head: asset_id, denom offset, pointer, enabled, paused,
            // minimum_deposit, custody_cap, custodied, released, pending
            word(7)
        }
    }
}

use std::collections::BTreeMap;
use std::fs;
use std::future::Future;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::pin::pin;
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Poll, Wake, Waker};
use std::time::{Duration, Instant};

use ed25519_dalek::SigningKey;
use layerx_agent_api::idempotency::IdempotentMutation;
use layerx_agent_api::identity::{AgentDid, AuthorityRef};
use layerx_agent_api::prepare::{PreparationRef, PrepareRequest as ApiPrepareRequest};
use layerx_agent_api::submit::SubmitRequest;
use layerx_agent_api::track::{
    EvidenceRef as AgentEvidenceRef, ReceiptRef, SubmissionRef, SubmissionState, TrackRequest,
    TrackedSubmission,
};
use layerx_agent_api::verify::Level;
use layerx_agentd::outbox::{Outbox, OutboxError, SubmissionState as OutboxState};
use layerx_agentd::prepare::{
    prepare_activity_for_protocol, PreparationDefaults, PrepareRequest, Prepared,
    ProductionCorePreparationBoundary,
};
use layerx_agentd::receipt::{self as daemon_receipt, ReceiptLookupKey as DaemonReceiptKey};
use layerx_agentd::sign::{attach_external_signature, verify_before_submit};
use layerx_agentd::store::{Store as AgentStore, TenantId};
use layerx_client::evidence::{ProofBundleSelector, VerifiedProofBundle};
use layerx_human_service::custody::{
    CustodySigner, EnvelopeKms, KeyClass, KeyEntropy, KeyId, Keystore, Operation, SigningLimits,
    StepUpEvidence,
};
use layerx_human_service::journeys::{
    AgentBoundary, AgentBoundaryError, AgentObservation, AgentPreparation, CancellationPolicy,
    PaxeerAction, PaxeerActionOutcome, ReceiptLookup, ReceiptMaterial, SettlementConfig,
    WithdrawalAgentPlan, WithdrawalBoundaryError, WithdrawalJourney, WithdrawalPlan,
    WithdrawalRuntime, WithdrawalStage, WithdrawalTransactionRequest,
};
use layerx_human_service::notify::JourneyId;
use layerx_human_service::store::{PrincipalId, PrincipalStore, TenancyDigest};
use layerx_human_service::trace::TraceId;
use layerx_paxeer_client::{
    CancelledFundsDisposition, DebitExpectation, PaxeerFundsDisposition, ProtocolDebitDisposition,
    TransactionHash, WithdrawalMaterial,
};
use layerx_sdk::{Call, Client as AgentClient};
use layerx_types::account::AccountId;
use layerx_types::activity::{Authority, TimestampBound};
use layerx_types::amount::Amount;
use layerx_types::ids::{AssetId, Did, IdempotencyKey};
use layerx_types::intent::{EvmAddress, NetworkId};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use sha2::{Digest as _, Sha256};

use paxd::{JourneyChain, PaxdNode, Settlement, VAULT_BALANCE};
use support::{directory, principal, retention_uniform, tenancy};

const NETWORK_ID: u32 = 77;
const ASSET: [u8; 32] = [
    0xb5, 0xa3, 0x2b, 0x12, 0x02, 0x9f, 0x8d, 0xdf, 0xb9, 0x05, 0xf9, 0x0f, 0x28, 0x0f, 0x66, 0x4b,
    0x46, 0x39, 0x0d, 0xe0, 0xfc, 0x62, 0x77, 0x0f, 0xc1, 0x97, 0xdd, 0x87, 0xb1, 0x8c, 0xd8, 0x98,
];
const AMOUNT: u128 = 25;
const RECIPIENT: [u8; 20] = [
    0x3c, 0x44, 0xcd, 0xdd, 0xb6, 0xa9, 0x00, 0xfa, 0x2b, 0x58, 0x5d, 0xd2, 0x99, 0xe0, 0x3d, 0x12,
    0xfa, 0x42, 0x93, 0xbc,
];

/// The settled withdrawal the node proves, shared between the real agent that
/// observes it and the runtime that publishes it to the custody precompile.
type SettledWithdrawal = Arc<Mutex<Option<Settlement>>>;

fn hold<T>(value: &Arc<Mutex<T>>) -> MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct NoopWake;

impl Wake for NoopWake {
    fn wake(self: Arc<Self>) {}
}

fn ready<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let waker = Waker::from(Arc::new(NoopWake));
    let mut context = Context::from_waker(&waker);
    match future.as_mut().poll(&mut context) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("withdrawal future unexpectedly blocked"),
    }
}

fn activity_type() -> ActivityType {
    ActivityType::new(ModuleId::Asset, 9).unwrap_or_else(|error| panic!("activity type: {error:?}"))
}

fn registry() -> ModuleRegistry {
    let registration = ModuleRegistration::new(ModuleId::Asset, &[activity_type()])
        .unwrap_or_else(|error| panic!("module registration: {error:?}"));
    ModuleRegistry::new(&[registration])
        .unwrap_or_else(|error| panic!("module registry: {error:?}"))
}

fn account(value: &str) -> AccountId {
    AccountId::parse(value).unwrap_or_else(|error| panic!("account: {error:?}"))
}

fn owner_public() -> [u8; 32] {
    SigningKey::from_bytes(&[0x11; 32])
        .verifying_key()
        .to_bytes()
}

fn owner_did() -> String {
    format!("did:layerx:{}", hex(&owner_public()))
}

fn owner_account() -> AccountId {
    account(&format!("agent:{}:main", owner_did()))
}

struct RealWithdrawalAgent {
    node: layerx_client::Client,
    store: AgentStore,
    outbox: Outbox,
    tenant: TenantId,
    registry: ModuleRegistry,
    preparations: BTreeMap<[u8; 32], Prepared>,
    observations: BTreeMap<[u8; 32], AgentObservation>,
    receipts: BTreeMap<[u8; 32], ReceiptMaterial>,
    submission_keys: BTreeMap<String, [u8; 32]>,
    effects: BTreeMap<[u8; 32], u32>,
    settled: SettledWithdrawal,
}

impl RealWithdrawalAgent {
    fn reconnect_before_submission(&mut self) -> Result<(), AgentBoundaryError> {
        self.node.reconnect().map_err(|error| {
            eprintln!("native withdrawal reconnect before first submit: {error:?}");
            AgentBoundaryError::Unavailable
        })
    }

    fn new(fixture: &Fixture, settled: SettledWithdrawal) -> Self {
        Self {
            node: withdraw_native::connect(&fixture.native.endpoint),
            store: AgentStore::open(&fixture.agent_root)
                .unwrap_or_else(|error| panic!("agent store: {error}")),
            outbox: Outbox::default(),
            tenant: TenantId::new("tenant-a").unwrap_or_else(|error| panic!("tenant: {error}")),
            registry: registry(),
            preparations: BTreeMap::new(),
            observations: BTreeMap::new(),
            receipts: BTreeMap::new(),
            submission_keys: BTreeMap::new(),
            effects: BTreeMap::new(),
            settled,
        }
    }

    fn tracked(
        key: [u8; 32],
        activity_id: [u8; 32],
        material: &ReceiptMaterial,
    ) -> AgentObservation {
        let digest: [u8; 32] = Sha256::digest(&material.canonical_bytes).into();
        AgentObservation {
            submission: TrackedSubmission {
                submission_ref: SubmissionRef::new(format!("sub-{}", hex(&key)))
                    .unwrap_or_else(|error| panic!("submission ref: {error:?}")),
                state: SubmissionState::Executed {
                    receipt_ref: ReceiptRef::new(format!("rcp-{}", hex(&key)))
                        .unwrap_or_else(|error| panic!("receipt ref: {error:?}")),
                },
                evidence: vec![AgentEvidenceRef {
                    kind: "sequencer-receipt".to_owned(),
                    digest,
                }],
                verification_level: Level::SequencerSigned,
                transitions: Vec::new(),
            },
            activity_id,
            receipt: Some(material.clone()),
        }
    }

    /// The node's own proven inclusion of the withdrawal receipt, in the exact
    /// shape `requestWithdrawal` and `finaliseWithdrawal` take.
    fn settle_withdrawal(&mut self, activity_id: [u8; 32]) -> Result<(), AgentBoundaryError> {
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut correlation = 20_000_u64;
        let bundle = loop {
            correlation = correlation.saturating_add(1);
            match self.node.proof_bundle(
                ProofBundleSelector::Receipt(activity_id),
                correlation,
                &self.registry,
            ) {
                Ok(bundle) => break bundle,
                Err(error) => {
                    if Instant::now() >= deadline {
                        eprintln!("native withdrawal material proof: {error:?}");
                        return Err(AgentBoundaryError::Unavailable);
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        };
        let VerifiedProofBundle::Receipt {
            canonical_bytes,
            proof,
            signed_header,
            ..
        } = bundle
        else {
            return Err(AgentBoundaryError::CorruptResponse);
        };
        *hold(&self.settled) = Some(Settlement::from_inclusion(
            canonical_bytes,
            &proof,
            signed_header.canonical_bytes,
            signed_header.signature,
        ));
        Ok(())
    }

    fn step_up(&self, now: u64) -> Option<StepUpEvidence> {
        self.preparations.values().next().map(|prepared| {
            let digest = prepared
                .disclosure
                .audit_digest()
                .unwrap_or_else(|error| panic!("withdrawal disclosure digest: {error}"));
            StepUpEvidence::new(
                "withdrawal-debit-stepup",
                Operation::Withdrawal,
                digest,
                now.saturating_sub(1),
                now.saturating_add(60),
            )
            .unwrap_or_else(|error| panic!("withdrawal step-up: {error}"))
        })
    }
}

impl AgentBoundary for RealWithdrawalAgent {
    fn prepare(
        &mut self,
        call: &Call<IdempotentMutation<ApiPrepareRequest>>,
    ) -> Result<AgentPreparation, AgentBoundaryError> {
        let request = &call.request().operation;
        let key = call.request().key.bytes();
        if !self.preparations.contains_key(&key) {
            let protocol_version = self.node.handshake().node().protocol_version;
            let mut core =
                ProductionCorePreparationBoundary::new(&mut self.node, 10).map_err(|error| {
                    eprintln!("native preparation initialization: {error:?}");
                    AgentBoundaryError::Unavailable
                })?;
            let prepared = prepare_activity_for_protocol(
                &mut core,
                PreparationDefaults {
                    timestamp_span: request
                        .timestamp_bound
                        .not_after
                        .get()
                        .saturating_sub(request.timestamp_bound.not_before.get()),
                    fee_limit: Amount::from_u128(request.fee_limit.get()),
                    maximum_payload_bytes: 1_024,
                },
                PrepareRequest {
                    actor: Did::new(request.actor.as_str().as_bytes())
                        .map_err(|_| AgentBoundaryError::CorruptResponse)?,
                    authority: Authority::owner(&owner_public())
                        .map_err(|_| AgentBoundaryError::CorruptResponse)?,
                    activity_type: activity_type(),
                    expected_account_sequence: Some(request.account_sequence.get()),
                    timestamp_bound: Some(
                        TimestampBound::new(
                            request.timestamp_bound.not_before.get(),
                            request.timestamp_bound.not_after.get(),
                        )
                        .map_err(|_| AgentBoundaryError::CorruptResponse)?,
                    ),
                    fee_limit: Some(Amount::from_u128(request.fee_limit.get())),
                    idempotency_key: IdempotencyKey::new(key),
                    payload: request.payload.as_bytes().to_vec(),
                    declared_payload_limit: 1_024,
                },
                protocol_version,
            )
            .map_err(|_| AgentBoundaryError::Refused)?;
            self.preparations.insert(key, prepared);
        }
        let prepared = self
            .preparations
            .get(&key)
            .cloned()
            .ok_or(AgentBoundaryError::CorruptResponse)?;
        Ok(AgentPreparation {
            preparation_ref: PreparationRef::new(format!("prep-{}", hex(&key)))
                .map_err(|_| AgentBoundaryError::CorruptResponse)?,
            unsigned_canonical_bytes: prepared.canonical_bytes.clone(),
            signing_preimage: prepared.signing_preimage.to_vec(),
            disclosure: prepared.disclosure.clone(),
            actor: request.actor.clone(),
            authority: request.authority.clone(),
            account_sequence: request.account_sequence.get(),
            not_before: request.timestamp_bound.not_before.get(),
            not_after: request.timestamp_bound.not_after.get(),
            fee_limit: request.fee_limit.get(),
            activity_type: prepared.envelope.activity_type(),
            payload: prepared.envelope.payload().as_bytes().to_vec(),
            payload_hash: prepared.envelope.payload_hash(),
            idempotency_key: prepared.envelope.idempotency_key().bytes(),
        })
    }

    fn submit(
        &mut self,
        call: &Call<IdempotentMutation<SubmitRequest>>,
        signer_public_key: [u8; 32],
    ) -> Result<AgentObservation, AgentBoundaryError> {
        let key = call.request().key.bytes();
        if let Some(observation) = self.observations.get(&key) {
            return Ok(observation.clone());
        }
        let prepared = self
            .preparations
            .get(&key)
            .cloned()
            .ok_or(AgentBoundaryError::CorruptResponse)?;
        let signature: [u8; 64] = call
            .request()
            .operation
            .signature
            .as_bytes()
            .try_into()
            .map_err(|_| AgentBoundaryError::Refused)?;
        let signed = attach_external_signature(&prepared, signature)
            .map_err(|_| AgentBoundaryError::Refused)?;
        let verified = verify_before_submit(&signed, &prepared, &signer_public_key, &self.registry)
            .map_err(|_| AgentBoundaryError::Refused)?;
        let activity_id = verified.activity_id();
        match self
            .outbox
            .enqueue(&mut self.store, self.tenant.clone(), key, verified)
        {
            Ok(()) => {}
            Err(OutboxError::Duplicate) => return Err(AgentBoundaryError::CorruptResponse),
            Err(_) => return Err(AgentBoundaryError::Refused),
        }
        self.outbox
            .transition(
                &mut self.store,
                key,
                OutboxState::Submitted,
                "real transport accepted withdrawal debit",
                None,
            )
            .map_err(|_| AgentBoundaryError::Refused)?;
        self.reconnect_before_submission()?;
        let submitted = self
            .node
            .submit_signed(&self.registry, signer_public_key, 20, 1, &signed)
            .map_err(|error| {
                eprintln!("native withdrawal submit: {error:?}");
                AgentBoundaryError::Unavailable
            })?;
        let layerx_client::submit::Submission::Acknowledged(ack) = submitted else {
            eprintln!("native withdrawal submission: {submitted:?}");
            return Err(AgentBoundaryError::Unavailable);
        };
        if ack.activity_id() != activity_id {
            return Err(AgentBoundaryError::CorruptResponse);
        }
        let (material, verified) = withdraw_native::receipt(
            &mut self.node,
            &self.registry,
            activity_id,
            prepared
                .envelope
                .account_sequence()
                .checked_add(1)
                .ok_or(AgentBoundaryError::CorruptResponse)?,
        );
        self.settle_withdrawal(activity_id)?;
        daemon_receipt::store(
            &mut self.store,
            self.tenant.clone(),
            key,
            &material.canonical_bytes,
            &material.authorised_batch,
        )
        .map_err(|_| AgentBoundaryError::CorruptResponse)?;
        self.outbox
            .transition(
                &mut self.store,
                key,
                OutboxState::Acknowledged,
                "real core acknowledged withdrawal debit",
                None,
            )
            .map_err(|_| AgentBoundaryError::Refused)?;
        self.outbox
            .transition(
                &mut self.store,
                key,
                OutboxState::Executed,
                "real sequencer receipt verified",
                Some(verified),
            )
            .map_err(|_| AgentBoundaryError::Refused)?;
        let observation = Self::tracked(key, activity_id, &material);
        self.receipts.insert(key, material);
        self.submission_keys.insert(
            observation.submission.submission_ref.as_str().to_owned(),
            key,
        );
        self.observations.insert(key, observation.clone());
        *self.effects.entry(key).or_default() += 1;
        Ok(observation)
    }

    fn track(&mut self, call: &Call<TrackRequest>) -> Result<AgentObservation, AgentBoundaryError> {
        let key = *self
            .submission_keys
            .get(call.request().submission_ref.as_str())
            .ok_or(AgentBoundaryError::CorruptResponse)?;
        self.observations
            .get(&key)
            .cloned()
            .ok_or(AgentBoundaryError::CorruptResponse)
    }

    fn receipt_by_idempotency_key(
        &mut self,
        idempotency_key: [u8; 32],
        expected_activity_id: [u8; 32],
    ) -> Result<ReceiptLookup, AgentBoundaryError> {
        let served = daemon_receipt::serve(
            &self.store,
            self.tenant.clone(),
            DaemonReceiptKey::Idempotency(idempotency_key),
        )
        .map_err(|_| AgentBoundaryError::Unavailable)?;
        if served.metadata.activity_id != expected_activity_id {
            return Err(AgentBoundaryError::CorruptResponse);
        }
        let material = self
            .receipts
            .get(&idempotency_key)
            .cloned()
            .ok_or(AgentBoundaryError::CorruptResponse)?;
        if served.canonical_bytes != material.canonical_bytes {
            return Err(AgentBoundaryError::CorruptResponse);
        }
        Ok(ReceiptLookup::Found(material))
    }
}

struct RealRuntime {
    node: Arc<PaxdNode>,
    chain: JourneyChain,
    settled: SettledWithdrawal,
    proof_available: bool,
    transactions: BTreeMap<[u8; 32], TransactionHash>,
    action_counts: BTreeMap<PaxeerAction, u32>,
    crash_after_broadcast: Option<PaxeerAction>,
}

impl RealRuntime {
    fn new(node: Arc<PaxdNode>, expectation: DebitExpectation, settled: SettledWithdrawal) -> Self {
        Self {
            chain: JourneyChain::new(Arc::clone(&node), expectation),
            node,
            settled,
            proof_available: false,
            transactions: BTreeMap::new(),
            action_counts: BTreeMap::new(),
            crash_after_broadcast: None,
        }
    }

    fn inject_crash_after_broadcast(&mut self, action: PaxeerAction) {
        self.crash_after_broadcast = Some(action);
    }
}

impl WithdrawalRuntime for RealRuntime {
    fn bind_debit(
        &mut self,
        _identity: &layerx_human_service::journeys::MovementExecutionIdentity,
        debit: &layerx_paxeer_client::CommittedWithdrawalDebit,
    ) -> Result<(), WithdrawalBoundaryError> {
        let expectation = debit.expectation();
        if expectation.activity_id != expectation.withdrawal_id {
            return Err(WithdrawalBoundaryError::ContractViolation);
        }
        self.chain = JourneyChain::new(Arc::clone(&self.node), expectation);
        Ok(())
    }

    fn verify_claim_signature(
        &mut self,
        _request: &WithdrawalTransactionRequest,
        _signature: &[u8],
    ) -> Result<Vec<u8>, WithdrawalBoundaryError> {
        Err(WithdrawalBoundaryError::ContractViolation)
    }

    fn withdrawal_material(
        &mut self,
        _debit: &DebitExpectation,
    ) -> Result<Option<WithdrawalMaterial>, WithdrawalBoundaryError> {
        if !self.proof_available {
            return Ok(None);
        }
        let settlement = hold(&self.settled)
            .clone()
            .ok_or(WithdrawalBoundaryError::Unavailable)?;
        self.chain.settle(&settlement);
        Ok(Some(settlement.material))
    }

    fn submit_or_resolve(
        &mut self,
        request: &WithdrawalTransactionRequest,
    ) -> Result<PaxeerActionOutcome, WithdrawalBoundaryError> {
        if let Some(transaction) = self.transactions.get(&request.action_key) {
            return Ok(PaxeerActionOutcome::Submitted(*transaction));
        }
        let transaction = self.chain.send(request);
        self.transactions.insert(request.action_key, transaction);
        *self.action_counts.entry(request.action).or_default() += 1;
        if self.crash_after_broadcast == Some(request.action) {
            self.crash_after_broadcast = None;
            panic!("injected process crash after real Paxeer broadcast");
        }
        Ok(PaxeerActionOutcome::Submitted(transaction))
    }

    fn lookup(
        &mut self,
        action_key: [u8; 32],
    ) -> Result<Option<TransactionHash>, WithdrawalBoundaryError> {
        Ok(self.transactions.get(&action_key).copied())
    }
}

struct Fixture {
    native: withdraw_native::NativeFixture,
    root: std::path::PathBuf,
    store_root: std::path::PathBuf,
    agent_root: std::path::PathBuf,
    tenancy_digest: TenancyDigest,
    principal: PrincipalId,
    signer: CustodySigner,
    agent_contract: AgentClient,
    trace: TraceId,
    plan: WithdrawalPlan,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let native = withdraw_native::NativeFixture::new();
        let root = directory(label);
        fs::create_dir_all(&root).unwrap_or_else(|error| panic!("fixture root: {error}"));
        let store_root = root.join("human-store");
        let secret_path = root.join("kms-mounted-root");
        fs::write(&secret_path, [0x42; 64]).unwrap_or_else(|error| panic!("KMS root: {error}"));
        let map = tenancy(&[("alice", "tenant-a")]);
        let tenancy_digest = map
            .install(&store_root)
            .unwrap_or_else(|error| panic!("tenancy: {error}"));
        let principal = principal("alice");
        let provider = EnvelopeKms::new("file-kms://human-primary", &secret_path)
            .unwrap_or_else(|error| panic!("KMS provider: {error}"));
        let keystore = Keystore::open_development(root.join("custody"), NETWORK_ID, provider)
            .unwrap_or_else(|error| panic!("keystore: {error}"));
        let key = KeyId::new("human-primary").unwrap_or_else(|error| panic!("key id: {error}"));
        keystore
            .generate(
                &principal,
                &key,
                KeyClass::HumanPrimary,
                KeyEntropy::new([0x11; 32], [0x52; 16], [0x53; 24])
                    .unwrap_or_else(|error| panic!("entropy: {error}")),
            )
            .unwrap_or_else(|error| panic!("generate key: {error}"));
        let signer_store = PrincipalStore::open(&store_root, retention_uniform(2), tenancy_digest)
            .unwrap_or_else(|error| panic!("signer store: {error}"));
        let signer = CustodySigner::new(
            keystore,
            signer_store,
            registry(),
            SigningLimits::new(1_000, 10_000).unwrap_or_else(|error| panic!("limits: {error}")),
        );
        let schema = layerx_agent_api::agent_api_schema_v1();
        let agent_contract = AgentClient::daemon("/run/layerx-agentd.sock", schema.version)
            .unwrap_or_else(|error| panic!("agent SDK: {error:?}"));
        let plan = WithdrawalPlan {
            request_anchor: layerx_types::ids::CheckpointId::new([18; 32]),
            layerx_protocol_version: layerx_intents::canonical::STATE_COMMITMENT_PROTOCOL_VERSION,
            journey_id: JourneyId::new(format!("jrn_{label}"))
                .unwrap_or_else(|error| panic!("journey id: {error}")),
            idempotency_key: [0x31; 32],
            network: NetworkId::new(NETWORK_ID)
                .unwrap_or_else(|error| panic!("network: {error:?}")),
            owner: owner_account(),
            withdrawals_account: account("system:paxeer-withdrawals"),
            payout_address: EvmAddress::new(RECIPIENT),
            asset: AssetId::new(ASSET),
            amount: Amount::from_u128(AMOUNT),
            currency: "LXP".to_owned(),
            settlement: SettlementConfig {
                checkpoint_interval_seconds: 600,
                paxeer_block_seconds: 12,
                required_confirmations: 2,
            },
            reminder_interval_seconds: 30,
            agent: WithdrawalAgentPlan {
                actor: AgentDid::new(owner_did())
                    .unwrap_or_else(|error| panic!("actor: {error:?}")),
                authority: AuthorityRef::new(hex(&owner_public()))
                    .unwrap_or_else(|error| panic!("authority: {error:?}")),
                account_sequence: native.account_sequence,
                not_before: native.timestamp,
                not_after: native.timestamp + 300_000,
                fee_limit: 7,
                custody_key: key,
            },
        };
        Self {
            native,
            store_root,
            agent_root: root.join("agent-store"),
            tenancy_digest,
            principal,
            signer,
            agent_contract,
            trace: TraceId::mint([0x44; 16]),
            plan,
            root,
        }
    }

    fn store(&self) -> PrincipalStore {
        PrincipalStore::open(&self.store_root, retention_uniform(2), self.tenancy_digest)
            .unwrap_or_else(|error| panic!("principal store: {error}"))
    }

    fn expectation(&self, activity_id: [u8; 32]) -> DebitExpectation {
        DebitExpectation {
            activity_id,
            network_id: NETWORK_ID,
            withdrawal_id: activity_id,
            account: layerx_paxeer_client::account_address_for_protocol(&self.plan.owner, 3)
                .unwrap_or_else(|error| panic!("owner account: {error:?}")),
            withdrawals_account: layerx_paxeer_client::account_address_for_protocol(
                &self.plan.withdrawals_account,
                3,
            )
            .unwrap_or_else(|error| panic!("withdrawal account: {error:?}")),
            asset_id: ASSET,
            amount: AMOUNT,
            recipient: EvmAddress::new(RECIPIENT),
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn reopen(fixture: &Fixture) -> (PrincipalStore, WithdrawalJourney) {
    let mut store = fixture.store();
    let mut scope = store
        .principal(&fixture.principal)
        .unwrap_or_else(|error| panic!("reopen scope: {error}"));
    let journey = WithdrawalJourney::load(&mut scope, &fixture.plan.journey_id)
        .unwrap_or_else(|error| panic!("load journey: {error}"))
        .unwrap_or_else(|| panic!("withdrawal missing"));
    drop(scope);
    (store, journey)
}

fn advance_once(
    fixture: &Fixture,
    runtime: &mut RealRuntime,
    agent: &mut RealWithdrawalAgent,
    mut store: PrincipalStore,
    mut journey: WithdrawalJourney,
    now: u64,
) -> (PrincipalStore, WithdrawalJourney) {
    let mut scope = store
        .principal(&fixture.principal)
        .unwrap_or_else(|error| panic!("advance scope: {error}"));
    let boundary = runtime.chain.boundary().clone();
    let step_up = agent.step_up(now);
    ready(journey.advance(
        &mut scope,
        runtime,
        &boundary,
        &fixture.agent_contract,
        agent,
        &fixture.signer,
        &registry(),
        &fixture.trace,
        step_up.as_ref(),
        now,
    ))
    .unwrap_or_else(|error| panic!("advance: {error}"));
    drop(scope);
    drop(store);
    reopen(fixture)
}

fn crash_after_real_broadcast(
    fixture: &Fixture,
    runtime: &mut RealRuntime,
    agent: &mut RealWithdrawalAgent,
    mut store: PrincipalStore,
    mut journey: WithdrawalJourney,
    now: u64,
) -> (PrincipalStore, WithdrawalJourney) {
    let mut scope = store
        .principal(&fixture.principal)
        .unwrap_or_else(|error| panic!("crash scope: {error}"));
    let boundary = runtime.chain.boundary().clone();
    let step_up = agent.step_up(now);
    let crashed = catch_unwind(AssertUnwindSafe(|| {
        let _ = ready(journey.advance(
            &mut scope,
            runtime,
            &boundary,
            &fixture.agent_contract,
            agent,
            &fixture.signer,
            &registry(),
            &fixture.trace,
            step_up.as_ref(),
            now,
        ));
    }));
    assert!(crashed.is_err());
    drop(scope);
    drop(store);
    reopen(fixture)
}

fn drive_to_settlement(
    fixture: &Fixture,
    runtime: &mut RealRuntime,
    agent: &mut RealWithdrawalAgent,
) -> (PrincipalStore, WithdrawalJourney, u64) {
    let mut store = fixture.store();
    let mut scope = store
        .principal(&fixture.principal)
        .unwrap_or_else(|error| panic!("start scope: {error}"));
    let journey = WithdrawalJourney::start(&mut scope, &fixture.plan, 100)
        .unwrap_or_else(|error| panic!("start: {error}"));
    assert_eq!(
        journey
            .status()
            .unwrap_or_else(|error| panic!("status: {error}"))
            .cancellation_policy(),
        CancellationPolicy::CannotCancelAfterCommitCompleteOnly
    );
    drop(scope);
    let mut journey = journey;
    for now in 101..120 {
        (store, journey) = advance_once(fixture, runtime, agent, store, journey, now);
        if matches!(
            journey
                .status()
                .unwrap_or_else(|error| panic!("status: {error}"))
                .stage(),
            WithdrawalStage::WaitingForSettlement { .. }
        ) {
            assert_eq!(agent.effects.values().sum::<u32>(), 1);
            let status = journey
                .status()
                .unwrap_or_else(|error| panic!("status: {error}"));
            assert!(status.withdrawal_id().is_some());
            assert_ne!(status.withdrawal_id(), Some([0x31; 32]));
            return (store, journey, now);
        }
    }
    panic!("withdrawal debit did not settle")
}

fn drive_claim_queued(
    fixture: &Fixture,
    runtime: &mut RealRuntime,
    agent: &mut RealWithdrawalAgent,
) -> (PrincipalStore, WithdrawalJourney, u64) {
    let (mut store, mut journey, mut now) = drive_to_settlement(fixture, runtime, agent);
    let expectation = match journey
        .status()
        .unwrap_or_else(|error| panic!("status: {error}"))
        .stage()
    {
        WithdrawalStage::WaitingForSettlement { expectation } => *expectation,
        stage => panic!("expected settlement, got {stage:?}"),
    };
    assert_eq!(expectation.expected_seconds, 624);
    runtime.proof_available = true;
    now += 1;
    (store, journey) = advance_once(fixture, runtime, agent, store, journey, now);
    assert!(matches!(
        journey
            .status()
            .unwrap_or_else(|error| panic!("status: {error}"))
            .stage(),
        WithdrawalStage::ReadyToClaim
    ));

    let mut scope = store
        .principal(&fixture.principal)
        .unwrap_or_else(|error| panic!("expiry scope: {error}"));
    let expiry = scope
        .expire(1_000_000)
        .unwrap_or_else(|error| panic!("expiry: {error}"));
    assert!(expiry.pinned_evidence_retained > 0);
    drop(scope);
    drop(store);
    (store, journey) = reopen(fixture);
    now = 1_000_001;
    (store, journey) = advance_once(fixture, runtime, agent, store, journey, now);
    assert_eq!(
        journey
            .status()
            .unwrap_or_else(|error| panic!("status: {error}"))
            .reminder_count(),
        1
    );
    let mut scope = store
        .principal(&fixture.principal)
        .unwrap_or_else(|error| panic!("claim scope: {error}"));
    journey
        .request_claim(&mut scope, now + 1)
        .unwrap_or_else(|error| panic!("request claim: {error}"));
    drop(scope);
    runtime.inject_crash_after_broadcast(PaxeerAction::QueueClaim);
    (store, journey) = crash_after_real_broadcast(fixture, runtime, agent, store, journey, now + 2);
    assert_eq!(
        runtime.action_counts.get(&PaxeerAction::QueueClaim),
        Some(&1)
    );
    for offset in 3..12 {
        runtime.chain.mine();
        (store, journey) = advance_once(fixture, runtime, agent, store, journey, now + offset);
        if matches!(
            journey
                .status()
                .unwrap_or_else(|error| panic!("status: {error}"))
                .stage(),
            WithdrawalStage::WaitingForChallengeWindow { .. }
        ) {
            return (store, journey, now + offset);
        }
    }
    panic!("claim did not queue")
}

#[test]
fn real_agentd_debit_and_anvil_claim_survive_ack_gaps_and_pay_exactly_once() {
    let fixture = Fixture::new("withdrawpayout");
    let settled: SettledWithdrawal = Arc::new(Mutex::new(None));
    let mut agent = RealWithdrawalAgent::new(&fixture, Arc::clone(&settled));
    let mut runtime = RealRuntime::new(
        fixture.native.paxd(),
        fixture.expectation([0x31; 32]),
        settled,
    );
    let (mut store, mut journey, mut now) = drive_claim_queued(&fixture, &mut runtime, &mut agent);
    let reminders = {
        let scope = store
            .principal(&fixture.principal)
            .unwrap_or_else(|error| panic!("reminder scope: {error}"));
        WithdrawalJourney::reminders(&scope, &fixture.plan.journey_id)
            .unwrap_or_else(|error| panic!("reminders: {error}"))
    };
    assert_eq!(reminders.len(), 1);
    runtime.chain.advance(paxd::CHALLENGE_WINDOW + 1);
    now += 1;
    (store, journey) = advance_once(&fixture, &mut runtime, &mut agent, store, journey, now);
    assert!(matches!(
        journey
            .status()
            .unwrap_or_else(|error| panic!("status: {error}"))
            .stage(),
        WithdrawalStage::ReadyToFinalise
    ));
    now += 1;
    (store, journey) = advance_once(&fixture, &mut runtime, &mut agent, store, journey, now);
    runtime.inject_crash_after_broadcast(PaxeerAction::FinalisePayout);
    now += 1;
    (store, journey) =
        crash_after_real_broadcast(&fixture, &mut runtime, &mut agent, store, journey, now);
    assert_eq!(
        runtime.action_counts.get(&PaxeerAction::FinalisePayout),
        Some(&1)
    );
    for _ in 0..8 {
        runtime.chain.mine();
        now += 1;
        (store, journey) = advance_once(&fixture, &mut runtime, &mut agent, store, journey, now);
        if matches!(
            journey
                .status()
                .unwrap_or_else(|error| panic!("status: {error}"))
                .stage(),
            WithdrawalStage::PaidOut(_)
        ) {
            break;
        }
    }
    assert!(matches!(
        journey
            .status()
            .unwrap_or_else(|error| panic!("status: {error}"))
            .stage(),
        WithdrawalStage::PaidOut(_)
    ));
    assert_eq!(runtime.chain.recipient_balance(), AMOUNT);
    assert_eq!(runtime.chain.vault_balance(), VAULT_BALANCE - AMOUNT);
    assert_eq!(agent.effects.values().sum::<u32>(), 1);
    now += 1;
    let _ = advance_once(&fixture, &mut runtime, &mut agent, store, journey, now);
    assert_eq!(
        runtime.action_counts.get(&PaxeerAction::QueueClaim),
        Some(&1)
    );
    assert_eq!(
        runtime.action_counts.get(&PaxeerAction::FinalisePayout),
        Some(&1)
    );
}

/// The custody authority cancels the queued claim inside its window. The
/// precompile's cancellation is a module message, so the journey's only honest
/// evidence is the agreed custody state: claim status 3 and a terminally
/// cancelled nullifier, with the funds still in custody and the `LayerX` debit
/// still committed.
#[test]
fn real_challenge_hold_and_cancellation_report_actual_funds_disposition() {
    let fixture = Fixture::new("withdrawcancel");
    let settled: SettledWithdrawal = Arc::new(Mutex::new(None));
    let mut agent = RealWithdrawalAgent::new(&fixture, Arc::clone(&settled));
    let mut runtime = RealRuntime::new(
        fixture.native.paxd(),
        fixture.expectation([0x31; 32]),
        settled,
    );
    let (mut store, mut journey, mut now) = drive_claim_queued(&fixture, &mut runtime, &mut agent);
    runtime.chain.cancel();
    let expected = CancelledFundsDisposition {
        paxeer: PaxeerFundsDisposition::RetainedInVault {
            vault: layerx_paxeer_client::CUSTODY_PRECOMPILE,
            asset_id: ASSET,
            amount: AMOUNT,
        },
        layerx: ProtocolDebitDisposition::RemainsCommittedPendingProtocolRecovery {
            debit_receipt_reference: journey
                .status()
                .unwrap_or_else(|error| panic!("status: {error}"))
                .debit_receipt_reference()
                .unwrap_or_else(|| panic!("debit receipt reference absent")),
        },
    };
    for _ in 0..10 {
        runtime.chain.mine();
        now += 1;
        (store, journey) = advance_once(&fixture, &mut runtime, &mut agent, store, journey, now);
        if matches!(
            journey
                .status()
                .unwrap_or_else(|error| panic!("status: {error}"))
                .stage(),
            WithdrawalStage::Cancelled(_)
        ) {
            break;
        }
    }
    assert!(matches!(
        journey
            .status()
            .unwrap_or_else(|error| panic!("status: {error}"))
            .stage(),
        WithdrawalStage::Cancelled(evidence) if evidence.disposition == expected
    ));
    assert_eq!(runtime.chain.recipient_balance(), 0);
    assert_eq!(runtime.chain.vault_balance(), VAULT_BALANCE);
    assert_eq!(
        runtime.action_counts.get(&PaxeerAction::QueueClaim),
        Some(&1)
    );
    assert_eq!(
        runtime.action_counts.get(&PaxeerAction::FinalisePayout),
        None
    );
    assert_eq!(agent.effects.values().sum::<u32>(), 1);
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(text, "{byte:02x}");
    }
    text
}

//! Ordinary withdrawal against a real `LayerX` node and the native custody
//! precompile.
//!
//! The debit half runs against an actual `layerxd` node driven through
//! `layerx-agentd`: the withdrawal activity is prepared, signed, submitted and
//! its receipt proven by the node itself. The Paxeer half runs against an
//! in-process JSON-RPC endpoint in the crate's established harness style that
//! answers the `layerxCustody` (`0x…1013`) and `layerxAnchor` (`0x…1014`)
//! precompiles. Nothing about the withdrawal is invented there: the request and
//! finalise calldata the chain accepts is the byte-for-byte material the node's
//! own receipt inclusion produced, the finalized roots it reports are the
//! signed batch header's own roots, the nullifier is the receipt's context hash
//! and the claim identifier is derived with the published custody helper.
//!
//! The node still settles against Solidity contracts, so `paxeer_real` keeps a
//! real Anvil chain with `GuarantorBond` and `CheckpointRegistry` for the node
//! fixture's genesis, exactly as `tests/daemon/withdraw-custody.py` does.

use layerx_human_test_support as support;

#[path = "support/withdraw_native.rs"]
mod withdraw_native;

// ---------------------------------------------------------------------------
// Solidity settlement genesis for the real node
// ---------------------------------------------------------------------------

mod paxeer_real {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::sync::atomic::{AtomicU16, Ordering};
    use std::sync::{Mutex, OnceLock};
    use std::thread;
    use std::time::Duration;

    use layerx_paxeer_client::{
        raw_call, EndpointConfig, EndpointTransport, ExecutionOutcome, Json, PaxeerClient,
        TransactionHash, TransactionInclusion,
    };
    use layerx_types::intent::EvmAddress;
    use sha3::{Digest as _, Keccak256};

    const FUNDED: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
    const CHALLENGER: &str = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8";
    const PROTOCOL_VERSION: u16 = 3;
    /// `Constants.USDL_TOKEN`; the bond refuses any other token address.
    const USDL_TOKEN: EvmAddress = EvmAddress::new([
        0x85, 0xfc, 0xd1, 0x37, 0x35, 0xf4, 0x30, 0x98, 0x33, 0xa5, 0x03, 0xee, 0x80, 0x4e, 0xa3,
        0x23, 0x95, 0x85, 0x14, 0x79,
    ]);
    /// `Constants.USDL_ASSET_ID` (`keccak256("USDL")`).
    const USDL_ASSET_ID: [u8; 32] = [
        0x70, 0xf5, 0xb6, 0x3a, 0x98, 0x55, 0xdd, 0x2b, 0xe2, 0xba, 0x94, 0x1c, 0x04, 0xa3, 0x3a,
        0x1f, 0x0e, 0xeb, 0x97, 0x50, 0xcc, 0xeb, 0x32, 0x4c, 0x22, 0x37, 0x64, 0xf0, 0xfd, 0xc5,
        0x01, 0xd8,
    ];

    static NEXT_PORT: AtomicU16 = AtomicU16::new(0);
    static BYTECODE: OnceLock<Mutex<BTreeMap<&'static str, String>>> = OnceLock::new();

    struct Anvil {
        child: Child,
        endpoint: EndpointConfig,
    }

    impl Anvil {
        fn launch() -> Self {
            for _ in 0..8 {
                let offset = NEXT_PORT.fetch_add(1, Ordering::Relaxed);
                let lane = u16::try_from(std::process::id() % 7_000).unwrap_or(0);
                let port = 24_000_u16
                    .saturating_add(lane)
                    .saturating_add(offset.saturating_mul(11));
                let endpoint = EndpointConfig {
                    url: format!("http://127.0.0.1:{port}"),
                    request_timeout: Duration::from_secs(10),
                    transport: EndpointTransport::LocalEmulator,
                    expected_chain_id: 31_337,
                };
                let child = Command::new(foundry_binary("anvil"))
                    .arg("--port")
                    .arg(port.to_string())
                    .arg("--chain-id")
                    .arg("31337")
                    .arg("--gas-limit")
                    .arg("100000000")
                    .arg("--silent")
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
            panic!("no free port for anvil")
        }

        fn ready(&self) -> bool {
            for _ in 0..200 {
                if raw_call(&self.endpoint, "eth_blockNumber", &[]).is_ok() {
                    return true;
                }
                thread::sleep(Duration::from_millis(25));
            }
            false
        }

        fn call(&self, method: &str, params: &[Json]) -> Json {
            raw_call(&self.endpoint, method, params)
                .unwrap_or_else(|failure| panic!("{method}: {failure:?}"))
        }

        fn send(&self, from: &str, to: Option<EvmAddress>, data: &[u8]) -> TransactionHash {
            let mut fields = vec![
                text_member("from", from),
                text_member("data", &bytes_hex(data)),
                text_member("gas", "0x3938700"),
            ];
            if let Some(address) = to {
                fields.push(text_member("to", &address_hex(address)));
            }
            let result = self.call("eth_sendTransaction", &[Json::Object(fields)]);
            let hash = result
                .as_text()
                .unwrap_or_else(|| panic!("eth_sendTransaction: expected hash"));
            TransactionHash::from_hex(hash)
                .unwrap_or_else(|error| panic!("transaction hash: {error:?}"))
        }

        fn deploy(&self, contract: &'static str, arguments: &[[u8; 32]]) -> EvmAddress {
            let mut creation = hex_bytes(&contract_bytecode(contract));
            for argument in arguments {
                creation.extend_from_slice(argument);
            }
            let transaction = self.send(FUNDED, None, &creation);
            let receipt = wait_receipt(self, transaction);
            assert_eq!(
                receipt.execution,
                ExecutionOutcome::Succeeded,
                "{contract} deployment reverted"
            );
            receipt
                .deployed_contract
                .unwrap_or_else(|| panic!("{contract}: no deployed address"))
        }

        fn install_code(&self, source: EvmAddress, target: EvmAddress) {
            let code = self.call(
                "eth_getCode",
                &[
                    Json::Text(address_hex(source)),
                    Json::Text("latest".to_owned()),
                ],
            );
            let code = code
                .as_text()
                .unwrap_or_else(|| panic!("eth_getCode: expected text"));
            let _ = self.call(
                "anvil_setCode",
                &[Json::Text(address_hex(target)), Json::Text(code.to_owned())],
            );
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

    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .unwrap_or_else(|| panic!("repository root absent"))
            .to_path_buf()
    }

    fn foundry_binary(name: &str) -> PathBuf {
        let binary = PathBuf::from(format!("/root/.foundry/bin/{name}"));
        if binary.exists() {
            binary
        } else {
            PathBuf::from(name)
        }
    }

    fn contract_bytecode(contract: &'static str) -> String {
        let cache = BYTECODE.get_or_init(|| Mutex::new(BTreeMap::new()));
        let mut cache = cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(bytecode) = cache.get(contract) {
            return bytecode.clone();
        }
        let output = Command::new(foundry_binary("forge"))
            .arg("inspect")
            .arg(contract)
            .arg("bytecode")
            .current_dir(repo_root())
            .output()
            .unwrap_or_else(|error| panic!("forge inspect {contract}: {error}"));
        assert!(
            output.status.success(),
            "forge inspect {contract}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let bytecode = String::from_utf8(output.stdout)
            .unwrap_or_else(|error| panic!("forge bytecode utf8: {error}"))
            .trim()
            .to_owned();
        assert!(bytecode.starts_with("0x"));
        cache.insert(contract, bytecode.clone());
        bytecode
    }

    fn wait_receipt(anvil: &Anvil, transaction: TransactionHash) -> TransactionInclusion {
        let client = PaxeerClient::new(vec![anvil.endpoint.clone()])
            .unwrap_or_else(|error| panic!("client: {error:?}"));
        for _ in 0..300 {
            if let Some(receipt) = client
                .transaction_receipt(transaction)
                .unwrap_or_else(|error| panic!("receipt: {error:?}"))
            {
                return receipt;
            }
            thread::sleep(Duration::from_millis(20));
        }
        panic!("transaction was not included")
    }

    fn text_member(name: &str, value: &str) -> (String, Json) {
        (name.to_owned(), Json::Text(value.to_owned()))
    }

    fn parse_address(text: &str) -> EvmAddress {
        let bytes = hex_bytes(text);
        EvmAddress::new(
            bytes
                .try_into()
                .unwrap_or_else(|bytes: Vec<u8>| panic!("address length {}", bytes.len())),
        )
    }

    fn address_hex(address: EvmAddress) -> String {
        bytes_hex(&address.bytes())
    }

    fn hex_bytes(text: &str) -> Vec<u8> {
        let digits = text
            .trim()
            .strip_prefix("0x")
            .unwrap_or_else(|| panic!("hex prefix absent"));
        assert_eq!(digits.len() % 2, 0);
        digits
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| (hex_nibble(pair[0]) << 4) | hex_nibble(pair[1]))
            .collect()
    }

    fn hex_nibble(byte: u8) -> u8 {
        match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' => byte - b'a' + 10,
            b'A'..=b'F' => byte - b'A' + 10,
            _ => panic!("non-hex digit"),
        }
    }

    fn bytes_hex(bytes: &[u8]) -> String {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        let mut text = String::from("0x");
        for byte in bytes {
            text.push(char::from(DIGITS[usize::from(byte >> 4)]));
            text.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
        }
        text
    }

    fn quantity_word(bytes: &[u8]) -> [u8; 32] {
        let mut word = [0_u8; 32];
        word[32_usize.saturating_sub(bytes.len())..].copy_from_slice(bytes);
        word
    }

    fn address_word(address: EvmAddress) -> [u8; 32] {
        let mut word = [0_u8; 32];
        word[12..].copy_from_slice(&address.bytes());
        word
    }

    /// Deploys exactly the settlement surface a `layerxd` node reads at
    /// bring-up: the bond it names as its settlement contract and the
    /// checkpoint registry whose genesis digests are its own.
    fn deploy_settlement(
        anvil: &Anvil,
        network_id: u32,
        genesis: [[u8; 32]; 3],
    ) -> (EvmAddress, EvmAddress) {
        let owner = parse_address(FUNDED);
        let challenger = parse_address(CHALLENGER);
        let token_template = anvil.deploy("IntegrationToken", &[address_word(owner)]);
        anvil.install_code(token_template, USDL_TOKEN);
        let asset_registry = anvil.deploy(
            "AssetRegistry",
            &[
                address_word(owner),
                address_word(challenger),
                [0x21; 32],
                quantity_word(&1_u128.to_be_bytes()),
            ],
        );
        let vault = anvil.deploy(
            "LayerXVault",
            &[
                address_word(asset_registry),
                address_word(owner),
                address_word(challenger),
                [0x22; 32],
                quantity_word(&1_u128.to_be_bytes()),
            ],
        );
        let bond = anvil.deploy(
            "GuarantorBond",
            &[
                address_word(owner),
                address_word(owner),
                address_word(USDL_TOKEN),
                address_word(vault),
                USDL_ASSET_ID,
                quantity_word(&PROTOCOL_VERSION.to_be_bytes()),
                quantity_word(&network_id.to_be_bytes()),
                quantity_word(&100_u32.to_be_bytes()),
                quantity_word(&86_400_u64.to_be_bytes()),
                [0x23; 32],
                quantity_word(&1_u128.to_be_bytes()),
            ],
        );
        let registry = anvil.deploy(
            "CheckpointRegistry",
            &[
                address_word(bond),
                quantity_word(&PROTOCOL_VERSION.to_be_bytes()),
                quantity_word(&network_id.to_be_bytes()),
                quantity_word(&1_u16.to_be_bytes()),
                quantity_word(&1_u16.to_be_bytes()),
                quantity_word(&3_600_u64.to_be_bytes()),
                quantity_word(&300_u64.to_be_bytes()),
                genesis[0],
                genesis[1],
                genesis[2],
                [0x24; 32],
                quantity_word(&1_u128.to_be_bytes()),
            ],
        );
        (bond, registry)
    }

    /// The real settlement chain the native node fixture registers against.
    pub(super) struct GenesisChain {
        _anvil: Anvil,
        pub(super) configuration: serde_json::Value,
    }

    impl GenesisChain {
        pub(super) fn new(generated: &serde_json::Value) -> Self {
            let digest = |name: &str| -> [u8; 32] {
                hex_bytes(
                    generated[name]
                        .as_str()
                        .unwrap_or_else(|| panic!("genesis {name}")),
                )
                .try_into()
                .unwrap_or_else(|_| panic!("genesis digest {name}"))
            };
            let genesis = [digest("manifest"), digest("state"), digest("receipt")];
            let anvil = Anvil::launch();
            let (bond, registry) = deploy_settlement(&anvil, super::NETWORK_ID, genesis);
            let selector = Keccak256::digest(b"latestFinalisedStateRoot()");
            let data = bytes_hex(&selector[..4]);
            let observed = anvil.call(
                "eth_call",
                &[
                    Json::Object(vec![
                        text_member("to", &address_hex(registry)),
                        text_member("data", &data),
                    ]),
                    Json::Text("latest".to_owned()),
                ],
            );
            assert_eq!(
                hex_bytes(observed.as_text().unwrap_or_else(|| panic!("genesis root"))),
                genesis[2]
            );
            let configuration = serde_json::json!({
                "url": anvil.endpoint.url,
                "chain_id": anvil.endpoint.expected_chain_id,
                "bond": address_hex(bond), "registry": address_hex(registry),
                "root_call": data,
            });
            Self {
                _anvil: anvil,
                configuration,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// In-process native custody chain
// ---------------------------------------------------------------------------

mod custody_chain {
    use std::collections::BTreeMap;
    use std::io::{Read as _, Write as _};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex, MutexGuard};
    use std::thread;
    use std::time::Duration;

    use serde_json::{json, Value};
    use sha2::{Digest as _, Sha256};

    use layerx_human_service::journeys::WithdrawalTransactionRequest;
    use layerx_intents::canonical::{decode_batch_header, decode_receipt};
    use layerx_paxeer_client::custody::{
        withdrawal_claim_id, CLAIM_FINALISED_TOPIC, CLAIM_QUEUED_TOPIC, CUSTODY_RELEASE_TOPIC,
        SELECTOR_GET_ASSET, SELECTOR_GET_CLAIM, SELECTOR_NATIVE_ASSET_ID,
        SELECTOR_NULLIFIER_STATUS,
    };
    use layerx_paxeer_client::{
        DebitExpectation, EndpointConfig, EndpointTransport, TransactionHash, WithdrawalBoundary,
        WithdrawalConfig, WithdrawalMaterial, ANCHOR_PRECOMPILE, CUSTODY_PRECOMPILE,
        WEI_PER_BASE_UNIT,
    };
    use layerx_proof::merkle::Proof;

    const CHAIN_ID: u64 = 31_337;
    const WORD: usize = 32;
    const REQUIRED_CONFIRMATIONS: u64 = 2;
    /// Base units the custody precompile holds for the withdrawing account.
    pub(super) const VAULT_BALANCE: u128 = 100;
    /// The precompile's queue-to-finalise delay, in chain seconds.
    const CHALLENGE_WINDOW: u64 = 3_600;
    const FIRST_HEAD: u64 = 8;
    const FIRST_TIMESTAMP: u64 = 1_700_000_000;
    const DENOM: &str = "ulxp";
    /// `finalizedStateRoot(uint64)` on the anchor precompile.
    const SELECTOR_FINALIZED_STATE_ROOT: [u8; 4] = [0x0f, 0x60, 0x7f, 0xe4];
    /// `finalizedReceiptRoot(uint64)` on the anchor precompile.
    const SELECTOR_FINALIZED_RECEIPT_ROOT: [u8; 4] = [0xe0, 0xa3, 0xcc, 0xaa];

    /// Everything the custody precompile needs about one settled `LayerX`
    /// withdrawal, derived only from the node's own proven receipt inclusion.
    #[derive(Clone, Debug)]
    pub(super) struct Settlement {
        pub(super) material: WithdrawalMaterial,
        batch_number: u64,
        state_root: [u8; 32],
        receipt_root: [u8; 32],
        anchor: [u8; 32],
        nullifier: [u8; 32],
    }

    impl Settlement {
        pub(super) fn from_inclusion(
            receipt: Vec<u8>,
            proof: &Proof,
            header: Vec<u8>,
            header_signature: [u8; 64],
        ) -> Self {
            let (batch_number, state_root, receipt_root) = {
                let decoded = decode_batch_header(&header)
                    .unwrap_or_else(|error| panic!("settled batch header: {error:?}"));
                (
                    decoded.batch_number(),
                    decoded.resulting_state_root(),
                    decoded.receipt_merkle_root(),
                )
            };
            let (anchor, nullifier) = {
                let decoded = decode_receipt(&receipt)
                    .unwrap_or_else(|error| panic!("withdrawal receipt: {error:?}"));
                let protocol = decoded
                    .protocol()
                    .unwrap_or_else(|| panic!("withdrawal protocol receipt absent"));
                let body = protocol
                    .effects()
                    .get(1)
                    .map(|effect| effect.body().to_vec())
                    .unwrap_or_else(|| panic!("withdrawal effect absent"));
                let anchor: [u8; 32] = body
                    .get(150..182)
                    .and_then(|slice| slice.try_into().ok())
                    .unwrap_or_else(|| panic!("withdrawal anchor absent"));
                (anchor, protocol.context_hash())
            };
            let material =
                WithdrawalMaterial::from_inclusion(receipt, proof, header, header_signature)
                    .unwrap_or_else(|error| panic!("withdrawal material: {error:?}"));
            Self {
                material,
                batch_number,
                state_root,
                receipt_root,
                anchor,
                nullifier,
            }
        }
    }

    #[derive(Clone, Debug)]
    struct Log {
        topics: Vec<[u8; 32]>,
        data: Vec<u8>,
    }

    #[derive(Clone, Debug)]
    struct Receipt {
        block: u64,
        status: u64,
        logs: Vec<Log>,
    }

    struct Chain {
        expectation: DebitExpectation,
        settlement: Option<Settlement>,
        head: u64,
        timestamp: u64,
        hashes: BTreeMap<u64, [u8; 32]>,
        /// `(status, available_at)` of the single stored claim.
        claim: Option<(u8, u64)>,
        nullifier_status: u8,
        /// Native balances in wei, exactly as `eth_getBalance` reports them.
        balances: BTreeMap<[u8; 20], u128>,
        receipts: BTreeMap<[u8; 32], Receipt>,
        transactions: BTreeMap<[u8; 32], Vec<u8>>,
        sequence: u64,
    }

    impl Chain {
        fn new(expectation: DebitExpectation) -> Arc<Mutex<Self>> {
            let mut hashes = BTreeMap::new();
            for number in 0..=FIRST_HEAD {
                hashes.insert(number, block_hash(number));
            }
            let mut balances = BTreeMap::new();
            balances.insert(
                CUSTODY_PRECOMPILE.bytes(),
                VAULT_BALANCE.saturating_mul(WEI_PER_BASE_UNIT),
            );
            balances.insert(expectation.recipient.bytes(), 0);
            Arc::new(Mutex::new(Self {
                expectation,
                settlement: None,
                head: FIRST_HEAD,
                timestamp: FIRST_TIMESTAMP,
                hashes,
                claim: None,
                nullifier_status: 0,
                balances,
                receipts: BTreeMap::new(),
                transactions: BTreeMap::new(),
                sequence: 0,
            }))
        }

        fn mine(&mut self) {
            self.head = self.head.saturating_add(1);
            self.hashes.insert(self.head, block_hash(self.head));
        }

        fn base_units(&self, address: [u8; 20]) -> u128 {
            self.balances
                .get(&address)
                .copied()
                .unwrap_or_default()
                .saturating_div(WEI_PER_BASE_UNIT)
        }

        fn settled(&self) -> Settlement {
            self.settlement
                .clone()
                .unwrap_or_else(|| panic!("custody chain has no settled withdrawal"))
        }

        fn claim_id(&self, settlement: &Settlement) -> [u8; 32] {
            withdrawal_claim_id(
                CHAIN_ID,
                settlement.nullifier,
                self.expectation.recipient,
            )
        }

        /// The custody authority's cancellation of a pending claim. It is a
        /// module message, so it moves claim and nullifier state with no EVM
        /// transaction and releases nothing.
        fn cancel(&mut self) {
            let Some((1, available_at)) = self.claim else {
                panic!("only a pending claim can be cancelled")
            };
            self.claim = Some((3, available_at));
            self.nullifier_status = 3;
        }

        /// Applies one user transaction exactly as the custody precompile
        /// would: `requestWithdrawal` reserves the nullifier and starts the
        /// challenge window, `finaliseWithdrawal` consumes it and releases
        /// custody once that window elapsed.
        fn submit(&mut self, calldata: &[u8]) -> TransactionHash {
            self.sequence = self.sequence.saturating_add(1);
            let mut bytes = [0_u8; 32];
            bytes[..8].copy_from_slice(&self.sequence.to_be_bytes());
            bytes[31] = 0x7c;
            let settlement = self.settled();
            let mut logs = Vec::new();
            let mut status = 1;
            if calldata == settlement.material.request_calldata() {
                if self.claim.is_none() && self.nullifier_status == 0 {
                    let available_at = self.timestamp.saturating_add(CHALLENGE_WINDOW);
                    self.claim = Some((1, available_at));
                    self.nullifier_status = 1;
                    logs.push(self.queued_log(&settlement, available_at));
                } else {
                    status = 0;
                }
            } else if calldata == settlement.material.finalise_calldata() {
                match self.claim {
                    Some((1, available_at)) if self.timestamp >= available_at => {
                        self.claim = Some((2, available_at));
                        self.nullifier_status = 2;
                        self.release();
                        logs.extend(self.release_logs(&settlement));
                    }
                    _ => status = 0,
                }
            } else {
                status = 0;
            }
            self.receipts.insert(
                bytes,
                Receipt {
                    block: self.head,
                    status,
                    logs,
                },
            );
            self.transactions.insert(bytes, calldata.to_vec());
            TransactionHash::new(bytes)
        }

        fn release(&mut self) {
            let wei = self.expectation.amount.saturating_mul(WEI_PER_BASE_UNIT);
            let vault = CUSTODY_PRECOMPILE.bytes();
            let held = self.balances.get(&vault).copied().unwrap_or_default();
            assert!(held >= wei, "custody balance below the released amount");
            self.balances.insert(vault, held.saturating_sub(wei));
            let recipient = self.expectation.recipient.bytes();
            let paid = self.balances.get(&recipient).copied().unwrap_or_default();
            self.balances.insert(recipient, paid.saturating_add(wei));
        }

        fn queued_log(&self, settlement: &Settlement, available_at: u64) -> Log {
            let mut data = self.expectation.asset_id.to_vec();
            data.extend_from_slice(&address_word(self.expectation.recipient.bytes()));
            data.extend_from_slice(&u128_word(self.expectation.amount));
            data.extend_from_slice(&u64_word(available_at));
            Log {
                topics: vec![
                    CLAIM_QUEUED_TOPIC,
                    self.claim_id(settlement),
                    settlement.nullifier,
                    settlement.anchor,
                ],
                data,
            }
        }

        fn release_logs(&self, settlement: &Settlement) -> Vec<Log> {
            let claim_id = self.claim_id(settlement);
            let mut data = u128_word(self.expectation.amount).to_vec();
            data.extend_from_slice(&address_word(CUSTODY_PRECOMPILE.bytes()));
            vec![
                Log {
                    topics: vec![CLAIM_FINALISED_TOPIC, claim_id, settlement.nullifier],
                    data: Vec::new(),
                },
                Log {
                    topics: vec![
                        CUSTODY_RELEASE_TOPIC,
                        claim_id,
                        self.expectation.asset_id,
                        address_word(self.expectation.recipient.bytes()),
                    ],
                    data,
                },
            ]
        }

        /// `ILayerXCustody.Claim` exactly as `getClaim(bytes32)` returns it.
        fn claim_tuple(&self) -> Vec<u8> {
            let (Some(settlement), Some((status, available_at))) =
                (self.settlement.as_ref(), self.claim)
            else {
                return tuple(&[[0_u8; 32]; 13], 7, "");
            };
            tuple(
                &[
                    self.claim_id(settlement),
                    u64_word(1),
                    u64_word(u64::from(status)),
                    settlement.nullifier,
                    self.expectation.withdrawal_id,
                    self.expectation.account,
                    self.expectation.asset_id,
                    [0_u8; 32],
                    address_word(self.expectation.recipient.bytes()),
                    u128_word(self.expectation.amount),
                    u64_word(settlement.batch_number),
                    settlement.anchor,
                    u64_word(available_at),
                ],
                7,
                DENOM,
            )
        }

        /// `ILayerXCustody.Asset` for the native asset: no ERC20 pointer.
        fn asset_tuple(&self) -> Vec<u8> {
            tuple(
                &[
                    self.expectation.asset_id,
                    [0_u8; 32],
                    [0_u8; 32],
                    u64_word(1),
                    u64_word(0),
                    [0_u8; 32],
                    [0_u8; 32],
                    u128_word(self.base_units(CUSTODY_PRECOMPILE.bytes())),
                    u128_word(
                        VAULT_BALANCE.saturating_sub(self.base_units(CUSTODY_PRECOMPILE.bytes())),
                    ),
                    [0_u8; 32],
                ],
                1,
                DENOM,
            )
        }

        fn view(&self, to: &[u8], data: &[u8]) -> Vec<u8> {
            let selector = data.get(..4).unwrap_or_default();
            if to == CUSTODY_PRECOMPILE.bytes().as_slice() {
                if selector == SELECTOR_NULLIFIER_STATUS {
                    return u64_word(u64::from(self.nullifier_status)).to_vec();
                }
                if selector == SELECTOR_GET_CLAIM {
                    return self.claim_tuple();
                }
                if selector == SELECTOR_GET_ASSET {
                    return self.asset_tuple();
                }
                if selector == SELECTOR_NATIVE_ASSET_ID {
                    return self.expectation.asset_id.to_vec();
                }
            } else if to == ANCHOR_PRECOMPILE.bytes().as_slice() {
                if let Some(settlement) = self.settlement.as_ref() {
                    let mut requested = [0_u8; 8];
                    requested.copy_from_slice(
                        data.get(4_usize.saturating_add(24)..4_usize.saturating_add(WORD))
                            .unwrap_or(&[0; 8]),
                    );
                    let matched = u64::from_be_bytes(requested) == settlement.batch_number;
                    if selector == SELECTOR_FINALIZED_STATE_ROOT {
                        return two_words(settlement.state_root, matched);
                    }
                    if selector == SELECTOR_FINALIZED_RECEIPT_ROOT {
                        return two_words(settlement.receipt_root, matched);
                    }
                }
            }
            Vec::new()
        }
    }

    /// The Paxeer half of one withdrawal journey.
    pub(super) struct JourneyChain {
        chain: Arc<Mutex<Chain>>,
        boundary: WithdrawalBoundary,
    }

    impl JourneyChain {
        pub(super) fn new(expectation: DebitExpectation) -> Self {
            let chain = Chain::new(expectation);
            let listener =
                TcpListener::bind("127.0.0.1:0").unwrap_or_else(|error| panic!("bind: {error}"));
            let address = listener
                .local_addr()
                .unwrap_or_else(|error| panic!("address: {error}"));
            serve(listener, Arc::clone(&chain));
            let boundary = WithdrawalBoundary::new_for_protocol(
                WithdrawalConfig {
                    endpoints: vec![EndpointConfig {
                        url: format!("http://{address}"),
                        request_timeout: Duration::from_secs(5),
                        transport: EndpointTransport::LocalEmulator,
                        expected_chain_id: CHAIN_ID,
                    }],
                    minimum_endpoint_agreement: 1,
                    required_confirmations: REQUIRED_CONFIRMATIONS,
                    poll_cadence: Duration::from_millis(20),
                    delayed_after_polls: 100,
                },
                3,
            )
            .unwrap_or_else(|error| panic!("withdrawal boundary: {error:?}"));
            Self { chain, boundary }
        }

        pub(super) fn boundary(&self) -> &WithdrawalBoundary {
            &self.boundary
        }

        /// Publishes the node's settled withdrawal to the precompile: the
        /// anchor's finalized roots and the claim the custody ledger will hold.
        pub(super) fn settle(&self, settlement: &Settlement) {
            lock(&self.chain).settlement = Some(settlement.clone());
        }

        pub(super) fn send(&self, request: &WithdrawalTransactionRequest) -> TransactionHash {
            assert_eq!(
                request.target, CUSTODY_PRECOMPILE,
                "withdrawal transactions target the custody precompile"
            );
            lock(&self.chain).submit(&request.calldata)
        }

        pub(super) fn mine(&self) {
            lock(&self.chain).mine();
        }

        pub(super) fn advance(&self, seconds: u64) {
            let mut chain = lock(&self.chain);
            chain.timestamp = chain.timestamp.saturating_add(seconds);
            chain.mine();
        }

        pub(super) fn cancel(&self) {
            lock(&self.chain).cancel();
        }

        pub(super) fn recipient_balance(&self) -> u128 {
            let chain = lock(&self.chain);
            let recipient = chain.expectation.recipient.bytes();
            chain.base_units(recipient)
        }

        pub(super) fn vault_balance(&self) -> u128 {
            lock(&self.chain).base_units(CUSTODY_PRECOMPILE.bytes())
        }
    }

    fn lock(chain: &Arc<Mutex<Chain>>) -> MutexGuard<'_, Chain> {
        chain
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn block_hash(number: u64) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(b"layerx-human-withdraw-block\0");
        hasher.update(number.to_be_bytes());
        hasher.finalize().into()
    }

    fn u64_word(value: u64) -> [u8; 32] {
        let mut word = [0_u8; 32];
        word[24..].copy_from_slice(&value.to_be_bytes());
        word
    }

    fn u128_word(value: u128) -> [u8; 32] {
        let mut word = [0_u8; 32];
        word[16..].copy_from_slice(&value.to_be_bytes());
        word
    }

    fn address_word(value: [u8; 20]) -> [u8; 32] {
        let mut word = [0_u8; 32];
        word[12..].copy_from_slice(&value);
        word
    }

    /// `(value, present)` exactly as the anchor precompile returns its roots.
    fn two_words(value: [u8; 32], present: bool) -> Vec<u8> {
        let mut out = if present { value.to_vec() } else { vec![0; WORD] };
        out.extend_from_slice(&u64_word(u64::from(present)));
        out
    }

    /// One dynamic Solidity tuple whose single `string` member sits at
    /// `text_index`, exactly as the precompile returns `getClaim`/`getAsset`.
    fn tuple(head: &[[u8; 32]], text_index: usize, value: &str) -> Vec<u8> {
        let mut out = u64_word(u64::try_from(WORD).unwrap_or_default()).to_vec();
        let offset = head.len().saturating_mul(WORD);
        for (index, word) in head.iter().enumerate() {
            if index == text_index {
                out.extend_from_slice(&u64_word(u64::try_from(offset).unwrap_or_default()));
            } else {
                out.extend_from_slice(word);
            }
        }
        out.extend_from_slice(&u64_word(u64::try_from(value.len()).unwrap_or_default()));
        let mut data = value.as_bytes().to_vec();
        while data.len() % WORD != 0 {
            data.push(0);
        }
        out.extend_from_slice(&data);
        out
    }

    fn read_request_body(stream: &mut TcpStream) -> Option<Vec<u8>> {
        let mut buffer = Vec::new();
        let mut byte = [0_u8; 1];
        let mut expected = None;
        loop {
            match stream.read(&mut byte) {
                Ok(1) => buffer.push(byte[0]),
                _ => return None,
            }
            if expected.is_none() && buffer.ends_with(b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&buffer).to_ascii_lowercase();
                let length: usize = head
                    .split("content-length:")
                    .nth(1)
                    .and_then(|rest| rest.split("\r\n").next())
                    .and_then(|value| value.trim().parse().ok())?;
                expected = Some(buffer.len().saturating_add(length));
            }
            if expected.is_some_and(|total| buffer.len() >= total) {
                break;
            }
        }
        let body = buffer
            .windows(4)
            .position(|window| window == b"\r\n\r\n")?
            .saturating_add(4);
        buffer.get(body..).map(<[u8]>::to_vec)
    }

    fn serve(listener: TcpListener, chain: Arc<Mutex<Chain>>) {
        thread::spawn(move || loop {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let Some(body) = read_request_body(&mut stream) else {
                continue;
            };
            let Ok(request) = serde_json::from_slice::<Value>(&body) else {
                continue;
            };
            let payload = json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": answer(&chain, &request),
            })
            .to_string();
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                payload.len()
            );
            let _ = stream
                .write_all(head.as_bytes())
                .and_then(|()| stream.write_all(payload.as_bytes()))
                .and_then(|()| stream.flush());
        });
    }

    fn answer(chain: &Arc<Mutex<Chain>>, request: &Value) -> Value {
        let method = request["method"].as_str().unwrap_or_default();
        let params = &request["params"];
        let chain = lock(chain);
        match method {
            "eth_chainId" => json!(quantity(CHAIN_ID)),
            "eth_blockNumber" => json!(quantity(chain.head)),
            "eth_getBlockByNumber" => {
                let number = match params[0].as_str() {
                    Some("latest") => chain.head,
                    _ => hex_quantity(&params[0]),
                };
                chain.hashes.get(&number).map_or(Value::Null, |hash| {
                    json!({
                        "number": quantity(number),
                        "hash": hex(hash),
                        "timestamp": quantity(chain.timestamp),
                    })
                })
            }
            "eth_getTransactionReceipt" => {
                let Some(requested) = requested_hash(&params[0]) else {
                    return Value::Null;
                };
                chain
                    .receipts
                    .get(&requested)
                    .map_or(Value::Null, |receipt| {
                        let block_hash = chain
                            .hashes
                            .get(&receipt.block)
                            .copied()
                            .unwrap_or_default();
                        receipt_json(requested, receipt, block_hash)
                    })
            }
            "eth_getTransactionByHash" => {
                let Some(requested) = requested_hash(&params[0]) else {
                    return Value::Null;
                };
                chain
                    .transactions
                    .get(&requested)
                    .map_or(Value::Null, |input| {
                        json!({
                            "hash": hex(&requested),
                            "to": hex(&CUSTODY_PRECOMPILE.bytes()),
                            "input": hex(input),
                            "value": "0x0",
                        })
                    })
            }
            "eth_getBalance" => {
                let bytes = hex_bytes(&params[0]);
                let mut address = [0_u8; 20];
                if bytes.len() != 20 {
                    return json!("0x0");
                }
                address.copy_from_slice(&bytes);
                let wei = chain.balances.get(&address).copied().unwrap_or_default();
                json!(format!("0x{wei:x}"))
            }
            "eth_call" => json!(hex(
                &chain.view(&hex_bytes(&params[0]["to"]), &hex_bytes(&params[0]["data"]))
            )),
            _ => Value::Null,
        }
    }

    fn requested_hash(value: &Value) -> Option<[u8; 32]> {
        let bytes = hex_bytes(value);
        if bytes.len() != 32 {
            return None;
        }
        let mut requested = [0_u8; 32];
        requested.copy_from_slice(&bytes);
        Some(requested)
    }

    fn receipt_json(transaction: [u8; 32], receipt: &Receipt, block_hash: [u8; 32]) -> Value {
        let logs = receipt
            .logs
            .iter()
            .map(|entry| {
                json!({
                    "address": hex(&CUSTODY_PRECOMPILE.bytes()),
                    "topics": entry.topics.iter().map(|topic| hex(topic)).collect::<Vec<_>>(),
                    "data": hex(&entry.data),
                    "transactionHash": hex(&transaction),
                    "blockHash": hex(&block_hash),
                    "blockNumber": quantity(receipt.block),
                    "transactionIndex": quantity(0),
                    "removed": false,
                })
            })
            .collect::<Vec<_>>();
        json!({
            "transactionHash": hex(&transaction),
            "blockNumber": quantity(receipt.block),
            "blockHash": hex(&block_hash),
            "transactionIndex": quantity(0),
            "status": quantity(receipt.status),
            "contractAddress": Value::Null,
            "logs": logs,
        })
    }

    fn quantity(value: u64) -> String {
        format!("0x{value:x}")
    }

    fn hex(bytes: &[u8]) -> String {
        let mut text = String::from("0x");
        for byte in bytes {
            use std::fmt::Write as _;
            let _ = write!(text, "{byte:02x}");
        }
        text
    }

    fn hex_quantity(value: &Value) -> u64 {
        value
            .as_str()
            .and_then(|text| text.strip_prefix("0x"))
            .and_then(|digits| u64::from_str_radix(digits, 16).ok())
            .unwrap_or_default()
    }

    fn hex_bytes(value: &Value) -> Vec<u8> {
        let Some(digits) = value.as_str().and_then(|text| text.strip_prefix("0x")) else {
            return Vec::new();
        };
        digits
            .as_bytes()
            .chunks_exact(2)
            .filter_map(|pair| {
                std::str::from_utf8(pair)
                    .ok()
                    .and_then(|text| u8::from_str_radix(text, 16).ok())
            })
            .collect()
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

use custody_chain::{JourneyChain, Settlement, VAULT_BALANCE};
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
    chain: JourneyChain,
    settled: SettledWithdrawal,
    proof_available: bool,
    transactions: BTreeMap<[u8; 32], TransactionHash>,
    action_counts: BTreeMap<PaxeerAction, u32>,
    crash_after_broadcast: Option<PaxeerAction>,
}

impl RealRuntime {
    fn new(expectation: DebitExpectation, settled: SettledWithdrawal) -> Self {
        Self {
            chain: JourneyChain::new(expectation),
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
        self.chain = JourneyChain::new(expectation);
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
    let mut runtime = RealRuntime::new(fixture.expectation([0x31; 32]), settled);
    let (mut store, mut journey, mut now) = drive_claim_queued(&fixture, &mut runtime, &mut agent);
    let reminders = {
        let scope = store
            .principal(&fixture.principal)
            .unwrap_or_else(|error| panic!("reminder scope: {error}"));
        WithdrawalJourney::reminders(&scope, &fixture.plan.journey_id)
            .unwrap_or_else(|error| panic!("reminders: {error}"))
    };
    assert_eq!(reminders.len(), 1);
    runtime.chain.advance(3_601);
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
    let mut runtime = RealRuntime::new(fixture.expectation([0x31; 32]), settled);
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

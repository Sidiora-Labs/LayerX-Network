#[allow(dead_code)]
mod support;

use std::collections::BTreeMap;
use std::fs;
use std::future::Future;
use std::path::PathBuf;
use std::pin::pin;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::thread;
use std::time::Duration;

use ed25519_dalek::{Signer as _, SigningKey as ReceiptSigningKey};
use k256::ecdsa::SigningKey as EvmSigningKey;
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
    prepare_activity, CorePreparationBoundary, CorePreparationState, CoreStateError,
    PreparationDefaults, PrepareRequest, Prepared,
};
use layerx_agentd::sign::{attach_external_signature, verify_before_submit};
use layerx_agentd::store::{Store as AgentStore, TenantId};
use layerx_human_service::binding::{
    AgentBindingContract, AgentBindingError, AgentBindingReceipt, AgentSubmission,
    BindingAgentRequest, BindingJourney,
};
use layerx_human_service::custody::{
    CustodySigner, EnvelopeKms, KeyClass, KeyEntropy, KeyId, Keystore, SigningLimits,
};
use layerx_human_service::journeys::{
    AgentBoundary, AgentBoundaryError, AgentObservation, AgentPreparation, DepositAgentBoundary,
    DepositAgentPlan, DepositBoundaryError, DepositJourney, DepositPlan, DepositRuntime,
    DepositStage, ReceiptLookup, ReceiptMaterial, WalletCustodyOutcome, WalletCustodyRequest,
};
use layerx_human_service::notify::JourneyId;
use layerx_human_service::store::{PrincipalId, PrincipalStore, TenancyDigest};
use layerx_human_service::trace::TraceId;
use layerx_paxeer_client::{
    raw_call, DepositFailure, DepositProof, EndpointConfig, ExecutionOutcome, FinalityReport,
    FinalityStage, FinalityTracker, FinalizedCheckpoint, Json, PaxeerClient, TrackerConfig,
    TransactionHash, TransactionInclusion,
};
use layerx_proof::checkpoint::{checkpoint_id, Attestation, Certificate, Checkpoint, GuarantorKey};
use layerx_proof::receipt::AuthorizedBatch;
use layerx_sdk::{Call, Client as AgentClient};
use layerx_types::account::AccountId;
use layerx_types::activity::{Authority, TimestampBound};
use layerx_types::amount::Amount;
use layerx_types::ids::{AssetId, CheckpointId, Did, IdempotencyKey};
use layerx_types::intent::{EvmAddress, NetworkId};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use sha2::{Digest as _, Sha256};
use sha3::Keccak256;

use support::{directory, principal, retention_uniform, tenancy};

const FUNDED: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
const EMERGENCY: &str = "0x70997970C51812dc3A0107d01b50e0d17dc79C8";
const FUNDED_PRIVATE_KEY: &str = "ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
const TOKEN_CREATION: &str =
    include_str!("../../layerx-paxeer-client/tests/contracts/IntegrationToken.hex");
const REGISTRY_CREATION: &str =
    include_str!("../../layerx-paxeer-client/tests/contracts/AssetRegistry.hex");
const VAULT_CREATION: &str =
    include_str!("../../layerx-paxeer-client/tests/contracts/LayerXVault.hex");
const ASSET: [u8; 32] = [0x42; 32];
const AMOUNT: u128 = 25;
const NETWORK_ID: u32 = 17;
const CHECKPOINT_HEADER_HEX: &str = "000117010f010001020000001103000000000000000704000000000000000805000000000000000b0600000000000000130700000020070707070707070707070707070707070707070707070707070707070707070708000000200808080808080808080808080808080808080808080808080808080808080808090000002009090909090909090909090909090909090909090909090909090909090909090a000000200a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0b000000200b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0c000000200c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0d000000200d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0e00000000000003e80f000000200f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f";
const CHECKPOINT_ID_HEX: &str = "5cd43e8c1a6a0ba5594d75846fe40bd851909368fc0a7439657180c5fb8b9572";
const CREDIT_RECEIPT_HEX: &str = "000152010001000000203ad4f279bd6297c488fbf76c0802a3fb20c5060d955a7648e6417f8653f8fa110000000000000009000000200707070707070707070707070707070707070707070707070707070707070707000000200808080808080808080808080808080808080808080808080808080808080808000000200808080808080808080808080808080808080808080808080808080808080808000000000000000000000000000000000000000000000001000000205cd43e8c1a6a0ba5594d75846fe40bd851909368fc0a7439657180c5fb8b957200080000000100000001010000002042424242424242424242424242424242424242424242424242424242424242420000000000000000000000000000001900000020f94d2cc01cae556915267bc3d1ad7c58034009ea25cbe56906be12b9ca876de0000000000000000000000000000000640000000000000000000000000000004b00000000000000010000002042de0bc2f3c75fd9995e3ad3d57efaf06530b93679d956ddf17fa9d325e1d60d0000000000000000000000000000000a00000000000000000000000000000023000000200909090909090909090909090909090909090909090909090909090909090909000000200a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a000000200b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b00000000000003e80100000040d8bdb7a072cdbc700f7390c482c40823192d2bfc983749456a20e08dc42f54526e153909c707f05d141c20ec9a728f6358fe1a460f6bf7ca7171758511b02e0d";
const GUARANTOR_PUBLIC_KEYS: [&str; 3] = [
    "0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798",
    "02c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5",
    "02f9308a019258c31049344f85f89d5229b531c845836f99b08601f113bce036f9",
];
const GUARANTOR_SIGNATURES: [&str; 3] = [
    "f9ea8b1dc2d6f6db15ac68bf2ac645081310822839fe31e40fca5f37665d8e2224a16055357219970908e922b785542548f2406ac2bfae1a8d420b613d3c701e",
    "ee20fdb15279bda8a90b66b5b3817ed294ec8362948665bc066c8b90a95366cb34902aa1845f66e66c97889b0039a7115463760ce0eb7a7620d8d3d857342aad",
    "cdaac951fb38f3e4d713fcb46a19d41caa60811be4532acdab7b7dc99d2d1de47afbd9349d93eaf98cf464a16b3a0b08229200f403048230d164c3abed716389",
];

static NEXT_PORT: AtomicU16 = AtomicU16::new(0);

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
        Poll::Pending => panic!("journey future unexpectedly blocked"),
    }
}

fn hex_nibble(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        b'A'..=b'F' => byte - b'A' + 10,
        _ => panic!("non-hex digit"),
    }
}

fn hex_bytes(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| (hex_nibble(pair[0]) << 4) | hex_nibble(pair[1]))
        .collect()
}

fn hex_array<const N: usize>(value: &str) -> [u8; N] {
    hex_bytes(value)
        .try_into()
        .unwrap_or_else(|value: Vec<u8>| panic!("expected {N} bytes, got {}", value.len()))
}

fn parse_address(value: &str) -> EvmAddress {
    let digits = value.strip_prefix("0x").unwrap_or(value);
    let mut address = [0_u8; 20];
    for (slot, pair) in address.iter_mut().zip(digits.as_bytes().chunks_exact(2)) {
        *slot = (hex_nibble(pair[0]) << 4) | hex_nibble(pair[1]);
    }
    EvmAddress::new(address)
}

fn address_word(address: EvmAddress) -> [u8; 32] {
    let mut word = [0_u8; 32];
    word[12..].copy_from_slice(&address.bytes());
    word
}

fn u128_word(value: u128) -> [u8; 32] {
    let mut word = [0_u8; 32];
    word[16..].copy_from_slice(&value.to_be_bytes());
    word
}

fn byte_word(value: u8) -> [u8; 32] {
    let mut word = [0_u8; 32];
    word[31] = value;
    word
}

fn calldata(prefix: &str, words: &[[u8; 32]]) -> String {
    let mut output = prefix.to_owned();
    for byte in words.iter().flatten() {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn address_hex(address: EvmAddress) -> String {
    format!("0x{}", &calldata("", &[address_word(address)])[24..])
}

fn member(name: &str, value: &str) -> (String, Json) {
    (name.to_owned(), Json::Text(value.to_owned()))
}

struct Anvil {
    child: Child,
    endpoint: EndpointConfig,
}

impl Anvil {
    fn launch() -> Self {
        for _ in 0..8 {
            let lane = u16::try_from(std::process::id() % 8_000).unwrap_or(0);
            let port = 21_000_u16
                .saturating_add(lane)
                .saturating_add(NEXT_PORT.fetch_add(1, Ordering::Relaxed).saturating_mul(7));
            let endpoint = EndpointConfig {
                url: format!("http://127.0.0.1:{port}"),
                request_timeout: Duration::from_secs(5),
            };
            let binary = if PathBuf::from("/root/.foundry/bin/anvil").exists() {
                PathBuf::from("/root/.foundry/bin/anvil")
            } else {
                PathBuf::from("anvil")
            };
            let child = Command::new(binary)
                .args(["--port", &port.to_string(), "--silent"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap_or_else(|error| panic!("spawn anvil: {error}"));
            let mut value = Self { child, endpoint };
            for _ in 0..100 {
                if raw_call(&value.endpoint, "eth_blockNumber", &[]).is_ok() {
                    return value;
                }
                thread::sleep(Duration::from_millis(30));
            }
            value.halt();
        }
        panic!("no Anvil port available")
    }

    fn call(&self, method: &str, params: &[Json]) -> Json {
        raw_call(&self.endpoint, method, params)
            .unwrap_or_else(|error| panic!("{method}: {error:?}"))
    }

    fn send(&self, to: Option<EvmAddress>, data: &str) -> TransactionHash {
        let mut fields = vec![member("from", FUNDED), member("data", data)];
        if let Some(to) = to {
            fields.push(member("to", &address_hex(to)));
        }
        fields.push(member("gas", "0x989680"));
        let result = self.call("eth_sendTransaction", &[Json::Object(fields)]);
        TransactionHash::from_hex(
            result
                .as_text()
                .unwrap_or_else(|| panic!("missing tx hash")),
        )
        .unwrap_or_else(|error| panic!("transaction hash: {error:?}"))
    }

    fn client(&self) -> PaxeerClient {
        PaxeerClient::new(vec![self.endpoint.clone()])
            .unwrap_or_else(|error| panic!("Paxeer client: {error:?}"))
    }

    fn receipt(&self, transaction: TransactionHash) -> TransactionInclusion {
        for _ in 0..200 {
            if let Some(receipt) = self
                .client()
                .transaction_receipt(transaction)
                .unwrap_or_else(|error| panic!("transaction receipt: {error:?}"))
            {
                return receipt;
            }
            thread::sleep(Duration::from_millis(20));
        }
        panic!("transaction not included")
    }

    fn deploy(&self, creation: &str, words: &[[u8; 32]]) -> EvmAddress {
        let receipt = self.receipt(self.send(None, &calldata(creation.trim(), words)));
        assert_eq!(receipt.execution, ExecutionOutcome::Succeeded);
        receipt
            .deployed_contract
            .unwrap_or_else(|| panic!("deployment address missing"))
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

fn registry() -> ModuleRegistry {
    let governance = ActivityType::new(ModuleId::Governance, 4)
        .unwrap_or_else(|error| panic!("governance activity: {error:?}"));
    let bridge = ActivityType::new(ModuleId::Bridge, 1)
        .unwrap_or_else(|error| panic!("bridge activity: {error:?}"));
    ModuleRegistry::new(&[
        ModuleRegistration::new(ModuleId::Governance, &[governance])
            .unwrap_or_else(|error| panic!("governance registration: {error:?}")),
        ModuleRegistration::new(ModuleId::Bridge, &[bridge])
            .unwrap_or_else(|error| panic!("bridge registration: {error:?}")),
    ])
    .unwrap_or_else(|error| panic!("registry: {error:?}"))
}

fn checkpoint() -> (FinalizedCheckpoint, Certificate, Vec<GuarantorKey>) {
    let checkpoint = Checkpoint::new(
        hex_bytes(CHECKPOINT_HEADER_HEX),
        b"REAL-CORE-PROOF".to_vec(),
    );
    let identifier =
        checkpoint_id(&checkpoint).unwrap_or_else(|error| panic!("checkpoint id: {error:?}"));
    assert_eq!(identifier, hex_array(CHECKPOINT_ID_HEX));
    let mut attestations = Vec::new();
    let mut bonded = Vec::new();
    for value in 1_u8..=3 {
        let index = usize::from(value - 1);
        let mut guarantor_id = [0_u8; 32];
        guarantor_id[0] = value;
        attestations.push(Attestation::new(
            identifier,
            identifier,
            guarantor_id,
            8,
            [12; 32],
            true,
            true,
            0x1f,
            1_000 + u64::from(value),
            hex_array(GUARANTOR_SIGNATURES[index]),
        ));
        bonded.push(GuarantorKey::new(
            guarantor_id,
            hex_array(GUARANTOR_PUBLIC_KEYS[index]),
            true,
        ));
    }
    let certificate = Certificate::new(checkpoint, attestations, 2, None);
    let finalized =
        FinalizedCheckpoint::verify(&certificate, &bonded, CheckpointId::new(identifier), None)
            .unwrap_or_else(|error| panic!("checkpoint verification: {error:?}"));
    (finalized, certificate, bonded)
}

fn evm_key() -> EvmSigningKey {
    EvmSigningKey::from_slice(&hex_bytes(FUNDED_PRIVATE_KEY))
        .unwrap_or_else(|error| panic!("EVM signing key: {error}"))
}

fn evm_address(key: &EvmSigningKey) -> EvmAddress {
    let point = key.verifying_key().to_encoded_point(false);
    let hash = Keccak256::digest(&point.as_bytes()[1..]);
    let mut address = [0_u8; 20];
    address.copy_from_slice(&hash[12..]);
    EvmAddress::new(address)
}

fn ownership_signature(key: &EvmSigningKey, digest: [u8; 32]) -> [u8; 65] {
    let (signature, recovery) = key
        .sign_prehash_recoverable(&digest)
        .unwrap_or_else(|error| panic!("ownership signature: {error}"));
    let mut bytes = [0_u8; 65];
    bytes[..64].copy_from_slice(&signature.to_bytes());
    bytes[64] = recovery.to_byte().saturating_add(27);
    bytes
}

struct CoreState {
    registry: ModuleRegistry,
}

impl CorePreparationBoundary for CoreState {
    fn preparation_state(&mut self, _actor: &Did) -> Result<CorePreparationState, CoreStateError> {
        Ok(CorePreparationState {
            network_id: NETWORK_ID,
            account_sequence: 5,
            protocol_timestamp: 1_000,
            observed_head_sequence: 91,
            module_registry: self.registry.clone(),
        })
    }
}

/// Real in-process binding path: core-derived preparation, canonical
/// disclosure, signature verification, and a durable agentd outbox.
struct BindingAgent {
    registry: ModuleRegistry,
    signer: layerx_crypto::local::LocalSigner,
    store: AgentStore,
    outbox: Outbox,
    tenant: TenantId,
}

impl BindingAgent {
    fn new(root: &std::path::Path) -> Self {
        Self {
            registry: registry(),
            signer: layerx_crypto::local::LocalSigner::new([0xa5; 32]),
            store: AgentStore::open(root)
                .unwrap_or_else(|error| panic!("binding agent store: {error}")),
            outbox: Outbox::default(),
            tenant: TenantId::new("tenant-a")
                .unwrap_or_else(|error| panic!("agent tenant: {error}")),
        }
    }
}

impl AgentBindingContract for BindingAgent {
    fn submit_binding(
        &mut self,
        request: BindingAgentRequest<'_>,
    ) -> Result<AgentSubmission, AgentBindingError> {
        use layerx_crypto::signer::{sign_disclosed, Signer as _};
        let mut core = CoreState {
            registry: self.registry.clone(),
        };
        let prepared = prepare_activity(
            &mut core,
            PreparationDefaults {
                timestamp_span: 30,
                fee_limit: Amount::from_u128(1),
                maximum_payload_bytes: 512,
            },
            PrepareRequest {
                actor: request.actor.clone(),
                authority: Authority::owner(&self.signer.public_key())
                    .map_err(|_| AgentBindingError::ContractViolation)?,
                activity_type: request.compiled.activity_type(),
                expected_account_sequence: Some(5),
                timestamp_bound: None,
                fee_limit: Some(Amount::from_u128(1)),
                idempotency_key: request.idempotency_key,
                payload: request.compiled.payload().as_bytes().to_vec(),
                declared_payload_limit: 512,
            },
        )
        .map_err(|_| AgentBindingError::ContractViolation)?;
        if prepared.disclosure.evm_payout_binding.is_none() {
            return Err(AgentBindingError::ContractViolation);
        }
        let signature = ready(sign_disclosed(
            &self.signer,
            &prepared.canonical_bytes,
            &prepared.disclosure,
            &self.registry,
        ))
        .map_err(|_| AgentBindingError::Refused)?;
        let signed = attach_external_signature(&prepared, *signature.as_bytes())
            .map_err(|_| AgentBindingError::ContractViolation)?;
        let verified = verify_before_submit(
            &signed,
            &prepared,
            &self.signer.public_key(),
            &self.registry,
        )
        .map_err(|_| AgentBindingError::ContractViolation)?;
        let submission_id = request.idempotency_key.bytes();
        let activity_id = verified.audit.activity_id;
        self.outbox
            .enqueue(
                &mut self.store,
                self.tenant.clone(),
                submission_id,
                verified,
            )
            .map_err(|_| AgentBindingError::Unavailable)?;
        Ok(AgentSubmission {
            submission_id,
            activity_id,
        })
    }
}

fn push_bytes(output: &mut Vec<u8>, bytes: &[u8]) {
    let length = u32::try_from(bytes.len()).unwrap_or_else(|_| panic!("receipt field too long"));
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(bytes);
}

fn binding_receipt(submission: AgentSubmission, address: EvmAddress) -> AgentBindingReceipt {
    let signer = ReceiptSigningKey::from_bytes(&[0x35; 32]);
    let encode = |signature: Option<[u8; 64]>| {
        let mut output = Vec::new();
        output.extend_from_slice(&1_u16.to_be_bytes());
        output.extend_from_slice(&0x5201_u16.to_be_bytes());
        output.extend_from_slice(&1_u16.to_be_bytes());
        push_bytes(&mut output, &submission.activity_id);
        output.extend_from_slice(&9_u64.to_be_bytes());
        push_bytes(&mut output, &[2; 32]);
        push_bytes(&mut output, &[3; 32]);
        push_bytes(&mut output, &[8; 32]);
        output.extend_from_slice(&0_i32.to_be_bytes());
        output.extend_from_slice(&0_u32.to_be_bytes());
        output.extend_from_slice(&0_u128.to_be_bytes());
        push_bytes(&mut output, &[4; 32]);
        output.extend_from_slice(&(ModuleId::Governance as u16).to_be_bytes());
        output.extend_from_slice(&1_u32.to_be_bytes());
        output.extend_from_slice(&1_u32.to_be_bytes());
        output.push(4);
        push_bytes(&mut output, &[5; 32]);
        output.extend_from_slice(&0_u128.to_be_bytes());
        push_bytes(&mut output, &[6; 32]);
        output.extend_from_slice(&100_u128.to_be_bytes());
        output.extend_from_slice(&100_u128.to_be_bytes());
        output.extend_from_slice(&1_u64.to_be_bytes());
        let mut recorded = [0_u8; 32];
        recorded[12..].copy_from_slice(&address.bytes());
        push_bytes(&mut output, &recorded);
        output.extend_from_slice(&10_u128.to_be_bytes());
        output.extend_from_slice(&10_u128.to_be_bytes());
        push_bytes(&mut output, &[9; 32]);
        push_bytes(&mut output, &[10; 32]);
        push_bytes(&mut output, &[11; 32]);
        output.extend_from_slice(&1_000_u64.to_be_bytes());
        output.push(u8::from(signature.is_some()));
        if let Some(signature) = signature {
            push_bytes(&mut output, &signature);
        }
        output
    };
    let unsigned = encode(None);
    let mut digest = Sha256::new();
    digest.update(b"LXP/v1/receipt\0");
    digest.update(&unsigned);
    let signature = signer.sign(&<[u8; 32]>::from(digest.finalize()));
    AgentBindingReceipt {
        submission_id: submission.submission_id,
        canonical_receipt: encode(Some(signature.to_bytes())),
        authorized_batch: AuthorizedBatch::new(
            [4; 32],
            [5; 32],
            [2; 32],
            [3; 32],
            signer.verifying_key().to_bytes(),
        ),
    }
}

/// Real wallet/Paxeer/core adapter. The wallet action map models the durable
/// wallet-provider idempotency record; an acknowledgement-gap fault occurs
/// after the real transaction was broadcast.
struct RealDepositRuntime {
    anvil: Anvil,
    vault: EvmAddress,
    certificate: Certificate,
    bonded: Vec<GuarantorKey>,
    checkpoint: FinalizedCheckpoint,
    wallet_actions: BTreeMap<[u8; 32], TransactionHash>,
    wallet_opens: u32,
    fail_wallet_ack_once: bool,
    reject_wallet: bool,
}

impl RealDepositRuntime {
    fn new() -> Self {
        let anvil = Anvil::launch();
        let owner = parse_address(FUNDED);
        let emergency = parse_address(EMERGENCY);
        let token = anvil.deploy(TOKEN_CREATION, &[address_word(owner)]);
        let asset_registry = anvil.deploy(
            REGISTRY_CREATION,
            &[
                address_word(owner),
                address_word(emergency),
                [7; 32],
                u128_word(1),
            ],
        );
        let vault = anvil.deploy(
            VAULT_CREATION,
            &[
                address_word(asset_registry),
                address_word(owner),
                address_word(emergency),
                [8; 32],
                u128_word(1),
            ],
        );
        for (to, data) in [
            (
                token,
                calldata("0x40c10f19", &[address_word(owner), u128_word(100)]),
            ),
            (
                asset_registry,
                calldata(
                    "0xea249288",
                    &[
                        ASSET,
                        address_word(token),
                        byte_word(6),
                        u128_word(1),
                        u128_word(1_000),
                    ],
                ),
            ),
            (
                token,
                calldata("0x095ea7b3", &[address_word(vault), u128_word(AMOUNT)]),
            ),
        ] {
            assert_eq!(
                anvil.receipt(anvil.send(Some(to), &data)).execution,
                ExecutionOutcome::Succeeded
            );
        }
        let (checkpoint, certificate, bonded) = checkpoint();
        Self {
            anvil,
            vault,
            certificate,
            bonded,
            checkpoint,
            wallet_actions: BTreeMap::new(),
            wallet_opens: 0,
            fail_wallet_ack_once: true,
            reject_wallet: false,
        }
    }

    fn final_report(&self, transaction: TransactionHash) -> FinalityReport {
        let mut tracker = FinalityTracker::new(
            TrackerConfig {
                endpoints: vec![self.anvil.endpoint.clone()],
                required_confirmations: 1,
                poll_cadence: Duration::from_millis(20),
                delayed_after_polls: 100,
            },
            transaction,
        )
        .unwrap_or_else(|error| panic!("finality tracker: {error:?}"));
        for _ in 0..200 {
            let report = tracker.poll();
            if matches!(report.stage, FinalityStage::Final { .. }) {
                return report;
            }
            thread::sleep(Duration::from_millis(20));
        }
        panic!("custody transaction did not finalize")
    }
}

impl DepositRuntime for RealDepositRuntime {
    fn submit_custody(
        &mut self,
        request: &WalletCustodyRequest,
    ) -> Result<WalletCustodyOutcome, DepositBoundaryError> {
        if request.wallet != parse_address(FUNDED)
            || request.network_id != NETWORK_ID
            || request.vault != self.vault
            || request.asset != AssetId::new(ASSET)
            || request.amount != Amount::from_u128(AMOUNT)
        {
            return Err(DepositBoundaryError::ContractViolation);
        }
        if self.reject_wallet {
            return Ok(WalletCustodyOutcome::Rejected);
        }
        if let Some(transaction) = self.wallet_actions.get(&request.action_key).copied() {
            return Ok(WalletCustodyOutcome::Submitted(transaction));
        }
        self.wallet_opens = self.wallet_opens.saturating_add(1);
        let transaction = self.anvil.send(
            Some(self.vault),
            &calldata(
                "0x8a9e532c",
                &[ASSET, u128_word(AMOUNT), request.beneficiary],
            ),
        );
        self.wallet_actions.insert(request.action_key, transaction);
        if self.fail_wallet_ack_once {
            self.fail_wallet_ack_once = false;
            return Err(DepositBoundaryError::Unavailable);
        }
        Ok(WalletCustodyOutcome::Submitted(transaction))
    }

    fn poll_finality(
        &mut self,
        transaction: TransactionHash,
    ) -> Result<FinalityReport, DepositBoundaryError> {
        Ok(self.final_report(transaction))
    }

    fn obtain_proof(
        &mut self,
        transaction: TransactionHash,
    ) -> Result<DepositProof, DepositFailure> {
        let report = self.final_report(transaction);
        DepositProof::obtain_from_certificate(
            &self.anvil.client(),
            &report,
            self.vault,
            &self.certificate,
            &self.bonded,
            self.checkpoint.id(),
            None,
        )
    }
}

struct RecordedCore(CorePreparationState);
impl CorePreparationBoundary for RecordedCore {
    fn preparation_state(&mut self, _actor: &Did) -> Result<CorePreparationState, CoreStateError> {
        Ok(self.0.clone())
    }
}

/// In-process production components for the deposit credit: agentd prepare,
/// signature verification, durable outbox, and sequencer-signed receipt.
struct RealCreditAgent {
    store: AgentStore,
    outbox: Outbox,
    tenant: TenantId,
    registry: ModuleRegistry,
    preparations: BTreeMap<[u8; 32], Prepared>,
    observations: BTreeMap<[u8; 32], AgentObservation>,
    materials: BTreeMap<[u8; 32], ReceiptMaterial>,
    effects: BTreeMap<[u8; 32], u32>,
    submit_ack_fault: bool,
}

impl RealCreditAgent {
    fn new(root: &std::path::Path) -> Self {
        Self {
            store: AgentStore::open(root).unwrap_or_else(|error| panic!("agent store: {error}")),
            outbox: Outbox::default(),
            tenant: TenantId::new("tenant-a")
                .unwrap_or_else(|error| panic!("agent tenant: {error}")),
            registry: registry(),
            preparations: BTreeMap::new(),
            observations: BTreeMap::new(),
            materials: BTreeMap::new(),
            effects: BTreeMap::new(),
            submit_ack_fault: true,
        }
    }

    fn material(activity_id: [u8; 32]) -> Result<ReceiptMaterial, AgentBoundaryError> {
        let signer = ReceiptSigningKey::from_bytes(&[3; 32]);
        let mut canonical = hex_bytes(CREDIT_RECEIPT_HEX);
        canonical[10..42].copy_from_slice(&activity_id);
        let signature_offset = canonical
            .len()
            .checked_sub(64)
            .ok_or(AgentBoundaryError::CorruptResponse)?;
        let mut unsigned = canonical[..signature_offset - 5].to_vec();
        unsigned.push(0);
        let mut digest = Sha256::new();
        digest.update(b"LXP/v1/receipt\0");
        digest.update(&unsigned);
        canonical[signature_offset..]
            .copy_from_slice(&signer.sign(&<[u8; 32]>::from(digest.finalize())).to_bytes());
        Ok(ReceiptMaterial {
            canonical_bytes: canonical,
            authorised_batch: AuthorizedBatch::new(
                hex_array(CHECKPOINT_ID_HEX),
                ASSET,
                [7; 32],
                [8; 32],
                signer.verifying_key().to_bytes(),
            ),
        })
    }

    fn tracked(
        key: [u8; 32],
        state: SubmissionState,
        receipt_digest: [u8; 32],
    ) -> TrackedSubmission {
        TrackedSubmission {
            submission_ref: SubmissionRef::new(format!("sub-{}", hex(&key)))
                .unwrap_or_else(|error| panic!("submission ref: {error:?}")),
            state,
            evidence: vec![AgentEvidenceRef {
                kind: "sequencer-receipt".to_owned(),
                digest: receipt_digest,
            }],
            verification_level: Level::SequencerSigned,
            transitions: Vec::new(),
        }
    }
}

impl AgentBoundary for RealCreditAgent {
    fn prepare(
        &mut self,
        call: &Call<IdempotentMutation<ApiPrepareRequest>>,
    ) -> Result<AgentPreparation, AgentBoundaryError> {
        let mutation = call.request();
        let key = mutation.key.bytes();
        if !self.preparations.contains_key(&key) {
            let request = &mutation.operation;
            let mut core = RecordedCore(CorePreparationState {
                network_id: NETWORK_ID,
                account_sequence: request.account_sequence.get(),
                protocol_timestamp: 1_000,
                observed_head_sequence: 88,
                module_registry: self.registry.clone(),
            });
            let prepared = prepare_activity(
                &mut core,
                PreparationDefaults {
                    timestamp_span: 15,
                    fee_limit: Amount::from_u128(7),
                    maximum_payload_bytes: 1_024,
                },
                PrepareRequest {
                    actor: Did::new(request.actor.as_str().as_bytes())
                        .map_err(|_| AgentBoundaryError::CorruptResponse)?,
                    authority: Authority::owner(request.authority.as_str().as_bytes())
                        .map_err(|_| AgentBoundaryError::CorruptResponse)?,
                    activity_type: ActivityType::new(ModuleId::Bridge, 1)
                        .map_err(|_| AgentBoundaryError::CorruptResponse)?,
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
            )
            .map_err(|_| AgentBoundaryError::Refused)?;
            self.preparations.insert(key, prepared);
        }
        let prepared = self
            .preparations
            .get(&key)
            .ok_or(AgentBoundaryError::CorruptResponse)?;
        let request = &mutation.operation;
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
        let mutation = call.request();
        let key = mutation.key.bytes();
        if let Some(observation) = self.observations.get(&key).cloned() {
            return Ok(observation);
        }
        let prepared = self
            .preparations
            .get(&key)
            .ok_or(AgentBoundaryError::CorruptResponse)?;
        let signature: [u8; 64] = mutation
            .operation
            .signature
            .as_bytes()
            .try_into()
            .map_err(|_| AgentBoundaryError::Refused)?;
        let signed = attach_external_signature(prepared, signature)
            .map_err(|_| AgentBoundaryError::Refused)?;
        let verified = verify_before_submit(&signed, prepared, &signer_public_key, &self.registry)
            .map_err(|_| AgentBoundaryError::Refused)?;
        let activity_id = verified.audit.activity_id;
        let material = Self::material(activity_id)?;
        match self
            .outbox
            .enqueue(&mut self.store, self.tenant.clone(), key, verified)
        {
            Ok(()) => {}
            Err(OutboxError::Duplicate) => return Err(AgentBoundaryError::CorruptResponse),
            Err(_) => return Err(AgentBoundaryError::Unavailable),
        }
        self.outbox
            .transition(
                &mut self.store,
                key,
                OutboxState::Submitted,
                "real transport accepted credit",
                None,
            )
            .map_err(|_| AgentBoundaryError::Unavailable)?;
        *self.effects.entry(key).or_default() += 1;
        self.materials.insert(key, material.clone());
        let receipt_digest: [u8; 32] = Sha256::digest(&material.canonical_bytes).into();
        let observation = AgentObservation {
            submission: Self::tracked(
                key,
                SubmissionState::Executed {
                    receipt_ref: ReceiptRef::new(format!("rcp-{}", hex(&key)))
                        .map_err(|_| AgentBoundaryError::CorruptResponse)?,
                },
                receipt_digest,
            ),
            activity_id,
            receipt: Some(material),
        };
        self.observations.insert(key, observation.clone());
        if self.submit_ack_fault {
            self.submit_ack_fault = false;
            return Err(AgentBoundaryError::Unavailable);
        }
        Ok(observation)
    }

    fn track(
        &mut self,
        _call: &Call<TrackRequest>,
    ) -> Result<AgentObservation, AgentBoundaryError> {
        Err(AgentBoundaryError::CorruptResponse)
    }

    fn receipt_by_idempotency_key(
        &mut self,
        idempotency_key: [u8; 32],
        _expected_activity_id: [u8; 32],
    ) -> Result<ReceiptLookup, AgentBoundaryError> {
        self.materials
            .get(&idempotency_key)
            .cloned()
            .map(ReceiptLookup::Found)
            .ok_or(AgentBoundaryError::Unavailable)
    }
}

impl DepositAgentBoundary for RealCreditAgent {
    fn credit_receipt(
        &mut self,
        action_key: [u8; 32],
        activity_id: [u8; 32],
    ) -> Result<ReceiptMaterial, AgentBoundaryError> {
        let material = self
            .materials
            .get(&action_key)
            .cloned()
            .ok_or(AgentBoundaryError::Unavailable)?;
        let observation = self
            .observations
            .get(&action_key)
            .ok_or(AgentBoundaryError::Unavailable)?;
        if activity_id != observation.activity_id {
            return Err(AgentBoundaryError::CorruptResponse);
        }
        Ok(material)
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

struct Fixture {
    root: PathBuf,
    store_root: PathBuf,
    tenancy_digest: TenancyDigest,
    principal: PrincipalId,
    signer: CustodySigner,
    binding: BindingJourney,
    agent_contract: AgentClient,
    trace: TraceId,
    plan: DepositPlan,
}

impl Fixture {
    #[allow(clippy::too_many_lines)]
    fn new(runtime: &RealDepositRuntime) -> Self {
        let root = directory("deposit-journey");
        fs::create_dir_all(&root).unwrap_or_else(|error| panic!("fixture root: {error}"));
        let store_root = root.join("human-store");
        let map = tenancy(&[("alice", "tenant-a")]);
        let tenancy_digest = map
            .install(&store_root)
            .unwrap_or_else(|error| panic!("tenancy: {error}"));
        let principal = principal("alice");
        let secret = root.join("kms-root");
        fs::write(&secret, [0x42; 64]).unwrap_or_else(|error| panic!("KMS root: {error}"));
        let provider = EnvelopeKms::new("file-kms://human-primary", &secret)
            .unwrap_or_else(|error| panic!("KMS provider: {error}"));
        let keystore = Keystore::open_development(root.join("custody"), NETWORK_ID, provider)
            .unwrap_or_else(|error| panic!("keystore: {error}"));
        let custody_key =
            KeyId::new("human-primary").unwrap_or_else(|error| panic!("key id: {error}"));
        let _ = keystore
            .generate(
                &principal,
                &custody_key,
                KeyClass::HumanPrimary,
                KeyEntropy::new([0x51; 32], [0x52; 16], [0x53; 24])
                    .unwrap_or_else(|error| panic!("entropy: {error}")),
            )
            .unwrap_or_else(|error| panic!("generate custody key: {error}"));
        let signer_store =
            PrincipalStore::open(&store_root, retention_uniform(10_000), tenancy_digest)
                .unwrap_or_else(|error| panic!("signer store: {error}"));
        let signer = CustodySigner::new(
            keystore,
            signer_store,
            registry(),
            SigningLimits::new(1_000, 10_000)
                .unwrap_or_else(|error| panic!("signing limits: {error}")),
        );
        let binding = BindingJourney::new(registry());
        let wallet = evm_address(&evm_key());
        assert_eq!(wallet, parse_address(FUNDED));
        let mut store =
            PrincipalStore::open(&store_root, retention_uniform(10_000), tenancy_digest)
                .unwrap_or_else(|error| panic!("binding store: {error}"));
        let mut scope = store
            .principal(&principal)
            .unwrap_or_else(|error| panic!("binding scope: {error}"));
        let did = Did::new(b"did:layerx:deposit-recipient")
            .unwrap_or_else(|error| panic!("DID: {error:?}"));
        let network =
            NetworkId::new(NETWORK_ID).unwrap_or_else(|error| panic!("network: {error:?}"));
        let statement = BindingJourney::issue_statement(&did, network, wallet, 100, 60)
            .unwrap_or_else(|error| panic!("binding statement: {error}"));
        let mut binding_agent = BindingAgent::new(&root.join("binding-agent"));
        let submission = binding
            .submit_initial(
                &mut scope,
                &statement,
                &ownership_signature(&evm_key(), statement.signing_digest()),
                IdempotencyKey::new([0x41; 32]),
                &mut binding_agent,
                101,
            )
            .unwrap_or_else(|error| panic!("binding submit: {error}"));
        let _ = binding
            .finalize(
                &mut scope,
                &binding_receipt(submission, wallet),
                102,
                &TraceId::mint([0x19; 16]),
            )
            .unwrap_or_else(|error| panic!("binding finalize: {error}"));
        drop(scope);
        drop(store);
        let recipient = AccountId::parse("agent:did:layerx:deposit-recipient:main")
            .unwrap_or_else(|error| panic!("recipient: {error:?}"));
        let reserve = AccountId::parse("system:paxeer-reserve")
            .unwrap_or_else(|error| panic!("reserve: {error:?}"));
        let plan = DepositPlan {
            journey_id: JourneyId::new("jrn_depositcrash")
                .unwrap_or_else(|error| panic!("journey id: {error}")),
            idempotency_key: [0x61; 32],
            wallet,
            network,
            vault: runtime.vault,
            asset: AssetId::new(ASSET),
            amount: Amount::from_u128(AMOUNT),
            recipient,
            reserve,
            currency: "LXP".to_owned(),
            agent: DepositAgentPlan {
                actor: AgentDid::new("did:layerx:deposit-recipient")
                    .unwrap_or_else(|error| panic!("agent DID: {error:?}")),
                authority: AuthorityRef::new("owner:deposit-owner")
                    .unwrap_or_else(|error| panic!("authority: {error:?}")),
                account_sequence: 5,
                not_before: 995,
                not_after: 1_010,
                fee_limit: 7,
                custody_key,
            },
        };
        let agent_contract = AgentClient::daemon(
            "/run/layerx-agentd.sock",
            layerx_agent_api::agent_api_schema_v1().version,
        )
        .unwrap_or_else(|error| panic!("agent SDK: {error:?}"));
        Self {
            root,
            store_root,
            tenancy_digest,
            principal,
            signer,
            binding,
            agent_contract,
            trace: TraceId::mint([0x44; 16]),
            plan,
        }
    }

    fn store(&self) -> PrincipalStore {
        PrincipalStore::open(
            &self.store_root,
            retention_uniform(10_000),
            self.tenancy_digest,
        )
        .unwrap_or_else(|error| panic!("principal store: {error}"))
    }

    fn reopen(&self) -> (PrincipalStore, DepositJourney) {
        let mut store = self.store();
        let scope = store
            .principal(&self.principal)
            .unwrap_or_else(|error| panic!("reopen scope: {error}"));
        let journey = DepositJourney::load(&scope, &self.plan.journey_id)
            .unwrap_or_else(|error| panic!("load deposit: {error}"))
            .unwrap_or_else(|| panic!("deposit missing"));
        drop(scope);
        (store, journey)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn real_deposit_resumes_every_stage_with_one_custody_and_credit_effect() {
    let mut runtime = RealDepositRuntime::new();
    let fixture = Fixture::new(&runtime);
    let mut store = fixture.store();
    let mut scope = store
        .principal(&fixture.principal)
        .unwrap_or_else(|error| panic!("start scope: {error}"));
    let journey = DepositJourney::start(&mut scope, &fixture.binding, &fixture.plan, 200)
        .unwrap_or_else(|error| panic!("start deposit: {error}"));
    let repeated = DepositJourney::start(&mut scope, &fixture.binding, &fixture.plan, 200)
        .unwrap_or_else(|error| panic!("repeat deposit: {error}"));
    assert_eq!(journey, repeated);
    assert!(matches!(
        journey
            .status()
            .unwrap_or_else(|error| panic!("initial status: {error}"))
            .stage(),
        DepositStage::WaitingForWallet
    ));
    assert_eq!(
        journey
            .status()
            .unwrap_or_else(|error| panic!("initial status: {error}"))
            .in_flight_amount(),
        Some(AMOUNT)
    );
    drop(scope);
    drop(store);

    let (mut store, mut journey) = fixture.reopen();
    let mut credit_agent = RealCreditAgent::new(&fixture.root.join("credit-agent"));
    for offset in 0..40_u64 {
        let mut scope = store
            .principal(&fixture.principal)
            .unwrap_or_else(|error| panic!("drive scope: {error}"));
        let result = ready(journey.advance(
            &mut scope,
            &mut runtime,
            &fixture.agent_contract,
            &mut credit_agent,
            &fixture.signer,
            &registry(),
            &fixture.trace,
            201 + offset,
        ));
        if let Err(error) = &result {
            let text = error.to_string();
            assert!(
                text.contains("boundary failure") || text.contains("agent boundary failure"),
                "unexpected retryable error: {error}"
            );
        }
        drop(scope);
        drop(store);
        (store, journey) = fixture.reopen();
        let status = journey
            .status()
            .unwrap_or_else(|error| panic!("deposit status: {error}"));
        if matches!(status.stage(), DepositStage::Done) {
            assert_eq!(status.in_flight_amount(), None);
            let activity = status
                .activity()
                .unwrap_or_else(|| panic!("joined activity missing"));
            assert_ne!(activity.proof_commitment, [0; 32]);
            assert_ne!(activity.credit_receipt_digest, [0; 32]);
            break;
        }
        assert_eq!(status.in_flight_amount(), Some(AMOUNT));
        assert!(offset != 39, "deposit did not complete");
    }
    assert_eq!(runtime.wallet_opens, 1);
    assert_eq!(runtime.wallet_actions.len(), 1);
    assert_eq!(credit_agent.effects.len(), 1);
    assert_eq!(credit_agent.effects.values().next(), Some(&1));
    let final_status = journey
        .status()
        .unwrap_or_else(|error| panic!("final status: {error}"));
    assert!(matches!(final_status.stage(), DepositStage::Done));
    let final_scope = store
        .principal(&fixture.principal)
        .unwrap_or_else(|error| panic!("notification scope: {error}"));
    let notification = DepositJourney::notification(&final_scope, &fixture.plan.journey_id)
        .unwrap_or_else(|error| panic!("notification: {error}"))
        .unwrap_or_else(|| panic!("terminal notification missing"));
    assert!(notification.completed);
    assert_eq!(notification.deep_link, "/app/journeys/jrn_depositcrash");
}

#[test]
fn wallet_rejection_is_terminal_typed_and_actionable_without_a_custody_effect() {
    let mut runtime = RealDepositRuntime::new();
    runtime.reject_wallet = true;
    runtime.fail_wallet_ack_once = false;
    let fixture = Fixture::new(&runtime);
    let mut store = fixture.store();
    let mut scope = store
        .principal(&fixture.principal)
        .unwrap_or_else(|error| panic!("scope: {error}"));
    let mut journey = DepositJourney::start(&mut scope, &fixture.binding, &fixture.plan, 200)
        .unwrap_or_else(|error| panic!("start: {error}"));
    let mut agent = RealCreditAgent::new(&fixture.root.join("unused-credit-agent"));
    let _ = ready(journey.advance(
        &mut scope,
        &mut runtime,
        &fixture.agent_contract,
        &mut agent,
        &fixture.signer,
        &registry(),
        &fixture.trace,
        201,
    ));
    let status = ready(journey.advance(
        &mut scope,
        &mut runtime,
        &fixture.agent_contract,
        &mut agent,
        &fixture.signer,
        &registry(),
        &fixture.trace,
        202,
    ))
    .unwrap_or_else(|error| panic!("reject: {error}"));
    assert!(matches!(
        status.stage(),
        DepositStage::Failed(layerx_human_service::journeys::DepositFailureKind::WalletRejected)
    ));
    assert_eq!(status.in_flight_amount(), None);
    assert_eq!(runtime.wallet_opens, 0);
    assert!(runtime.wallet_actions.is_empty());
    let notification = DepositJourney::notification(&scope, &fixture.plan.journey_id)
        .unwrap_or_else(|error| panic!("notification: {error}"))
        .unwrap_or_else(|| panic!("failure notification missing"));
    assert!(!notification.completed);
    assert_eq!(notification.failure.as_deref(), Some("wallet-rejected"));
    assert_eq!(notification.deep_link, "/app/journeys/jrn_depositcrash");
}

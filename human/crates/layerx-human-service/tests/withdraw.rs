use layerx_human_test_support as support;

#[path = "support/withdraw_native.rs"]
mod withdraw_native;

mod paxeer_real {
    include!("../../layerx-paxeer-client/tests/withdraw.rs");

    use layerx_human_service::journeys::WithdrawalTransactionRequest;

    pub(super) struct GenesisChain {
        _anvil: Anvil,
        pub configuration: serde_json::Value,
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
            let (_, _, bond, registry, _, _) =
                deploy_suite_for_genesis(&anvil, 3, super::ASSET, super::NETWORK_ID, genesis);
            let selector = sha3::Keccak256::digest(b"latestFinalisedStateRoot()");
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

    pub(super) struct JourneyChain {
        anvil: Anvil,
        boundary: WithdrawalBoundary,
        proof: CheckpointProof,
        challenge_manager: EvmAddress,
        token: EvmAddress,
        vault: EvmAddress,
        recipient: EvmAddress,
        checkpoint_hash: [u8; 32],
    }

    impl JourneyChain {
        pub(super) fn new(expectation: DebitExpectation) -> Self {
            let anvil = Anvil::launch();
            let (token, vault, bond, checkpoint_registry, challenge_manager, claims) =
                deploy_suite_for_asset_network(
                    &anvil,
                    3,
                    expectation.asset_id,
                    expectation.network_id,
                );
            let leaf = withdrawal_leaf(expectation);
            let timestamp_ms = anvil
                .latest_timestamp()
                .checked_mul(1_000)
                .unwrap_or_else(|| panic!("latest block timestamp exceeds canonical milliseconds"));
            let mut header = checkpoint_header(leaf, timestamp_ms);
            header.network_id = expectation.network_id;
            header.protocol_version = 3;
            let checkpoint_hash = checkpoint_hash(&header);
            let attestation = signed_attestation(&header, checkpoint_hash, bond);
            anvil.send_checked(
                FUNDED,
                checkpoint_registry,
                &register_checkpoint_calldata(&header, &attestation),
                0,
            );
            let boundary = WithdrawalBoundary::new_for_protocol(
                WithdrawalConfig {
                    endpoints: vec![anvil.endpoint.clone()],
                    minimum_endpoint_agreement: 1,
                    claims_contract: claims,
                    required_confirmations: 2,
                    poll_cadence: Duration::from_millis(20),
                    delayed_after_polls: 100,
                },
                3,
            )
            .unwrap_or_else(|error| panic!("withdrawal boundary: {error:?}"));
            let proof = CheckpointProof {
                native: None,
                checkpoint_hash,
                state_root: leaf,
                epoch: header.epoch,
                batch_number: header.batch_number,
                data_availability_root: header.data_availability_root,
                leaf_index: 0,
                siblings: Vec::new(),
                attestations: vec![attestation],
            };
            Self {
                anvil,
                boundary,
                proof,
                challenge_manager,
                token,
                vault,
                recipient: expectation.recipient,
                checkpoint_hash,
            }
        }

        pub(super) fn boundary(&self) -> &WithdrawalBoundary {
            &self.boundary
        }

        pub(super) fn proof(&self) -> CheckpointProof {
            self.proof.clone()
        }

        pub(super) fn send(&self, request: &WithdrawalTransactionRequest) -> TransactionHash {
            self.anvil
                .send(FUNDED, Some(request.target), &request.calldata, 0)
        }

        pub(super) fn mine(&self) {
            self.anvil.mine();
        }

        pub(super) fn advance(&self, seconds: u64) {
            self.anvil.advance(seconds);
        }

        pub(super) fn raise_challenge(&self, evidence_hash: [u8; 32]) {
            self.anvil.send_checked(
                CHALLENGER,
                self.challenge_manager,
                &call_data(
                    RAISE_CHALLENGE,
                    &[
                        self.checkpoint_hash,
                        quantity_word(&1_u8.to_be_bytes()),
                        evidence_hash,
                    ],
                ),
                1,
            );
        }

        pub(super) fn uphold_challenge(&self) {
            self.anvil.send_checked(
                FUNDED,
                self.challenge_manager,
                &call_data(RESOLVE_CHALLENGE, &[self.checkpoint_hash, bool_word(true)]),
                0,
            );
        }

        pub(super) fn recipient_balance(&self) -> u128 {
            self.anvil.token_balance(self.token, self.recipient)
        }

        pub(super) fn vault_balance(&self) -> u128 {
            self.anvil.token_balance(self.token, self.vault)
        }
    }
}

use std::collections::BTreeMap;
use std::fs;
use std::future::Future;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

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
    CancelledFundsDisposition, ChallengeKind, CheckpointProof, DebitExpectation,
    PaxeerFundsDisposition, ProtocolDebitDisposition, TransactionHash,
};
use layerx_sdk::{Call, Client as AgentClient};
use layerx_types::account::AccountId;
use layerx_types::activity::{Authority, TimestampBound};
use layerx_types::amount::Amount;
use layerx_types::ids::{AssetId, Did, IdempotencyKey};
use layerx_types::intent::{EvmAddress, NetworkId};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use sha2::{Digest as _, Sha256};

use paxeer_real::JourneyChain;
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
}

impl RealWithdrawalAgent {
    fn new(fixture: &Fixture) -> Self {
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
    proof_available: bool,
    transactions: BTreeMap<[u8; 32], TransactionHash>,
    action_counts: BTreeMap<PaxeerAction, u32>,
    crash_after_broadcast: Option<PaxeerAction>,
}

impl RealRuntime {
    fn new(expectation: DebitExpectation) -> Self {
        Self {
            chain: JourneyChain::new(expectation),
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

    fn checkpoint_proof(
        &mut self,
        _debit: &DebitExpectation,
    ) -> Result<Option<CheckpointProof>, WithdrawalBoundaryError> {
        Ok(self.proof_available.then(|| self.chain.proof()))
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
    let mut agent = RealWithdrawalAgent::new(&fixture);
    let mut runtime = RealRuntime::new(fixture.expectation([0x31; 32]));
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
    assert_eq!(runtime.chain.vault_balance(), 100 - AMOUNT);
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

#[test]
fn real_challenge_hold_and_cancellation_report_actual_funds_disposition() {
    let fixture = Fixture::new("withdrawcancel");
    let mut agent = RealWithdrawalAgent::new(&fixture);
    let mut runtime = RealRuntime::new(fixture.expectation([0x31; 32]));
    let (mut store, mut journey, mut now) = drive_claim_queued(&fixture, &mut runtime, &mut agent);
    runtime.chain.raise_challenge([0x91; 32]);
    now += 1;
    (store, journey) = advance_once(&fixture, &mut runtime, &mut agent, store, journey, now);
    let hold = match journey
        .status()
        .unwrap_or_else(|error| panic!("status: {error}"))
        .stage()
    {
        WithdrawalStage::ChallengeHeld(hold) => *hold,
        stage => panic!("expected challenge hold, got {stage:?}"),
    };
    assert_eq!(hold.kind, ChallengeKind::DataAvailability);
    assert!(hold.resolution_has_no_on_chain_deadline);
    runtime.chain.uphold_challenge();
    now += 1;
    (store, journey) = advance_once(&fixture, &mut runtime, &mut agent, store, journey, now);
    let expected = CancelledFundsDisposition {
        paxeer: PaxeerFundsDisposition::RetainedInVault {
            vault: match journey
                .status()
                .unwrap_or_else(|error| panic!("status: {error}"))
                .stage()
            {
                WithdrawalStage::ChallengeUpheldAwaitingCancellation { disposition } => {
                    let PaxeerFundsDisposition::RetainedInVault { vault, .. } = disposition.paxeer;
                    vault
                }
                stage => panic!("expected cancellation state, got {stage:?}"),
            },
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
    assert!(matches!(
        journey
            .status()
            .unwrap_or_else(|error| panic!("status: {error}"))
            .stage(),
        WithdrawalStage::ChallengeUpheldAwaitingCancellation { disposition } if *disposition == expected
    ));
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
    assert_eq!(runtime.chain.vault_balance(), 100);
    assert_eq!(
        runtime
            .action_counts
            .get(&PaxeerAction::CancelChallengedPayout),
        Some(&1)
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

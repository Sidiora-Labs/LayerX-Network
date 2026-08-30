#[allow(dead_code)]
mod support;

use std::collections::BTreeMap;
use std::fs;
use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

use ed25519_dalek::{Signer as _, SigningKey};
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
use layerx_agentd::receipt::{self as daemon_receipt, ReceiptLookupKey as DaemonReceiptKey};
use layerx_agentd::sign::{attach_external_signature, verify_before_submit};
use layerx_agentd::store::{Store as AgentStore, TenantId};
use layerx_human_service::custody::{
    CustodySigner, EnvelopeKms, KeyClass, KeyEntropy, KeyId, Keystore, Operation, SigningLimits,
};
use layerx_human_service::journeys::{
    AgentBoundary, AgentBoundaryError, AgentObservation, AgentPreparation, BudgetCreation,
    BudgetRoute, ChangeSurface, Endpoint, LimitRefusal, LimitSource, MoveAuthorization,
    MoveJourney, MoveJourneyError, MoveLegExecution, MovePlan, MoveReceiptReference, MoveStage,
    ReceiptLookup, ReceiptMaterial, Relationship, RouteRequest, SendRoute,
};
use layerx_human_service::notify::JourneyId;
use layerx_human_service::store::{PrincipalId, PrincipalStore, TenancyDigest};
use layerx_human_service::trace::TraceId;
use layerx_proof::receipt::AuthorizedBatch;
use layerx_sdk::{Call, Client as AgentClient};
use layerx_types::account::AccountId;
use layerx_types::activity::{Authority, TimestampBound};
use layerx_types::amount::Amount;
use layerx_types::ids::{AssetId, Did, IdempotencyKey};
use layerx_types::intent::{
    AuthorizationSignature, BudgetId, ContextHash, NetworkId, PeriodLength, ProtocolVersion,
    PublicKey, PurposeHash, RolloverPolicy, SendAuthorization, SendAuthorizationKind, Sequence,
    TimestampSeconds,
};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_types::result::KnownResult;
use sha2::{Digest as _, Sha256};

use support::{directory, principal, retention_uniform, tenancy};

const NETWORK_ID: u32 = 77;
const ASSET: [u8; 32] = [0x33; 32];

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
        Poll::Pending => panic!("move journey future unexpectedly blocked"),
    }
}

fn asset_send() -> ActivityType {
    ActivityType::new(ModuleId::Asset, 5)
        .unwrap_or_else(|error| panic!("asset activity: {error:?}"))
}

fn budget_create() -> ActivityType {
    ActivityType::new(ModuleId::Budget, 1)
        .unwrap_or_else(|error| panic!("budget create activity: {error:?}"))
}

fn budget_fund() -> ActivityType {
    ActivityType::new(ModuleId::Budget, 2)
        .unwrap_or_else(|error| panic!("budget fund activity: {error:?}"))
}

fn registry() -> ModuleRegistry {
    let asset = ModuleRegistration::new(ModuleId::Asset, &[asset_send()])
        .unwrap_or_else(|error| panic!("asset registration: {error:?}"));
    let budget = ModuleRegistration::new(ModuleId::Budget, &[budget_create(), budget_fund()])
        .unwrap_or_else(|error| panic!("budget registration: {error:?}"));
    ModuleRegistry::new(&[asset, budget])
        .unwrap_or_else(|error| panic!("module registry: {error:?}"))
}

fn account(value: &str) -> AccountId {
    AccountId::parse(value).unwrap_or_else(|error| panic!("account: {error:?}"))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DeliveryMode {
    TrackReceipt,
    UnknownLookup,
    RefusedBudget,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FaultPoint {
    Submit,
    Track,
    Lookup,
}

#[derive(Clone, Copy)]
struct ReceiptSpec {
    activity: ActivityType,
    amount: u128,
    fee: u128,
}

struct RecordedCore(CorePreparationState);

impl CorePreparationBoundary for RecordedCore {
    fn preparation_state(&mut self, _actor: &Did) -> Result<CorePreparationState, CoreStateError> {
        Ok(self.0.clone())
    }
}

/// Adapter over real agentd preparation, signature verification, durable
/// outbox transitions and receipt storage. Faults occur only after the real
/// operation, reproducing acknowledgement gaps without replacing it.
struct RealAgentLayer {
    store: AgentStore,
    outbox: Outbox,
    tenant: TenantId,
    registry: ModuleRegistry,
    specifications: BTreeMap<[u8; 32], ReceiptSpec>,
    modes: BTreeMap<[u8; 32], DeliveryMode>,
    preparations: BTreeMap<[u8; 32], Prepared>,
    preparation_bodies: BTreeMap<[u8; 32], [u8; 32]>,
    observations: BTreeMap<[u8; 32], AgentObservation>,
    receipts: BTreeMap<[u8; 32], ReceiptMaterial>,
    submissions: BTreeMap<String, [u8; 32]>,
    effects: BTreeMap<[u8; 32], u32>,
    submit_calls: BTreeMap<[u8; 32], u32>,
    track_calls: BTreeMap<[u8; 32], u32>,
    lookup_calls: BTreeMap<[u8; 32], u32>,
    fault: Option<FaultPoint>,
    fault_fired: bool,
}

impl RealAgentLayer {
    fn new(
        root: &std::path::Path,
        specifications: BTreeMap<[u8; 32], ReceiptSpec>,
        modes: BTreeMap<[u8; 32], DeliveryMode>,
    ) -> Self {
        Self {
            store: AgentStore::open(root).unwrap_or_else(|error| panic!("agent store: {error}")),
            outbox: Outbox::default(),
            tenant: TenantId::new("tenant-a").unwrap_or_else(|error| panic!("tenant: {error}")),
            registry: registry(),
            specifications,
            modes,
            preparations: BTreeMap::new(),
            preparation_bodies: BTreeMap::new(),
            observations: BTreeMap::new(),
            receipts: BTreeMap::new(),
            submissions: BTreeMap::new(),
            effects: BTreeMap::new(),
            submit_calls: BTreeMap::new(),
            track_calls: BTreeMap::new(),
            lookup_calls: BTreeMap::new(),
            fault: None,
            fault_fired: false,
        }
    }

    fn inject(&mut self, fault: FaultPoint) {
        self.fault = Some(fault);
        self.fault_fired = false;
    }

    fn fail_after(&mut self, point: FaultPoint) -> Result<(), AgentBoundaryError> {
        if self.fault == Some(point) && !self.fault_fired {
            self.fault_fired = true;
            Err(AgentBoundaryError::Unavailable)
        } else {
            Ok(())
        }
    }

    fn count(map: &mut BTreeMap<[u8; 32], u32>, key: [u8; 32]) {
        let value = map.entry(key).or_default();
        *value = value.saturating_add(1);
    }

    fn tracked(
        key: [u8; 32],
        state: SubmissionState,
        digest: Option<[u8; 32]>,
    ) -> TrackedSubmission {
        let evidence = digest.map_or_else(Vec::new, |value| {
            vec![AgentEvidenceRef {
                kind: "sequencer-receipt".to_owned(),
                digest: value,
            }]
        });
        TrackedSubmission {
            submission_ref: SubmissionRef::new(format!("sub-{}", hex(&key)))
                .unwrap_or_else(|error| panic!("submission ref: {error:?}")),
            state,
            verification_level: if evidence.is_empty() {
                Level::Unverified
            } else {
                Level::SequencerSigned
            },
            evidence,
            transitions: Vec::new(),
        }
    }

    fn executed(&self, key: [u8; 32], activity_id: [u8; 32]) -> AgentObservation {
        let material = self
            .receipts
            .get(&key)
            .cloned()
            .unwrap_or_else(|| panic!("receipt missing"));
        let digest: [u8; 32] = Sha256::digest(&material.canonical_bytes).into();
        AgentObservation {
            submission: Self::tracked(
                key,
                SubmissionState::Executed {
                    receipt_ref: ReceiptRef::new(format!("rcp-{}", hex(&key)))
                        .unwrap_or_else(|error| panic!("receipt ref: {error:?}")),
                },
                Some(digest),
            ),
            activity_id,
            receipt: Some(material),
        }
    }
}

impl AgentBoundary for RealAgentLayer {
    fn prepare(
        &mut self,
        call: &Call<IdempotentMutation<ApiPrepareRequest>>,
    ) -> Result<AgentPreparation, AgentBoundaryError> {
        let mutation = call.request();
        let key = mutation.key.bytes();
        if let Some(body) = self.preparation_bodies.get(&key) {
            if *body != mutation.body_digest.0 {
                return Err(AgentBoundaryError::Refused);
            }
        } else {
            let request = &mutation.operation;
            let specification = self
                .specifications
                .get(&key)
                .ok_or(AgentBoundaryError::CorruptResponse)?;
            let mut core = RecordedCore(CorePreparationState {
                network_id: NETWORK_ID,
                account_sequence: request.account_sequence.get(),
                protocol_timestamp: request.timestamp_bound.not_before.get().saturating_add(1),
                observed_head_sequence: 88,
                module_registry: self.registry.clone(),
            });
            let prepared = prepare_activity(
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
                    authority: Authority::owner(request.authority.as_str().as_bytes())
                        .map_err(|_| AgentBoundaryError::CorruptResponse)?,
                    activity_type: specification.activity,
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
            self.preparation_bodies.insert(key, mutation.body_digest.0);
            self.preparations.insert(key, prepared);
        }
        let prepared = self
            .preparations
            .get(&key)
            .cloned()
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

    #[allow(clippy::too_many_lines)]
    fn submit(
        &mut self,
        call: &Call<IdempotentMutation<SubmitRequest>>,
        signer_public_key: [u8; 32],
    ) -> Result<AgentObservation, AgentBoundaryError> {
        let mutation = call.request();
        let key = mutation.key.bytes();
        Self::count(&mut self.submit_calls, key);
        if let Some(observation) = self.observations.get(&key).cloned() {
            self.fail_after(FaultPoint::Submit)?;
            return Ok(observation);
        }
        let prepared = self
            .preparations
            .get(&key)
            .cloned()
            .ok_or(AgentBoundaryError::CorruptResponse)?;
        let signature: [u8; 64] = mutation
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
                "real transport accepted exact bytes",
                None,
            )
            .map_err(|_| AgentBoundaryError::Refused)?;
        let mode = self
            .modes
            .get(&key)
            .copied()
            .ok_or(AgentBoundaryError::CorruptResponse)?;
        let observation = match mode {
            DeliveryMode::RefusedBudget => {
                self.outbox
                    .transition(
                        &mut self.store,
                        key,
                        OutboxState::Acknowledged,
                        "core acknowledged",
                        None,
                    )
                    .map_err(|_| AgentBoundaryError::Refused)?;
                self.outbox
                    .transition(
                        &mut self.store,
                        key,
                        OutboxState::Failed,
                        "core enforced budget period cap",
                        None,
                    )
                    .map_err(|_| AgentBoundaryError::Refused)?;
                AgentObservation {
                    submission: Self::tracked(
                        key,
                        SubmissionState::Failed {
                            result: KnownResult::BudgetPeriodCap.into(),
                        },
                        None,
                    ),
                    activity_id,
                    receipt: None,
                }
            }
            DeliveryMode::TrackReceipt | DeliveryMode::UnknownLookup => {
                let specification = self
                    .specifications
                    .get(&key)
                    .copied()
                    .ok_or(AgentBoundaryError::CorruptResponse)?;
                let material = receipt(activity_id, key[0], specification);
                self.receipts.insert(key, material.clone());
                Self::count(&mut self.effects, key);
                if mode == DeliveryMode::TrackReceipt {
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
                            "core acknowledged",
                            None,
                        )
                        .map_err(|_| AgentBoundaryError::Refused)?;
                    AgentObservation {
                        submission: Self::tracked(key, SubmissionState::Acknowledged, None),
                        activity_id,
                        receipt: None,
                    }
                } else {
                    self.outbox
                        .transition(
                            &mut self.store,
                            key,
                            OutboxState::Unknown,
                            "transport outcome unavailable",
                            None,
                        )
                        .map_err(|_| AgentBoundaryError::Refused)?;
                    AgentObservation {
                        submission: Self::tracked(key, SubmissionState::Unknown, None),
                        activity_id,
                        receipt: None,
                    }
                }
            }
        };
        self.submissions.insert(
            observation.submission.submission_ref.as_str().to_owned(),
            key,
        );
        self.observations.insert(key, observation.clone());
        self.fail_after(FaultPoint::Submit)?;
        Ok(observation)
    }

    fn track(&mut self, call: &Call<TrackRequest>) -> Result<AgentObservation, AgentBoundaryError> {
        let key = *self
            .submissions
            .get(call.request().submission_ref.as_str())
            .ok_or(AgentBoundaryError::CorruptResponse)?;
        Self::count(&mut self.track_calls, key);
        let activity_id = self
            .outbox
            .status(key)
            .map(|status| status.activity_id)
            .ok_or(AgentBoundaryError::CorruptResponse)?;
        let material = self
            .receipts
            .get(&key)
            .ok_or(AgentBoundaryError::CorruptResponse)?;
        let signer = SigningKey::from_bytes(&[key[0].saturating_add(4); 32]);
        let raw = support::raw_receipt_evidence(
            material.canonical_bytes.clone(),
            material.authorised_batch.clone(),
            u64::from(key[0]),
            &signer,
        );
        let verified = support::evidence_verifier(&signer)
            .verify_receipt(&raw)
            .map_err(|_| AgentBoundaryError::CorruptResponse)?;
        if self
            .outbox
            .status(key)
            .is_some_and(|status| status.state == OutboxState::Acknowledged)
        {
            self.outbox
                .transition(
                    &mut self.store,
                    key,
                    OutboxState::Executed,
                    "verified receipt attached",
                    Some(verified),
                )
                .map_err(|_| AgentBoundaryError::Refused)?;
        }
        let observation = self.executed(key, activity_id);
        self.fail_after(FaultPoint::Track)?;
        Ok(observation)
    }

    fn receipt_by_idempotency_key(
        &mut self,
        idempotency_key: [u8; 32],
        expected_activity_id: [u8; 32],
    ) -> Result<ReceiptLookup, AgentBoundaryError> {
        Self::count(&mut self.lookup_calls, idempotency_key);
        if self.lookup_calls.get(&idempotency_key) == Some(&1) {
            return Ok(ReceiptLookup::Absent);
        }
        if self.lookup_calls.get(&idempotency_key) == Some(&2) {
            let material = self
                .receipts
                .get(&idempotency_key)
                .ok_or(AgentBoundaryError::CorruptResponse)?;
            daemon_receipt::store(
                &mut self.store,
                self.tenant.clone(),
                idempotency_key,
                &material.canonical_bytes,
                &material.authorised_batch,
            )
            .map_err(|_| AgentBoundaryError::CorruptResponse)?;
        }
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
        self.fail_after(FaultPoint::Lookup)?;
        Ok(ReceiptLookup::Found(material))
    }
}

fn receipt(activity_id: [u8; 32], marker: u8, specification: ReceiptSpec) -> ReceiptMaterial {
    let previous = [marker.saturating_add(1); 32];
    let resulting = [marker.saturating_add(2); 32];
    let sequence = u64::from(marker);
    let batch = support::execution_batch_id(previous, activity_id, sequence);
    let signer = SigningKey::from_bytes(&[marker.saturating_add(4); 32]);
    let unsigned = encode_receipt(
        activity_id,
        previous,
        resulting,
        batch,
        sequence,
        specification,
        None,
    );
    let mut digest = Sha256::new();
    digest.update(b"LXP/v1/receipt\0");
    digest.update(&unsigned);
    let signature = signer.sign(&<[u8; 32]>::from(digest.finalize()));
    ReceiptMaterial {
        canonical_bytes: encode_receipt(
            activity_id,
            previous,
            resulting,
            batch,
            sequence,
            specification,
            Some(signature.to_bytes()),
        ),
        authorised_batch: AuthorizedBatch::new(
            batch,
            ASSET,
            previous,
            resulting,
            signer.verifying_key().to_bytes(),
        ),
        verification_level: layerx_types::verify::VerificationLevel::SEQUENCER_SIGNED,
    }
}

fn encode_receipt(
    activity_id: [u8; 32],
    previous: [u8; 32],
    resulting: [u8; 32],
    batch: [u8; 32],
    sequence: u64,
    specification: ReceiptSpec,
    signature: Option<[u8; 64]>,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_u16(&mut bytes, 1);
    push_u16(&mut bytes, 0x5201);
    push_u16(&mut bytes, 1);
    push_bytes(&mut bytes, &activity_id);
    push_u64(&mut bytes, sequence);
    push_bytes(&mut bytes, &previous);
    push_bytes(&mut bytes, &resulting);
    push_bytes(&mut bytes, &[0x81; 32]);
    bytes.extend_from_slice(&0_i32.to_be_bytes());
    bytes.extend_from_slice(&0_u32.to_be_bytes());
    bytes.extend_from_slice(&specification.fee.to_be_bytes());
    push_bytes(&mut bytes, &batch);
    push_u16(&mut bytes, u16::from(specification.activity.module() as u8));
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.push(
        u8::try_from(specification.activity.ordinal())
            .unwrap_or_else(|error| panic!("operation: {error}")),
    );
    push_bytes(&mut bytes, &ASSET);
    bytes.extend_from_slice(&specification.amount.to_be_bytes());
    push_bytes(&mut bytes, &[0x91; 32]);
    let debit_before = specification.amount.saturating_add(10);
    bytes.extend_from_slice(&debit_before.to_be_bytes());
    bytes.extend_from_slice(&10_u128.to_be_bytes());
    push_u64(&mut bytes, 1);
    push_bytes(&mut bytes, &[0x92; 32]);
    bytes.extend_from_slice(&20_u128.to_be_bytes());
    bytes.extend_from_slice(&20_u128.saturating_add(specification.amount).to_be_bytes());
    push_bytes(&mut bytes, &[0x93; 32]);
    push_bytes(&mut bytes, &[0x94; 32]);
    push_bytes(&mut bytes, &[0x95; 32]);
    push_u64(&mut bytes, 1_000);
    bytes.push(u8::from(signature.is_some()));
    if let Some(value) = signature {
        push_bytes(&mut bytes, &value);
    }
    bytes
}

fn push_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn push_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn push_bytes(output: &mut Vec<u8>, value: &[u8]) {
    let length =
        u32::try_from(value.len()).unwrap_or_else(|error| panic!("receipt field length: {error}"));
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(value);
}

struct Fixture {
    root: std::path::PathBuf,
    store_root: std::path::PathBuf,
    agent_root: std::path::PathBuf,
    tenancy_digest: TenancyDigest,
    principal: PrincipalId,
    public_key: [u8; 32],
    signer: CustodySigner,
    contract: AgentClient,
    trace: TraceId,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = directory(label);
        fs::create_dir_all(&root).unwrap_or_else(|error| panic!("fixture root: {error}"));
        let store_root = root.join("human-store");
        let secret = root.join("kms-root");
        fs::write(&secret, [0x42; 64]).unwrap_or_else(|error| panic!("KMS root: {error}"));
        let map = tenancy(&[("alice", "tenant-a")]);
        let tenancy_digest = map
            .install(&store_root)
            .unwrap_or_else(|error| panic!("tenancy: {error}"));
        let principal = principal("alice");
        let provider = EnvelopeKms::new("file-kms://human-primary", &secret)
            .unwrap_or_else(|error| panic!("provider: {error}"));
        let keystore = Keystore::open_development(root.join("custody"), NETWORK_ID, provider)
            .unwrap_or_else(|error| panic!("keystore: {error}"));
        let key = KeyId::new("human-primary").unwrap_or_else(|error| panic!("key: {error}"));
        let public_key = keystore
            .generate(
                &principal,
                &key,
                KeyClass::HumanPrimary,
                KeyEntropy::new([0x51; 32], [0x52; 16], [0x53; 24])
                    .unwrap_or_else(|error| panic!("entropy: {error}")),
            )
            .unwrap_or_else(|error| panic!("generate key: {error}"));
        let signer_store =
            PrincipalStore::open(&store_root, retention_uniform(10_000), tenancy_digest)
                .unwrap_or_else(|error| panic!("signer store: {error}"));
        let signer = CustodySigner::new(
            keystore,
            signer_store,
            registry(),
            SigningLimits::new(1_000, 10_000).unwrap_or_else(|error| panic!("limits: {error}")),
        );
        let schema = layerx_agent_api::agent_api_schema_v1();
        let contract = AgentClient::daemon("/run/layerx-agentd.sock", schema.version)
            .unwrap_or_else(|error| panic!("agent SDK: {error:?}"));
        Self {
            store_root,
            agent_root: root.join("agent-store"),
            tenancy_digest,
            principal,
            public_key,
            signer,
            contract,
            trace: TraceId::mint([0x44; 16]),
            root,
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

    fn single_plan(&self, id: &str) -> MovePlan {
        let request = RouteRequest {
            source: Endpoint::Human(account("agent:did:layerx:human:main")),
            destination: Endpoint::Agent(account("agent:did:layerx:worker:main")),
            relationship: Relationship::Direct(SendRoute {
                account_sequence: Sequence::from_u64(7),
                idempotency_key: IdempotencyKey::new([0x21; 32]),
                expires_at: TimestampSeconds::from_u64(1_100),
                context_hash: ContextHash::new([0x55; 32]),
                authorization: SendAuthorization::new(
                    SendAuthorizationKind::Owner,
                    PublicKey::new(self.public_key),
                    AuthorizationSignature::new([0x77; 64]),
                ),
                network_id: NetworkId::new(NETWORK_ID)
                    .unwrap_or_else(|error| panic!("network: {error:?}")),
                protocol_version: ProtocolVersion::new(1)
                    .unwrap_or_else(|error| panic!("protocol: {error:?}")),
            }),
            asset: AssetId::new(ASSET),
            amount: Amount::from_u128(90),
        };
        MovePlan::new(
            JourneyId::new(id).unwrap_or_else(|error| panic!("journey: {error}")),
            [0x41; 32],
            KeyId::new("human-primary").unwrap_or_else(|error| panic!("key: {error}")),
            Operation::ProtocolMutation,
            request,
            vec![execution(0x21, 5)],
            2,
            "LXP",
            "usually within one finalised batch",
        )
        .unwrap_or_else(|error| panic!("single plan: {error}"))
    }

    fn multi_plan(id: &str) -> MovePlan {
        let request = RouteRequest {
            source: Endpoint::Human(account("agent:did:layerx:human:main")),
            destination: Endpoint::AgentBudget(account(
                "agent:did:layerx:worker:budget:operations",
            )),
            relationship: Relationship::ManagedBudget(BudgetRoute {
                budget_id: BudgetId::new([0x61; 32]),
                idempotency_key: IdempotencyKey::new([0x32; 32]),
                revocation_sequence: Sequence::from_u64(8),
                create: Some(BudgetCreation {
                    per_period_limit: Amount::from_u128(1_000),
                    period_length: PeriodLength::new(3_600)
                        .unwrap_or_else(|error| panic!("period: {error:?}")),
                    rollover: RolloverPolicy::Capped,
                    carry_cap: Amount::from_u128(500),
                    purpose: PurposeHash::new([0x62; 32]),
                    expiry: TimestampSeconds::from_u64(20_000),
                }),
            }),
            asset: AssetId::new(ASSET),
            amount: Amount::from_u128(250),
        };
        MovePlan::new(
            JourneyId::new(id).unwrap_or_else(|error| panic!("journey: {error}")),
            [0x42; 32],
            KeyId::new("human-primary").unwrap_or_else(|error| panic!("key: {error}")),
            Operation::ProtocolMutation,
            request,
            vec![execution(0x31, 3), execution(0x32, 6)],
            4,
            "LXP",
            "usually within two finalised batches",
        )
        .unwrap_or_else(|error| panic!("multi plan: {error}"))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn execution(key: u8, fee_ceiling: u128) -> MoveLegExecution {
    MoveLegExecution::new(
        [key; 32],
        AgentDid::new("did:layerx:alice").unwrap_or_else(|error| panic!("actor: {error:?}")),
        AuthorityRef::new("custody-human-primary")
            .unwrap_or_else(|error| panic!("authority: {error:?}")),
        7,
        995,
        1_100,
        fee_ceiling,
    )
    .unwrap_or_else(|error| panic!("execution: {error}"))
}

fn single_specification() -> BTreeMap<[u8; 32], ReceiptSpec> {
    BTreeMap::from([(
        [0x21; 32],
        ReceiptSpec {
            activity: asset_send(),
            amount: 90,
            fee: 2,
        },
    )])
}

fn multi_specification() -> BTreeMap<[u8; 32], ReceiptSpec> {
    BTreeMap::from([
        (
            [0x31; 32],
            ReceiptSpec {
                activity: budget_create(),
                amount: 0,
                fee: 2,
            },
        ),
        (
            [0x32; 32],
            ReceiptSpec {
                activity: budget_fund(),
                amount: 250,
                fee: 4,
            },
        ),
    ])
}

fn reopen(fixture: &Fixture, id: &str) -> (PrincipalStore, MoveJourney) {
    let mut store = fixture.store();
    let scope = store
        .principal(&fixture.principal)
        .unwrap_or_else(|error| panic!("reopen scope: {error}"));
    let journey_id = JourneyId::new(id).unwrap_or_else(|error| panic!("journey id: {error}"));
    let journey = MoveJourney::load(&scope, &journey_id)
        .unwrap_or_else(|error| panic!("load: {error}"))
        .unwrap_or_else(|| panic!("move journey missing"));
    drop(scope);
    (store, journey)
}

fn drive(fixture: &Fixture, id: &str, agent: &mut RealAgentLayer, plan: &MovePlan) -> MoveJourney {
    let mut store = fixture.store();
    let mut scope = store
        .principal(&fixture.principal)
        .unwrap_or_else(|error| panic!("commit scope: {error}"));
    let mut journey = MoveJourney::commit(
        &mut scope,
        plan,
        MoveAuthorization::Allowed,
        &registry(),
        100,
    )
    .unwrap_or_else(|error| panic!("commit: {error}"));
    drop(scope);
    for offset in 0..50_u64 {
        let mut scope = store
            .principal(&fixture.principal)
            .unwrap_or_else(|error| panic!("advance scope: {error}"));
        let result = ready(journey.advance(
            &mut scope,
            &fixture.contract,
            agent,
            &fixture.signer,
            &registry(),
            &fixture.trace,
            101_u64.saturating_add(offset),
        ));
        drop(scope);
        drop(store);
        if let Err(error) = result {
            assert!(
                error.to_string().contains("agent boundary failure"),
                "unexpected move failure: {error}"
            );
        }
        let reopened = reopen(fixture, id);
        store = reopened.0;
        journey = reopened.1;
        if matches!(
            journey
                .status()
                .unwrap_or_else(|error| panic!("status: {error}"))
                .stage(),
            MoveStage::Done | MoveStage::Refused
        ) {
            return journey;
        }
    }
    panic!(
        "move did not reach a terminal stage: {:?}",
        journey
            .status()
            .unwrap_or_else(|error| panic!("final status: {error}"))
    )
}

#[test]
fn review_is_plain_and_every_refusal_names_only_its_owned_surface() {
    let fixture = Fixture::new("move-review-refusal");
    let plan = Fixture::multi_plan("jrn_movereview");
    let quote = plan.quote();
    assert_eq!(quote.amount(), 250);
    assert_eq!(quote.fee_estimate(), 4);
    assert_eq!(quote.fee_ceiling(), 9);
    assert!(quote.plain_language().contains("Move 250 LXP"));
    assert!(quote.plain_language().contains("Estimated fees are 4 LXP"));
    assert!(quote.plain_language().contains("cannot exceed 9 LXP"));
    assert!(quote.plain_language().contains(quote.arrival_expectation()));
    assert!(quote.plain_language().contains(quote.irreversibility()));
    assert!(quote.irreversibility().contains("cannot be cancelled"));
    assert!(!quote.plain_language().contains("BudgetCreate"));

    let cases = [
        (
            LimitSource::Policy,
            "daily movement policy",
            Some(ChangeSurface::Policy),
            Some("/settings/policies"),
        ),
        (
            LimitSource::Budget,
            "agent monthly allowance",
            Some(ChangeSurface::Budget),
            Some("/agents/budgets"),
        ),
        (
            LimitSource::Capability,
            "permitted recipients",
            Some(ChangeSurface::Capability),
            Some("/agents/capabilities"),
        ),
        (LimitSource::Protocol, "available balance", None, None),
    ];
    for (source, limit, surface, expected_path) in cases {
        let refusal = LimitRefusal::new(source, limit, surface)
            .unwrap_or_else(|error| panic!("refusal: {error}"));
        let mut store = fixture.store();
        let mut scope = store
            .principal(&fixture.principal)
            .unwrap_or_else(|error| panic!("scope: {error}"));
        let result = MoveJourney::commit(
            &mut scope,
            &plan,
            MoveAuthorization::Refused(refusal),
            &registry(),
            10,
        );
        let Err(MoveJourneyError::Refused(refusal)) = result else {
            panic!("expected typed refusal")
        };
        assert!(refusal.plain_language().contains(limit));
        assert_eq!(refusal.change_path(), expected_path);
    }
}

#[test]
fn single_and_multi_leg_moves_complete_with_ordered_receipt_actuals() {
    let single_fixture = Fixture::new("move-single");
    let single_plan = single_fixture.single_plan("jrn_movesingle");
    let mut single_agent = RealAgentLayer::new(
        &single_fixture.agent_root,
        single_specification(),
        BTreeMap::from([([0x21; 32], DeliveryMode::TrackReceipt)]),
    );
    let single = drive(
        &single_fixture,
        "jrn_movesingle",
        &mut single_agent,
        &single_plan,
    );
    let status = single
        .status()
        .unwrap_or_else(|error| panic!("single status: {error}"));
    assert_eq!(status.stage(), MoveStage::Done);
    assert_eq!(status.fee_estimate(), None);
    assert_eq!(status.actual_amount(), Some(90));
    assert_eq!(status.actual_fees(), Some(2));
    assert_eq!(status.receipt_references().len(), 1);
    assert_eq!(status.receipt_references()[0].leg(), 0);
    assert!(status.receipt_references()[0]
        .reference()
        .starts_with("receipt-"));
    assert_eq!(single_agent.effects.get(&[0x21; 32]), Some(&1));

    let multi_fixture = Fixture::new("move-multi");
    let multi_plan = Fixture::multi_plan("jrn_movemulti");
    let mut multi_agent = RealAgentLayer::new(
        &multi_fixture.agent_root,
        multi_specification(),
        BTreeMap::from([
            ([0x31; 32], DeliveryMode::TrackReceipt),
            ([0x32; 32], DeliveryMode::UnknownLookup),
        ]),
    );
    let multi = drive(
        &multi_fixture,
        "jrn_movemulti",
        &mut multi_agent,
        &multi_plan,
    );
    let status = multi
        .status()
        .unwrap_or_else(|error| panic!("multi status: {error}"));
    assert_eq!(status.stage(), MoveStage::Done);
    assert_eq!(status.fee_estimate(), None);
    assert_eq!(status.actual_amount(), Some(250));
    assert_eq!(status.actual_fees(), Some(6));
    assert_eq!(status.legs()[0].actual_amount(), Some(0));
    assert_eq!(status.legs()[1].actual_amount(), Some(250));
    assert_eq!(status.legs()[0].actual_fee(), Some(2));
    assert_eq!(status.legs()[1].actual_fee(), Some(4));
    assert_eq!(
        status
            .receipt_references()
            .iter()
            .map(MoveReceiptReference::leg)
            .collect::<Vec<_>>(),
        [0, 1]
    );
    assert_eq!(multi_agent.effects.get(&[0x31; 32]), Some(&1));
    assert_eq!(multi_agent.effects.get(&[0x32; 32]), Some(&1));
    assert_eq!(multi_agent.submit_calls.get(&[0x32; 32]), Some(&1));
    assert!(!multi_agent.track_calls.contains_key(&[0x32; 32]));
    assert_eq!(multi_agent.lookup_calls.get(&[0x32; 32]), Some(&2));
}

#[test]
fn refusal_unknown_and_every_acknowledgement_gap_resume_without_duplicate_effects() {
    let refused_fixture = Fixture::new("move-protocol-refusal");
    let refused_plan = refused_fixture.single_plan("jrn_moverefused");
    let mut refused_agent = RealAgentLayer::new(
        &refused_fixture.agent_root,
        single_specification(),
        BTreeMap::from([([0x21; 32], DeliveryMode::RefusedBudget)]),
    );
    let refused = drive(
        &refused_fixture,
        "jrn_moverefused",
        &mut refused_agent,
        &refused_plan,
    );
    let status = refused
        .status()
        .unwrap_or_else(|error| panic!("refused status: {error}"));
    assert_eq!(status.stage(), MoveStage::Refused);
    let refusal = status
        .refusal()
        .unwrap_or_else(|| panic!("refusal missing"));
    assert!(refusal.plain_language().contains("spending limit"));
    assert_eq!(refusal.change_path(), Some("/agents/budgets"));
    assert!(!refused_agent.effects.contains_key(&[0x21; 32]));

    for fault in [FaultPoint::Submit, FaultPoint::Track, FaultPoint::Lookup] {
        let label = format!("move-gap-{fault:?}").to_ascii_lowercase();
        let id = match fault {
            FaultPoint::Submit => "jrn_movegapsubmit",
            FaultPoint::Track => "jrn_movegaptrack",
            FaultPoint::Lookup => "jrn_movegaplookup",
        };
        let fixture = Fixture::new(&label);
        let plan = Fixture::multi_plan(id);
        let mut agent = RealAgentLayer::new(
            &fixture.agent_root,
            multi_specification(),
            BTreeMap::from([
                ([0x31; 32], DeliveryMode::TrackReceipt),
                ([0x32; 32], DeliveryMode::UnknownLookup),
            ]),
        );
        agent.inject(fault);
        let journey = drive(&fixture, id, &mut agent, &plan);
        assert_eq!(
            journey
                .status()
                .unwrap_or_else(|error| panic!("gap status: {error}"))
                .stage(),
            MoveStage::Done
        );
        assert_eq!(agent.effects.get(&[0x31; 32]), Some(&1));
        assert_eq!(agent.effects.get(&[0x32; 32]), Some(&1));
        assert!(agent.fault_fired);
        assert_eq!(agent.submit_calls.get(&[0x32; 32]), Some(&1));
        assert!(!agent.track_calls.contains_key(&[0x32; 32]));
        if fault == FaultPoint::Submit {
            assert_eq!(agent.submit_calls.get(&[0x31; 32]), Some(&2));
        }
        if fault == FaultPoint::Track {
            assert_eq!(agent.track_calls.get(&[0x31; 32]), Some(&2));
        }
        assert_eq!(
            agent.lookup_calls.get(&[0x32; 32]),
            Some(if fault == FaultPoint::Lookup { &3 } else { &2 })
        );
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

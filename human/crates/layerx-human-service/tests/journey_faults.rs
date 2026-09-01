#[allow(dead_code)]
mod support;

use std::collections::{BTreeMap, BTreeSet};
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
    AgentBoundary, AgentBoundaryError, AgentObservation, AgentPreparation, JourneyEngine,
    JourneyLeg, JourneyPhase, JourneyPlan, JourneyProgress, JourneyState, ReceiptLookup,
    ReceiptMaterial,
};
use layerx_human_service::notify::JourneyId;
use layerx_human_service::store::{PrincipalId, PrincipalStore, TenancyDigest};
use layerx_human_service::trace::TraceId;
use layerx_intents::{Intent, IntentKind, LxpSend};
use layerx_proof::receipt::AuthorizedBatch;
use layerx_sdk::{Call, Client as AgentClient};
use layerx_types::account::AccountId;
use layerx_types::activity::{Authority, TimestampBound};
use layerx_types::amount::Amount;
use layerx_types::ids::{AssetId, Did, IdempotencyKey};
use layerx_types::intent::{
    AuthorizationSignature, ContextHash, NetworkId, ProtocolVersion, PublicKey, SendAuthorization,
    SendAuthorizationKind, Sequence, TimestampSeconds,
};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use sha2::{Digest as _, Sha256};

use support::{directory, principal, retention_uniform, tenancy};

const NETWORK_ID: u32 = 77;
const ACCOUNT_SEQUENCE: u64 = 7;

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

fn activity_type() -> ActivityType {
    ActivityType::new(ModuleId::Asset, 5).unwrap_or_else(|error| panic!("activity type: {error:?}"))
}

fn registry() -> ModuleRegistry {
    let asset = ModuleRegistration::new(
        ModuleId::Asset,
        &[
            activity_type(),
            ActivityType::new(ModuleId::Asset, 6)
                .unwrap_or_else(|error| panic!("receive activity type: {error:?}")),
        ],
    )
    .unwrap_or_else(|error| panic!("asset registration: {error:?}"));
    let budget = ModuleRegistration::new(
        ModuleId::Budget,
        &[ActivityType::new(ModuleId::Budget, 7)
            .unwrap_or_else(|error| panic!("defund activity type: {error:?}"))],
    )
    .unwrap_or_else(|error| panic!("budget registration: {error:?}"));
    ModuleRegistry::new(&[asset, budget])
        .unwrap_or_else(|error| panic!("module registry: {error:?}"))
}

fn payload_activity_type(payload: &[u8]) -> Result<ActivityType, AgentBoundaryError> {
    let tag = payload
        .get(..2)
        .and_then(|bytes| <[u8; 2]>::try_from(bytes).ok())
        .map(u16::from_be_bytes)
        .ok_or(AgentBoundaryError::CorruptResponse)?;
    match tag {
        0x5301 => ActivityType::new(ModuleId::Asset, 5),
        0x5201 => ActivityType::new(ModuleId::Asset, 6),
        0x4207 => ActivityType::new(ModuleId::Budget, 7),
        _ => return Err(AgentBoundaryError::Refused),
    }
    .map_err(|_| AgentBoundaryError::CorruptResponse)
}

fn account(value: &str) -> AccountId {
    AccountId::parse(value).unwrap_or_else(|error| panic!("account: {error:?}"))
}

fn send_intent(public_key: [u8; 32], amount: u128, key: u8) -> Intent {
    let send = LxpSend::new(
        account("agent:did:layerx:alice:main"),
        account("agent:did:layerx:recipient:main"),
        AssetId::new([0x33; 32]),
        Amount::from_u128(amount),
        Sequence::from_u64(ACCOUNT_SEQUENCE),
        IdempotencyKey::new([key; 32]),
        TimestampSeconds::from_u64(1_010),
        ContextHash::new([0x55; 32]),
        SendAuthorization::new(
            SendAuthorizationKind::Owner,
            PublicKey::new(public_key),
            AuthorizationSignature::new([0x77; 64]),
        ),
        NetworkId::new(NETWORK_ID).unwrap_or_else(|error| panic!("network: {error:?}")),
        ProtocolVersion::new(layerx_wire::limits::PROTOCOL_VERSION)
            .unwrap_or_else(|error| panic!("protocol: {error:?}")),
    )
    .unwrap_or_else(|error| panic!("send intent: {error:?}"));
    Intent::v1(IntentKind::LxpSend(send))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DeliveryMode {
    TrackThenReceipt,
    UnknownThenLookup,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FaultPoint {
    Prepare,
    Submit,
    Track,
    Lookup,
}

struct RecordedCore(CorePreparationState);

impl CorePreparationBoundary for RecordedCore {
    fn preparation_state(&mut self, _actor: &Did) -> Result<CorePreparationState, CoreStateError> {
        Ok(self.0.clone())
    }
}

/// Test adapter over the real agentd prepare, signature-verification, durable
/// outbox, and receipt store. Faults are injected only after the named real
/// operation has completed, reproducing a service crash in the acknowledgement
/// gap without replacing any production component.
struct RealAgentLayer {
    store: AgentStore,
    outbox: Outbox,
    tenant: TenantId,
    registry: ModuleRegistry,
    preparations: BTreeMap<[u8; 32], Prepared>,
    preparation_bodies: BTreeMap<[u8; 32], [u8; 32]>,
    observations: BTreeMap<[u8; 32], AgentObservation>,
    receipt_material: BTreeMap<[u8; 32], ReceiptMaterial>,
    submission_keys: BTreeMap<String, [u8; 32]>,
    modes: BTreeMap<[u8; 32], DeliveryMode>,
    effects: BTreeMap<[u8; 32], u32>,
    prepare_calls: BTreeMap<[u8; 32], u32>,
    submit_calls: BTreeMap<[u8; 32], u32>,
    track_calls: BTreeMap<[u8; 32], u32>,
    lookup_calls: BTreeMap<[u8; 32], u32>,
    fault: Option<FaultPoint>,
    fault_fired: bool,
}

impl RealAgentLayer {
    fn new(root: &std::path::Path, modes: BTreeMap<[u8; 32], DeliveryMode>) -> Self {
        Self {
            store: AgentStore::open(root).unwrap_or_else(|error| panic!("agent store: {error}")),
            outbox: Outbox::default(),
            tenant: TenantId::new("tenant-a").unwrap_or_else(|error| panic!("tenant: {error}")),
            registry: registry(),
            preparations: BTreeMap::new(),
            preparation_bodies: BTreeMap::new(),
            observations: BTreeMap::new(),
            receipt_material: BTreeMap::new(),
            submission_keys: BTreeMap::new(),
            modes,
            effects: BTreeMap::new(),
            prepare_calls: BTreeMap::new(),
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
        let count = map.entry(key).or_default();
        *count = count.saturating_add(1);
    }

    fn tracked(
        key: [u8; 32],
        state: SubmissionState,
        receipt_digest: Option<[u8; 32]>,
    ) -> TrackedSubmission {
        let evidence = receipt_digest.map_or_else(Vec::new, |digest| {
            vec![AgentEvidenceRef {
                kind: "sequencer-receipt".to_owned(),
                digest,
            }]
        });
        let verification_level = if evidence.is_empty() {
            Level::Unverified
        } else {
            Level::SequencerSigned
        };
        TrackedSubmission {
            submission_ref: SubmissionRef::new(format!("sub-{}", hex(&key)))
                .unwrap_or_else(|error| panic!("submission ref: {error:?}")),
            state,
            evidence,
            verification_level,
            transitions: Vec::new(),
        }
    }

    fn executed_observation(&self, key: [u8; 32], activity_id: [u8; 32]) -> AgentObservation {
        let material = self
            .receipt_material
            .get(&key)
            .cloned()
            .unwrap_or_else(|| panic!("receipt material missing"));
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
        Self::count(&mut self.prepare_calls, key);
        if let Some(body) = self.preparation_bodies.get(&key) {
            if *body != mutation.body_digest.0 {
                return Err(AgentBoundaryError::Refused);
            }
        } else {
            let request = &mutation.operation;
            let activity = payload_activity_type(request.payload.as_bytes())?;
            let mut core = RecordedCore(CorePreparationState {
                network_id: NETWORK_ID,
                account_sequence: request.account_sequence.get(),
                protocol_timestamp: request.timestamp_bound.not_before.get().saturating_add(5),
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
                    activity_type: activity,
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
        let result = AgentPreparation {
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
        };
        self.fail_after(FaultPoint::Prepare)?;
        Ok(result)
    }

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
        if mutation.operation.preparation_ref.as_str() != format!("prep-{}", hex(&key)) {
            return Err(AgentBoundaryError::Refused);
        }
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
        let receipt = receipt(activity_id, key[0], prepared.envelope.activity_type());
        self.receipt_material.insert(key, receipt.clone());
        Self::count(&mut self.effects, key);
        let mode = self
            .modes
            .get(&key)
            .copied()
            .ok_or(AgentBoundaryError::CorruptResponse)?;
        let observation = match mode {
            DeliveryMode::TrackThenReceipt => {
                daemon_receipt::store(
                    &mut self.store,
                    self.tenant.clone(),
                    key,
                    &receipt.canonical_bytes,
                    &receipt.authorised_batch,
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
            }
            DeliveryMode::UnknownThenLookup => {
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
        };
        self.submission_keys.insert(
            observation.submission.submission_ref.as_str().to_owned(),
            key,
        );
        self.observations.insert(key, observation.clone());
        self.fail_after(FaultPoint::Submit)?;
        Ok(observation)
    }

    fn track(&mut self, call: &Call<TrackRequest>) -> Result<AgentObservation, AgentBoundaryError> {
        let reference = call.request().submission_ref.as_str();
        let key = *self
            .submission_keys
            .get(reference)
            .ok_or(AgentBoundaryError::CorruptResponse)?;
        Self::count(&mut self.track_calls, key);
        if self.modes.get(&key) == Some(&DeliveryMode::UnknownThenLookup) {
            return Err(AgentBoundaryError::CorruptResponse);
        }
        let activity_id = self
            .outbox
            .status(key)
            .map(|status| status.activity_id)
            .ok_or(AgentBoundaryError::CorruptResponse)?;
        let current = self
            .outbox
            .status(key)
            .map(|status| status.state)
            .ok_or(AgentBoundaryError::CorruptResponse)?;
        if current == OutboxState::Acknowledged {
            let material = self
                .receipt_material
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
        let observation = self.executed_observation(key, activity_id);
        self.observations.insert(key, observation.clone());
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
                .receipt_material
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
            .receipt_material
            .get(&idempotency_key)
            .cloned()
            .ok_or(AgentBoundaryError::CorruptResponse)?;
        if served.canonical_bytes != material.canonical_bytes {
            return Err(AgentBoundaryError::CorruptResponse);
        }
        self.fail_after(FaultPoint::Lookup)?;
        Ok(ReceiptLookup::Found(material))
    }
}

#[derive(Clone)]
struct ReceiptFields {
    activity_id: [u8; 32],
    previous_state_root: [u8; 32],
    resulting_state_root: [u8; 32],
    batch_id: [u8; 32],
    asset: [u8; 32],
    module: ModuleId,
    operation: u8,
    sequence: u64,
}

fn receipt(activity_id: [u8; 32], marker: u8, activity: ActivityType) -> ReceiptMaterial {
    let fields = ReceiptFields {
        activity_id,
        previous_state_root: [marker.saturating_add(1); 32],
        resulting_state_root: [marker.saturating_add(2); 32],
        batch_id: support::execution_batch_id(
            [marker.saturating_add(1); 32],
            activity_id,
            u64::from(marker),
        ),
        asset: [0x33; 32],
        module: activity.module(),
        operation: u8::try_from(activity.ordinal())
            .unwrap_or_else(|_| panic!("activity ordinal is outside receipt range")),
        sequence: u64::from(marker),
    };
    let signer = SigningKey::from_bytes(&[marker.saturating_add(4); 32]);
    let unsigned = encode_receipt(&fields, None);
    let mut digest = Sha256::new();
    digest.update(b"LXP/v1/receipt\0");
    digest.update(&unsigned);
    let signature = signer.sign(&<[u8; 32]>::from(digest.finalize()));
    ReceiptMaterial {
        canonical_bytes: encode_receipt(&fields, Some(signature.to_bytes())),
        authorised_batch: AuthorizedBatch::new(
            fields.batch_id,
            fields.asset,
            fields.previous_state_root,
            fields.resulting_state_root,
            signer.verifying_key().to_bytes(),
        ),
        verification_level: layerx_types::verify::VerificationLevel::SEQUENCER_SIGNED,
    }
}

fn encode_receipt(fields: &ReceiptFields, signature: Option<[u8; 64]>) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_u16(&mut bytes, layerx_wire::limits::PROTOCOL_VERSION);
    push_u16(&mut bytes, 0x5201);
    push_u16(&mut bytes, layerx_wire::limits::PROTOCOL_VERSION);
    push_bytes(&mut bytes, &fields.activity_id);
    push_u64(&mut bytes, fields.sequence);
    push_bytes(&mut bytes, &fields.previous_state_root);
    push_bytes(&mut bytes, &fields.resulting_state_root);
    push_bytes(&mut bytes, &[0x81; 32]);
    bytes.extend_from_slice(&0_i32.to_be_bytes());
    bytes.extend_from_slice(&0_u32.to_be_bytes());
    bytes.extend_from_slice(&1_u128.to_be_bytes());
    push_bytes(&mut bytes, &fields.batch_id);
    push_u16(&mut bytes, u16::from(fields.module as u8));
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.push(fields.operation);
    push_bytes(&mut bytes, &fields.asset);
    bytes.extend_from_slice(&1_u128.to_be_bytes());
    push_bytes(&mut bytes, &[0x91; 32]);
    bytes.extend_from_slice(&10_u128.to_be_bytes());
    bytes.extend_from_slice(&9_u128.to_be_bytes());
    push_u64(&mut bytes, 1);
    push_bytes(&mut bytes, &[0x92; 32]);
    bytes.extend_from_slice(&20_u128.to_be_bytes());
    bytes.extend_from_slice(&21_u128.to_be_bytes());
    push_bytes(&mut bytes, &[0x93; 32]);
    push_bytes(&mut bytes, &[0x94; 32]);
    push_bytes(&mut bytes, &[0x95; 32]);
    push_u64(&mut bytes, 1_000);
    bytes.push(u8::from(signature.is_some()));
    if let Some(signature) = signature {
        push_bytes(&mut bytes, &signature);
    }
    bytes
}

fn push_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

fn push_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

fn push_bytes(output: &mut Vec<u8>, value: &[u8]) {
    let length =
        u32::try_from(value.len()).unwrap_or_else(|_| panic!("receipt field length overflow"));
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(value);
}

struct Fixture {
    root: std::path::PathBuf,
    store_root: std::path::PathBuf,
    custody_root: std::path::PathBuf,
    agent_root: std::path::PathBuf,
    secret_path: std::path::PathBuf,
    tenancy_digest: TenancyDigest,
    principal: PrincipalId,
    public_key: [u8; 32],
    signer: CustodySigner,
    agent_contract: AgentClient,
    trace: TraceId,
}

impl Fixture {
    fn new(label: &str) -> Self {
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
        let agent_contract = AgentClient::daemon("/run/layerx-agentd.sock", schema.version)
            .unwrap_or_else(|error| panic!("agent SDK: {error:?}"));
        Self {
            store_root,
            custody_root: root.join("custody"),
            agent_root: root.join("agent-store"),
            secret_path,
            tenancy_digest,
            principal,
            public_key,
            signer,
            agent_contract,
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

    fn plan(&self) -> JourneyPlan {
        let legs = [0x21_u8, 0x22_u8]
            .into_iter()
            .map(|key| {
                JourneyLeg::new(
                    send_intent(self.public_key, u128::from(key), key),
                    [key; 32],
                    AgentDid::new("did:layerx:alice")
                        .unwrap_or_else(|error| panic!("actor: {error:?}")),
                    AuthorityRef::new("custody-human-primary")
                        .unwrap_or_else(|error| panic!("authority: {error:?}")),
                    ACCOUNT_SEQUENCE,
                    995,
                    1_010,
                    7,
                )
                .unwrap_or_else(|error| panic!("leg: {error}"))
            })
            .collect();
        JourneyPlan::new(
            JourneyId::new("jrn_crashjourney")
                .unwrap_or_else(|error| panic!("journey id: {error}")),
            layerx_human_service::journeys::JourneyKind::Move,
            [0x31; 32],
            KeyId::new("human-primary").unwrap_or_else(|error| panic!("key: {error}")),
            Operation::ProtocolMutation,
            legs,
        )
        .unwrap_or_else(|error| panic!("plan: {error}"))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = (&self.custody_root, &self.secret_path);
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn modes() -> BTreeMap<[u8; 32], DeliveryMode> {
    BTreeMap::from([
        ([0x21; 32], DeliveryMode::TrackThenReceipt),
        ([0x22; 32], DeliveryMode::UnknownThenLookup),
    ])
}

fn reopen(fixture: &Fixture) -> (PrincipalStore, JourneyEngine) {
    let mut store = fixture.store();
    let scope = store
        .principal(&fixture.principal)
        .unwrap_or_else(|error| panic!("reopen scope: {error}"));
    let id =
        JourneyId::new("jrn_crashjourney").unwrap_or_else(|error| panic!("journey id: {error}"));
    let engine = JourneyEngine::load(&scope, &id)
        .unwrap_or_else(|error| panic!("load journey: {error}"))
        .unwrap_or_else(|| panic!("journey missing"));
    drop(scope);
    (store, engine)
}

fn drive_with_restart(
    fixture: &Fixture,
    agent: &mut RealAgentLayer,
    mut store: PrincipalStore,
    mut engine: JourneyEngine,
    start_time: u64,
) -> JourneyEngine {
    let registry = registry();
    for offset in 0..40_u64 {
        let mut scope = store
            .principal(&fixture.principal)
            .unwrap_or_else(|error| panic!("drive scope: {error}"));
        let result = ready(engine.advance(
            &mut scope,
            &fixture.agent_contract,
            agent,
            &fixture.signer,
            &registry,
            &fixture.trace,
            start_time.saturating_add(offset),
        ));
        drop(scope);
        drop(store);
        if let Err(error) = result {
            assert!(
                error.to_string().contains("agent boundary failure"),
                "unexpected journey error: {error}"
            );
        }
        let reopened = reopen(fixture);
        store = reopened.0;
        engine = reopened.1;
        if engine
            .status()
            .unwrap_or_else(|error| panic!("status: {error}"))
            .state()
            == JourneyState::Done
        {
            return engine;
        }
    }
    panic!("journey did not complete within the bounded driver")
}

#[test]
fn crashes_after_every_durable_stage_resume_with_one_effect_per_leg() {
    let fixture = Fixture::new("journey-stage-crashes");
    let registry = registry();
    let mut store = fixture.store();
    let mut scope = store
        .principal(&fixture.principal)
        .unwrap_or_else(|error| panic!("scope: {error}"));
    let plan = fixture.plan();
    let engine = JourneyEngine::start(&mut scope, &plan, &registry, 100)
        .unwrap_or_else(|error| panic!("start: {error}"));
    let repeated = JourneyEngine::start(&mut scope, &plan, &registry, 100)
        .unwrap_or_else(|error| panic!("repeat: {error}"));
    assert_eq!(
        repeated
            .status()
            .unwrap_or_else(|error| panic!("repeat status: {error}")),
        engine
            .status()
            .unwrap_or_else(|error| panic!("initial status: {error}"))
    );
    drop(scope);

    let mut agent = RealAgentLayer::new(&fixture.agent_root, modes());
    let engine = drive_with_restart(&fixture, &mut agent, store, engine, 101);
    let status = engine
        .status()
        .unwrap_or_else(|error| panic!("final status: {error}"));
    assert_eq!(status.state(), JourneyState::Done);
    assert_eq!(
        status.phases(),
        [JourneyPhase::ReceiptVerified, JourneyPhase::ReceiptVerified]
    );
    assert!(status.receipt_digests().iter().all(Option::is_some));
    assert_eq!(agent.effects.get(&[0x21; 32]), Some(&1));
    assert_eq!(agent.effects.get(&[0x22; 32]), Some(&1));
    assert_eq!(agent.submit_calls.get(&[0x21; 32]), Some(&1));
    assert_eq!(agent.submit_calls.get(&[0x22; 32]), Some(&1));
    assert_eq!(agent.track_calls.get(&[0x21; 32]), Some(&1));
    assert!(!agent.track_calls.contains_key(&[0x22; 32]));
    assert_eq!(agent.lookup_calls.get(&[0x22; 32]), Some(&2));

    let mut final_store = fixture.store();
    let final_scope = final_store
        .principal(&fixture.principal)
        .unwrap_or_else(|error| panic!("final scope: {error}"));
    let id =
        JourneyId::new("jrn_crashjourney").unwrap_or_else(|error| panic!("journey id: {error}"));
    let stream = JourneyEngine::stream_events(&final_scope, &id)
        .unwrap_or_else(|error| panic!("stream: {error}"));
    let notifications = JourneyEngine::notification_events(&final_scope, &id)
        .unwrap_or_else(|error| panic!("notifications: {error}"));
    assert_eq!(stream, notifications);
    assert!(stream
        .windows(2)
        .all(|pair| pair[1].sequence() == pair[0].sequence() + 1));
    let cursors = stream
        .iter()
        .map(JourneyProgress::cursor)
        .collect::<BTreeSet<_>>();
    assert_eq!(cursors.len(), stream.len());
}

#[test]
fn acknowledgement_gap_faults_converge_without_resubmitting_unknown() {
    for fault in [
        FaultPoint::Prepare,
        FaultPoint::Submit,
        FaultPoint::Track,
        FaultPoint::Lookup,
    ] {
        let label = format!("journey-gap-{fault:?}").to_ascii_lowercase();
        let fixture = Fixture::new(&label);
        let registry = registry();
        let mut store = fixture.store();
        let mut scope = store
            .principal(&fixture.principal)
            .unwrap_or_else(|error| panic!("scope: {error}"));
        let engine = JourneyEngine::start(&mut scope, &fixture.plan(), &registry, 200)
            .unwrap_or_else(|error| panic!("start: {error}"));
        drop(scope);
        let mut agent = RealAgentLayer::new(&fixture.agent_root, modes());
        agent.inject(fault);
        let engine = drive_with_restart(&fixture, &mut agent, store, engine, 201);
        assert_eq!(
            engine
                .status()
                .unwrap_or_else(|error| panic!("status: {error}"))
                .state(),
            JourneyState::Done
        );
        assert_eq!(agent.effects.get(&[0x21; 32]), Some(&1));
        assert_eq!(agent.effects.get(&[0x22; 32]), Some(&1));
        assert!(agent.fault_fired);
        if fault == FaultPoint::Submit {
            assert_eq!(agent.submit_calls.get(&[0x21; 32]), Some(&2));
            assert_eq!(agent.effects.get(&[0x21; 32]), Some(&1));
        }
        assert!(!agent.track_calls.contains_key(&[0x22; 32]));
        assert_eq!(agent.submit_calls.get(&[0x22; 32]), Some(&1));
        if fault == FaultPoint::Lookup {
            assert_eq!(agent.lookup_calls.get(&[0x22; 32]), Some(&3));
        } else {
            assert_eq!(agent.lookup_calls.get(&[0x22; 32]), Some(&2));
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(text, "{byte:02x}");
    }
    text
}

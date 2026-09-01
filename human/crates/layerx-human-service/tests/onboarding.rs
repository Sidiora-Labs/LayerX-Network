#[allow(dead_code)]
mod support;

use std::fs;

use ed25519_dalek::{Signer as _, SigningKey};
use layerx_agent_api::agent_api_schema_v1;
use layerx_agent_api::identity::{AgentDid, AuthorityRef};
use layerx_agent_api::track::{EvidenceRef as AgentEvidenceRef, ReceiptRef, SubmissionRef};
use layerx_agent_api::verify::Level;
use layerx_agent_api::{SubmissionState, SubmissionState as AgentSubmissionState};
use layerx_human_service::audit::{verify_export, AuditChain, IdentityEvent};
use layerx_human_service::custody::{EnvelopeKms, Keystore};
use layerx_human_service::onboarding::{
    AgentPreparationContext, AgentStageOutcome, AgentStageUpdate, Dependency, EvidenceVerification,
    OnboardingError, OnboardingJourney, OnboardingStage, OnboardingStart, OnboardingState,
    ProtocolStage, RecoveryPolicy, StageState,
};
use layerx_human_service::store::{PrincipalStore, TenancyDigest};
use layerx_human_service::trace::TraceId;
use layerx_intents::IntentKind;
use layerx_proof::receipt::AuthorizedBatch;
use layerx_sdk::{Client as AgentClient, Operation};
use layerx_types::ids::Did;
use layerx_types::intent::{ApprovalThreshold, RecoveryRoot};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use sha2::{Digest as _, Sha256};

use support::{directory, install_and_open, principal, retention_uniform, tenancy};

const NETWORK_ID: u32 = 77;
const CHALLENGE_DELAY: u64 = 86_400;

struct Fixture {
    root: std::path::PathBuf,
    store: PrincipalStore,
    digest: TenancyDigest,
    keystore: Keystore,
    agent: AgentClient,
    registry: ModuleRegistry,
    trace: TraceId,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = directory(label);
        fs::create_dir_all(&root).unwrap_or_else(|error| panic!("fixture root: {error}"));
        let store_root = root.join("store");
        let custody_root = root.join("custody");
        let secret_path = root.join("kms-root");
        fs::write(&secret_path, [0x51_u8; 64])
            .unwrap_or_else(|error| panic!("KMS secret: {error}"));
        let map = tenancy(&[("alice", "tenant-alice")]);
        let (store, digest) = install_and_open(&store_root, &map, retention_uniform(10));
        let kms = EnvelopeKms::new("kms://onboarding-primary", secret_path.clone())
            .unwrap_or_else(|error| panic!("KMS: {error}"));
        let keystore = Keystore::open_development(&custody_root, NETWORK_ID, kms)
            .unwrap_or_else(|error| panic!("keystore: {error}"));
        let did = ActivityType::new(ModuleId::Governance, 1)
            .unwrap_or_else(|error| panic!("DID activity: {error:?}"));
        let recovery = ActivityType::new(ModuleId::Governance, 3)
            .unwrap_or_else(|error| panic!("recovery activity: {error:?}"));
        let registration = ModuleRegistration::new(ModuleId::Governance, &[did, recovery])
            .unwrap_or_else(|error| panic!("governance registration: {error:?}"));
        let registry = ModuleRegistry::new(&[registration])
            .unwrap_or_else(|error| panic!("registry: {error:?}"));
        let agent = AgentClient::daemon("/run/layerx/agentd.sock", agent_api_schema_v1().version)
            .unwrap_or_else(|error| panic!("agent SDK: {error:?}"));
        Self {
            root,
            store,
            digest,
            keystore,
            agent,
            registry,
            trace: TraceId::mint([0x31; 16]),
        }
    }

    fn reopen_store(&self) -> PrincipalStore {
        PrincipalStore::open(self.root.join("store"), retention_uniform(10), self.digest)
            .unwrap_or_else(|error| panic!("reopen store: {error}"))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn start_request(key: u8) -> OnboardingStart {
    let recovery = RecoveryPolicy::new(
        RecoveryRoot::new([0x43; 32]),
        ApprovalThreshold::new(2).unwrap_or_else(|error| panic!("threshold: {error:?}")),
        CHALLENGE_DELAY,
    )
    .unwrap_or_else(|error| panic!("recovery policy: {error}"));
    OnboardingStart::new(
        [key; 32],
        Did::new(b"did:layerx:alice").unwrap_or_else(|error| panic!("DID: {error:?}")),
        recovery,
    )
    .unwrap_or_else(|error| panic!("start request: {error}"))
}

fn context(sequence: u64, timestamp: u64) -> AgentPreparationContext {
    AgentPreparationContext::new(
        AgentDid::new("did:layerx:registrar").unwrap_or_else(|error| panic!("actor: {error:?}")),
        AuthorityRef::new("custody:human-primary")
            .unwrap_or_else(|error| panic!("authority: {error:?}")),
        sequence,
        timestamp,
        timestamp.saturating_add(30),
        10,
    )
    .unwrap_or_else(|error| panic!("agent context: {error}"))
}

#[derive(Clone)]
struct ReceiptFields {
    activity_id: [u8; 32],
    previous_state_root: [u8; 32],
    resulting_state_root: [u8; 32],
    batch_id: [u8; 32],
    asset: [u8; 32],
    operation: u8,
    sequence: u64,
}

struct ReceiptFixture {
    bytes: Vec<u8>,
    authorised: AuthorizedBatch,
    activity_id: [u8; 32],
}

fn receipt(stage: ProtocolStage, marker: u8) -> ReceiptFixture {
    let fields = ReceiptFields {
        activity_id: [marker; 32],
        previous_state_root: [marker.saturating_add(1); 32],
        resulting_state_root: [marker.saturating_add(2); 32],
        batch_id: [marker.saturating_add(3); 32],
        asset: [0x71; 32],
        operation: match stage {
            ProtocolStage::DidRegistration => 1,
            ProtocolStage::RecoveryRegistration => 3,
        },
        sequence: u64::from(marker),
    };
    let signer = SigningKey::from_bytes(&[marker.saturating_add(4); 32]);
    let unsigned = encode_receipt(&fields, None);
    let mut receipt_digest = Sha256::new();
    receipt_digest.update(b"LXP/v1/receipt\0");
    receipt_digest.update(&unsigned);
    let signature = signer.sign(&<[u8; 32]>::from(receipt_digest.finalize()));
    let bytes = encode_receipt(&fields, Some(signature.to_bytes()));
    ReceiptFixture {
        bytes,
        authorised: AuthorizedBatch::new(
            fields.batch_id,
            fields.asset,
            fields.previous_state_root,
            fields.resulting_state_root,
            signer.verifying_key().to_bytes(),
        ),
        activity_id: fields.activity_id,
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
    push_u16(&mut bytes, u16::from(ModuleId::Governance as u8));
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
        u32::try_from(value.len()).unwrap_or_else(|_| panic!("test receipt field length overflow"));
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(value);
}

fn tracked(
    submission: &str,
    receipt: &str,
    state: AgentSubmissionState,
    evidence_digest: [u8; 32],
) -> layerx_agent_api::track::TrackedSubmission {
    let evidence = if matches!(state, SubmissionState::Executed { .. }) {
        vec![AgentEvidenceRef {
            kind: "sequencer-receipt".to_owned(),
            digest: evidence_digest,
        }]
    } else {
        Vec::new()
    };
    let verification_level = if evidence.is_empty() {
        Level::Unverified
    } else {
        Level::SequencerSigned
    };
    layerx_agent_api::track::TrackedSubmission {
        submission_ref: SubmissionRef::new(submission)
            .unwrap_or_else(|error| panic!("submission ref: {error:?}")),
        state: match state {
            SubmissionState::Executed { .. } => SubmissionState::Executed {
                receipt_ref: ReceiptRef::new(receipt)
                    .unwrap_or_else(|error| panic!("receipt ref: {error:?}")),
            },
            other => other,
        },
        evidence,
        verification_level,
        transitions: Vec::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn complete_stage(
    journey: &mut OnboardingJourney,
    scope: &mut layerx_human_service::store::PrincipalScope<'_>,
    audit: &mut AuditChain,
    trace: &TraceId,
    action_key: [u8; 32],
    receipt: &ReceiptFixture,
    stage: ProtocolStage,
    now: u64,
) {
    let digest = Sha256::digest(&receipt.bytes).into();
    let submission = tracked(
        match stage {
            ProtocolStage::DidRegistration => "sub-did",
            ProtocolStage::RecoveryRegistration => "sub-recovery",
        },
        match stage {
            ProtocolStage::DidRegistration => "rcp-did",
            ProtocolStage::RecoveryRegistration => "rcp-recovery",
        },
        SubmissionState::Executed {
            receipt_ref: ReceiptRef::new("rcp-input")
                .unwrap_or_else(|error| panic!("receipt ref: {error:?}")),
        },
        digest,
    );
    journey
        .apply_agent_update(
            scope,
            audit,
            trace,
            &AgentStageUpdate {
                stage,
                action_key,
                outcome: AgentStageOutcome::Executed {
                    submission: &submission,
                    activity_id: receipt.activity_id,
                    receipt_bytes: &receipt.bytes,
                    authorised_batch: &receipt.authorised,
                },
            },
            now,
        )
        .unwrap_or_else(|error| panic!("complete stage: {error}"));
}

#[test]
#[allow(clippy::too_many_lines)]
fn receipt_gated_onboarding_survives_unavailability_and_restart() {
    let mut fixture = Fixture::new("onboarding-complete");
    let alice = principal("alice");
    let mut scope = fixture
        .store
        .principal(&alice)
        .unwrap_or_else(|error| panic!("scope: {error}"));
    let mut journey = OnboardingJourney::start(&mut scope, &start_request(0x21), 100)
        .unwrap_or_else(|error| panic!("start: {error}"));
    assert!(!journey.status().account_active());
    journey
        .resume_local(&mut scope, &fixture.keystore, 101)
        .unwrap_or_else(|error| panic!("local resume: {error}"));
    let did_action = journey
        .prepare_agent_action(
            &mut scope,
            &fixture.registry,
            &fixture.agent,
            &context(7, 1_000),
            102,
        )
        .unwrap_or_else(|error| panic!("DID action: {error}"))
        .unwrap_or_else(|| panic!("DID action missing"));
    assert_eq!(did_action.stage(), ProtocolStage::DidRegistration);
    assert!(matches!(
        did_action.intent().kind(),
        IntentKind::DidRegistration(_)
    ));
    assert_eq!(did_action.call().operation(), Operation::Prepare);
    assert_eq!(
        did_action.call().request().operation.payload.as_bytes(),
        did_action.disclosure().canonical_payload()
    );

    let did_key = did_action.action_key();
    let mut audit = AuditChain::open(&scope).unwrap_or_else(|error| panic!("audit: {error}"));
    let unavailable = journey
        .apply_agent_update(
            &mut scope,
            &mut audit,
            &fixture.trace,
            &AgentStageUpdate {
                stage: ProtocolStage::DidRegistration,
                action_key: did_key,
                outcome: AgentStageOutcome::Unavailable {
                    dependency: Dependency::AgentLayer,
                },
            },
            103,
        )
        .unwrap_or_else(|error| panic!("unavailable update: {error}"));
    assert!(!unavailable.account_active());
    assert_eq!(unavailable.state(), OnboardingState::Queued);
    assert_eq!(
        unavailable.stages()[2].state(),
        StageState::Queued {
            unavailable: Some(Dependency::AgentLayer)
        }
    );

    drop(scope);
    let mut reopened = fixture.reopen_store();
    let mut scope = reopened
        .principal(&alice)
        .unwrap_or_else(|error| panic!("reopened scope: {error}"));
    let mut journey = OnboardingJourney::load(&scope)
        .unwrap_or_else(|error| panic!("load: {error}"))
        .unwrap_or_else(|| panic!("journey missing"));
    journey
        .resume_local(&mut scope, &fixture.keystore, 104)
        .unwrap_or_else(|error| panic!("key rediscovery: {error}"));
    let repeated = journey
        .prepare_agent_action(
            &mut scope,
            &fixture.registry,
            &fixture.agent,
            &context(999, 9_999),
            105,
        )
        .unwrap_or_else(|error| panic!("repeated action: {error}"))
        .unwrap_or_else(|| panic!("repeated action missing"));
    assert_eq!(repeated.action_key(), did_key);
    assert_eq!(repeated.call(), did_action.call());

    let mut audit = AuditChain::open(&scope).unwrap_or_else(|error| panic!("audit: {error}"));
    let unknown = tracked("sub-did", "unused", SubmissionState::Unknown, [0; 32]);
    let still_checking = journey
        .apply_agent_update(
            &mut scope,
            &mut audit,
            &fixture.trace,
            &AgentStageUpdate {
                stage: ProtocolStage::DidRegistration,
                action_key: did_key,
                outcome: AgentStageOutcome::Tracked {
                    submission: &unknown,
                },
            },
            106,
        )
        .unwrap_or_else(|error| panic!("unknown update: {error}"));
    assert_eq!(still_checking.state(), OnboardingState::StillChecking);
    assert!(!still_checking.account_active());
    assert!(journey
        .prepare_agent_action(
            &mut scope,
            &fixture.registry,
            &fixture.agent,
            &context(7, 1_000),
            107,
        )
        .unwrap_or_else(|error| panic!("locked action check: {error}"))
        .is_none());

    let did_receipt = receipt(ProtocolStage::DidRegistration, 0x11);
    complete_stage(
        &mut journey,
        &mut scope,
        &mut audit,
        &fixture.trace,
        did_key,
        &did_receipt,
        ProtocolStage::DidRegistration,
        108,
    );
    assert!(journey.status().account_active());
    assert_eq!(
        journey.status().state(),
        OnboardingState::ActiveRecoveryPending
    );
    let did_evidence = journey
        .activation_evidence(&scope)
        .unwrap_or_else(|error| panic!("DID evidence: {error}"));
    assert_eq!(did_evidence.len(), 2);
    assert_eq!(did_evidence[0].event(), IdentityEvent::DidRegistration);
    assert_eq!(did_evidence[1].event(), IdentityEvent::KeyActivation);
    assert_eq!(
        did_evidence[0].pointer().verification(),
        EvidenceVerification::ReceiptVerified
    );
    assert_eq!(did_evidence[0].canonical_receipt(), did_receipt.bytes);
    assert_eq!(did_evidence[1].canonical_receipt(), did_receipt.bytes);

    let recovery_action = journey
        .prepare_agent_action(
            &mut scope,
            &fixture.registry,
            &fixture.agent,
            &context(8, 1_100),
            109,
        )
        .unwrap_or_else(|error| panic!("recovery action: {error}"))
        .unwrap_or_else(|| panic!("recovery action missing"));
    assert!(matches!(
        recovery_action.intent().kind(),
        IntentKind::RecoveryRegistration(_)
    ));
    let recovery_receipt = receipt(ProtocolStage::RecoveryRegistration, 0x22);
    complete_stage(
        &mut journey,
        &mut scope,
        &mut audit,
        &fixture.trace,
        recovery_action.action_key(),
        &recovery_receipt,
        ProtocolStage::RecoveryRegistration,
        110,
    );
    let complete = journey.status();
    assert!(complete.account_active());
    assert_eq!(complete.state(), OnboardingState::Complete);
    assert_eq!(complete.recovery_challenge_delay_secs(), CHALLENGE_DELAY);
    let evidence = journey
        .activation_evidence(&scope)
        .unwrap_or_else(|error| panic!("activation evidence: {error}"));
    assert_eq!(evidence.len(), 3);
    assert_eq!(
        evidence[2].recovery_challenge_delay_secs(),
        Some(CHALLENGE_DELAY)
    );

    complete_stage(
        &mut journey,
        &mut scope,
        &mut audit,
        &fixture.trace,
        recovery_action.action_key(),
        &recovery_receipt,
        ProtocolStage::RecoveryRegistration,
        111,
    );
    assert_eq!(
        audit
            .entries(&scope)
            .unwrap_or_else(|error| panic!("audit entries: {error}"))
            .len(),
        3
    );
    let export = audit
        .export(&scope)
        .unwrap_or_else(|error| panic!("audit export: {error}"));
    let report = verify_export(&export).unwrap_or_else(|error| panic!("verify export: {error}"));
    assert_eq!(report.entries(), 3);
    assert_eq!(report.evidence_rows(), 3);
}

#[test]
fn tampered_or_wrong_operation_receipt_never_activates_account() {
    let mut fixture = Fixture::new("onboarding-receipt-refusal");
    let mut scope = fixture
        .store
        .principal(&principal("alice"))
        .unwrap_or_else(|error| panic!("scope: {error}"));
    let mut journey = OnboardingJourney::start(&mut scope, &start_request(0x32), 200)
        .unwrap_or_else(|error| panic!("start: {error}"));
    journey
        .resume_local(&mut scope, &fixture.keystore, 201)
        .unwrap_or_else(|error| panic!("resume: {error}"));
    let action = journey
        .prepare_agent_action(
            &mut scope,
            &fixture.registry,
            &fixture.agent,
            &context(1, 2_000),
            202,
        )
        .unwrap_or_else(|error| panic!("action: {error}"))
        .unwrap_or_else(|| panic!("action missing"));
    let wrong = receipt(ProtocolStage::RecoveryRegistration, 0x44);
    let digest = Sha256::digest(&wrong.bytes).into();
    let submission = tracked(
        "sub-did",
        "rcp-wrong",
        SubmissionState::Executed {
            receipt_ref: ReceiptRef::new("rcp-input")
                .unwrap_or_else(|error| panic!("receipt: {error:?}")),
        },
        digest,
    );
    let mut audit = AuditChain::open(&scope).unwrap_or_else(|error| panic!("audit: {error}"));
    assert!(matches!(
        journey.apply_agent_update(
            &mut scope,
            &mut audit,
            &fixture.trace,
            &AgentStageUpdate {
                stage: ProtocolStage::DidRegistration,
                action_key: action.action_key(),
                outcome: AgentStageOutcome::Executed {
                    submission: &submission,
                    activity_id: wrong.activity_id,
                    receipt_bytes: &wrong.bytes,
                    authorised_batch: &wrong.authorised,
                },
            },
            203,
        ),
        Err(OnboardingError::ReceiptOperationMismatch)
    ));
    assert!(!journey.status().account_active());
    let mut tampered = receipt(ProtocolStage::DidRegistration, 0x45);
    tampered.bytes[100] ^= 1;
    assert!(journey
        .apply_agent_update(
            &mut scope,
            &mut audit,
            &fixture.trace,
            &AgentStageUpdate {
                stage: ProtocolStage::DidRegistration,
                action_key: action.action_key(),
                outcome: AgentStageOutcome::Executed {
                    submission: &submission,
                    activity_id: tampered.activity_id,
                    receipt_bytes: &tampered.bytes,
                    authorised_batch: &tampered.authorised,
                },
            },
            204,
        )
        .is_err());
    assert!(!journey.status().account_active());
    assert!(journey
        .activation_evidence(&scope)
        .unwrap_or_else(|error| panic!("evidence: {error}"))
        .is_empty());
}

#[test]
fn duplicate_start_and_key_generation_converge_without_orphans() {
    let mut fixture = Fixture::new("onboarding-idempotency");
    let alice = principal("alice");
    let mut scope = fixture
        .store
        .principal(&alice)
        .unwrap_or_else(|error| panic!("scope: {error}"));
    let mut first = OnboardingJourney::start(&mut scope, &start_request(0x53), 300)
        .unwrap_or_else(|error| panic!("first start: {error}"));
    first
        .resume_local(&mut scope, &fixture.keystore, 301)
        .unwrap_or_else(|error| panic!("first key generation: {error}"));
    let first_status = first.status();
    let repeated = OnboardingJourney::start(&mut scope, &start_request(0x53), 999)
        .unwrap_or_else(|error| panic!("repeat start: {error}"));
    assert_eq!(repeated, first);
    let mut recovered = OnboardingJourney::load(&scope)
        .unwrap_or_else(|error| panic!("load: {error}"))
        .unwrap_or_else(|| panic!("journey missing"));
    recovered
        .resume_local(&mut scope, &fixture.keystore, 302)
        .unwrap_or_else(|error| panic!("key rediscovery: {error}"));
    assert_eq!(recovered.status(), first_status);
    assert_eq!(
        fixture
            .keystore
            .keys(&alice)
            .unwrap_or_else(|error| panic!("keys: {error}"))
            .len(),
        1
    );
    assert!(matches!(
        OnboardingJourney::start(&mut scope, &start_request(0x54), 303),
        Err(OnboardingError::IdempotencyConflict)
    ));
}

#[test]
fn recovery_is_never_silently_optional() {
    let mut fixture = Fixture::new("onboarding-required-recovery");
    let mut scope = fixture
        .store
        .principal(&principal("alice"))
        .unwrap_or_else(|error| panic!("scope: {error}"));
    let mut journey = OnboardingJourney::start(&mut scope, &start_request(0x63), 400)
        .unwrap_or_else(|error| panic!("start: {error}"));
    journey
        .resume_local(&mut scope, &fixture.keystore, 401)
        .unwrap_or_else(|error| panic!("resume: {error}"));
    let action = journey
        .prepare_agent_action(
            &mut scope,
            &fixture.registry,
            &fixture.agent,
            &context(1, 3_000),
            402,
        )
        .unwrap_or_else(|error| panic!("DID action: {error}"))
        .unwrap_or_else(|| panic!("DID action missing"));
    let did_receipt = receipt(ProtocolStage::DidRegistration, 0x64);
    let mut audit = AuditChain::open(&scope).unwrap_or_else(|error| panic!("audit: {error}"));
    complete_stage(
        &mut journey,
        &mut scope,
        &mut audit,
        &fixture.trace,
        action.action_key(),
        &did_receipt,
        ProtocolStage::DidRegistration,
        403,
    );
    let status = journey.status();
    assert!(status.account_active());
    assert_ne!(status.state(), OnboardingState::Complete);
    assert_eq!(status.state(), OnboardingState::ActiveRecoveryPending);
    assert_eq!(
        status.stages()[3].stage(),
        OnboardingStage::RecoveryRegistration
    );
    assert_eq!(
        status.stages()[3].state(),
        StageState::Queued { unavailable: None }
    );
}

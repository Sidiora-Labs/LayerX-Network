#[allow(dead_code)]
mod support;

use std::fs;
use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

use layerx_agentd::prepare::{
    prepare_activity, CorePreparationBoundary, CorePreparationState, CoreStateError,
    PreparationDefaults, PrepareRequest, Prepared,
};
use layerx_agentd::sign::{attach_external_signature, verify_before_submit};
use layerx_human_service::audit::{
    AuditChain, AuditEvent, Decision, SigningOperation, StepUpEvidence as AuditStepUpEvidence,
};
use layerx_human_service::custody::{
    CustodyError, CustodySigner, EnvelopeKms, KeyClass, KeyEntropy, KeyId, Keystore, KmsError,
    Operation, SignAuthorization, SignRequest, SigningLimits, StepUpEvidence,
};
use layerx_human_service::store::{PrincipalId, PrincipalStore, TenancyDigest};
use layerx_human_service::trace::TraceId;
use layerx_intents::{compile, Intent, IntentKind, LxpSend};
use layerx_types::account::AccountId;
use layerx_types::activity::{Authority, TimestampBound};
use layerx_types::amount::Amount;
use layerx_types::ids::{AssetId, Did, IdempotencyKey};
use layerx_types::intent::{
    AuthorizationSignature, ContextHash, NetworkId, ProtocolVersion, PublicKey, SendAuthorization,
    SendAuthorizationKind, Sequence, TimestampSeconds,
};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};

use support::{directory, principal, retention_uniform, tenancy};

const NETWORK_ID: u32 = 77;

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
        Poll::Pending => panic!("KMS file signer unexpectedly blocked"),
    }
}

fn activity_type() -> ActivityType {
    ActivityType::new(ModuleId::Asset, 5).unwrap_or_else(|error| panic!("activity type: {error:?}"))
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

fn send_intent(public_key: [u8; 32], amount: u128, idempotency: u8) -> Intent {
    let send = LxpSend::new(
        account("agent:did:layerx:alice:main"),
        account("agent:did:layerx:recipient:main"),
        AssetId::new([0x33; 32]),
        Amount::from_u128(amount),
        Sequence::from_u64(7),
        IdempotencyKey::new([idempotency; 32]),
        TimestampSeconds::from_u64(1_010),
        ContextHash::new([0x55; 32]),
        SendAuthorization::new(
            SendAuthorizationKind::Owner,
            PublicKey::new(public_key),
            AuthorizationSignature::new([0x77; 64]),
        ),
        NetworkId::new(NETWORK_ID).unwrap_or_else(|error| panic!("network: {error:?}")),
        ProtocolVersion::new(1).unwrap_or_else(|error| panic!("protocol: {error:?}")),
    )
    .unwrap_or_else(|error| panic!("send intent: {error:?}"));
    Intent::v1(IntentKind::LxpSend(send))
}

struct PreparedCore {
    state: CorePreparationState,
}

impl CorePreparationBoundary for PreparedCore {
    fn preparation_state(&mut self, _actor: &Did) -> Result<CorePreparationState, CoreStateError> {
        Ok(self.state.clone())
    }
}

fn prepared(public_key: [u8; 32], amount: u128, idempotency: u8) -> Prepared {
    let registry = registry();
    let compiled = compile(&send_intent(public_key, amount, idempotency), &registry)
        .unwrap_or_else(|error| panic!("compile: {error:?}"));
    let mut core = PreparedCore {
        state: CorePreparationState {
            network_id: NETWORK_ID,
            account_sequence: 7,
            protocol_timestamp: 1_000,
            observed_head_sequence: 88,
            module_registry: registry,
        },
    };
    prepare_activity(
        &mut core,
        PreparationDefaults {
            timestamp_span: 30,
            fee_limit: Amount::from_u128(12),
            maximum_payload_bytes: 1_024,
        },
        PrepareRequest {
            actor: Did::new(b"did:layerx:human-custody")
                .unwrap_or_else(|error| panic!("DID: {error:?}")),
            authority: Authority::owner(&public_key)
                .unwrap_or_else(|error| panic!("authority: {error:?}")),
            activity_type: compiled.activity_type(),
            expected_account_sequence: Some(7),
            timestamp_bound: Some(
                TimestampBound::new(995, 1_010)
                    .unwrap_or_else(|error| panic!("timestamp: {error:?}")),
            ),
            fee_limit: Some(Amount::from_u128(7)),
            idempotency_key: IdempotencyKey::new([idempotency; 32]),
            payload: compiled.payload().as_bytes().to_vec(),
            declared_payload_limit: 1_024,
        },
    )
    .unwrap_or_else(|error| panic!("prepare: {error:?}"))
}

struct Fixture {
    root: std::path::PathBuf,
    secret_path: std::path::PathBuf,
    custody_root: std::path::PathBuf,
    store_root: std::path::PathBuf,
    tenancy_digest: TenancyDigest,
    trace: TraceId,
    alice: PrincipalId,
    bob: PrincipalId,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = directory(label);
        fs::create_dir_all(&root).unwrap_or_else(|error| panic!("fixture root: {error}"));
        let secret_path = root.join("kms-mounted-root");
        fs::write(&secret_path, [0x42; 64]).unwrap_or_else(|error| panic!("KMS root: {error}"));
        let store_root = root.join("store");
        let map = tenancy(&[("alice", "tenant-a"), ("bob", "tenant-b")]);
        let tenancy_digest = map
            .install(&store_root)
            .unwrap_or_else(|error| panic!("tenancy: {error}"));
        Self {
            custody_root: root.join("custody"),
            root,
            secret_path,
            store_root,
            tenancy_digest,
            trace: TraceId::mint([0x44; 16]),
            alice: principal("alice"),
            bob: principal("bob"),
        }
    }

    fn keystore(&self) -> Keystore {
        let provider = EnvelopeKms::new("file-kms://human-primary", &self.secret_path)
            .unwrap_or_else(|error| panic!("KMS provider: {error}"));
        Keystore::open_development(&self.custody_root, NETWORK_ID, provider)
            .unwrap_or_else(|error| panic!("keystore: {error}"))
    }

    fn store(&self) -> PrincipalStore {
        PrincipalStore::open(
            &self.store_root,
            retention_uniform(10_000),
            self.tenancy_digest,
        )
        .unwrap_or_else(|error| panic!("principal store: {error}"))
    }

    fn signer(&self, limits: SigningLimits) -> CustodySigner {
        CustodySigner::new(self.keystore(), self.store(), registry(), limits)
    }

    fn audits(&self, principal: &PrincipalId) -> Vec<AuditEvent> {
        let mut store = self.store();
        let scope = store
            .principal(principal)
            .unwrap_or_else(|error| panic!("principal scope: {error}"));
        AuditChain::open(&scope)
            .unwrap_or_else(|error| panic!("audit chain: {error}"))
            .entries(&scope)
            .unwrap_or_else(|error| panic!("audit entries: {error}"))
            .into_iter()
            .map(|entry| entry.event().clone())
            .collect()
    }

    fn audit_export(&self, principal: &PrincipalId) -> Vec<u8> {
        let mut store = self.store();
        let scope = store
            .principal(principal)
            .unwrap_or_else(|error| panic!("principal scope: {error}"));
        AuditChain::open(&scope)
            .unwrap_or_else(|error| panic!("audit chain: {error}"))
            .export(&scope)
            .unwrap_or_else(|error| panic!("audit export: {error}"))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn key_id(value: &str) -> KeyId {
    KeyId::new(value).unwrap_or_else(|error| panic!("key id: {error}"))
}

fn generate(
    keystore: &Keystore,
    principal: &PrincipalId,
    key: &KeyId,
    seed: [u8; 32],
    salt: u8,
    nonce: u8,
) -> [u8; 32] {
    keystore
        .generate(
            principal,
            key,
            KeyClass::HumanPrimary,
            KeyEntropy::new(seed, [salt; 16], [nonce; 24])
                .unwrap_or_else(|error| panic!("key entropy: {error}")),
        )
        .unwrap_or_else(|error| panic!("generate: {error}"))
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

#[test]
fn real_kms_envelope_signs_only_the_exact_disclosed_bytes_and_audits_the_grant() {
    let fixture = Fixture::new("custody-real");
    let key = key_id("primary");
    let seed = [0xa5; 32];
    let keystore = fixture.keystore();
    let public_key = generate(&keystore, &fixture.alice, &key, seed, 0x11, 0x22);
    let record = fs::read(fixture.custody_root.join("principals/alice/primary.key"))
        .unwrap_or_else(|error| panic!("sealed record: {error}"));
    assert!(!contains(&record, &seed));
    assert!(!contains(&record, &[0x42; 64]));
    assert_eq!(
        keystore
            .describe(&fixture.alice, &key)
            .unwrap_or_else(|error| panic!("describe: {error}"))
            .public_key,
        public_key
    );
    assert_eq!(
        keystore
            .keys(&fixture.alice)
            .unwrap_or_else(|error| panic!("list keys: {error}")),
        vec![key.clone()]
    );

    let prepared = prepared(public_key, 25, 4);
    let signer = CustodySigner::new(
        keystore,
        fixture.store(),
        registry(),
        SigningLimits::new(10, 60).unwrap_or_else(|error| panic!("limits: {error}")),
    );
    let grant = ready(signer.sign(SignRequest::new(
        &fixture.alice,
        &key,
        &fixture.trace,
        SignAuthorization::new(Operation::ProtocolMutation, None),
        &prepared.canonical_bytes,
        &prepared.disclosure,
        100,
    )))
    .unwrap_or_else(|error| panic!("custody sign: {error}"));
    assert_eq!(grant.signer_public_key(), public_key);
    assert_eq!(grant.disclosure_digest(), prepared.disclosure_digest.0);
    let signed_activity = attach_external_signature(&prepared, *grant.signature())
        .unwrap_or_else(|error| panic!("attach signature: {error:?}"));
    assert!(verify_before_submit(&signed_activity, &prepared, &public_key, &registry()).is_ok());
    drop(signer);

    let audits = fixture.audits(&fixture.alice);
    assert_eq!(
        audits,
        vec![AuditEvent::SigningDecision {
            operation: SigningOperation::ProtocolMutation,
            disclosure_digest: prepared.disclosure_digest.0,
            step_up: AuditStepUpEvidence::NotRequired,
            outcome: Decision::Granted,
        }]
    );
    let export = fixture.audit_export(&fixture.alice);
    assert!(!contains(&export, &seed));
    assert!(!contains(&export, &prepared.canonical_bytes));
}

#[test]
#[allow(clippy::too_many_lines)]
fn step_up_and_disclosure_mismatches_are_typed_and_every_decision_is_audited() {
    let fixture = Fixture::new("custody-step-up");
    let key = key_id("primary");
    let keystore = fixture.keystore();
    let public_key = generate(&keystore, &fixture.alice, &key, [0xb5; 32], 0x12, 0x23);
    let signer = CustodySigner::new(
        keystore,
        fixture.store(),
        registry(),
        SigningLimits::new(20, 60).unwrap_or_else(|error| panic!("limits: {error}")),
    );
    let original = prepared(public_key, 25, 4);
    let altered = prepared(public_key, 26, 5);

    let missing = ready(signer.sign(SignRequest::new(
        &fixture.alice,
        &key,
        &fixture.trace,
        SignAuthorization::new(Operation::ApprovalDecision, None),
        &original.canonical_bytes,
        &original.disclosure,
        100,
    )));
    assert!(matches!(missing, Err(CustodyError::StepUpRequired)));

    let wrong_operation = StepUpEvidence::new(
        "ceremony-operation",
        Operation::Withdrawal,
        original.disclosure_digest.0,
        90,
        110,
    )
    .unwrap_or_else(|error| panic!("operation evidence: {error}"));
    let operation_mismatch = ready(signer.sign(SignRequest::new(
        &fixture.alice,
        &key,
        &fixture.trace,
        SignAuthorization::new(Operation::ApprovalDecision, Some(&wrong_operation)),
        &original.canonical_bytes,
        &original.disclosure,
        100,
    )));
    assert!(matches!(
        operation_mismatch,
        Err(CustodyError::StepUpOperationMismatch)
    ));

    let wrong = StepUpEvidence::new(
        "ceremony-wrong",
        Operation::ApprovalDecision,
        [0x99; 32],
        90,
        110,
    )
    .unwrap_or_else(|error| panic!("wrong evidence: {error}"));
    let mismatch = ready(signer.sign(SignRequest::new(
        &fixture.alice,
        &key,
        &fixture.trace,
        SignAuthorization::new(Operation::ApprovalDecision, Some(&wrong)),
        &original.canonical_bytes,
        &original.disclosure,
        100,
    )));
    assert!(matches!(mismatch, Err(CustodyError::StepUpMismatch)));

    let future = StepUpEvidence::new(
        "ceremony-future",
        Operation::Withdrawal,
        original.disclosure_digest.0,
        101,
        110,
    )
    .unwrap_or_else(|error| panic!("future evidence: {error}"));
    let not_yet = ready(signer.sign(SignRequest::new(
        &fixture.alice,
        &key,
        &fixture.trace,
        SignAuthorization::new(Operation::Withdrawal, Some(&future)),
        &original.canonical_bytes,
        &original.disclosure,
        100,
    )));
    assert!(matches!(not_yet, Err(CustodyError::StepUpNotYetValid)));

    let expired = StepUpEvidence::new(
        "ceremony-expired",
        Operation::WalletRebinding,
        original.disclosure_digest.0,
        90,
        100,
    )
    .unwrap_or_else(|error| panic!("expired evidence: {error}"));
    let stale = ready(signer.sign(SignRequest::new(
        &fixture.alice,
        &key,
        &fixture.trace,
        SignAuthorization::new(Operation::WalletRebinding, Some(&expired)),
        &original.canonical_bytes,
        &original.disclosure,
        100,
    )));
    assert!(matches!(stale, Err(CustodyError::StepUpExpired)));

    let fresh = StepUpEvidence::new(
        "ceremony-fresh",
        Operation::EmergencyExit,
        original.disclosure_digest.0,
        90,
        110,
    )
    .unwrap_or_else(|error| panic!("fresh evidence: {error}"));
    assert!(ready(signer.sign(SignRequest::new(
        &fixture.alice,
        &key,
        &fixture.trace,
        SignAuthorization::new(Operation::EmergencyExit, Some(&fresh)),
        &original.canonical_bytes,
        &original.disclosure,
        100,
    )))
    .is_ok());
    let replayed = ready(signer.sign(SignRequest::new(
        &fixture.alice,
        &key,
        &fixture.trace,
        SignAuthorization::new(Operation::EmergencyExit, Some(&fresh)),
        &original.canonical_bytes,
        &original.disclosure,
        101,
    )));
    assert!(matches!(replayed, Err(CustodyError::StepUpReplayed)));

    let wrong_bytes = ready(signer.sign(SignRequest::new(
        &fixture.alice,
        &key,
        &fixture.trace,
        SignAuthorization::new(Operation::ProtocolMutation, None),
        &altered.canonical_bytes,
        &original.disclosure,
        101,
    )));
    assert!(matches!(
        wrong_bytes,
        Err(CustodyError::Sign(
            layerx_crypto::signer::SignError::DisclosureMismatch(_)
        ))
    ));
    drop(signer);

    let audits = fixture.audits(&fixture.alice);
    assert_eq!(audits.len(), 8);
    assert_eq!(
        audits
            .iter()
            .filter(|event| matches!(
                event,
                AuditEvent::SigningDecision {
                    outcome: Decision::Refused,
                    ..
                }
            ))
            .count(),
        7
    );
    assert!(audits.iter().any(|event| matches!(
        event,
        AuditEvent::SigningDecision {
            operation: SigningOperation::EmergencyExit,
            step_up: AuditStepUpEvidence::Fresh { ceremony_digest },
            outcome: Decision::Granted,
            ..
        } if *ceremony_digest != [0; 32]
    )));
}

#[test]
fn throughput_is_durable_per_principal_and_kms_loss_has_no_fallback() {
    let fixture = Fixture::new("custody-rate");
    let key = key_id("primary");
    let keystore = fixture.keystore();
    let alice_key = generate(&keystore, &fixture.alice, &key, [0xc5; 32], 0x13, 0x24);
    let bob_key = generate(&keystore, &fixture.bob, &key, [0xd5; 32], 0x14, 0x25);
    drop(keystore);
    let alice = prepared(alice_key, 25, 4);
    let bob = prepared(bob_key, 30, 6);
    let limits = SigningLimits::new(2, 10).unwrap_or_else(|error| panic!("limits: {error}"));
    let signer = fixture.signer(limits);

    for now in [100, 101] {
        assert!(ready(signer.sign(SignRequest::new(
            &fixture.alice,
            &key,
            &fixture.trace,
            SignAuthorization::new(Operation::ProtocolMutation, None),
            &alice.canonical_bytes,
            &alice.disclosure,
            now,
        )))
        .is_ok());
    }
    let limited = ready(signer.sign(SignRequest::new(
        &fixture.alice,
        &key,
        &fixture.trace,
        SignAuthorization::new(Operation::ProtocolMutation, None),
        &alice.canonical_bytes,
        &alice.disclosure,
        102,
    )));
    assert!(matches!(
        limited,
        Err(CustodyError::ThroughputExceeded { retry_at: 110 })
    ));
    assert!(ready(signer.sign(SignRequest::new(
        &fixture.bob,
        &key,
        &fixture.trace,
        SignAuthorization::new(Operation::ProtocolMutation, None),
        &bob.canonical_bytes,
        &bob.disclosure,
        102,
    )))
    .is_ok());
    drop(signer);

    let signer = fixture.signer(limits);
    let persisted = ready(signer.sign(SignRequest::new(
        &fixture.alice,
        &key,
        &fixture.trace,
        SignAuthorization::new(Operation::ProtocolMutation, None),
        &alice.canonical_bytes,
        &alice.disclosure,
        105,
    )));
    assert!(matches!(
        persisted,
        Err(CustodyError::ThroughputExceeded { retry_at: 110 })
    ));
    assert!(ready(signer.sign(SignRequest::new(
        &fixture.alice,
        &key,
        &fixture.trace,
        SignAuthorization::new(Operation::ProtocolMutation, None),
        &alice.canonical_bytes,
        &alice.disclosure,
        110,
    )))
    .is_ok());

    fs::remove_file(&fixture.secret_path)
        .unwrap_or_else(|error| panic!("remove KMS mount: {error}"));
    let unavailable = ready(signer.sign(SignRequest::new(
        &fixture.bob,
        &key,
        &fixture.trace,
        SignAuthorization::new(Operation::ProtocolMutation, None),
        &bob.canonical_bytes,
        &bob.disclosure,
        111,
    )));
    assert!(matches!(
        unavailable,
        Err(CustodyError::Kms(KmsError::Unavailable))
    ));
    drop(signer);

    assert_eq!(fixture.audits(&fixture.alice).len(), 5);
    let bob_audits = fixture.audits(&fixture.bob);
    assert_eq!(bob_audits.len(), 2);
    assert!(bob_audits.iter().any(|event| matches!(
        event,
        AuditEvent::SigningDecision {
            outcome: Decision::Refused,
            ..
        }
    )));
}

#[test]
fn copied_principal_record_cannot_cross_the_authenticated_kms_identity() {
    let fixture = Fixture::new("custody-isolation");
    let key = key_id("primary");
    let keystore = fixture.keystore();
    let public_key = generate(&keystore, &fixture.alice, &key, [0xe5; 32], 0x15, 0x26);
    drop(keystore);
    let alice_record = fixture.custody_root.join("principals/alice/primary.key");
    let bob_directory = fixture.custody_root.join("principals/bob");
    fs::create_dir_all(&bob_directory).unwrap_or_else(|error| panic!("bob key directory: {error}"));
    fs::copy(&alice_record, bob_directory.join("primary.key"))
        .unwrap_or_else(|error| panic!("copy sealed record: {error}"));

    let prepared = prepared(public_key, 25, 4);
    let signer = fixture
        .signer(SigningLimits::new(10, 60).unwrap_or_else(|error| panic!("limits: {error}")));
    let crossed = ready(signer.sign(SignRequest::new(
        &fixture.bob,
        &key,
        &fixture.trace,
        SignAuthorization::new(Operation::ProtocolMutation, None),
        &prepared.canonical_bytes,
        &prepared.disclosure,
        100,
    )));
    assert!(matches!(crossed, Err(CustodyError::Kms(KmsError::Refused))));
    assert!(ready(signer.sign(SignRequest::new(
        &fixture.alice,
        &key,
        &fixture.trace,
        SignAuthorization::new(Operation::ProtocolMutation, None),
        &prepared.canonical_bytes,
        &prepared.disclosure,
        100,
    )))
    .is_ok());
    drop(signer);

    assert!(matches!(
        fixture.audits(&fixture.bob).as_slice(),
        [AuditEvent::SigningDecision {
            outcome: Decision::Refused,
            ..
        }]
    ));
    assert!(matches!(
        fixture.audits(&fixture.alice).as_slice(),
        [AuditEvent::SigningDecision {
            outcome: Decision::Granted,
            ..
        }]
    ));
}

#[cfg(unix)]
#[test]
fn principal_symlinks_are_refused_before_any_key_access() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new("custody-symlink");
    let outside = fixture.root.join("outside-principal");
    fs::create_dir_all(&outside).unwrap_or_else(|error| panic!("outside directory: {error}"));
    let principals = fixture.custody_root.join("principals");
    fs::create_dir_all(&principals).unwrap_or_else(|error| panic!("principals: {error}"));
    symlink(&outside, principals.join("alice"))
        .unwrap_or_else(|error| panic!("principal symlink: {error}"));
    let provider = EnvelopeKms::new("file-kms://human-primary", &fixture.secret_path)
        .unwrap_or_else(|error| panic!("KMS provider: {error}"));
    assert!(matches!(
        Keystore::open_development(&fixture.custody_root, NETWORK_ID, provider),
        Err(CustodyError::CorruptRecord("foreign principals entry"))
    ));
}

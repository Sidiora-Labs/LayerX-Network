use layerx_human_test_support as support;

use std::fs;
use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

use layerx_agentd::prepare::{
    prepare_activity, CorePreparationBoundary, CorePreparationState, CoreStateError,
    PreparationDefaults, PrepareRequest, Prepared,
};
use layerx_human_service::audit::{AuditChain, AuditEvent, Decision};
use layerx_human_service::custody::{
    CustodyError, CustodySigner, EnvelopeKms, KeyClass, KeyEntropy, KeyId, Keystore, Operation,
    SignAuthorization, SignRequest, SigningLimits,
};
use layerx_human_service::security::{
    security_action_digest, KeyExportCeremony, SecurityAction, SecurityError,
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

fn send_intent(public_key: [u8; 32], signing_key: [u8; 32], amount: u128) -> Intent {
    let signer = layerx_crypto::local::LocalSigner::new(signing_key);
    assert_eq!(
        layerx_crypto::signer::Signer::public_key(&signer),
        public_key
    );
    let debit = layerx_crypto::send::SendDebit {
        from: layerx_intents::canonical::account_id_for_protocol(
            &account("agent:did:layerx:alice:main"),
            layerx_intents::canonical::PROTOCOL_VERSION,
        )
        .unwrap_or_else(|error| panic!("source account: {error:?}")),
        to: layerx_intents::canonical::account_id_for_protocol(
            &account("agent:did:layerx:recipient:main"),
            layerx_intents::canonical::PROTOCOL_VERSION,
        )
        .unwrap_or_else(|error| panic!("destination account: {error:?}")),
        asset: [0x33; 32],
        amount,
        source_sequence: 7,
        idempotency_key: [4; 32],
        expires_at: 1_010,
        context_hash: [0x55; 32],
        conditions: Vec::new(),
        authorization_kind: SendAuthorizationKind::Owner as u8,
        network_id: NETWORK_ID,
        protocol_version: layerx_intents::canonical::PROTOCOL_VERSION,
    };

    let send = LxpSend::new(
        account("agent:did:layerx:alice:main"),
        account("agent:did:layerx:recipient:main"),
        AssetId::new([0x33; 32]),
        Amount::from_u128(amount),
        Sequence::from_u64(7),
        IdempotencyKey::new([4; 32]),
        TimestampSeconds::from_u64(1_010),
        ContextHash::new([0x55; 32]),
        support::sign_send(
            &signer,
            &debit,
            SendAuthorization::new(
                SendAuthorizationKind::Owner,
                PublicKey::new(public_key),
                AuthorizationSignature::new([0x77; 64]),
            ),
        ),
        NetworkId::new(NETWORK_ID).unwrap_or_else(|error| panic!("network: {error:?}")),
        ProtocolVersion::new(layerx_intents::canonical::PROTOCOL_VERSION)
            .unwrap_or_else(|error| panic!("protocol: {error:?}")),
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

fn prepared(public_key: [u8; 32], seed: [u8; 32], amount: u128) -> Prepared {
    let registry = registry();
    let compiled = compile(&send_intent(public_key, seed, amount), &registry)
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
            idempotency_key: IdempotencyKey::new([4; 32]),
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
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = directory(label);
        fs::create_dir_all(&root).unwrap_or_else(|error| panic!("fixture root: {error}"));
        let secret_path = root.join("kms-mounted-root");
        fs::write(&secret_path, [0x42; 64]).unwrap_or_else(|error| panic!("KMS root: {error}"));
        let store_root = root.join("store");
        let map = tenancy(&[("alice", "tenant-a")]);
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

fn generate(keystore: &Keystore, principal: &PrincipalId, key: &KeyId, seed: [u8; 32]) -> [u8; 32] {
    keystore
        .generate(
            principal,
            key,
            KeyClass::HumanPrimary,
            KeyEntropy::new(seed, [0x11; 16], [0x22; 24])
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
#[allow(clippy::too_many_lines)]
fn key_export_ceremony_is_the_only_route_that_ever_returns_the_primary_key() {
    let fixture = Fixture::new("key-export-only-route");
    let key = key_id("primary");
    let seed = [0xa5; 32];
    let keystore = fixture.keystore();
    let public_key = generate(&keystore, &fixture.alice, &key, seed);

    let record_path = fixture.custody_root.join("principals/alice/primary.key");
    let sealed = fs::read(&record_path).unwrap_or_else(|error| panic!("sealed record: {error}"));
    assert!(!contains(&sealed, &seed));
    assert!(!contains(&sealed, &[0x42; 64]));
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
    assert!(!keystore
        .self_custodied(&fixture.alice, &key)
        .unwrap_or_else(|error| panic!("custody posture: {error}")));

    let prepared = prepared(public_key, seed, 25);
    let signer = CustodySigner::new(
        fixture.keystore(),
        fixture.store(),
        registry(),
        SigningLimits::new(10, 60).unwrap_or_else(|error| panic!("limits: {error}")),
    );
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

    let exported = keystore
        .export_primary_once(&fixture.alice, &key)
        .unwrap_or_else(|error| panic!("key export: {error}"));
    assert_eq!(exported.expose(), &seed);
    assert_eq!(exported.public_key(), public_key);
    assert!(keystore
        .self_custodied(&fixture.alice, &key)
        .unwrap_or_else(|error| panic!("custody posture: {error}")));

    assert!(matches!(
        keystore.export_primary_once(&fixture.alice, &key),
        Err(CustodyError::SelfCustodied)
    ));
    assert!(matches!(
        keystore.rotate(&fixture.alice, &key),
        Err(CustodyError::SelfCustodied)
    ));
    let refused = ready(signer.sign(SignRequest::new(
        &fixture.alice,
        &key,
        &fixture.trace,
        SignAuthorization::new(Operation::ProtocolMutation, None),
        &prepared.canonical_bytes,
        &prepared.disclosure,
        101,
    )));
    assert!(matches!(refused, Err(CustodyError::SelfCustodied)));
    drop(signer);

    assert_eq!(
        keystore
            .describe(&fixture.alice, &key)
            .unwrap_or_else(|error| panic!("describe after export: {error}"))
            .public_key,
        public_key
    );
    let sealed = fs::read(&record_path).unwrap_or_else(|error| panic!("sealed record: {error}"));
    assert!(!contains(&sealed, &seed));
    let reopened = fixture.keystore();
    assert!(reopened
        .self_custodied(&fixture.alice, &key)
        .unwrap_or_else(|error| panic!("durable custody posture: {error}")));

    let audits = fixture.audits(&fixture.alice);
    assert_eq!(audits.len(), 2);
    assert!(matches!(
        audits[1],
        AuditEvent::SigningDecision {
            outcome: Decision::Refused,
            ..
        }
    ));
    let export = fixture.audit_export(&fixture.alice);
    assert!(!contains(&export, &seed));
    assert!(!contains(&export, &prepared.canonical_bytes));
}

#[test]
fn key_export_begin_offers_one_digest_and_refuses_an_identity_that_already_holds_its_key() {
    let fixture = Fixture::new("key-export-begin");
    let key = key_id("primary");
    let seed = [0xb5; 32];
    let keystore = fixture.keystore();
    generate(&keystore, &fixture.alice, &key, seed);
    let ceremony = KeyExportCeremony::new(key.clone(), 300)
        .unwrap_or_else(|error| panic!("ceremony: {error}"));

    let challenge = ceremony
        .begin(&keystore, &fixture.alice, 1_000)
        .unwrap_or_else(|error| panic!("begin: {error}"));
    let expected = security_action_digest(&fixture.alice, SecurityAction::ExportPrimaryKey, None)
        .unwrap_or_else(|error| panic!("action digest: {error}"));
    assert_eq!(challenge.confirms, expected);
    assert_eq!(challenge.expires_at, 1_300);
    let repeated = ceremony
        .begin(&keystore, &fixture.alice, 1_010)
        .unwrap_or_else(|error| panic!("repeated begin: {error}"));
    assert_eq!(repeated.export_id, challenge.export_id);
    assert_eq!(repeated.confirms, challenge.confirms);
    assert_ne!(
        challenge.confirms,
        security_action_digest(&fixture.alice, SecurityAction::RevealRecoveryEvidence, None)
            .unwrap_or_else(|error| panic!("reveal digest: {error}"))
    );

    keystore
        .export_primary_once(&fixture.alice, &key)
        .unwrap_or_else(|error| panic!("key export: {error}"));
    assert!(matches!(
        ceremony.begin(&keystore, &fixture.alice, 1_020),
        Err(SecurityError::KeyExported)
    ));
}

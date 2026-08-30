#[allow(dead_code)]
mod support;

use std::fmt::Display;
use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use ciborium::value::Value as CborValue;
use ed25519_dalek::{Signer as _, SigningKey as Ed25519SigningKey};
use k256::ecdsa::SigningKey;
use layerx_agentd::outbox::{Outbox, SubmissionState};
use layerx_agentd::prepare::{
    prepare_activity, CorePreparationBoundary, CorePreparationState, CoreStateError,
    PreparationDefaults, PrepareRequest,
};
use layerx_agentd::sign::{attach_external_signature, verify_before_submit};
use layerx_agentd::store::{Store as AgentStore, TenantId};
use layerx_crypto::local::LocalSigner;
use layerx_crypto::signer::{sign_disclosed, Signer};
use layerx_human_service::audit::{
    AuditChain, AuditEvent, IdentityEvent, NotificationClass, SecurityChangeKind,
};
use layerx_human_service::auth::{
    AccountIdentity, AuthConfig, Device, Passkeys, RateLimit, SessionGrant,
};
use layerx_human_service::binding::{
    AgentBindingContract, AgentBindingError, AgentBindingReceipt, AgentSubmission,
    BindingAgentRequest, BindingError, BindingJourney, BindingState,
};
use layerx_human_service::store::PrincipalScope;
use layerx_human_service::trace::TraceId;
use layerx_proof::receipt::AuthorizedBatch;
use layerx_types::activity::Authority;
use layerx_types::amount::Amount;
use layerx_types::ids::{Did, IdempotencyKey};
use layerx_types::intent::{EvmAddress, NetworkId};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};
use sha3::Keccak256;

use support::{directory, install_and_open, principal, retention_uniform, tenancy};

const RP_ID: &str = "id.layerx.example";
const ORIGIN: &str = "https://id.layerx.example";
const FLAG_UP: u8 = 1;
const FLAG_UV: u8 = 1 << 2;
const FLAG_AT: u8 = 1 << 6;

fn required<T, E: Display>(result: Result<T, E>, label: &str) -> T {
    result.unwrap_or_else(|error| panic!("{label}: {error}"))
}

fn registry() -> ModuleRegistry {
    let binding = ActivityType::new(ModuleId::Governance, 4)
        .unwrap_or_else(|error| panic!("binding activity: {error:?}"));
    let registration = ModuleRegistration::new(ModuleId::Governance, &[binding])
        .unwrap_or_else(|error| panic!("governance registration: {error:?}"));
    ModuleRegistry::new(&[registration])
        .unwrap_or_else(|error| panic!("module registry: {error:?}"))
}

fn evm_address(key: &SigningKey) -> EvmAddress {
    let encoded = key.verifying_key().to_encoded_point(false);
    let hash = Keccak256::digest(&encoded.as_bytes()[1..]);
    let mut bytes = [0_u8; 20];
    bytes.copy_from_slice(&hash[12..]);
    EvmAddress::new(bytes)
}

fn ownership_signature(key: &SigningKey, digest: [u8; 32]) -> [u8; 65] {
    let (signature, recovery) = key
        .sign_prehash_recoverable(&digest)
        .unwrap_or_else(|error| panic!("ownership signature: {error}"));
    let mut bytes = [0_u8; 65];
    bytes[..64].copy_from_slice(&signature.to_bytes());
    bytes[64] = recovery.to_byte().saturating_add(27);
    bytes
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
        Poll::Pending => panic!("local signer unexpectedly blocked"),
    }
}

struct VerifiedCorePreparation {
    registry: ModuleRegistry,
}

impl CorePreparationBoundary for VerifiedCorePreparation {
    fn preparation_state(&mut self, _actor: &Did) -> Result<CorePreparationState, CoreStateError> {
        Ok(CorePreparationState {
            network_id: 17,
            account_sequence: 7,
            protocol_timestamp: 100,
            observed_head_sequence: 91,
            module_registry: self.registry.clone(),
        })
    }
}

/// The complete in-process agent path: core-derived preparation, semantic
/// disclosure, external signing, exact-byte verification, and durable outbox.
struct InProcessAgentContract {
    accepted: usize,
    registry: ModuleRegistry,
    signer: LocalSigner,
    store: AgentStore,
    outbox: Outbox,
    tenant: TenantId,
}

impl InProcessAgentContract {
    fn new(root: &std::path::Path) -> Self {
        Self {
            accepted: 0,
            registry: registry(),
            signer: LocalSigner::new([0xa5; 32]),
            store: AgentStore::open(root)
                .unwrap_or_else(|error| panic!("open agent store: {error}")),
            outbox: Outbox::default(),
            tenant: TenantId::new("tenant-a")
                .unwrap_or_else(|error| panic!("agent tenant: {error}")),
        }
    }

    fn mark_executed(&mut self, submission: AgentSubmission, receipt: AgentBindingReceipt) {
        for state in [SubmissionState::Submitted, SubmissionState::Acknowledged] {
            self.outbox
                .transition(
                    &mut self.store,
                    submission.submission_id,
                    state,
                    format!("binding {state:?}"),
                    None,
                )
                .unwrap_or_else(|error| panic!("agent transition: {error:?}"));
        }
        let signer = Ed25519SigningKey::from_bytes(&[0x35; 32]);
        let raw = support::raw_receipt_evidence(
            receipt.canonical_receipt,
            receipt.authorized_batch,
            9,
            &signer,
        );
        let verified = support::evidence_verifier(&signer)
            .verify_receipt(&raw)
            .unwrap_or_else(|error| panic!("binding receipt verification: {error:?}"));
        self.outbox
            .transition(
                &mut self.store,
                submission.submission_id,
                SubmissionState::Executed,
                "cryptographically verified binding receipt",
                Some(verified),
            )
            .unwrap_or_else(|error| panic!("agent execution transition: {error:?}"));
    }
}

impl AgentBindingContract for InProcessAgentContract {
    fn submit_binding(
        &mut self,
        request: BindingAgentRequest<'_>,
    ) -> Result<AgentSubmission, AgentBindingError> {
        if !matches!(
            request.intent.kind(),
            layerx_intents::IntentKind::EvmPayoutBinding(_)
        ) || request.compiled.activity_type().module() != ModuleId::Governance
            || request.compiled.activity_type().ordinal() != 4
            || request.compiled.payload().as_bytes().get(..4) != Some(&[0x71, 0x04, 0, 4])
        {
            return Err(AgentBindingError::ContractViolation);
        }
        let mut core = VerifiedCorePreparation {
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
                expected_account_sequence: Some(7),
                timestamp_bound: None,
                fee_limit: Some(Amount::from_u128(1)),
                idempotency_key: request.idempotency_key,
                payload: request.compiled.payload().as_bytes().to_vec(),
                declared_payload_limit: 512,
            },
        )
        .map_err(|_| AgentBindingError::ContractViolation)?;
        if !prepared
            .disclosure
            .evm_payout_binding
            .is_some_and(|binding| {
                let mut did_hasher = Sha256::new();
                did_hasher.update(b"LXP/v1/did-id\0");
                did_hasher.update(request.actor.as_bytes());
                let did_id: [u8; 32] = did_hasher.finalize().into();
                binding.did_id == did_id
                    && binding.payout_address == request.recovered_signer.bytes()
                    && binding.network_id == 17
            })
        {
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
        self.accepted = self.accepted.saturating_add(1);
        let submission_id = request.idempotency_key.bytes();
        let activity_id = verified.activity_id();
        self.outbox
            .enqueue(
                &mut self.store,
                self.tenant.clone(),
                submission_id,
                verified,
            )
            .map_err(|_| AgentBindingError::Unavailable)?;
        self.outbox
            .bytes_for_transmission(submission_id)
            .map_err(|_| AgentBindingError::Unavailable)?;
        Ok(AgentSubmission {
            submission_id,
            activity_id,
        })
    }
}

#[derive(Clone)]
struct ReceiptFields {
    activity_id: [u8; 32],
    previous_state_root: [u8; 32],
    resulting_state_root: [u8; 32],
    batch_id: [u8; 32],
    asset: [u8; 32],
    operation: u8,
    recorded_address: [u8; 20],
}

fn push_bytes(output: &mut Vec<u8>, bytes: &[u8]) {
    let length = u32::try_from(bytes.len()).unwrap_or_else(|_| panic!("receipt field too long"));
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(bytes);
}

fn encode_receipt(fields: &ReceiptFields, signature: Option<[u8; 64]>) -> Vec<u8> {
    let mut output = Vec::new();
    output.extend_from_slice(&1_u16.to_be_bytes());
    output.extend_from_slice(&0x5201_u16.to_be_bytes());
    output.extend_from_slice(&1_u16.to_be_bytes());
    push_bytes(&mut output, &fields.activity_id);
    output.extend_from_slice(&9_u64.to_be_bytes());
    push_bytes(&mut output, &fields.previous_state_root);
    push_bytes(&mut output, &fields.resulting_state_root);
    push_bytes(&mut output, &[8; 32]);
    output.extend_from_slice(&0_i32.to_be_bytes());
    output.extend_from_slice(&0_u32.to_be_bytes());
    output.extend_from_slice(&0_u128.to_be_bytes());
    push_bytes(&mut output, &fields.batch_id);
    output.extend_from_slice(&(ModuleId::Governance as u16).to_be_bytes());
    output.extend_from_slice(&1_u32.to_be_bytes());
    output.extend_from_slice(&1_u32.to_be_bytes());
    output.push(fields.operation);
    push_bytes(&mut output, &fields.asset);
    output.extend_from_slice(&0_u128.to_be_bytes());
    push_bytes(&mut output, &[6; 32]);
    output.extend_from_slice(&100_u128.to_be_bytes());
    output.extend_from_slice(&100_u128.to_be_bytes());
    output.extend_from_slice(&1_u64.to_be_bytes());
    let mut receipt_address = [0_u8; 32];
    receipt_address[12..].copy_from_slice(&fields.recorded_address);
    push_bytes(&mut output, &receipt_address);
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
}

fn binding_receipt(
    submission: AgentSubmission,
    address: EvmAddress,
    operation: u8,
) -> AgentBindingReceipt {
    let signing_key = Ed25519SigningKey::from_bytes(&[0x35; 32]);
    let fields = ReceiptFields {
        activity_id: submission.activity_id,
        previous_state_root: [2; 32],
        resulting_state_root: [3; 32],
        batch_id: support::execution_batch_id([2; 32], submission.activity_id, 9),
        asset: [5; 32],
        operation,
        recorded_address: address.bytes(),
    };
    let unsigned = encode_receipt(&fields, None);
    let mut hasher = Sha256::new();
    hasher.update(b"LXP/v1/receipt\0");
    hasher.update(&unsigned);
    let digest: [u8; 32] = hasher.finalize().into();
    let canonical_receipt = encode_receipt(&fields, Some(signing_key.sign(&digest).to_bytes()));
    AgentBindingReceipt {
        submission_id: submission.submission_id,
        canonical_receipt,
        authorized_batch: AuthorizedBatch::new(
            fields.batch_id,
            fields.asset,
            fields.previous_state_root,
            fields.resulting_state_root,
            signing_key.verifying_key().to_bytes(),
        ),
    }
}

fn auth_config() -> AuthConfig {
    AuthConfig {
        rp_id: RP_ID.to_owned(),
        rp_name: "LayerX".to_owned(),
        origin: ORIGIN.to_owned(),
        ceremony_ttl_secs: 300,
        assertion_ttl_secs: 30,
        session_ttl_secs: 300,
        refresh_ttl_secs: 600,
        step_up_ttl_secs: 30,
        rate_limit: RateLimit {
            attempts: 100,
            window_secs: 60,
        },
    }
}

fn decode_ceremony(value: &str) -> Value {
    let bytes = required(URL_SAFE_NO_PAD.decode(value), "decode ceremony");
    required(serde_json::from_slice(&bytes), "parse ceremony")
}

fn required_text<'a>(value: &'a Value, pointer: &str) -> &'a str {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("missing ceremony field {pointer}"))
}

fn encode_response(value: &Value) -> String {
    URL_SAFE_NO_PAD.encode(required(serde_json::to_vec(value), "serialize response"))
}

struct SoftwareAuthenticator {
    signing_key: Ed25519SigningKey,
    credential_id: Vec<u8>,
    counter: u32,
    user_handle: Option<String>,
}

impl SoftwareAuthenticator {
    fn new() -> Self {
        let mut seed = [0_u8; 32];
        required(getrandom::fill(&mut seed), "authenticator entropy");
        let mut credential_id = vec![0_u8; 32];
        required(getrandom::fill(&mut credential_id), "credential entropy");
        Self {
            signing_key: Ed25519SigningKey::from_bytes(&seed),
            credential_id,
            counter: 0,
            user_handle: None,
        }
    }

    fn register(&mut self, ceremony: &str) -> String {
        let options = decode_ceremony(ceremony);
        self.user_handle = Some(required_text(&options, "/user/id").to_owned());
        let client_data = client_data("webauthn.create", required_text(&options, "/challenge"));
        encode_response(&json!({
            "id": URL_SAFE_NO_PAD.encode(&self.credential_id),
            "transports": ["internal"],
            "attestationObject": URL_SAFE_NO_PAD.encode(self.attestation_object()),
            "clientDataJSON": URL_SAFE_NO_PAD.encode(client_data),
        }))
    }

    fn assert(&mut self, ceremony: &str) -> String {
        let options = decode_ceremony(ceremony);
        self.counter = self.counter.saturating_add(1);
        let authenticator_data = self.authenticator_data(self.counter, false);
        let client_data = client_data("webauthn.get", required_text(&options, "/challenge"));
        let client_hash = Sha256::digest(&client_data);
        let mut signed = authenticator_data.clone();
        signed.extend_from_slice(&client_hash);
        let signature = self.signing_key.sign(&signed).to_bytes();
        encode_response(&json!({
            "id": URL_SAFE_NO_PAD.encode(&self.credential_id),
            "authenticatorData": URL_SAFE_NO_PAD.encode(authenticator_data),
            "signature": URL_SAFE_NO_PAD.encode(signature),
            "clientDataJSON": URL_SAFE_NO_PAD.encode(client_data),
            "userHandle": self.user_handle,
        }))
    }

    fn attestation_object(&self) -> Vec<u8> {
        let map = CborValue::Map(vec![
            (
                CborValue::Text("fmt".to_owned()),
                CborValue::Text("none".to_owned()),
            ),
            (
                CborValue::Text("attStmt".to_owned()),
                CborValue::Map(Vec::new()),
            ),
            (
                CborValue::Text("authData".to_owned()),
                CborValue::Bytes(self.authenticator_data(0, true)),
            ),
        ]);
        let mut bytes = Vec::new();
        required(
            ciborium::ser::into_writer(&map, &mut bytes),
            "encode attestation",
        );
        bytes
    }

    fn authenticator_data(&self, counter: u32, attested: bool) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&Sha256::digest(RP_ID.as_bytes()));
        bytes.push(FLAG_UP | FLAG_UV | if attested { FLAG_AT } else { 0 });
        bytes.extend_from_slice(&counter.to_be_bytes());
        if attested {
            bytes.extend_from_slice(&[0_u8; 16]);
            let length = u16::try_from(self.credential_id.len())
                .unwrap_or_else(|_| panic!("credential identifier too long"));
            bytes.extend_from_slice(&length.to_be_bytes());
            bytes.extend_from_slice(&self.credential_id);
            bytes.extend_from_slice(&self.cose_public_key());
        }
        bytes
    }

    fn cose_public_key(&self) -> Vec<u8> {
        let map = CborValue::Map(vec![
            (CborValue::Integer(1.into()), CborValue::Integer(1.into())),
            (
                CborValue::Integer(3.into()),
                CborValue::Integer((-8).into()),
            ),
            (
                CborValue::Integer((-1).into()),
                CborValue::Integer(6.into()),
            ),
            (
                CborValue::Integer((-2).into()),
                CborValue::Bytes(self.signing_key.verifying_key().to_bytes().to_vec()),
            ),
        ]);
        let mut bytes = Vec::new();
        required(
            ciborium::ser::into_writer(&map, &mut bytes),
            "encode public key",
        );
        bytes
    }
}

fn client_data(kind: &str, challenge: &str) -> Vec<u8> {
    required(
        serde_json::to_vec(&json!({
            "type": kind,
            "challenge": challenge,
            "origin": ORIGIN,
            "crossOrigin": false,
        })),
        "encode client data",
    )
}

fn open_session(
    passkeys: &Passkeys,
    scope: &mut PrincipalScope<'_>,
    authenticator: &mut SoftwareAuthenticator,
    now: u64,
) -> SessionGrant {
    let identity = required(AccountIdentity::new("mara@example.com", "Mara"), "identity");
    let registration = required(
        passkeys.begin_registration(scope, &identity, "Phone passkey", now),
        "begin registration",
    );
    let response = authenticator.register(&registration.ceremony);
    required(
        passkeys.finish_registration(scope, &registration.registration_id, &response, now + 1),
        "finish registration",
    );
    let assertion = required(passkeys.begin_assertion(scope, now + 2), "begin assertion");
    let response = authenticator.assert(&assertion.ceremony);
    required(
        passkeys.finish_assertion(scope, &assertion.assertion_id, &response, now + 3),
        "finish assertion",
    );
    let device = required(
        Device::new("dev_aabbccddeeff00112233445566778899", "Phone", "ios"),
        "device",
    );
    required(
        passkeys.open_session(scope, &assertion.assertion_id, device, now + 4),
        "open session",
    )
}

#[test]
fn initial_binding_requires_real_signature_and_verified_matching_receipt() {
    let root = directory("binding-initial");
    let map = tenancy(&[("alice", "tenant-a")]);
    let (mut store, _) = install_and_open(&root, &map, retention_uniform(10_000));
    let mut scope = required(store.principal(&principal("alice")), "principal");
    let journey = BindingJourney::new(registry());
    let did = Did::new(b"did:layerx:alice").unwrap_or_else(|error| panic!("DID: {error:?}"));
    let network = NetworkId::new(17).unwrap_or_else(|error| panic!("network: {error:?}"));
    let owner = SigningKey::from_bytes((&[0x11; 32]).into())
        .unwrap_or_else(|error| panic!("owner key: {error}"));
    let address = evm_address(&owner);
    let statement = required(
        BindingJourney::issue_statement(&did, network, address, 100, 60),
        "statement",
    );
    assert!(statement.text().contains("DID: did:layerx:alice"));
    assert!(statement.text().contains("network_id: 17"));
    assert!(statement.text().contains(&statement.checksummed_address()));
    assert!(statement
        .text()
        .contains("grants no authority to move funds"));

    let wrong = SigningKey::from_bytes((&[0x12; 32]).into())
        .unwrap_or_else(|error| panic!("wrong key: {error}"));
    let mut agent = InProcessAgentContract::new(&root.join("agentd"));
    let refusal = journey.submit_initial(
        &mut scope,
        &statement,
        &ownership_signature(&wrong, statement.signing_digest()),
        IdempotencyKey::new([0x41; 32]),
        &mut agent,
        101,
    );
    assert!(matches!(refusal, Err(BindingError::SignerAddressMismatch)));
    assert_eq!(agent.accepted, 0);

    let submission = required(
        journey.submit_initial(
            &mut scope,
            &statement,
            &ownership_signature(&owner, statement.signing_digest()),
            IdempotencyKey::new([0x42; 32]),
            &mut agent,
            102,
        ),
        "submit binding",
    );
    assert!(
        matches!(journey.state(&scope), Ok(BindingState::Binding { candidate }) if candidate == address.bytes())
    );

    let mismatch = journey.finalize(
        &mut scope,
        &binding_receipt(submission, evm_address(&wrong), 4),
        103,
        &TraceId::mint([1; 16]),
    );
    assert!(matches!(
        mismatch,
        Err(BindingError::ReceiptAddressMismatch)
    ));
    assert!(matches!(
        journey.state(&scope),
        Ok(BindingState::Binding { .. })
    ));

    let accepted_receipt = binding_receipt(submission, address, 4);
    let active = required(
        journey.finalize(&mut scope, &accepted_receipt, 104, &TraceId::mint([2; 16])),
        "finalize binding",
    );
    assert_eq!(active.address(), address.bytes());
    agent.mark_executed(submission, accepted_receipt);
    assert!(agent
        .outbox
        .status(submission.submission_id)
        .is_some_and(|status| status.state == SubmissionState::Executed));
    assert!(matches!(journey.state(&scope), Ok(BindingState::Active(value)) if value == active));
    let history = required(journey.history(&scope), "history");
    assert_eq!(history.len(), 1);
    assert_eq!(
        history[0].canonical_receipt,
        binding_receipt(submission, address, 4).canonical_receipt
    );
    let audit = required(AuditChain::open(&scope), "audit");
    let entries = required(audit.entries(&scope), "audit entries");
    assert!(entries.iter().any(|entry| matches!(entry.event(), AuditEvent::IdentityLifecycle { event: IdentityEvent::WalletBinding, receipt_digest } if *receipt_digest == active.receipt_digest())));
    drop(scope);
    drop(store);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
#[allow(clippy::too_many_lines)]
fn rebind_requires_real_step_up_keeps_old_active_and_audits_notification() {
    let root = directory("binding-rebind");
    let map = tenancy(&[("alice", "tenant-a")]);
    let (mut store, _) = install_and_open(&root, &map, retention_uniform(10_000));
    let mut scope = required(store.principal(&principal("alice")), "principal");
    let journey = BindingJourney::new(registry());
    let did = Did::new(b"did:layerx:alice").unwrap_or_else(|error| panic!("DID: {error:?}"));
    let network = NetworkId::new(17).unwrap_or_else(|error| panic!("network: {error:?}"));
    let old_owner = SigningKey::from_bytes((&[0x21; 32]).into())
        .unwrap_or_else(|error| panic!("old owner: {error}"));
    let new_owner = SigningKey::from_bytes((&[0x22; 32]).into())
        .unwrap_or_else(|error| panic!("new owner: {error}"));
    let old_address = evm_address(&old_owner);
    let new_address = evm_address(&new_owner);
    let old_statement = required(
        BindingJourney::issue_statement(&did, network, old_address, 100, 60),
        "old statement",
    );
    let mut agent = InProcessAgentContract::new(&root.join("agentd"));
    let initial = required(
        journey.submit_initial(
            &mut scope,
            &old_statement,
            &ownership_signature(&old_owner, old_statement.signing_digest()),
            IdempotencyKey::new([0x51; 32]),
            &mut agent,
            101,
        ),
        "submit initial",
    );
    let old_receipt = binding_receipt(initial, old_address, 4);
    let old_active = required(
        journey.finalize(&mut scope, &old_receipt, 102, &TraceId::mint([3; 16])),
        "finalize initial",
    );
    agent.mark_executed(initial, old_receipt);

    let passkeys = required(Passkeys::new(auth_config()), "passkeys");
    let mut authenticator = SoftwareAuthenticator::new();
    let session = open_session(&passkeys, &mut scope, &mut authenticator, 110);
    let new_statement = required(
        BindingJourney::issue_statement(&did, network, new_address, 120, 60),
        "new statement",
    );
    let operation = BindingJourney::rebind_operation_digest(&old_active, &new_statement);
    let challenge = required(
        passkeys.begin_step_up(
            &mut scope,
            session.access_token().expose(),
            session.csrf_token().expose(),
            operation,
            121,
        ),
        "begin rebind step-up",
    );
    let assertion = authenticator.assert(&challenge.ceremony);
    let evidence = required(
        passkeys.finish_step_up(&mut scope, &challenge.challenge_id, &assertion, 122),
        "finish rebind step-up",
    );

    let submission = required(
        journey.submit_rebind(
            &mut scope,
            &passkeys,
            session.access_token().expose(),
            session.csrf_token().expose(),
            &evidence,
            &new_statement,
            &ownership_signature(&new_owner, new_statement.signing_digest()),
            IdempotencyKey::new([0x52; 32]),
            &mut agent,
            123,
        ),
        "submit rebind",
    );
    assert!(
        matches!(journey.state(&scope), Ok(BindingState::Rebinding { active, candidate }) if active == old_active && candidate == new_address.bytes())
    );

    let bad = journey.finalize(
        &mut scope,
        &binding_receipt(submission, old_address, 4),
        124,
        &TraceId::mint([4; 16]),
    );
    assert!(matches!(bad, Err(BindingError::ReceiptAddressMismatch)));
    assert!(
        matches!(journey.state(&scope), Ok(BindingState::Rebinding { active, .. }) if active == old_active)
    );

    let new_receipt = binding_receipt(submission, new_address, 4);
    let new_active = required(
        journey.finalize(&mut scope, &new_receipt, 125, &TraceId::mint([5; 16])),
        "finalize rebind",
    );
    assert_eq!(new_active.address(), new_address.bytes());
    agent.mark_executed(submission, new_receipt);
    assert_ne!(new_active.receipt_digest(), old_active.receipt_digest());
    assert_eq!(required(journey.history(&scope), "history").len(), 2);
    let notifications = required(journey.security_notifications(&scope), "notifications");
    assert_eq!(notifications.len(), 1);
    assert_eq!(notifications[0].deep_link, "/app/settings/wallet");
    assert_eq!(
        notifications[0].action_copy_key,
        "notification.action.review-wallet"
    );
    assert!(notifications[0]
        .message
        .contains(&new_statement.checksummed_address()));

    let audit = required(AuditChain::open(&scope), "audit");
    let entries = required(audit.entries(&scope), "audit entries");
    assert!(entries.iter().any(|entry| matches!(
        entry.event(),
        AuditEvent::SecurityChange {
            change: SecurityChangeKind::WalletRebinding,
            ..
        }
    )));
    assert!(entries.iter().any(|entry| matches!(
        entry.event(),
        AuditEvent::NotificationDispatch {
            class: NotificationClass::Security,
            ..
        }
    )));
    assert_eq!(
        entries
            .iter()
            .filter(|entry| matches!(
                entry.event(),
                AuditEvent::IdentityLifecycle {
                    event: IdentityEvent::WalletBinding,
                    ..
                }
            ))
            .count(),
        2
    );
    drop(scope);
    drop(store);
    let _ = std::fs::remove_dir_all(root);
}

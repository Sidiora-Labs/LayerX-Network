use layerx_human_test_support as support;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Display;
use std::fs;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use ciborium::value::Value as CborValue;
use ed25519_dalek::{Signer as _, SigningKey};
use layerx_agent_api::identity::{ActivityType, AgentDid, Asset, AuthorityRef, ExplicitSet};
use layerx_agent_api::prepare::{
    CanonicalBytes, Disclosure, IdempotencyRef, PreparationRef, Prepared, SigningPreimage,
};
use layerx_agent_api::track::{
    EvidenceRef as AgentEvidenceRef, SubmissionRef, SubmissionState, TrackedSubmission,
};
use layerx_agent_api::verify::Level;
use layerx_agent_api::{Amount, TimestampSeconds};
use layerx_agentd::approval::{
    ApprovalExpiry, ApprovalOutcome as AgentOutcome, ApprovalService, ApprovalSubmissionQueue,
    DecisionKey, DecisionRequest,
};
use layerx_agentd::budget::BudgetLimiter;
use layerx_agentd::capability::CapabilityId;
use layerx_agentd::policy::approval::{
    hold, ApprovalContext, ApprovalRegistry, ApprovalState, ApproverId,
};
use layerx_agentd::session::SessionId;
use layerx_agentd::store::TenantId;
use layerx_human_service::approvals::{
    AgentApprovalRecord, AgentApprovalState, AgentDecision, AgentDecisionBoundary,
    AgentDecisionResolution, AgentDecisionStatus, ApprovalBoundary, ApprovalBoundaryError,
    Decisions, VerifiedBudgetAfter,
};
use layerx_human_service::audit::{verify_export, ApprovalOutcome, AuditChain, AuditEvent};
use layerx_human_service::auth::{
    AccountIdentity, AuthConfig, Device, OperationDigest, Passkeys, RateLimit, SessionGrant,
};
use layerx_human_service::store::PrincipalScope;
use layerx_human_service::trace::TraceId;
use layerx_types::ids::Did;
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};

const RP_ID: &str = "id.layerx.example";
const ORIGIN: &str = "https://id.layerx.example";
const FLAG_UP: u8 = 1;
const FLAG_UV: u8 = 1 << 2;
const FLAG_AT: u8 = 1 << 6;

fn required<T, E: Display>(result: Result<T, E>, label: &str) -> T {
    result.unwrap_or_else(|error| panic!("{label}: {error}"))
}

fn auth_config() -> AuthConfig {
    AuthConfig {
        rp_id: RP_ID.to_owned(),
        rp_name: "LayerX".to_owned(),
        origin: ORIGIN.to_owned(),
        ceremony_ttl_secs: 300,
        assertion_ttl_secs: 30,
        session_ttl_secs: 60,
        refresh_ttl_secs: 300,
        step_up_ttl_secs: 10,
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
    signing_key: SigningKey,
    credential_id: Vec<u8>,
    counter: u32,
    user_handle: Option<String>,
}

impl SoftwareAuthenticator {
    fn new() -> Self {
        let mut seed = [0_u8; 32];
        required(getrandom::fill(&mut seed), "authenticator entropy");
        let mut credential_id = vec![0_u8; 32];
        required(
            getrandom::fill(&mut credential_id),
            "credential identifier entropy",
        );
        Self {
            signing_key: SigningKey::from_bytes(&seed),
            credential_id,
            counter: 0,
            user_handle: None,
        }
    }

    fn register(&mut self, ceremony: &str) -> String {
        let options = decode_ceremony(ceremony);
        let challenge = required_text(&options, "/challenge");
        self.user_handle = Some(required_text(&options, "/user/id").to_owned());
        let client_data = client_data("webauthn.create", challenge);
        encode_response(&json!({
            "id": URL_SAFE_NO_PAD.encode(&self.credential_id),
            "transports": ["internal"],
            "attestationObject": URL_SAFE_NO_PAD.encode(self.attestation_object()),
            "clientDataJSON": URL_SAFE_NO_PAD.encode(client_data),
        }))
    }

    fn assert(&mut self, ceremony: &str) -> String {
        let options = decode_ceremony(ceremony);
        let challenge = required_text(&options, "/challenge");
        self.counter = self.counter.saturating_add(1);
        let authenticator_data = self.authenticator_data(self.counter, false);
        let client_data = client_data("webauthn.get", challenge);
        let client_hash = Sha256::digest(&client_data);
        let mut signed = Vec::with_capacity(authenticator_data.len() + client_hash.len());
        signed.extend_from_slice(&authenticator_data);
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
            let credential_length = u16::try_from(self.credential_id.len())
                .unwrap_or_else(|_| panic!("credential identifier too long"));
            bytes.extend_from_slice(&credential_length.to_be_bytes());
            bytes.extend_from_slice(&self.credential_id);
            bytes.extend_from_slice(&self.cose_public_key());
        }
        bytes
    }

    fn cose_public_key(&self) -> Vec<u8> {
        let public_key = self.signing_key.verifying_key().to_bytes().to_vec();
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
                CborValue::Bytes(public_key),
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

fn register_session(
    passkeys: &Passkeys,
    scope: &mut PrincipalScope<'_>,
    authenticator: &mut SoftwareAuthenticator,
    now: u64,
) -> SessionGrant {
    let identity = required(
        AccountIdentity::new("mara@example.com", "Mara"),
        "account identity",
    );
    let registration = required(
        passkeys.begin_registration(scope, &identity, "Phone passkey", now),
        "begin registration",
    );
    let response = authenticator.register(&registration.ceremony);
    required(
        passkeys.finish_registration(scope, &registration.registration_id, &response, now + 1),
        "finish registration",
    );
    open_session(
        passkeys,
        scope,
        authenticator,
        "dev_aabbccddeeff00112233445566778891",
        now + 2,
    )
}

fn open_session(
    passkeys: &Passkeys,
    scope: &mut PrincipalScope<'_>,
    authenticator: &mut SoftwareAuthenticator,
    device_id: &str,
    now: u64,
) -> SessionGrant {
    let challenge = required(passkeys.begin_assertion(scope, now), "begin assertion");
    let response = authenticator.assert(&challenge.ceremony);
    required(
        passkeys.finish_assertion(scope, &challenge.assertion_id, &response, now + 1),
        "finish assertion",
    );
    let device = required(Device::new(device_id, "Phone", "mobile"), "device");
    required(
        passkeys.open_session(scope, &challenge.assertion_id, device, now + 2),
        "open session",
    )
}

fn tenant() -> TenantId {
    TenantId::new("tenant-decisions").unwrap_or_else(|error| panic!("tenant: {error}"))
}

fn prepared(id: u8) -> Prepared {
    let bytes = format!("held-approval-decision-{id}").into_bytes();
    let digest = Sha256::digest(&bytes).into();
    Prepared {
        preparation_ref: PreparationRef::new(format!("preparation-{id}"))
            .unwrap_or_else(|error| panic!("preparation: {error:?}")),
        unsigned_canonical_bytes: CanonicalBytes::new(bytes)
            .unwrap_or_else(|error| panic!("canonical bytes: {error:?}")),
        signing_preimage: SigningPreimage::new(vec![id; 32])
            .unwrap_or_else(|error| panic!("signing preimage: {error:?}")),
        disclosure: Disclosure {
            canonical_digest: digest,
            activity_type: ActivityType(5),
            actor: AgentDid::new("did:layerx:decision-agent")
                .unwrap_or_else(|error| panic!("actor: {error:?}")),
            authority: AuthorityRef::new("session-key")
                .unwrap_or_else(|error| panic!("authority: {error:?}")),
            counterparties: ExplicitSet::deny_all(),
            amounts: ExplicitSet::deny_all(),
            asset: Asset::new("LXP").unwrap_or_else(|error| panic!("asset: {error:?}")),
            fee_limit: Amount(2),
            expiry: TimestampSeconds(200),
            idempotency_key: IdempotencyRef::new(format!("movement-{id}"))
                .unwrap_or_else(|error| panic!("idempotency: {error:?}")),
        },
        expiry: TimestampSeconds(200),
    }
}

fn context(id: u8) -> ApprovalContext {
    ApprovalContext {
        tenant: tenant(),
        agent: Did::new(b"did:layerx:decision-agent")
            .unwrap_or_else(|error| panic!("agent: {error:?}")),
        session: SessionId([2; 32]),
        capability: CapabilityId([3; 32]),
        policy_version: "policy-decisions-v1".to_owned(),
        request_id: [id; 32],
    }
}

struct AgentdDecisions<'a> {
    service: ApprovalService<'a>,
    queue: &'a ApprovalSubmissionQueue,
    tenant: TenantId,
    prepared: BTreeMap<[u8; 32], Prepared>,
    released: BTreeMap<[u8; 32], [u8; 32]>,
    seen: BTreeSet<String>,
}

impl AgentdDecisions<'_> {
    fn normalize(
        &mut self,
        approval_id: [u8; 32],
        idempotency_key: &str,
        current_sequence: u64,
        outcome: &layerx_agentd::approval::ApprovalDecision,
    ) -> Result<AgentDecision, ApprovalBoundaryError> {
        let repeated = !self.seen.insert(idempotency_key.to_owned());
        let resolution = if repeated {
            AgentDecisionResolution::Repeated
        } else if matches!(
            outcome.outcome,
            AgentOutcome::Conflict | AgentOutcome::AlreadyDecided
        ) {
            AgentDecisionResolution::AlreadyDecided
        } else {
            AgentDecisionResolution::Applied
        };
        if let Some(reference) = outcome.submission_ref {
            self.released.insert(approval_id, reference);
        }
        let winner = outcome.winning_outcome.unwrap_or(outcome.outcome);
        let status = match winner {
            AgentOutcome::Granted => AgentDecisionStatus::Approved {
                submission_ref: outcome
                    .submission_ref
                    .or_else(|| self.released.get(&approval_id).copied()),
            },
            AgentOutcome::Rejected => AgentDecisionStatus::Rejected,
            AgentOutcome::Expired => AgentDecisionStatus::Expired,
            AgentOutcome::Defective => AgentDecisionStatus::Defective,
            AgentOutcome::AlreadyDecided | AgentOutcome::Conflict => {
                match self
                    .service
                    .get(&self.tenant, approval_id, current_sequence)
                    .map_err(|_| ApprovalBoundaryError::Unavailable)?
                    .state
                {
                    ApprovalState::Approved => AgentDecisionStatus::Approved {
                        submission_ref: self.released.get(&approval_id).copied(),
                    },
                    ApprovalState::Rejected => AgentDecisionStatus::Rejected,
                    ApprovalState::Expired => AgentDecisionStatus::Expired,
                    ApprovalState::Defective => AgentDecisionStatus::Defective,
                    ApprovalState::AwaitingApproval => return Err(ApprovalBoundaryError::Corrupt),
                }
            }
        };
        Ok(AgentDecision { status, resolution })
    }
}

impl ApprovalBoundary for AgentdDecisions<'_> {
    fn approval(
        &mut self,
        approval_id: [u8; 32],
        at_sequence: u64,
    ) -> Result<AgentApprovalRecord, ApprovalBoundaryError> {
        let record = self
            .service
            .get(&self.tenant, approval_id, at_sequence)
            .map_err(|_| ApprovalBoundaryError::Unavailable)?;
        let state = match record.state {
            ApprovalState::AwaitingApproval => AgentApprovalState::AwaitingApproval,
            ApprovalState::Approved => AgentApprovalState::Approved {
                submission_ref: *self
                    .released
                    .get(&approval_id)
                    .ok_or(ApprovalBoundaryError::Corrupt)?,
            },
            ApprovalState::Rejected => AgentApprovalState::Rejected,
            ApprovalState::Expired => AgentApprovalState::Expired,
            ApprovalState::Defective => AgentApprovalState::Defective,
        };
        Ok(AgentApprovalRecord {
            approval_id,
            held_activity: record.held_activity,
            canonical_bytes_digest: record.canonical_bytes_digest,
            hold_reason_code: record.hold_reason.code.to_owned(),
            hold_reason: record.hold_reason.message.to_owned(),
            created_at_sequence: record.created_at_sequence,
            expires_at_sequence: record.expires_at_sequence,
            state,
        })
    }

    fn verified_budget_after(
        &mut self,
        _hold: &AgentApprovalRecord,
        at_sequence: u64,
    ) -> Result<VerifiedBudgetAfter, ApprovalBoundaryError> {
        Ok(VerifiedBudgetAfter {
            remaining: 975,
            level: Level::StateProven,
            evidence_digest: [9; 32],
            observed_at_sequence: at_sequence,
        })
    }

    fn track_released(
        &mut self,
        submission_ref: [u8; 32],
    ) -> Result<TrackedSubmission, ApprovalBoundaryError> {
        if self.queue.prepared(submission_ref).is_none() {
            return Err(ApprovalBoundaryError::NotFound);
        }
        Ok(TrackedSubmission {
            submission_ref: SubmissionRef::new(hex(submission_ref))
                .map_err(|_| ApprovalBoundaryError::Corrupt)?,
            state: SubmissionState::Queued,
            evidence: vec![AgentEvidenceRef {
                kind: "approval-release".to_owned(),
                digest: submission_ref,
            }],
            verification_level: Level::SequencerSigned,
            transitions: Vec::new(),
        })
    }
}

impl AgentDecisionBoundary for AgentdDecisions<'_> {
    fn approve(
        &mut self,
        approval_id: [u8; 32],
        held_digest: [u8; 32],
        idempotency_key: &str,
        current_sequence: u64,
    ) -> Result<AgentDecision, ApprovalBoundaryError> {
        let prepared = self
            .prepared
            .get(&approval_id)
            .cloned()
            .ok_or(ApprovalBoundaryError::NotFound)?;
        if prepared.disclosure.canonical_digest != held_digest {
            return Err(ApprovalBoundaryError::VerificationFailed);
        }
        let decision = self
            .service
            .approve(
                DecisionRequest {
                    tenant: &self.tenant,
                    approval_id,
                    idempotency_key: &DecisionKey::new(idempotency_key)
                        .map_err(|_| ApprovalBoundaryError::Corrupt)?,
                    approver: ApproverId::new("human:mara")
                        .map_err(|_| ApprovalBoundaryError::Corrupt)?,
                    current_sequence,
                },
                &prepared,
                self.queue,
            )
            .map_err(|_| ApprovalBoundaryError::Unavailable)?;
        self.normalize(approval_id, idempotency_key, current_sequence, &decision)
    }

    fn reject(
        &mut self,
        approval_id: [u8; 32],
        held_digest: [u8; 32],
        idempotency_key: &str,
        current_sequence: u64,
    ) -> Result<AgentDecision, ApprovalBoundaryError> {
        let record = self
            .service
            .get(&self.tenant, approval_id, current_sequence)
            .map_err(|_| ApprovalBoundaryError::Unavailable)?;
        if record.canonical_bytes_digest != held_digest {
            return Err(ApprovalBoundaryError::VerificationFailed);
        }
        let decision = self
            .service
            .reject(DecisionRequest {
                tenant: &self.tenant,
                approval_id,
                idempotency_key: &DecisionKey::new(idempotency_key)
                    .map_err(|_| ApprovalBoundaryError::Corrupt)?,
                approver: ApproverId::new("human:mara")
                    .map_err(|_| ApprovalBoundaryError::Corrupt)?,
                current_sequence,
            })
            .map_err(|_| ApprovalBoundaryError::Unavailable)?;
        self.normalize(approval_id, idempotency_key, current_sequence, &decision)
    }
}

fn hex(bytes: impl AsRef<[u8]>) -> String {
    use std::fmt::Write as _;

    let bytes = bytes.as_ref();
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

#[test]
#[allow(clippy::too_many_lines)]
fn digest_bound_decisions_converge_and_export_the_agent_outcome() {
    let directory = support::directory("approval-decisions");
    let agent_root = directory.join("agent");
    fs::create_dir_all(&agent_root).unwrap_or_else(|error| panic!("agent root: {error}"));
    let registry = ApprovalRegistry::default();
    let limiter =
        BudgetLimiter::new(Vec::new()).unwrap_or_else(|error| panic!("limiter: {error:?}"));
    let expiry = ApprovalExpiry::open(&agent_root)
        .unwrap_or_else(|error| panic!("approval expiry: {error:?}"));
    let service = ApprovalService::new(&registry, &limiter, &expiry);
    let queue = ApprovalSubmissionQueue::default();
    let first = prepared(1);
    let second = prepared(2);
    hold(&registry, context(1), first.clone(), 10, 190)
        .unwrap_or_else(|error| panic!("first hold: {error:?}"));
    hold(&registry, context(2), second.clone(), 10, 190)
        .unwrap_or_else(|error| panic!("second hold: {error:?}"));
    let mut boundary = AgentdDecisions {
        service,
        queue: &queue,
        tenant: tenant(),
        prepared: BTreeMap::from([([1; 32], first.clone()), ([2; 32], second.clone())]),
        released: BTreeMap::new(),
        seen: BTreeSet::new(),
    };

    let map = support::tenancy(&[("mara", "tenant-decisions")]);
    let (mut store, _) = support::install_and_open(
        &directory.join("human"),
        &map,
        support::retention_uniform(10_000),
    );
    let mut scope = store
        .principal(&support::principal("mara"))
        .unwrap_or_else(|error| panic!("principal scope: {error}"));
    let passkeys = required(Passkeys::new(auth_config()), "passkeys");
    let mut authenticator = SoftwareAuthenticator::new();
    let first_session = register_session(&passkeys, &mut scope, &mut authenticator, 100);
    let second_session = open_session(
        &passkeys,
        &mut scope,
        &mut authenticator,
        "dev_aabbccddeeff00112233445566778892",
        106,
    );
    let mut audit = AuditChain::open(&scope).unwrap_or_else(|error| panic!("audit: {error}"));

    let wrong_challenge = required(
        passkeys.begin_step_up(
            &mut scope,
            first_session.access_token().expose(),
            first_session.csrf_token().expose(),
            OperationDigest::new(second.disclosure.canonical_digest),
            110,
        ),
        "begin wrong step-up",
    );
    let wrong_assertion = authenticator.assert(&wrong_challenge.ceremony);
    let wrong_evidence = required(
        passkeys.finish_step_up(
            &mut scope,
            &wrong_challenge.challenge_id,
            &wrong_assertion,
            111,
        ),
        "finish wrong step-up",
    );
    assert!(Decisions::approve(
        &mut scope,
        &passkeys,
        first_session.access_token().expose(),
        first_session.csrf_token().expose(),
        &wrong_evidence,
        &mut boundary,
        [1; 32],
        "approve-first-wrong",
        12,
        112,
        &mut audit,
        &TraceId::mint([1; 16]),
    )
    .is_err());
    assert!(queue.is_empty());

    let challenge = required(
        passkeys.begin_step_up(
            &mut scope,
            first_session.access_token().expose(),
            first_session.csrf_token().expose(),
            OperationDigest::new(first.disclosure.canonical_digest),
            113,
        ),
        "begin approval step-up",
    );
    let assertion = authenticator.assert(&challenge.ceremony);
    let evidence = required(
        passkeys.finish_step_up(&mut scope, &challenge.challenge_id, &assertion, 114),
        "finish approval step-up",
    );
    let approved = Decisions::approve(
        &mut scope,
        &passkeys,
        first_session.access_token().expose(),
        first_session.csrf_token().expose(),
        &evidence,
        &mut boundary,
        [1; 32],
        "approve-first",
        13,
        115,
        &mut audit,
        &TraceId::mint([2; 16]),
    )
    .unwrap_or_else(|error| panic!("approve: {error}"));
    assert_eq!(approved.resolution(), AgentDecisionResolution::Applied);
    assert!(matches!(
        approved.status(),
        AgentDecisionStatus::Approved {
            submission_ref: Some(_)
        }
    ));
    assert_eq!(
        approved.tracking().map(|tracking| &tracking.state),
        Some(&SubmissionState::Queued)
    );
    assert_eq!(queue.len(), 1);

    let second_device = Decisions::reject(
        &mut scope,
        &passkeys,
        second_session.access_token().expose(),
        second_session.csrf_token().expose(),
        &mut boundary,
        [1; 32],
        "reject-first-other-device",
        14,
        116,
        &mut audit,
        &TraceId::mint([3; 16]),
    )
    .unwrap_or_else(|error| panic!("second device: {error}"));
    assert!(second_device.already_decided());
    assert!(matches!(
        second_device.status(),
        AgentDecisionStatus::Approved { .. }
    ));
    assert_eq!(queue.len(), 1, "the held activity is released once");

    let rejected = Decisions::reject(
        &mut scope,
        &passkeys,
        second_session.access_token().expose(),
        second_session.csrf_token().expose(),
        &mut boundary,
        [2; 32],
        "reject-second",
        15,
        117,
        &mut audit,
        &TraceId::mint([4; 16]),
    )
    .unwrap_or_else(|error| panic!("reject: {error}"));
    assert_eq!(rejected.resolution(), AgentDecisionResolution::Applied);
    assert_eq!(rejected.status(), AgentDecisionStatus::Rejected);
    assert_eq!(rejected.nothing_moved(), Some("Nothing moved."));

    let entries = audit
        .entries(&scope)
        .unwrap_or_else(|error| panic!("audit entries: {error}"));
    assert_eq!(entries.len(), 3);
    assert!(matches!(
        entries[0].event(),
        AuditEvent::ApprovalDecision {
            hold_digest,
            outcome: ApprovalOutcome::Approved,
            ..
        } if *hold_digest == first.disclosure.canonical_digest
    ));
    assert!(matches!(
        entries[1].event(),
        AuditEvent::ApprovalDecision {
            outcome: ApprovalOutcome::Approved,
            ..
        }
    ));
    assert!(matches!(
        entries[2].event(),
        AuditEvent::ApprovalDecision {
            hold_digest,
            outcome: ApprovalOutcome::Rejected,
            ..
        } if *hold_digest == second.disclosure.canonical_digest
    ));
    let export = audit
        .export(&scope)
        .unwrap_or_else(|error| panic!("audit export: {error}"));
    let report = verify_export(&export).unwrap_or_else(|error| panic!("verify export: {error}"));
    assert_eq!(report.entries(), 3);
    assert_eq!(report.evidence_rows(), 3);
    let _ = fs::remove_dir_all(directory);
}

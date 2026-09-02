include!("reclaim.rs");

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use ciborium::value::Value as CborValue;
use layerx_agentd::identity::{
    register, CoreIdentity, IdentityError, IdentityResolver, ProtocolAuthority,
};
use layerx_agentd::session::{
    close, invalidate_on_revocation, open, InvalidationReason, OpenRequest, RevocationEvent,
    SessionId, SessionRegistry, Token,
};
use layerx_agentd::sign::ProvisionedSessionKey;
use layerx_crypto::session::IssuedSessionKey;
use layerx_human_service::agents::{
    AgentBalance, ArchiveAgentContract, ArchiveBoundary, ArchiveError, ArchiveJourney,
    ArchiveRequest, ArchiveStage, FundsDispositionEvidence, ReclaimStatus, SessionArchiveAdapter,
    ARCHIVE_ACTION_LABEL, ARCHIVE_CONFIRMATION_TONE, ARCHIVE_IRREVERSIBILITY_NOTICE,
};
use layerx_human_service::audit::{AuditChain, AuditEvent, SecurityChangeKind};
use layerx_human_service::auth::{
    AccountIdentity, AuthConfig, Device, Passkeys, RateLimit, SessionGrant,
};
use layerx_human_service::custody::{
    AgentContractError, AgentSessionContract, AgentSessionProvision, ProtocolIdentitySnapshot,
    ProvisionEvidence, RevocationEvidence, RevocationReason, RotationEvidence, RotationObservation,
    RotationSubmission, SessionEntropySource, SessionKeyEntropy, SessionKeyError,
    SessionKeyProvisioner, SessionLeaseState, SessionPolicy, SessionTarget, SuspensionEvidence,
};
use layerx_human_service::notify::AgentId;
use layerx_human_service::store::{EvidenceRef, PrincipalScope, RowKey, Table};
use layerx_types::verify::VerificationLevel;
use serde_json::{json, Value};

const ARCHIVE_ASSET: [u8; 32] = [0x33; 32];
const ARCHIVE_RP_ID: &str = "id.layerx.example";
const ARCHIVE_ORIGIN: &str = "https://id.layerx.example";
const FLAG_UP: u8 = 1;
const FLAG_UV: u8 = 1 << 2;
const FLAG_AT: u8 = 1 << 6;

struct IdentityBoundary(CoreIdentity);

impl IdentityResolver for IdentityBoundary {
    fn resolve(&mut self, _did: &Did) -> Result<Option<CoreIdentity>, IdentityError> {
        Ok(Some(self.0.clone()))
    }
}

struct InstalledAuthority {
    _issued: IssuedSessionKey,
    _signer: ProvisionedSessionKey,
    token: Token,
}

struct AuthorityLayer {
    store: AgentStore,
    sessions: SessionRegistry,
    tenant: TenantId,
    principal: PrincipalId,
    did: Did,
    primary_public_key: [u8; 32],
    protocol_identity: [u8; 32],
    revocation_sequence: u64,
    now: u64,
    core_sequence: u64,
    installed: BTreeMap<[u8; 32], InstalledAuthority>,
    funds: FundsDispositionEvidence,
    suspension_effects: usize,
    revocation_effects: usize,
}

impl AuthorityLayer {
    fn new(root: &std::path::Path, principal: PrincipalId, did: Did, key: [u8; 32]) -> Self {
        Self {
            store: AgentStore::open(root)
                .unwrap_or_else(|error| panic!("archive agent store: {error}")),
            sessions: SessionRegistry::default(),
            tenant: TenantId::new("tenant-archive")
                .unwrap_or_else(|error| panic!("archive tenant: {error}")),
            protocol_identity: Sha256::digest(did.as_bytes()).into(),
            principal,
            did,
            primary_public_key: key,
            revocation_sequence: 1,
            now: 1_000,
            core_sequence: 10,
            installed: BTreeMap::new(),
            funds: FundsDispositionEvidence {
                before: vec![AgentBalance {
                    asset: AssetId::new(ARCHIVE_ASSET),
                    amount: 1,
                }],
                after: vec![AgentBalance {
                    asset: AssetId::new(ARCHIVE_ASSET),
                    amount: 1,
                }],
                protocol_state_digest: [0x81; 32],
                observed_sequence: 10,
                observed_at: 1_000,
                verification_level: VerificationLevel::STATE_PROVEN,
            },
            suspension_effects: 0,
            revocation_effects: 0,
        }
    }

    fn require_scope(&self, principal: &PrincipalId, did: &Did) -> Result<(), AgentContractError> {
        if principal == &self.principal && did == &self.did {
            Ok(())
        } else {
            Err(AgentContractError::Refused("archive scope mismatch"))
        }
    }

    fn mark_reclaimed(&mut self, status: &ReclaimStatus) {
        let result = status
            .result()
            .unwrap_or_else(|| panic!("reclaim receipt missing"));
        assert_eq!(status.stage(), ReclaimStage::Done);
        assert_eq!(result.asset(), ARCHIVE_ASSET);
        assert_eq!(result.amount(), 1);
        self.funds.after[0].amount = 0;
        self.funds.protocol_state_digest = result.receipt_digest();
        self.funds.observed_sequence = self.funds.observed_sequence.saturating_add(1);
        self.funds.observed_at = self.funds.observed_at.saturating_add(1);
    }

    fn daemon_open(&self, grant: [u8; 32]) -> bool {
        self.sessions
            .get(&self.tenant, SessionId(grant))
            .is_some_and(|session| session.open)
    }

    fn authorizes(&self, grant: [u8; 32]) -> bool {
        self.installed.get(&grant).is_some_and(|installed| {
            installed
                .token
                .authorize(
                    &self.sessions,
                    &self.tenant,
                    &self.did,
                    "prepare",
                    self.core_sequence,
                )
                .is_ok()
        })
    }
}

impl AgentSessionContract for AuthorityLayer {
    fn identity_snapshot(
        &mut self,
        principal: &PrincipalId,
        did: &Did,
    ) -> Result<ProtocolIdentitySnapshot, AgentContractError> {
        self.require_scope(principal, did)?;
        Ok(ProtocolIdentitySnapshot {
            protocol_identity: self.protocol_identity,
            primary_public_key: self.primary_public_key,
            revocation_sequence: self.revocation_sequence,
            protocol_time: self.now,
            core_sequence: self.core_sequence,
            verification_level: VerificationLevel::STATE_PROVEN,
        })
    }

    fn provision_session(
        &mut self,
        request: AgentSessionProvision,
    ) -> Result<ProvisionEvidence, AgentContractError> {
        let (principal, did, issued, scopes, policy_version, secret) = request.into_parts();
        self.require_scope(&principal, &did)?;
        let grant_id = issued.grant_id;
        let session_public_key = issued.session_public_key;
        let signer = ProvisionedSessionKey::new(secret.into_seed(), issued.clone())
            .map_err(|_| AgentContractError::Refused("archive session key mismatch"))?;
        let authorities = self
            .installed
            .keys()
            .copied()
            .map(ProtocolAuthority::SessionKey)
            .chain(std::iter::once(ProtocolAuthority::SessionKey(grant_id)))
            .collect();
        let mut resolver = IdentityBoundary(CoreIdentity {
            canonical_bytes: self.protocol_identity.to_vec(),
            head_sequence: self.core_sequence,
            revocation_sequence: 1,
            verification_level: VerificationLevel::STATE_PROVEN,
            frozen: false,
            authorities,
        });
        let identity = register(
            &mut self.store,
            self.tenant.clone(),
            self.did.clone(),
            &mut resolver,
        )
        .map_err(|_| AgentContractError::Refused("archive identity registration failed"))?;
        let token = open(
            &mut self.store,
            &mut self.sessions,
            &identity,
            OpenRequest {
                session_id: SessionId(grant_id),
                token_id: Sha256::digest(grant_id).into(),
                tenant: self.tenant.clone(),
                agent: self.did.clone(),
                authority: ProtocolAuthority::SessionKey(grant_id),
                permitted_activity_types: issued
                    .permitted_activity_types
                    .iter()
                    .map(|activity| activity.ordinal())
                    .collect(),
                scopes: scopes.into_iter().collect(),
                expiry_sequence: issued.expires_at,
                opening_client: "layerx-human-service".to_owned(),
                policy_version,
            },
            self.core_sequence,
        )
        .map_err(|_| AgentContractError::Refused("archive daemon session failed"))?;
        self.installed.insert(
            grant_id,
            InstalledAuthority {
                _issued: issued,
                _signer: signer,
                token,
            },
        );
        let mut evidence = ProvisionEvidence {
            grant_id,
            session_public_key,
            daemon_session_id: grant_id,
            protocol_sequence: self.core_sequence,
            observed_at: self.now,
            verification_level: VerificationLevel::BATCH_INCLUDED,
            receipt_digest: [0; 32],
        };
        evidence.receipt_digest = evidence.expected_digest();
        Ok(evidence)
    }

    fn suspend_permissions(
        &mut self,
        target: &SessionTarget,
        reason: RevocationReason,
        requested_at: u64,
    ) -> Result<SuspensionEvidence, AgentContractError> {
        self.require_scope(&target.principal, &target.agent_did)?;
        close(
            &mut self.store,
            &mut self.sessions,
            &self.tenant,
            SessionId(target.daemon_session_id),
        )
        .map_err(|_| AgentContractError::Refused("archive daemon close failed"))?;
        self.suspension_effects = self.suspension_effects.saturating_add(1);
        self.now = self.now.max(requested_at);
        let mut evidence = SuspensionEvidence {
            grant_id: target.grant_id,
            daemon_session_id: target.daemon_session_id,
            reason,
            observed_at: self.now,
            receipt_digest: [0; 32],
        };
        evidence.receipt_digest = evidence.expected_digest();
        Ok(evidence)
    }

    fn revoke_protocol_session(
        &mut self,
        target: &SessionTarget,
        reason: RevocationReason,
        requested_at: u64,
    ) -> Result<RevocationEvidence, AgentContractError> {
        self.require_scope(&target.principal, &target.agent_did)?;
        self.core_sequence = self.core_sequence.saturating_add(1);
        self.revocation_sequence = self.revocation_sequence.saturating_add(1);
        invalidate_on_revocation(
            &mut self.store,
            &mut self.sessions,
            &mut [],
            &RevocationEvent {
                did: self.did.clone(),
                authority: Some(ProtocolAuthority::SessionKey(target.grant_id)),
                reason: InvalidationReason::SessionKeyRevoked,
                observed_sequence: self.core_sequence,
            },
        )
        .map_err(|_| AgentContractError::Refused("archive protocol invalidation failed"))?;
        self.installed.remove(&target.grant_id);
        self.revocation_effects = self.revocation_effects.saturating_add(1);
        self.now = self.now.max(requested_at).saturating_add(1);
        let mut evidence = RevocationEvidence {
            grant_id: target.grant_id,
            reason,
            observed_sequence: self.core_sequence,
            observed_at: self.now,
            verification_level: VerificationLevel::BATCH_INCLUDED,
            receipt_digest: [0; 32],
        };
        evidence.receipt_digest = evidence.expected_digest();
        Ok(evidence)
    }

    fn announce_rotation(
        &mut self,
        _submission: RotationSubmission,
    ) -> Result<RotationEvidence, AgentContractError> {
        Err(AgentContractError::Refused("archive is not rotation"))
    }

    fn rotation_observation(
        &mut self,
        _principal: &PrincipalId,
        _did: &Did,
    ) -> Result<RotationObservation, AgentContractError> {
        Err(AgentContractError::Refused("archive is not rotation"))
    }
}

impl ArchiveAgentContract for AuthorityLayer {
    fn funds_disposition(
        &mut self,
        principal: &PrincipalId,
        did: &Did,
        agent_id: &AgentId,
    ) -> Result<FundsDispositionEvidence, AgentContractError> {
        self.require_scope(principal, did)?;
        if agent_id.as_str() != "agt_worker" {
            return Err(AgentContractError::Refused("archive agent mismatch"));
        }
        Ok(self.funds.clone())
    }
}

struct ArchiveEntropy;

impl SessionEntropySource for ArchiveEntropy {
    fn next_session_entropy(&mut self) -> Result<SessionKeyEntropy, SessionKeyError> {
        SessionKeyEntropy::new([0x91; 32])
    }
}

struct AckGap<B> {
    inner: B,
    fired: bool,
}

impl<B: ArchiveBoundary> ArchiveBoundary for AckGap<B> {
    fn funds_disposition(
        &mut self,
        principal: &PrincipalId,
        did: &Did,
        agent_id: &AgentId,
    ) -> Result<FundsDispositionEvidence, ArchiveError> {
        self.inner.funds_disposition(principal, did, agent_id)
    }

    fn archive_authority(
        &mut self,
        principal: &PrincipalId,
        did: &Did,
        requested_at: u64,
    ) -> Result<layerx_human_service::custody::RevocationOutcome, ArchiveError> {
        let outcome = self.inner.archive_authority(principal, did, requested_at)?;
        if self.fired {
            Ok(outcome)
        } else {
            self.fired = true;
            Err(ArchiveError::Agent(AgentContractError::Unavailable))
        }
    }
}

fn archive_policy() -> SessionPolicy {
    SessionPolicy::new(
        1_000,
        100,
        3,
        vec!["prepare".to_owned(), "submit".to_owned()],
        "managed-agent-archive-v1",
    )
    .unwrap_or_else(|error| panic!("archive policy: {error}"))
}

fn auth_config() -> AuthConfig {
    AuthConfig {
        rp_id: ARCHIVE_RP_ID.to_owned(),
        rp_name: "LayerX".to_owned(),
        origin: ARCHIVE_ORIGIN.to_owned(),
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

fn required<T, E: std::fmt::Display>(result: Result<T, E>, label: &str) -> T {
    result.unwrap_or_else(|error| panic!("{label}: {error}"))
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

struct ArchiveAuthenticator {
    signing_key: SigningKey,
    credential_id: Vec<u8>,
    counter: u32,
    user_handle: Option<String>,
}

impl ArchiveAuthenticator {
    fn new() -> Self {
        Self {
            signing_key: SigningKey::from_bytes(&[0x92; 32]),
            credential_id: vec![0x93; 32],
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
        let mut signed = authenticator_data.clone();
        signed.extend_from_slice(&Sha256::digest(&client_data));
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
        bytes.extend_from_slice(&Sha256::digest(ARCHIVE_RP_ID.as_bytes()));
        bytes.push(FLAG_UP | FLAG_UV | if attested { FLAG_AT } else { 0 });
        bytes.extend_from_slice(&counter.to_be_bytes());
        if attested {
            bytes.extend_from_slice(&[0; 16]);
            bytes.extend_from_slice(
                &u16::try_from(self.credential_id.len())
                    .unwrap_or_else(|_| panic!("credential identifier too long"))
                    .to_be_bytes(),
            );
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
            "origin": ARCHIVE_ORIGIN,
            "crossOrigin": false,
        })),
        "encode client data",
    )
}

fn open_session(
    passkeys: &Passkeys,
    scope: &mut PrincipalScope<'_>,
    authenticator: &mut ArchiveAuthenticator,
    now: u64,
) -> SessionGrant {
    let identity = required(
        AccountIdentity::new("alice@example.com", "Alice"),
        "identity",
    );
    let registration = required(
        passkeys.begin_registration(scope, &identity, "Archive passkey", now),
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
#[allow(clippy::too_many_lines)]
fn archive_requires_real_reclaim_and_step_up_then_retires_authority_once() {
    assert_eq!(ARCHIVE_CONFIRMATION_TONE, "danger");
    assert_eq!(ARCHIVE_ACTION_LABEL, "Archive agent");
    assert!(ARCHIVE_IRREVERSIBILITY_NOTICE.contains("never"));
    let fixture = Fixture::new("agent-archive-real");
    let archive_did =
        Did::new(b"did:layerx:worker").unwrap_or_else(|error| panic!("archive DID: {error:?}"));
    let authority = AuthorityLayer::new(
        &fixture.root.join("archive-authority-agentd"),
        fixture.principal.clone(),
        archive_did.clone(),
        fixture.public_key,
    );
    let mut sessions =
        SessionKeyProvisioner::new(authority, ArchiveEntropy, archive_policy(), registry());
    let lease = sessions
        .provision(&fixture.principal, &archive_did, vec![activity_type()])
        .unwrap_or_else(|error| panic!("archive session: {error}"));
    assert!(sessions.contract().daemon_open(lease.grant_id));
    assert!(sessions.contract().authorizes(lease.grant_id));
    let adapter = SessionArchiveAdapter::new(sessions);
    let mut boundary = AckGap {
        inner: adapter,
        fired: false,
    };

    let history_key = RowKey::new("agent-worker-receipt-history")
        .unwrap_or_else(|error| panic!("history key: {error}"));
    let mut store = fixture.store();
    let mut scope = store
        .principal(&fixture.principal)
        .unwrap_or_else(|error| panic!("archive scope: {error}"));
    scope
        .put(
            Table::Cache,
            history_key.clone(),
            590,
            b"immutable worker receipt history".to_vec(),
        )
        .unwrap_or_else(|error| panic!("history row: {error}"));
    let request = ArchiveRequest {
        idempotency_key: [0x76; 32],
        agent_id: AgentId::new("agt_worker").unwrap_or_else(|error| panic!("agent ID: {error}")),
        agent_name: "worker".to_owned(),
        did: archive_did.clone(),
        history: vec![EvidenceRef::new(Table::Cache, history_key.clone())],
    };
    assert!(matches!(
        ArchiveJourney::start(&mut scope, &mut boundary, &request, &[], 600),
        Err(ArchiveError::FundsRemain(remaining)) if remaining[0].amount == 1
    ));
    drop(scope);

    let reclaim_request = reclaim_request(
        "jrn_archivereclaim",
        0x75,
        ReclaimMechanism::BudgetDefund {
            budget_account: account("agent:did:layerx:worker:budget:operations"),
            budget_id: BudgetId::new([0x45; 32]),
            revocation_sequence: Sequence::from_u64(4),
        },
    );
    let mut store = fixture.store();
    let mut scope = store
        .principal(&fixture.principal)
        .unwrap_or_else(|error| panic!("reclaim start scope: {error}"));
    Reclaim::start(&mut scope, &reclaim_request, &registry(), 601)
        .unwrap_or_else(|error| panic!("archive reclaim start: {error}"));
    drop(scope);
    let mut reclaim_agent = RealAgentLayer::new(
        &fixture.agent_root,
        BTreeMap::from([([0x75; 32], DeliveryMode::TrackThenReceipt)]),
    );
    let reclaim = drive_reclaim(
        &fixture,
        &mut reclaim_agent,
        &reclaim_request.journey_id,
        602,
    );
    let reclaim_status = reclaim
        .status()
        .unwrap_or_else(|error| panic!("archive reclaim status: {error}"));
    boundary
        .inner
        .sessions_mut()
        .contract_mut()
        .mark_reclaimed(&reclaim_status);

    let mut store = fixture.store();
    let mut scope = store
        .principal(&fixture.principal)
        .unwrap_or_else(|error| panic!("archive start scope: {error}"));
    let mut archive = ArchiveJourney::start(
        &mut scope,
        &mut boundary,
        &request,
        std::slice::from_ref(&reclaim_status),
        630,
    )
    .unwrap_or_else(|error| panic!("archive start: {error}"));
    assert_eq!(
        required(archive.status(), "archive status after name mismatch").stage(),
        ArchiveStage::AwaitingConfirmation
    );

    let passkeys = required(Passkeys::new(auth_config()), "passkeys");
    let mut authenticator = ArchiveAuthenticator::new();
    let session = open_session(&passkeys, &mut scope, &mut authenticator, 631);
    let challenge = required(
        passkeys.begin_step_up(
            &mut scope,
            session.access_token().expose(),
            session.csrf_token().expose(),
            archive.operation_digest(),
            636,
        ),
        "begin archive step-up",
    );
    let assertion = authenticator.assert(&challenge.ceremony);
    let evidence = required(
        passkeys.finish_step_up(&mut scope, &challenge.challenge_id, &assertion, 637),
        "finish archive step-up",
    );
    assert!(matches!(
        archive.confirm(
            &mut scope,
            &passkeys,
            session.access_token().expose(),
            session.csrf_token().expose(),
            &evidence,
            "Worker",
            &mut boundary,
            &fixture.trace,
            638,
        ),
        Err(ArchiveError::ConfirmationMismatch)
    ));
    let lost_ack = archive.confirm(
        &mut scope,
        &passkeys,
        session.access_token().expose(),
        session.csrf_token().expose(),
        &evidence,
        "worker",
        &mut boundary,
        &fixture.trace,
        638,
    );
    assert!(matches!(
        lost_ack,
        Err(ArchiveError::Agent(AgentContractError::Unavailable))
    ));
    drop(scope);

    let mut store = fixture.store();
    let mut scope = store
        .principal(&fixture.principal)
        .unwrap_or_else(|error| panic!("archive retry scope: {error}"));
    let mut archive = ArchiveJourney::load(&scope, [0x76; 32])
        .unwrap_or_else(|error| panic!("archive reload: {error}"))
        .unwrap_or_else(|| panic!("archive missing"));
    let status = archive
        .confirm(
            &mut scope,
            &passkeys,
            session.access_token().expose(),
            session.csrf_token().expose(),
            &evidence,
            "worker",
            &mut boundary,
            &fixture.trace,
            639,
        )
        .unwrap_or_else(|error| panic!("archive retry: {error}"));
    assert_eq!(status.stage(), ArchiveStage::Archived);
    assert!(status.irreversible());
    assert_eq!(status.history_entries(), 1);
    assert!(status.suspension_receipt_digest().is_some());
    assert!(status.revocation_receipt_digest().is_some());
    let sessions = boundary.inner.sessions_mut();
    assert_eq!(sessions.contract().suspension_effects, 1);
    assert_eq!(sessions.contract().revocation_effects, 1);
    assert!(!sessions.contract().daemon_open(lease.grant_id));
    assert!(!sessions.contract().authorizes(lease.grant_id));
    assert!(matches!(
        sessions.resume(&fixture.principal, &archive_did),
        Err(SessionKeyError::Archived)
    ));
    let suspension_receipt_digest = status
        .suspension_receipt_digest()
        .unwrap_or_else(|| panic!("archive suspension receipt missing"));
    let revocation_receipt_digest = status
        .revocation_receipt_digest()
        .unwrap_or_else(|| panic!("archive revocation receipt missing"));
    assert!(matches!(
        sessions
            .session(&fixture.principal, &archive_did)
            .map(|lease| &lease.state),
        Some(SessionLeaseState::Revoked {
            reason: RevocationReason::Archived,
            suspension_receipt_digest: stored_suspension_digest,
            revocation_receipt_digest: stored_revocation_digest,
            ..
        }) if *stored_suspension_digest == suspension_receipt_digest
            && *stored_revocation_digest == revocation_receipt_digest
    ));

    let retained = archive
        .history(&scope)
        .unwrap_or_else(|error| panic!("archived history: {error}"));
    assert_eq!(retained[0].bytes(), b"immutable worker receipt history");
    let audit = AuditChain::open(&scope).unwrap_or_else(|error| panic!("archive audit: {error}"));
    let entries = audit
        .entries(&scope)
        .unwrap_or_else(|error| panic!("archive audit entries: {error}"));
    assert!(entries.iter().any(|entry| matches!(
        entry.event(),
        AuditEvent::SecurityChange {
            change: SecurityChangeKind::AgentArchive,
            ..
        }
    )));
    assert!(scope
        .keys(Table::Notifications)
        .iter()
        .any(|key| key.as_str().starts_with("agent-archive-notification-")));
    let report = scope
        .expire(10_000_000)
        .unwrap_or_else(|error| panic!("archive retention: {error}"));
    assert!(report.pinned_evidence_retained > 0);
    assert_eq!(
        archive
            .history(&scope)
            .unwrap_or_else(|error| panic!("retained archive history: {error}"))[0]
            .bytes(),
        b"immutable worker receipt history"
    );

    let repeated = archive
        .confirm(
            &mut scope,
            &passkeys,
            session.access_token().expose(),
            session.csrf_token().expose(),
            &evidence,
            "worker",
            &mut boundary,
            &fixture.trace,
            1_002,
        )
        .unwrap_or_else(|error| panic!("archive terminal retry: {error}"));
    assert_eq!(repeated, status);
    assert_eq!(boundary.inner.sessions().contract().suspension_effects, 1);
    assert_eq!(boundary.inner.sessions().contract().revocation_effects, 1);
}

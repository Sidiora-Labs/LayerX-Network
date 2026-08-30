#[allow(dead_code)]
mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

use layerx_agentd::identity::{
    register, CoreIdentity, IdentityError, IdentityResolver, ProtocolAuthority,
};
use layerx_agentd::prepare::{
    prepare_activity, CorePreparationBoundary, CorePreparationState, CoreStateError,
    PreparationDefaults, PrepareRequest, Prepared,
};
use layerx_agentd::session::{
    close, invalidate_on_revocation, open, InvalidationReason, OpenRequest, RevocationEvent,
    SessionId, SessionRegistry, Token,
};
use layerx_agentd::sign::{self_sign, ProvisionedSessionKey, SigningError, SigningMode};
use layerx_agentd::store::{Store, TenantId};
use layerx_crypto::local::LocalSigner;
use layerx_crypto::session::IssuedSessionKey;
use layerx_crypto::signer::Signer as _;
use layerx_human_service::custody::{
    AgentContractError, AgentSessionContract, AgentSessionProvision, EnvelopeKms, KeyClass,
    KeyEntropy, KeyId, Keystore, ProtocolIdentitySnapshot, ProvisionEvidence, RevocationEvidence,
    RevocationReason, RotationEvidence, RotationJourneyState, RotationObservation, RotationSubject,
    RotationSubmission, SessionEntropySource, SessionKeyEntropy, SessionKeyError,
    SessionKeyProvisioner, SessionPolicy, SessionTarget, SuspensionEvidence,
};
use layerx_human_service::store::PrincipalId;
use layerx_intents::{compile, DisclosureCheck, Intent, IntentKind, LxpSend};
use layerx_types::account::AccountId;
use layerx_types::activity::{Authority, TimestampBound};
use layerx_types::amount::Amount;
use layerx_types::ids::{AssetId, Did, IdempotencyKey};
use layerx_types::intent::{
    AuthorizationSignature, ContextHash, NetworkId, ProtocolVersion, PublicKey, SendAuthorization,
    SendAuthorizationKind, Sequence, TimestampSeconds,
};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_types::verify::VerificationLevel;
use sha2::{Digest as _, Sha256};

use support::directory;

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
        Poll::Pending => panic!("local agent signer unexpectedly blocked"),
    }
}

fn principal() -> PrincipalId {
    PrincipalId::new("alice").unwrap_or_else(|error| panic!("principal: {error}"))
}

fn did(label: &str) -> Did {
    Did::new(format!("did:layerx:{label}").as_bytes())
        .unwrap_or_else(|error| panic!("DID: {error:?}"))
}

fn send_type() -> ActivityType {
    ActivityType::new(ModuleId::Asset, 5).unwrap_or_else(|error| panic!("send activity: {error:?}"))
}

fn rotation_type() -> ActivityType {
    ActivityType::new(ModuleId::Governance, 2)
        .unwrap_or_else(|error| panic!("rotation activity: {error:?}"))
}

fn registry() -> ModuleRegistry {
    let asset = ModuleRegistration::new(ModuleId::Asset, &[send_type()])
        .unwrap_or_else(|error| panic!("asset registry: {error:?}"));
    let governance = ModuleRegistration::new(ModuleId::Governance, &[rotation_type()])
        .unwrap_or_else(|error| panic!("governance registry: {error:?}"));
    ModuleRegistry::new(&[asset, governance])
        .unwrap_or_else(|error| panic!("module registry: {error:?}"))
}

fn policy() -> SessionPolicy {
    SessionPolicy::new(
        100,
        20,
        3,
        vec!["prepare".to_owned(), "submit".to_owned()],
        "managed-agent-v1",
    )
    .unwrap_or_else(|error| panic!("session policy: {error}"))
}

struct OperatingEntropy;

impl SessionEntropySource for OperatingEntropy {
    fn next_session_entropy(&mut self) -> Result<SessionKeyEntropy, SessionKeyError> {
        let mut seed = [0_u8; 32];
        getrandom::fill(&mut seed).map_err(|_| SessionKeyError::EntropyUnavailable)?;
        SessionKeyEntropy::new(seed)
    }
}

struct IdentityBoundary(CoreIdentity);

impl IdentityResolver for IdentityBoundary {
    fn resolve(&mut self, _did: &Did) -> Result<Option<CoreIdentity>, IdentityError> {
        Ok(Some(self.0.clone()))
    }
}

struct PreparationBoundary(CorePreparationState);

impl CorePreparationBoundary for PreparationBoundary {
    fn preparation_state(&mut self, _actor: &Did) -> Result<CorePreparationState, CoreStateError> {
        Ok(self.0.clone())
    }
}

struct InstalledSession {
    issued: IssuedSessionKey,
    signer: ProvisionedSessionKey,
    token: Token,
    current_revocation_sequence: u64,
}

struct CoreRotation {
    current: [u8; 32],
    pending: [u8; 32],
    effective_at: u64,
    lapse_at: u64,
    effective_sequence: u64,
    committed: bool,
}

/// Concrete in-process adapter over the actual agentd identity, session,
/// preparation, self-signing, and invalidation code paths.
struct AgentLayer {
    root: std::path::PathBuf,
    store: Store,
    registry: SessionRegistry,
    tenant: TenantId,
    principal: PrincipalId,
    did: Did,
    protocol_identity: [u8; 32],
    primary_public_key: [u8; 32],
    revocation_sequence: u64,
    now: u64,
    core_sequence: u64,
    installed: BTreeMap<[u8; 32], InstalledSession>,
    rotation: Option<CoreRotation>,
    events: Vec<(&'static str, [u8; 32])>,
    last_rotation_payload_hash: Option<[u8; 32]>,
}

impl AgentLayer {
    fn new(
        root: std::path::PathBuf,
        principal: PrincipalId,
        did: Did,
        primary_public_key: [u8; 32],
    ) -> Self {
        fs::create_dir_all(&root).unwrap_or_else(|error| panic!("agent root: {error}"));
        let store = Store::open(&root).unwrap_or_else(|error| panic!("agent store: {error}"));
        let protocol_identity = Sha256::digest(did.as_bytes()).into();
        Self {
            root,
            store,
            registry: SessionRegistry::default(),
            tenant: TenantId::new("tenant-a").unwrap_or_else(|error| panic!("tenant: {error}")),
            principal,
            did,
            protocol_identity,
            primary_public_key,
            revocation_sequence: 1,
            now: 1_000,
            core_sequence: 10,
            installed: BTreeMap::new(),
            rotation: None,
            events: Vec::new(),
            last_rotation_payload_hash: None,
        }
    }

    fn advance(&mut self, now: u64, core_sequence: u64) {
        assert!(now >= self.now, "protocol time must not move backwards");
        assert!(
            core_sequence >= self.core_sequence,
            "core sequence must not move backwards"
        );
        self.now = now;
        self.core_sequence = core_sequence;
    }

    fn daemon_open(&self, grant_id: [u8; 32]) -> bool {
        self.registry
            .get(SessionId(grant_id))
            .is_some_and(|record| record.open)
    }

    fn events(&self) -> &[(&'static str, [u8; 32])] {
        &self.events
    }

    fn token_authorizes(&self, grant_id: [u8; 32], scope: &str) -> bool {
        self.installed.get(&grant_id).is_some_and(|installed| {
            installed
                .token
                .authorize(&self.tenant, &self.did, scope, self.core_sequence)
                .is_ok()
        })
    }

    fn direct_sign(&self, grant_id: [u8; 32]) -> Result<SigningMode, SigningError> {
        let installed = self
            .installed
            .get(&grant_id)
            .unwrap_or_else(|| panic!("installed signer missing"));
        let prepared = prepared_send(
            &self.did,
            installed.issued.authority.clone(),
            installed.issued.session_public_key,
            self.now,
        );
        ready(self_sign(
            Some(&installed.signer),
            &prepared,
            &registry(),
            self.now,
            installed.current_revocation_sequence,
        ))
        .map(|signed| signed.mode)
    }

    fn commit_rotation(&mut self) -> Result<(), AgentContractError> {
        let rotation = self
            .rotation
            .as_mut()
            .ok_or(AgentContractError::Refused("rotation missing"))?;
        if self.now < rotation.effective_at {
            return Err(AgentContractError::Refused("challenge delay open"));
        }
        if self.now > rotation.lapse_at {
            return Err(AgentContractError::Refused("rotation lapsed"));
        }
        if rotation.committed {
            return Err(AgentContractError::Refused("rotation already committed"));
        }
        rotation.committed = true;
        self.primary_public_key = rotation.pending;
        self.revocation_sequence = self.revocation_sequence.saturating_add(1);
        let event = RevocationEvent {
            did: self.did.clone(),
            authority: None,
            reason: InvalidationReason::PrimaryKeyRotated,
            observed_sequence: self.core_sequence,
        };
        invalidate_on_revocation(&mut self.store, &mut self.registry, &mut [], &event)
            .map_err(|_| AgentContractError::Refused("agent invalidation failed"))?;
        for installed in self.installed.values_mut() {
            installed.current_revocation_sequence =
                installed.current_revocation_sequence.saturating_add(1);
        }
        Ok(())
    }

    fn primary_key_valid(&self, candidate: [u8; 32], timestamp: u64, global_sequence: u64) -> bool {
        let Some(rotation) = &self.rotation else {
            return candidate == self.primary_public_key;
        };
        if !rotation.committed {
            return candidate == rotation.current
                || (timestamp <= rotation.lapse_at && candidate == rotation.pending);
        }
        candidate == rotation.pending
            || (global_sequence < rotation.effective_sequence && candidate == rotation.current)
    }
}

impl Drop for AgentLayer {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

impl AgentSessionContract for AgentLayer {
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
        if scopes.is_empty() || policy_version.is_empty() {
            return Err(AgentContractError::Refused("daemon bounds missing"));
        }
        let grant_id = issued.grant_id;
        let session_public_key = issued.session_public_key;
        let seed = secret.into_seed();
        let signer = ProvisionedSessionKey::new(seed, issued.clone())
            .map_err(|_| AgentContractError::Refused("session secret does not match grant"))?;
        let authorities = self
            .installed
            .keys()
            .copied()
            .map(ProtocolAuthority::SessionKey)
            .chain(std::iter::once(ProtocolAuthority::SessionKey(grant_id)))
            .collect();
        let mut boundary = IdentityBoundary(CoreIdentity {
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
            &mut boundary,
        )
        .map_err(|_| AgentContractError::Refused("identity registration failed"))?;
        let token = open(
            &mut self.store,
            &mut self.registry,
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
                    .collect::<BTreeSet<_>>(),
                scopes: scopes.into_iter().collect(),
                expiry_sequence: issued.expires_at,
                opening_client: "layerx-human-service".to_owned(),
                policy_version,
            },
            self.core_sequence,
        )
        .map_err(|_| AgentContractError::Refused("daemon session open failed"))?;
        self.installed.insert(
            grant_id,
            InstalledSession {
                current_revocation_sequence: issued.revocation_sequence,
                issued,
                signer,
                token,
            },
        );
        self.events.push(("provision", grant_id));
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
        if target.grant_id != target.daemon_session_id {
            return Err(AgentContractError::Refused("session target mismatch"));
        }
        close(
            &mut self.store,
            &mut self.registry,
            SessionId(target.daemon_session_id),
        )
        .map_err(|_| AgentContractError::Refused("daemon session close failed"))?;
        self.now = self.now.max(requested_at);
        self.events.push(("suspend", target.grant_id));
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
        let installed = self
            .installed
            .get_mut(&target.grant_id)
            .ok_or(AgentContractError::Refused("session signer missing"))?;
        installed.current_revocation_sequence =
            installed.current_revocation_sequence.saturating_add(1);
        let event = RevocationEvent {
            did: self.did.clone(),
            authority: Some(ProtocolAuthority::SessionKey(target.grant_id)),
            reason: InvalidationReason::SessionKeyRevoked,
            observed_sequence: self.core_sequence.saturating_add(1),
        };
        invalidate_on_revocation(&mut self.store, &mut self.registry, &mut [], &event)
            .map_err(|_| AgentContractError::Refused("protocol invalidation failed"))?;
        self.core_sequence = self.core_sequence.saturating_add(1);
        self.now = self.now.max(requested_at).saturating_add(1);
        self.events.push(("revoke", target.grant_id));
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
        submission: RotationSubmission,
    ) -> Result<RotationEvidence, AgentContractError> {
        self.require_scope(&submission.principal, &submission.did)?;
        if self.rotation.is_some() {
            return Err(AgentContractError::Refused("competing rotation"));
        }
        if DisclosureCheck::verify(&submission.intent, &submission.compiled)
            != Ok(submission.disclosure.clone())
            || submission.compiled.activity_type() != rotation_type()
            || submission.current_public_key != self.primary_public_key
        {
            return Err(AgentContractError::Refused("rotation intent mismatch"));
        }
        self.last_rotation_payload_hash = Some(submission.compiled.payload_hash());
        self.rotation = Some(CoreRotation {
            current: submission.current_public_key,
            pending: submission.pending_public_key,
            effective_at: submission.effective_at,
            lapse_at: submission.lapse_at,
            effective_sequence: submission.effective_sequence,
            committed: false,
        });
        self.core_sequence = self.core_sequence.saturating_add(1);
        let mut evidence = RotationEvidence {
            payload_hash: submission.compiled.payload_hash(),
            pending_public_key: submission.pending_public_key,
            effective_at: submission.effective_at,
            lapse_at: submission.lapse_at,
            effective_sequence: submission.effective_sequence,
            observed_sequence: self.core_sequence,
            observed_at: self.now,
            verification_level: VerificationLevel::BATCH_INCLUDED,
            receipt_digest: [0; 32],
        };
        evidence.receipt_digest = evidence.expected_digest();
        Ok(evidence)
    }

    fn rotation_observation(
        &mut self,
        principal: &PrincipalId,
        did: &Did,
    ) -> Result<RotationObservation, AgentContractError> {
        self.require_scope(principal, did)?;
        let rotation = self
            .rotation
            .as_ref()
            .ok_or(AgentContractError::Refused("rotation missing"))?;
        Ok(RotationObservation {
            primary_public_key: if rotation.committed {
                rotation.pending
            } else {
                rotation.current
            },
            pending_public_key: (!rotation.committed).then_some(rotation.pending),
            superseded_public_key: rotation.committed.then_some(rotation.current),
            effective_at: rotation.effective_at,
            lapse_at: rotation.lapse_at,
            effective_sequence: rotation.effective_sequence,
            observed_at: self.now,
            observed_sequence: self.core_sequence,
            verification_level: VerificationLevel::STATE_PROVEN,
        })
    }
}

impl AgentLayer {
    fn require_scope(&self, principal: &PrincipalId, did: &Did) -> Result<(), AgentContractError> {
        if principal == &self.principal && did == &self.did {
            Ok(())
        } else {
            Err(AgentContractError::Refused("wrong principal"))
        }
    }
}

fn account(value: &str) -> AccountId {
    AccountId::parse(value).unwrap_or_else(|error| panic!("account: {error:?}"))
}

fn send_intent(session_public_key: [u8; 32], now: u64) -> Intent {
    let send = LxpSend::new(
        account("agent:did:layerx:managed:main"),
        account("agent:did:layerx:recipient:main"),
        AssetId::new([0x33; 32]),
        Amount::from_u128(25),
        Sequence::from_u64(7),
        IdempotencyKey::new([0x44; 32]),
        TimestampSeconds::from_u64(now.saturating_add(20)),
        ContextHash::new([0x55; 32]),
        SendAuthorization::new(
            SendAuthorizationKind::SessionKey,
            PublicKey::new(session_public_key),
            AuthorizationSignature::new([0x66; 64]),
        ),
        NetworkId::new(NETWORK_ID).unwrap_or_else(|error| panic!("network: {error:?}")),
        ProtocolVersion::new(1).unwrap_or_else(|error| panic!("protocol: {error:?}")),
    )
    .unwrap_or_else(|error| panic!("send intent: {error:?}"));
    Intent::v1(IntentKind::LxpSend(send))
}

fn prepared_send(
    did: &Did,
    authority: Authority,
    session_public_key: [u8; 32],
    now: u64,
) -> Prepared {
    let compiled = compile(&send_intent(session_public_key, now), &registry())
        .unwrap_or_else(|error| panic!("send compile: {error:?}"));
    let mut boundary = PreparationBoundary(CorePreparationState {
        network_id: NETWORK_ID,
        account_sequence: 7,
        protocol_timestamp: now,
        observed_head_sequence: 90,
        module_registry: registry(),
    });
    prepare_activity(
        &mut boundary,
        PreparationDefaults {
            timestamp_span: 30,
            fee_limit: Amount::from_u128(10),
            maximum_payload_bytes: 1_024,
        },
        PrepareRequest {
            actor: did.clone(),
            authority,
            activity_type: compiled.activity_type(),
            expected_account_sequence: Some(7),
            timestamp_bound: Some(
                TimestampBound::new(now, now.saturating_add(20))
                    .unwrap_or_else(|error| panic!("timestamp: {error:?}")),
            ),
            fee_limit: Some(Amount::from_u128(7)),
            idempotency_key: IdempotencyKey::new([0x44; 32]),
            payload: compiled.payload().as_bytes().to_vec(),
            declared_payload_limit: 1_024,
        },
    )
    .unwrap_or_else(|error| panic!("prepare: {error:?}"))
}

fn keystore(
    root: &std::path::Path,
    principal: &PrincipalId,
    class: KeyClass,
    current_seed: [u8; 32],
    pending_seed: [u8; 32],
) -> (Keystore, KeyId, KeyId, [u8; 32], [u8; 32]) {
    fs::create_dir_all(root).unwrap_or_else(|error| panic!("custody root: {error}"));
    let secret_path = root.join("kms-root");
    fs::write(&secret_path, [0xa5; 64]).unwrap_or_else(|error| panic!("KMS root: {error}"));
    let provider = EnvelopeKms::new("file-kms://rotation", secret_path)
        .unwrap_or_else(|error| panic!("KMS provider: {error}"));
    let keystore = Keystore::open_development(root.join("sealed"), NETWORK_ID, provider)
        .unwrap_or_else(|error| panic!("keystore: {error}"));
    let current =
        KeyId::new("current-primary").unwrap_or_else(|error| panic!("current key id: {error}"));
    let pending =
        KeyId::new("pending-primary").unwrap_or_else(|error| panic!("pending key id: {error}"));
    let current_public = keystore
        .generate(
            principal,
            &current,
            class,
            KeyEntropy::new(current_seed, [1; 16], [2; 24])
                .unwrap_or_else(|error| panic!("current entropy: {error}")),
        )
        .unwrap_or_else(|error| panic!("current key: {error}"));
    let pending_public = keystore
        .generate(
            principal,
            &pending,
            class,
            KeyEntropy::new(pending_seed, [3; 16], [4; 24])
                .unwrap_or_else(|error| panic!("pending entropy: {error}")),
        )
        .unwrap_or_else(|error| panic!("pending key: {error}"));
    (keystore, current, pending, current_public, pending_public)
}

#[test]
fn real_agent_session_is_scope_bounded_and_pause_archive_revoke_both_layers() {
    let root = directory("session-revocation");
    let principal = principal();
    let did = did("managed");
    let primary_public = LocalSigner::new([0x11; 32]).public_key();
    let layer = AgentLayer::new(
        root.join("agent"),
        principal.clone(),
        did.clone(),
        primary_public,
    );
    let mut provisioner = SessionKeyProvisioner::new(layer, OperatingEntropy, policy(), registry());

    let first = provisioner
        .provision(&principal, &did, vec![send_type()])
        .unwrap_or_else(|error| panic!("provision: {error}"));
    assert_eq!(first.permitted_activity_types, vec![send_type()]);
    assert_eq!(first.expires_at, 1_100);
    assert_ne!(first.session_public_key, primary_public);
    assert!(provisioner.contract().daemon_open(first.grant_id));
    assert!(provisioner
        .contract()
        .token_authorizes(first.grant_id, "prepare"));
    assert_eq!(
        provisioner.contract().direct_sign(first.grant_id),
        Ok(SigningMode::ProtocolSessionKey)
    );

    provisioner.contract_mut().advance(1_005, 11);
    let paused = provisioner
        .pause(&principal, &did, 1_005)
        .unwrap_or_else(|error| panic!("pause: {error}"));
    assert_eq!(paused.latency_seconds, 1);
    assert!(paused.within_declared_target);
    assert!(!provisioner.contract().daemon_open(first.grant_id));
    assert_eq!(
        provisioner.contract().direct_sign(first.grant_id),
        Err(SigningError::Revoked)
    );

    let resumed = provisioner
        .resume(&principal, &did)
        .unwrap_or_else(|error| panic!("resume: {error}"));
    assert_ne!(resumed.grant_id, first.grant_id);
    assert_eq!(
        provisioner.contract().direct_sign(resumed.grant_id),
        Ok(SigningMode::ProtocolSessionKey)
    );
    provisioner.contract_mut().advance(1_010, 13);
    let archived = provisioner
        .archive(&principal, &did, 1_010)
        .unwrap_or_else(|error| panic!("archive: {error}"));
    assert!(archived.within_declared_target);
    assert!(matches!(
        provisioner.resume(&principal, &did),
        Err(SessionKeyError::Archived)
    ));
    assert!(provisioner.renew_expiring(2_000).is_empty());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn expiring_authority_renews_before_old_grant_is_retired() {
    let root = directory("session-renewal");
    let principal = principal();
    let did = did("renewal");
    let primary_public = LocalSigner::new([0x12; 32]).public_key();
    let layer = AgentLayer::new(
        root.join("agent"),
        principal.clone(),
        did.clone(),
        primary_public,
    );
    let mut provisioner = SessionKeyProvisioner::new(layer, OperatingEntropy, policy(), registry());
    let first = provisioner
        .provision(&principal, &did, vec![send_type()])
        .unwrap_or_else(|error| panic!("provision: {error}"));
    assert!(provisioner.renew_expiring(1_079).is_empty());

    provisioner.contract_mut().advance(1_081, 20);
    let results = provisioner.renew_expiring(1_081);
    assert_eq!(results.len(), 1);
    let renewal = results
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("renewal result missing"))
        .unwrap_or_else(|error| panic!("renewal: {error}"));
    assert_eq!(renewal.previous_grant_id, first.grant_id);
    assert_ne!(renewal.replacement.grant_id, first.grant_id);
    assert_eq!(
        renewal.replacement.permitted_activity_types,
        first.permitted_activity_types
    );
    let events = provisioner.contract().events();
    assert_eq!(events[events.len() - 3].0, "provision");
    assert_eq!(events[events.len() - 2], ("suspend", first.grant_id));
    assert_eq!(events[events.len() - 1], ("revoke", first.grant_id));
    assert_eq!(
        provisioner.contract().direct_sign(first.grant_id),
        Err(SigningError::Revoked)
    );
    assert_eq!(
        provisioner
            .contract()
            .direct_sign(renewal.replacement.grant_id),
        Ok(SigningMode::ProtocolSessionKey)
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn human_and_agent_rotation_keep_the_protocol_window_and_agent_identity_continuous() {
    let root = directory("protocol-rotation");
    let principal = principal();
    let agent_did = did("rotating-agent");
    let (agent_keys, current_id, pending_id, current_public, pending_public) = keystore(
        &root.join("agent-custody"),
        &principal,
        KeyClass::AgentPrimary,
        [0x21; 32],
        [0x22; 32],
    );
    let layer = AgentLayer::new(
        root.join("agent-layer"),
        principal.clone(),
        agent_did.clone(),
        current_public,
    );
    let mut provisioner = SessionKeyProvisioner::new(layer, OperatingEntropy, policy(), registry());
    let old_session = provisioner
        .provision(&principal, &agent_did, vec![send_type()])
        .unwrap_or_else(|error| panic!("initial session: {error}"));
    let announced = provisioner
        .announce_rotation(
            &agent_keys,
            &principal,
            RotationSubject::Agent,
            &agent_did,
            &current_id,
            &pending_id,
            60,
            50,
        )
        .unwrap_or_else(|error| panic!("agent rotation: {error}"));
    assert_eq!(announced.challenge_delay.to_string(), "1 minute");
    assert_eq!(announced.effective_at, 1_060);
    assert_eq!(announced.lapse_at, 1_120);
    assert!(matches!(
        provisioner.announce_rotation(
            &agent_keys,
            &principal,
            RotationSubject::Agent,
            &agent_did,
            &current_id,
            &pending_id,
            60,
            51,
        ),
        Err(SessionKeyError::RotationAlreadyOpen)
    ));

    provisioner.contract_mut().advance(1_050, 49);
    let (waiting, no_replacement) = provisioner
        .reconcile_rotation(&principal, &agent_did)
        .unwrap_or_else(|error| panic!("waiting rotation: {error}"));
    assert_eq!(waiting.state, RotationJourneyState::ChallengeOpen);
    assert!(no_replacement.is_none());
    provisioner.contract_mut().advance(1_060, 49);
    provisioner
        .contract_mut()
        .commit_rotation()
        .unwrap_or_else(|error| panic!("commit rotation: {error}"));
    assert!(provisioner
        .contract()
        .primary_key_valid(current_public, 1_060, 49));
    assert!(provisioner
        .contract()
        .primary_key_valid(pending_public, 1_060, 49));
    let (effective, replacement) = provisioner
        .reconcile_rotation(&principal, &agent_did)
        .unwrap_or_else(|error| panic!("effective rotation: {error}"));
    assert_eq!(
        effective.state,
        RotationJourneyState::Effective {
            superseded_key_usable_before_sequence: 50,
            observed_sequence: 49,
        }
    );
    let replacement = replacement.unwrap_or_else(|| panic!("agent session not restored"));
    assert_eq!(replacement.agent_did, old_session.agent_did);
    assert_eq!(
        provisioner.contract().direct_sign(old_session.grant_id),
        Err(SigningError::Revoked)
    );
    assert_eq!(
        provisioner.contract().direct_sign(replacement.grant_id),
        Ok(SigningMode::ProtocolSessionKey)
    );
    provisioner.contract_mut().advance(1_061, 50);
    assert!(!provisioner
        .contract()
        .primary_key_valid(current_public, 1_061, 50));
    assert!(provisioner
        .contract()
        .primary_key_valid(pending_public, 1_061, 50));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn human_primary_rotation_uses_the_same_protocol_challenge_path_without_agent_authority() {
    let root = directory("human-protocol-rotation");
    let principal = principal();
    let human_did = did("rotating-human");
    let (human_keys, human_current_id, human_pending_id, human_current, _) = keystore(
        &root.join("human-custody"),
        &principal,
        KeyClass::HumanPrimary,
        [0x31; 32],
        [0x32; 32],
    );
    let human_layer = AgentLayer::new(
        root.join("human-agent-layer"),
        principal.clone(),
        human_did.clone(),
        human_current,
    );
    let mut human = SessionKeyProvisioner::new(human_layer, OperatingEntropy, policy(), registry());
    human
        .announce_rotation(
            &human_keys,
            &principal,
            RotationSubject::Human,
            &human_did,
            &human_current_id,
            &human_pending_id,
            3_600,
            80,
        )
        .unwrap_or_else(|error| panic!("human rotation: {error}"));
    human.contract_mut().advance(4_600, 79);
    human
        .contract_mut()
        .commit_rotation()
        .unwrap_or_else(|error| panic!("human commit: {error}"));
    let (human_effective, human_session) = human
        .reconcile_rotation(&principal, &human_did)
        .unwrap_or_else(|error| panic!("human reconcile: {error}"));
    assert!(matches!(
        human_effective.state,
        RotationJourneyState::Effective { .. }
    ));
    assert!(human_session.is_none());
    let _ = fs::remove_dir_all(root);
}

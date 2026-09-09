use layerx_human_test_support as support;

use std::fs;

use layerx_human_service::agents::{
    AgentKeyChangeKind, AgentKeyChangeRequest, AgentKeyChangeStage, AgentRecovery,
    AgentRecoveryBoundary, AgentRecoveryBoundaryError, AgentRecoveryError, CompetingRotation,
    ProtocolKeyChangeEvidence, ProtocolKeyChangeObservation, ProtocolKeyChangeState,
    RECOVERY_DELAY_COPY_KEY, ROTATION_COMPETITION_COPY_KEY, ROTATION_DELAY_COPY_KEY,
};
use layerx_human_service::audit::{AuditChain, AuditEvent, SecurityChangeKind};
use layerx_human_service::notify::{AgentId, Dispatcher, NotificationClass};
use layerx_human_service::store::{EvidenceRef, PrincipalId, Table};
use layerx_human_service::trace::TraceId;
use layerx_types::ids::Did;
use layerx_types::verify::VerificationLevel;

use support::{directory, install_and_open, principal, retention_uniform, row_key, tenancy};

const DELAY: u64 = 60;

#[derive(Clone)]
struct RotationState {
    previous: [u8; 32],
    pending: [u8; 32],
    effective_at: u64,
    lapse_at: u64,
    effective_sequence: u64,
    committed: bool,
}

#[derive(Clone)]
struct RecoveryState {
    previous: [u8; 32],
    pending: [u8; 32],
    effective_at: u64,
    lapse_at: u64,
    committed: bool,
    vetoed: bool,
}

struct CoreRecoveryContract {
    principal: PrincipalId,
    agent_did: Did,
    human_did: Did,
    primary: [u8; 32],
    superseded: Option<[u8; 32]>,
    now: u64,
    sequence: u64,
    rotation: Option<RotationState>,
    recovery: Option<RecoveryState>,
    next_key: u8,
    begin_calls: usize,
}

impl CoreRecoveryContract {
    fn new(principal: PrincipalId, agent_did: Did, human_did: Did) -> Self {
        Self {
            principal,
            agent_did,
            human_did,
            primary: [0x11; 32],
            superseded: None,
            now: 100,
            sequence: 10,
            rotation: None,
            recovery: None,
            next_key: 0x20,
            begin_calls: 0,
        }
    }

    fn advance(&mut self, now: u64, sequence: u64) {
        assert!(now >= self.now);
        assert!(sequence >= self.sequence);
        self.now = now;
        self.sequence = sequence;
    }

    fn commit_rotation(&mut self) -> Result<(), &'static str> {
        let rotation = self.rotation.as_mut().ok_or("rotation missing")?;
        if self.now < rotation.effective_at {
            return Err("challenge delay open");
        }
        if self.now > rotation.lapse_at {
            self.rotation = None;
            return Ok(());
        }
        self.superseded = Some(self.primary);
        self.primary = rotation.pending;
        rotation.committed = true;
        Ok(())
    }

    fn commit_recovery(&mut self) -> Result<(), &'static str> {
        let recovery = self.recovery.as_mut().ok_or("recovery missing")?;
        if recovery.vetoed {
            return Err("recovery vetoed");
        }
        if self.now < recovery.effective_at {
            return Err("challenge delay open");
        }
        if self.now > recovery.lapse_at {
            self.recovery = None;
            return Ok(());
        }
        self.superseded = Some(self.primary);
        self.primary = recovery.pending;
        recovery.committed = true;
        Ok(())
    }

    fn rotation_competition(&self) -> Option<CompetingRotation> {
        self.rotation.as_ref().map(|rotation| CompetingRotation {
            pending_public_key: rotation.pending,
            effective_at: rotation.effective_at,
            lapse_at: rotation.lapse_at,
            effective_sequence: rotation.effective_sequence,
            state: if rotation.committed {
                ProtocolKeyChangeState::Effective
            } else if self.now < rotation.effective_at {
                ProtocolKeyChangeState::ChallengeOpen
            } else if self.now <= rotation.lapse_at {
                ProtocolKeyChangeState::ReadyToCommit
            } else {
                ProtocolKeyChangeState::Lapsed
            },
        })
    }

    fn old_key_valid(&self, key: [u8; 32], sequence: u64) -> bool {
        if key == self.primary {
            return true;
        }
        self.superseded == Some(key)
            && self
                .rotation
                .as_ref()
                .is_some_and(|rotation| sequence < rotation.effective_sequence)
    }

    fn require_scope(
        &self,
        principal: &PrincipalId,
        request: &AgentKeyChangeRequest,
    ) -> Result<(), AgentRecoveryBoundaryError> {
        if principal == &self.principal
            && request.did == self.agent_did
            && request.human_recovery_authority == self.human_did
        {
            Ok(())
        } else {
            Err(AgentRecoveryBoundaryError::Refused(
                "recovery authority does not control this agent",
            ))
        }
    }
}

impl AgentRecoveryBoundary for CoreRecoveryContract {
    fn begin_key_change(
        &mut self,
        principal: &PrincipalId,
        kind: AgentKeyChangeKind,
        request: &AgentKeyChangeRequest,
    ) -> Result<ProtocolKeyChangeEvidence, AgentRecoveryBoundaryError> {
        self.require_scope(principal, request)?;
        self.begin_calls = self.begin_calls.saturating_add(1);
        let pending = [self.next_key; 32];
        self.next_key = self.next_key.saturating_add(1);
        let effective_at = self.now.saturating_add(DELAY);
        let lapse_at = effective_at.saturating_add(DELAY);
        let (effective_sequence, competing_rotation) = match kind {
            AgentKeyChangeKind::Rotation => {
                if let Some(existing) = self.rotation_competition() {
                    return Err(AgentRecoveryBoundaryError::CompetingRotation(existing));
                }
                let effective_sequence = self.sequence.saturating_add(40);
                self.rotation = Some(RotationState {
                    previous: self.primary,
                    pending,
                    effective_at,
                    lapse_at,
                    effective_sequence,
                    committed: false,
                });
                (Some(effective_sequence), None)
            }
            AgentKeyChangeKind::Recovery => {
                if self.recovery.is_some() {
                    return Err(AgentRecoveryBoundaryError::Refused("recovery already open"));
                }
                self.recovery = Some(RecoveryState {
                    previous: self.primary,
                    pending,
                    effective_at,
                    lapse_at,
                    committed: false,
                    vetoed: false,
                });
                (None, self.rotation_competition())
            }
        };
        self.sequence = self.sequence.saturating_add(1);
        let mut evidence = ProtocolKeyChangeEvidence {
            kind,
            did: self.agent_did.clone(),
            recovery_authority: self.human_did.clone(),
            previous_public_key: self.primary,
            pending_public_key: pending,
            effective_at,
            lapse_at,
            effective_sequence,
            observed_at: self.now,
            observed_sequence: self.sequence,
            verification_level: VerificationLevel::BATCH_INCLUDED,
            receipt_digest: [0; 32],
            competing_rotation,
        };
        evidence.receipt_digest = evidence.expected_digest();
        Ok(evidence)
    }

    fn observe_key_change(
        &mut self,
        principal: &PrincipalId,
        kind: AgentKeyChangeKind,
        did: &Did,
    ) -> Result<ProtocolKeyChangeObservation, AgentRecoveryBoundaryError> {
        if principal != &self.principal || did != &self.agent_did {
            return Err(AgentRecoveryBoundaryError::Refused("wrong agent scope"));
        }
        match kind {
            AgentKeyChangeKind::Rotation => {
                let rotation =
                    self.rotation
                        .as_ref()
                        .ok_or(AgentRecoveryBoundaryError::Refused(
                            "rotation is not present",
                        ))?;
                let state = if rotation.committed {
                    ProtocolKeyChangeState::Effective
                } else if self.now < rotation.effective_at {
                    ProtocolKeyChangeState::ChallengeOpen
                } else if self.now <= rotation.lapse_at {
                    ProtocolKeyChangeState::ReadyToCommit
                } else {
                    ProtocolKeyChangeState::Lapsed
                };
                Ok(ProtocolKeyChangeObservation {
                    kind,
                    did: self.agent_did.clone(),
                    previous_public_key: rotation.previous,
                    pending_public_key: rotation.pending,
                    primary_public_key: self.primary,
                    superseded_public_key: state
                        .eq(&ProtocolKeyChangeState::Effective)
                        .then_some(rotation.previous),
                    effective_at: rotation.effective_at,
                    lapse_at: rotation.lapse_at,
                    effective_sequence: Some(rotation.effective_sequence),
                    state,
                    observed_at: self.now,
                    observed_sequence: self.sequence,
                    verification_level: VerificationLevel::STATE_PROVEN,
                    competing_rotation: None,
                })
            }
            AgentKeyChangeKind::Recovery => {
                let recovery =
                    self.recovery
                        .as_ref()
                        .ok_or(AgentRecoveryBoundaryError::Refused(
                            "recovery is not present",
                        ))?;
                let state = if recovery.vetoed {
                    ProtocolKeyChangeState::Vetoed
                } else if recovery.committed {
                    ProtocolKeyChangeState::Effective
                } else if self.now < recovery.effective_at {
                    ProtocolKeyChangeState::ChallengeOpen
                } else if self.now <= recovery.lapse_at {
                    ProtocolKeyChangeState::ReadyToCommit
                } else {
                    ProtocolKeyChangeState::Lapsed
                };
                Ok(ProtocolKeyChangeObservation {
                    kind,
                    did: self.agent_did.clone(),
                    previous_public_key: recovery.previous,
                    pending_public_key: recovery.pending,
                    primary_public_key: self.primary,
                    superseded_public_key: state
                        .eq(&ProtocolKeyChangeState::Effective)
                        .then_some(recovery.previous),
                    effective_at: recovery.effective_at,
                    lapse_at: recovery.lapse_at,
                    effective_sequence: None,
                    state,
                    observed_at: self.now,
                    observed_sequence: self.sequence,
                    verification_level: VerificationLevel::STATE_PROVEN,
                    competing_rotation: self.rotation_competition(),
                })
            }
        }
    }
}

fn did(value: &str) -> Did {
    Did::new(format!("did:layerx:{value}").as_bytes())
        .unwrap_or_else(|error| panic!("DID: {error:?}"))
}

fn agent_id(value: &str) -> AgentId {
    AgentId::new(format!("agt_{value}")).unwrap_or_else(|error| panic!("agent id: {error}"))
}

fn request(
    key: u8,
    agent: &AgentId,
    identity: &Did,
    human_did: &Did,
    history: &EvidenceRef,
) -> AgentKeyChangeRequest {
    AgentKeyChangeRequest {
        idempotency_key: [key; 32],
        agent_id: agent.clone(),
        did: identity.clone(),
        human_recovery_authority: human_did.clone(),
        authority_evidence_digest: [key.saturating_add(0x40); 32],
        history: vec![history.clone()],
    }
}

#[test]
fn protocol_rotation_preserves_identity_history_and_the_exact_superseded_window() {
    let root = directory("agent-key-rotation");
    let map = tenancy(&[("alice", "tenant-a")]);
    let (mut store, _) = install_and_open(&root, &map, retention_uniform(1_000));
    let principal = principal("alice");
    let agent_did = did("managed-rotation");
    let human_did = did("alice");
    let stable_agent_id = agent_id("managedrotation");
    let history = EvidenceRef::new(Table::Journeys, row_key("agent-created-receipt"));
    let trace = TraceId::mint([0x31; 16]);
    let contract =
        CoreRecoveryContract::new(principal.clone(), agent_did.clone(), human_did.clone());
    let old_key = contract.primary;
    let mut service = AgentRecovery::new(contract);
    let mut scope = store
        .principal(&principal)
        .unwrap_or_else(|error| panic!("scope: {error}"));
    scope
        .put(
            Table::Journeys,
            history.key().clone(),
            90,
            b"verified-agent-creation".to_vec(),
        )
        .unwrap_or_else(|error| panic!("history: {error}"));
    let request = request(1, &stable_agent_id, &agent_did, &human_did, &history);

    let started = service
        .rotate(&mut scope, &request, &trace, 100)
        .unwrap_or_else(|error| panic!("rotation start: {error}"));
    assert_eq!(started.agent_id(), &stable_agent_id);
    assert_eq!(started.did(), &agent_did);
    assert_eq!(started.history(), std::slice::from_ref(&history));
    assert_eq!(started.stage(), AgentKeyChangeStage::ChallengeOpen);
    assert_eq!(started.challenge_delay().seconds(), DELAY);
    assert_eq!(started.challenge_delay().to_string(), "1 minute");
    assert_eq!(started.delay_copy_key(), ROTATION_DELAY_COPY_KEY);
    assert_eq!(started.ready_at(), 160);
    assert_eq!(service.boundary().begin_calls, 1);

    let duplicate = service
        .rotate(&mut scope, &request, &trace, 101)
        .unwrap_or_else(|error| panic!("rotation retry: {error}"));
    assert_eq!(duplicate, started);
    assert_eq!(service.boundary().begin_calls, 1);

    service.boundary_mut().advance(160, 49);
    let ready = service
        .reconcile(&mut scope, request.idempotency_key, &trace, 160)
        .unwrap_or_else(|error| panic!("rotation ready: {error}"));
    assert_eq!(ready.stage(), AgentKeyChangeStage::ReadyToCommit);
    assert_eq!(ready.agent_id(), started.agent_id());
    assert_eq!(ready.did(), started.did());
    service
        .boundary_mut()
        .commit_rotation()
        .unwrap_or_else(|error| panic!("rotation commit: {error}"));
    let effective = service
        .reconcile(&mut scope, request.idempotency_key, &trace, 160)
        .unwrap_or_else(|error| panic!("rotation effective: {error}"));
    assert_eq!(effective.stage(), AgentKeyChangeStage::Effective);
    assert_eq!(effective.agent_id(), &stable_agent_id);
    assert_eq!(effective.did(), &agent_did);
    assert_eq!(effective.history(), &[history]);
    assert_eq!(effective.superseded_public_key(), Some(old_key));
    assert_eq!(effective.superseded_key_usable_before_sequence(), Some(50));
    assert!(service.boundary().old_key_valid(old_key, 49));
    assert!(!service.boundary().old_key_valid(old_key, 50));

    let deliveries = Dispatcher::deliveries(&scope)
        .unwrap_or_else(|error| panic!("rotation notifications: {error}"));
    assert_eq!(deliveries.len(), 9);
    assert!(deliveries
        .iter()
        .all(|delivery| delivery.class() == NotificationClass::SecurityKeyRotation));
    drop(scope);
    drop(store);
    fs::remove_dir_all(root).unwrap_or_else(|error| panic!("cleanup: {error}"));
}

#[test]
#[allow(clippy::too_many_lines)]
fn recovery_runs_beside_the_protocols_open_rotation_and_notifies_each_transition() {
    let root = directory("agent-key-recovery-competition");
    let map = tenancy(&[("alice", "tenant-a")]);
    let (mut store, _) = install_and_open(&root, &map, retention_uniform(1_000));
    let principal = principal("alice");
    let agent_did = did("managed-recovery");
    let human_did = did("alice");
    let stable_agent_id = agent_id("managedrecovery");
    let history = EvidenceRef::new(Table::Journeys, row_key("agent-history-receipt"));
    let trace = TraceId::mint([0x41; 16]);
    let contract =
        CoreRecoveryContract::new(principal.clone(), agent_did.clone(), human_did.clone());
    let mut service = AgentRecovery::new(contract);
    let mut scope = store
        .principal(&principal)
        .unwrap_or_else(|error| panic!("scope: {error}"));
    scope
        .put(
            Table::Journeys,
            history.key().clone(),
            90,
            b"verified-continuous-history".to_vec(),
        )
        .unwrap_or_else(|error| panic!("history: {error}"));
    let rotation_request = request(2, &stable_agent_id, &agent_did, &human_did, &history);
    let rotation = service
        .rotate(&mut scope, &rotation_request, &trace, 100)
        .unwrap_or_else(|error| panic!("rotation: {error}"));

    let competing_request = request(3, &stable_agent_id, &agent_did, &human_did, &history);
    let competition = service.rotate(&mut scope, &competing_request, &trace, 100);
    assert!(matches!(
        competition,
        Err(AgentRecoveryError::Boundary(
            AgentRecoveryBoundaryError::CompetingRotation(_)
        ))
    ));

    let recovery_request = request(4, &stable_agent_id, &agent_did, &human_did, &history);
    let recovering = service
        .recover(&mut scope, &recovery_request, &trace, 100)
        .unwrap_or_else(|error| panic!("recovery: {error}"));
    assert_eq!(recovering.kind(), AgentKeyChangeKind::Recovery);
    assert_eq!(recovering.delay_copy_key(), RECOVERY_DELAY_COPY_KEY);
    assert_eq!(recovering.agent_id(), rotation.agent_id());
    assert_eq!(recovering.did(), rotation.did());
    assert_eq!(recovering.history(), rotation.history());
    assert_eq!(
        recovering.competition_copy_key(),
        Some(ROTATION_COMPETITION_COPY_KEY)
    );
    let open_rotation = recovering
        .competing_rotation()
        .unwrap_or_else(|| panic!("competing rotation missing"));
    assert_eq!(
        open_rotation.pending_public_key,
        rotation.pending_public_key()
    );
    assert_eq!(open_rotation.state, ProtocolKeyChangeState::ChallengeOpen);

    service.boundary_mut().advance(159, 20);
    let still_waiting = service
        .reconcile(&mut scope, recovery_request.idempotency_key, &trace, 159)
        .unwrap_or_else(|error| panic!("recovery waiting: {error}"));
    assert_eq!(still_waiting.stage(), AgentKeyChangeStage::ChallengeOpen);
    assert_eq!(
        Dispatcher::deliveries(&scope).map_or(0, |rows| rows.len()),
        6
    );

    service.boundary_mut().advance(160, 21);
    service
        .boundary_mut()
        .commit_recovery()
        .unwrap_or_else(|error| panic!("recovery commit: {error}"));
    let recovered = service
        .reconcile(&mut scope, recovery_request.idempotency_key, &trace, 160)
        .unwrap_or_else(|error| panic!("recovery effective: {error}"));
    assert_eq!(recovered.stage(), AgentKeyChangeStage::Effective);
    assert_eq!(recovered.agent_id(), &stable_agent_id);
    assert_eq!(recovered.did(), &agent_did);
    assert_eq!(recovered.history(), std::slice::from_ref(&history));
    assert_eq!(recovered.superseded_key_usable_before_sequence(), None);
    assert_eq!(
        recovered.competing_rotation().map(|value| value.state),
        Some(ProtocolKeyChangeState::ReadyToCommit)
    );

    let restarted =
        AgentRecovery::<CoreRecoveryContract>::load(&scope, recovery_request.idempotency_key)
            .unwrap_or_else(|error| panic!("load after restart: {error}"))
            .unwrap_or_else(|| panic!("stored recovery missing"));
    assert_eq!(restarted, recovered);
    let duplicate = service
        .recover(&mut scope, &recovery_request, &trace, 161)
        .unwrap_or_else(|error| panic!("recovery retry: {error}"));
    assert_eq!(duplicate, recovered);

    let deliveries = Dispatcher::deliveries(&scope)
        .unwrap_or_else(|error| panic!("security notifications: {error}"));
    assert_eq!(deliveries.len(), 9);
    assert_eq!(
        deliveries
            .iter()
            .filter(|delivery| delivery.class() == NotificationClass::SecurityKeyRotation)
            .count(),
        3
    );
    assert_eq!(
        deliveries
            .iter()
            .filter(|delivery| delivery.class() == NotificationClass::SecurityRecovery)
            .count(),
        6
    );
    let audit = AuditChain::open(&scope).unwrap_or_else(|error| panic!("audit: {error}"));
    let entries = audit
        .entries(&scope)
        .unwrap_or_else(|error| panic!("audit entries: {error}"));
    assert_eq!(
        entries
            .iter()
            .filter(|entry| matches!(entry.event(), AuditEvent::SecurityChange { .. }))
            .count(),
        3
    );
    assert!(entries.iter().any(|entry| matches!(
        entry.event(),
        AuditEvent::SecurityChange {
            change: SecurityChangeKind::RecoveryInitiated,
            ..
        }
    )));
    drop(scope);
    drop(store);
    fs::remove_dir_all(root).unwrap_or_else(|error| panic!("cleanup: {error}"));
}

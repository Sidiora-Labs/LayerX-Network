mod support;

use std::collections::BTreeSet;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::thread;

use ed25519_dalek::{Signer as _, SigningKey};
use layerx_agentd::budget::{
    release, reserve, BudgetLimiter, LimitConfig, LimitId, LimitRefusal, LimitScope, ReleaseKind,
    ReservationRequest,
};
use layerx_agentd::capability::{
    consume, evaluate, Capability, CapabilityDimensions, CapabilityId, Ceiling, CeilingError,
    Decision, Dimension, PreparedIntent, RateCeiling, ReceiptApplication,
};
use layerx_agentd::protocol_evidence::RawReceiptEvidence;
use layerx_agentd::receipt as daemon_receipt;
use layerx_agentd::store::{Store, TenantId};
use layerx_human_service::agents::{AgentShell, SpendError, SpendProfile, SpendReconciliation};
use layerx_human_service::store::PrincipalId;
use layerx_proof::receipt::AuthorizedBatch;
use layerx_types::payload::ModuleId;
use sha2::{Digest as _, Sha256};

const AGENT_ID: [u8; 32] = [0x31; 32];
const AGENT_ACCOUNT: [u8; 32] = [0xa1; 32];
const BUDGET_ID: [u8; 32] = [0xb1; 32];
const ASSET: [u8; 32] = [0xc1; 32];
const COUNTERPARTY: [u8; 32] = [0xd1; 32];
const PERIOD_START: u64 = 1_700_000_000;
const WINDOW_START: u64 = 100;
const WINDOW_END: u64 = 10_000;

fn principal(name: &str) -> PrincipalId {
    PrincipalId::new(name).unwrap_or_else(|error| panic!("principal: {error}"))
}

fn profile() -> SpendProfile {
    SpendProfile {
        principal: principal("alice"),
        agent_id: AGENT_ID,
        agent_account: AGENT_ACCOUNT,
        budget_id: BUDGET_ID,
        asset: ASSET,
    }
}

struct DurableReceipts {
    store: Store,
}

/// In-process contract over the actual agentd capability evaluator, capability
/// ceiling, multi-scope budget limiter, receipt verifier, and durable receipt
/// indexes.
struct RealAgentLayer {
    root: std::path::PathBuf,
    tenant: TenantId,
    capability: Capability,
    capability_ceiling: Ceiling,
    budget: BudgetLimiter,
    budget_limit_id: LimitId,
    next_sequence: AtomicU64,
    commit: Mutex<()>,
    durable: Mutex<DurableReceipts>,
}

impl RealAgentLayer {
    fn new(label: &str, capability_total: u128, budget_limit: u128) -> Arc<Self> {
        let root = std::env::temp_dir().join(format!(
            "layerx-human-spend-{}-{}",
            label,
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap_or_else(|error| panic!("agent root: {error}"));
        let tenant = TenantId::new(format!("spend-{label}"))
            .unwrap_or_else(|error| panic!("tenant: {error}"));
        let capability = Capability::new(
            CapabilityId([0x41; 32]),
            tenant.clone(),
            CapabilityDimensions {
                activity_types: BTreeSet::from([7]),
                counterparties: BTreeSet::from([COUNTERPARTY]),
                assets: BTreeSet::from([ASSET]),
                amount_ceiling: 150,
                rate_ceiling: RateCeiling {
                    maximum_uses: 1_000,
                    window_sequences: WINDOW_END - WINDOW_START + 1,
                },
                purposes: BTreeSet::from(["managed-work".to_owned()]),
                expiry_sequence: WINDOW_END + 1,
            },
        )
        .unwrap_or_else(|error| panic!("capability: {error:?}"));
        let budget_limit_id = LimitId([0x51; 16]);
        let budget = BudgetLimiter::new(vec![LimitConfig {
            id: budget_limit_id,
            name: "managed agent period budget".to_owned(),
            scope: LimitScope::Agent(AGENT_ID),
            ceiling: budget_limit,
            consumed: 0,
        }])
        .unwrap_or_else(|error| panic!("budget limiter: {error:?}"));
        let store = Store::open(root.join("agent-store"))
            .unwrap_or_else(|error| panic!("agent store: {error}"));
        Arc::new(Self {
            root,
            tenant,
            capability,
            capability_ceiling: Ceiling::new(
                capability_total,
                support::evidence_verifier(&SigningKey::from_bytes(&[0x84; 32])),
            ),
            budget,
            budget_limit_id,
            next_sequence: AtomicU64::new(WINDOW_START),
            commit: Mutex::new(()),
            durable: Mutex::new(DurableReceipts { store }),
        })
    }

    fn submit(
        &self,
        activity_type: u16,
        counterparty: [u8; 32],
        asset: [u8; 32],
        amount: u128,
        idempotency_key: [u8; 32],
    ) -> Result<(), SubmitRefusal> {
        let sequence = self.next_sequence.fetch_add(1, Ordering::SeqCst);
        let intent = PreparedIntent {
            activity_type,
            counterparty,
            asset,
            amount,
            purpose: "managed-work".to_owned(),
            core_sequence: sequence,
            uses_in_window: sequence - WINDOW_START,
        };
        match evaluate(&self.capability, &intent) {
            Decision::Allow => {}
            Decision::Refuse(dimension) => {
                return Err(SubmitRefusal::Capability(dimension));
            }
        }
        let activity_id = spend_activity_id(sequence, idempotency_key);

        let reservation = ReservationRequest {
            id: idempotency_key,
            amount,
            expiry_sequence: sequence + 100,
            current_sequence: sequence,
            applicable_limits: vec![self.budget_limit_id],
        };
        consume(
            &self.capability_ceiling,
            idempotency_key,
            activity_id,
            amount,
            sequence + 100,
            sequence,
        )
        .map_err(SubmitRefusal::CapabilityCeiling)?;
        if let Err(error) = reserve(&self.budget, &reservation) {
            self.capability_ceiling
                .cancel_unsubmitted(idempotency_key)
                .unwrap_or_else(|failure| panic!("release capability hold: {failure:?}"));
            return Err(SubmitRefusal::Budget(error));
        }

        let material = signed_receipt(sequence, activity_id, amount, asset, counterparty);
        let _commit = self
            .commit
            .lock()
            .unwrap_or_else(|error| panic!("commit lock: {error}"));
        let store_result = {
            let mut durable = self
                .durable
                .lock()
                .unwrap_or_else(|error| panic!("durable lock: {error}"));
            let metadata = daemon_receipt::store(
                &mut durable.store,
                self.tenant.clone(),
                idempotency_key,
                &material.canonical_receipt,
                &material.authorized_batch,
            );
            metadata.map(|_| ()).map_err(|_| ())
        };
        self.capability_ceiling
            .apply_receipt(&ReceiptApplication {
                reservation_id: idempotency_key,
                expected_activity_id: activity_id,
                evidence: material.evidence,
            })
            .unwrap_or_else(|error| panic!("apply capability receipt: {error:?}"));
        release(
            &self.budget,
            idempotency_key,
            ReleaseKind::Executed,
            sequence,
        )
        .unwrap_or_else(|error| panic!("apply budget receipt: {error:?}"));
        if store_result.is_err() {
            Err(SubmitRefusal::ReceiptStore)
        } else {
            Ok(())
        }
    }
}

impl Drop for RealAgentLayer {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[derive(Debug, Eq, PartialEq)]
enum SubmitRefusal {
    Capability(Dimension),
    CapabilityCeiling(CeilingError),
    Budget(LimitRefusal),
    ReceiptStore,
}

#[derive(Clone)]
struct ReceiptMaterial {
    canonical_receipt: Vec<u8>,
    authorized_batch: AuthorizedBatch,
    evidence: RawReceiptEvidence,
}

struct ReceiptFields {
    activity_id: [u8; 32],
    sequence: u64,
    previous_state_root: [u8; 32],
    resulting_state_root: [u8; 32],
    batch_id: [u8; 32],
    asset: [u8; 32],
    amount: u128,
    counterparty: [u8; 32],
}

fn signed_receipt(
    sequence: u64,
    activity_id: [u8; 32],
    amount: u128,
    asset: [u8; 32],
    counterparty: [u8; 32],
) -> ReceiptMaterial {
    let previous_state_root: [u8; 32] =
        Sha256::digest([b"before".as_slice(), &activity_id].concat()).into();
    let fields = ReceiptFields {
        activity_id,
        sequence,
        previous_state_root,
        resulting_state_root: Sha256::digest([b"after".as_slice(), &activity_id].concat()).into(),
        batch_id: support::execution_batch_id(previous_state_root, activity_id, sequence),
        asset,
        amount,
        counterparty,
    };
    let signer = SigningKey::from_bytes(&[0x84; 32]);
    let unsigned = encode_receipt(&fields, None);
    let mut digest = Sha256::new();
    digest.update(b"LXP/v1/receipt\0");
    digest.update(&unsigned);
    let signature = signer.sign(&<[u8; 32]>::from(digest.finalize()));
    let canonical_receipt = encode_receipt(&fields, Some(signature.to_bytes()));
    let authorized_batch = AuthorizedBatch::new(
        fields.batch_id,
        fields.asset,
        fields.previous_state_root,
        fields.resulting_state_root,
        signer.verifying_key().to_bytes(),
    );
    let evidence = support::raw_receipt_evidence(
        canonical_receipt.clone(),
        authorized_batch,
        sequence,
        &signer,
    );
    ReceiptMaterial {
        canonical_receipt,
        authorized_batch,
        evidence,
    }
}

fn spend_activity_id(sequence: u64, idempotency_key: [u8; 32]) -> [u8; 32] {
    Sha256::digest(
        [
            b"layerx-human-spend-activity/v1".as_slice(),
            &sequence.to_be_bytes(),
            &idempotency_key,
        ]
        .concat(),
    )
    .into()
}

fn encode_receipt(fields: &ReceiptFields, signature: Option<[u8; 64]>) -> Vec<u8> {
    let debit_before = 10_000_u128;
    let credit_before = 20_000_u128;
    let mut bytes = Vec::new();
    push_u16(&mut bytes, 1);
    push_u16(&mut bytes, 0x5201);
    push_u16(&mut bytes, 1);
    push_bytes(&mut bytes, &fields.activity_id);
    push_u64(&mut bytes, fields.sequence);
    push_bytes(&mut bytes, &fields.previous_state_root);
    push_bytes(&mut bytes, &fields.resulting_state_root);
    push_bytes(&mut bytes, &[0x81; 32]);
    bytes.extend_from_slice(&0_i32.to_be_bytes());
    bytes.extend_from_slice(&0_u32.to_be_bytes());
    bytes.extend_from_slice(&1_u128.to_be_bytes());
    push_bytes(&mut bytes, &fields.batch_id);
    push_u16(&mut bytes, ModuleId::Asset as u16);
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.push(1);
    push_bytes(&mut bytes, &fields.asset);
    bytes.extend_from_slice(&fields.amount.to_be_bytes());
    push_bytes(&mut bytes, &AGENT_ACCOUNT);
    bytes.extend_from_slice(&debit_before.to_be_bytes());
    bytes.extend_from_slice(&(debit_before - fields.amount).to_be_bytes());
    push_u64(&mut bytes, fields.sequence - WINDOW_START + 1);
    push_bytes(&mut bytes, &fields.counterparty);
    bytes.extend_from_slice(&credit_before.to_be_bytes());
    bytes.extend_from_slice(&(credit_before + fields.amount).to_be_bytes());
    push_bytes(&mut bytes, &[0x91; 32]);
    push_bytes(&mut bytes, &[0x92; 32]);
    push_bytes(&mut bytes, &[0x93; 32]);
    push_u64(&mut bytes, PERIOD_START + fields.sequence);
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
    let length = u32::try_from(value.len()).unwrap_or_else(|_| panic!("receipt field overflow"));
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(value);
}

#[test]
fn spend_surface_fails_closed_without_a_canonical_budget_state_record() {
    let layer = RealAgentLayer::new("reconcile", 1_000, 500);
    assert_eq!(
        layer.submit(7, COUNTERPARTY, ASSET, 100, [1; 32]),
        Err(SubmitRefusal::CapabilityCeiling(CeilingError::Unreconciled))
    );
    assert_eq!(
        layer.submit(7, COUNTERPARTY, ASSET, 100, [2; 32]),
        Err(SubmitRefusal::CapabilityCeiling(CeilingError::Unreconciled))
    );
    let snapshot = layer
        .capability_ceiling
        .snapshot()
        .unwrap_or_else(|error| panic!("capability snapshot: {error:?}"));
    assert_eq!(snapshot.consumed, 0);
    assert_eq!(snapshot.held, 0);
    assert_eq!(snapshot.reservations, 0);
    assert!(!snapshot.reconciled);
    assert_eq!(
        layer
            .budget
            .held_reservations()
            .unwrap_or_else(|error| panic!("budget reservations: {error:?}")),
        0
    );
    assert_eq!(
        layer
            .budget
            .consumed(layer.budget_limit_id)
            .unwrap_or_else(|error| panic!("budget consumption: {error:?}")),
        0
    );
    let durable = layer
        .durable
        .lock()
        .unwrap_or_else(|error| panic!("durable lock: {error}"));
    for idempotency_key in [[1; 32], [2; 32]] {
        assert!(matches!(
            daemon_receipt::serve(
                &durable.store,
                layer.tenant.clone(),
                daemon_receipt::ReceiptLookupKey::Idempotency(idempotency_key),
            ),
            Err(daemon_receipt::ReceiptStoreError::Missing)
        ));
    }
    drop(durable);

    let service = SpendReconciliation::new(profile())
        .unwrap_or_else(|error| panic!("spend service: {error}"));
    assert!(matches!(
        service.both_shells(&principal("alice")),
        Err(SpendError::ProtocolBudgetStateUnavailable)
    ));
    drop(layer);
}

#[test]
fn concurrent_hostile_activity_stays_inside_capability_and_budget_bounds() {
    let capability_bound = RealAgentLayer::new("capability-bound", 400, 500);
    let barrier = Arc::new(Barrier::new(9));
    let mut workers = Vec::new();
    for index in 0_u8..8 {
        let layer = Arc::clone(&capability_bound);
        let start = Arc::clone(&barrier);
        workers.push(thread::spawn(move || {
            start.wait();
            layer.submit(7, COUNTERPARTY, ASSET, 100, [index.saturating_add(1); 32])
        }));
    }
    barrier.wait();
    let outcomes: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap_or_else(|_| panic!("worker panicked")))
        .collect();
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(
                outcome,
                Err(SubmitRefusal::CapabilityCeiling(CeilingError::Unreconciled))
            ))
            .count(),
        8
    );
    let snapshot = capability_bound
        .capability_ceiling
        .snapshot()
        .unwrap_or_else(|error| panic!("capability snapshot: {error:?}"));
    assert_eq!(snapshot.consumed, 0);
    assert_eq!(snapshot.held, 0);
    assert_eq!(snapshot.reservations, 0);
    assert!(!snapshot.reconciled);
    assert_eq!(
        capability_bound
            .budget
            .held_reservations()
            .unwrap_or_else(|error| panic!("budget reservations: {error:?}")),
        0
    );
    assert_eq!(
        capability_bound.submit(8, COUNTERPARTY, ASSET, 1, [0x21; 32]),
        Err(SubmitRefusal::Capability(Dimension::ActivityType))
    );
    assert_eq!(
        capability_bound.submit(7, [0xee; 32], ASSET, 1, [0x22; 32]),
        Err(SubmitRefusal::Capability(Dimension::Counterparty))
    );
    assert_eq!(
        capability_bound.submit(7, COUNTERPARTY, ASSET, 151, [0x23; 32]),
        Err(SubmitRefusal::Capability(Dimension::Amount))
    );

    let service = SpendReconciliation::new(profile())
        .unwrap_or_else(|error| panic!("spend service: {error}"));
    assert!(matches!(
        service.both_shells(&principal("alice")),
        Err(SpendError::ProtocolBudgetStateUnavailable)
    ));

    for shell in [AgentShell::Mobile, AgentShell::Desktop] {
        assert!(matches!(
            service.for_shell(&principal("hostile-agent"), shell),
            Err(SpendError::WrongPrincipal)
        ));
    }

    let budget_bound = RealAgentLayer::new("budget-bound", 1_000, 250);
    assert_eq!(
        budget_bound.submit(7, COUNTERPARTY, ASSET, 100, [0x31; 32]),
        Err(SubmitRefusal::CapabilityCeiling(CeilingError::Unreconciled))
    );
    assert_eq!(
        budget_bound.submit(7, COUNTERPARTY, ASSET, 100, [0x32; 32]),
        Err(SubmitRefusal::CapabilityCeiling(CeilingError::Unreconciled))
    );
    for (id, amount) in [([0x41; 32], 100), ([0x42; 32], 100)] {
        reserve(
            &budget_bound.budget,
            &ReservationRequest {
                id,
                amount,
                expiry_sequence: WINDOW_START + 100,
                current_sequence: WINDOW_START,
                applicable_limits: vec![budget_bound.budget_limit_id],
            },
        )
        .unwrap_or_else(|error| panic!("real budget reservation: {error:?}"));
    }
    assert!(matches!(
        reserve(
            &budget_bound.budget,
            &ReservationRequest {
                id: [0x43; 32],
                amount: 60,
                expiry_sequence: WINDOW_START + 100,
                current_sequence: WINDOW_START,
                applicable_limits: vec![budget_bound.budget_limit_id],
            },
        ),
        Err(LimitRefusal::Exceeded { .. })
    ));
    assert_eq!(
        budget_bound
            .budget
            .held_reservations()
            .unwrap_or_else(|error| panic!("budget reservations: {error:?}")),
        2
    );
    assert_eq!(
        budget_bound
            .budget
            .consumed(budget_bound.budget_limit_id)
            .unwrap_or_else(|error| panic!("budget consumption: {error:?}")),
        0
    );
    let budget_service = SpendReconciliation::new(profile())
        .unwrap_or_else(|error| panic!("budget spend service: {error}"));
    assert!(matches!(
        budget_service.for_shell(&principal("alice"), AgentShell::Desktop),
        Err(SpendError::ProtocolBudgetStateUnavailable)
    ));
    drop(budget_bound);
}

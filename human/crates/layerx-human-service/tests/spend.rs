mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::thread;

use ed25519_dalek::{Signer as _, SigningKey};
use layerx_agentd::budget::{
    reconcile, release, reserve, BudgetLimiter, LimitConfig, LimitId, LimitRefusal, LimitScope,
    LocalAccounting, ProtocolBudgetState, ReconcileError, ReleaseKind, ReservationRequest,
    SpendReceiptEvidence as BudgetReceiptEvidence,
};
use layerx_agentd::capability::{
    consume, evaluate, Capability, CapabilityDimensions, CapabilityId, Ceiling, CeilingError,
    Decision, Dimension, PreparedIntent, RateCeiling, ReceiptApplication,
};
use layerx_agentd::protocol_evidence::RawReceiptEvidence;
use layerx_agentd::receipt::{self as daemon_receipt, ReceiptLookupKey};
use layerx_agentd::store::{Store, TenantId};
use layerx_human_service::agents::{
    AgentShell, ProtocolBudgetEvidence, ReconciliationDirection, SpendAgentContract,
    SpendBoundaryError, SpendError, SpendProfile, SpendReceiptEvidence, SpendReconciliation,
    SpendReconciliationStatus, SpendSnapshot, RECONCILIATION_COPY_KEY, RECONCILIATION_EXPLANATION,
};
use layerx_human_service::store::PrincipalId;
use layerx_proof::receipt::{verify, AuthorizedBatch};
use layerx_types::payload::ModuleId;
use layerx_types::verify::VerificationLevel;
use sha2::{Digest as _, Sha256};

const AGENT_ID: [u8; 32] = [0x31; 32];
const AGENT_ACCOUNT: [u8; 32] = [0xa1; 32];
const BUDGET_ID: [u8; 32] = [0xb1; 32];
const ASSET: [u8; 32] = [0xc1; 32];
const COUNTERPARTY: [u8; 32] = [0xd1; 32];
const PERIOD_START: u64 = 1_700_000_000;
const PERIOD_END: u64 = PERIOD_START + 2_592_000;
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

#[derive(Clone)]
struct AgentBoundary {
    layer: Arc<RealAgentLayer>,
    omit_latest_receipt: bool,
}

impl SpendAgentContract for AgentBoundary {
    fn spend_snapshot(
        &self,
        principal: &PrincipalId,
        agent_id: [u8; 32],
    ) -> Result<SpendSnapshot, SpendBoundaryError> {
        let mut snapshot = self.layer.snapshot(principal, agent_id)?;
        if self.omit_latest_receipt {
            snapshot.receipts.pop();
        }
        Ok(snapshot)
    }
}

#[derive(Clone)]
struct ReceiptRecord {
    idempotency_key: [u8; 32],
    authorized_batch: AuthorizedBatch,
    evidence: RawReceiptEvidence,
}

struct DurableReceipts {
    store: Store,
    records: BTreeMap<u64, ReceiptRecord>,
}

/// In-process contract over the actual agentd capability evaluator, capability
/// ceiling, multi-scope budget limiter, receipt verifier, durable receipt
/// indexes, and budget reconciler.
struct RealAgentLayer {
    root: std::path::PathBuf,
    principal: PrincipalId,
    tenant: TenantId,
    capability: Capability,
    capability_ceiling: Ceiling,
    budget: BudgetLimiter,
    budget_limit_id: LimitId,
    budget_limit: u128,
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
            principal: principal("alice"),
            tenant,
            capability,
            capability_ceiling: Ceiling::new(capability_total),
            budget,
            budget_limit_id,
            budget_limit,
            next_sequence: AtomicU64::new(WINDOW_START),
            commit: Mutex::new(()),
            durable: Mutex::new(DurableReceipts {
                store,
                records: BTreeMap::new(),
            }),
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

        let material = signed_receipt(sequence, idempotency_key, amount, asset, counterparty);
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
            match metadata {
                Ok(metadata) => {
                    durable.records.insert(
                        metadata.global_sequence,
                        ReceiptRecord {
                            idempotency_key,
                            authorized_batch: material.authorized_batch,
                            evidence: material.evidence.clone(),
                        },
                    );
                    Ok(())
                }
                Err(_) => Err(()),
            }
        };
        self.capability_ceiling
            .apply_receipt(&ReceiptApplication {
                reservation_id: idempotency_key,
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

    fn snapshot(
        &self,
        requesting_principal: &PrincipalId,
        agent_id: [u8; 32],
    ) -> Result<SpendSnapshot, SpendBoundaryError> {
        if requesting_principal != &self.principal || agent_id != AGENT_ID {
            return Err(SpendBoundaryError::Refused(
                "wrong principal or managed agent",
            ));
        }
        let _commit = self
            .commit
            .lock()
            .map_err(|_| SpendBoundaryError::Unavailable)?;
        let durable = self
            .durable
            .lock()
            .map_err(|_| SpendBoundaryError::Unavailable)?;
        let mut receipts = Vec::with_capacity(durable.records.len());
        let mut accounting_receipts = Vec::with_capacity(durable.records.len());
        let mut receipt_total = 0_u128;
        let mut last_receipt = None;
        for (sequence, record) in &durable.records {
            let served = daemon_receipt::serve(
                &durable.store,
                self.tenant.clone(),
                ReceiptLookupKey::Idempotency(record.idempotency_key),
            )
            .map_err(|_| SpendBoundaryError::CorruptResponse)?;
            if served.metadata.global_sequence != *sequence {
                return Err(SpendBoundaryError::CorruptResponse);
            }
            let verified = verify(&served.canonical_bytes, &record.authorized_batch)
                .map_err(|_| SpendBoundaryError::CorruptResponse)?;
            let protocol = verified
                .receipt()
                .protocol()
                .ok_or(SpendBoundaryError::CorruptResponse)?;
            receipt_total = receipt_total
                .checked_add(protocol.amount())
                .ok_or(SpendBoundaryError::CorruptResponse)?;
            last_receipt = Some(protocol.activity_id());
            accounting_receipts.push(BudgetReceiptEvidence {
                window_start_sequence: WINDOW_START,
                evidence: record.evidence.clone(),
            });
            receipts.push(SpendReceiptEvidence {
                canonical_receipt: served.canonical_bytes,
                authorized_batch: record.authorized_batch,
            });
        }
        let consumed = self
            .budget
            .consumed(self.budget_limit_id)
            .map_err(|_| SpendBoundaryError::CorruptResponse)?;
        let observed_head_sequence = self
            .next_sequence
            .load(Ordering::SeqCst)
            .saturating_sub(1)
            .max(WINDOW_START);
        let protocol = ProtocolBudgetState {
            evidence: support::raw_budget_state(
                consumed,
                self.budget_limit - consumed,
                WINDOW_START,
                WINDOW_END,
                observed_head_sequence,
            ),
        };
        let mut local = LocalAccounting {
            consumed: receipt_total,
            window_start_sequence: WINDOW_START,
            last_receipt,
        };
        let reconciled = reconcile(&mut local, protocol, &accounting_receipts)
            .map_err(map_reconciliation_failure)?;
        let evidence_digest: [u8; 32] = Sha256::digest(
            [
                AGENT_ID.as_slice(),
                BUDGET_ID.as_slice(),
                &reconciled.protocol_consumed.to_be_bytes(),
                &reconciled.observed_head_sequence.to_be_bytes(),
            ]
            .concat(),
        )
        .into();
        Ok(SpendSnapshot {
            protocol_budget: ProtocolBudgetEvidence {
                agent_id: AGENT_ID,
                budget_id: BUDGET_ID,
                asset: ASSET,
                period_start: PERIOD_START,
                period_end: PERIOD_END,
                window_start_sequence: reconciled.window_start_sequence,
                window_end_sequence: reconciled.window_end_sequence,
                observed_head_sequence: reconciled.observed_head_sequence,
                limit: self.budget_limit,
                consumed: reconciled.protocol_consumed,
                remaining: reconciled.remaining,
                verification_level: VerificationLevel::STATE_PROVEN,
                evidence_digest,
            },
            receipts,
        })
    }
}

impl Drop for RealAgentLayer {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn map_reconciliation_failure(_error: ReconcileError) -> SpendBoundaryError {
    SpendBoundaryError::CorruptResponse
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
    idempotency_key: [u8; 32],
    amount: u128,
    asset: [u8; 32],
    counterparty: [u8; 32],
) -> ReceiptMaterial {
    let activity_id: [u8; 32] = Sha256::digest(
        [
            b"layerx-human-spend-activity/v1".as_slice(),
            &sequence.to_be_bytes(),
            &idempotency_key,
        ]
        .concat(),
    )
    .into();
    let fields = ReceiptFields {
        activity_id,
        sequence,
        previous_state_root: Sha256::digest([b"before".as_slice(), &activity_id].concat()).into(),
        resulting_state_root: Sha256::digest([b"after".as_slice(), &activity_id].concat()).into(),
        batch_id: Sha256::digest([b"batch".as_slice(), &activity_id].concat()).into(),
        asset,
        amount,
        counterparty,
    };
    let signing_seed: [u8; 32] =
        Sha256::digest([b"spend-sequencer".as_slice(), &activity_id].concat()).into();
    let signer = SigningKey::from_bytes(&signing_seed);
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
fn receipt_only_spend_adopts_verified_protocol_state_and_both_shells_match() {
    let layer = RealAgentLayer::new("reconcile", 1_000, 500);
    layer
        .submit(7, COUNTERPARTY, ASSET, 100, [1; 32])
        .unwrap_or_else(|error| panic!("first activity: {error:?}"));
    layer
        .submit(7, COUNTERPARTY, ASSET, 100, [2; 32])
        .unwrap_or_else(|error| panic!("second activity: {error:?}"));

    let current = SpendReconciliation::new(
        AgentBoundary {
            layer: Arc::clone(&layer),
            omit_latest_receipt: false,
        },
        profile(),
    )
    .unwrap_or_else(|error| panic!("current spend service: {error}"));
    let in_sync = current
        .both_shells(&principal("alice"))
        .unwrap_or_else(|error| panic!("in-sync spend: {error}"));
    assert_eq!(in_sync.mobile.spend, in_sync.desktop.spend);
    assert_eq!(in_sync.mobile.spend.spent, 200);
    assert_eq!(in_sync.mobile.spend.receipt_spent, 200);
    assert_eq!(in_sync.mobile.spend.remaining, 300);
    assert_eq!(in_sync.mobile.spend.receipt_count, 2);
    assert_eq!(
        in_sync.mobile.spend.reconciliation,
        SpendReconciliationStatus::InSync
    );
    assert_eq!(in_sync.mobile.spend.reconciliation_copy_key, None);

    let lagging = SpendReconciliation::new(
        AgentBoundary {
            layer,
            omit_latest_receipt: true,
        },
        profile(),
    )
    .unwrap_or_else(|error| panic!("lagging spend service: {error}"));
    let reconciled = lagging
        .both_shells(&principal("alice"))
        .unwrap_or_else(|error| panic!("reconciled spend: {error}"));
    assert_eq!(reconciled.mobile.spend, reconciled.desktop.spend);
    assert_eq!(reconciled.mobile.shell, AgentShell::Mobile);
    assert_eq!(reconciled.desktop.shell, AgentShell::Desktop);
    assert_eq!(reconciled.mobile.spend.receipt_spent, 100);
    assert_eq!(reconciled.mobile.spend.spent, 200);
    assert_eq!(reconciled.mobile.spend.remaining, 300);
    assert_eq!(
        reconciled.mobile.spend.reconciliation,
        SpendReconciliationStatus::ProtocolAdopted {
            direction: ReconciliationDirection::ProtocolHigher,
            difference: 100,
        }
    );
    assert_eq!(
        reconciled.mobile.spend.reconciliation_copy_key,
        Some(RECONCILIATION_COPY_KEY)
    );
    assert_eq!(
        reconciled.mobile.spend.reconciliation_explanation,
        Some(RECONCILIATION_EXPLANATION)
    );
    assert!(reconciled.mobile.spend.evidence_digests.len() >= 2);
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
    assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 4);
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(
                outcome,
                Err(SubmitRefusal::CapabilityCeiling(CeilingError::Exceeded))
            ))
            .count(),
        4
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

    let service = SpendReconciliation::new(
        AgentBoundary {
            layer: Arc::clone(&capability_bound),
            omit_latest_receipt: false,
        },
        profile(),
    )
    .unwrap_or_else(|error| panic!("spend service: {error}"));
    let surfaces = service
        .both_shells(&principal("alice"))
        .unwrap_or_else(|error| panic!("spend surfaces: {error}"));
    assert_eq!(surfaces.mobile.spend, surfaces.desktop.spend);
    assert_eq!(surfaces.mobile.spend.spent, 400);
    assert_eq!(surfaces.mobile.spend.receipt_spent, 400);
    assert_eq!(surfaces.mobile.spend.remaining, 100);

    for shell in [AgentShell::Mobile, AgentShell::Desktop] {
        assert!(matches!(
            service.for_shell(&principal("hostile-agent"), shell),
            Err(SpendError::WrongPrincipal)
        ));
    }

    let budget_bound = RealAgentLayer::new("budget-bound", 1_000, 250);
    assert!(budget_bound
        .submit(7, COUNTERPARTY, ASSET, 100, [0x31; 32])
        .is_ok());
    assert!(budget_bound
        .submit(7, COUNTERPARTY, ASSET, 100, [0x32; 32])
        .is_ok());
    assert!(matches!(
        budget_bound.submit(7, COUNTERPARTY, ASSET, 60, [0x33; 32]),
        Err(SubmitRefusal::Budget(LimitRefusal::Exceeded { .. }))
    ));
    let budget_service = SpendReconciliation::new(
        AgentBoundary {
            layer: budget_bound,
            omit_latest_receipt: false,
        },
        profile(),
    )
    .unwrap_or_else(|error| panic!("budget spend service: {error}"));
    let budget_view = budget_service
        .for_shell(&principal("alice"), AgentShell::Desktop)
        .unwrap_or_else(|error| panic!("budget view: {error}"));
    assert_eq!(budget_view.spend.spent, 200);
    assert_eq!(budget_view.spend.remaining, 50);
}

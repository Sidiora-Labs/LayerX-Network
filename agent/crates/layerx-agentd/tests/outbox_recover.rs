use ed25519_dalek::SigningKey;
use layerx_agentd::budget::{
    budget_state_key, hold_unknown, ProtocolBudgetState, UnknownReservation, BUDGET_MODULE_ID,
};
use layerx_agentd::human_runtime::{recover_tenant_budget, BudgetRecoveryRequest, RecoveryRefusal};
use layerx_agentd::outbox::RecoveryError;
use layerx_agentd::protocol_evidence::{
    EvidenceAuthority, RawReceiptEvidence, RawStateEvidence, StateEvidenceError,
    VerifierPolicyError,
};
use layerx_agentd::receipt::{
    evidence_inventory, persist_evidence, serve_evidence, store as store_receipt, ReceiptStoreError,
};
use layerx_agentd::store::{Store, TenantId};
use layerx_client::evidence::RootSelector;
use layerx_proof::receipt::AuthorizedBatch;
use layerx_proof::state_witness::StateWitness;
use layerx_types::verify::VerificationLevel;
use layerx_wire::hash::program_execution_batch_id;

use support::{directory, tenant, StateHeaderIdentity};

mod support;

const BUDGET_ID: [u8; 32] = [0x51; 32];
const SEQUENCER_SEED: [u8; 32] = [0x3a; 32];

fn core_budget_record(per_period_limit: u128, spent_this_period: u128) -> Vec<u8> {
    let mut bytes = vec![0_u8; 278];
    bytes[1] = 1;
    bytes[2..34].copy_from_slice(&BUDGET_ID);
    bytes[34..66].copy_from_slice(&[0x52; 32]);
    bytes[66..98].copy_from_slice(&[0x53; 32]);
    bytes[98..130].copy_from_slice(&[0x24; 32]);
    bytes[130..162].copy_from_slice(&[0x54; 32]);
    bytes[162..178].copy_from_slice(&per_period_limit.to_be_bytes());
    bytes[178..194].copy_from_slice(&per_period_limit.to_be_bytes());
    bytes[194..210].copy_from_slice(&0_u128.to_be_bytes());
    bytes[210..226].copy_from_slice(&spent_this_period.to_be_bytes());
    bytes[226..242].copy_from_slice(&0_u128.to_be_bytes());
    bytes[242..250].copy_from_slice(&1_000_u64.to_be_bytes());
    bytes[250..258].copy_from_slice(&80_u64.to_be_bytes());
    bytes[258..266].copy_from_slice(&5_000_u64.to_be_bytes());
    bytes[266..274].copy_from_slice(&0_u64.to_be_bytes());
    bytes
}

fn authorised_batch(global_sequence: u64) -> AuthorizedBatch {
    let batch_id =
        program_execution_batch_id([0x21; 32], [0x25; 32], global_sequence, global_sequence, 7)
            .unwrap_or_else(|error| panic!("execution batch id: {error:?}"));
    AuthorizedBatch::new(
        batch_id,
        [0x24; 32],
        [0x21; 32],
        [0x22; 32],
        SigningKey::from_bytes(&SEQUENCER_SEED)
            .verifying_key()
            .to_bytes(),
    )
}

fn store_served_receipt(
    durable: &mut Store,
    tenant: &TenantId,
    idempotency_key: [u8; 32],
    raw: &RawReceiptEvidence,
    global_sequence: u64,
) {
    let metadata = store_receipt(
        durable,
        tenant.clone(),
        idempotency_key,
        raw.canonical_receipt(),
        &authorised_batch(global_sequence),
    )
    .unwrap_or_else(|error| panic!("store receipt: {error:?}"));
    assert_eq!(metadata.global_sequence, global_sequence);
    assert_eq!(metadata.idempotency_key, idempotency_key);
}

fn sequencer_pin() -> [u8; 32] {
    SigningKey::from_bytes(&SEQUENCER_SEED)
        .verifying_key()
        .to_bytes()
}

fn budget_witness(record: &[u8]) -> StateWitness {
    StateWitness {
        module_id: BUDGET_MODULE_ID,
        key: budget_state_key(BUDGET_ID),
        value: record.to_vec(),
        account_path: None,
        leaf_index_a: 0,
        leaf_count_a: 1,
        siblings_a: Vec::new(),
        leaf_count_b: 10,
        siblings_b: vec![[0x61; 32], [0x62; 32], [0x63; 32], [0x64; 32]],
    }
}

fn module_proof_material(
    witness: &StateWitness,
    signing_seed: [u8; 32],
    pinned_key: [u8; 32],
    observed_head: u64,
) -> Vec<u8> {
    let root = witness
        .root()
        .unwrap_or_else(|error| panic!("witness root: {error:?}"));
    let (header, signature) = support::signed_batch_header(
        StateHeaderIdentity {
            signing_seed,
            protocol_version: 3,
            network_id: 42,
            epoch: 2,
            batch_number: 7,
        },
        pinned_key,
        observed_head,
        root,
    );
    let witness_bytes = witness
        .encode()
        .unwrap_or_else(|error| panic!("witness encode: {error:?}"));
    let mut material = Vec::new();
    material.extend_from_slice(&1_u16.to_be_bytes());
    material.push(4);
    material.push(1);
    material.extend_from_slice(
        &u32::try_from(witness_bytes.len())
            .unwrap_or_else(|_| panic!("witness length"))
            .to_be_bytes(),
    );
    material.extend_from_slice(&witness_bytes);
    material.extend_from_slice(&1_u16.to_be_bytes());
    material.extend_from_slice(&pinned_key);
    material.extend_from_slice(&pinned_key);
    material.extend_from_slice(&7_u64.to_be_bytes());
    material.extend_from_slice(&7_u64.to_be_bytes());
    material.extend_from_slice(
        &u32::try_from(header.len())
            .unwrap_or_else(|_| panic!("header length"))
            .to_be_bytes(),
    );
    material.extend_from_slice(&header);
    material.extend_from_slice(&signature);
    material.push(0);
    material
}

fn module_witness_evidence(record: &[u8], pinned_key: [u8; 32]) -> RawStateEvidence {
    let witness = budget_witness(record);
    RawStateEvidence::module_witness(
        record.to_vec(),
        BUDGET_MODULE_ID,
        budget_state_key(BUDGET_ID),
        module_proof_material(&witness, SEQUENCER_SEED, pinned_key, 99),
        RootSelector::Latest,
        pinned_key,
    )
}

#[test]
fn receipt_evidence_round_trips_through_a_real_store() {
    let root = directory("receipt-evidence");
    let tenant = tenant();
    let raw = support::raw_receipt_at([0x11; 32], 0, 25, 100);
    let older = support::raw_receipt_at([0x12; 32], 0, 10, 101);
    let mut durable = Store::open(&root).unwrap_or_else(|error| panic!("store: {error}"));
    store_served_receipt(&mut durable, &tenant, [0x44; 32], &raw, 100);
    store_served_receipt(&mut durable, &tenant, [0x45; 32], &older, 101);
    assert_eq!(
        serve_evidence(&durable, tenant.clone(), [0x44; 32])
            .unwrap_or_else(|error| panic!("serve before persist: {error:?}")),
        None
    );
    let record = persist_evidence(&mut durable, tenant.clone(), [0x44; 32], &raw)
        .unwrap_or_else(|error| panic!("persist evidence: {error:?}"));
    assert_eq!(record.idempotency_key, [0x44; 32]);
    assert_eq!(record.activity_id, [0x11; 32]);
    assert_eq!(record.global_sequence, 100);
    assert_eq!(record.evidence, raw);
    assert_eq!(
        persist_evidence(&mut durable, tenant.clone(), [0x44; 32], &raw)
            .unwrap_or_else(|error| panic!("persist evidence again: {error:?}")),
        record
    );
    let altered_signature = RawReceiptEvidence::new(
        raw.canonical_receipt().to_vec(),
        raw.proof().clone(),
        raw.canonical_header().to_vec(),
        [0x99; 64],
    );
    assert!(matches!(
        persist_evidence(&mut durable, tenant.clone(), [0x44; 32], &altered_signature),
        Err(ReceiptStoreError::Corrupt)
    ));
    assert!(matches!(
        persist_evidence(&mut durable, tenant.clone(), [0x45; 32], &raw),
        Err(ReceiptStoreError::Corrupt)
    ));
    assert!(matches!(
        persist_evidence(&mut durable, tenant.clone(), [0x46; 32], &raw),
        Err(ReceiptStoreError::Missing)
    ));
    drop(durable);

    let reopened = Store::open(&root).unwrap_or_else(|error| panic!("reopen: {error}"));
    assert_eq!(
        serve_evidence(&reopened, tenant.clone(), [0x44; 32])
            .unwrap_or_else(|error| panic!("serve after reopen: {error:?}")),
        Some(record.clone())
    );
    assert_eq!(
        serve_evidence(&reopened, tenant.clone(), [0x45; 32])
            .unwrap_or_else(|error| panic!("serve older after reopen: {error:?}")),
        None
    );
    let inventory = evidence_inventory(&reopened, tenant.clone())
        .unwrap_or_else(|error| panic!("inventory: {error:?}"));
    assert_eq!(inventory.with_evidence, vec![record]);
    assert_eq!(inventory.without_evidence.len(), 1);
    assert_eq!(inventory.without_evidence[0].idempotency_key, [0x45; 32]);
    assert_eq!(inventory.without_evidence[0].activity_id, [0x12; 32]);
    assert_eq!(inventory.without_evidence[0].global_sequence, 101);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn startup_recovery_reconciles_evidenced_tenant_and_holds_tenant_without_evidence() {
    let root = directory("startup-recovery");
    let tenant_a = tenant();
    let tenant_b = TenantId::new("tenant-b").unwrap_or_else(|error| panic!("tenant: {error}"));
    let mut durable = Store::open(&root).unwrap_or_else(|error| panic!("store: {error}"));
    let evidenced = support::raw_receipt_at([0x11; 32], 0, 25, 100);
    store_served_receipt(&mut durable, &tenant_a, [0x44; 32], &evidenced, 100);
    persist_evidence(&mut durable, tenant_a.clone(), [0x44; 32], &evidenced)
        .unwrap_or_else(|error| panic!("persist evidence: {error:?}"));
    let unevidenced = support::raw_receipt_at([0x12; 32], 0, 10, 101);
    store_served_receipt(&mut durable, &tenant_b, [0x45; 32], &unevidenced, 101);
    hold_unknown(
        &mut durable,
        &UnknownReservation {
            tenant: tenant_b.clone(),
            id: [1; 32],
            amount: 300,
            expiry_sequence: 10,
            resolved: None,
        },
    )
    .unwrap_or_else(|error| panic!("hold unknown: {error:?}"));
    drop(durable);

    let mut reopened = Store::open(&root).unwrap_or_else(|error| panic!("reopen: {error}"));
    let verifier = support::evidence_verifier();
    let protocol = ProtocolBudgetState {
        evidence: support::raw_state_leaf(core_budget_record(500, 25), 99),
    };
    let inventory_a = evidence_inventory(&reopened, tenant_a.clone())
        .unwrap_or_else(|error| panic!("inventory a: {error:?}"));
    assert_eq!(inventory_a.with_evidence.len(), 1);
    assert!(inventory_a.without_evidence.is_empty());
    let recovery_a = recover_tenant_budget(
        &mut reopened,
        &tenant_a,
        &BudgetRecoveryRequest {
            budget_id: BUDGET_ID,
            protocol_budget: protocol.clone(),
            verifier: verifier.clone(),
            receipts_with_evidence: &inventory_a.with_evidence,
            receipts_without_evidence: &inventory_a.without_evidence,
            ceiling_maximum: 1_000,
            current_sequence: 1,
        },
    )
    .unwrap_or_else(|error| panic!("recover tenant a: {error:?}"));
    let accounting_a = recovery_a.recovered.budget_accounting;
    assert_eq!(accounting_a.protocol_consumed, Some(25));
    assert_eq!(accounting_a.receipt_consumed, 25);
    assert_eq!(accounting_a.held_unresolved, 0);
    assert_eq!(accounting_a.unresolved_count, 0);
    assert!(accounting_a.reconciled);
    assert!(recovery_a.recovered.queued_for_transmission.is_empty());
    assert!(recovery_a.recovered.awaiting_receipt_resolution.is_empty());
    assert!(matches!(
        recovery_a.recovered.require_write_ready(),
        Err(RecoveryError::WritesBlocked)
    ));
    assert!(matches!(
        recovery_a.admission,
        Err(RecoveryRefusal::WritesBlocked { budget_id, accounting })
            if budget_id == BUDGET_ID && accounting == accounting_a
    ));

    let inventory_b = evidence_inventory(&reopened, tenant_b.clone())
        .unwrap_or_else(|error| panic!("inventory b: {error:?}"));
    assert!(inventory_b.with_evidence.is_empty());
    assert_eq!(inventory_b.without_evidence.len(), 1);
    let recovery_b = recover_tenant_budget(
        &mut reopened,
        &tenant_b,
        &BudgetRecoveryRequest {
            budget_id: BUDGET_ID,
            protocol_budget: protocol.clone(),
            verifier: verifier.clone(),
            receipts_with_evidence: &inventory_b.with_evidence,
            receipts_without_evidence: &inventory_b.without_evidence,
            ceiling_maximum: 1_000,
            current_sequence: 1,
        },
    )
    .unwrap_or_else(|error| panic!("recover tenant b: {error:?}"));
    let accounting_b = recovery_b.recovered.budget_accounting;
    assert_eq!(accounting_b.protocol_consumed, Some(25));
    assert_eq!(accounting_b.receipt_consumed, 0);
    assert_eq!(accounting_b.held_unresolved, 300);
    assert_eq!(accounting_b.unresolved_count, 1);
    assert!(accounting_b.reconciled);
    assert!(matches!(
        recovery_b.admission,
        Err(RecoveryRefusal::EvidenceMissing { budget_id, count })
            if budget_id == BUDGET_ID && count == 1
    ));

    assert!(matches!(
        recover_tenant_budget(
            &mut reopened,
            &tenant_a,
            &BudgetRecoveryRequest {
                budget_id: [0x52; 32],
                protocol_budget: protocol.clone(),
                verifier: verifier.clone(),
                receipts_with_evidence: &inventory_a.with_evidence,
                receipts_without_evidence: &inventory_a.without_evidence,
                ceiling_maximum: 1_000,
                current_sequence: 1,
            },
        ),
        Err(RecoveryRefusal::BudgetState { budget_id, .. }) if budget_id == [0x52; 32]
    ));
    assert!(matches!(
        recover_tenant_budget(
            &mut reopened,
            &tenant_a,
            &BudgetRecoveryRequest {
                budget_id: BUDGET_ID,
                protocol_budget: ProtocolBudgetState {
                    evidence: support::corrupt_raw_state(&protocol.evidence, vec![1, 2, 3]),
                },
                verifier,
                receipts_with_evidence: &inventory_a.with_evidence,
                receipts_without_evidence: &inventory_a.without_evidence,
                ceiling_maximum: 1_000,
                current_sequence: 1,
            },
        ),
        Err(RecoveryRefusal::BudgetState { budget_id, .. }) if budget_id == BUDGET_ID
    ));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn module_witness_state_evidence_verifies_only_under_the_handshake_pin() {
    let pin = sequencer_pin();
    let record = core_budget_record(500, 25);
    let evidence = module_witness_evidence(&record, pin);
    assert!(evidence.proof().is_none());
    assert!(evidence.resulting_state_root().is_none());
    assert!(evidence.canonical_header().is_none());
    assert!(evidence.header_signature().is_none());
    assert!(evidence.proof_material().is_some());
    assert_eq!(evidence.canonical_state(), record.as_slice());

    let authority = EvidenceAuthority::pinned_to_handshake(3, 42, pin)
        .unwrap_or_else(|error| panic!("pinned authority: {error:?}"));
    let verified = authority
        .verify_state(&evidence)
        .unwrap_or_else(|error| panic!("verify module witness: {error:?}"));
    assert_eq!(verified.level(), VerificationLevel::STATE_PROVEN);
    assert_eq!(verified.observed_head_sequence(), 99);
    assert_eq!(verified.canonical_state(), record.as_slice());

    let tampered = RawStateEvidence::module_witness(
        core_budget_record(500, 26),
        BUDGET_MODULE_ID,
        budget_state_key(BUDGET_ID),
        module_proof_material(&budget_witness(&record), SEQUENCER_SEED, pin, 99),
        RootSelector::Latest,
        pin,
    );
    assert!(matches!(
        authority.verify_state(&tampered),
        Err(StateEvidenceError::Module(_))
    ));

    let wrong_selector = RawStateEvidence::module_witness(
        record.clone(),
        BUDGET_MODULE_ID,
        budget_state_key(BUDGET_ID),
        module_proof_material(&budget_witness(&record), SEQUENCER_SEED, pin, 99),
        RootSelector::Batch(7),
        pin,
    );
    assert!(matches!(
        authority.verify_state(&wrong_selector),
        Err(StateEvidenceError::Module(_))
    ));

    let other_pin = SigningKey::from_bytes(&[0x3b; 32])
        .verifying_key()
        .to_bytes();
    let unsigned_by_pin = module_witness_evidence(&record, other_pin);
    assert!(matches!(
        authority.verify_state(&unsigned_by_pin),
        Err(StateEvidenceError::Module(_))
    ));

    let other_authority = EvidenceAuthority::pinned_to_handshake(3, 42, other_pin)
        .unwrap_or_else(|error| panic!("other pinned authority: {error:?}"));
    assert_eq!(
        other_authority.verify_state(&evidence),
        Err(StateEvidenceError::Policy(
            VerifierPolicyError::HandshakeKey
        ))
    );

    let protocol_two = EvidenceAuthority::pinned_to_handshake(2, 42, pin)
        .unwrap_or_else(|error| panic!("protocol-2 pinned authority: {error:?}"));
    assert!(matches!(
        protocol_two.verify_state(&evidence),
        Err(StateEvidenceError::Module(_))
    ));

    assert_eq!(
        EvidenceAuthority::pinned_to_handshake(0, 42, pin).err(),
        Some(VerifierPolicyError::EmptyPolicy)
    );
    assert_eq!(
        EvidenceAuthority::pinned_to_handshake(3, 0, pin).err(),
        Some(VerifierPolicyError::EmptyPolicy)
    );
    assert_eq!(
        EvidenceAuthority::pinned_to_handshake(3, 42, [0; 32]).err(),
        Some(VerifierPolicyError::InvalidAuthorization)
    );
}

#[test]
fn startup_recovery_accepts_module_witness_budget_state() {
    let root = directory("module-witness-recovery");
    let tenant_c = TenantId::new("tenant-c").unwrap_or_else(|error| panic!("tenant: {error}"));
    let mut durable = Store::open(&root).unwrap_or_else(|error| panic!("store: {error}"));
    let pin = sequencer_pin();
    let verifier = EvidenceAuthority::pinned_to_handshake(3, 42, pin)
        .unwrap_or_else(|error| panic!("pinned authority: {error:?}"));
    let recovery = recover_tenant_budget(
        &mut durable,
        &tenant_c,
        &BudgetRecoveryRequest {
            budget_id: BUDGET_ID,
            protocol_budget: ProtocolBudgetState {
                evidence: module_witness_evidence(&core_budget_record(500, 0), pin),
            },
            verifier,
            receipts_with_evidence: &[],
            receipts_without_evidence: &[],
            ceiling_maximum: 1_000,
            current_sequence: 1,
        },
    )
    .unwrap_or_else(|error| panic!("recover tenant c: {error:?}"));
    let accounting = recovery.recovered.budget_accounting;
    assert_eq!(accounting.protocol_consumed, Some(0));
    assert_eq!(accounting.receipt_consumed, 0);
    assert_eq!(accounting.held_unresolved, 0);
    assert!(accounting.reconciled);
    assert!(matches!(
        recovery.admission,
        Err(RecoveryRefusal::WritesBlocked { budget_id, .. }) if budget_id == BUDGET_ID
    ));
    let _ = std::fs::remove_dir_all(&root);
}

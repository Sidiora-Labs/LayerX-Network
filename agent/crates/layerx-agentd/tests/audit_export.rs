use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use ed25519_dalek::{Signer as _, SigningKey};
use layerx_agentd::audit::{
    export, protect_payload, record, redact, review, ChainError, Coverage, DataClass, Decision,
    Entry, EventClass, EvidenceStore, ExportError, Log, OutputSurface, PayloadEvidence, Query,
    ReviewError,
};
use layerx_agentd::identity::ProtocolAuthority;
use layerx_agentd::store::TenantId;
use layerx_agentd::tenant::{Config, RedactionPolicy, Retention};
use layerx_proof::checkpoint::SettlementDomain;
use layerx_proof::export::{InclusionFact, InclusionKind, OfflineExport, ReceiptFact};
use layerx_proof::inclusion::SequencerAuthorization;
use layerx_proof::merkle::build_proof;
use layerx_proof::receipt::{verify_outcome, AuthorizedBatch};
use layerx_types::ids::{ActivityId, Did, IdempotencyKey};
use layerx_types::verify::VerificationLevel;
use layerx_wire::encode::Encoder;
use layerx_wire::hash::{batch_header_digest, receipt_digest};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

fn settlement_domain() -> SettlementDomain {
    SettlementDomain::new(31_337, [0x55; 20])
}

fn root(label: &str) -> PathBuf {
    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "layerx-audit-export-{label}-{}-{sequence}",
        std::process::id()
    ))
}

fn tenant(value: &str) -> TenantId {
    TenantId::new(value).unwrap_or_else(|error| panic!("tenant: {error}"))
}

fn agent(value: &[u8]) -> Did {
    Did::new(value).unwrap_or_else(|error| panic!("agent DID: {error:?}"))
}

fn config(audit: u64) -> Config {
    Config {
        tenant: tenant("tenant-a"),
        policy_version: "policy-v9".to_owned(),
        redaction: RedactionPolicy::Standard,
        retention: Retention {
            events: 100,
            audit,
            receipts: 100,
        },
        verification_default: VerificationLevel::STATE_PROVEN,
        approval_required_for: BTreeSet::new(),
    }
}

fn entry(
    class: EventClass,
    observed_at_ms: u64,
    agent: Did,
    decision: Decision,
    reason: &[u8],
    receipt_id: Option<[u8; 32]>,
    submitted_bytes: Option<PayloadEvidence>,
) -> Entry {
    let config = config(100);
    let reason = redact(
        &config,
        &config.tenant,
        OutputSurface::Audit,
        DataClass::PublicText,
        reason,
        10,
    )
    .unwrap_or_else(|error| panic!("reason redaction: {error}"))
    .value;
    Entry {
        class,
        observed_at_ms,
        tenant: config.tenant,
        agent,
        session: None,
        capability: None,
        policy_version: "policy-v9".to_owned(),
        request_id: [3; 32],
        idempotency_key: Some(IdempotencyKey::new([4; 32])),
        decision,
        reason,
        resulting_activity_id: receipt_id.map(|_| ActivityId::new([5; 32])),
        verification_level: if receipt_id.is_some() {
            VerificationLevel::STATE_PROVEN
        } else {
            VerificationLevel::UNVERIFIED
        },
        protocol_authority: (class == EventClass::Submission)
            .then_some(ProtocolAuthority::CapabilityGrant([6; 32])),
        submitted_bytes,
        receipt_id,
    }
}

fn receipt_bytes(signature: Option<[u8; 64]>) -> Vec<u8> {
    let mut encoder = Encoder::new(4096);
    assert_eq!(
        encoder.structure_header_version(0x5201, layerx_wire::limits::PROTOCOL_VERSION),
        Ok(())
    );
    assert_eq!(encoder.u16(layerx_wire::limits::PROTOCOL_VERSION), Ok(()));
    assert_eq!(encoder.bytes(&[1; 32], 32), Ok(()));
    assert_eq!(encoder.u64(9), Ok(()));
    assert_eq!(encoder.bytes(&[2; 32], 32), Ok(()));
    assert_eq!(encoder.bytes(&[3; 32], 32), Ok(()));
    assert_eq!(encoder.bytes(&[8; 32], 32), Ok(()));
    assert_eq!(encoder.i32(0), Ok(()));
    assert_eq!(encoder.sequence_length(0, 512), Ok(()));
    assert_eq!(encoder.u128(1), Ok(()));
    assert_eq!(encoder.bytes(&[4; 32], 32), Ok(()));
    assert_eq!(encoder.u16(1), Ok(()));
    assert_eq!(encoder.u32(1), Ok(()));
    assert_eq!(encoder.u32(1), Ok(()));
    assert_eq!(encoder.u8(1), Ok(()));
    assert_eq!(encoder.bytes(&[5; 32], 32), Ok(()));
    assert_eq!(encoder.u128(25), Ok(()));
    assert_eq!(encoder.bytes(&[6; 32], 32), Ok(()));
    assert_eq!(encoder.u128(100), Ok(()));
    assert_eq!(encoder.u128(75), Ok(()));
    assert_eq!(encoder.u64(1), Ok(()));
    assert_eq!(encoder.bytes(&[7; 32], 32), Ok(()));
    assert_eq!(encoder.u128(10), Ok(()));
    assert_eq!(encoder.u128(35), Ok(()));
    assert_eq!(encoder.bytes(&[9; 32], 32), Ok(()));
    assert_eq!(encoder.bytes(&[10; 32], 32), Ok(()));
    assert_eq!(encoder.bytes(&[11; 32], 32), Ok(()));
    assert_eq!(encoder.u64(1_000), Ok(()));
    assert_eq!(encoder.u8(u8::from(signature.is_some())), Ok(()));
    if let Some(value) = signature {
        assert_eq!(encoder.bytes(&value, 64), Ok(()));
    }
    encoder.finish()
}

fn header_bytes(state_root: [u8; 32], activity_root: [u8; 32], sequencer: [u8; 32]) -> Vec<u8> {
    let mut encoder = Encoder::new(354);
    assert_eq!(
        encoder.structure_header_version(0x1701, layerx_wire::limits::PROTOCOL_VERSION),
        Ok(())
    );
    assert_eq!(encoder.u8(15), Ok(()));
    for field in 1..=15 {
        assert_eq!(encoder.tag(field, 15), Ok(()));
        match field {
            1 => assert_eq!(encoder.u16(layerx_wire::limits::PROTOCOL_VERSION), Ok(())),
            2 => assert_eq!(encoder.u32(42), Ok(())),
            3 => assert_eq!(encoder.u64(7), Ok(())),
            4 => assert_eq!(encoder.u64(8), Ok(())),
            5 => assert_eq!(encoder.u64(11), Ok(())),
            6 => assert_eq!(encoder.u64(19), Ok(())),
            7 => assert_eq!(encoder.bytes(&[7; 32], 32), Ok(())),
            8 => assert_eq!(encoder.bytes(&state_root, 32), Ok(())),
            9 => assert_eq!(encoder.bytes(&activity_root, 32), Ok(())),
            10 => assert_eq!(encoder.bytes(&[10; 32], 32), Ok(())),
            11 => assert_eq!(encoder.bytes(&[11; 32], 32), Ok(())),
            12 => assert_eq!(encoder.bytes(&[12; 32], 32), Ok(())),
            13 => assert_eq!(encoder.bytes(&[13; 32], 32), Ok(())),
            14 => assert_eq!(encoder.u64(1_000), Ok(())),
            15 => assert_eq!(encoder.bytes(&sequencer, 32), Ok(())),
            _ => panic!("unreachable header field"),
        }
    }
    encoder.finish()
}

fn protocol_evidence() -> ([u8; 32], OfflineExport) {
    let receipt_key = SigningKey::from_bytes(&[3; 32]);
    let unsigned = receipt_bytes(None);
    let signing_digest =
        receipt_digest(&unsigned).unwrap_or_else(|error| panic!("receipt digest: {error:?}"));
    let canonical_receipt_bytes = receipt_bytes(Some(receipt_key.sign(&signing_digest).to_bytes()));
    let authorised_batch = AuthorizedBatch::new(
        [4; 32],
        [5; 32],
        [2; 32],
        [3; 32],
        receipt_key.verifying_key().to_bytes(),
    );
    let receipt_id = verify_outcome(&canonical_receipt_bytes, &authorised_batch)
        .unwrap_or_else(|error| panic!("receipt verification: {error:?}"))
        .evidence()
        .receipt_digest()
        .unwrap_or_else(|| panic!("verified receipt digest absent"));

    let leaf = b"state-leaf".to_vec();
    let (proof, state_root) =
        build_proof(&[leaf.as_slice()], 0).unwrap_or_else(|error| panic!("state proof: {error:?}"));
    let (_, activity_root) = build_proof(&[b"activity-leaf".as_slice()], 0)
        .unwrap_or_else(|error| panic!("activity proof: {error:?}"));
    let sequencer_key = SigningKey::from_bytes(&[7; 32]);
    let sequencer = sequencer_key.verifying_key().to_bytes();
    let header = header_bytes(state_root, activity_root, sequencer);
    let header_digest =
        batch_header_digest(&header).unwrap_or_else(|error| panic!("header digest: {error:?}"));

    (
        receipt_id,
        OfflineExport {
            receipts: vec![ReceiptFact {
                statement: "core receipt proves the terminal activity outcome".to_owned(),
                canonical_receipt_bytes,
                authorised_batch,
                expected_receipt_digest: receipt_id,
            }],
            inclusions: vec![InclusionFact {
                statement: "state leaf is included under the signed header root".to_owned(),
                kind: InclusionKind::State,
                canonical_leaf_bytes: leaf,
                proof,
                named_root: state_root,
                canonical_header_bytes: header,
                header_signature: sequencer_key.sign(&header_digest).to_bytes(),
                sequencer_authorization: SequencerAuthorization::new(sequencer, sequencer, 8, 8),
            }],
            checkpoints: Vec::new(),
            derived_aggregates: Vec::new(),
        },
    )
}

#[test]
fn tenant_agent_and_time_slice_carries_real_receipt_and_proof_evidence() {
    let root = root("evidence");
    let config = config(100);
    let mut log = Log::open(&root, &config.tenant).unwrap_or_else(|error| panic!("open: {error}"));
    let path = log.path().to_path_buf();
    let mut coverage = Coverage::default();
    let (receipt_id, evidence) = protocol_evidence();
    let records = [
        entry(
            EventClass::TerminalOutcome,
            100,
            agent(b"did:layerx:agent-a"),
            Decision::Executed,
            b"core receipt verified",
            Some(receipt_id),
            None,
        ),
        entry(
            EventClass::PolicyDecision,
            120,
            agent(b"did:layerx:agent-b"),
            Decision::Allowed,
            b"policy allowed",
            None,
            None,
        ),
        entry(
            EventClass::MutationAttempt,
            200,
            agent(b"did:layerx:agent-a"),
            Decision::Failed,
            b"availability range unavailable",
            None,
            None,
        ),
    ];
    for item in &records {
        record(&mut log, &mut coverage, item, || ())
            .unwrap_or_else(|error| panic!("record: {error}"));
    }
    drop(log);

    let mut evidence_store = EvidenceStore::new(config.tenant.clone());
    evidence_store.insert(receipt_id, evidence);
    let query = Query {
        tenant: config.tenant.clone(),
        agent: Some(agent(b"did:layerx:agent-a")),
        from_observed_at_ms: Some(90),
        through_observed_at_ms: Some(150),
    };
    let exported = export(&path, &config, query, &evidence_store, settlement_domain())
        .unwrap_or_else(|error| panic!("export: {error}"));
    assert_eq!(exported.entries.len(), 1);
    assert_eq!(exported.entries[0].entry.observed_at_ms, 100);
    assert_eq!(exported.referenced_evidence.len(), 1);
    assert_eq!(exported.chain.links.len(), 3);
    assert!(exported.chain.links[0].canonical_entry_bytes.is_some());
    assert!(exported.chain.links[1].canonical_entry_bytes.is_none());
    let report =
        review(&exported, settlement_domain()).unwrap_or_else(|error| panic!("review: {error}"));
    assert_eq!(report.exported_entries, 1);
    assert_eq!(report.verified_receipts, 1);
    assert_eq!(report.verified_inclusions, 1);

    let mut altered = exported.clone();
    altered.referenced_evidence[0].protocol_facts.receipts[0].canonical_receipt_bytes[10] ^= 1;
    assert!(matches!(
        review(&altered, settlement_domain()),
        Err(ReviewError::Evidence { .. })
    ));

    let wrong_tenant = Query {
        tenant: tenant("tenant-b"),
        agent: None,
        from_observed_at_ms: None,
        through_observed_at_ms: None,
    };
    assert_eq!(
        export(
            &path,
            &config,
            wrong_tenant,
            &evidence_store,
            settlement_domain()
        ),
        Err(ExportError::WrongTenant)
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn retention_boundary_redacts_old_payload_and_keeps_availability_failure() {
    let root = root("retention");
    let config = config(1);
    let mut log = Log::open(&root, &config.tenant).unwrap_or_else(|error| panic!("open: {error}"));
    let path = log.path().to_path_buf();
    let mut coverage = Coverage::default();
    let old_bytes = b"old-canonical-signed-activity";
    let recent_bytes = b"recent-canonical-signed-activity";
    let records = [
        entry(
            EventClass::Submission,
            100,
            agent(b"did:layerx:agent-a"),
            Decision::Submitted,
            b"old submission",
            None,
            Some(protect_payload(&config, 0, 0, old_bytes)),
        ),
        entry(
            EventClass::MutationAttempt,
            150,
            agent(b"did:layerx:agent-a"),
            Decision::Failed,
            b"availability range unavailable",
            None,
            None,
        ),
        entry(
            EventClass::Submission,
            200,
            agent(b"did:layerx:agent-a"),
            Decision::Submitted,
            b"recent submission",
            None,
            Some(protect_payload(&config, 2, 2, recent_bytes)),
        ),
    ];
    for item in &records {
        record(&mut log, &mut coverage, item, || ())
            .unwrap_or_else(|error| panic!("record: {error}"));
    }
    drop(log);

    let evidence_store = EvidenceStore::new(config.tenant.clone());
    let exported = export(
        &path,
        &config,
        Query {
            tenant: config.tenant.clone(),
            agent: None,
            from_observed_at_ms: Some(100),
            through_observed_at_ms: Some(200),
        },
        &evidence_store,
        settlement_domain(),
    )
    .unwrap_or_else(|error| panic!("export: {error}"));
    assert_eq!(
        exported.entries[0].entry.submitted_bytes,
        Some(PayloadEvidence::Redacted)
    );
    assert!(matches!(
        exported.entries[2].entry.submitted_bytes,
        Some(PayloadEvidence::Digest(_))
    ));
    assert_eq!(exported.entries[1].entry.decision, Decision::Failed);
    assert_eq!(
        exported.entries[1].entry.reason.as_str(),
        "availability range unavailable"
    );
    assert!(exported.chain.links[0].canonical_entry_bytes.is_none());
    assert!(exported.chain.links[2].canonical_entry_bytes.is_some());
    let report =
        review(&exported, settlement_domain()).unwrap_or_else(|error| panic!("review: {error}"));
    assert_eq!(report.failed_records, 1);

    let mut excised = exported;
    excised.chain.links[1].previous_hash[0] ^= 1;
    assert_eq!(
        review(&excised, settlement_domain()),
        Err(ReviewError::Chain(ChainError::Link))
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn missing_referenced_evidence_is_an_explicit_availability_error() {
    let root = root("missing-evidence");
    let config = config(100);
    let mut log = Log::open(&root, &config.tenant).unwrap_or_else(|error| panic!("open: {error}"));
    let path = log.path().to_path_buf();
    let mut coverage = Coverage::default();
    let (receipt_id, _) = protocol_evidence();
    record(
        &mut log,
        &mut coverage,
        &entry(
            EventClass::TerminalOutcome,
            100,
            agent(b"did:layerx:agent-a"),
            Decision::Executed,
            b"terminal receipt",
            Some(receipt_id),
            None,
        ),
        || (),
    )
    .unwrap_or_else(|error| panic!("record: {error}"));
    drop(log);

    let evidence_store = EvidenceStore::new(config.tenant.clone());
    let result = export(
        &path,
        &config,
        Query {
            tenant: config.tenant.clone(),
            agent: None,
            from_observed_at_ms: None,
            through_observed_at_ms: None,
        },
        &evidence_store,
        settlement_domain(),
    );
    assert_eq!(result, Err(ExportError::EvidenceUnavailable { receipt_id }));
    let _ = fs::remove_dir_all(root);
}

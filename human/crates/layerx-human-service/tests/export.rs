mod support;

use std::fmt::Write as _;

use ed25519_dalek::{Signer as _, SigningKey};
use layerx_agent_api::subscription::{Cursor, EventDelivery, EventIdentity, ReceiptReference};
use layerx_agent_api::track::ReceiptRef;
use layerx_agent_api::verify::Level;
use layerx_agent_api::Sequence;
use layerx_agentd::export::build as build_agent_export;
use layerx_human_service::activity::{
    ActivityKind, AgentActivity, EvidenceExport, ExportError, Feed, FilterDraft, PendingStatus,
    VerifiedStatus,
};
use layerx_human_service::audit::{
    verify_export as verify_audit_export, AuditChain, AuditEvent, SecurityChangeKind,
    StepUpEvidence,
};
use layerx_human_service::notify::ActivityEntryId;
use layerx_human_service::store::{EvidenceRef, PrincipalScope, Table};
use layerx_human_service::trace::TraceId;
use layerx_intents::vectors::{
    batch_header, batch_header_signing_digest, receipt as receipt_vector, receipt_signing_digest,
};
use layerx_proof::checkpoint::SettlementDomain;
use layerx_proof::export::{InclusionFact, InclusionKind, OfflineExport, ReceiptFact};
use layerx_proof::inclusion::SequencerAuthorization;
use layerx_proof::merkle::{build_proof, encode_proof};
use layerx_proof::receipt::{verify_outcome, AuthorizedBatch};
use sha2::{Digest as _, Sha256};

fn result<T, E: std::fmt::Debug>(value: Result<T, E>, label: &str) -> T {
    value.unwrap_or_else(|error| panic!("{label}: {error:?}"))
}

fn settlement_domain() -> SettlementDomain {
    SettlementDomain::new(31_337, [0x55; 20])
}

fn protocol_artifact() -> OfflineExport {
    let receipt_key = SigningKey::from_bytes(&[3; 32]);
    let unsigned = result(receipt_vector(None), "unsigned receipt");
    let signing_digest = result(receipt_signing_digest(&unsigned), "receipt digest");
    let canonical_receipt = result(
        receipt_vector(Some(receipt_key.sign(&signing_digest).to_bytes())),
        "signed receipt",
    );
    let authorised_batch = AuthorizedBatch::new(
        [4; 32],
        [5; 32],
        [2; 32],
        [3; 32],
        receipt_key.verifying_key().to_bytes(),
    );
    let verified = result(
        verify_outcome(&canonical_receipt, &authorised_batch),
        "receipt verification",
    );
    let expected_receipt_digest = verified
        .evidence()
        .receipt_digest()
        .unwrap_or_else(|| panic!("receipt digest missing"));

    let state_leaf = b"canonical-state-leaf".to_vec();
    let activity_leaf = b"canonical-activity-leaf".to_vec();
    let (proof, state_root) = result(
        build_proof(&[state_leaf.as_slice()], 0),
        "state inclusion proof",
    );
    let (_, activity_root) = result(build_proof(&[activity_leaf.as_slice()], 0), "activity root");
    let sequencer_key = SigningKey::from_bytes(&[7; 32]);
    let sequencer_id = sequencer_key.verifying_key().to_bytes();
    let canonical_header = result(
        batch_header(state_root, activity_root, sequencer_id),
        "canonical header",
    );
    let header_digest = result(
        batch_header_signing_digest(&canonical_header),
        "header digest",
    );

    OfflineExport {
        receipts: vec![ReceiptFact {
            statement: "selected movement has an authorised canonical receipt".to_owned(),
            canonical_receipt_bytes: canonical_receipt,
            authorised_batch,
            expected_receipt_digest,
        }],
        inclusions: vec![InclusionFact {
            statement: "the referenced state leaf is included under the signed root".to_owned(),
            kind: InclusionKind::State,
            canonical_leaf_bytes: state_leaf,
            proof,
            named_root: state_root,
            canonical_header_bytes: canonical_header,
            header_signature: sequencer_key.sign(&header_digest).to_bytes(),
            sequencer_authorization: SequencerAuthorization::new(sequencer_id, sequencer_id, 8, 8),
        }],
        checkpoints: Vec::new(),
        derived_aggregates: Vec::new(),
    }
}

fn receipt_reference(artifact: &OfflineExport) -> String {
    let digest = Sha256::digest(&artifact.receipts[0].canonical_receipt_bytes);
    let mut reference = String::with_capacity(64);
    for byte in digest {
        let _ = write!(reference, "{byte:02x}");
    }
    reference
}

fn activity_id(value: &str) -> ActivityEntryId {
    result(ActivityEntryId::new(value), "activity id")
}

fn record_export_activity(
    scope: &mut PrincipalScope<'_>,
    artifact: &OfflineExport,
    reference: &str,
    movement_id: &ActivityEntryId,
) {
    let delivery = result(
        EventDelivery::new(
            EventIdentity::new([0x41; 32]),
            artifact.receipts[0].canonical_receipt_bytes.clone(),
            Cursor(Sequence(1)),
            ReceiptReference::Verified {
                receipt_ref: result(ReceiptRef::new(reference), "receipt reference"),
                verification_level: Level::SequencerSigned,
            },
        ),
        "event delivery",
    );
    let movement = result(
        AgentActivity::new(
            movement_id.clone(),
            ActivityKind::Movement,
            Some("did:layerx:private-agent-label".to_owned()),
            10,
            PendingStatus::Processing,
            VerifiedStatus::Done,
        ),
        "activity descriptor",
    );
    result(
        Feed::record_agent_event(scope, &movement, &delivery, 20),
        "activity projection",
    );
    let unrelated_delivery = result(
        EventDelivery::new(
            EventIdentity::new([0x42; 32]),
            b"unrelated approval event".to_vec(),
            Cursor(Sequence(2)),
            ReceiptReference::None,
        ),
        "unrelated delivery",
    );
    let unrelated = result(
        AgentActivity::new(
            activity_id("act_approvalexport1"),
            ActivityKind::Approval,
            Some("did:layerx:private-agent-label".to_owned()),
            11,
            PendingStatus::WaitingForYou,
            VerifiedStatus::Done,
        ),
        "unrelated descriptor",
    );
    result(
        Feed::record_agent_event(scope, &unrelated, &unrelated_delivery, 21),
        "unrelated projection",
    );
}

#[test]
fn filtered_statement_and_evidence_are_offline_verifiable_and_redacted() {
    let artifact = protocol_artifact();
    let reference = receipt_reference(&artifact);

    let root = support::directory("activity-export");
    let tenancy = support::tenancy(&[("alice", "tenant-a"), ("bob", "tenant-b")]);
    let (mut store, _) =
        support::install_and_open(&root, &tenancy, support::retention_uniform(100));
    let alice = support::principal("alice");
    let movement_id = activity_id("act_movementexport1");
    let mut scope = result(store.principal(&alice), "alice scope");
    record_export_activity(&mut scope, &artifact, &reference, &movement_id);
    let feed = result(Feed::new(5), "feed");
    let filters = result(
        Feed::apply_filters(FilterDraft::new().with_kinds([ActivityKind::Movement])),
        "filters",
    );

    let statement = result(
        result(EvidenceExport::new(feed, 64 * 1024), "statement exporter")
            .statement(&scope, &filters, 21, 2),
        "statement",
    );
    let csv = std::str::from_utf8(statement.content())
        .unwrap_or_else(|error| panic!("statement UTF-8: {error}"));
    assert_eq!(statement.entries(), 1);
    assert!(csv.contains("act_movementexport1,movement,Done"));
    assert!(csv.contains(&reference));
    assert!(!csv.contains("private-agent-label"));
    assert!(!csv.contains("canonical-state-leaf"));

    let bundle = result(
        result(EvidenceExport::new(feed, 64 * 1024), "evidence exporter").evidence(
            &scope,
            &filters,
            std::slice::from_ref(&movement_id),
            vec![artifact.clone()],
            settlement_domain(),
            21,
            2,
        ),
        "evidence bundle",
    );
    let report = result(
        bundle.verify(settlement_domain()),
        "third-party bundle verification",
    );
    let agent_verified = result(
        build_agent_export(bundle.protocol_evidence()[0].clone(), settlement_domain()),
        "agent offline verification of bundle",
    );
    assert_eq!(report.entries(), 1);
    assert_eq!(report.verified_receipts(), 1);
    assert_eq!(report.verified_inclusions(), 1);
    assert_eq!(agent_verified.local_verification.verified_receipts, 1);
    assert_eq!(agent_verified.local_verification.verified_inclusions, 1);
    assert_eq!(bundle.protocol_evidence(), std::slice::from_ref(&artifact));
    assert!(matches!(
        result(EvidenceExport::new(feed, 64), "bounded exporter").evidence(
            &scope,
            &filters,
            std::slice::from_ref(&movement_id),
            vec![artifact.clone()],
            settlement_domain(),
            21,
            2,
        ),
        Err(ExportError::SizeBoundExceeded { maximum: 64 })
    ));

    drop(scope);
    let bob = support::principal("bob");
    let bob_scope = result(store.principal(&bob), "bob scope");
    assert!(matches!(
        result(EvidenceExport::new(feed, 64 * 1024), "bob exporter").evidence(
            &bob_scope,
            &filters,
            std::slice::from_ref(&movement_id),
            vec![artifact.clone()],
            settlement_domain(),
            21,
            0,
        ),
        Err(ExportError::EntryNotFound)
    ));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn audit_uses_the_same_bounded_bundle_and_keeps_referenced_bytes() {
    let artifact = protocol_artifact();
    let root = support::directory("audit-evidence-export");
    let tenancy = support::tenancy(&[("alice", "tenant-a")]);
    let (mut store, _) =
        support::install_and_open(&root, &tenancy, support::retention_uniform(100));
    let mut scope = result(store.principal(&support::principal("alice")), "alice scope");
    let receipt_key = support::row_key("canonical-receipt");
    let proof_key = support::row_key("canonical-inclusion-proof");
    result(
        scope.put(
            Table::Journeys,
            receipt_key.clone(),
            1,
            artifact.receipts[0].canonical_receipt_bytes.clone(),
        ),
        "store receipt",
    );
    result(
        scope.put(
            Table::Journeys,
            proof_key.clone(),
            1,
            encode_proof(&artifact.inclusions[0].proof),
        ),
        "store proof leaf",
    );
    let mut chain = result(AuditChain::open(&scope), "audit chain");
    result(
        chain.append(
            &mut scope,
            2,
            &TraceId::mint([8; 16]),
            &AuditEvent::SecurityChange {
                change: SecurityChangeKind::KeyRotation,
                step_up: StepUpEvidence::Fresh {
                    ceremony_digest: [9; 32],
                },
            },
            &[
                EvidenceRef::new(Table::Journeys, receipt_key),
                EvidenceRef::new(Table::Journeys, proof_key),
            ],
        ),
        "audit append",
    );
    let feed = result(Feed::new(5), "feed");
    let bundle = result(
        result(EvidenceExport::new(feed, 64 * 1024), "audit exporter").audit(
            &scope,
            &chain,
            settlement_domain(),
        ),
        "audit evidence bundle",
    );
    let report = result(
        bundle.verify(settlement_domain()),
        "offline audit bundle verification",
    );
    assert_eq!(report.audit_entries(), 1);
    assert_eq!(report.entries(), 0);
    let audit_bytes = bundle
        .audit_export()
        .unwrap_or_else(|| panic!("audit export missing"));
    let audit_report = result(
        verify_audit_export(audit_bytes),
        "independent audit verifier",
    );
    assert_eq!(audit_report.principal().as_str(), "alice");
    assert_eq!(audit_report.evidence_rows(), 2);
    assert!(matches!(
        result(EvidenceExport::new(feed, 64), "bounded audit exporter").audit(
            &scope,
            &chain,
            settlement_domain(),
        ),
        Err(ExportError::SizeBoundExceeded { maximum: 64 })
    ));
    let _ = std::fs::remove_dir_all(root);
}

mod support;

use std::fmt::Write as _;

use ed25519_dalek::{Signer as _, SigningKey};
use layerx_agent_api::subscription::{Cursor, EventDelivery, EventIdentity, ReceiptReference};
use layerx_agent_api::track::ReceiptRef;
use layerx_agent_api::verify::Level;
use layerx_agent_api::Sequence;
use layerx_agentd::export::build as build_agent_export;
use layerx_human_service::activity::{
    verification_status, ActivityKind, AgentActivity, EvidenceBundle, EvidenceExport, ExportError,
    Feed, FilterDraft, PendingStatus, ReceiptAuthority, UnverifiedReason, VerifiedStatus,
    VerifyError,
};
use layerx_human_service::audit::{
    verify_export as verify_audit_export, AuditChain, AuditEvent, SecurityChangeKind,
    StepUpEvidence,
};
use layerx_human_service::notify::ActivityEntryId;
use layerx_human_service::store::{EvidenceRef, PrincipalScope, RowKey, Table};
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

fn receipt_artifact() -> OfflineExport {
    OfflineExport {
        inclusions: Vec::new(),
        ..protocol_artifact()
    }
}

fn hex_digest(digest: &[u8]) -> String {
    let mut reference = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(reference, "{byte:02x}");
    }
    reference
}

fn receipt_reference(artifact: &OfflineExport) -> String {
    hex_digest(&Sha256::digest(
        &artifact.receipts[0].canonical_receipt_bytes,
    ))
}

fn receipt_authority(artifact: &OfflineExport) -> ReceiptAuthority {
    let mut authority = ReceiptAuthority::default();
    authority.insert(
        receipt_reference(artifact),
        artifact.receipts[0].authorised_batch,
    );
    authority
}

fn evidence_row_key(digest: [u8; 32]) -> RowKey {
    result(
        RowKey::new(format!("activity-evidence-{}", hex_digest(&digest))),
        "evidence row key",
    )
}

fn unique_position(bytes: &[u8], needle: &[u8]) -> usize {
    let positions = bytes
        .windows(needle.len())
        .enumerate()
        .filter(|(_, window)| *window == needle)
        .map(|(position, _)| position)
        .collect::<Vec<_>>();
    assert_eq!(positions.len(), 1, "expected exactly one occurrence");
    positions[0]
}

fn cache_bundle(
    scope: &mut PrincipalScope<'_>,
    bytes: Vec<u8>,
    written_at: u64,
) -> ([u8; 32], EvidenceBundle) {
    let digest: [u8; 32] = Sha256::digest(&bytes).into();
    result(
        scope.put(Table::Cache, evidence_row_key(digest), written_at, bytes),
        "evidence cache row",
    );
    let row = scope
        .get(Table::Cache, &evidence_row_key(digest))
        .unwrap_or_else(|| panic!("evidence cache row missing"));
    (
        digest,
        result(EvidenceBundle::decode(row.bytes()), "cached bundle decode"),
    )
}

fn export_bundle(
    scope: &PrincipalScope<'_>,
    artifact: &OfflineExport,
    movement_id: &ActivityEntryId,
    domain: SettlementDomain,
) -> EvidenceBundle {
    let feed = result(Feed::new(5), "feed");
    let filters = result(
        Feed::apply_filters(FilterDraft::new().with_kinds([ActivityKind::Movement])),
        "filters",
    );
    result(
        result(EvidenceExport::new(feed, 64 * 1024), "evidence exporter").evidence(
            scope,
            &filters,
            std::slice::from_ref(movement_id),
            vec![artifact.clone()],
            domain,
            &receipt_authority(artifact),
            21,
            2,
        ),
        "evidence bundle",
    )
    .0
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

    let (bundle, report) = result(
        result(EvidenceExport::new(feed, 64 * 1024), "evidence exporter").evidence(
            &scope,
            &filters,
            std::slice::from_ref(&movement_id),
            vec![artifact.clone()],
            settlement_domain(),
            &receipt_authority(&artifact),
            21,
            2,
        ),
        "evidence bundle",
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
            &receipt_authority(&artifact),
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
            &receipt_authority(&artifact),
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
    let digest = result(bundle.digest(), "audit bundle digest");
    let report = result(
        bundle.verify(
            digest,
            scope.principal(),
            settlement_domain(),
            &ReceiptAuthority::default(),
        ),
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

#[test]
#[allow(clippy::too_many_lines)]
fn cached_bundle_rows_are_labelled_only_by_the_verifier() {
    let artifact = receipt_artifact();
    let reference = receipt_reference(&artifact);
    let movement_id = activity_id("act_movementexport1");
    let root = support::directory("activity-evidence-rows");
    let tenancy = support::tenancy(&[("alice", "tenant-a"), ("bob", "tenant-b")]);
    let (mut store, _) =
        support::install_and_open(&root, &tenancy, support::retention_uniform(100));
    let alice = support::principal("alice");
    let bob = support::principal("bob");
    let foreign_domain = SettlementDomain::new(1, [0x66; 20]);
    let authority = receipt_authority(&artifact);

    let bob_bytes = {
        let mut bob_scope = result(store.principal(&bob), "bob scope");
        record_export_activity(&mut bob_scope, &artifact, &reference, &movement_id);
        result(
            export_bundle(&bob_scope, &artifact, &movement_id, settlement_domain()).encode(),
            "bob bundle bytes",
        )
    };
    let mut scope = result(store.principal(&alice), "alice scope");
    record_export_activity(&mut scope, &artifact, &reference, &movement_id);
    let genuine = export_bundle(&scope, &artifact, &movement_id, settlement_domain());
    let genuine_bytes = result(genuine.encode(), "genuine bundle bytes");
    let foreign_domain_bytes = result(
        export_bundle(&scope, &artifact, &movement_id, foreign_domain).encode(),
        "foreign-domain bundle bytes",
    );

    let (digest, cached) = cache_bundle(&mut scope, genuine_bytes.clone(), 30);
    assert_eq!(digest, result(genuine.digest(), "genuine digest"));
    let status = verification_status(cached.verify(
        digest,
        scope.principal(),
        settlement_domain(),
        &authority,
    ));
    assert!(status.is_receipt_verified());
    assert!(!status.is_unavailable());
    assert_eq!(status.label(), "receipt-verified");
    assert_eq!(status.unverified_reason(), None);
    assert_eq!(
        status.report().map(|report| report.verified_receipts()),
        Some(1)
    );

    let feed = result(Feed::new(5), "feed");
    let loaded = result(cached.receipt_authority(feed, &scope), "feed authority");
    let status =
        verification_status(cached.verify(digest, scope.principal(), settlement_domain(), &loaded));
    assert!(status.is_unavailable());
    assert!(!status.is_receipt_verified());
    assert_eq!(status.label(), "unavailable");
    assert_eq!(status.unverified_reason(), None);
    assert!(matches!(
        cached.verify(
            digest,
            scope.principal(),
            settlement_domain(),
            &ReceiptAuthority::default(),
        ),
        Err(VerifyError::Unavailable(ExportError::AuthorityUnavailable { reference: ref missing }))
            if *missing == reference
    ));

    let mut substituted = digest;
    substituted[0] ^= 0xff;
    let status = verification_status(cached.verify(
        substituted,
        scope.principal(),
        settlement_domain(),
        &authority,
    ));
    assert_eq!(status.label(), "unverified");
    assert_eq!(
        status.unverified_reason(),
        Some(UnverifiedReason::DigestMismatch)
    );

    let mut altered_bytes = genuine_bytes.clone();
    let altered_at = unique_position(
        &altered_bytes,
        &artifact.receipts[0].canonical_receipt_bytes,
    ) + 8;
    altered_bytes[altered_at] ^= 0x01;
    result(
        scope.put(
            Table::Cache,
            evidence_row_key(digest),
            31,
            altered_bytes.clone(),
        ),
        "altered evidence row",
    );
    let altered = result(
        EvidenceBundle::decode(
            scope
                .get(Table::Cache, &evidence_row_key(digest))
                .unwrap_or_else(|| panic!("altered evidence row missing"))
                .bytes(),
        ),
        "altered row decode",
    );
    let status = verification_status(altered.verify(
        digest,
        scope.principal(),
        settlement_domain(),
        &authority,
    ));
    assert!(!status.is_receipt_verified());
    assert_eq!(status.label(), "unverified");
    assert_eq!(
        status.unverified_reason(),
        Some(UnverifiedReason::DigestMismatch)
    );
    let (rekeyed_digest, rekeyed) = cache_bundle(&mut scope, altered_bytes, 32);
    assert_ne!(rekeyed_digest, digest);
    let status = verification_status(rekeyed.verify(
        rekeyed_digest,
        scope.principal(),
        settlement_domain(),
        &authority,
    ));
    assert_eq!(status.label(), "unverified");
    assert_eq!(status.unverified_reason(), Some(UnverifiedReason::Tampered));

    let (bob_digest, bob_row) = cache_bundle(&mut scope, bob_bytes, 33);
    let status = verification_status(bob_row.verify(
        bob_digest,
        scope.principal(),
        settlement_domain(),
        &authority,
    ));
    assert_eq!(status.label(), "unverified");
    assert_eq!(
        status.unverified_reason(),
        Some(UnverifiedReason::PrincipalMismatch)
    );
    assert!(bob_row
        .verify(bob_digest, &bob, settlement_domain(), &authority)
        .is_ok());

    let (foreign_digest, foreign_row) = cache_bundle(&mut scope, foreign_domain_bytes, 34);
    let status = verification_status(foreign_row.verify(
        foreign_digest,
        scope.principal(),
        settlement_domain(),
        &authority,
    ));
    assert_eq!(status.label(), "unverified");
    assert_eq!(
        status.unverified_reason(),
        Some(UnverifiedReason::SettlementDomainMismatch)
    );

    let genuine_key = SigningKey::from_bytes(&[3; 32]).verifying_key().to_bytes();
    let other_key = SigningKey::from_bytes(&[9; 32]).verifying_key().to_bytes();
    let mut spliced_bytes = genuine_bytes.clone();
    let spliced_at = unique_position(&spliced_bytes, &genuine_key);
    spliced_bytes[spliced_at..spliced_at + 32].copy_from_slice(&other_key);
    let (spliced_digest, spliced) = cache_bundle(&mut scope, spliced_bytes, 35);
    let status = verification_status(spliced.verify(
        spliced_digest,
        scope.principal(),
        settlement_domain(),
        &authority,
    ));
    assert_eq!(status.label(), "unverified");
    assert_eq!(
        status.unverified_reason(),
        Some(UnverifiedReason::AuthorityMismatch)
    );
    let mut wrong_authority = ReceiptAuthority::default();
    wrong_authority.insert(
        reference.clone(),
        AuthorizedBatch::new([4; 32], [5; 32], [2; 32], [3; 32], other_key),
    );
    let status = verification_status(cached.verify(
        digest,
        scope.principal(),
        settlement_domain(),
        &wrong_authority,
    ));
    assert_eq!(
        status.unverified_reason(),
        Some(UnverifiedReason::AuthorityMismatch)
    );
    let _ = std::fs::remove_dir_all(root);
}

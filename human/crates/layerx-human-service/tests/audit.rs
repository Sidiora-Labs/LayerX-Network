use layerx_human_test_support as support;

use std::fs;

use layerx_human_service::audit::{
    verify_export, ApprovalOutcome, AuditChain, AuditError, AuditEvent, AuthMethod, Decision,
    IdentityEvent, JourneyKind, JourneyState, NotificationChannel, NotificationClass,
    SecurityChangeKind, SigningOperation, StepUpEvidence,
};
use layerx_human_service::redaction::{
    decode, emit, read_stage, record_stage, verify_registry, FieldValue, Funnel, Label,
    RedactionError, StageOutcome, StageRecord, AGENT_CALL_SCHEMA, ERROR_RESPONSE_SCHEMA, FUNNELS,
};
use layerx_human_service::store::{EvidenceRef, PrincipalStore, StoreError, Table};
use layerx_human_service::trace::{TraceId, TRACE_HEADER};
use support::{directory, install_and_open, principal, retention_uniform, row_key, tenancy};

fn event_set() -> Vec<AuditEvent> {
    vec![
        AuditEvent::Authentication {
            method: AuthMethod::Passkey,
            outcome: Decision::Granted,
        },
        AuditEvent::SigningDecision {
            operation: SigningOperation::LxpSend,
            disclosure_digest: [2; 32],
            step_up: StepUpEvidence::Fresh {
                ceremony_digest: [3; 32],
            },
            outcome: Decision::Granted,
        },
        AuditEvent::ApprovalDecision {
            hold_digest: [4; 32],
            step_up: StepUpEvidence::Fresh {
                ceremony_digest: [5; 32],
            },
            outcome: ApprovalOutcome::Approved,
        },
        AuditEvent::JourneyTransition {
            journey: Label::new("journey-1")
                .unwrap_or_else(|error| panic!("journey label: {error}")),
            kind: JourneyKind::Deposit,
            from: JourneyState::Processing,
            to: JourneyState::Done,
        },
        AuditEvent::SecurityChange {
            change: SecurityChangeKind::WalletRebinding,
            step_up: StepUpEvidence::Fresh {
                ceremony_digest: [6; 32],
            },
        },
        AuditEvent::NotificationDispatch {
            class: NotificationClass::MoneyArrived,
            channel: NotificationChannel::Push,
        },
        AuditEvent::IdentityLifecycle {
            event: IdentityEvent::DidRegistration,
            receipt_digest: [7; 32],
        },
    ]
}

#[test]
fn chain_covers_every_required_event_class_and_exports_its_evidence() {
    let root = directory("audit-events");
    let map = tenancy(&[("alice", "tenant-a")]);
    let (mut store, _digest) = install_and_open(&root, &map, retention_uniform(1));
    let mut scope = store
        .principal(&principal("alice"))
        .unwrap_or_else(|error| panic!("principal scope: {error}"));
    let evidence_key = row_key("receipt-1");
    scope
        .put(
            Table::Journeys,
            evidence_key.clone(),
            9,
            b"canonical-receipt-proof".to_vec(),
        )
        .unwrap_or_else(|error| panic!("evidence put: {error}"));
    let reference = EvidenceRef::new(Table::Journeys, evidence_key.clone());
    let trace = TraceId::mint([1; 16]);
    let expected = event_set();
    let mut chain = AuditChain::open(&scope).unwrap_or_else(|error| panic!("open: {error}"));
    for (index, event) in expected.iter().enumerate() {
        let now = 10_u64
            .checked_add(u64::try_from(index).unwrap_or_else(|_| panic!("index overflow")))
            .unwrap_or_else(|| panic!("time overflow"));
        chain
            .append(
                &mut scope,
                now,
                &trace,
                event,
                std::slice::from_ref(&reference),
            )
            .unwrap_or_else(|error| panic!("append: {error}"));
    }

    let entries = chain
        .entries(&scope)
        .unwrap_or_else(|error| panic!("entries: {error}"));
    assert_eq!(entries.len(), expected.len());
    for (entry, event) in entries.iter().zip(&expected) {
        assert_eq!(entry.event(), event);
        assert_eq!(entry.trace(), &trace);
        assert_eq!(entry.evidence().len(), 1);
    }

    let bundle = chain
        .export(&scope)
        .unwrap_or_else(|error| panic!("export: {error}"));
    let report = verify_export(&bundle).unwrap_or_else(|error| panic!("verify export: {error}"));
    assert_eq!(report.principal().as_str(), "alice");
    assert_eq!(report.entries(), expected.len());
    assert_eq!(report.evidence_rows(), 1);
    assert_eq!(report.head(), chain.head());

    scope
        .expire(u64::MAX)
        .unwrap_or_else(|error| panic!("expiry: {error}"));
    assert_eq!(
        scope
            .get(Table::Journeys, &evidence_key)
            .unwrap_or_else(|| panic!("pinned evidence expired"))
            .bytes(),
        b"canonical-receipt-proof"
    );
}

#[test]
fn independent_export_verification_refuses_altered_entries_and_evidence() {
    let root = directory("audit-export-tamper");
    let map = tenancy(&[("alice", "tenant-a")]);
    let (mut store, _digest) = install_and_open(&root, &map, retention_uniform(100));
    let mut scope = store
        .principal(&principal("alice"))
        .unwrap_or_else(|error| panic!("principal scope: {error}"));
    let evidence_key = row_key("receipt-1");
    scope
        .put(
            Table::Journeys,
            evidence_key.clone(),
            1,
            b"receipt".to_vec(),
        )
        .unwrap_or_else(|error| panic!("evidence put: {error}"));
    let mut chain = AuditChain::open(&scope).unwrap_or_else(|error| panic!("open: {error}"));
    chain
        .append(
            &mut scope,
            2,
            &TraceId::mint([2; 16]),
            &AuditEvent::Authentication {
                method: AuthMethod::StepUp,
                outcome: Decision::Refused,
            },
            &[EvidenceRef::new(Table::Journeys, evidence_key)],
        )
        .unwrap_or_else(|error| panic!("append: {error}"));
    let bundle = chain
        .export(&scope)
        .unwrap_or_else(|error| panic!("export: {error}"));
    assert!(verify_export(&bundle).is_ok());

    let mut altered = bundle.clone();
    let last = altered
        .last_mut()
        .unwrap_or_else(|| panic!("empty audit export"));
    *last ^= 1;
    assert!(matches!(
        verify_export(&altered),
        Err(AuditError::EvidenceDigestMismatch { .. })
    ));

    let mut trailing = bundle;
    trailing.push(0);
    assert!(matches!(
        verify_export(&trailing),
        Err(AuditError::Corrupt("trailing audit export bytes"))
    ));
}

#[test]
fn startup_refuses_valid_tail_truncation_against_the_durable_anchor() {
    let root = directory("audit-tail");
    let map = tenancy(&[("alice", "tenant-a")]);
    let (mut store, digest) = install_and_open(&root, &map, retention_uniform(100));
    let alice = principal("alice");
    let trace = TraceId::mint([3; 16]);
    let old_store;
    let expected_head;
    {
        let mut scope = store
            .principal(&alice)
            .unwrap_or_else(|error| panic!("principal scope: {error}"));
        let mut chain = AuditChain::open(&scope).unwrap_or_else(|error| panic!("open: {error}"));
        chain
            .append(
                &mut scope,
                1,
                &trace,
                &AuditEvent::Authentication {
                    method: AuthMethod::Passkey,
                    outcome: Decision::Granted,
                },
                &[],
            )
            .unwrap_or_else(|error| panic!("first append: {error}"));
        old_store = fs::read(root.join("principals/alice/store.bin"))
            .unwrap_or_else(|error| panic!("read first store: {error}"));
        chain
            .append(
                &mut scope,
                2,
                &trace,
                &AuditEvent::SecurityChange {
                    change: SecurityChangeKind::SessionRevoked,
                    step_up: StepUpEvidence::NotRequired,
                },
                &[],
            )
            .unwrap_or_else(|error| panic!("second append: {error}"));
        expected_head = chain.head();
    }
    drop(store);
    fs::write(root.join("principals/alice/store.bin"), old_store)
        .unwrap_or_else(|error| panic!("restore truncated store: {error}"));
    let mut reopened = PrincipalStore::open(&root, retention_uniform(100), digest)
        .unwrap_or_else(|error| panic!("reopen store: {error}"));
    let scope = reopened
        .principal(&alice)
        .unwrap_or_else(|error| panic!("reopen scope: {error}"));
    assert!(matches!(
        AuditChain::open_anchored(&scope, expected_head),
        Err(AuditError::HeadMismatch { .. })
    ));
}

#[test]
fn reads_refuse_reordered_or_altered_chain_rows() {
    let root = directory("audit-reorder");
    let map = tenancy(&[("alice", "tenant-a")]);
    let (mut store, digest) = install_and_open(&root, &map, retention_uniform(100));
    let alice = principal("alice");
    {
        let mut scope = store
            .principal(&alice)
            .unwrap_or_else(|error| panic!("principal scope: {error}"));
        let mut chain = AuditChain::open(&scope).unwrap_or_else(|error| panic!("open: {error}"));
        for now in [1_u64, 2] {
            chain
                .append(
                    &mut scope,
                    now,
                    &TraceId::mint([u8::try_from(now).unwrap_or(0); 16]),
                    &AuditEvent::Authentication {
                        method: AuthMethod::Passkey,
                        outcome: Decision::Granted,
                    },
                    &[],
                )
                .unwrap_or_else(|error| panic!("append: {error}"));
        }
    }
    drop(store);

    let store_path = root.join("principals/alice/store.bin");
    let original = fs::read(&store_path).unwrap_or_else(|error| panic!("read store: {error}"));
    let mut reordered = original.clone();
    let key = b"chain-0000000000000001";
    let position = reordered
        .windows(key.len())
        .position(|window| window == key)
        .unwrap_or_else(|| panic!("second chain key not found"));
    reordered[position + key.len() - 1] = b'2';
    fs::write(&store_path, reordered).unwrap_or_else(|error| panic!("write reordered: {error}"));
    let mut reopened = PrincipalStore::open(&root, retention_uniform(100), digest)
        .unwrap_or_else(|error| panic!("reopen reordered store: {error}"));
    let scope = reopened
        .principal(&alice)
        .unwrap_or_else(|error| panic!("reopen reordered scope: {error}"));
    assert!(matches!(
        AuditChain::open(&scope),
        Err(AuditError::MissingEntry { sequence: 1 })
    ));
    drop(scope);
    drop(reopened);

    let mut altered = original;
    let trace = TraceId::mint([1; 16]).to_string();
    let position = altered
        .windows(trace.len())
        .position(|window| window == trace.as_bytes())
        .unwrap_or_else(|| panic!("first trace not found"));
    altered[position + trace.len() - 1] = b'2';
    fs::write(&store_path, altered).unwrap_or_else(|error| panic!("write altered: {error}"));
    let mut reopened = PrincipalStore::open(&root, retention_uniform(100), digest)
        .unwrap_or_else(|error| panic!("reopen altered store: {error}"));
    let scope = reopened
        .principal(&alice)
        .unwrap_or_else(|error| panic!("reopen altered scope: {error}"));
    assert!(matches!(
        AuditChain::open(&scope),
        Err(AuditError::LinkMismatch { sequence: 1 })
    ));
}

#[test]
fn redaction_registry_and_funnel_instrumentation_exclude_sensitive_values() {
    verify_registry().unwrap_or_else(|error| panic!("redaction registry: {error}"));
    assert!(matches!(
        Label::new("alice@example.com"),
        Err(RedactionError::InvalidLabel)
    ));
    assert!(matches!(
        Label::new("100.25"),
        Err(RedactionError::InvalidLabel)
    ));
    assert_eq!(FUNNELS.len(), 6);

    let root = directory("funnel-redaction");
    let map = tenancy(&[("alice", "tenant-a")]);
    let (mut store, _digest) = install_and_open(&root, &map, retention_uniform(100));
    let mut scope = store
        .principal(&principal("alice"))
        .unwrap_or_else(|error| panic!("principal scope: {error}"));
    for (index, funnel) in FUNNELS.into_iter().enumerate() {
        let record = StageRecord::new(
            funnel,
            funnel.stages()[0],
            StageOutcome::Completed,
            TraceId::mint([u8::try_from(index).unwrap_or(0); 16]),
            25,
        )
        .unwrap_or_else(|error| panic!("stage record: {error}"));
        let key = row_key(&format!("stage-{index}"));
        record_stage(&mut scope, key.clone(), 1, &record)
            .unwrap_or_else(|error| panic!("record stage: {error}"));
        let stored = scope
            .get(Table::Telemetry, &key)
            .unwrap_or_else(|| panic!("stage not stored"));
        assert_eq!(
            read_stage(stored.bytes()).unwrap_or_else(|error| panic!("read stage: {error}")),
            record
        );
        assert!(!stored
            .bytes()
            .windows(b"alice".len())
            .any(|window| window == b"alice"));
    }
}

#[test]
fn trace_identifier_propagates_to_agent_calls_and_typed_errors() {
    let inbound = TraceId::mint([9; 16]);
    let adopted = TraceId::from_inbound(Some(inbound.as_str()), [1; 16]);
    assert_eq!(adopted, inbound);
    assert_eq!(adopted.outbound(), (TRACE_HEADER, inbound.as_str()));
    let malformed = TraceId::from_inbound(Some("trace-secret"), [8; 16]);
    assert_ne!(malformed.as_str(), "trace-secret");

    let agent_span = emit(
        AGENT_CALL_SCHEMA,
        &[
            FieldValue::Label(
                Label::new("prepare").unwrap_or_else(|error| panic!("label: {error}")),
            ),
            FieldValue::Label(
                Label::new("refused").unwrap_or_else(|error| panic!("label: {error}")),
            ),
            FieldValue::DurationMs(14),
            FieldValue::Trace(adopted.clone()),
        ],
    )
    .unwrap_or_else(|error| panic!("agent span: {error}"));
    assert_eq!(
        decode(&agent_span)
            .unwrap_or_else(|error| panic!("decode span: {error}"))
            .schema()
            .name,
        AGENT_CALL_SCHEMA
    );
    let error_line = emit(
        ERROR_RESPONSE_SCHEMA,
        &[
            FieldValue::Label(
                Label::new("session-expired").unwrap_or_else(|error| panic!("label: {error}")),
            ),
            FieldValue::Label(
                Label::new("reauthenticate").unwrap_or_else(|error| panic!("label: {error}")),
            ),
            FieldValue::Trace(adopted.clone()),
        ],
    )
    .unwrap_or_else(|error| panic!("error emission: {error}"));
    assert!(decode(&error_line).is_ok());

    let traced = adopted.wrap(StoreError::MissingEvidence);
    assert_eq!(traced.trace(), &inbound);
    assert!(traced.to_string().contains(inbound.as_str()));
}

#[test]
fn every_declared_funnel_rejects_foreign_stages() {
    for funnel in [
        Funnel::Onboarding,
        Funnel::Deposit,
        Funnel::Move,
        Funnel::CreateAgent,
        Funnel::Approval,
        Funnel::Withdrawal,
    ] {
        assert!(matches!(
            StageRecord::new(
                funnel,
                "undeclared-stage",
                StageOutcome::Failed,
                TraceId::mint([4; 16]),
                1,
            ),
            Err(RedactionError::ForeignStage)
        ));
    }
}

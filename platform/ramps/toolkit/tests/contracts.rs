use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use layerx_ramp_toolkit::journal::{
    CallbackIdentity, Journal, OrderSnapshot, TornTail, TransitionEvidence, WorkflowStage,
    WriteFault, WriteStep,
};
use layerx_ramp_toolkit::{
    operator_send_authorization_message, AggregateStatus, AuthenticatedPrincipal, CreateOrder,
    OperatorIdentity, QuoteTerms, RampDirection, RampError, RampOrder, EXTERNAL_CUSTODY_LABEL,
};
use layerx_wire::limits::{LEGACY_PROTOCOL_VERSION, PROTOCOL_VERSION};

const WORKER: &str = "test-worker";
const CALLBACK_ID: &str = "provider-callback-1";
const CALLBACK_EVIDENCE_DIGEST: [u8; 32] = [9; 32];
const CALLBACK_AT: u64 = 1_003;

fn order(direction: RampDirection, customer: &str) -> RampOrder {
    let payer_grant = match direction {
        RampDirection::OnRamp => None,
        RampDirection::OffRamp => Some([7; 32]),
    };
    RampOrder::bind(
        CreateOrder {
            order_id: "order-1".to_owned(),
            quote_id: "quote-1".to_owned(),
            payer_grant,
        },
        QuoteTerms {
            quote_id: "quote-1".to_owned(),
            direction,
            layerx_asset: [1; 32],
            layerx_amount: 1_000,
            external_currency: "EUR".to_owned(),
            external_amount_minor: 100,
            rate_numerator: 10,
            rate_denominator: 1,
            fee_minor: 2,
            maximum_slippage_bps: 25,
            context: [8; 32],
            provider_token: "product-eur".to_owned(),
            payout_token: "beneficiary-123".to_owned(),
            expires_at: 2_000,
        },
        AuthenticatedPrincipal {
            principal_id: customer.to_owned(),
            account: format!("agent:did:layerx:{customer}:main"),
        },
        OperatorIdentity {
            principal_id: "operator-1".to_owned(),
            account: "agent:did:layerx:operator-1:main".to_owned(),
            signer_key_handle: "kms.operator-1.primary".to_owned(),
        },
        1_000,
    )
    .unwrap_or_else(|error| panic!("bind order: {error:?}"))
}

fn journal_path(name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("layerx-ramp-{name}-{}.jsonl", std::process::id()));
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&path)
        .unwrap_or_else(|error| panic!("create journal: {error}"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .unwrap_or_else(|error| panic!("secure journal: {error}"));
    }
    drop(file);
    path
}

fn remove_journal(path: &Path) {
    fs::remove_file(path).unwrap_or_else(|error| panic!("remove journal: {error}"));
    #[cfg(unix)]
    {
        let mut lock = path.as_os_str().to_owned();
        lock.push(".writer-lock");
        fs::remove_file(PathBuf::from(lock))
            .unwrap_or_else(|error| panic!("remove journal lock: {error}"));
    }
}

fn open_journal(path: &Path) -> Journal {
    Journal::open(path).unwrap_or_else(|error| panic!("open journal: {error:?}"))
}

fn journal_bytes(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|error| panic!("read journal: {error}"))
}

fn append_raw(path: &Path, bytes: &[u8]) {
    let mut file = OpenOptions::new()
        .append(true)
        .open(path)
        .unwrap_or_else(|error| panic!("open journal for raw append: {error}"));
    file.write_all(bytes)
        .unwrap_or_else(|error| panic!("raw append: {error}"));
    file.sync_all()
        .unwrap_or_else(|error| panic!("sync raw append: {error}"));
}

fn seeded_journal(path: &Path) -> (Journal, [u8; 32]) {
    let bound = order(RampDirection::OnRamp, "customer-a");
    let digest = bound.order_digest;
    let mut journal = open_journal(path);
    journal
        .create_order(bound, 1_001)
        .unwrap_or_else(|error| panic!("create order: {error:?}"));
    journal
        .acquire_lease(digest, WORKER, 1_001, 30)
        .unwrap_or_else(|error| panic!("lease: {error:?}"));
    journal
        .transition(
            digest,
            WorkflowStage::CompliancePending,
            WorkflowStage::AwaitingExternalCredit,
            TransitionEvidence::empty(),
            WORKER,
            1_002,
        )
        .unwrap_or_else(|error| panic!("approve: {error:?}"));
    (journal, digest)
}

fn settled_evidence() -> TransitionEvidence {
    let mut evidence = TransitionEvidence::empty();
    evidence.provider_operation_id = Some("provider-op-1".to_owned());
    evidence.provider_evidence_digest = Some([3; 32]);
    evidence
}

fn settle_callback(journal: &mut Journal, digest: [u8; 32]) -> Result<bool, RampError> {
    journal.apply_provider_callback(
        digest,
        CALLBACK_ID,
        1,
        CALLBACK_EVIDENCE_DIGEST,
        WorkflowStage::AwaitingExternalCredit,
        WorkflowStage::ProviderSettled,
        settled_evidence(),
        CALLBACK_AT,
    )
}

fn settled_identity(digest: [u8; 32]) -> CallbackIdentity {
    CallbackIdentity {
        order_digest: digest,
        provider_sequence: 1,
        evidence_digest: CALLBACK_EVIDENCE_DIGEST,
    }
}

fn reference_journal(name: &str) -> (PathBuf, Journal) {
    let path = journal_path(name);
    let (mut journal, digest) = seeded_journal(&path);
    assert_eq!(settle_callback(&mut journal, digest), Ok(true));
    (path, journal)
}

fn assert_same_state(journal: &Journal, reference: &Journal, step: WriteStep) {
    assert_eq!(journal.projection(), reference.projection(), "{step:?}");
    assert_eq!(journal.head(), reference.head(), "{step:?}");
    assert_eq!(journal.record_count(), reference.record_count(), "{step:?}");
}

#[test]
fn digest_binds_authenticated_customer_and_direction() {
    let first = order(RampDirection::OnRamp, "customer-a");
    let another_customer = order(RampDirection::OnRamp, "customer-b");
    let opposite_direction = order(RampDirection::OffRamp, "customer-a");
    assert_ne!(first.order_digest, another_customer.order_digest);
    assert_ne!(first.order_digest, opposite_direction.order_digest);
    assert_eq!(first.context, [8; 32]);
}

#[test]
fn payment_direction_selects_direct_send_or_customer_grant() {
    let on_ramp = order(RampDirection::OnRamp, "customer-a");
    let off_ramp = order(RampDirection::OffRamp, "customer-a");
    assert_eq!(on_ramp.payer_grant, None);
    assert_eq!(off_ramp.payer_grant, Some([7; 32]));
    let authorization = operator_send_authorization_message(&on_ramp, 9, 1, PROTOCOL_VERSION)
        .unwrap_or_else(|error| panic!("authorization message: {error:?}"));
    assert_eq!(authorization.len(), 266);
    assert_eq!(&authorization[..2], &0x5301_u16.to_be_bytes());
    assert_eq!(&authorization[264..], &PROTOCOL_VERSION.to_be_bytes());
    assert_eq!(
        operator_send_authorization_message(&on_ramp, 9, 1, LEGACY_PROTOCOL_VERSION),
        Err(RampError::InvalidOrder)
    );
    assert_eq!(
        operator_send_authorization_message(&off_ramp, 9, 1, PROTOCOL_VERSION),
        Err(RampError::InvalidOrder)
    );
}

#[test]
fn done_requires_both_verified_legs_and_external_label() {
    let path = journal_path("done-gate");
    let bound = order(RampDirection::OnRamp, "customer-a");
    let digest = bound.order_digest;
    let mut journal = open_journal(&path);
    journal
        .create_order(bound, 1_001)
        .unwrap_or_else(|error| panic!("create order: {error:?}"));
    journal
        .acquire_lease(digest, WORKER, 1_001, 30)
        .unwrap_or_else(|error| panic!("lease: {error:?}"));
    journal
        .transition(
            digest,
            WorkflowStage::CompliancePending,
            WorkflowStage::AwaitingExternalCredit,
            TransitionEvidence::empty(),
            WORKER,
            1_002,
        )
        .unwrap_or_else(|error| panic!("approve: {error:?}"));
    let direct_done = journal.transition(
        digest,
        WorkflowStage::AwaitingExternalCredit,
        WorkflowStage::Done,
        TransitionEvidence::empty(),
        WORKER,
        1_003,
    );
    assert_eq!(direct_done, Err(RampError::IllegalTransition));
    let presentation = journal
        .order(&digest)
        .unwrap_or_else(|| panic!("order missing"))
        .presentation();
    assert_eq!(presentation.status, AggregateStatus::Pending);
    assert_eq!(presentation.external_custody_label, EXTERNAL_CUSTODY_LABEL);
    drop(journal);
    remove_journal(&path);
}

#[test]
fn callback_validation_is_staged_before_any_durable_append() {
    let path = journal_path("staged-callback");
    let (mut journal, digest) = seeded_journal(&path);
    let before = journal.projection().clone();
    let bytes_before = journal_bytes(&path);
    let head_before = journal.head();
    let count_before = journal.record_count();

    let illegal_transition = journal.apply_provider_callback(
        digest,
        CALLBACK_ID,
        1,
        CALLBACK_EVIDENCE_DIGEST,
        WorkflowStage::AwaitingExternalCredit,
        WorkflowStage::LayerxVerified,
        settled_evidence(),
        CALLBACK_AT,
    );
    assert_eq!(illegal_transition, Err(RampError::IllegalTransition));
    let missing_evidence = journal.apply_provider_callback(
        digest,
        CALLBACK_ID,
        1,
        CALLBACK_EVIDENCE_DIGEST,
        WorkflowStage::AwaitingExternalCredit,
        WorkflowStage::ProviderSettled,
        TransitionEvidence::empty(),
        CALLBACK_AT,
    );
    assert_eq!(missing_evidence, Err(RampError::IllegalTransition));
    let stale_stage = journal.apply_provider_callback(
        digest,
        CALLBACK_ID,
        1,
        CALLBACK_EVIDENCE_DIGEST,
        WorkflowStage::CompliancePending,
        WorkflowStage::AwaitingExternalCredit,
        TransitionEvidence::empty(),
        CALLBACK_AT,
    );
    assert_eq!(stale_stage, Err(RampError::Conflict));
    let unknown_order = journal.apply_provider_callback(
        [2; 32],
        CALLBACK_ID,
        1,
        CALLBACK_EVIDENCE_DIGEST,
        WorkflowStage::AwaitingExternalCredit,
        WorkflowStage::ProviderSettled,
        settled_evidence(),
        CALLBACK_AT,
    );
    assert_eq!(unknown_order, Err(RampError::InvalidOrder));
    let zero_sequence = journal.apply_provider_callback(
        digest,
        CALLBACK_ID,
        0,
        CALLBACK_EVIDENCE_DIGEST,
        WorkflowStage::AwaitingExternalCredit,
        WorkflowStage::ProviderSettled,
        settled_evidence(),
        CALLBACK_AT,
    );
    assert_eq!(zero_sequence, Err(RampError::Provider));
    assert_eq!(journal.projection(), &before);
    assert_eq!(journal.callback(CALLBACK_ID), None);
    assert_eq!(journal.provider_sequence(&digest), None);
    assert_eq!(journal.head(), head_before);
    assert_eq!(journal.record_count(), count_before);
    assert_eq!(journal_bytes(&path), bytes_before);

    assert_eq!(settle_callback(&mut journal, digest), Ok(true));
    assert_eq!(
        journal.callback(CALLBACK_ID),
        Some(settled_identity(digest))
    );
    assert_eq!(journal.provider_sequence(&digest), Some(1));
    assert_eq!(
        journal.order(&digest).map(|snapshot| snapshot.stage),
        Some(WorkflowStage::ProviderSettled)
    );
    assert_eq!(journal.record_count(), count_before + 1);
    assert_eq!(journal_bytes(&path).len() as u64, journal.durable_len());
    drop(journal);
    remove_journal(&path);
}

#[test]
fn applied_callback_identity_is_idempotent_and_forged_retries_conflict() {
    let path = journal_path("callback-identity");
    let (mut journal, digest) = seeded_journal(&path);
    assert_eq!(settle_callback(&mut journal, digest), Ok(true));
    let applied = journal.projection().clone();
    let bytes_applied = journal_bytes(&path);
    let count_applied = journal.record_count();

    assert_eq!(settle_callback(&mut journal, digest), Ok(false));
    let forged_identity = journal.apply_provider_callback(
        digest,
        CALLBACK_ID,
        2,
        [10; 32],
        WorkflowStage::ProviderSettled,
        WorkflowStage::ProviderReversed,
        settled_evidence(),
        CALLBACK_AT,
    );
    assert_eq!(forged_identity, Err(RampError::Conflict));
    let stale_sequence = journal.apply_provider_callback(
        digest,
        "provider-callback-0",
        1,
        [11; 32],
        WorkflowStage::ProviderSettled,
        WorkflowStage::ProviderReversed,
        settled_evidence(),
        CALLBACK_AT,
    );
    assert_eq!(stale_sequence, Err(RampError::Conflict));
    assert_eq!(journal.projection(), &applied);
    assert_eq!(journal.projection().callbacks().len(), 1);
    assert_eq!(journal.projection().provider_sequences().len(), 1);
    assert_eq!(journal.record_count(), count_applied);
    assert_eq!(journal_bytes(&path), bytes_applied);
    drop(journal);
    remove_journal(&path);
}

#[test]
fn failed_callback_apply_retains_no_event_and_retry_is_idempotent() {
    let (reference_path, reference) = reference_journal("callback-fail-reference");
    let reference_bytes = journal_bytes(&reference_path);
    for step in WriteStep::ALL {
        let path = journal_path(&format!("callback-fail-{step:?}"));
        let (mut journal, digest) = seeded_journal(&path);
        let before = journal.projection().clone();
        let bytes_before = journal_bytes(&path);
        let head_before = journal.head();
        let count_before = journal.record_count();

        journal.arm_write_fault(step, WriteFault::Fail);
        assert_eq!(
            settle_callback(&mut journal, digest),
            Err(RampError::Journal),
            "{step:?}"
        );
        assert_eq!(journal.armed_write_fault(), None, "{step:?}");
        assert!(!journal.halted(), "{step:?}");
        assert_eq!(journal.projection(), &before, "{step:?}");
        assert_eq!(journal.callback(CALLBACK_ID), None, "{step:?}");
        assert_eq!(journal.provider_sequence(&digest), None, "{step:?}");
        assert_eq!(journal.head(), head_before, "{step:?}");
        assert_eq!(journal.record_count(), count_before, "{step:?}");
        assert_eq!(journal_bytes(&path), bytes_before, "{step:?}");

        assert_eq!(settle_callback(&mut journal, digest), Ok(true), "{step:?}");
        assert_same_state(&journal, &reference, step);
        assert_eq!(journal_bytes(&path), reference_bytes, "{step:?}");
        assert_eq!(settle_callback(&mut journal, digest), Ok(false), "{step:?}");
        drop(journal);

        let replayed = open_journal(&path);
        assert_eq!(replayed.recovery(), None, "{step:?}");
        assert_same_state(&replayed, &reference, step);
        drop(replayed);
        remove_journal(&path);
    }
    drop(reference);
    remove_journal(&reference_path);
}

#[test]
fn interrupted_callback_write_recovers_on_restart_without_repair() {
    let (reference_path, reference) = reference_journal("callback-interrupt-reference");
    let reference_bytes = journal_bytes(&reference_path);
    for step in WriteStep::ALL {
        let path = journal_path(&format!("callback-interrupt-{step:?}"));
        let (mut journal, digest) = seeded_journal(&path);
        let before = journal.projection().clone();
        let bytes_before = journal_bytes(&path);
        let head_before = journal.head();

        journal.arm_write_fault(step, WriteFault::Interrupt);
        assert_eq!(
            settle_callback(&mut journal, digest),
            Err(RampError::Journal),
            "{step:?}"
        );
        assert_eq!(journal.armed_write_fault(), None, "{step:?}");
        assert!(journal.halted(), "{step:?}");
        assert_eq!(journal.projection(), &before, "{step:?}");
        assert_eq!(journal.head(), head_before, "{step:?}");
        assert_eq!(
            journal.acquire_lease(digest, WORKER, CALLBACK_AT, 30),
            Err(RampError::Journal),
            "{step:?}"
        );
        let interrupted = journal_bytes(&path);
        match step {
            WriteStep::BeforeRecord => assert_eq!(interrupted, bytes_before, "{step:?}"),
            WriteStep::DuringRecord | WriteStep::BeforeTerminator => {
                assert!(interrupted.len() > bytes_before.len(), "{step:?}");
                assert!(interrupted.starts_with(&bytes_before), "{step:?}");
                assert!(reference_bytes.starts_with(&interrupted), "{step:?}");
                assert_ne!(interrupted.last(), Some(&b'\n'), "{step:?}");
            }
            WriteStep::AfterRecord | WriteStep::AfterSync => {
                assert_eq!(interrupted, reference_bytes, "{step:?}");
            }
        }
        drop(journal);

        let mut recovered = open_journal(&path);
        match step {
            WriteStep::BeforeRecord => {
                assert_eq!(recovered.recovery(), None, "{step:?}");
                assert_eq!(recovered.projection(), &before, "{step:?}");
                assert_eq!(recovered.head(), head_before, "{step:?}");
                assert_eq!(
                    settle_callback(&mut recovered, digest),
                    Ok(true),
                    "{step:?}"
                );
            }
            WriteStep::DuringRecord | WriteStep::BeforeTerminator => {
                assert_eq!(
                    recovered.recovery(),
                    Some(TornTail {
                        offset: bytes_before.len() as u64,
                        bytes: (interrupted.len() - bytes_before.len()) as u64,
                    }),
                    "{step:?}"
                );
                assert_eq!(journal_bytes(&path), bytes_before, "{step:?}");
                assert_eq!(recovered.projection(), &before, "{step:?}");
                assert_eq!(recovered.head(), head_before, "{step:?}");
                assert_eq!(
                    settle_callback(&mut recovered, digest),
                    Ok(true),
                    "{step:?}"
                );
            }
            WriteStep::AfterRecord | WriteStep::AfterSync => {
                assert_eq!(recovered.recovery(), None, "{step:?}");
                assert_eq!(recovered.projection(), reference.projection(), "{step:?}");
                assert_eq!(
                    settle_callback(&mut recovered, digest),
                    Ok(false),
                    "{step:?}"
                );
            }
        }
        assert_eq!(
            recovered.callback(CALLBACK_ID),
            Some(settled_identity(digest))
        );
        assert_eq!(recovered.provider_sequence(&digest), Some(1));
        assert_same_state(&recovered, &reference, step);
        assert_eq!(journal_bytes(&path), reference_bytes, "{step:?}");
        drop(recovered);

        let replayed = open_journal(&path);
        assert_eq!(replayed.recovery(), None, "{step:?}");
        assert_same_state(&replayed, &reference, step);
        drop(replayed);
        remove_journal(&path);
    }
    drop(reference);
    remove_journal(&reference_path);
}

#[test]
fn replaying_the_journal_reproduces_identical_projection() {
    let path = journal_path("replay-equivalence");
    let (mut journal, digest) = seeded_journal(&path);
    assert_eq!(settle_callback(&mut journal, digest), Ok(true));
    let mut layerx = TransitionEvidence::empty();
    layerx.activity_id = Some([4; 32]);
    layerx.canonical_activity = Some(vec![1, 2, 3]);
    layerx.retry_at = Some(1_100);
    journal
        .transition(
            digest,
            WorkflowStage::ProviderSettled,
            WorkflowStage::LayerxPending,
            layerx,
            WORKER,
            1_004,
        )
        .unwrap_or_else(|error| panic!("layerx pending: {error:?}"));
    journal
        .plan_paxeer([5; 32], [1; 32], 10, 1_005)
        .unwrap_or_else(|error| panic!("plan paxeer: {error:?}"));
    journal
        .observe_paxeer(
            [5; 32],
            "paxeer-op-1",
            [6; 32],
            "broadcast_unknown",
            None,
            0,
            1_006,
        )
        .unwrap_or_else(|error| panic!("observe paxeer: {error:?}"));
    let live = journal.projection().clone();
    let head = journal.head();
    let count = journal.record_count();
    let durable_len = journal.durable_len();
    assert_eq!(count, 7);
    assert_eq!(journal_bytes(&path).len() as u64, durable_len);
    drop(journal);

    let replayed = open_journal(&path);
    assert_eq!(replayed.recovery(), None);
    assert_eq!(replayed.projection(), &live);
    assert_eq!(replayed.head(), head);
    assert_eq!(replayed.record_count(), count);
    assert_eq!(replayed.durable_len(), durable_len);
    assert_eq!(
        replayed.order(&digest).map(OrderSnapshot::presentation),
        live.order(&digest).map(OrderSnapshot::presentation)
    );
    assert_eq!(replayed.paxeer(&[5; 32]), live.paxeer(&[5; 32]));
    assert_eq!(
        replayed.callback(CALLBACK_ID),
        Some(settled_identity(digest))
    );
    drop(replayed);
    remove_journal(&path);
}

#[test]
fn torn_tail_is_recovered_but_terminated_corruption_is_rejected() {
    let path = journal_path("torn-tail");
    let (journal, digest) = seeded_journal(&path);
    let clean = journal.projection().clone();
    let head = journal.head();
    let count = journal.record_count();
    let bytes = journal_bytes(&path);
    drop(journal);

    let first_record_end = bytes
        .iter()
        .position(|byte| *byte == b'\n')
        .unwrap_or_else(|| panic!("journal has no terminated record"));
    let torn = &bytes[..first_record_end / 2];
    append_raw(&path, torn);
    let recovered = open_journal(&path);
    assert_eq!(
        recovered.recovery(),
        Some(TornTail {
            offset: bytes.len() as u64,
            bytes: torn.len() as u64,
        })
    );
    assert_eq!(recovered.projection(), &clean);
    assert_eq!(recovered.head(), head);
    assert_eq!(recovered.record_count(), count);
    assert_eq!(journal_bytes(&path), bytes);
    assert_eq!(
        recovered.order(&digest).map(|snapshot| snapshot.stage),
        Some(WorkflowStage::AwaitingExternalCredit)
    );
    drop(recovered);

    append_raw(&path, b"{}\n");
    assert!(matches!(Journal::open(&path), Err(RampError::Journal)));
    fs::write(&path, &bytes).unwrap_or_else(|error| panic!("restore journal: {error}"));

    append_raw(&path, b"\n");
    assert!(matches!(Journal::open(&path), Err(RampError::Journal)));
    fs::write(&path, &bytes).unwrap_or_else(|error| panic!("restore journal: {error}"));

    let last_record_start = bytes[..bytes.len() - 1]
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |position| position + 1);
    append_raw(&path, &bytes[last_record_start..]);
    assert!(matches!(Journal::open(&path), Err(RampError::Journal)));
    fs::write(&path, &bytes).unwrap_or_else(|error| panic!("restore journal: {error}"));

    let reopened = open_journal(&path);
    assert_eq!(reopened.recovery(), None);
    assert_eq!(reopened.projection(), &clean);
    assert_eq!(reopened.head(), head);
    drop(reopened);
    remove_journal(&path);
}

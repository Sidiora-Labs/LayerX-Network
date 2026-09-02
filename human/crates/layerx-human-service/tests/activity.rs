#[allow(dead_code)]
mod support;

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;

use ed25519_dalek::{Signer as _, SigningKey};
use layerx_agent_api::identity::{
    ActivityType as ApiActivityType, AgentDid, Asset, CapabilityId, Counterparty, ExplicitSet,
    TenantId as ApiTenantId,
};
use layerx_agent_api::read::{AccountRef, ModuleRef};
use layerx_agent_api::subscription::{
    Cursor as ApiCursor, CursorAcknowledgement, DeliveryTarget, EventDelivery, EventIdentity,
    ReceiptReference, SubscriptionCreate, SubscriptionFilter, SubscriptionId, SubscriptionScope,
    SubscriptionTarget, TenantObject,
};
use layerx_agent_api::track::ReceiptRef;
use layerx_agent_api::verify::Level;
use layerx_agent_api::Sequence;
use layerx_agentd::events::subscription::Store as SubscriptionStore;
use layerx_agentd::events::{
    backfill, deliver, ingest, CoreEvent, DeliveryEngine, DeliveryItem, EventAttributes,
    EventIngestor, RetryPolicy,
};
use layerx_agentd::store::{Store as AgentStore, TenantId};
use layerx_human_service::activity::{
    verification_status, ActivityKind, ActivityStatus, AgentActivity, DepositStage, EvidenceBundle,
    Feed, FeedCursor, FeedError, FilterDraft, PageRequest, PendingStatus, ReceiptAuthority,
    UnverifiedReason, VerifiedStatus, VerifyError, WithdrawalStage,
};
use layerx_human_service::audit::{AuditChain, AuditEvent, SecurityChangeKind, StepUpEvidence};
use layerx_human_service::notify::ActivityEntryId;
use layerx_human_service::store::{PrincipalStore, RowKey, Table};
use layerx_human_service::trace::TraceId;
use layerx_proof::checkpoint::SettlementDomain;
use layerx_proof::receipt::{verify as verify_receipt, AuthorizedBatch};
use layerx_types::result::ResultCode;
use layerx_types::verify::VerificationLevel;
use layerx_wire::limits::PROTOCOL_VERSION;
use sha2::{Digest as _, Sha256};

fn text<T, E: std::fmt::Debug>(value: Result<T, E>, label: &str) -> T {
    value.unwrap_or_else(|error| panic!("{label}: {error:?}"))
}

fn tenant() -> TenantId {
    text(TenantId::new("tenant-a"), "tenant")
}

fn api_tenant() -> ApiTenantId {
    text(ApiTenantId::new("tenant-a"), "API tenant")
}

fn subscription_scope() -> SubscriptionScope {
    SubscriptionScope {
        tenant: api_tenant(),
        agent: text(AgentDid::new("did:layerx:agent-a"), "agent"),
        capability: text(CapabilityId::new("activity-feed"), "capability"),
    }
}

fn subscription_id() -> SubscriptionId {
    text(SubscriptionId::new("human-activity"), "subscription")
}

fn target() -> SubscriptionTarget {
    SubscriptionTarget {
        scope: subscription_scope(),
        subscription_id: subscription_id(),
    }
}

fn filter() -> SubscriptionFilter {
    SubscriptionFilter {
        agents: ExplicitSet::allow(vec![TenantObject {
            tenant: api_tenant(),
            value: text(AgentDid::new("did:layerx:agent-a"), "filter agent"),
        }]),
        accounts: ExplicitSet::allow(vec![TenantObject {
            tenant: api_tenant(),
            value: text(AccountRef::new("account-a"), "account"),
        }]),
        activity_types: ExplicitSet::allow((9..=14).map(ApiActivityType).collect()),
        modules: ExplicitSet::allow(
            ["bridge", "asset", "identity", "approval"]
                .into_iter()
                .map(|module| TenantObject {
                    tenant: api_tenant(),
                    value: text(ModuleRef::new(module), "module"),
                })
                .collect(),
        ),
        assets: ExplicitSet::allow(vec![TenantObject {
            tenant: api_tenant(),
            value: text(Asset::new("LXR"), "asset"),
        }]),
        counterparties: ExplicitSet::allow(vec![TenantObject {
            tenant: api_tenant(),
            value: text(Counterparty::new("counterparty-a"), "counterparty"),
        }]),
        result_classes: ExplicitSet::allow(vec![ResultCode::from_raw(0)]),
    }
}

fn attributes(sequence: u64) -> EventAttributes {
    let (activity_type, module) = match sequence {
        0 | 6 => (9, "bridge"),
        1 => (10, "bridge"),
        2 => (11, "asset"),
        3 => (12, "identity"),
        4 => (13, "approval"),
        _ => (14, "identity"),
    };
    EventAttributes {
        agent: "did:layerx:agent-a".to_owned(),
        account: "account-a".to_owned(),
        activity_type,
        module: module.to_owned(),
        asset: "LXR".to_owned(),
        counterparty: "counterparty-a".to_owned(),
        result_code: 0,
    }
}

#[derive(Clone)]
struct ReceiptFields {
    activity_id: [u8; 32],
    previous_state_root: [u8; 32],
    resulting_state_root: [u8; 32],
    batch_id: [u8; 32],
    asset: [u8; 32],
}

fn push_bytes(output: &mut Vec<u8>, value: &[u8]) {
    let length =
        u32::try_from(value.len()).unwrap_or_else(|_| panic!("receipt field length overflow"));
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(value);
}

fn encode_receipt(fields: &ReceiptFields, signature: Option<[u8; 64]>) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&PROTOCOL_VERSION.to_be_bytes());
    bytes.extend_from_slice(&0x5201_u16.to_be_bytes());
    bytes.extend_from_slice(&PROTOCOL_VERSION.to_be_bytes());
    push_bytes(&mut bytes, &fields.activity_id);
    bytes.extend_from_slice(&17_u64.to_be_bytes());
    push_bytes(&mut bytes, &fields.previous_state_root);
    push_bytes(&mut bytes, &fields.resulting_state_root);
    push_bytes(&mut bytes, &[0x81; 32]);
    bytes.extend_from_slice(&0_i32.to_be_bytes());
    bytes.extend_from_slice(&0_u32.to_be_bytes());
    bytes.extend_from_slice(&1_u128.to_be_bytes());
    push_bytes(&mut bytes, &fields.batch_id);
    bytes.extend_from_slice(&1_u16.to_be_bytes());
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.push(1);
    push_bytes(&mut bytes, &fields.asset);
    bytes.extend_from_slice(&25_u128.to_be_bytes());
    push_bytes(&mut bytes, &[0x91; 32]);
    bytes.extend_from_slice(&100_u128.to_be_bytes());
    bytes.extend_from_slice(&75_u128.to_be_bytes());
    bytes.extend_from_slice(&1_u64.to_be_bytes());
    push_bytes(&mut bytes, &[0x92; 32]);
    bytes.extend_from_slice(&10_u128.to_be_bytes());
    bytes.extend_from_slice(&35_u128.to_be_bytes());
    push_bytes(&mut bytes, &[0x93; 32]);
    push_bytes(&mut bytes, &[0x94; 32]);
    push_bytes(&mut bytes, &[0x95; 32]);
    bytes.extend_from_slice(&1_000_u64.to_be_bytes());
    bytes.push(u8::from(signature.is_some()));
    if let Some(signature) = signature {
        push_bytes(&mut bytes, &signature);
    }
    bytes
}

fn real_verified_receipt() -> ([u8; 32], Vec<u8>) {
    let (reference, canonical, _) = real_receipt_material();
    (reference, canonical)
}

fn real_receipt_material() -> ([u8; 32], Vec<u8>, AuthorizedBatch) {
    let fields = ReceiptFields {
        activity_id: [0x11; 32],
        previous_state_root: [0x12; 32],
        resulting_state_root: [0x13; 32],
        batch_id: [0x14; 32],
        asset: [0x15; 32],
    };
    let signing_key = SigningKey::from_bytes(&[0x16; 32]);
    let unsigned = encode_receipt(&fields, None);
    let mut digest = Sha256::new();
    digest.update(b"LXP/v1/receipt\0");
    digest.update(&unsigned);
    let signature = signing_key.sign(&<[u8; 32]>::from(digest.finalize()));
    let canonical = encode_receipt(&fields, Some(signature.to_bytes()));
    let authorised = AuthorizedBatch::new(
        fields.batch_id,
        fields.asset,
        fields.previous_state_root,
        fields.resulting_state_root,
        signing_key.verifying_key().to_bytes(),
    );
    let verified = verify_receipt(&canonical, &authorised)
        .unwrap_or_else(|error| panic!("real receipt verification: {error:?}"));
    assert_eq!(verified.level(), VerificationLevel::SEQUENCER_SIGNED);
    (
        Sha256::digest(verified.canonical_bytes()).into(),
        canonical,
        authorised,
    )
}

fn digest_hex(digest: &[u8]) -> String {
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn evidence_row_key(digest: [u8; 32]) -> RowKey {
    text(
        RowKey::new(format!("activity-evidence-{}", digest_hex(&digest))),
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

fn cache_row(
    scope: &mut layerx_human_service::store::PrincipalScope<'_>,
    bytes: Vec<u8>,
    written_at: u64,
) -> ([u8; 32], EvidenceBundle) {
    let digest: [u8; 32] = Sha256::digest(&bytes).into();
    text(
        scope.put(Table::Cache, evidence_row_key(digest), written_at, bytes),
        "evidence cache row",
    );
    let row = scope
        .get(Table::Cache, &evidence_row_key(digest))
        .unwrap_or_else(|| panic!("evidence cache row missing"));
    (
        digest,
        text(EvidenceBundle::decode(row.bytes()), "cached row decode"),
    )
}

fn delivered_events(root: &std::path::Path) -> Vec<EventDelivery> {
    let (receipt_reference, canonical_receipt) = real_verified_receipt();
    let durable = text(AgentStore::open(root), "agent store");
    let mut ingestor = text(
        EventIngestor::open(durable, tenant(), 16, 0),
        "event ingestor",
    );
    for sequence in 0_u64..7 {
        let receipt = (sequence == 6).then_some(receipt_reference);
        let bytes = if sequence == 6 {
            canonical_receipt.clone()
        } else {
            vec![0x45, u8::try_from(sequence).unwrap_or(0), 0x56]
        };
        text(
            ingest(
                &mut ingestor,
                CoreEvent {
                    global_sequence: sequence,
                    canonical_bytes: bytes,
                    receipt_reference: receipt,
                    receipt_verification_level: if receipt.is_some() {
                        VerificationLevel::SEQUENCER_SIGNED
                    } else {
                        VerificationLevel::UNVERIFIED
                    },
                    attributes: attributes(sequence),
                },
            ),
            "core event ingestion",
        );
    }
    let mut subscriptions = text(
        SubscriptionStore::open(ingestor.into_store(), tenant()),
        "subscription store",
    );
    text(
        subscriptions.create(
            subscription_id(),
            SubscriptionCreate {
                scope: subscription_scope(),
                filter: filter(),
                start: ApiCursor(Sequence(0)),
                delivery_target: text(DeliveryTarget::new("uds://human-feed"), "delivery target"),
            },
        ),
        "create subscription",
    );
    let mut engine = text(
        DeliveryEngine::open(
            subscriptions,
            target(),
            0,
            16,
            RetryPolicy {
                base_delay_ms: 1,
                maximum_delay_ms: 8,
                jitter_percent: 0,
                maximum_attempts: 3,
            },
        ),
        "delivery engine",
    );
    let mut events = Vec::new();
    let mut now = 1_000_u64;
    while events.len() < 7 {
        text(backfill(&mut engine), "delivery backfill");
        let item = text(deliver(&mut engine), "delivery attempt")
            .unwrap_or_else(|| panic!("expected pending delivery"));
        let accepted = text(engine.accept_front(now), "accept delivery");
        assert_eq!(accepted, item);
        if let DeliveryItem::Event(event) = accepted {
            text(
                engine.acknowledge(&CursorAcknowledgement {
                    scope: subscription_scope(),
                    subscription_id: subscription_id(),
                    cursor: event.delivery.cursor,
                }),
                "acknowledge delivery",
            );
            events.push(event.delivery);
        }
        now = now.saturating_add(1);
    }
    events
}

fn id(value: &str) -> ActivityEntryId {
    text(ActivityEntryId::new(value), "activity entry")
}

fn descriptor(
    entry_id: ActivityEntryId,
    kind: ActivityKind,
    pending: PendingStatus,
) -> AgentActivity {
    let verified = match kind {
        ActivityKind::Deposit => VerifiedStatus::DepositDone,
        ActivityKind::Withdrawal => VerifiedStatus::WithdrawalPaidOut,
        ActivityKind::Movement
        | ActivityKind::AgentAction
        | ActivityKind::Approval
        | ActivityKind::Security => VerifiedStatus::Done,
    };
    text(
        AgentActivity::new(
            entry_id,
            kind,
            Some("did:layerx:agent-a".to_owned()),
            10,
            pending,
            verified,
        ),
        "agent activity descriptor",
    )
}

#[test]
#[allow(clippy::too_many_lines)]
fn real_agent_receipts_restart_and_snapshot_cursors_drive_one_unified_feed() {
    let root = support::directory("activity-feed");
    fs::create_dir_all(&root).unwrap_or_else(|error| panic!("fixture root: {error}"));
    let deliveries = delivered_events(&root.join("agent"));
    let human_root = root.join("human");
    let tenancy = support::tenancy(&[("alice", "tenant-a"), ("bob", "tenant-b")]);
    let (mut store, tenancy_digest) =
        support::install_and_open(&human_root, &tenancy, support::retention_uniform(10_000));
    let alice = support::principal("alice");
    let bob = support::principal("bob");
    let feed = text(Feed::new(5), "feed");
    let mut scope = text(store.principal(&alice), "alice scope");

    let mut audit = text(AuditChain::open(&scope), "audit chain");
    text(
        audit.append(
            &mut scope,
            9,
            &TraceId::mint([0x31; 16]),
            &AuditEvent::SecurityChange {
                change: SecurityChangeKind::SessionRevoked,
                step_up: StepUpEvidence::NotRequired,
            },
            &[],
        ),
        "security audit",
    );
    let security_entry = text(audit.entries(&scope), "verified audit entries")
        .pop()
        .unwrap_or_else(|| panic!("security entry missing"));
    let security = text(
        Feed::record_security(&mut scope, &security_entry, 99),
        "security projection",
    );
    assert_eq!(security.status(), ActivityStatus::Processing);

    let deposit_id = id("act_deposit");
    let descriptors = [
        descriptor(
            deposit_id.clone(),
            ActivityKind::Deposit,
            PendingStatus::Deposit(DepositStage::WaitingForWallet),
        ),
        descriptor(
            id("act_withdrawal"),
            ActivityKind::Withdrawal,
            PendingStatus::Withdrawal(WithdrawalStage::WaitingForSettlement),
        ),
        descriptor(
            id("act_movement"),
            ActivityKind::Movement,
            PendingStatus::DidntGoThrough { money_left: true },
        ),
        descriptor(
            id("act_agentaction"),
            ActivityKind::AgentAction,
            PendingStatus::DidntGoThrough { money_left: false },
        ),
        descriptor(
            id("act_approval"),
            ActivityKind::Approval,
            PendingStatus::WaitingForYou,
        ),
        descriptor(
            security.entry_id().clone(),
            ActivityKind::Security,
            PendingStatus::Processing,
        ),
    ];
    for (index, (meaning, delivery)) in descriptors.iter().zip(&deliveries[..6]).enumerate() {
        text(
            Feed::record_agent_event(
                &mut scope,
                meaning,
                delivery,
                100_u64.saturating_add(u64::try_from(index).unwrap_or(0)),
            ),
            "agent event projection",
        );
    }

    let mut draft = FilterDraft::new();
    let all_filters = text(Feed::apply_filters(draft.clone()), "all filters");
    let first = text(
        feed.page(&scope, PageRequest::new(2, all_filters.clone()), 105, 6),
        "first page",
    );
    assert_eq!(first.entries().len(), 2);
    assert_eq!(first.applied_filters(), &all_filters);
    assert!(first.freshness().is_current());
    assert_eq!(first.freshness().agent_cursor(), Some(6));
    assert_eq!(first.freshness().agent_lag(), 0);
    let continuation = first
        .next()
        .cloned()
        .unwrap_or_else(|| panic!("first page needs continuation"));
    let first_ids: Vec<String> = first
        .entries()
        .iter()
        .map(|entry| entry.entry_id().as_str().to_owned())
        .collect();

    let completed = text(
        Feed::record_agent_event(
            &mut scope,
            &descriptor(
                deposit_id.clone(),
                ActivityKind::Deposit,
                PendingStatus::Deposit(DepositStage::Crediting),
            ),
            &deliveries[6],
            106,
        ),
        "verified receipt projection",
    );
    assert_eq!(
        completed.status(),
        ActivityStatus::Deposit(DepositStage::Done)
    );
    assert_eq!(completed.receipts().len(), 1);
    assert_eq!(completed.receipts()[0].level(), Level::SequencerSigned);

    drop(scope);
    drop(store);
    let mut reopened = text(
        PrincipalStore::open(
            &human_root,
            support::retention_uniform(10_000),
            tenancy_digest,
        ),
        "reopen principal store",
    );
    let scope = text(reopened.principal(&alice), "reopened alice scope");
    let second = text(
        feed.page(
            &scope,
            PageRequest::new(2, all_filters.clone()).after(continuation),
            106,
            7,
        ),
        "second page after restart",
    );
    let third = text(
        feed.page(
            &scope,
            PageRequest::new(2, all_filters.clone()).after(
                second
                    .next()
                    .cloned()
                    .unwrap_or_else(|| panic!("second page needs continuation")),
            ),
            106,
            7,
        ),
        "third page after restart",
    );
    assert!(third.next().is_none());
    let mut snapshot_ids = first_ids;
    snapshot_ids.extend(
        second
            .entries()
            .iter()
            .chain(third.entries())
            .map(|entry| entry.entry_id().as_str().to_owned()),
    );
    assert_eq!(snapshot_ids.len(), 6);
    assert_eq!(snapshot_ids.iter().collect::<BTreeSet<_>>().len(), 6);

    let current = text(
        feed.page(&scope, PageRequest::new(100, all_filters.clone()), 106, 7),
        "current traversal",
    );
    assert_eq!(current.entries().len(), 6);
    assert_eq!(current.entries()[0].entry_id(), &deposit_id);
    assert_eq!(
        current.entries()[0].status(),
        ActivityStatus::Deposit(DepositStage::Done)
    );
    assert_eq!(current.freshness().projected_at(), Some(106));
    assert_eq!(current.freshness().age_seconds(), Some(0));

    draft = draft
        .with_kinds([ActivityKind::Deposit])
        .with_agent("did:layerx:agent-a")
        .with_dates(Some(10), Some(10));
    let still_unfiltered = text(
        feed.page(&scope, PageRequest::new(100, all_filters.clone()), 106, 7),
        "unapplied draft",
    );
    assert_eq!(still_unfiltered.entries().len(), 6);
    let applied = text(Feed::apply_filters(draft), "applied filters");
    let filtered = text(
        feed.page(&scope, PageRequest::new(100, applied.clone()), 106, 7),
        "filtered page",
    );
    assert_eq!(filtered.applied_filters(), &applied);
    assert_eq!(filtered.entries().len(), 1);
    assert_eq!(filtered.entries()[0].kind(), ActivityKind::Deposit);
    assert_eq!(
        filtered.applied_filters().agent(),
        Some("did:layerx:agent-a")
    );

    let stale = text(
        feed.page(&scope, PageRequest::new(100, all_filters.clone()), 112, 7),
        "stale page",
    );
    assert!(!stale.freshness().is_current());
    assert_eq!(stale.freshness().freshness_bound_seconds(), 5);

    let alice_cursor = current.next().cloned().unwrap_or_else(|| {
        text(
            feed.page(&scope, PageRequest::new(1, all_filters.clone()), 106, 7),
            "cursor page",
        )
        .next()
        .cloned()
        .unwrap_or_else(|| panic!("cursor missing"))
    });
    drop(scope);
    let bob_scope = text(reopened.principal(&bob), "bob scope");
    assert!(matches!(
        feed.page(
            &bob_scope,
            PageRequest::new(1, all_filters.clone()).after(alice_cursor.clone()),
            106,
            0,
        ),
        Err(FeedError::CursorScopeMismatch)
    ));
    let bob_page = text(
        feed.page(&bob_scope, PageRequest::new(10, all_filters), 106, 0),
        "bob page",
    );
    assert!(bob_page.entries().is_empty());

    let mut altered = alice_cursor.as_str().as_bytes().to_vec();
    let last = altered
        .last_mut()
        .unwrap_or_else(|| panic!("cursor unexpectedly empty"));
    *last = if *last == b'a' { b'b' } else { b'a' };
    let altered =
        String::from_utf8(altered).unwrap_or_else(|error| panic!("cursor UTF-8: {error}"));
    assert!(matches!(
        FeedCursor::parse(altered),
        Err(FeedError::InvalidCursor)
    ));
    assert_eq!(
        ActivityStatus::DidntGoThrough { money_left: true }.label(),
        "Didn't go through — money already left"
    );
    assert_eq!(
        ActivityStatus::DidntGoThrough { money_left: false }.label(),
        "Didn't go through — no money left"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn completion_is_unrepresentable_without_verified_receipt_evidence() {
    assert!(matches!(
        AgentActivity::new(
            id("act_invaliddeposit"),
            ActivityKind::Deposit,
            Some("did:layerx:agent-a".to_owned()),
            1,
            PendingStatus::Deposit(DepositStage::Done),
            VerifiedStatus::DepositDone,
        ),
        Err(FeedError::CompletionWithoutReceipt)
    ));

    let root = support::directory("activity-done-gate");
    let map = support::tenancy(&[("alice", "tenant-a")]);
    let (mut store, _) = support::install_and_open(&root, &map, support::retention_uniform(100));
    let mut scope = text(store.principal(&support::principal("alice")), "scope");
    let delivery = text(
        EventDelivery::new(
            EventIdentity::new([0x41; 32]),
            vec![1, 2, 3],
            ApiCursor(Sequence(1)),
            ReceiptReference::Verified {
                receipt_ref: text(ReceiptRef::new("receipt-unverified"), "receipt ref"),
                verification_level: Level::Unverified,
            },
        ),
        "event delivery",
    );
    assert!(matches!(
        Feed::record_agent_event(
            &mut scope,
            &descriptor(
                id("act_unverified"),
                ActivityKind::Movement,
                PendingStatus::Processing,
            ),
            &delivery,
            1,
        ),
        Err(FeedError::UnverifiedReceipt)
    ));
    assert!(text(
        text(Feed::new(1), "feed").page(
            &scope,
            PageRequest::new(10, text(Feed::apply_filters(FilterDraft::new()), "filters"),),
            1,
            0,
        ),
        "empty page",
    )
    .entries()
    .is_empty());
    let _ = fs::remove_dir_all(root);
}

#[test]
#[allow(clippy::too_many_lines)]
fn cached_receipt_rows_are_labelled_only_by_the_verifier() {
    let (reference, canonical, authorised) = real_receipt_material();
    let root = support::directory("activity-receipt-rows");
    let map = support::tenancy(&[("alice", "tenant-a"), ("bob", "tenant-b")]);
    let (mut store, _) = support::install_and_open(&root, &map, support::retention_uniform(100));
    let alice = support::principal("alice");
    let bob = support::principal("bob");
    let domain = SettlementDomain::new(31_337, [0x55; 20]);
    let foreign_domain = SettlementDomain::new(1, [0x66; 20]);
    let entry_id = id("act_receiptrow1");
    let feed = text(Feed::new(5), "feed");
    let other_key = SigningKey::from_bytes(&[0x17; 32])
        .verifying_key()
        .to_bytes();
    let wrong_batch =
        AuthorizedBatch::new([0x14; 32], [0x15; 32], [0x12; 32], [0x13; 32], other_key);
    let mut authority = ReceiptAuthority::default();
    authority.insert(digest_hex(&reference), authorised);
    let mut scope = text(store.principal(&alice), "alice scope");

    let genuine = text(
        EvidenceBundle::receipt(
            entry_id.clone(),
            canonical.clone(),
            authorised,
            scope.principal(),
            domain,
        ),
        "genuine receipt bundle",
    );
    let genuine_bytes = text(genuine.encode(), "genuine bundle bytes");
    let (digest, cached) = cache_row(&mut scope, genuine_bytes.clone(), 1);
    assert_eq!(digest, text(genuine.digest(), "genuine digest"));
    assert_eq!(cached.settlement_domain(), domain);
    let status = verification_status(cached.verify(digest, scope.principal(), domain, &authority));
    assert!(status.is_receipt_verified());
    assert_eq!(status.label(), "receipt-verified");
    assert_eq!(status.unverified_reason(), None);
    assert_eq!(
        status.report().map(|report| report.verified_receipts()),
        Some(1)
    );
    assert_eq!(status.report().map(|report| report.entries()), Some(1));

    let loaded = text(cached.receipt_authority(feed, &scope), "feed authority");
    let status = verification_status(cached.verify(digest, scope.principal(), domain, &loaded));
    assert!(status.is_unavailable());
    assert!(!status.is_receipt_verified());
    assert_eq!(status.label(), "unavailable");
    assert!(matches!(
        cached.verify(digest, scope.principal(), domain, &loaded),
        Err(VerifyError::Unavailable(_))
    ));

    let mut altered_bytes = genuine_bytes.clone();
    let altered_at = unique_position(&altered_bytes, &canonical) + 8;
    altered_bytes[altered_at] ^= 0x01;
    text(
        scope.put(
            Table::Cache,
            evidence_row_key(digest),
            2,
            altered_bytes.clone(),
        ),
        "altered evidence row",
    );
    let altered = text(
        EvidenceBundle::decode(
            scope
                .get(Table::Cache, &evidence_row_key(digest))
                .unwrap_or_else(|| panic!("altered evidence row missing"))
                .bytes(),
        ),
        "altered row decode",
    );
    let status = verification_status(altered.verify(digest, scope.principal(), domain, &authority));
    assert!(!status.is_receipt_verified());
    assert_eq!(status.label(), "unverified");
    assert_eq!(
        status.unverified_reason(),
        Some(UnverifiedReason::DigestMismatch)
    );
    let (rekeyed_digest, rekeyed) = cache_row(&mut scope, altered_bytes, 3);
    let status =
        verification_status(rekeyed.verify(rekeyed_digest, scope.principal(), domain, &authority));
    assert_eq!(status.label(), "unverified");
    assert_eq!(status.unverified_reason(), Some(UnverifiedReason::Tampered));

    let bob_bytes = text(
        text(
            EvidenceBundle::receipt(
                entry_id.clone(),
                canonical.clone(),
                authorised,
                &bob,
                domain,
            ),
            "bob receipt bundle",
        )
        .encode(),
        "bob bundle bytes",
    );
    let (bob_digest, bob_row) = cache_row(&mut scope, bob_bytes, 4);
    let status =
        verification_status(bob_row.verify(bob_digest, scope.principal(), domain, &authority));
    assert_eq!(status.label(), "unverified");
    assert_eq!(
        status.unverified_reason(),
        Some(UnverifiedReason::PrincipalMismatch)
    );
    assert!(
        verification_status(bob_row.verify(bob_digest, &bob, domain, &authority))
            .is_receipt_verified()
    );

    let foreign_bytes = text(
        text(
            EvidenceBundle::receipt(
                entry_id.clone(),
                canonical.clone(),
                authorised,
                scope.principal(),
                foreign_domain,
            ),
            "foreign-domain receipt bundle",
        )
        .encode(),
        "foreign-domain bundle bytes",
    );
    let (foreign_digest, foreign_row) = cache_row(&mut scope, foreign_bytes, 5);
    let status = verification_status(foreign_row.verify(
        foreign_digest,
        scope.principal(),
        domain,
        &authority,
    ));
    assert_eq!(status.label(), "unverified");
    assert_eq!(
        status.unverified_reason(),
        Some(UnverifiedReason::SettlementDomainMismatch)
    );

    let wrong_bytes = text(
        text(
            EvidenceBundle::receipt(
                entry_id.clone(),
                canonical.clone(),
                wrong_batch,
                scope.principal(),
                domain,
            ),
            "wrong-authority receipt bundle",
        )
        .encode(),
        "wrong-authority bundle bytes",
    );
    let (wrong_digest, wrong_row) = cache_row(&mut scope, wrong_bytes, 6);
    let status =
        verification_status(wrong_row.verify(wrong_digest, scope.principal(), domain, &authority));
    assert_eq!(status.label(), "unverified");
    assert_eq!(
        status.unverified_reason(),
        Some(UnverifiedReason::AuthorityMismatch)
    );

    let report = text(
        EvidenceBundle::verify_receipt(
            entry_id.clone(),
            &canonical,
            &authorised,
            reference,
            scope.principal(),
            domain,
        ),
        "single receipt verification",
    );
    assert_eq!(report.verified_receipts(), 1);
    assert!(matches!(
        EvidenceBundle::verify_receipt(
            entry_id.clone(),
            &canonical,
            &wrong_batch,
            reference,
            scope.principal(),
            domain,
        ),
        Err(VerifyError::Unverified(UnverifiedReason::Tampered))
    ));
    let mut wrong_reference = reference;
    wrong_reference[0] ^= 0xff;
    assert!(matches!(
        EvidenceBundle::verify_receipt(
            entry_id,
            &canonical,
            &authorised,
            wrong_reference,
            scope.principal(),
            domain,
        ),
        Err(VerifyError::Unverified(UnverifiedReason::DigestMismatch))
    ));
    let _ = fs::remove_dir_all(root);
}

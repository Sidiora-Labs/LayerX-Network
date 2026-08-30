//! Principal-scoped, receipt-gated projection of Human and agent activity.

pub mod detail;
mod export;

pub use detail::EntryDetail;
pub use export::{
    BundleReport, EvidenceBundle, EvidenceEntry, EvidenceExport, ExportError, StatementExport,
};

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Display, Formatter, Write as _};

use layerx_agent_api::subscription::{EventDelivery, ReceiptReference};
use layerx_agent_api::track::SubmissionState;
use layerx_agent_api::verify::Level;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::approvals::{ApprovalEvent, InboxItem, InboxState};
use crate::audit::{AuditEvent, ChainEntry};
use crate::journeys::{JourneyProgress, JourneyState, JourneyStatus};
use crate::notify::ActivityEntryId;
use crate::store::{PrincipalScope, RowKey, StoreError, Table};

const STATE_KEY: &str = "activity-feed";
const STATE_VERSION: u8 = 1;
const CURSOR_VERSION: u8 = 1;
const MAXIMUM_PAGE_SIZE: usize = 100;
const MAXIMUM_REVISIONS: usize = 100_000;
const TEXT_LIMIT: usize = 128;
const CURSOR_BODY_LENGTH: usize = 81;
const CURSOR_LENGTH: usize = CURSOR_BODY_LENGTH + 32;
const CURSOR_DOMAIN: &[u8] = b"layerx-human-feed-cursor/v1";
const ENTRY_DOMAIN: &[u8] = b"layerx-human-feed-entry/v1";
const SOURCE_DOMAIN: &[u8] = b"layerx-human-feed-source/v1";

/// Every activity class required by the unified Human feed.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActivityKind {
    Deposit,
    Withdrawal,
    Movement,
    AgentAction,
    Approval,
    Security,
}

/// Deposit-specific progress retained in its one joined custody entry.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DepositStage {
    WaitingForWallet,
    ConfirmingOnPaxeer,
    Crediting,
    Done,
}

/// Withdrawal-specific progress retained in its one joined custody entry.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WithdrawalStage {
    Processing,
    WaitingForSettlement,
    ReadyToClaim,
    PaidOut,
}

/// Normative status vocabulary. Completion variants are accepted only with a
/// verified receipt by the ingestion methods; callers cannot append a row.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActivityStatus {
    GettingReady,
    Sending,
    Processing,
    StillChecking,
    WaitingForYou,
    Deposit(DepositStage),
    Withdrawal(WithdrawalStage),
    Done,
    DoneFinalised,
    DidntGoThrough { money_left: bool },
}

impl ActivityStatus {
    /// Exact Human label for the normative state.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::GettingReady => "Getting ready",
            Self::Sending => "Sending",
            Self::Processing | Self::Withdrawal(WithdrawalStage::Processing) => "Processing",
            Self::StillChecking => "Still checking — don't send again",
            Self::WaitingForYou => "Waiting for you",
            Self::Deposit(DepositStage::WaitingForWallet) => "Waiting for wallet",
            Self::Deposit(DepositStage::ConfirmingOnPaxeer) => "Confirming on Paxeer",
            Self::Deposit(DepositStage::Crediting) => "Crediting",
            Self::Deposit(DepositStage::Done) | Self::Done => "Done",
            Self::Withdrawal(WithdrawalStage::WaitingForSettlement) => "Waiting for settlement",
            Self::Withdrawal(WithdrawalStage::ReadyToClaim) => "Ready to claim",
            Self::Withdrawal(WithdrawalStage::PaidOut) => "Paid out",
            Self::DoneFinalised => "Done, finalised",
            Self::DidntGoThrough { money_left: true } => "Didn't go through — money already left",
            Self::DidntGoThrough { money_left: false } => "Didn't go through — no money left",
        }
    }

    const fn requires_receipt(self) -> bool {
        matches!(
            self,
            Self::Done
                | Self::DoneFinalised
                | Self::Deposit(DepositStage::Done)
                | Self::Withdrawal(WithdrawalStage::PaidOut)
        )
    }
}

/// A non-completion state supplied alongside a real agent delivery. The
/// closed type makes it impossible to claim Done before receipt evidence is
/// present on that delivery.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PendingStatus {
    GettingReady,
    Sending,
    Processing,
    StillChecking,
    WaitingForYou,
    Deposit(DepositStage),
    Withdrawal(WithdrawalStage),
    DidntGoThrough { money_left: bool },
}

impl PendingStatus {
    fn status(self) -> Result<ActivityStatus, FeedError> {
        let status = match self {
            Self::GettingReady => ActivityStatus::GettingReady,
            Self::Sending => ActivityStatus::Sending,
            Self::Processing => ActivityStatus::Processing,
            Self::StillChecking => ActivityStatus::StillChecking,
            Self::WaitingForYou => ActivityStatus::WaitingForYou,
            Self::Deposit(stage) => ActivityStatus::Deposit(stage),
            Self::Withdrawal(stage) => ActivityStatus::Withdrawal(stage),
            Self::DidntGoThrough { money_left } => ActivityStatus::DidntGoThrough { money_left },
        };
        if status.requires_receipt() {
            Err(FeedError::CompletionWithoutReceipt)
        } else {
            Ok(status)
        }
    }
}

/// How a verified agent receipt should present the completed activity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerifiedStatus {
    Done,
    DepositDone,
    WithdrawalPaidOut,
}

/// Source-specific outcome required for every refused movement. The Feed
/// never infers whether value moved from partial receipt presence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FundsDisposition {
    MoneyLeft,
    NoMoneyLeft,
}

/// Typed meaning joined to an exact real agent delivery.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentActivity {
    entry_id: ActivityEntryId,
    kind: ActivityKind,
    agent: Option<String>,
    occurred_at: u64,
    pending: PendingStatus,
    verified: VerifiedStatus,
}

impl AgentActivity {
    /// Describes the Human meaning already decoded at the agent contract
    /// boundary. Receipt state itself is never accepted here.
    ///
    /// # Errors
    ///
    /// Refuses invalid agent labels and custody completion presentations that
    /// do not match the activity class.
    pub fn new(
        entry_id: ActivityEntryId,
        kind: ActivityKind,
        agent: Option<String>,
        occurred_at: u64,
        pending: PendingStatus,
        verified: VerifiedStatus,
    ) -> Result<Self, FeedError> {
        validate_agent(agent.as_deref())?;
        if matches!(verified, VerifiedStatus::DepositDone) != matches!(kind, ActivityKind::Deposit)
            || matches!(verified, VerifiedStatus::WithdrawalPaidOut)
                != matches!(kind, ActivityKind::Withdrawal)
        {
            return Err(FeedError::InvalidActivity);
        }
        let _ = pending.status()?;
        Ok(Self {
            entry_id,
            kind,
            agent,
            occurred_at,
            pending,
            verified,
        })
    }
}

/// Receipt reference retained with the achieved verification level.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReceiptEvidence {
    reference: String,
    level: Level,
    canonical: Option<Vec<u8>>,
    authority: Option<layerx_proof::receipt::AuthorizedBatch>,
}

impl ReceiptEvidence {
    #[must_use]
    pub fn reference(&self) -> &str {
        &self.reference
    }

    #[must_use]
    pub const fn level(&self) -> Level {
        self.level
    }

    #[must_use]
    pub fn canonical(&self) -> Option<&[u8]> {
        self.canonical.as_deref()
    }
    #[must_use]
    pub const fn authority(&self) -> Option<&layerx_proof::receipt::AuthorizedBatch> {
        self.authority.as_ref()
    }
}

/// One current joined feed entry reconstructed at a snapshot revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivityEntry {
    entry_id: ActivityEntryId,
    revision: u64,
    kind: ActivityKind,
    agent: Option<String>,
    occurred_at: u64,
    projected_at: u64,
    status: ActivityStatus,
    receipts: Vec<ReceiptEvidence>,
}

impl ActivityEntry {
    #[must_use]
    pub const fn entry_id(&self) -> &ActivityEntryId {
        &self.entry_id
    }

    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    #[must_use]
    pub const fn kind(&self) -> ActivityKind {
        self.kind
    }

    #[must_use]
    pub fn agent(&self) -> Option<&str> {
        self.agent.as_deref()
    }

    #[must_use]
    pub const fn occurred_at(&self) -> u64 {
        self.occurred_at
    }

    #[must_use]
    pub const fn projected_at(&self) -> u64 {
        self.projected_at
    }

    #[must_use]
    pub const fn status(&self) -> ActivityStatus {
        self.status
    }

    #[must_use]
    pub fn receipts(&self) -> &[ReceiptEvidence] {
        &self.receipts
    }
}

/// Editable filter state. It does not affect a query until explicitly
/// converted to [`AppliedFilters`].
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FilterDraft {
    kinds: BTreeSet<ActivityKind>,
    agent: Option<String>,
    from: Option<u64>,
    through: Option<u64>,
}

impl FilterDraft {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            kinds: BTreeSet::new(),
            agent: None,
            from: None,
            through: None,
        }
    }

    #[must_use]
    pub fn with_kinds(mut self, kinds: impl IntoIterator<Item = ActivityKind>) -> Self {
        self.kinds = kinds.into_iter().collect();
        self
    }

    #[must_use]
    pub fn with_agent(mut self, agent: impl Into<String>) -> Self {
        self.agent = Some(agent.into());
        self
    }

    #[must_use]
    pub const fn with_dates(mut self, from: Option<u64>, through: Option<u64>) -> Self {
        self.from = from;
        self.through = through;
        self
    }
}

/// Immutable filter state echoed on every page.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AppliedFilters {
    kinds: BTreeSet<ActivityKind>,
    agent: Option<String>,
    from: Option<u64>,
    through: Option<u64>,
}

impl AppliedFilters {
    #[must_use]
    pub const fn kinds(&self) -> &BTreeSet<ActivityKind> {
        &self.kinds
    }

    #[must_use]
    pub fn agent(&self) -> Option<&str> {
        self.agent.as_deref()
    }

    #[must_use]
    pub const fn from(&self) -> Option<u64> {
        self.from
    }

    #[must_use]
    pub const fn through(&self) -> Option<u64> {
        self.through
    }
}

/// Opaque, principal- and filter-bound snapshot cursor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeedCursor(String);

impl FeedCursor {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Parses an opaque cursor. Its principal and filter bindings are checked
    /// when the cursor is used.
    ///
    /// # Errors
    ///
    /// Refuses malformed or altered cursor bytes.
    pub fn parse(value: impl Into<String>) -> Result<Self, FeedError> {
        let value = value.into();
        let bytes = decode_hex(&value).ok_or(FeedError::InvalidCursor)?;
        decode_cursor(&bytes)?;
        Ok(Self(value))
    }
}

/// One stable pagination request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PageRequest {
    limit: usize,
    cursor: Option<FeedCursor>,
    filters: AppliedFilters,
}

impl PageRequest {
    #[must_use]
    pub const fn new(limit: usize, filters: AppliedFilters) -> Self {
        Self {
            limit,
            cursor: None,
            filters,
        }
    }

    #[must_use]
    pub fn after(mut self, cursor: FeedCursor) -> Self {
        self.cursor = Some(cursor);
        self
    }
}

/// Source freshness carried by every response, including an empty page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FeedFreshness {
    projected_at: Option<u64>,
    age_seconds: Option<u64>,
    freshness_bound_seconds: u64,
    agent_cursor: Option<u64>,
    observed_agent_head: u64,
    agent_lag: u64,
    current: bool,
}

impl FeedFreshness {
    #[must_use]
    pub const fn projected_at(self) -> Option<u64> {
        self.projected_at
    }

    #[must_use]
    pub const fn age_seconds(self) -> Option<u64> {
        self.age_seconds
    }

    #[must_use]
    pub const fn freshness_bound_seconds(self) -> u64 {
        self.freshness_bound_seconds
    }

    #[must_use]
    pub const fn agent_cursor(self) -> Option<u64> {
        self.agent_cursor
    }

    #[must_use]
    pub const fn observed_agent_head(self) -> u64 {
        self.observed_agent_head
    }

    #[must_use]
    pub const fn agent_lag(self) -> u64 {
        self.agent_lag
    }

    #[must_use]
    pub const fn is_current(self) -> bool {
        self.current
    }
}

/// One snapshot-stable page with exact applied filter state and freshness.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeedPage {
    entries: Vec<ActivityEntry>,
    next: Option<FeedCursor>,
    applied_filters: AppliedFilters,
    freshness: FeedFreshness,
}

impl FeedPage {
    #[must_use]
    pub fn entries(&self) -> &[ActivityEntry] {
        &self.entries
    }

    #[must_use]
    pub const fn next(&self) -> Option<&FeedCursor> {
        self.next.as_ref()
    }

    #[must_use]
    pub const fn applied_filters(&self) -> &AppliedFilters {
        &self.applied_filters
    }

    #[must_use]
    pub const fn freshness(&self) -> FeedFreshness {
        self.freshness
    }
}

/// Principal-scoped unified activity projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Feed {
    freshness_bound_seconds: u64,
}

impl Feed {
    /// Creates a projection reader/writer with an explicit freshness bound.
    ///
    /// # Errors
    ///
    /// Refuses a zero bound.
    pub const fn new(freshness_bound_seconds: u64) -> Result<Self, FeedError> {
        if freshness_bound_seconds == 0 {
            Err(FeedError::InvalidFreshnessBound)
        } else {
            Ok(Self {
                freshness_bound_seconds,
            })
        }
    }

    /// Loads one current entry from the authenticated principal projection.
    ///
    /// # Errors
    ///
    /// Refuses corrupt durable projection state.
    pub fn entry(
        self,
        scope: &PrincipalScope<'_>,
        entry_id: &ActivityEntryId,
    ) -> Result<Option<ActivityEntry>, FeedError> {
        let state = load_state(scope)?;
        let current = state
            .revisions
            .iter()
            .filter(|revision| revision.entry_id == entry_id.as_str())
            .max_by_key(|revision| revision.revision);
        current.map(activity_entry).transpose()
    }

    /// Returns the last agent cursor durably consumed by this projection.
    /// This is the only source head against which a local feed page can claim
    /// currency without consulting a newer agent-layer read.
    pub fn projected_agent_head(self, scope: &PrincipalScope<'_>) -> Result<u64, FeedError> {
        Ok(load_state(scope)?.last_agent_cursor.unwrap_or(0))
    }

    /// Applies a draft atomically. Until this call, changes to the draft have
    /// no effect on page requests.
    ///
    /// # Errors
    ///
    /// Refuses invalid agent and date bounds.
    pub fn apply_filters(draft: FilterDraft) -> Result<AppliedFilters, FeedError> {
        validate_agent(draft.agent.as_deref())?;
        if draft
            .from
            .zip(draft.through)
            .is_some_and(|(from, through)| through < from)
        {
            return Err(FeedError::InvalidDateRange);
        }
        Ok(AppliedFilters {
            kinds: draft.kinds,
            agent: draft.agent,
            from: draft.from,
            through: draft.through,
        })
    }

    /// Projects one real agent-contract delivery. A verified delivery becomes
    /// a completion immediately; an unverified delivery can only use the
    /// descriptor's non-completion state.
    ///
    /// # Errors
    ///
    /// Refuses event gaps, unverified receipt references, corrupt state and
    /// persistence failures.
    pub fn record_agent_event(
        scope: &mut PrincipalScope<'_>,
        activity: &AgentActivity,
        delivery: &EventDelivery,
        projected_at: u64,
    ) -> Result<ActivityEntry, FeedError> {
        let cursor = delivery.cursor.0 .0;
        if cursor == 0
            || delivery.event_bytes.is_empty()
            || delivery.deduplication_id.as_bytes()
                != layerx_agent_api::subscription::DeduplicationId::from_event_identity(
                    delivery.event_identity,
                )
                .as_bytes()
        {
            return Err(FeedError::InvalidAgentEvent);
        }
        let (status, receipts) = match &delivery.receipt_reference {
            ReceiptReference::None => (activity.pending.status()?, Vec::new()),
            ReceiptReference::Verified {
                receipt_ref,
                verification_level,
            } => {
                if *verification_level == Level::Unverified {
                    return Err(FeedError::UnverifiedReceipt);
                }
                let status = match activity.verified {
                    VerifiedStatus::DepositDone => ActivityStatus::Deposit(DepositStage::Done),
                    VerifiedStatus::WithdrawalPaidOut => {
                        ActivityStatus::Withdrawal(WithdrawalStage::PaidOut)
                    }
                    VerifiedStatus::Done if *verification_level >= Level::CheckpointFinalised => {
                        ActivityStatus::DoneFinalised
                    }
                    VerifiedStatus::Done => ActivityStatus::Done,
                };
                (
                    status,
                    vec![StoredReceipt {
                        reference: receipt_ref.as_str().to_owned(),
                        level: level_code(*verification_level),
                        canonical: None,
                        authority: None,
                    }],
                )
            }
        };
        let receipt_source_digest = match &delivery.receipt_reference {
            ReceiptReference::None => source_id(&[b"no-receipt"]),
            ReceiptReference::Verified {
                receipt_ref,
                verification_level,
            } => source_id(&[
                b"verified-receipt".as_slice(),
                receipt_ref.as_str().as_bytes(),
                &[level_code(*verification_level)],
            ]),
        };
        let candidate = Candidate {
            entry_id: activity.entry_id.as_str().to_owned(),
            kind: activity.kind,
            agent: activity.agent.clone(),
            occurred_at: activity.occurred_at,
            status,
            receipts,
            source_id: delivery.event_identity.as_bytes(),
            source_digest: source_id(&[
                b"agent-delivery",
                &delivery.event_bytes,
                &delivery.cursor.0 .0.to_be_bytes(),
                &receipt_source_digest,
            ]),
            agent_cursor: Some(cursor),
        };
        append(scope, candidate, projected_at)
    }

    /// Joins the current state of a real receipt-verifying `JourneyEngine` into
    /// the entry named by its stable journey identifier.
    ///
    /// # Errors
    ///
    /// Refuses mismatched progress, unsupported journey classes and any Done
    /// state without receipt digests.
    pub fn record_journey(
        scope: &mut PrincipalScope<'_>,
        kind: ActivityKind,
        status: &JourneyStatus,
        progress: &JourneyProgress,
        agent: Option<String>,
        refusal: Option<FundsDisposition>,
        projected_at: u64,
    ) -> Result<ActivityEntry, FeedError> {
        if !matches!(
            kind,
            ActivityKind::Deposit
                | ActivityKind::Withdrawal
                | ActivityKind::Movement
                | ActivityKind::AgentAction
        ) || progress.journey_id() != status.journey_id().as_str()
            || status.phases().get(progress.leg()) != Some(&progress.phase())
        {
            return Err(FeedError::InvalidActivity);
        }
        validate_agent(agent.as_deref())?;
        let receipts: Vec<StoredReceipt> = status
            .receipt_digests()
            .iter()
            .zip(status.receipt_material())
            .zip(status.receipt_authorities())
            .filter_map(|((digest, material), authority)| {
                digest.map(|digest| StoredReceipt {
                    reference: hex(&digest),
                    level: level_code(Level::SequencerSigned),
                    canonical: material.clone(),
                    authority: authority.as_ref().map(StoredReceiptAuthority::from_public),
                })
            })
            .collect();
        let activity_status = journey_status(kind, status, refusal)?;
        validate_completion(activity_status, &receipts)?;
        let entry_id = stable_entry_id(status.journey_id().as_str())?;
        let source_identity = source_id(&[
            status.journey_id().as_str().as_bytes(),
            &progress.sequence().to_be_bytes(),
        ]);
        append(
            scope,
            Candidate {
                entry_id: entry_id.as_str().to_owned(),
                kind,
                agent,
                occurred_at: progress.observed_at(),
                status: activity_status,
                receipts,
                source_id: source_identity,
                source_digest: source_id(&[
                    b"journey-progress".as_slice(),
                    status.journey_id().as_str().as_bytes(),
                    &progress.sequence().to_be_bytes(),
                    &[journey_phase_code(progress.phase())],
                ]),
                agent_cursor: None,
            },
            projected_at,
        )
    }

    /// Projects one real approval-module lifecycle item.
    ///
    /// # Errors
    ///
    /// Refuses a mismatched event/item pair, an unverified executed approval,
    /// corrupt state and persistence failures.
    pub fn record_approval(
        scope: &mut PrincipalScope<'_>,
        event: &ApprovalEvent,
        item: &InboxItem,
        projected_at: u64,
    ) -> Result<ActivityEntry, FeedError> {
        let expected = format!("apr_{}", hex(event.approval_id));
        if item.approval_id().as_str() != expected
            || item.disclosure_digest() != event.disclosure_digest
        {
            return Err(FeedError::InvalidActivity);
        }
        validate_agent(Some(item.agent()))?;
        let mut receipts = Vec::new();
        let status = match item.state() {
            InboxState::AwaitingApproval => ActivityStatus::WaitingForYou,
            InboxState::Rejected | InboxState::Expired => {
                ActivityStatus::DidntGoThrough { money_left: false }
            }
            InboxState::Approved { tracking } => match &tracking.state {
                SubmissionState::Prepared | SubmissionState::Signed => ActivityStatus::GettingReady,
                SubmissionState::Queued | SubmissionState::Submitted => ActivityStatus::Sending,
                SubmissionState::Acknowledged => ActivityStatus::Processing,
                SubmissionState::Unknown => ActivityStatus::StillChecking,
                SubmissionState::Executed { receipt_ref } => {
                    if tracking.verification_level == Level::Unverified {
                        ActivityStatus::Processing
                    } else {
                        receipts.push(StoredReceipt {
                            reference: receipt_ref.as_str().to_owned(),
                            level: level_code(tracking.verification_level),
                            canonical: None,
                            authority: None,
                        });
                        if tracking.verification_level >= Level::CheckpointFinalised {
                            ActivityStatus::DoneFinalised
                        } else {
                            ActivityStatus::Done
                        }
                    }
                }
                SubmissionState::Failed { .. } | SubmissionState::Expired => {
                    ActivityStatus::DidntGoThrough { money_left: false }
                }
            },
        };
        validate_completion(status, &receipts)?;
        let entry_id = ActivityEntryId::new(format!("act_{}", hex(event.approval_id)))?;
        let source_identity = source_id(&[
            &event.approval_id,
            &event.sequence.to_be_bytes(),
            &[event_kind_code(event)],
        ]);
        append(
            scope,
            Candidate {
                entry_id: entry_id.as_str().to_owned(),
                kind: ActivityKind::Approval,
                agent: Some(item.agent().to_owned()),
                occurred_at: event.observed_at,
                status,
                receipts,
                source_id: source_identity,
                source_digest: source_id(&[
                    b"approval-event".as_slice(),
                    &event.approval_id,
                    &event.disclosure_digest,
                    &event.sequence.to_be_bytes(),
                    &[event_kind_code(event)],
                ]),
                agent_cursor: None,
            },
            projected_at,
        )
    }

    /// Projects a security change from a verified, hash-chained audit entry.
    /// It remains Processing until a matching agent receipt updates the same
    /// returned entry identifier.
    ///
    /// # Errors
    ///
    /// Refuses non-security audit entries and persistence failures.
    pub fn record_security(
        scope: &mut PrincipalScope<'_>,
        entry: &ChainEntry,
        projected_at: u64,
    ) -> Result<ActivityEntry, FeedError> {
        if !matches!(entry.event(), AuditEvent::SecurityChange { .. }) {
            return Err(FeedError::InvalidActivity);
        }
        let entry_id = ActivityEntryId::new(format!("act_{}", hex(entry.link())))?;
        append(
            scope,
            Candidate {
                entry_id: entry_id.as_str().to_owned(),
                kind: ActivityKind::Security,
                agent: None,
                occurred_at: entry.recorded_at(),
                status: ActivityStatus::Processing,
                receipts: Vec::new(),
                source_id: entry.link(),
                source_digest: source_id(&[b"audit-entry", entry.bytes()]),
                agent_cursor: None,
            },
            projected_at,
        )
    }

    /// Returns one snapshot-stable page. New rows and updates after the first
    /// page's upper revision are invisible to every continuation of that page.
    ///
    /// # Errors
    ///
    /// Refuses invalid page sizes, cross-principal/filter cursors, corrupt
    /// durable state and a claimed source head behind the consumed cursor.
    pub fn page(
        self,
        scope: &PrincipalScope<'_>,
        request: PageRequest,
        now: u64,
        observed_agent_head: u64,
    ) -> Result<FeedPage, FeedError> {
        if request.limit == 0 || request.limit > MAXIMUM_PAGE_SIZE {
            return Err(FeedError::InvalidPageSize);
        }
        let state = load_state(scope)?;
        let principal = principal_digest(scope);
        let filter = filter_digest(&request.filters)?;
        let (upper, after) = if let Some(cursor) = &request.cursor {
            let decoded =
                decode_cursor(&decode_hex(cursor.as_str()).ok_or(FeedError::InvalidCursor)?)?;
            if decoded.principal != principal || decoded.filter != filter {
                return Err(FeedError::CursorScopeMismatch);
            }
            if decoded.upper > state.next_revision {
                return Err(FeedError::InvalidCursor);
            }
            (decoded.upper, Some(decoded.after))
        } else {
            (state.next_revision, None)
        };
        if state
            .last_agent_cursor
            .is_some_and(|cursor| cursor > observed_agent_head)
        {
            return Err(FeedError::SourceHeadRegressed);
        }
        let mut current = BTreeMap::<String, &StoredRevision>::new();
        for revision in state.revisions.iter().filter(|item| item.revision < upper) {
            current.insert(revision.entry_id.clone(), revision);
        }
        let mut entries: Vec<&StoredRevision> = current
            .values()
            .copied()
            .filter(|item| after.is_none_or(|after| item.revision < after))
            .filter(|item| matches_filters(item, &request.filters))
            .collect();
        entries.sort_by(|left, right| {
            right
                .revision
                .cmp(&left.revision)
                .then_with(|| right.entry_id.cmp(&left.entry_id))
        });
        let has_more = entries.len() > request.limit;
        entries.truncate(request.limit);
        let owned: Vec<ActivityEntry> = entries
            .into_iter()
            .map(activity_entry)
            .collect::<Result<_, _>>()?;
        let next = if has_more {
            let after = owned
                .last()
                .map(ActivityEntry::revision)
                .ok_or(FeedError::CorruptState)?;
            Some(encode_cursor(CursorFields {
                principal,
                filter,
                upper,
                after,
            }))
        } else {
            None
        };
        let age_seconds = state
            .last_projected_at
            .map(|projected_at| now.saturating_sub(projected_at));
        let agent_lag = observed_agent_head.saturating_sub(state.last_agent_cursor.unwrap_or(0));
        let current =
            agent_lag == 0 && age_seconds.is_none_or(|age| age <= self.freshness_bound_seconds);
        Ok(FeedPage {
            entries: owned,
            next,
            applied_filters: request.filters,
            freshness: FeedFreshness {
                projected_at: state.last_projected_at,
                age_seconds,
                freshness_bound_seconds: self.freshness_bound_seconds,
                agent_cursor: state.last_agent_cursor,
                observed_agent_head,
                agent_lag,
                current,
            },
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct FeedState {
    version: u8,
    next_revision: u64,
    last_projected_at: Option<u64>,
    last_agent_cursor: Option<u64>,
    revisions: Vec<StoredRevision>,
}

impl Default for FeedState {
    fn default() -> Self {
        Self {
            version: STATE_VERSION,
            next_revision: 0,
            last_projected_at: None,
            last_agent_cursor: None,
            revisions: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct StoredRevision {
    revision: u64,
    entry_id: String,
    kind: ActivityKind,
    agent: Option<String>,
    occurred_at: u64,
    projected_at: u64,
    status: ActivityStatus,
    receipts: Vec<StoredReceipt>,
    source_id: [u8; 32],
    source_digest: [u8; 32],
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct StoredReceipt {
    reference: String,
    level: u8,
    #[serde(default)]
    canonical: Option<Vec<u8>>,
    #[serde(default)]
    authority: Option<StoredReceiptAuthority>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
struct StoredReceiptAuthority {
    batch_id: [u8; 32],
    asset: [u8; 32],
    previous_state_root: [u8; 32],
    resulting_state_root: [u8; 32],
    sequencer_public_key: [u8; 32],
}
impl StoredReceiptAuthority {
    fn from_public(value: &layerx_proof::receipt::AuthorizedBatch) -> Self {
        Self {
            batch_id: value.batch_id(),
            asset: value.asset(),
            previous_state_root: value.previous_state_root(),
            resulting_state_root: value.resulting_state_root(),
            sequencer_public_key: value.sequencer_public_key(),
        }
    }
}

struct Candidate {
    entry_id: String,
    kind: ActivityKind,
    agent: Option<String>,
    occurred_at: u64,
    status: ActivityStatus,
    receipts: Vec<StoredReceipt>,
    source_id: [u8; 32],
    source_digest: [u8; 32],
    agent_cursor: Option<u64>,
}

fn append(
    scope: &mut PrincipalScope<'_>,
    candidate: Candidate,
    projected_at: u64,
) -> Result<ActivityEntry, FeedError> {
    let mut state = load_state(scope)?;
    if state
        .last_projected_at
        .is_some_and(|previous| projected_at < previous)
    {
        return Err(FeedError::TimeRegressed);
    }
    if let Some(existing) = state
        .revisions
        .iter()
        .find(|revision| revision.source_id == candidate.source_id)
    {
        if existing.entry_id != candidate.entry_id
            || existing.source_digest != candidate.source_digest
        {
            return Err(FeedError::SourceCollision);
        }
        let current = state
            .revisions
            .iter()
            .rev()
            .find(|revision| revision.entry_id == candidate.entry_id)
            .ok_or(FeedError::CorruptState)?;
        return activity_entry(current);
    }
    if let Some(cursor) = candidate.agent_cursor {
        if state
            .last_agent_cursor
            .is_some_and(|last| cursor != last.saturating_add(1))
        {
            return Err(FeedError::AgentEventGap);
        }
        state.last_agent_cursor = Some(cursor);
    }
    if state.revisions.len() >= MAXIMUM_REVISIONS {
        return Err(FeedError::CapacityExceeded);
    }
    validate_completion(candidate.status, &candidate.receipts)?;
    let revision = StoredRevision {
        revision: state.next_revision,
        entry_id: candidate.entry_id,
        kind: candidate.kind,
        agent: candidate.agent,
        occurred_at: candidate.occurred_at,
        projected_at,
        status: candidate.status,
        receipts: candidate.receipts,
        source_id: candidate.source_id,
        source_digest: candidate.source_digest,
    };
    validate_revision(&revision)?;
    state.next_revision = state
        .next_revision
        .checked_add(1)
        .ok_or(FeedError::SequenceOverflow)?;
    state.last_projected_at = Some(projected_at);
    state.revisions.push(revision);
    let bytes = serde_json::to_vec(&state).map_err(|_| FeedError::CorruptState)?;
    scope.put(
        Table::Journeys,
        RowKey::new(STATE_KEY)?,
        projected_at,
        bytes,
    )?;
    activity_entry(state.revisions.last().ok_or(FeedError::CorruptState)?)
}

fn load_state(scope: &PrincipalScope<'_>) -> Result<FeedState, FeedError> {
    let key = RowKey::new(STATE_KEY)?;
    let Some(row) = scope.get(Table::Journeys, &key) else {
        return Ok(FeedState::default());
    };
    let state: FeedState =
        serde_json::from_slice(row.bytes()).map_err(|_| FeedError::CorruptState)?;
    validate_state(&state)?;
    Ok(state)
}

fn validate_state(state: &FeedState) -> Result<(), FeedError> {
    if state.version != STATE_VERSION
        || state.revisions.len() > MAXIMUM_REVISIONS
        || usize::try_from(state.next_revision).ok() != Some(state.revisions.len())
        || state.revisions.iter().enumerate().any(|(index, revision)| {
            u64::try_from(index).ok() != Some(revision.revision)
                || validate_revision(revision).is_err()
        })
    {
        return Err(FeedError::CorruptState);
    }
    Ok(())
}

fn validate_revision(revision: &StoredRevision) -> Result<(), FeedError> {
    ActivityEntryId::new(revision.entry_id.clone())?;
    validate_agent(revision.agent.as_deref())?;
    if revision.source_id == [0; 32]
        || revision.source_digest == [0; 32]
        || revision
            .receipts
            .iter()
            .any(|receipt| receipt.reference.is_empty() || level(receipt.level).is_none())
    {
        return Err(FeedError::CorruptState);
    }
    validate_completion(revision.status, &revision.receipts)
}

fn validate_completion(
    status: ActivityStatus,
    receipts: &[StoredReceipt],
) -> Result<(), FeedError> {
    if status.requires_receipt()
        && (receipts.is_empty()
            || receipts
                .iter()
                .any(|receipt| receipt.level == level_code(Level::Unverified)))
    {
        Err(FeedError::CompletionWithoutReceipt)
    } else {
        Ok(())
    }
}

fn activity_entry(revision: &StoredRevision) -> Result<ActivityEntry, FeedError> {
    let receipts = revision
        .receipts
        .iter()
        .map(|receipt| {
            Ok(ReceiptEvidence {
                reference: receipt.reference.clone(),
                level: level(receipt.level).ok_or(FeedError::CorruptState)?,
                canonical: receipt.canonical.clone(),
                authority: receipt.authority.as_ref().map(|value| {
                    layerx_proof::receipt::AuthorizedBatch::new(
                        value.batch_id,
                        value.asset,
                        value.previous_state_root,
                        value.resulting_state_root,
                        value.sequencer_public_key,
                    )
                }),
            })
        })
        .collect::<Result<_, FeedError>>()?;
    Ok(ActivityEntry {
        entry_id: ActivityEntryId::new(revision.entry_id.clone())?,
        revision: revision.revision,
        kind: revision.kind,
        agent: revision.agent.clone(),
        occurred_at: revision.occurred_at,
        projected_at: revision.projected_at,
        status: revision.status,
        receipts,
    })
}

fn journey_status(
    kind: ActivityKind,
    status: &JourneyStatus,
    refusal: Option<FundsDisposition>,
) -> Result<ActivityStatus, FeedError> {
    use crate::journeys::JourneyPhase;

    let phase = status
        .phases()
        .get(status.current_leg())
        .copied()
        .ok_or(FeedError::InvalidActivity)?;
    let mapped = match (kind, status.state(), phase) {
        (_, JourneyState::Refused, _) => ActivityStatus::DidntGoThrough {
            money_left: match refusal.ok_or(FeedError::MissingFundsDisposition)? {
                FundsDisposition::MoneyLeft => true,
                FundsDisposition::NoMoneyLeft => false,
            },
        },
        (ActivityKind::Deposit, JourneyState::Done, _) => {
            ActivityStatus::Deposit(DepositStage::Done)
        }
        (ActivityKind::Withdrawal, JourneyState::Done, _) => {
            ActivityStatus::Withdrawal(WithdrawalStage::PaidOut)
        }
        (_, JourneyState::Done, _) => ActivityStatus::Done,
        (
            ActivityKind::Deposit,
            _,
            JourneyPhase::Compiled | JourneyPhase::Preparing | JourneyPhase::Prepared,
        ) => ActivityStatus::Deposit(DepositStage::WaitingForWallet),
        (ActivityKind::Deposit, _, JourneyPhase::Signed | JourneyPhase::Submitted) => {
            ActivityStatus::Deposit(DepositStage::ConfirmingOnPaxeer)
        }
        (ActivityKind::Deposit, _, JourneyPhase::ReceiptVerified) => {
            ActivityStatus::Deposit(DepositStage::Crediting)
        }
        (
            ActivityKind::Withdrawal,
            _,
            JourneyPhase::Compiled | JourneyPhase::Preparing | JourneyPhase::Prepared,
        ) => ActivityStatus::Withdrawal(WithdrawalStage::Processing),
        (ActivityKind::Withdrawal, _, JourneyPhase::Signed | JourneyPhase::Submitted) => {
            ActivityStatus::Sending
        }
        (ActivityKind::Withdrawal, _, JourneyPhase::ReceiptVerified) => {
            ActivityStatus::Withdrawal(WithdrawalStage::WaitingForSettlement)
        }
        (_, _, JourneyPhase::Compiled | JourneyPhase::Preparing | JourneyPhase::Prepared) => {
            ActivityStatus::GettingReady
        }
        (_, _, JourneyPhase::Signed | JourneyPhase::Submitted) => ActivityStatus::Sending,
        (_, _, JourneyPhase::ReceiptVerified) => ActivityStatus::Processing,
        (_, _, JourneyPhase::StillChecking) => ActivityStatus::StillChecking,
        (_, _, JourneyPhase::Refused) => return Err(FeedError::MissingFundsDisposition),
    };
    Ok(mapped)
}

fn matches_filters(revision: &StoredRevision, filters: &AppliedFilters) -> bool {
    (filters.kinds.is_empty() || filters.kinds.contains(&revision.kind))
        && filters
            .agent
            .as_deref()
            .is_none_or(|agent| revision.agent.as_deref() == Some(agent))
        && filters.from.is_none_or(|from| revision.occurred_at >= from)
        && filters
            .through
            .is_none_or(|through| revision.occurred_at <= through)
}

fn validate_agent(agent: Option<&str>) -> Result<(), FeedError> {
    if agent.is_some_and(|value| {
        value.is_empty()
            || value.len() > TEXT_LIMIT
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'/' | b'\\'))
    }) {
        Err(FeedError::InvalidAgent)
    } else {
        Ok(())
    }
}

fn stable_entry_id(value: &str) -> Result<ActivityEntryId, FeedError> {
    let digest = source_id(&[ENTRY_DOMAIN, value.as_bytes()]);
    Ok(ActivityEntryId::new(format!("act_{}", hex(digest)))?)
}

fn source_id(parts: &[&[u8]]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(SOURCE_DOMAIN);
    for part in parts {
        digest.update(u64::try_from(part.len()).unwrap_or(u64::MAX).to_be_bytes());
        digest.update(part);
    }
    digest.finalize().into()
}

fn principal_digest(scope: &PrincipalScope<'_>) -> [u8; 32] {
    source_id(&[
        b"principal",
        scope.principal().as_str().as_bytes(),
        scope.tenant().as_str().as_bytes(),
    ])
}

fn filter_digest(filters: &AppliedFilters) -> Result<[u8; 32], FeedError> {
    let bytes = serde_json::to_vec(filters).map_err(|_| FeedError::CorruptState)?;
    Ok(source_id(&[b"filters", &bytes]))
}

#[derive(Clone, Copy)]
struct CursorFields {
    principal: [u8; 32],
    filter: [u8; 32],
    upper: u64,
    after: u64,
}

fn encode_cursor(fields: CursorFields) -> FeedCursor {
    let mut bytes = Vec::with_capacity(CURSOR_LENGTH);
    bytes.push(CURSOR_VERSION);
    bytes.extend_from_slice(&fields.principal);
    bytes.extend_from_slice(&fields.filter);
    bytes.extend_from_slice(&fields.upper.to_be_bytes());
    bytes.extend_from_slice(&fields.after.to_be_bytes());
    let mut digest = Sha256::new();
    digest.update(CURSOR_DOMAIN);
    digest.update(&bytes);
    bytes.extend_from_slice(&digest.finalize());
    FeedCursor(hex(&bytes))
}

fn decode_cursor(bytes: &[u8]) -> Result<CursorFields, FeedError> {
    if bytes.len() != CURSOR_LENGTH || bytes[0] != CURSOR_VERSION {
        return Err(FeedError::InvalidCursor);
    }
    let mut digest = Sha256::new();
    digest.update(CURSOR_DOMAIN);
    digest.update(&bytes[..CURSOR_BODY_LENGTH]);
    if digest.finalize().as_slice() != &bytes[CURSOR_BODY_LENGTH..] {
        return Err(FeedError::InvalidCursor);
    }
    let principal = bytes[1..33]
        .try_into()
        .map_err(|_| FeedError::InvalidCursor)?;
    let filter = bytes[33..65]
        .try_into()
        .map_err(|_| FeedError::InvalidCursor)?;
    let upper = u64::from_be_bytes(
        bytes[65..73]
            .try_into()
            .map_err(|_| FeedError::InvalidCursor)?,
    );
    let after = u64::from_be_bytes(
        bytes[73..81]
            .try_into()
            .map_err(|_| FeedError::InvalidCursor)?,
    );
    if after >= upper {
        return Err(FeedError::InvalidCursor);
    }
    Ok(CursorFields {
        principal,
        filter,
        upper,
        after,
    })
}

const fn level_code(level: Level) -> u8 {
    match level {
        Level::Unverified => 0,
        Level::SequencerSigned => 1,
        Level::BatchIncluded => 2,
        Level::StateProven => 3,
        Level::CheckpointFinalised => 4,
        Level::SettlementAnchored => 5,
    }
}

const fn level(code: u8) -> Option<Level> {
    match code {
        0 => Some(Level::Unverified),
        1 => Some(Level::SequencerSigned),
        2 => Some(Level::BatchIncluded),
        3 => Some(Level::StateProven),
        4 => Some(Level::CheckpointFinalised),
        5 => Some(Level::SettlementAnchored),
        _ => None,
    }
}

fn event_kind_code(event: &ApprovalEvent) -> u8 {
    use crate::approvals::ApprovalEventKind;
    match event.kind {
        ApprovalEventKind::Created => 1,
        ApprovalEventKind::Approved => 2,
        ApprovalEventKind::Rejected => 3,
        ApprovalEventKind::Expired => 4,
    }
}

const fn journey_phase_code(phase: crate::journeys::JourneyPhase) -> u8 {
    use crate::journeys::JourneyPhase;
    match phase {
        JourneyPhase::Compiled => 1,
        JourneyPhase::Preparing => 2,
        JourneyPhase::Prepared => 3,
        JourneyPhase::Signed => 4,
        JourneyPhase::Submitted => 5,
        JourneyPhase::StillChecking => 6,
        JourneyPhase::ReceiptVerified => 7,
        JourneyPhase::Refused => 8,
    }
}

fn hex(bytes: impl AsRef<[u8]>) -> String {
    let bytes = bytes.as_ref();
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if !value.len().is_multiple_of(2) {
        return None;
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = hex_digit(pair[0])?;
            let low = hex_digit(pair[1])?;
            Some((high << 4) | low)
        })
        .collect()
}

const fn hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

/// Typed feed failures. No failure path changes a durable cursor or appends a
/// partial revision.
#[derive(Debug)]
pub enum FeedError {
    Store(StoreError),
    Notify(crate::notify::NotifyError),
    InvalidFreshnessBound,
    InvalidActivity,
    InvalidAgent,
    InvalidAgentEvent,
    InvalidDateRange,
    InvalidPageSize,
    InvalidCursor,
    CursorScopeMismatch,
    CompletionWithoutReceipt,
    MissingFundsDisposition,
    UnverifiedReceipt,
    AgentEventGap,
    SourceHeadRegressed,
    SourceCollision,
    TimeRegressed,
    SequenceOverflow,
    CapacityExceeded,
    CorruptState,
}

impl Display for FeedError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "activity store failure: {error}"),
            Self::Notify(error) => write!(formatter, "activity identifier failure: {error}"),
            Self::InvalidFreshnessBound => formatter.write_str("freshness bound must be non-zero"),
            Self::InvalidActivity => formatter.write_str("activity source does not match its type"),
            Self::InvalidAgent => formatter.write_str("activity agent identifier is invalid"),
            Self::InvalidAgentEvent => formatter.write_str("agent event contract is invalid"),
            Self::InvalidDateRange => formatter.write_str("activity date range is inverted"),
            Self::InvalidPageSize => formatter.write_str("activity page size is outside bounds"),
            Self::InvalidCursor => formatter.write_str("activity cursor is invalid"),
            Self::CursorScopeMismatch => {
                formatter.write_str("activity cursor belongs to another scope or filter")
            }
            Self::CompletionWithoutReceipt => {
                formatter.write_str("activity completion requires a verified LayerX receipt")
            }
            Self::MissingFundsDisposition => {
                formatter.write_str("refused activity requires an explicit funds disposition")
            }
            Self::UnverifiedReceipt => formatter.write_str("activity receipt is unverified"),
            Self::AgentEventGap => formatter.write_str("agent activity stream has a gap"),
            Self::SourceHeadRegressed => {
                formatter.write_str("observed agent head is behind the projected cursor")
            }
            Self::SourceCollision => formatter.write_str("activity source identity collided"),
            Self::TimeRegressed => formatter.write_str("activity projection time regressed"),
            Self::SequenceOverflow => formatter.write_str("activity revision sequence overflowed"),
            Self::CapacityExceeded => formatter.write_str("activity revision capacity exceeded"),
            Self::CorruptState => formatter.write_str("activity projection state is corrupt"),
        }
    }
}

impl std::error::Error for FeedError {}

impl From<StoreError> for FeedError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<crate::notify::NotifyError> for FeedError {
    fn from(value: crate::notify::NotifyError) -> Self {
        Self::Notify(value)
    }
}

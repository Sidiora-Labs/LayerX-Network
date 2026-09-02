//! Explicit global-sequence gaps, core backfill reports and retention loss.

use std::fmt::{Display, Formatter};

use layerx_agent_api::subscription::SubscriptionTarget;
use layerx_client::head::Head;
use layerx_client::lni::transport::FrameTransport;
use layerx_client::stream::{
    subscribe, Cursor as ClientCursor, StreamConfig, StreamError, StreamItem,
};

use super::ingestion::durable_event;
use super::subscription::{Continuity, Store as SubscriptionStore, SubscriptionError};
use crate::session::{SessionRegistry, Token};
use crate::tenant::{Operation, TenantObservability};

/// Explicit missing interval emitted instead of silently closing sequence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Gap {
    pub missing_first: u64,
    pub missing_last: u64,
    pub observed_sequence: u64,
}

/// Exact core bytes recovered for one missing sequence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveredEvent {
    pub global_sequence: u64,
    pub canonical_bytes: Vec<u8>,
}

/// Why a real core backfill did not restore the entire interval.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackfillFailure {
    RangeExceedsBound {
        required: u64,
        maximum: u32,
    },
    Stream(StreamError),
    CoreReportedGap {
        missing_first: u64,
        missing_last: u64,
    },
    Incomplete,
}

/// Consumer-visible outcome of the core backfill attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BackfillReport {
    Recovered {
        gap: Gap,
        events: Vec<RecoveredEvent>,
    },
    Incomplete {
        gap: Gap,
        recovered: Vec<RecoveredEvent>,
        failure: BackfillFailure,
    },
}

impl BackfillReport {
    /// Returns the last exact sequence recovered before the attempt stopped.
    #[must_use]
    pub fn recovered_through(&self) -> Option<u64> {
        match self {
            Self::Recovered { events, .. }
            | Self::Incomplete {
                recovered: events, ..
            } => events.last().map(|event| event.global_sequence),
        }
    }
}

/// Durable terminal notice when requested events have left retention.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Truncated {
    pub requested_from: u64,
    pub oldest_available: u64,
    pub lost_through: u64,
}

/// Explicit sequence-based retention policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Retention {
    pub maximum_undelivered_sequences: u64,
}

/// Result of applying a core backfill report to durable continuity state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackfillResolution {
    Restored,
    StillBlocked { recovered_through: Option<u64> },
}

/// Gap operations never infer or manufacture a protocol result.
#[derive(Debug)]
pub enum GapError {
    Regressed { expected: u64, observed: u64 },
    InvalidRetention,
    Blocked(Gap),
    Truncated(Truncated),
    MismatchedReport,
    Subscription(SubscriptionError),
    DurableEvent,
}

impl Display for GapError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Regressed { expected, observed } => write!(
                formatter,
                "observed sequence {observed} precedes expected sequence {expected}"
            ),
            Self::InvalidRetention => formatter.write_str("retention bound must be non-zero"),
            Self::Blocked(gap) => write!(
                formatter,
                "delivery is blocked by missing sequences {} through {}",
                gap.missing_first, gap.missing_last
            ),
            Self::Truncated(notice) => write!(
                formatter,
                "subscription is truncated from {} through {}",
                notice.requested_from, notice.lost_through
            ),
            Self::MismatchedReport => {
                formatter.write_str("backfill report does not match the durable gap")
            }
            Self::Subscription(error) => Display::fmt(error, formatter),
            Self::DurableEvent => {
                formatter.write_str("recovered core event is not durably available")
            }
        }
    }
}

impl std::error::Error for GapError {}

impl From<SubscriptionError> for GapError {
    fn from(value: SubscriptionError) -> Self {
        Self::Subscription(value)
    }
}

/// Detects a forward jump, persists the block and returns the exact missing
/// interval for consumer disclosure.
///
/// # Errors
///
/// Returns `Regressed` when the observed sequence precedes the expected one, and
/// propagates the subscription-store failure that prevented persisting the block.
pub fn detect(
    subscriptions: &mut SubscriptionStore,
    target: &SubscriptionTarget,
    expected: u64,
    observed: u64,
) -> Result<Option<Gap>, GapError> {
    subscriptions.require_unbound_target(target)?;
    detect_inner(subscriptions, target, expected, observed)
}

/// Detects and durably blocks a gap for an exact session-bound subscription after resolving the
/// current token generation through the common tenant gate.
pub fn detect_authorized(
    subscriptions: &mut SubscriptionStore,
    sessions: &SessionRegistry,
    token: &Token,
    observability: &mut TenantObservability,
    core_sequence: u64,
    target: &SubscriptionTarget,
    expected: u64,
    observed: u64,
) -> Result<Option<Gap>, GapError> {
    subscriptions.authorize_target(
        sessions,
        token,
        observability,
        core_sequence,
        target,
        Operation::SubscriptionResume,
    )?;
    detect_inner(subscriptions, target, expected, observed)
}

fn detect_inner(
    subscriptions: &mut SubscriptionStore,
    target: &SubscriptionTarget,
    expected: u64,
    observed: u64,
) -> Result<Option<Gap>, GapError> {
    if observed < expected {
        return Err(GapError::Regressed { expected, observed });
    }
    if observed == expected {
        return Ok(None);
    }
    let gap = Gap {
        missing_first: expected,
        missing_last: observed.saturating_sub(1),
        observed_sequence: observed,
    };
    subscriptions.block_gap_inner(target, gap.missing_first, gap.missing_last)?;
    Ok(Some(gap))
}

/// Uses the real `layerx-client` stream contract to request exactly the
/// missing range from the core boundary under explicit record limits.
pub fn attempt_backfill(
    transport: &mut dyn FrameTransport,
    gap: Gap,
    head: Head,
    config: StreamConfig,
    maximum_records: u32,
) -> BackfillReport {
    let required = gap
        .missing_last
        .saturating_sub(gap.missing_first)
        .saturating_add(1);
    if maximum_records == 0 || required > u64::from(maximum_records) {
        return BackfillReport::Incomplete {
            gap,
            recovered: Vec::new(),
            failure: BackfillFailure::RangeExceedsBound {
                required,
                maximum: maximum_records,
            },
        };
    }
    let cursor = ClientCursor::new(gap.missing_first, head);
    let mut stream = match subscribe(transport, cursor, config) {
        Ok(value) => value,
        Err(error) => {
            return BackfillReport::Incomplete {
                gap,
                recovered: Vec::new(),
                failure: BackfillFailure::Stream(error),
            };
        }
    };
    let mut recovered = Vec::new();
    for _ in 0..required {
        match stream.next_item() {
            Ok(StreamItem::Event(event)) => recovered.push(RecoveredEvent {
                global_sequence: event.global_sequence,
                canonical_bytes: event.canonical_bytes().to_vec(),
            }),
            Ok(StreamItem::Gap(missing)) => {
                return BackfillReport::Incomplete {
                    gap,
                    recovered,
                    failure: BackfillFailure::CoreReportedGap {
                        missing_first: missing.missing_first,
                        missing_last: missing.missing_last,
                    },
                };
            }
            Err(error) => {
                return BackfillReport::Incomplete {
                    gap,
                    recovered,
                    failure: BackfillFailure::Stream(error),
                };
            }
        }
    }
    if recovered.first().map(|event| event.global_sequence) != Some(gap.missing_first)
        || recovered.last().map(|event| event.global_sequence) != Some(gap.missing_last)
    {
        return BackfillReport::Incomplete {
            gap,
            recovered,
            failure: BackfillFailure::Incomplete,
        };
    }
    BackfillReport::Recovered {
        gap,
        events: recovered,
    }
}

/// Applies a backfill report only after every recovered event is present in
/// durable history with the exact core bytes returned by the boundary.
///
/// # Errors
///
/// Returns `MismatchedReport` when continuity is not blocked on exactly this
/// interval or the recovered sequences are not contiguous through its end, and
/// `DurableEvent` when a recovered event is absent from durable history or its
/// bytes differ from the core bytes.
pub fn apply_backfill(
    subscriptions: &mut SubscriptionStore,
    target: &SubscriptionTarget,
    gap: Gap,
    report: &BackfillReport,
) -> Result<BackfillResolution, GapError> {
    subscriptions.require_unbound_target(target)?;
    apply_backfill_inner(subscriptions, target, gap, report)
}

/// Applies a core backfill report for an exact session-bound subscription only after resolving
/// its current token generation through the common tenant gate.
pub fn apply_backfill_authorized(
    subscriptions: &mut SubscriptionStore,
    sessions: &SessionRegistry,
    token: &Token,
    observability: &mut TenantObservability,
    core_sequence: u64,
    target: &SubscriptionTarget,
    gap: Gap,
    report: &BackfillReport,
) -> Result<BackfillResolution, GapError> {
    subscriptions.authorize_target(
        sessions,
        token,
        observability,
        core_sequence,
        target,
        Operation::SubscriptionResume,
    )?;
    apply_backfill_inner(subscriptions, target, gap, report)
}

fn apply_backfill_inner(
    subscriptions: &mut SubscriptionStore,
    target: &SubscriptionTarget,
    gap: Gap,
    report: &BackfillReport,
) -> Result<BackfillResolution, GapError> {
    let Continuity::GapBlocked {
        missing_first,
        missing_last,
        ..
    } = subscriptions.continuity_inner(target)?
    else {
        return Err(GapError::MismatchedReport);
    };
    if missing_first != gap.missing_first || missing_last != gap.missing_last {
        return Err(GapError::MismatchedReport);
    }
    match report {
        BackfillReport::Recovered {
            gap: report_gap,
            events,
        } if *report_gap == gap => {
            let tenant = crate::store::TenantId::new(target.scope.tenant.as_str())
                .map_err(|_| GapError::DurableEvent)?;
            for (offset, recovered) in events.iter().enumerate() {
                let offset = u64::try_from(offset).map_err(|_| GapError::MismatchedReport)?;
                let expected = gap
                    .missing_first
                    .checked_add(offset)
                    .ok_or(GapError::MismatchedReport)?;
                if recovered.global_sequence != expected {
                    return Err(GapError::MismatchedReport);
                }
                let durable =
                    durable_event(subscriptions.durable(), &tenant, recovered.global_sequence)
                        .map_err(|_| GapError::DurableEvent)?;
                if durable.canonical_bytes != recovered.canonical_bytes {
                    return Err(GapError::DurableEvent);
                }
            }
            if events.last().map(|event| event.global_sequence) != Some(gap.missing_last) {
                return Err(GapError::MismatchedReport);
            }
            subscriptions.clear_gap_inner(target)?;
            Ok(BackfillResolution::Restored)
        }
        BackfillReport::Incomplete {
            gap: report_gap, ..
        } if *report_gap == gap => {
            let recovered_through = report.recovered_through();
            subscriptions.record_backfill_inner(target, recovered_through)?;
            Ok(BackfillResolution::StillBlocked { recovered_through })
        }
        _ => Err(GapError::MismatchedReport),
    }
}

/// Refuses any delivery while durable continuity is blocked or truncated.
///
/// # Errors
///
/// Returns `Blocked` with the missing interval or `Truncated` with the retention
/// notice, and propagates the subscription-store read failure.
pub fn admit(
    subscriptions: &SubscriptionStore,
    target: &SubscriptionTarget,
) -> Result<(), GapError> {
    subscriptions.require_unbound_target(target)?;
    admit_inner(subscriptions, target)
}

/// Checks continuity for an exact session-bound subscription after resolving its current token
/// generation through the common tenant gate.
pub fn admit_authorized(
    subscriptions: &SubscriptionStore,
    sessions: &SessionRegistry,
    token: &Token,
    observability: &mut TenantObservability,
    core_sequence: u64,
    target: &SubscriptionTarget,
) -> Result<(), GapError> {
    subscriptions.authorize_target(
        sessions,
        token,
        observability,
        core_sequence,
        target,
        Operation::SubscriptionHealth,
    )?;
    admit_inner(subscriptions, target)
}

fn admit_inner(
    subscriptions: &SubscriptionStore,
    target: &SubscriptionTarget,
) -> Result<(), GapError> {
    match subscriptions.continuity_inner(target)? {
        Continuity::Healthy => Ok(()),
        Continuity::GapBlocked {
            missing_first,
            missing_last,
            ..
        } => Err(GapError::Blocked(Gap {
            missing_first,
            missing_last,
            observed_sequence: missing_last.saturating_add(1),
        })),
        Continuity::Truncated {
            requested_from,
            oldest_available,
            lost_through,
        } => Err(GapError::Truncated(Truncated {
            requested_from,
            oldest_available,
            lost_through,
        })),
    }
}

/// Enforces the declared retention bound and persists terminal truncation when
/// an unacknowledged cursor is older than the effective retained window.
///
/// # Errors
///
/// Returns `InvalidRetention` for a zero bound and propagates the
/// subscription-store failure that prevented reading the cursor or persisting the
/// notice.
pub fn enforce_retention(
    subscriptions: &mut SubscriptionStore,
    target: &SubscriptionTarget,
    head_exclusive: u64,
    core_oldest_available: u64,
    retention: Retention,
) -> Result<Option<Truncated>, GapError> {
    subscriptions.require_unbound_target(target)?;
    enforce_retention_inner(
        subscriptions,
        target,
        head_exclusive,
        core_oldest_available,
        retention,
    )
}

/// Enforces retention for an exact session-bound subscription after resolving its current token
/// generation through the common tenant gate.
pub fn enforce_retention_authorized(
    subscriptions: &mut SubscriptionStore,
    sessions: &SessionRegistry,
    token: &Token,
    observability: &mut TenantObservability,
    core_sequence: u64,
    target: &SubscriptionTarget,
    head_exclusive: u64,
    core_oldest_available: u64,
    retention: Retention,
) -> Result<Option<Truncated>, GapError> {
    subscriptions.authorize_target(
        sessions,
        token,
        observability,
        core_sequence,
        target,
        Operation::SubscriptionResume,
    )?;
    enforce_retention_inner(
        subscriptions,
        target,
        head_exclusive,
        core_oldest_available,
        retention,
    )
}

fn enforce_retention_inner(
    subscriptions: &mut SubscriptionStore,
    target: &SubscriptionTarget,
    head_exclusive: u64,
    core_oldest_available: u64,
    retention: Retention,
) -> Result<Option<Truncated>, GapError> {
    if retention.maximum_undelivered_sequences == 0 {
        return Err(GapError::InvalidRetention);
    }
    let requested_from = subscriptions.resume_cursor_inner(target)?.0;
    let bounded_oldest = head_exclusive.saturating_sub(retention.maximum_undelivered_sequences);
    let oldest_available = core_oldest_available.max(bounded_oldest);
    if requested_from >= oldest_available {
        return Ok(None);
    }
    let notice = Truncated {
        requested_from,
        oldest_available,
        lost_through: oldest_available.saturating_sub(1),
    };
    subscriptions.mark_truncated_inner(
        target,
        notice.requested_from,
        notice.oldest_available,
        notice.lost_through,
    )?;
    Ok(Some(notice))
}

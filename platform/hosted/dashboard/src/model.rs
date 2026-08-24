//! The developer-facing view models.
//!
//! Every protocol fact the dashboard renders carries the verification level the
//! human plane displays, and settlement is presented only where a verified
//! `LayerX` receipt stands behind it. Gateway configuration - keys, quotas and
//! the request counters behind them - is hosted policy rather than protocol
//! state, so it is presented without a verification level instead of being
//! dressed in one it has no evidence for.

use layerx_platform_webhooks::deliveries::DeliveryRecord;
use layerx_platform_webhooks::endpoints::EndpointHealth;
use layerx_platform_webhooks::events::{ProtocolEvent, ProtocolFact, Verification};
use serde::Serialize;

pub(crate) fn per_mille(part: u64, whole: u64) -> u64 {
    if whole == 0 {
        return 0;
    }
    part.saturating_mul(1_000) / whole
}

fn fact_value(event: &ProtocolEvent, name: &str) -> Option<String> {
    event
        .facts()
        .iter()
        .find(|fact| fact.name() == name)
        .map(|fact| fact.value().to_owned())
}

fn fact<'a>(event: &'a ProtocolEvent, name: &str) -> Option<&'a ProtocolFact> {
    event.facts().iter().find(|fact| fact.name() == name)
}

/// How the hosted gateway answered one recorded request.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RequestOutcome {
    /// The request has durable pending or unknown state and may still complete.
    Pending,
    /// The operation ran and returned receipt-backed evidence.
    Completed,
    /// The per-key quota was exhausted and the refusal carried retry timing.
    RateLimited,
    /// The gateway refused the request before it reached the protocol.
    Refused,
}

impl RequestOutcome {
    /// Returns the wire word for this outcome.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Completed => "completed",
            Self::RateLimited => "rate-limited",
            Self::Refused => "refused",
        }
    }
}

/// One issued API key and the quota window it is spending.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct KeyView {
    /// Key identifier. The secret itself is never recoverable from the store.
    pub key_id: String,
    /// Owning developer principal.
    pub principal: String,
    /// Whether the key has been disabled by a rotation.
    pub disabled: bool,
    /// Requests the key may spend inside one window.
    pub requests_per_window: u64,
    /// Length of the fixed quota window in whole seconds.
    pub window_seconds: u64,
    /// Requests already spent inside the current window.
    pub used_in_window: u64,
    /// Requests still available inside the current window.
    pub remaining_in_window: u64,
    /// When the counted window started.
    pub window_started_at: u64,
    /// When the counted window ends.
    pub window_resets_at: u64,
    /// True when the recorded window has already elapsed, so the next request
    /// starts a fresh window and the spent count reads as zero.
    pub window_lapsed: bool,
    /// Share of the window spent, in parts per thousand.
    pub utilisation_per_mille: u64,
}

/// Quota and usage across every key the principal holds.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct UsageSummary {
    /// Keys issued to the principal.
    pub keys: u64,
    /// Keys that still authenticate.
    pub live_keys: u64,
    /// Keys disabled by a rotation.
    pub disabled_keys: u64,
    /// Requests the live keys may spend per window in total.
    pub requests_allowed: u64,
    /// Requests spent across the live keys in their current windows.
    pub requests_used: u64,
    /// Requests still available across the live keys.
    pub requests_remaining: u64,
    /// Share of the allowance spent, in parts per thousand.
    pub utilisation_per_mille: u64,
}

/// One line of the gateway request log.
///
/// The gateway audit trail deliberately records digests rather than operations
/// and principals, and keeps no receipt evidence, so a request line never
/// presents above `unverified`. Settlement evidence lives in [`ReceiptView`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RequestRecord {
    /// When the gateway recorded the request.
    pub at: u64,
    /// The operation, when its digest matches a known production route.
    pub operation: Option<String>,
    /// The recorded operation digest, always presented.
    pub operation_digest: String,
    /// How the gateway answered.
    pub outcome: RequestOutcome,
    /// The level this line presents, which is never above `unverified`.
    pub verification: Verification,
}

/// The shape of the request log the principal owns.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct RequestSummary {
    /// Requests recorded for this principal.
    pub records: u64,
    /// Requests whose durable state is pending or unknown.
    pub pending: u64,
    /// Requests that completed against the protocol.
    pub completed: u64,
    /// Requests refused by the quota with retry timing.
    pub rate_limited: u64,
    /// Requests the gateway refused outright.
    pub refused: u64,
    /// When the first recorded request was made.
    pub first_at: Option<u64>,
    /// When the most recent recorded request was made.
    pub last_at: Option<u64>,
}

/// One receipt-backed payment visible to the authenticated developer.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ReceiptView {
    /// Canonical activity identifier established by the verified receipt.
    pub activity_id: String,
    /// Event which carries this receipt-backed activity.
    pub event: String,
    /// Digest of the independently verified receipt bytes.
    pub receipt_digest: String,
    /// Human-plane verification level established by that receipt.
    pub verification: Verification,
    /// True only when verified evidence establishes settlement.
    pub settled: bool,
}

/// One test payment as the dashboard renders it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PaymentView {
    /// Event identifier.
    pub event: String,
    /// Ordering subject.
    pub subject: String,
    /// Position of the event inside its subject.
    pub subject_sequence: u64,
    /// Protocol observation time in whole seconds.
    pub occurred_at: u64,
    /// Exact protocol amount, when the event carries one.
    pub amount: Option<String>,
    /// Protocol asset, when the event carries one.
    pub asset: Option<String>,
    /// The weakest level among the displayed facts.
    pub verification: Verification,
    /// Evidence level carried by the settlement-state fact specifically.
    pub settlement_verification: Verification,
    /// Receipt digest backing the settlement-state fact specifically.
    pub receipt_digest: Option<String>,
    /// True only when a verified `LayerX` receipt establishes settlement.
    pub settled: bool,
    /// Every displayed fact with its own evidence.
    pub facts: Vec<ProtocolFact>,
}

impl PaymentView {
    /// Projects one payment event onto the dashboard view.
    ///
    /// Settlement is claimed only when the state fact is at least
    /// `receipt-verified` and a receipt digest stands behind that exact fact.
    /// The event-level verification remains the weakest level among all facts,
    /// including source annotations such as amount and asset.
    #[must_use]
    pub fn of(event: &ProtocolEvent) -> Self {
        let settlement = fact(event, "state");
        let settlement_verification = settlement
            .map(ProtocolFact::verification)
            .unwrap_or(Verification::Unverified);
        let receipt_digest = settlement.and_then(ProtocolFact::receipt_digest);
        let settled = settlement.is_some_and(|fact| fact.value() == "settled")
            && settlement_verification.at_least(Verification::ReceiptVerified)
            && receipt_digest.is_some();
        Self {
            event: event.id().as_str().to_owned(),
            subject: event.subject().as_str().to_owned(),
            subject_sequence: event.subject_sequence(),
            occurred_at: event.occurred_at(),
            amount: fact_value(event, "amount"),
            asset: fact_value(event, "asset"),
            verification: event.verification(),
            settlement_verification,
            receipt_digest: receipt_digest.map(str::to_owned),
            settled,
            facts: event.facts().to_vec(),
        }
    }
}

/// Webhook delivery health across every endpoint the principal owns.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct DeliverySummary {
    /// Registered endpoints.
    pub endpoints: u64,
    /// Endpoints delivery is currently suspended for.
    pub suspended_endpoints: u64,
    /// Deliveries queued and never attempted.
    pub pending: u64,
    /// Deliveries whose attempt has not yet been resolved.
    pub in_flight: u64,
    /// Deliveries waiting for their next scheduled attempt.
    pub retrying: u64,
    /// Deliveries accepted by an endpoint.
    pub delivered_total: u64,
    /// Deliveries that exhausted their attempts.
    pub dead_lettered_total: u64,
    /// Share of finished deliveries that were accepted, in parts per thousand.
    pub delivered_per_mille: u64,
    /// Age in seconds of the oldest delivery not yet accepted anywhere.
    pub oldest_undelivered_seconds: u64,
    /// When the next scheduled attempt is due anywhere.
    pub next_attempt_at: Option<u64>,
}

impl DeliverySummary {
    /// Summarises the health of every endpoint the principal owns.
    #[must_use]
    pub fn of(endpoints: &[EndpointHealth]) -> Self {
        let mut summary = Self::default();
        for endpoint in endpoints {
            summary.endpoints = summary.endpoints.saturating_add(1);
            if endpoint.suspended {
                summary.suspended_endpoints = summary.suspended_endpoints.saturating_add(1);
            }
            summary.pending = summary.pending.saturating_add(endpoint.pending);
            summary.in_flight = summary.in_flight.saturating_add(endpoint.in_flight);
            summary.retrying = summary.retrying.saturating_add(endpoint.retrying);
            summary.delivered_total = summary
                .delivered_total
                .saturating_add(endpoint.delivered_total);
            summary.dead_lettered_total = summary
                .dead_lettered_total
                .saturating_add(endpoint.dead_lettered_total);
            summary.oldest_undelivered_seconds = summary
                .oldest_undelivered_seconds
                .max(endpoint.oldest_undelivered_seconds);
            summary.next_attempt_at = match (summary.next_attempt_at, endpoint.next_attempt_at) {
                (Some(current), Some(candidate)) => Some(current.min(candidate)),
                (current, None) => current,
                (None, candidate) => candidate,
            };
        }
        summary.delivered_per_mille = per_mille(
            summary.delivered_total,
            summary
                .delivered_total
                .saturating_add(summary.dead_lettered_total),
        );
        summary
    }
}

/// The developer landing view: keys and quota, the request log, webhook
/// delivery health and the most recent test payments in one read.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Overview {
    /// The principal every record below belongs to.
    pub principal: String,
    /// When the view was assembled.
    pub generated_at: u64,
    /// Quota and usage across every key.
    pub usage: UsageSummary,
    /// The issued keys themselves.
    pub keys: Vec<KeyView>,
    /// The shape of the request log.
    pub requests: RequestSummary,
    /// The most recent request lines, newest first.
    pub recent_requests: Vec<RequestRecord>,
    /// Webhook delivery health across every endpoint.
    pub deliveries: DeliverySummary,
    /// Per-endpoint webhook delivery health.
    pub endpoints: Vec<EndpointHealth>,
    /// The dead-letter path, newest first.
    pub dead_letters: Vec<DeliveryRecord>,
    /// The most recent test payments, newest first.
    pub payments: Vec<PaymentView>,
}

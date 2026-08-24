//! Registered developer endpoints, their retry contract and the delivery health
//! the developer dashboards render.

use serde::{Deserialize, Serialize};

use crate::error::WebhookError;
use crate::events::{EventKind, Verification};

/// Bounded deterministic retry contract for one unreachable endpoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RetryPolicy {
    /// Delay applied before the second attempt, in whole seconds.
    pub base_delay_seconds: u64,
    /// Upper bound on any computed delay, in whole seconds.
    pub maximum_delay_seconds: u64,
    /// Total attempts before the delivery is dead-lettered.
    pub maximum_attempts: u32,
    /// Deterministic spread applied around the doubled delay, in percent.
    pub spread_percent: u8,
    /// Consecutive dead letters that suspend the endpoint.
    pub suspend_after_dead_letters: u32,
    /// Seconds after which an attempt left in flight by a crash is retried.
    pub in_flight_timeout_seconds: u64,
}

impl RetryPolicy {
    /// Returns the hosted default: eight attempts spread from ten seconds to one
    /// hour, suspending an endpoint after twenty consecutive dead letters.
    #[must_use]
    pub const fn hosted() -> Self {
        Self {
            base_delay_seconds: 10,
            maximum_delay_seconds: 3_600,
            maximum_attempts: 8,
            spread_percent: 20,
            suspend_after_dead_letters: 20,
            in_flight_timeout_seconds: 120,
        }
    }

    /// Checks that every bound is usable.
    ///
    /// # Errors
    /// Returns [`WebhookError::InvalidRequest`] when a bound is zero, the
    /// maximum delay is below the base delay, or the spread exceeds a half.
    pub fn validate(self) -> Result<Self, WebhookError> {
        if self.base_delay_seconds == 0
            || self.maximum_delay_seconds < self.base_delay_seconds
            || self.maximum_attempts == 0
            || self.spread_percent > 50
            || self.in_flight_timeout_seconds == 0
        {
            return Err(WebhookError::InvalidRequest);
        }
        Ok(self)
    }

    /// Returns the delay before the given attempt number, doubling from the base
    /// delay and spread deterministically by the delivery's own digest so a
    /// replayed schedule is reproducible.
    #[must_use]
    pub fn backoff_seconds(&self, attempt: u32, spread_source: &[u8; 32]) -> u64 {
        let exponent = attempt.saturating_sub(1).min(16);
        let doubled = self
            .base_delay_seconds
            .saturating_mul(1_u64 << exponent)
            .min(self.maximum_delay_seconds);
        let swing = doubled.saturating_mul(u64::from(self.spread_percent)) / 100;
        let span = swing.saturating_mul(2).saturating_add(1);
        let index = usize::try_from(attempt % 32).unwrap_or(0);
        let offset = u64::from(spread_source[index]) % span;
        doubled
            .saturating_sub(swing)
            .saturating_add(offset)
            .clamp(1, self.maximum_delay_seconds)
    }
}

/// Delivery health for one endpoint, rendered by the developer dashboards.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EndpointHealth {
    /// Endpoint identifier.
    pub endpoint: String,
    /// Destination the deliveries are posted to.
    pub url: String,
    /// Subscribed families, empty meaning every family.
    pub kinds: Vec<EventKind>,
    /// Weakest level the endpoint accepts.
    pub minimum_verification: Verification,
    /// Whether delivery is currently suspended.
    pub suspended: bool,
    /// Why delivery was suspended, when it was.
    pub suspended_reason: Option<String>,
    /// Deliveries queued and never attempted.
    pub pending: u64,
    /// Deliveries whose attempt has not yet been resolved.
    pub in_flight: u64,
    /// Deliveries waiting for their next scheduled attempt.
    pub retrying: u64,
    /// Deliveries accepted by the endpoint.
    pub delivered_total: u64,
    /// Deliveries that exhausted their attempts.
    pub dead_lettered_total: u64,
    /// Consecutive dead letters since the last acceptance.
    pub consecutive_dead_letters: u32,
    /// Age in seconds of the oldest delivery that has not been accepted.
    pub oldest_undelivered_seconds: u64,
    /// When the next scheduled attempt is due.
    pub next_attempt_at: Option<u64>,
    /// When the endpoint last accepted a delivery.
    pub last_delivery_at: Option<u64>,
    /// The last recorded refusal or transport failure.
    pub last_failure: Option<String>,
    /// When the last refusal or transport failure was recorded.
    pub last_failure_at: Option<u64>,
    /// Identifier of the key deliveries are signed under right now.
    pub key_id: String,
    /// Public half of that key, base64 as the shipped consumers accept it.
    pub public_key: String,
    /// When the signing key was last rotated.
    pub key_rotated_at: u64,
    /// Identifier of an announced key that has not started signing yet.
    pub pending_key_id: Option<String>,
    /// Public half of the announced key.
    pub pending_public_key: Option<String>,
    /// When the announced key starts signing.
    pub pending_key_activates_at: Option<u64>,
}

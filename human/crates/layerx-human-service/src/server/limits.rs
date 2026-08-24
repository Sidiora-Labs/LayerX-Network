use std::collections::BTreeMap;
use std::sync::Mutex;

use super::backend::ApiFailure;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Window {
    started_at: u64,
    requests: u32,
}

/// Finite principal-scoped fixed-window request gate. Saturation refuses new
/// principals instead of evicting live counters and permitting a bypass.
pub struct PrincipalLimits {
    requests_per_window: u32,
    window_seconds: u64,
    maximum_principals: usize,
    windows: Mutex<BTreeMap<String, Window>>,
}

impl PrincipalLimits {
    /// Creates a request gate only from finite non-zero bounds.
    ///
    /// # Errors
    ///
    /// Refuses disabled limits.
    pub fn new(
        requests_per_window: u32,
        window_seconds: u64,
        maximum_principals: usize,
    ) -> Result<Self, ApiFailure> {
        if requests_per_window == 0 || window_seconds == 0 || maximum_principals == 0 {
            return Err(ApiFailure::unavailable());
        }
        Ok(Self {
            requests_per_window,
            window_seconds,
            maximum_principals,
            windows: Mutex::new(BTreeMap::new()),
        })
    }

    /// Accounts one request to the authenticated principal.
    ///
    /// # Errors
    ///
    /// Returns a typed refusal carrying exact retry timing at the configured bound.
    pub fn admit(&self, principal: &str, now: u64) -> Result<(), ApiFailure> {
        let mut windows = self.windows.lock().map_err(|_| ApiFailure::unavailable())?;
        windows.retain(|_, window| {
            now.saturating_sub(window.started_at) < self.window_seconds.saturating_mul(2)
        });
        if !windows.contains_key(principal) && windows.len() >= self.maximum_principals {
            return Err(ApiFailure::rate_limited(self.window_seconds.saturating_mul(1_000)));
        }
        let window = windows.entry(principal.to_owned()).or_insert(Window {
            started_at: now,
            requests: 0,
        });
        if now.saturating_sub(window.started_at) >= self.window_seconds {
            *window = Window {
                started_at: now,
                requests: 0,
            };
        }
        if window.requests >= self.requests_per_window {
            let elapsed = now.saturating_sub(window.started_at);
            let remaining = self.window_seconds.saturating_sub(elapsed).max(1);
            return Err(ApiFailure::rate_limited(remaining.saturating_mul(1_000)));
        }
        window.requests = window.requests.saturating_add(1);
        Ok(())
    }
}

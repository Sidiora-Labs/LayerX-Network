use std::fmt;
use std::time::Duration;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClockError {
    Unavailable,
    Invalid,
    Overflow,
    Regression,
}

impl fmt::Display for ClockError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Unavailable => "clock authority unavailable",
            Self::Invalid => "invalid clock authority response",
            Self::Overflow => "clock counter overflow",
            Self::Regression => "clock authority regressed or changed",
        })
    }
}

impl std::error::Error for ClockError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClockReading {
    pub generation: [u8; 16],
    pub unix_milliseconds: u64,
    pub monotonic_nanoseconds: u64,
}

impl ClockReading {
    #[must_use]
    pub const fn unix_seconds(self) -> u64 {
        self.unix_milliseconds / 1_000
    }

    /// # Errors
    /// Refuses a changed generation or regressing wall or monotonic observation.
    pub fn follows(self, previous: Self) -> Result<Self, ClockError> {
        if self.generation == [0; 16]
            || self.generation != previous.generation
            || self.unix_milliseconds < previous.unix_milliseconds
            || self.monotonic_nanoseconds < previous.monotonic_nanoseconds
        {
            return Err(ClockError::Regression);
        }
        Ok(self)
    }

    /// # Errors
    /// Refuses observations from different generations or in reverse order.
    pub fn elapsed_since(self, previous: Self) -> Result<Duration, ClockError> {
        self.follows(previous)?;
        Ok(Duration::from_nanos(
            self.monotonic_nanoseconds - previous.monotonic_nanoseconds,
        ))
    }
}

pub trait Clock: Send + Sync {
    /// # Errors
    /// Returns a refusal when the explicit authority cannot provide a valid observation within the transport budget.
    fn sample(&self, transport_budget: Duration) -> Result<ClockReading, ClockError>;
}

#[derive(Clone, Copy, Debug)]
pub struct Deadline {
    start: ClockReading,
    expires: u64,
    last_remaining: Duration,
}

impl Deadline {
    /// # Errors
    /// Refuses unavailable clocks, invalid generations and overflowing deadlines.
    pub fn start(clock: &dyn Clock, duration: Duration) -> Result<Self, ClockError> {
        let start = clock.sample(Duration::from_secs(1).min(duration))?;
        let nanoseconds = u64::try_from(duration.as_nanos()).map_err(|_| ClockError::Overflow)?;
        if start.generation == [0; 16] {
            return Err(ClockError::Invalid);
        }
        Ok(Self {
            start,
            expires: start
                .monotonic_nanoseconds
                .checked_add(nanoseconds)
                .ok_or(ClockError::Overflow)?,
            last_remaining: duration,
        })
    }

    /// # Errors
    /// Refuses unavailable, changed or regressing clock observations.
    pub fn remaining(&mut self, clock: &dyn Clock) -> Result<Duration, ClockError> {
        self.remaining_bounded(clock, Duration::from_secs(1))
    }

    /// # Errors
    /// Refuses unavailable, changed or regressing observations within the supplied observation budget.
    pub fn remaining_bounded(
        &mut self,
        clock: &dyn Clock,
        budget: Duration,
    ) -> Result<Duration, ClockError> {
        if self.last_remaining.is_zero() {
            return Ok(Duration::ZERO);
        }
        let now = clock
            .sample(budget.min(self.last_remaining))?
            .follows(self.start)?;
        self.last_remaining =
            Duration::from_nanos(self.expires.saturating_sub(now.monotonic_nanoseconds));
        self.start = now;
        Ok(self.last_remaining)
    }
}

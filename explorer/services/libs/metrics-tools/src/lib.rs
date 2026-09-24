use std::time::{Duration, Instant};

/// Timer that combines multiple time intervals into a single measurements.
///
/// The intervals are non-overlapping. Next interval can be started via [`AggregateTimer::start_interval`]
#[derive(Debug)]
pub struct AggregateTimer {
    recorded_time_secs: Duration,
}

impl Default for AggregateTimer {
    fn default() -> Self {
        AggregateTimer::new()
    }
}

impl AggregateTimer {
    pub fn new() -> Self {
        Self {
            recorded_time_secs: Duration::from_secs(0),
        }
    }

    pub fn start_interval(&mut self) -> Interval<'_> {
        Interval {
            start_time: Instant::now(),
            recorder: self,
            discarded: false,
        }
    }

    /// Add to the total
    pub fn add_time(&mut self, time: Duration) {
        self.recorded_time_secs += time;
    }

    /// Total time recorded so far
    pub fn total_time(&self) -> Duration {
        self.recorded_time_secs
    }
}

/// Timer tracking next interval.
/// Records passed time when it's dropped.
#[must_use = "Interval cannot record duration if it is not kept in a variable"]
#[derive(Debug)]
pub struct Interval<'a> {
    start_time: Instant,
    recorder: &'a mut AggregateTimer,
    discarded: bool,
}

impl Interval<'_> {
    /// Get current time of the interval without recording.
    pub fn elapsed_from_start(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// Do not record this interval.
    pub fn discard(mut self) {
        self.discarded = true;
    }

    pub fn finish(self) {
        drop(self);
    }
}

impl<'a> Drop for Interval<'a> {
    fn drop(&mut self) {
        if !self.discarded {
            self.recorder.add_time(self.elapsed_from_start())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::thread::sleep;

    use super::*;

    #[test]
    fn intervals_are_approx_recorded() {
        let mut timer = AggregateTimer::new();
        let mut total_min_time = Duration::from_secs(0);
        {
            let time = Duration::from_secs_f64(0.1);
            total_min_time += time;
            let _interval = timer.start_interval();
            sleep(time);
        }
        {
            let time = Duration::from_secs_f64(0.2);
            total_min_time += time;
            let _interval = timer.start_interval();
            sleep(time);
        }
        // sleep pauses for "at least the specified amount of time"
        assert!(timer.total_time() > total_min_time)
        // thus the test should be not flaky
    }
}

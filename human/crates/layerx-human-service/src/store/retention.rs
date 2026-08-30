use super::Table;

/// Declared retention span in the same unit as every injected `written_at` and
/// `now` stamp. A zero period retains nothing past the next sweep.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetentionPeriod(u64);

impl RetentionPeriod {
    /// Declares a retention span.
    #[must_use]
    pub const fn new(units: u64) -> Self {
        Self(units)
    }

    /// Returns the declared span.
    #[must_use]
    pub const fn units(self) -> u64 {
        self.0
    }
}

/// Retention declared for every namespace the store persists. Exportable
/// audit entries and the evidence they reference outlive every period here.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetentionPolicy {
    /// Journey rows.
    pub journeys: RetentionPeriod,
    /// Notification rows.
    pub notifications: RetentionPeriod,
    /// Internal audit entries; exportable entries never expire.
    pub audit: RetentionPeriod,
    /// Telemetry buffer rows.
    pub telemetry: RetentionPeriod,
    /// Cache rows rebuilt from upstream output.
    pub cache: RetentionPeriod,
}

impl RetentionPolicy {
    /// Returns the declared period for one non-audit table.
    #[must_use]
    pub const fn period(&self, table: Table) -> RetentionPeriod {
        match table {
            Table::Journeys => self.journeys,
            Table::Notifications | Table::Support | Table::Stream => self.notifications,
            Table::Telemetry => self.telemetry,
            Table::Cache => self.cache,
        }
    }
}

/// Counts from one expiry sweep over a principal scope.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExpiryReport {
    /// Non-audit rows removed by the sweep.
    pub expired_rows: usize,
    /// Internal audit entries removed by the sweep.
    pub expired_audit: usize,
    /// Rows past retention kept because an exportable audit entry references them.
    pub pinned_evidence_retained: usize,
    /// Exportable audit entries past retention kept as the record of export.
    pub exportable_audit_retained: usize,
}

pub(crate) const fn elapsed(now: u64, written_at: u64, period: RetentionPeriod) -> bool {
    now.saturating_sub(written_at) >= period.0
}

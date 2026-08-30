const SECONDS_PER_DAY: u64 = 86_400;

fn civil_from_days(days: u64) -> (u64, u64, u64) {
    let shifted = days.saturating_add(719_468);
    let era = shifted / 146_097;
    let day_of_era = shifted % 146_097;
    let year_of_era = day_of_era
        .saturating_sub(day_of_era / 1460)
        .saturating_add(day_of_era / 36_524)
        .saturating_sub(day_of_era / 146_096)
        / 365;
    let year = year_of_era.saturating_add(era.saturating_mul(400));
    let day_of_year = day_of_era
        .saturating_sub(year_of_era.saturating_mul(365))
        .saturating_sub(year_of_era / 4)
        .saturating_add(year_of_era / 100);
    let month_period = day_of_year.saturating_mul(5).saturating_add(2) / 153;
    let day = day_of_year
        .saturating_sub(month_period.saturating_mul(153).saturating_add(2) / 5)
        .saturating_add(1);
    let month = if month_period < 10 {
        month_period.saturating_add(3)
    } else {
        month_period.saturating_sub(9)
    };
    let year = if month <= 2 {
        year.saturating_add(1)
    } else {
        year
    };
    (year, month, day)
}

/// Formats one Unix timestamp as the strict UTC representation required by
/// human-api. Saturating civil arithmetic keeps even hostile stored values
/// bounded and deterministic.
#[must_use]
pub(crate) fn rfc3339(seconds: u64) -> String {
    let days = seconds / SECONDS_PER_DAY;
    let remainder = seconds % SECONDS_PER_DAY;
    let (year, month, day) = civil_from_days(days);
    let hour = remainder / 3600;
    let minute = remainder % 3600 / 60;
    let second = remainder % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

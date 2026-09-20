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

fn days_from_civil(year: u64, month: u64, day: u64) -> Option<u64> {
    if !(1..=12).contains(&month) || day == 0 || day > 31 {
        return None;
    }
    let shifted_year = if month <= 2 { year.checked_sub(1)? } else { year };
    let era = shifted_year / 400;
    let year_of_era = shifted_year - era * 400;
    let month_period = if month > 2 {
        month.checked_sub(3)?
    } else {
        month.checked_add(9)?
    };
    let day_of_year = month_period
        .checked_mul(153)?
        .checked_add(2)?
        / 5
        + day.checked_sub(1)?;
    let day_of_era = year_of_era
        .checked_mul(365)?
        .checked_add(year_of_era / 4)?
        .checked_sub(year_of_era / 100)?
        .checked_add(day_of_year)?;
    era.checked_mul(146_097)?
        .checked_add(day_of_era)?
        .checked_sub(719_468)
}

/// Parses the strict UTC representation human-api declares back into one Unix
/// timestamp. Nothing else is admitted: no offset, no fractional second and no
/// alternative separator, so a timestamp the contract does not allow is a
/// refusal rather than an approximation.
#[must_use]
pub(crate) fn seconds_from_rfc3339(text: &str) -> Option<u64> {
    let bytes = text.as_bytes();
    if bytes.len() != 20 || bytes[4] != b'-' || bytes[7] != b'-' || bytes[10] != b'T' {
        return None;
    }
    if bytes[13] != b':' || bytes[16] != b':' || bytes[19] != b'Z' {
        return None;
    }
    let number = |range: std::ops::Range<usize>| -> Option<u64> {
        let slice = text.get(range)?;
        if !slice.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        slice.parse::<u64>().ok()
    };
    let year = number(0..4)?;
    let month = number(5..7)?;
    let day = number(8..10)?;
    let hour = number(11..13)?;
    let minute = number(14..16)?;
    let second = number(17..19)?;
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let days = days_from_civil(year, month, day)?;
    let (civil_year, civil_month, civil_day) = civil_from_days(days);
    if (civil_year, civil_month, civil_day) != (year, month, day) {
        return None;
    }
    days.checked_mul(SECONDS_PER_DAY)?
        .checked_add(hour.checked_mul(3_600)?)?
        .checked_add(minute.checked_mul(60)?)?
        .checked_add(second)
}

#[cfg(test)]
mod round_trip_tests {
    use super::{rfc3339, seconds_from_rfc3339};

    #[test]
    fn every_declared_timestamp_round_trips_exactly() {
        for seconds in [0_u64, 1, 86_399, 86_400, 1_755_500_000, 4_102_444_800] {
            let text = rfc3339(seconds);
            assert_eq!(seconds_from_rfc3339(&text), Some(seconds), "{text}");
        }
    }

    #[test]
    fn a_timestamp_the_contract_forbids_is_refused() {
        for refused in [
            "2026-08-18T09:35:00+00:00",
            "2026-08-18 09:35:00Z",
            "2026-08-18T09:35:00.5Z",
            "2026-02-30T00:00:00Z",
            "2026-13-01T00:00:00Z",
            "2026-08-18T24:00:00Z",
            "",
        ] {
            assert_eq!(seconds_from_rfc3339(refused), None, "{refused}");
        }
    }
}

//! RFC 3339 UTC timestamps without a date dependency.

/// Format Unix seconds as `YYYY-MM-DDTHH:MM:SSZ`.
///
/// The store writes this into an envelope's `written_at`. Values before the
/// epoch are not representable because the input is unsigned.
pub fn format_rfc3339_utc(epoch_secs: u64) -> String {
    let days = i64::try_from(epoch_secs / 86_400).unwrap_or(i64::MAX);
    let seconds = epoch_secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = seconds / 3_600;
    let minute = (seconds % 3_600) / 60;
    let second = seconds % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Civil date from days since 1970-01-01, using Howard Hinnant's algorithm.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    (
        if month <= 2 { year + 1 } else { year },
        u32::try_from(month).unwrap_or_default(),
        u32::try_from(day).unwrap_or_default(),
    )
}

#[cfg(test)]
mod tests {
    use super::format_rfc3339_utc;

    #[test]
    fn rfc3339_formatter_handles_epoch_and_leap_days() {
        assert_eq!(format_rfc3339_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_rfc3339_utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(format_rfc3339_utc(4_107_542_400), "2100-03-01T00:00:00Z");
    }
}

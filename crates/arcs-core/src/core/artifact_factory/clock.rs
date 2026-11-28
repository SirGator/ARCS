use std::time::{SystemTime, UNIX_EPOCH};

/// Injizierbare vertrauenswürdige Zeitquelle.
pub trait Clock: Send + Sync {
    fn now_rfc3339(&self) -> String;
}

/// UTC-Systemzeit ohne zusätzliche Laufzeitabhängigkeit.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_rfc3339(&self) -> String {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        format_unix_timestamp(seconds)
    }
}

/// Formatiert einen Unix-Zeitstempel als RFC-3339-Zeit in UTC.
pub fn format_rfc3339(seconds: u64) -> String {
    format_unix_timestamp(seconds)
}

fn format_unix_timestamp(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let seconds_of_day = seconds % 86_400;
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    let (year, month, day) = civil_date_from_unix_days(days);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

// Howard Hinnants kalenderarithmetische Umrechnung. Sie arbeitet rein
// ganzzahlig und ist für alle vom u64-Unixzeitstempel erreichbaren Tage stabil.
fn civil_date_from_unix_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::format_unix_timestamp;

    #[test]
    fn formats_known_unix_timestamps_as_rfc3339_utc() {
        assert_eq!(format_unix_timestamp(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_unix_timestamp(946_684_800), "2000-01-01T00:00:00Z");
        assert_eq!(format_unix_timestamp(1_720_396_800), "2024-07-08T00:00:00Z");
    }
}

/// Rechnet einen UTC-Zeitstempel in Sekunden seit der Unix-Epoche um.
///
/// Die Umrechnung ist die Umkehrung von `format_unix_timestamp` und wird für
/// Laufzeitgrenzen gebraucht, die der Core selbst verwaltet, etwa das Ablauf
/// einer Aufgaben-Lease. Ein nicht parsebarer Wert liefert `None`; die
/// aufrufende Grenze entscheidet dann fail-closed.
pub fn unix_seconds_from_rfc3339(value: &str) -> Option<u64> {
    let bytes = value.as_bytes();
    if bytes.len() != 20 || bytes[4] != b'-' || bytes[7] != b'-' || bytes[10] != b'T' {
        return None;
    }
    if bytes[13] != b':' || bytes[16] != b':' || bytes[19] != b'Z' {
        return None;
    }
    let number = |range: std::ops::Range<usize>| -> Option<u32> {
        let text = value.get(range)?;
        if !text.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        text.parse::<u32>().ok()
    };
    let year = number(0..4)?;
    let month = number(5..7)?;
    let day = number(8..10)?;
    let hour = number(11..13)?;
    let minute = number(14..16)?;
    let second = number(17..19)?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let days = days_from_civil(i64::from(year), month, day);
    let seconds = days
        .checked_mul(86_400)?
        .checked_add(i64::from(hour) * 3_600)?
        .checked_add(i64::from(minute) * 60)?
        .checked_add(i64::from(second))?;
    u64::try_from(seconds).ok()
}

/// Umkehrung der Kalenderrechnung: Tage seit der Unix-Epoche aus einem Datum.
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let shifted_year = if month <= 2 { year - 1 } else { year };
    let era = if shifted_year >= 0 {
        shifted_year
    } else {
        shifted_year - 399
    } / 400;
    let year_of_era = shifted_year - era * 400;
    let month_prime = if month > 2 { month - 3 } else { month + 9 } as i64;
    let day_of_year = (153 * month_prime + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod lease_time_tests {
    use super::{format_unix_timestamp, unix_seconds_from_rfc3339};

    #[test]
    fn round_trips_known_timestamps() {
        for seconds in [0_u64, 946_684_800, 1_720_396_800, 2_000_000_000] {
            let formatted = format_unix_timestamp(seconds);
            assert_eq!(unix_seconds_from_rfc3339(&formatted), Some(seconds));
        }
    }

    #[test]
    fn rejects_malformed_timestamps() {
        assert_eq!(unix_seconds_from_rfc3339(""), None);
        assert_eq!(unix_seconds_from_rfc3339("2026-11-28"), None);
        assert_eq!(unix_seconds_from_rfc3339("2026-11-28T10:00:00+01:00"), None);
        assert_eq!(unix_seconds_from_rfc3339("2026-13-28T10:00:00Z"), None);
        assert_eq!(unix_seconds_from_rfc3339("2026-11-28T25:00:00Z"), None);
    }
}

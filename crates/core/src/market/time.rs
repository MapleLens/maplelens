//! The few date/time operations the cards need, without a date library: ISO-8601 parsing to Unix seconds,
//! weekdays and short month names. Everything is UTC, and "now" is always passed in by the platform.

/// Seconds since 1970-01-01T00:00:00Z of an ISO-8601 timestamp ("2026-10-21T20:06:20Z", "...+00:00",
/// fractional seconds, or a bare date). None if it is not one.
pub fn parse_iso(text: &str) -> Option<f64> {
    let text = text.trim();
    let (date, rest) = text.split_at(text.find(['T', ' ']).unwrap_or(text.len()));
    let days = days_from_date(date)?;
    let rest = rest.get(1..).unwrap_or("");
    if rest.is_empty() {
        return Some(days as f64 * 86400.0);
    }
    let zone_at = rest.find(['Z', '+', '-']).unwrap_or(rest.len());
    let (clock, zone) = rest.split_at(zone_at);
    let mut parts = clock.split(':');
    let hours: f64 = parts.next()?.parse().ok()?;
    let minutes: f64 = parts.next().unwrap_or("0").parse().ok()?;
    let seconds: f64 = parts.next().unwrap_or("0").parse().ok()?;
    let offset = match zone.chars().next() {
        Some(sign @ ('+' | '-')) => {
            let digits: String = zone[1..].chars().filter(char::is_ascii_digit).collect();
            let (h, m) = digits.split_at(digits.len().min(2));
            let minutes = h.parse::<f64>().ok()? * 60.0 + m.parse::<f64>().unwrap_or(0.0);
            if sign == '+' { minutes * 60.0 } else { -minutes * 60.0 }
        }
        _ => 0.0,
    };
    Some(days as f64 * 86400.0 + hours * 3600.0 + minutes * 60.0 + seconds - offset)
}

/// Days since 1970-01-01 of "YYYY-MM-DD" (proleptic Gregorian).
pub fn days_from_date(date: &str) -> Option<i64> {
    let mut parts = date.split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // Howard Hinnant's days_from_civil.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146097 + doe - 719468)
}

/// (year, month 1-12, day) of a day count since 1970-01-01.
pub fn date_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + (month <= 2) as i64, month, day)
}

/// "Mon".."Sun" of a day count since 1970-01-01 (a Thursday).
pub fn weekday_name(days: i64) -> &'static str {
    ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"][days.rem_euclid(7) as usize]
}

pub fn month_name(month: u32) -> &'static str {
    ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]
        [(month.clamp(1, 12) - 1) as usize]
}

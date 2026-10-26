//! Human-friendly number and time formatting for overlay text (pure).

use super::time::parse_iso;

fn trim(value: f64, unit: &str) -> String {
    let decimals = if value < 10.0 {
        2
    } else if value < 100.0 {
        1
    } else {
        0
    };
    let mut text = format!("{value:.decimals$}");
    // Only fractional zeros are padding; whole-number zeros carry value.
    if decimals > 0 {
        text = text.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    text + unit
}

/// 1320 -> "1.32k", 10500 -> "10.5k", 2_300_000 -> "2.3m". Under 1000 stays plain.
pub fn format_mesos(amount: Option<f64>) -> String {
    let Some(amount) = amount else { return "?".into() };
    if amount < 1000.0 {
        return if amount >= 100.0 {
            format!("{amount:.0}")
        } else {
            format!("{amount:.2}").trim_end_matches('0').trim_end_matches('.').to_string()
        };
    }
    for (divisor, unit) in [(1e3, "k"), (1e6, "m"), (1e9, "b")] {
        let scaled = amount / divisor;
        let shown: f64 = format!("{scaled:.2}").parse().unwrap_or(scaled);
        if shown < 1000.0 || unit == "b" {
            return trim(scaled, unit);
        }
    }
    unreachable!("the billions unit always returns")
}

/// "12,345": digit grouping like Python's `{:,}`.
pub fn group_thousands(value: i64) -> String {
    let digits = value.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if value < 0 { format!("-{out}") } else { out }
}

/// "3h ago" style age of an ISO-8601 timestamp ("—" when unknown); `now` is Unix seconds.
pub fn format_ago(iso_utc: Option<&str>, now: f64) -> String {
    let Some(then) = iso_utc.filter(|s| !s.is_empty()).and_then(parse_iso) else { return "—".into() };
    let seconds = now - then;
    if seconds < 90.0 {
        "just now".into()
    } else if seconds < 5400.0 {
        format!("{:.0}m ago", seconds / 60.0)
    } else if seconds < 172_800.0 {
        format!("{:.0}h ago", seconds / 3600.0)
    } else {
        format!("{:.0}d ago", seconds / 86400.0)
    }
}

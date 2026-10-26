//! Release versions are plain X.Y.Z (the tag is vX.Y.Z). Pre-release tags are never offered, so they do not parse.

use super::{UpdateError, fail};

pub type Version = (u32, u32, u32);

pub fn parse_version(text: &str) -> Result<Version, UpdateError> {
    let digits = text.strip_prefix('v').unwrap_or(text);
    let parts: Vec<&str> = digits.split('.').collect();
    let valid =
        parts.len() == 3 && parts.iter().all(|p| (1..=6).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_digit()));
    if !valid {
        return fail(format!("unsupported version {text:?}"));
    }
    let number = |i: usize| parts[i].parse::<u32>().map_err(|_| UpdateError(format!("unsupported version {text:?}")));
    Ok((number(0)?, number(1)?, number(2)?))
}

pub fn format_version((major, minor, patch): Version) -> String {
    format!("{major}.{minor}.{patch}")
}

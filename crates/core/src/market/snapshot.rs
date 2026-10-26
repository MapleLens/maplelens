//! Parse a MesoWatch snapshot (list.json) into items.

use std::fmt;

use serde_json::Value;

use super::models::Item;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotError {
    NotFound(String),
    /// Not a list.json at all (an HTML challenge page, `{}`, broken JSON).
    Invalid(String),
    /// Looks like a list.json but an item lacks what the catalog needs (a name).
    Incompatible(String),
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(path) => write!(f, "snapshot not found: {path}"),
            Self::Invalid(why) => write!(f, "not a MesoWatch list.json ({why})"),
            Self::Incompatible(why) => write!(f, "snapshot cannot build a catalog ({why})"),
        }
    }
}

impl std::error::Error for SnapshotError {}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub items: Vec<Item>,
    /// ISO-8601 UTC time MesoWatch exported the data.
    pub generated_at: Option<String>,
}

pub fn parse_snapshot(text: &str) -> Result<Snapshot, SnapshotError> {
    let document: Value = serde_json::from_str(text).map_err(|e| SnapshotError::Invalid(e.to_string()))?;
    let raw_items = document
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| SnapshotError::Invalid("no items list".into()))?;
    let items = raw_items
        .iter()
        .enumerate()
        .map(|(i, raw)| Item::from_raw(raw).ok_or_else(|| SnapshotError::Incompatible(format!("item {i} has no name"))))
        .collect::<Result<Vec<_>, _>>()?;
    let generated_at = document.get("generated_at").and_then(Value::as_str).map(str::to_string);
    Ok(Snapshot { items, generated_at })
}

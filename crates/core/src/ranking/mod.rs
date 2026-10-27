//! Character ranks from the public classicrank.com API (<https://classicrank.com/developer>, no key needed):
//! the response model and a cache that paces lookups. The HTTP request itself is the platform's job.

use std::collections::{HashMap, HashSet, VecDeque};

use serde_json::Value;

use crate::config;

pub const API_URL: &str = "https://classicrank.com/api/v1/characters/";
/// The only fields the cards use (the photo is the web app's); asking for less keeps responses tiny.
pub const API_FIELDS: &str =
    "name,level,job,fame,overall_rank,job_rank,fame_rank,botting,last_checked,profile_url,photo";
/// Retry a failure after this long unless the API said otherwise.
pub const DEFAULT_RETRY_S: f64 = 30.0;

/// Other spellings a read name may have, fewest changes first, at most `max`: the game's small name font draws
/// capital I and lowercase l as the same plain bar, so "SharkChiIi" may be "SharkChili".
pub fn look_alikes(name: &str, max: usize) -> Vec<String> {
    let chars: Vec<char> = name.chars().collect();
    let spots: Vec<usize> = (0..chars.len()).filter(|&i| matches!(chars[i], 'I' | 'l')).collect();
    // Names are at most 13 letters; this only bounds the work for junk reads.
    if spots.is_empty() || spots.len() > 13 {
        return Vec::new();
    }
    let mut flips: Vec<u32> = (1..1u32 << spots.len()).collect();
    flips.sort_by_key(|&f| (f.count_ones(), f));
    let spelled = |flip: u32| {
        let mut out = chars.clone();
        for (bit, &i) in spots.iter().enumerate() {
            if flip & (1 << bit) != 0 {
                out[i] = if out[i] == 'I' { 'l' } else { 'I' };
            }
        }
        out.into_iter().collect()
    };
    flips.into_iter().take(max).map(spelled).collect()
}

/// The request URL for `name`.
pub fn rank_url(name: &str) -> String {
    let quoted: String = name
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    format!("{API_URL}{quoted}?fields={API_FIELDS}")
}

/// What the classicrank.com API says about one character.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Rank {
    pub name: String,
    pub level: Option<i64>,
    pub job: Option<String>,
    pub fame: Option<i64>,
    /// By level; for botters it is the place in the Botters ranking.
    pub overall_rank: Option<i64>,
    pub job_rank: Option<i64>,
    pub fame_rank: Option<i64>,
    pub botting: bool,
    /// ISO-8601 UTC.
    pub last_checked: Option<String>,
    pub profile_url: Option<String>,
    /// A PNG of the character (96 px square, transparent around it), when classicrank.com has one.
    pub photo: Option<String>,
}

impl Rank {
    /// Parse one character object; unknown fields are ignored (the API may add some).
    pub fn from_api(raw: &Value) -> Option<Self> {
        let int = |k: &str| raw.get(k).and_then(Value::as_i64);
        let text = |k: &str| raw.get(k).and_then(Value::as_str).map(str::to_string);
        Some(Self {
            name: match raw.get("name")? {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            },
            level: int("level"),
            job: text("job"),
            fame: int("fame"),
            overall_rank: int("overall_rank"),
            job_rank: int("job_rank"),
            fame_rank: int("fame_rank"),
            botting: raw.get("botting").and_then(Value::as_bool).unwrap_or(false),
            last_checked: text("last_checked"),
            profile_url: text("profile_url"),
            photo: text("photo"),
        })
    }
}

/// How a lookup ended, as the platform's HTTP client reports it.
#[derive(Clone, Debug, PartialEq)]
pub enum Fetched {
    Found(Rank),
    /// HTTP 404: classicrank.com does not know the name.
    Missing,
    /// Network trouble, a bad response, or a rate limit; retry after this many seconds.
    Failed(f64),
}

/// The body of a 200 response, unwrapped from `{"data": {...}}`.
pub fn parse_response(body: &str) -> Fetched {
    match serde_json::from_str::<Value>(body).ok().as_ref().and_then(|v| v.get("data")).and_then(Rank::from_api) {
        Some(rank) => Fetched::Found(rank),
        None => Fetched::Failed(DEFAULT_RETRY_S),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Status {
    Loading,
    Found,
    /// classicrank.com does not know the name.
    Missing,
    /// Network trouble or rate limit.
    Failed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RankResult {
    pub status: Status,
    pub rank: Option<Rank>,
    /// Clock time after which the entry is looked up again.
    pub expires_at: f64,
}

impl RankResult {
    pub const LOADING: Self = Self { status: Status::Loading, rank: None, expires_at: 0.0 };

    pub fn of(status: Status) -> Self {
        Self { status, rank: None, expires_at: 0.0 }
    }
}

/// Cached rank answers. `get` answers at once and queues a lookup when one is due; the platform drains the
/// queue one name at a time (paced by `config::RANK_REQUEST_GAP_S`) and reports back with `store`.
#[derive(Default)]
pub struct RankCache {
    results: HashMap<String, RankResult>,
    queued: HashSet<String>,
    queue: VecDeque<String>,
}

impl RankCache {
    /// The cached answer for `name` (stale ones are shown while they refresh); queues a lookup if due.
    pub fn get(&mut self, name: &str, now: f64) -> RankResult {
        let result = self.results.get(name).cloned().unwrap_or(RankResult::LOADING);
        if now >= result.expires_at && !self.queued.contains(name) {
            self.queued.insert(name.to_string());
            self.queue.push_back(name.to_string());
        }
        result
    }

    /// The next name to look up, if any.
    pub fn next_due(&mut self) -> Option<String> {
        self.queue.pop_front()
    }

    /// Record how the lookup of `name` went.
    pub fn store(&mut self, name: &str, fetched: Fetched, now: f64) {
        let result = match fetched {
            Fetched::Found(rank) => {
                RankResult { status: Status::Found, rank: Some(rank), expires_at: now + config::RANK_TTL_S }
            }
            Fetched::Missing => {
                RankResult { status: Status::Missing, rank: None, expires_at: now + config::RANK_MISSING_TTL_S }
            }
            Fetched::Failed(retry_after) => {
                // A failure never throws away a rank we already have: it keeps showing until the retry.
                let old = self.results.get(name).and_then(|r| r.rank.clone());
                let status = if old.is_some() { Status::Found } else { Status::Failed };
                RankResult { status, rank: old, expires_at: now + retry_after }
            }
        };
        self.results.insert(name.to_string(), result);
        self.queued.remove(name);
    }
}

//! The two network requests MapleLens makes: the price snapshot (through the relay) and classicrank.com rank lookups.

use std::time::Duration;

use maplelens_core::config;
use maplelens_core::ranking::{DEFAULT_RETRY_S, Fetched, parse_response, rank_url};

/// The price list, through MapleLens's own relay (a Cloudflare Worker): it asks MesoWatch at most once every 30
/// minutes whoever asks, and the relay is what MesoWatch's permission is built around.
pub const SNAPSHOT_URL: &str = "https://maplelens-prices.prices-proxy.workers.dev/prices.json";
/// classicrank.com's public API: who is asking, honestly.
const USER_AGENT: &str = concat!("MapleLens/", env!("CARGO_PKG_VERSION"), " (+https://github.com/maplelens/maplelens)");
/// list.json is several MB; well above it, far below anything that could hurt.
const MAX_SNAPSHOT_BYTES: u64 = 128 * 1024 * 1024;
const RANK_TIMEOUT_S: u64 = 5;

pub(crate) fn agent(timeout_s: u64) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(timeout_s)))
        .user_agent(USER_AGENT)
        .http_status_as_error(false)
        .build()
        .into()
}

#[derive(Debug)]
pub struct NetError(pub String);

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl NetError {
    /// The server answered 404: the thing is not there (asking again will not help).
    pub fn is_not_found(&self) -> bool {
        self.0.starts_with("HTTP 404")
    }
}

impl std::error::Error for NetError {}

/// The body of the price snapshot.
pub fn fetch_snapshot() -> Result<String, NetError> {
    let mut response =
        agent(config::SNAPSHOT_TIMEOUT_S).get(SNAPSHOT_URL).call().map_err(|e| NetError(e.to_string()))?;
    let status = response.status().as_u16();
    if status != 200 {
        return Err(NetError(format!("snapshot request answered HTTP {status}")));
    }
    response.body_mut().with_config().limit(MAX_SNAPSHOT_BYTES).read_to_string().map_err(|e| NetError(e.to_string()))
}

/// Look `name` up on classicrank.com (public API, no key; 30 requests/minute per IP).
pub fn fetch_rank(name: &str) -> Fetched {
    let response = agent(RANK_TIMEOUT_S).get(rank_url(name)).header("Accept", "application/json").call();
    let mut response = match response {
        Ok(response) => response,
        Err(error) => {
            crate::warn!("rank lookup for {name:?} failed: {error}");
            return Fetched::Failed(DEFAULT_RETRY_S);
        }
    };
    match response.status().as_u16() {
        200 => match response.body_mut().read_to_string() {
            Ok(body) => parse_response(&body),
            Err(_) => Fetched::Failed(DEFAULT_RETRY_S),
        },
        404 => Fetched::Missing,
        429 => {
            // The API says how long to back off.
            let retry =
                response.headers().get("Retry-After").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok());
            Fetched::Failed(retry.unwrap_or(DEFAULT_RETRY_S))
        }
        status => {
            crate::warn!("rank lookup for {name:?} answered HTTP {status}");
            Fetched::Failed(DEFAULT_RETRY_S)
        }
    }
}

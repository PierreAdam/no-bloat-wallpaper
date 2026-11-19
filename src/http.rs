//! One shared HTTP client with timeouts and size limits, so a slow or misbehaving server can
//! never hang a scheduled run or fill the disk.

use crate::error::Coded;
use anyhow::{Context, Result};
use std::sync::OnceLock;
use std::time::Duration;

/// Largest picture we accept (UHD JPEGs are typically 1 to 10 MB).
pub const MAX_IMAGE_BYTES: u64 = 40 * 1024 * 1024;
/// Largest feed or JSON answer we accept.
const MAX_TEXT_BYTES: u64 = 10 * 1024 * 1024;

/// Sent with every request: the project's name and version, and where to find it, as
/// Wikimedia's User-Agent policy asks of tools.
pub const USER_AGENT: &str =
    concat!("no-bloat-wallpaper/", env!("CARGO_PKG_VERSION"), " (+", env!("CARGO_PKG_REPOSITORY"), ")");

fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .user_agent(USER_AGENT)
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_recv_body(Some(Duration::from_secs(30)))
            // Whole request, body included: bounds even a server that trickles bytes.
            .timeout_global(Some(Duration::from_secs(120)))
            .build()
            .into()
    })
}

/// The body of a GET, refusing anything larger than `limit` bytes.
pub fn get_bytes(url: &str, limit: u64) -> Result<Vec<u8>> {
    let mut resp = match agent().get(url).call() {
        Ok(r) => r,
        Err(ureq::Error::StatusCode(429)) => {
            return Err(Coded::new("rateLimited", format!("GET {url}: rate limited (HTTP 429)")).into())
        }
        Err(e) => return Err(anyhow::Error::new(e).context(format!("GET {url}"))),
    };
    match resp.body_mut().with_config().limit(limit).read_to_vec() {
        Ok(bytes) => Ok(bytes),
        Err(ureq::Error::BodyExceedsLimit(_)) => {
            anyhow::bail!("{url}: answer larger than {} MB", limit / (1024 * 1024))
        }
        Err(e) => Err(anyhow::Error::new(e).context(format!("reading {url}"))),
    }
}

/// Percent-encode a value for a URL query string.
pub fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn get_text(url: &str) -> Result<String> {
    String::from_utf8(get_bytes(url, MAX_TEXT_BYTES)?).with_context(|| format!("{url}: not UTF-8 text"))
}

pub fn get_json<T: serde::de::DeserializeOwned>(url: &str) -> Result<T> {
    serde_json::from_str(&get_text(url)?).with_context(|| format!("{url}: decoding JSON"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_agent_names_the_project_and_where_to_find_it() {
        assert!(USER_AGENT.starts_with("no-bloat-wallpaper/"));
        assert!(USER_AGENT.contains("https://github.com/PierreAdam/no-bloat-wallpaper"));
    }

    #[test]
    fn encoding_keeps_only_unreserved_characters() {
        assert_eq!(encode("en-US"), "en-US");
        assert_eq!(encode("a&b=c d"), "a%26b%3Dc%20d");
    }
}

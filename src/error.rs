//! Errors the interface can recognise by a stable code (a translation key in the GUI)
//! instead of matching English sentences.

use std::fmt;

#[derive(Debug)]
pub struct Coded {
    /// Translation key, e.g. "errNoSource" or "rateLimited".
    pub code: &'static str,
    /// English text for logs and the command line.
    pub message: String,
}

impl Coded {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Coded { code, message: message.into() }
    }
}

impl fmt::Display for Coded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Coded {}

/// The code of the first coded error in the chain, if any.
pub fn code_of(e: &anyhow::Error) -> Option<&'static str> {
    e.chain().find_map(|c| c.downcast_ref::<Coded>()).map(|c| c.code)
}

/// True when the server could not be reached at all (no network yet, DNS, timeout): worth
/// retrying a little later. An answer from the server, even an error status, is not.
pub fn is_network(e: &anyhow::Error) -> bool {
    e.chain().any(|c| {
        matches!(
            c.downcast_ref::<ureq::Error>(),
            Some(
                ureq::Error::HostNotFound
                    | ureq::Error::ConnectionFailed
                    | ureq::Error::Timeout(_)
                    | ureq::Error::Io(_)
            )
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_connection_failures_count_as_network() {
        assert!(is_network(&anyhow::Error::new(ureq::Error::HostNotFound).context("GET x")));
        assert!(is_network(&anyhow::Error::new(ureq::Error::ConnectionFailed)));
        assert!(!is_network(&anyhow::Error::new(ureq::Error::StatusCode(404))));
        assert!(!is_network(&anyhow::Error::new(ureq::Error::StatusCode(500))));
        assert!(!is_network(&anyhow::anyhow!("not a JPEG/PNG image")));
    }

    #[test]
    fn code_is_found_under_context() {
        let e = anyhow::Error::new(Coded::new("errBusy", "busy")).context("outer");
        assert_eq!(code_of(&e), Some("errBusy"));
    }
}

//! Inference client errors and PII-redacting helpers.
//!
//! Diagnostics propagate to the agent-facing CLI surface, so URLs and
//! filesystem paths are scrubbed before the message leaves this crate. The
//! redaction patterns mirror `embed/chunks-diagnostics.ts::redactForAgent`.

use std::sync::OnceLock;

use regex::Regex;
use thiserror::Error;

/// Maximum diagnostic message length after redaction (matches TS).
pub const MAX_DIAGNOSTIC_CHARS: usize = 280;

/// Errors returned by the inference client.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum InferenceError {
    /// `reqwest::Client` could not be constructed.
    #[error("inference client build failed: {message}")]
    Build {
        /// Redacted detail.
        message: String,
    },

    /// HTTP transport or non-2xx status from the sidecar.
    #[error("inference HTTP error{}: {message}", .status.map(|s| format!(" ({s})")).unwrap_or_default())]
    Http {
        /// HTTP status code, if any (None for transport failures).
        status: Option<u16>,
        /// Redacted detail (URL, response body snippet).
        message: String,
    },

    /// Response body could not be decoded into the expected JSON shape.
    #[error("inference response decode failed: {message}")]
    Decode {
        /// Redacted detail.
        message: String,
    },

    /// Configuration is invalid or incomplete.
    #[error("inference config error: {message}")]
    Config {
        /// Configuration detail.
        message: String,
    },
}

/// Compiles a static regex literal. The pattern is hard-coded so a
/// compile failure is a programmer bug, not a runtime condition — falling
/// through to `unreachable!` reflects that.
fn compile_static(pattern: &str) -> Regex {
    match Regex::new(pattern) {
        Ok(re) => re,
        Err(err) => unreachable!("static regex {pattern:?} did not compile: {err}"),
    }
}

fn url_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| compile_static(r"https?://[^\s]+"))
}

fn users_path_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| compile_static(r"/Users/[^\s]+"))
}

fn home_path_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| compile_static(r"/home/[^\s]+"))
}

/// Strips URLs and host paths from a diagnostic string, then truncates to
/// [`MAX_DIAGNOSTIC_CHARS`].
///
/// The sidecar URL and vault paths are PII-adjacent (they reveal local install
/// layout); the agent surface gets a sanitized form so a logged failure does
/// not leak the user's filesystem shape.
#[must_use]
pub fn redact(value: &str) -> String {
    let stage1 = url_re().replace_all(value, "[sidecar]");
    let stage2 = users_path_re().replace_all(stage1.as_ref(), "[host-path]");
    let stage3 = home_path_re().replace_all(stage2.as_ref(), "[host-path]");
    stage3.chars().take(MAX_DIAGNOSTIC_CHARS).collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn redact_replaces_https_url() {
        let input = "GET https://localhost:8080/embed failed";
        let out = redact(input);
        assert!(out.contains("[sidecar]"));
        assert!(!out.contains("https://"));
    }

    #[test]
    fn redact_replaces_http_url() {
        let input = "POST http://example.com/api 500";
        let out = redact(input);
        assert!(out.contains("[sidecar]"));
        assert!(!out.contains("example.com"));
    }

    #[test]
    fn redact_replaces_users_path() {
        let input = "/Users/alice/Documents/vault/note.md not found";
        let out = redact(input);
        assert!(out.contains("[host-path]"));
        assert!(!out.contains("alice"));
    }

    #[test]
    fn redact_replaces_home_path() {
        let input = "open /home/bob/talon/idx.sqlite";
        let out = redact(input);
        assert!(out.contains("[host-path]"));
        assert!(!out.contains("bob"));
    }

    #[test]
    fn redact_truncates_to_max_chars() {
        let long = "x".repeat(MAX_DIAGNOSTIC_CHARS + 100);
        let out = redact(&long);
        assert_eq!(out.chars().count(), MAX_DIAGNOSTIC_CHARS);
    }

    #[test]
    fn redact_passes_through_short_clean_input() {
        let input = "embedding dimension mismatch";
        assert_eq!(redact(input), input);
    }

    #[test]
    fn redact_handles_multiple_urls_in_one_message() {
        let input = "fetch https://a.com/x and http://b.com/y both failed";
        let out = redact(input);
        assert!(!out.contains("a.com"));
        assert!(!out.contains("b.com"));
    }
}

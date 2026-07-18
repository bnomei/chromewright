//! URL normalization and navigation safety checks for high-level browser tools.
//!
//! Non-http(s) absolute schemes and protocol-relative targets require explicit opt-in
//! (`allow_unsafe` / confirm-style gates at the tool boundary). Relative paths pass through.

use crate::error::{BrowserError, Result};

/// Normalize an incomplete URL by adding a protocol when the input looks like a host or domain.
///
/// Leaves absolute schemes, relative paths, and special protocols (`about:`, `data:`,
/// `file:`, `chrome:`) unchanged. Bare hosts get `https://`; localhost/loopback get `http://`.
pub fn normalize_url(url: &str) -> String {
    let trimmed = url.trim();

    // If already has a protocol, return as-is
    if trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.starts_with("file://")
        || trimmed.starts_with("data:")
        || trimmed.starts_with("about:")
        || trimmed.starts_with("chrome://")
        || trimmed.starts_with("chrome-extension://")
    {
        return trimmed.to_string();
    }

    // Relative path - return as-is
    if trimmed.starts_with('/') || trimmed.starts_with("./") || trimmed.starts_with("../") {
        return trimmed.to_string();
    }

    // localhost special case - use http by default
    if trimmed.starts_with("localhost") || trimmed.starts_with("127.0.0.1") {
        return format!("http://{}", trimmed);
    }

    // Check if it looks like a domain (contains dot or is a known TLD)
    if trimmed.contains('.') {
        // Looks like a domain - add https://
        return format!("https://{}", trimmed);
    }

    // Single word - assume it's a domain name, add www. prefix and https://
    // This handles cases like "google" -> "https://www.google.com"
    format!("https://www.{}.com", trimmed)
}

fn has_absolute_scheme(url: &str) -> bool {
    let Some((scheme, _rest)) = url.split_once(':') else {
        return false;
    };

    !scheme.is_empty()
        && scheme
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
}

fn is_protocol_relative(url: &str) -> bool {
    let trimmed = url.trim_start();
    let mut chars = trimmed.chars();
    matches!(
        (chars.next(), chars.next()),
        (Some('/' | '\\'), Some('/' | '\\'))
    )
}

/// Validate a high-level navigation target after normalization.
///
/// # Errors
///
/// Returns [`BrowserError::InvalidArgument`] when an unsafe absolute scheme or protocol-
/// relative URL is used without `allow_unsafe = true`. Safe http(s) and same-origin relative
/// paths succeed without opt-in.
pub fn validate_navigation_url(url: &str, allow_unsafe: bool) -> Result<String> {
    let normalized = normalize_url(url);
    if allow_unsafe {
        return Ok(normalized);
    }

    if is_protocol_relative(url) || is_protocol_relative(&normalized) {
        return Err(BrowserError::InvalidArgument(format!(
            "Protocol-relative navigation target '{}' is blocked by default; pass allow_unsafe=true or use an absolute http(s) URL.",
            normalized
        )));
    }

    if !has_absolute_scheme(&normalized) {
        return Ok(normalized);
    }

    let scheme = normalized
        .split_once(':')
        .map(|(scheme, _)| scheme.to_ascii_lowercase())
        .unwrap_or_default();
    if matches!(scheme.as_str(), "http" | "https") {
        return Ok(normalized);
    }

    Err(BrowserError::InvalidArgument(format!(
        "Unsafe navigation target '{}' is blocked by default; pass allow_unsafe=true to opt in.",
        normalized
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_url_complete() {
        assert_eq!(normalize_url("https://example.com"), "https://example.com");
        assert_eq!(normalize_url("http://example.com"), "http://example.com");
        assert_eq!(
            normalize_url("https://example.com/path"),
            "https://example.com/path"
        );
    }

    #[test]
    fn test_normalize_url_missing_protocol() {
        assert_eq!(normalize_url("example.com"), "https://example.com");
        assert_eq!(
            normalize_url("example.com/path"),
            "https://example.com/path"
        );
        assert_eq!(normalize_url("sub.example.com"), "https://sub.example.com");
    }

    #[test]
    fn test_normalize_url_partial_domain() {
        assert_eq!(normalize_url("google"), "https://www.google.com");
        assert_eq!(normalize_url("github"), "https://www.github.com");
        assert_eq!(normalize_url("amazon"), "https://www.amazon.com");
    }

    #[test]
    fn test_normalize_url_localhost() {
        assert_eq!(normalize_url("localhost"), "http://localhost");
        assert_eq!(normalize_url("localhost:3000"), "http://localhost:3000");
        assert_eq!(normalize_url("127.0.0.1"), "http://127.0.0.1");
        assert_eq!(normalize_url("127.0.0.1:8080"), "http://127.0.0.1:8080");
    }

    #[test]
    fn test_normalize_url_special_protocols() {
        assert_eq!(normalize_url("about:blank"), "about:blank");
        assert_eq!(
            normalize_url("file:///path/to/file"),
            "file:///path/to/file"
        );
        assert_eq!(
            normalize_url("data:text/html,<h1>Test</h1>"),
            "data:text/html,<h1>Test</h1>"
        );
        assert_eq!(normalize_url("chrome://settings"), "chrome://settings");
    }

    #[test]
    fn test_normalize_url_relative_paths() {
        assert_eq!(normalize_url("/path"), "/path");
        assert_eq!(normalize_url("/path/to/page"), "/path/to/page");
        assert_eq!(normalize_url("./relative"), "./relative");
        assert_eq!(normalize_url("../parent"), "../parent");
    }

    #[test]
    fn test_normalize_url_whitespace() {
        assert_eq!(normalize_url("  example.com  "), "https://example.com");
        assert_eq!(
            normalize_url("  https://example.com  "),
            "https://example.com"
        );
    }

    #[test]
    fn test_validate_navigation_url_blocks_unsafe_scheme_by_default() {
        let err = validate_navigation_url("data:text/html,<h1>Test</h1>", false)
            .expect_err("data: should be blocked without explicit opt-in");
        assert!(matches!(err, BrowserError::InvalidArgument(_)));
    }

    #[test]
    fn test_validate_navigation_url_allows_unsafe_scheme_with_opt_in() {
        let normalized = validate_navigation_url("data:text/html,<h1>Test</h1>", true)
            .expect("explicit opt-in should allow data: navigation");
        assert_eq!(normalized, "data:text/html,<h1>Test</h1>");
    }

    #[test]
    fn test_validate_navigation_url_blocks_protocol_relative_by_default() {
        for target in [
            "//evil.example/phish",
            "  //evil.example/phish",
            "/\\evil.example/phish",
            "\\\\evil.example/phish",
            "\\/evil.example/phish",
        ] {
            let err = validate_navigation_url(target, false)
                .err()
                .unwrap_or_else(|| panic!("protocol-relative target {target:?} should be blocked"));
            assert!(
                matches!(err, BrowserError::InvalidArgument(_)),
                "unexpected error for {target:?}: {err}"
            );
        }
    }

    #[test]
    fn test_validate_navigation_url_allows_protocol_relative_with_opt_in() {
        let normalized = validate_navigation_url("//cdn.example/app.js", true)
            .expect("explicit opt-in should allow protocol-relative navigation");
        assert_eq!(normalized, "//cdn.example/app.js");
    }

    #[test]
    fn test_validate_navigation_url_allows_same_origin_relative_paths() {
        for target in ["/settings", "./relative", "../parent", "/path/to/page"] {
            let normalized = validate_navigation_url(target, false)
                .unwrap_or_else(|err| panic!("relative path {target:?} should pass: {err}"));
            assert_eq!(normalized, target);
        }
    }
}

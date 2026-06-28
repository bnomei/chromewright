//! Embedded Mozilla Readability script for main-content extraction in `get_markdown`.

/// Minified Readability bundle evaluated in-page before HTML-to-Markdown conversion.
pub const READABILITY_SCRIPT: &str = include_str!("Readability.min.js");

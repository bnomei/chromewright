//! Embedded Mozilla Readability script for main-content extraction in `get_markdown`.
//!
//! The minified vendor bundle is injected as a string constant into the markdown extraction
//! script; do not edit `Readability.min.js` unless intentionally updating the vendor asset.

/// Minified Readability bundle evaluated in-page before HTML-to-Markdown conversion.
pub const READABILITY_SCRIPT: &str = include_str!("Readability.min.js");

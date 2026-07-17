//! Resource budgets enforced before DOM extraction, snapshots, screenshots, and waits.

use crate::error::{BrowserError, Result};

/// Upper bound for `wait.timeout_ms` (two minutes).
pub(crate) const MAX_WAIT_TIMEOUT_MS: u64 = 120_000;

/// Per-axis CSS pixel cap for screenshot capture regions.
pub(crate) const SCREENSHOT_MAX_CSS_DIMENSION: f64 = 32_768.0;
/// CSS pixel area cap (`width * height`) for screenshot capture regions.
pub(crate) const SCREENSHOT_MAX_CSS_AREA: f64 = 50_000_000.0;
/// Encoded PNG size budget returned to the agent.
pub(crate) const SCREENSHOT_MAX_PNG_BYTES: usize = 64 * 1024 * 1024;

/// Maximum actionable nodes retained during DOM extraction.
pub(crate) const MAX_DOM_NODES: usize = 10_000;
/// Maximum DOM tree depth during extraction.
pub(crate) const MAX_DOM_DEPTH: usize = 64;
/// Maximum same-origin frames expanded during extraction.
pub(crate) const MAX_DOM_FRAMES: usize = 50;
/// Cap on individual DOM/string fields (names, URLs, excerpts, etc.).
pub(crate) const MAX_DOM_STRING_CHARS: usize = 4_096;
/// Cap on collection sizes emitted from DOM extraction paths.
pub(crate) const MAX_DOM_COLLECTION_ITEMS: usize = 50_000;
/// Combined snapshot text + serialized nodes byte budget.
pub(crate) const MAX_SNAPSHOT_OUTPUT_BYTES: usize = 1_000_000;
/// Character budget for extract tool payload bodies.
pub(crate) const MAX_EXTRACT_CHARS: usize = 1_000_000;
/// Maximum links returned by `read_links`.
pub(crate) const MAX_READ_LINKS_COUNT: usize = 2_000;
/// Per-field character cap for link title/href-like strings.
pub(crate) const MAX_READ_LINK_FIELD_CHARS: usize = 2_048;
/// Character budget for Readability HTML (and related markdown text) inputs.
pub(crate) const MAX_MARKDOWN_HTML_CHARS: usize = 1_000_000;
/// Maximum markdown characters per pagination page.
pub(crate) const MAX_MARKDOWN_PAGE_SIZE: usize = 200_000;
/// Compact-field truncation budget for `inspect_node` string fields.
pub(crate) const MAX_INSPECT_COMPACT_CHARS: usize = 2_000;
/// Maximum CSS classes retained on an inspect identity payload.
pub(crate) const MAX_INSPECT_CLASSES: usize = 64;

/// Reject wait timeouts above [`MAX_WAIT_TIMEOUT_MS`].
pub(crate) fn validate_wait_timeout(timeout_ms: u64) -> Result<()> {
    if timeout_ms > MAX_WAIT_TIMEOUT_MS {
        return Err(BrowserError::InvalidArgument(format!(
            "wait.timeout_ms must be less than or equal to {MAX_WAIT_TIMEOUT_MS}"
        )));
    }

    Ok(())
}

/// Enforce per-axis and area CSS pixel budgets for a screenshot source region.
pub(crate) fn validate_screenshot_css_size(source: &str, width: f64, height: f64) -> Result<()> {
    if width > SCREENSHOT_MAX_CSS_DIMENSION {
        return Err(BrowserError::resource_limit_exceeded(
            "screenshot_css_width",
            format!(
                "screenshot {source} width exceeds the {SCREENSHOT_MAX_CSS_DIMENSION:.0} CSS pixel limit"
            ),
            format!("{SCREENSHOT_MAX_CSS_DIMENSION:.0} CSS pixels"),
            format!("{width:.0} CSS pixels"),
        ));
    }

    if height > SCREENSHOT_MAX_CSS_DIMENSION {
        return Err(BrowserError::resource_limit_exceeded(
            "screenshot_css_height",
            format!(
                "screenshot {source} height exceeds the {SCREENSHOT_MAX_CSS_DIMENSION:.0} CSS pixel limit"
            ),
            format!("{SCREENSHOT_MAX_CSS_DIMENSION:.0} CSS pixels"),
            format!("{height:.0} CSS pixels"),
        ));
    }

    let area = width * height;
    if area > SCREENSHOT_MAX_CSS_AREA {
        return Err(BrowserError::resource_limit_exceeded(
            "screenshot_css_area",
            format!(
                "screenshot {source} area exceeds the {SCREENSHOT_MAX_CSS_AREA:.0} CSS pixel limit"
            ),
            format!("{SCREENSHOT_MAX_CSS_AREA:.0} CSS pixels"),
            format!("{area:.0} CSS pixels"),
        ));
    }

    Ok(())
}

/// Reject PNG payloads larger than [`SCREENSHOT_MAX_PNG_BYTES`].
pub(crate) fn validate_screenshot_png_bytes(byte_count: usize) -> Result<()> {
    if byte_count > SCREENSHOT_MAX_PNG_BYTES {
        return Err(BrowserError::resource_limit_exceeded(
            "screenshot_png_bytes",
            format!(
                "screenshot PNG is {byte_count} bytes, exceeding the {SCREENSHOT_MAX_PNG_BYTES} byte limit"
            ),
            format!("{SCREENSHOT_MAX_PNG_BYTES} bytes"),
            format!("{byte_count} bytes"),
        ));
    }

    Ok(())
}

/// Reject snapshot envelopes larger than [`MAX_SNAPSHOT_OUTPUT_BYTES`].
pub(crate) fn validate_snapshot_output_bytes(byte_count: usize) -> Result<()> {
    if byte_count > MAX_SNAPSHOT_OUTPUT_BYTES {
        return Err(BrowserError::resource_limit_exceeded(
            "snapshot_output_bytes",
            format!(
                "snapshot output is {byte_count} bytes, exceeding the {MAX_SNAPSHOT_OUTPUT_BYTES} byte limit"
            ),
            format!("{MAX_SNAPSHOT_OUTPUT_BYTES} bytes"),
            format!("{byte_count} bytes"),
        ));
    }

    Ok(())
}

/// Reject extract payloads larger than [`MAX_EXTRACT_CHARS`] (format is for the error text only).
pub(crate) fn validate_extract_chars(char_count: usize, format: &str) -> Result<()> {
    if char_count > MAX_EXTRACT_CHARS {
        return Err(BrowserError::resource_limit_exceeded(
            "extract_chars",
            format!(
                "extract {format} output is {char_count} characters, exceeding the {MAX_EXTRACT_CHARS} character limit"
            ),
            format!("{MAX_EXTRACT_CHARS} characters"),
            format!("{char_count} characters"),
        ));
    }

    Ok(())
}

/// Reject markdown HTML inputs larger than [`MAX_MARKDOWN_HTML_CHARS`].
pub(crate) fn validate_markdown_html_chars(char_count: usize) -> Result<()> {
    if char_count > MAX_MARKDOWN_HTML_CHARS {
        return Err(BrowserError::resource_limit_exceeded(
            "markdown_html_chars",
            format!(
                "markdown HTML input is {char_count} characters, exceeding the {MAX_MARKDOWN_HTML_CHARS} character limit"
            ),
            format!("{MAX_MARKDOWN_HTML_CHARS} characters"),
            format!("{char_count} characters"),
        ));
    }

    Ok(())
}

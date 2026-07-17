//! Resource budgets for semantic document capture and normalization.

use crate::error::{BrowserError, Result};

/// Maximum number of semantic components in one captured document.
pub const MAX_SEMANTIC_COMPONENTS: usize = 10_000;

/// Maximum nesting depth of the normalized semantic tree.
pub const MAX_SEMANTIC_DEPTH: usize = 64;

/// Maximum character length of any single semantic string field.
pub const MAX_SEMANTIC_STRING_CHARS: usize = 4_096;

/// Maximum number of select options retained for one control.
pub const MAX_SEMANTIC_SELECT_OPTIONS: usize = 256;

/// Maximum total characters across all retained text fields in one document.
pub const MAX_SEMANTIC_TOTAL_TEXT_CHARS: usize = 1_000_000;

/// Fail closed when a single semantic string field exceeds [`MAX_SEMANTIC_STRING_CHARS`].
///
/// Used during normalization and document indexing so oversized labels, text,
/// hrefs, and related fields never enter a `SemanticDocument`.
pub(crate) fn validate_semantic_string(field: &str, value: &str) -> Result<()> {
    let char_count = value.chars().count();
    if char_count > MAX_SEMANTIC_STRING_CHARS {
        return Err(BrowserError::resource_limit_exceeded(
            "semantic_string_chars",
            format!(
                "semantic field {field} is {char_count} characters, exceeding the {MAX_SEMANTIC_STRING_CHARS} character limit"
            ),
            format!("{MAX_SEMANTIC_STRING_CHARS} characters"),
            format!("{char_count} characters"),
        ));
    }
    Ok(())
}

/// Fail closed when the document tree has more than [`MAX_SEMANTIC_COMPONENTS`] nodes.
pub(crate) fn validate_component_count(count: usize) -> Result<()> {
    if count > MAX_SEMANTIC_COMPONENTS {
        return Err(BrowserError::resource_limit_exceeded(
            "semantic_components",
            format!(
                "semantic document has {count} components, exceeding the {MAX_SEMANTIC_COMPONENTS} component limit"
            ),
            format!("{MAX_SEMANTIC_COMPONENTS} components"),
            format!("{count} components"),
        ));
    }
    Ok(())
}

/// Fail closed when nesting exceeds [`MAX_SEMANTIC_DEPTH`].
pub(crate) fn validate_depth(depth: usize) -> Result<()> {
    if depth > MAX_SEMANTIC_DEPTH {
        return Err(BrowserError::resource_limit_exceeded(
            "semantic_depth",
            format!(
                "semantic document depth is {depth}, exceeding the {MAX_SEMANTIC_DEPTH} depth limit"
            ),
            format!("{MAX_SEMANTIC_DEPTH}"),
            format!("{depth}"),
        ));
    }
    Ok(())
}

/// Fail closed when retained text across the document exceeds [`MAX_SEMANTIC_TOTAL_TEXT_CHARS`].
pub(crate) fn validate_total_text_chars(total: usize) -> Result<()> {
    if total > MAX_SEMANTIC_TOTAL_TEXT_CHARS {
        return Err(BrowserError::resource_limit_exceeded(
            "semantic_total_text_chars",
            format!(
                "semantic document text is {total} characters, exceeding the {MAX_SEMANTIC_TOTAL_TEXT_CHARS} character limit"
            ),
            format!("{MAX_SEMANTIC_TOTAL_TEXT_CHARS} characters"),
            format!("{total} characters"),
        ));
    }
    Ok(())
}

//! Output budgets shared by semantic renderers.

use crate::semantic::identity::SemanticRefError;
use std::fmt;

/// Maximum characters a single renderer may emit (after Phase 2 document bounds).
pub const MAX_RENDER_OUTPUT_CHARS: usize = 2_000_000;

/// Marker appended when renderer output is truncated at the character budget.
pub const TRUNCATION_MARKER: &str = "\n\n…[truncated]\n";

/// Bounded render result with source document id and revision for consumers.
///
/// Text projections may set `truncated` when clipped at the character budget.
/// JSON projections never truncate: over-budget payloads fail as [`RenderError::OutputLimit`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedOutput {
    /// Projection body (Markdown, JSON text, or line-oriented view).
    pub content: String,
    /// True when a text projection clipped at the character budget (JSON never truncates).
    pub truncated: bool,
    /// Source document id for consumers correlating resources and captures.
    pub document_id: String,
    /// Source document revision for fail-closed ref and retention checks.
    pub revision: String,
}

impl RenderedOutput {
    pub(crate) fn new(
        content: String,
        truncated: bool,
        document_id: impl Into<String>,
        revision: impl Into<String>,
    ) -> Self {
        Self {
            content,
            truncated,
            document_id: document_id.into(),
            revision: revision.into(),
        }
    }
}

/// Fail-closed renderer errors (reference resolution or output budget).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderError {
    /// Exact `semantic_ref` resolution failed (malformed, stale, unknown, wrong document).
    Ref(SemanticRefError),
    /// Serialized output would exceed the renderer character budget.
    OutputLimit { limit: usize, produced_chars: usize },
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ref(err) => write!(f, "{err}"),
            Self::OutputLimit {
                limit,
                produced_chars,
            } => write!(
                f,
                "render output of {produced_chars} characters exceeds the {limit} character limit"
            ),
        }
    }
}

impl std::error::Error for RenderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Ref(err) => Some(err),
            Self::OutputLimit { .. } => None,
        }
    }
}

impl From<SemanticRefError> for RenderError {
    fn from(value: SemanticRefError) -> Self {
        Self::Ref(value)
    }
}

/// Truncate `content` to at most `limit` characters, appending a marker when cut.
///
/// Returns `(text, truncated)`.
pub fn truncate_output(content: String, limit: usize) -> (String, bool) {
    let char_count = content.chars().count();
    if char_count <= limit {
        return (content, false);
    }

    let marker_chars = TRUNCATION_MARKER.chars().count();
    let keep = limit.saturating_sub(marker_chars);
    if keep == 0 {
        return (TRUNCATION_MARKER.chars().take(limit).collect(), true);
    }

    let mut out: String = content.chars().take(keep).collect();
    out.push_str(TRUNCATION_MARKER);
    (out, true)
}

/// Apply the default render output budget (text projections may truncate).
pub(crate) fn bound_output(content: String, document_id: &str, revision: &str) -> RenderedOutput {
    bound_output_with_limit(content, document_id, revision, MAX_RENDER_OUTPUT_CHARS)
}

/// Apply an explicit character budget for text projections (may truncate).
pub(crate) fn bound_output_with_limit(
    content: String,
    document_id: &str,
    revision: &str,
    limit: usize,
) -> RenderedOutput {
    let (content, truncated) = truncate_output(content, limit);
    RenderedOutput::new(content, truncated, document_id, revision)
}

/// Bound a JSON document: never truncates. Over-limit payloads fail closed so
/// callers never receive invalid JSON.
pub(crate) fn bound_json_output(
    content: String,
    document_id: &str,
    revision: &str,
    limit: usize,
) -> Result<RenderedOutput, RenderError> {
    let produced_chars = content.chars().count();
    if produced_chars > limit {
        return Err(RenderError::OutputLimit {
            limit,
            produced_chars,
        });
    }
    Ok(RenderedOutput::new(content, false, document_id, revision))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_output_preserves_short_content() {
        let (text, truncated) = truncate_output("hello".into(), 100);
        assert_eq!(text, "hello");
        assert!(!truncated);
    }

    #[test]
    fn truncate_output_marks_long_content() {
        let long = "a".repeat(50);
        let (text, truncated) = truncate_output(long, 20);
        assert!(truncated);
        assert!(text.contains("…[truncated]"));
        assert!(text.chars().count() <= 20);
    }

    #[test]
    fn bound_json_output_rejects_oversize_without_truncating() {
        let payload = r#"{"ok":true,"pad":"xxxxxxxxxxxxxxxxxxxxxxxx"}"#.to_string();
        let err = bound_json_output(payload, "d", "r", 10).expect_err("oversize");
        assert!(matches!(
            err,
            RenderError::OutputLimit {
                limit: 10,
                produced_chars
            } if produced_chars > 10
        ));
    }

    #[test]
    fn bound_json_output_accepts_valid_json_under_limit() {
        let payload = r#"{"ok":true}"#.to_string();
        let out = bound_json_output(payload.clone(), "d", "r", 100).expect("ok");
        assert!(!out.truncated);
        assert_eq!(out.content, payload);
        serde_json::from_str::<serde_json::Value>(&out.content).expect("parse");
    }
}

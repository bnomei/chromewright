//! Companion-only TUI tool domain co-hosted with [`SharedTuiState`].
//!
//! These tools coordinate selection, attention, and semantic render views for the
//! terminal UI. They are never registered on standard [`crate::tools::ToolRegistry`]
//! defaults; the TUI companion binds them with shared state. Without co-hosted
//! state, calls return [`unavailable`].

use crate::error::Result;
use crate::tools::{Tool, ToolContext, ToolDescriptor, ToolResult, ToolSafetyAnnotations};
use crate::tui::{CoordinationError, SharedTuiState};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Shared parameter envelope for all companion TUI tools.
///
/// Fields are interpreted per tool name: selection/attention updates require
/// `semantic_ref`; render/inspect/query honor `limit`; attention set may carry
/// `message`. No field chooses arbitrary filesystem paths.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TuiParams {
    /// Exact opaque semantic reference for selection/attention updates.
    pub semantic_ref: Option<String>,
    /// Optional bounded message for agent attention (never mutates Chrome).
    pub message: Option<String>,
    /// Character budget for render/outline payloads (defaults applied in [`execute`]).
    pub limit: Option<usize>,
}

/// Wire result for companion TUI tools: availability flag plus exclusive data or error.
///
/// Success always sets `available = true` and `data`; failures set `available = false`
/// and `error` without a `data` payload so clients need not inspect content shape.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct TuiResult {
    /// Whether the companion runtime handled the request (false when TUI is not co-hosted).
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<TuiData>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Typed success payloads for the companion-only TUI tools.
///
/// A successful result always carries `data`; `error` is reserved for failed
/// requests so MCP clients do not have to infer success from an error-shaped
/// field containing page content.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TuiData {
    Content {
        content: String,
    },
    Selection {
        semantic_ref: Option<String>,
    },
    Attention {
        semantic_ref: Option<String>,
        document_id: Option<String>,
        revision: Option<String>,
        message: Option<String>,
    },
    Refresh {
        document_id: String,
        revision: String,
        url: String,
        title: String,
    },
    Acknowledged,
}

/// Frozen companion tool names; must stay out of default/operator MCP registries.
pub const NAMES: [&str; 9] = [
    "tui_render",
    "tui_refresh",
    "tui_inspect",
    "tui_query",
    "tui_selection_read",
    "tui_selection_update",
    "tui_attention_read",
    "tui_attention_set",
    "tui_attention_clear",
];

/// MCP descriptors for the companion tool set (shared schemas, per-name safety hints).
///
/// Mutation-ish tools (`tui_refresh`, `*_update`, `*_set`, `*_clear`) clear the
/// read-only hint; none are marked destructive or open-world.
pub fn descriptors() -> Vec<ToolDescriptor> {
    NAMES
        .iter()
        .map(|name| ToolDescriptor {
            name: (*name).into(),
            description: "Shared TUI coordination operation".into(),
            parameters_schema: serde_json::to_value(schemars::schema_for!(TuiParams)).unwrap(),
            output_schema: serde_json::to_value(schemars::schema_for!(TuiResult)).unwrap(),
            annotations: ToolSafetyAnnotations {
                read_only_hint: *name != "tui_refresh"
                    && !name.ends_with("_set")
                    && !name.ends_with("_clear")
                    && !name.ends_with("_update"),
                destructive_hint: false,
                idempotent_hint: true,
                open_world_hint: false,
            },
        })
        .collect()
}

/// Dispatch a companion tool against co-hosted [`SharedTuiState`].
///
/// Reads selection/attention without touching Chrome. `tui_refresh` re-pulls the
/// current page into the semantic document. Unknown names return [`unavailable`].
pub fn execute(name: &str, params: TuiParams, shared: &SharedTuiState) -> TuiResult {
    match name {
        "tui_render" | "tui_query" => match shared.render(params.limit.unwrap_or(32_000)) {
            Ok(content) => success(TuiData::Content { content }),
            Err(error) => failure(error),
        },
        "tui_refresh" => match shared.refresh() {
            Ok(page) => success(TuiData::Refresh {
                document_id: page.document_id,
                revision: page.revision,
                url: page.url,
                title: page.title,
            }),
            Err(error) => failure(error),
        },
        "tui_inspect" => match shared.outline(params.limit.unwrap_or(32_000)) {
            Ok(content) => success(TuiData::Content { content }),
            Err(error) => failure(error),
        },
        "tui_selection_read" => success(TuiData::Selection {
            semantic_ref: shared.selection().map(|r| r.to_string()),
        }),
        "tui_selection_update" => params
            .semantic_ref
            .ok_or(CoordinationError::MalformedReference)
            .map(crate::semantic::SemanticRef::from_opaque)
            .and_then(|r| shared.set_selection(r))
            .map(|_| success(TuiData::Acknowledged))
            .unwrap_or_else(failure),
        "tui_attention_read" => {
            let attention = shared.attention();
            success(TuiData::Attention {
                semantic_ref: attention.semantic_ref.map(|r| r.to_string()),
                document_id: attention.document_id,
                revision: attention.revision,
                message: attention.message,
            })
        }
        "tui_attention_set" => params
            .semantic_ref
            .ok_or(CoordinationError::MalformedReference)
            .map(crate::semantic::SemanticRef::from_opaque)
            .and_then(|reference| shared.set_attention(reference, params.message.clone()))
            .map(|_| success(TuiData::Acknowledged))
            .unwrap_or_else(failure),
        "tui_attention_clear" => {
            shared.clear_attention();
            success(TuiData::Acknowledged)
        }
        _ => unavailable(),
    }
}

fn success(data: TuiData) -> TuiResult {
    TuiResult {
        available: true,
        data: Some(data),
        error: None,
    }
}

fn failure(error: impl std::fmt::Display) -> TuiResult {
    TuiResult {
        available: false,
        data: None,
        error: Some(error.to_string()),
    }
}

/// Result when the companion runtime is not co-hosted (no [`SharedTuiState`]).
///
/// Sets `available = false` and a stable error string; never panics.
pub fn unavailable() -> TuiResult {
    TuiResult {
        available: false,
        data: None,
        error: Some(
            "runtime-required: co-hosted TUI transport is not enabled in this foundation slice"
                .into(),
        ),
    }
}

/// [`Tool`] adapter for one companion TUI name, optionally bound to [`SharedTuiState`].
///
/// Without shared state, [`Tool::execute_typed`] still returns a successful outer
/// [`ToolResult`] whose payload is [`unavailable`] so MCP clients see a structured
/// companion-domain error rather than a hard tool failure.
#[derive(Clone)]
pub struct TuiTool {
    name: &'static str,
    shared: Option<SharedTuiState>,
}

impl TuiTool {
    /// Unbound companion tool; execution yields [`unavailable`] until shared state is attached.
    pub const fn new(name: &'static str) -> Self {
        Self { name, shared: None }
    }

    /// Companion tool bound to the co-hosted TUI coordination state.
    pub fn with_shared(name: &'static str, shared: SharedTuiState) -> Self {
        Self {
            name,
            shared: Some(shared),
        }
    }
}

impl Default for TuiTool {
    fn default() -> Self {
        Self::new(NAMES[0])
    }
}

impl Tool for TuiTool {
    type Params = TuiParams;
    type Output = TuiResult;

    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        "Shared TUI coordination operation"
    }

    fn parameters_schema(&self) -> Value {
        serde_json::to_value(schemars::schema_for!(TuiParams)).unwrap()
    }

    fn output_schema(&self) -> Value {
        serde_json::to_value(schemars::schema_for!(TuiResult)).unwrap()
    }

    /// Run against shared TUI state when bound; otherwise return [`unavailable`] as payload.
    ///
    /// Ignores browser [`ToolContext`]—companion tools coordinate TUI state, not CDP actions.
    ///
    /// # Errors
    ///
    /// This path does not return `Err`; companion failures are encoded inside [`TuiResult`].
    fn execute_typed(&self, params: TuiParams, _context: &mut ToolContext) -> Result<ToolResult> {
        let result = self
            .shared
            .as_ref()
            .map(|s| execute(self.name, params, s))
            .unwrap_or_else(unavailable);
        Ok(ToolResult::success_with(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::BrowserSession;
    use crate::browser::backend::FakeSessionBackend;
    use crate::dom::DocumentMetadata;
    use crate::semantic::SemanticDocument;
    use std::sync::Arc;

    fn document(revision: &str) -> SemanticDocument {
        SemanticDocument::empty(DocumentMetadata {
            document_id: "fake-tab".into(),
            revision: revision.into(),
            url: "https://example.test/".into(),
            title: "Example".into(),
            ready_state: "complete".into(),
            frames: vec![],
        })
        .expect("semantic document")
    }

    #[test]
    fn successful_render_is_typed_data_without_error_field() {
        let shared = SharedTuiState::new(Arc::new(BrowserSession::with_test_backend(
            FakeSessionBackend::new(),
        )));
        shared.publish(document("one"));

        let result = execute("tui_render", TuiParams::default(), &shared);
        assert!(result.available);
        assert!(matches!(result.data, Some(TuiData::Content { .. })));
        assert!(result.error.is_none());
        let encoded = serde_json::to_value(result).expect("serialize result");
        assert!(encoded.get("data").is_some());
        assert!(encoded.get("error").is_none());
    }

    #[test]
    fn failed_render_has_only_error_shape() {
        let shared = SharedTuiState::new(Arc::new(BrowserSession::with_test_backend(
            FakeSessionBackend::new(),
        )));

        let result = execute("tui_render", TuiParams::default(), &shared);
        assert!(!result.available);
        assert!(result.data.is_none());
        assert!(result.error.is_some());
        let encoded = serde_json::to_value(result).expect("serialize result");
        assert!(encoded.get("data").is_none());
        assert!(encoded.get("error").is_some());
    }

    #[test]
    fn shared_tool_calls_observe_the_published_state() {
        use crate::semantic::normalize::{RawSemanticNode, normalize_fixture};

        let shared = SharedTuiState::new(Arc::new(BrowserSession::with_test_backend(
            FakeSessionBackend::new(),
        )));
        let doc = normalize_fixture(
            DocumentMetadata {
                document_id: "fake-tab".into(),
                revision: "shared-revision".into(),
                url: "https://example.test/".into(),
                title: "Example".into(),
                ready_state: "complete".into(),
                frames: vec![],
            },
            vec![RawSemanticNode {
                kind: "text".into(),
                tag: Some("p".into()),
                id: Some("spotlight".into()),
                unique_id: true,
                selector: None,
                text: Some("hello".into()),
                href: None,
                landmark: None,
                heading_level: None,
                ordered: None,
                label: None,
                src: None,
                alt: None,
                name: None,
                value: None,
                input_type: None,
                placeholder: None,
                checked: None,
                disabled: None,
                required: None,
                readonly: None,
                multiple: None,
                button_type: None,
                options: vec![],
                children: vec![],
            }],
        )
        .expect("doc");
        let reference = doc.semantic_refs().into_iter().next().unwrap();
        shared.publish(doc);
        shared
            .set_attention(reference.clone(), Some("agent focus".into()))
            .expect("attention");

        let render = execute("tui_render", TuiParams::default(), &shared);
        assert!(matches!(render.data, Some(TuiData::Content { .. })));
        let attention = execute("tui_attention_read", TuiParams::default(), &shared);
        assert!(matches!(
            attention.data,
            Some(TuiData::Attention {
                semantic_ref: Some(ref token),
                message: Some(ref message),
                ..
            }) if token == reference.as_str() && message == "agent focus"
        ));
    }

    #[test]
    fn attention_set_rejects_stale_and_requires_exact_ref() {
        use crate::semantic::normalize::{RawSemanticNode, normalize_fixture};

        let shared = SharedTuiState::new(Arc::new(BrowserSession::with_test_backend(
            FakeSessionBackend::new(),
        )));
        let doc = normalize_fixture(
            DocumentMetadata {
                document_id: "fake-tab".into(),
                revision: "one".into(),
                url: "https://example.test/".into(),
                title: "Example".into(),
                ready_state: "complete".into(),
                frames: vec![],
            },
            vec![RawSemanticNode {
                kind: "text".into(),
                tag: Some("p".into()),
                id: Some("spotlight".into()),
                unique_id: true,
                selector: None,
                text: Some("hello".into()),
                href: None,
                landmark: None,
                heading_level: None,
                ordered: None,
                label: None,
                src: None,
                alt: None,
                name: None,
                value: None,
                input_type: None,
                placeholder: None,
                checked: None,
                disabled: None,
                required: None,
                readonly: None,
                multiple: None,
                button_type: None,
                options: vec![],
                children: vec![],
            }],
        )
        .expect("doc");
        let reference = doc.semantic_refs().into_iter().next().unwrap();
        shared.publish(doc);

        let missing = execute("tui_attention_set", TuiParams::default(), &shared);
        assert!(!missing.available);

        let stale = execute(
            "tui_attention_set",
            TuiParams {
                semantic_ref: Some("not-a-ref".into()),
                ..TuiParams::default()
            },
            &shared,
        );
        assert!(!stale.available);

        let ok = execute(
            "tui_attention_set",
            TuiParams {
                semantic_ref: Some(reference.to_string()),
                message: Some("focus".into()),
                ..TuiParams::default()
            },
            &shared,
        );
        assert!(ok.available);
        assert_eq!(shared.attention().semantic_ref.as_ref(), Some(&reference));
    }

    #[test]
    fn refresh_requires_the_active_companion_runtime() {
        let shared = SharedTuiState::new(Arc::new(BrowserSession::with_test_backend(
            FakeSessionBackend::new(),
        )));

        let result = execute("tui_refresh", TuiParams::default(), &shared);
        assert!(!result.available);
        assert_eq!(
            result.error.as_deref(),
            Some("active TUI runtime is required")
        );
    }

    #[test]
    fn refresh_reloads_and_publishes_a_fresh_semantic_revision() {
        let shared = SharedTuiState::new(Arc::new(BrowserSession::with_test_backend(
            FakeSessionBackend::new(),
        )));
        shared.activate_runtime();

        let result = execute("tui_refresh", TuiParams::default(), &shared);
        assert!(result.available);
        assert!(matches!(
            result.data,
            Some(TuiData::Refresh { ref revision, .. }) if revision == "fake:2"
        ));
        assert_eq!(
            shared
                .active()
                .expect("published document")
                .document
                .revision,
            "fake:2"
        );
        assert!(shared.lifecycle().is_ready());
    }
}

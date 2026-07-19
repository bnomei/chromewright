//! MCP tool that probes one resolved node for identity, layout, accessibility, and form state.
//!
//! Stale cursors may rebound via target recovery; compact detail is the default for agent loops.

use crate::dom::{Cursor, NodeRef};
use crate::error::{BrowserError, Result};
use crate::tools::{
    DocumentActionResult, TargetEnvelope, Tool, ToolContext, ToolResult, core::PublicTarget,
    services::inspection::execute_inspect_node,
};
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::de::Deserializer;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::OnceLock;

const INSPECT_NODE_JS: &str = include_str!("inspect_node.js");
static INSPECT_NODE_SHELL: OnceLock<crate::tools::browser_kernel::BrowserKernelTemplateShell> =
    OnceLock::new();

/// How much payload to return: compact agent defaults or full sections.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InspectDetail {
    /// Identity, a11y, form, and layout only — preferred for agent loops (default).
    Compact,
    /// Include bounded text/HTML/attribute/style sections with truncation metadata.
    Full,
}

fn default_detail() -> InspectDetail {
    InspectDetail::Compact
}

/// Selector or cursor target, detail level, and optional computed style names to probe.
///
/// MCP clients send a tagged [`PublicTarget`]; deserialization expands it into the
/// exclusive fields below for shared resolution.
#[derive(Debug, Clone, Serialize)]
pub struct InspectNodeParams {
    /// CSS selector target (exclusive with cursor/index/node_ref).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,

    /// Interactive index target (legacy path; prefer cursor).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,

    /// Revision-scoped node reference from a prior snapshot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_ref: Option<NodeRef>,

    /// Preferred revision-scoped cursor handoff from snapshot/inspect.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,

    /// Compact vs full section payload (default compact).
    #[serde(default = "default_detail")]
    pub detail: InspectDetail,

    /// Optional computed style property names to include in the layout section.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub style_names: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct StrictInspectNodeParams {
    /// Target to inspect.
    pub target: PublicTarget,
    #[serde(default = "default_detail")]
    pub detail: InspectDetail,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub style_names: Vec<String>,
}

impl From<StrictInspectNodeParams> for InspectNodeParams {
    fn from(params: StrictInspectNodeParams) -> Self {
        let (selector, cursor) = params.target.into_selector_or_cursor();
        Self {
            selector,
            index: None,
            node_ref: None,
            cursor,
            detail: params.detail,
            style_names: params.style_names,
        }
    }
}

impl<'de> Deserialize<'de> for InspectNodeParams {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        StrictInspectNodeParams::deserialize(deserializer).map(Into::into)
    }
}

impl JsonSchema for InspectNodeParams {
    fn schema_name() -> Cow<'static, str> {
        "InspectNodeParams".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        StrictInspectNodeParams::json_schema(generator)
    }
}

/// Probes a resolved target for identity, layout, accessibility, and form state.
#[derive(Default)]
pub struct InspectNodeTool;

/// Full inspect payload: target envelope, identity, a11y, form, layout, and optional sections.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InspectNodeOutput {
    /// Document identity at probe time.
    #[serde(flatten)]
    pub result: DocumentActionResult,
    /// Resolved target including recovery status and follow-up handles.
    pub target: TargetEnvelope,
    /// Tag, id, and class identity of the probed node.
    pub identity: InspectIdentity,
    /// Accessibility role/name and related a11y fields.
    pub accessibility: InspectAccessibility,
    /// Form control state when the node is an input-like element.
    pub form_state: InspectFormState,
    /// Geometry and optional computed styles.
    pub layout: InspectLayout,
    /// Surrounding DOM/context summary for agent reasoning.
    pub context: InspectContext,
    /// Frame/document boundary information when the node is not in the main frame.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boundary: Option<InspectBoundary>,
    /// Extra detail sections when `detail` is not compact.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sections: Option<InspectSections>,
    /// Field names omitted or shortened by resource limits.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub truncated_fields: Vec<String>,
}

/// DOM tag, id, and class list for the inspected node.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InspectIdentity {
    /// Lowercase HTML tag name.
    pub tag: String,
    /// Author `id` attribute when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Class tokens on the element.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub classes: Vec<String>,
}

/// ARIA tri-state values that may be a boolean or the mixed string.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum BooleanOrMixed {
    /// True/false ARIA state.
    Bool(bool),
    /// Mixed indeterminate state (serialized as `"mixed"`).
    Mixed(String),
}

/// Role, name, and common ARIA state flags for the inspected node.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InspectAccessibility {
    /// Computed accessibility role.
    pub role: String,
    /// Accessible name used by assistive tech and snapshot matching.
    pub name: String,
    /// Whether the node is the document's active element.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,
    /// ARIA checked tri-state when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checked: Option<BooleanOrMixed>,
    /// Whether the node is disabled for interaction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    /// Expanded state for disclosure widgets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expanded: Option<bool>,
    /// ARIA pressed tri-state when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pressed: Option<BooleanOrMixed>,
    /// Selected state for options and similar widgets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<bool>,
}

/// Form control value and editability flags when the node is form-like.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InspectFormState {
    /// Current form value when the node exposes one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Placeholder text when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    /// Whether the control rejects edits.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub readonly: Option<bool>,
    /// Whether the control is disabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
}

/// Bounding box, visibility, and pointer-event readiness for actionability diagnosis.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InspectLayout {
    /// Axis-aligned box in CSS pixels.
    pub bounding_box: InspectBoundingBox,
    /// Whether the element has a non-zero layout box and is not `visibility: hidden`.
    pub visible: bool,
    /// Whether any part of the element intersects the current viewport.
    pub visible_in_viewport: bool,
    /// Whether the element can receive pointer events at its center.
    pub receives_pointer_events: bool,
    /// Computed `pointer-events` CSS value when collected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pointer_events: Option<String>,
    /// CSS `cursor` property value (not a revision-scoped snapshot cursor).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

/// Axis-aligned box in CSS pixels for the inspected node.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InspectBoundingBox {
    /// Left edge in CSS pixels relative to the layout origin.
    pub x: f64,
    /// Top edge in CSS pixels relative to the layout origin.
    pub y: f64,
    /// Width in CSS pixels.
    pub width: f64,
    /// Height in CSS pixels.
    pub height: f64,
}

/// Document URL, frame depth, and shadow-root nesting for the inspected node.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InspectContext {
    /// URL of the document that owns the node (may differ from the top frame).
    pub document_url: String,
    /// Nesting depth of the frame that contains the node (0 = top frame).
    pub frame_depth: usize,
    /// Whether the node lives inside a shadow root.
    pub inside_shadow_root: bool,
}

/// Frame, shadow, or cross-origin boundary discovered while resolving the node.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InspectBoundary {
    /// Boundary kind (frame, shadow, cross-origin, …).
    pub kind: String,
    /// Availability/status token for the boundary.
    pub status: String,
    /// Whether content beyond the boundary is readable.
    pub available: bool,
    /// Frame URL when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// Full-detail text, HTML, attribute, and style sections with truncation metadata.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InspectSections {
    /// Bounded text content of the node.
    pub text: BoundedTextSection,
    /// Bounded outer HTML of the node.
    pub html: BoundedTextSection,
    /// Bounded attribute map.
    pub attributes: BoundedMapSection,
    /// Bounded computed style map for requested property names.
    pub styles: BoundedMapSection,
}

/// String payload that may be truncated against inspect size budgets.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BoundedTextSection {
    /// Returned text (may be shorter than `total_chars` when truncated).
    pub value: String,
    /// Whether the value was shortened to fit budgets.
    pub truncated: bool,
    /// Full Unicode scalar count before truncation.
    pub total_chars: usize,
}

/// Key/value map that may be truncated against inspect size budgets.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BoundedMapSection {
    /// Returned entries (may be fewer than `total_entries` when truncated).
    pub values: BTreeMap<String, String>,
    /// Whether entries were dropped to fit budgets.
    pub truncated: bool,
    /// Full entry count before truncation.
    pub total_entries: usize,
}

/// Decoded in-page probe result before mapping into `InspectNodeOutput`.
///
/// Carries resolution status, identity/layout/a11y sections, and optional failure codes from
/// the browser-kernel inspect script.
#[derive(Debug, Deserialize)]
pub(crate) struct InspectNodeProbePayload {
    /// Whether the in-page inspect script completed successfully.
    pub success: bool,
    /// Stable machine code when the script reports a known failure mode.
    #[serde(default)]
    pub code: Option<String>,
    /// Human-readable failure detail from the inspect script.
    #[serde(default)]
    pub error: Option<String>,
    /// Interactive index of the resolved node when known.
    #[serde(default)]
    pub actionable_index: Option<usize>,
    /// CSS selector the probe settled on after resolution.
    #[serde(default)]
    pub resolved_selector: Option<String>,
    /// Tag/id/class identity section when resolution succeeded.
    #[serde(default)]
    pub identity: Option<InspectIdentity>,
    /// Accessibility section when resolution succeeded.
    #[serde(default)]
    pub accessibility: Option<InspectAccessibility>,
    /// Form state section when the node is form-like.
    #[serde(default)]
    pub form_state: Option<InspectFormState>,
    /// Layout/geometry section when resolution succeeded.
    #[serde(default)]
    pub layout: Option<InspectLayout>,
    /// Document/frame context when resolution succeeded.
    #[serde(default)]
    pub context: Option<InspectContext>,
    /// Boundary info when the node is not in the top light DOM.
    #[serde(default)]
    pub boundary: Option<InspectBoundary>,
    /// Full boundary chain when multiple frames/shadows were crossed.
    #[serde(default)]
    pub boundaries: Option<Vec<InspectBoundary>>,
    /// Full-detail sections when the probe requested non-compact payload.
    #[serde(default)]
    pub sections: Option<InspectSections>,
}

impl Tool for InspectNodeTool {
    type Params = InspectNodeParams;
    type Output = InspectNodeOutput;

    fn name(&self) -> &str {
        "inspect_node"
    }

    fn description(&self) -> &str {
        "Inspect one node via target.selector/cursor. Stale cursors may rebound; snapshot rereads."
    }

    fn execute_typed(
        &self,
        params: InspectNodeParams,
        context: &mut ToolContext,
    ) -> Result<ToolResult> {
        execute_inspect_node(params, context)
    }
}

/// Render the inspect-node browser-kernel script with the given probe config payload.
pub(crate) fn build_inspect_node_js(config: &serde_json::Value) -> String {
    use crate::tools::browser_kernel::render_browser_kernel_script;
    render_browser_kernel_script(
        &INSPECT_NODE_SHELL,
        INSPECT_NODE_JS,
        "__INSPECT_CONFIG__",
        config,
    )
}

/// Parse a CDP evaluation value (object or JSON string) into [`InspectNodeProbePayload`].
///
/// Missing results become a structured `inspect_failed` payload rather than a hard error.
pub(crate) fn decode_probe_payload(
    value: Option<serde_json::Value>,
) -> Result<InspectNodeProbePayload> {
    let parsed = if let Some(serde_json::Value::String(json_str)) = value {
        serde_json::from_str::<serde_json::Value>(&json_str).map_err(BrowserError::from)?
    } else {
        value.unwrap_or(serde_json::json!({
            "success": false,
            "code": "inspect_failed",
            "error": "No result returned",
        }))
    };

    serde_json::from_value(parsed).map_err(BrowserError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::BrowserSession;
    use crate::browser::backend::FakeSessionBackend;
    use crate::tools::limits::{MAX_INSPECT_CLASSES, MAX_INSPECT_COMPACT_CHARS};
    use crate::tools::{OPERATION_METRICS_METADATA_KEY, Tool, ToolContext};
    use schemars::schema_for;
    use serde_json::json;

    #[test]
    fn test_decode_probe_payload_accepts_json_string() {
        let payload = decode_probe_payload(Some(serde_json::Value::String(
            serde_json::json!({
                "success": true,
                "identity": {
                    "tag": "button",
                    "id": "save",
                    "classes": ["primary"]
                },
                "accessibility": {
                    "role": "button",
                    "name": "Save"
                },
                "form_state": {},
                "layout": {
                    "bounding_box": {
                        "x": 0.0,
                        "y": 0.0,
                        "width": 10.0,
                        "height": 20.0
                    },
                    "visible": true,
                    "visible_in_viewport": true,
                    "receives_pointer_events": true
                },
                "context": {
                    "document_url": "https://example.com",
                    "frame_depth": 0,
                    "inside_shadow_root": false
                }
            })
            .to_string(),
        )))
        .expect("probe payload should parse");

        assert!(payload.success);
        assert_eq!(payload.identity.unwrap().tag, "button");
    }

    #[test]
    fn test_inspect_node_js_prefers_selector_before_target_index() {
        let inspect_js = build_inspect_node_js(&serde_json::json!({
            "selector": "#save",
            "target_index": 1,
            "detail": "compact",
            "style_names": [],
        }));

        assert!(inspect_js.contains("function resolveTargetMatch(config, options)"));
        assert!(
            inspect_js.contains(
                "const resolved = resolveTargetMatch(config, { collectBoundaries: true });"
            )
        );
        assert!(inspect_js.contains("querySelectorAcrossScopes("));
        assert!(inspect_js.contains("searchActionableIndex(config.target_index)"));
        assert!(inspect_js.contains("resolved_selector: buildSelector(element),"));
    }

    #[test]
    fn test_inspect_node_params_deserialize_strict_target_and_hide_legacy_fields() {
        let params: InspectNodeParams = serde_json::from_value(json!({
            "target": {
                "kind": "selector",
                "selector": "#save"
            },
            "detail": "full",
            "style_names": ["display"]
        }))
        .expect("strict inspect params should deserialize");

        assert_eq!(params.selector.as_deref(), Some("#save"));
        assert_eq!(params.index, None);
        assert_eq!(params.node_ref, None);
        assert_eq!(params.cursor, None);
        assert_eq!(params.detail, InspectDetail::Full);
        assert_eq!(params.style_names, vec!["display".to_string()]);

        let plain_string_params: InspectNodeParams = serde_json::from_value(json!({
            "target": "#save",
            "detail": "full",
            "style_names": ["display"]
        }))
        .expect("plain string selector target should deserialize");
        assert_eq!(plain_string_params.selector.as_deref(), Some("#save"));
        assert_eq!(plain_string_params.detail, InspectDetail::Full);
        assert_eq!(plain_string_params.style_names, vec!["display".to_string()]);

        let error = serde_json::from_value::<InspectNodeParams>(json!({
            "cursor": {
                "node_ref": {
                    "document_id": "doc-1",
                    "revision": "main:1",
                    "index": 1
                },
                "selector": "#save",
                "index": 1,
                "role": "button",
                "name": "Save"
            }
        }))
        .expect_err("legacy cursor field should be rejected");
        assert!(error.to_string().contains("unknown field `cursor`"));

        let schema = schema_for!(InspectNodeParams);
        let schema_json = serde_json::to_value(&schema).expect("schema should serialize");
        let properties = schema_json
            .get("properties")
            .and_then(|value| value.as_object())
            .expect("inspect_node params schema should expose properties");
        assert!(properties.contains_key("target"));
        assert!(!properties.contains_key("selector"));
        assert!(!properties.contains_key("index"));
        assert!(!properties.contains_key("node_ref"));
        assert!(!properties.contains_key("cursor"));
    }

    #[test]
    fn test_inspect_node_tool_executes_against_fake_backend_and_attaches_metrics() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let tool = InspectNodeTool;
        let mut context = ToolContext::new(&session);

        let result = tool
            .execute_typed(
                InspectNodeParams {
                    selector: Some("#fake-target".to_string()),
                    index: None,
                    node_ref: None,
                    cursor: None,
                    detail: InspectDetail::Compact,
                    style_names: Vec::new(),
                },
                &mut context,
            )
            .expect("inspect_node should succeed");

        assert!(result.success);
        let data = result.data.expect("inspect_node should include data");
        assert_eq!(data["identity"]["tag"].as_str(), Some("button"));
        assert!(data.get("cursor").is_none());
        assert!(result.metadata.contains_key(OPERATION_METRICS_METADATA_KEY));
    }

    #[test]
    fn test_inspect_node_tool_returns_structured_failure_for_incomplete_probe_payload() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let tool = InspectNodeTool;
        let mut context = ToolContext::new(&session);

        let result = tool
            .execute_typed(
                InspectNodeParams {
                    selector: Some("#fake-target".to_string()),
                    index: None,
                    node_ref: None,
                    cursor: None,
                    detail: InspectDetail::Compact,
                    style_names: vec!["__incomplete_payload__".to_string()],
                },
                &mut context,
            )
            .expect("incomplete inspect payload should stay a tool failure");

        assert!(!result.success);
        let data = result
            .data
            .expect("incomplete inspect payload should include details");
        assert_eq!(data["code"].as_str(), Some("inspect_payload_incomplete"));
        assert_eq!(
            data["recovery"]["suggested_tool"].as_str(),
            Some("snapshot")
        );
        let missing_fields = data["details"]["missing_fields"]
            .as_array()
            .expect("missing_fields should be present");
        assert!(
            missing_fields
                .iter()
                .any(|field| field.as_str() == Some("identity"))
        );
    }

    #[test]
    fn test_inspect_node_tool_truncates_compact_fields_with_metadata() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let tool = InspectNodeTool;
        let mut context = ToolContext::new(&session);

        let result = tool
            .execute_typed(
                InspectNodeParams {
                    selector: Some("#fake-target".to_string()),
                    index: None,
                    node_ref: None,
                    cursor: None,
                    detail: InspectDetail::Compact,
                    style_names: vec!["__long_compact_payload__".to_string()],
                },
                &mut context,
            )
            .expect("inspect_node should succeed with bounded compact fields");

        assert!(result.success);
        let data = result.data.expect("inspect_node should include data");
        assert_eq!(
            data["identity"]["tag"]
                .as_str()
                .expect("tag should serialize")
                .chars()
                .count(),
            MAX_INSPECT_COMPACT_CHARS
        );
        assert_eq!(
            data["accessibility"]["name"]
                .as_str()
                .expect("name should serialize")
                .chars()
                .count(),
            MAX_INSPECT_COMPACT_CHARS
        );
        assert_eq!(
            data["identity"]["classes"]
                .as_array()
                .expect("classes should serialize")
                .len(),
            MAX_INSPECT_CLASSES
        );
        let truncated_fields = data["truncated_fields"]
            .as_array()
            .expect("truncation metadata should be present");
        assert!(
            truncated_fields
                .iter()
                .any(|field| field.as_str() == Some("identity.tag"))
        );
        assert!(
            truncated_fields
                .iter()
                .any(|field| field.as_str() == Some("identity.classes"))
        );
        assert!(
            truncated_fields
                .iter()
                .any(|field| field.as_str() == Some("accessibility.name"))
        );
        assert!(
            truncated_fields
                .iter()
                .any(|field| field.as_str() == Some("context.document_url"))
        );
    }
}

use crate::dom::{Cursor, DocumentMetadata, NodeRef};
use crate::error::{BrowserError, Result};
use crate::tools::{
    TargetEnvelope, Tool, ToolContext, ToolResult, services::inspection::execute_inspect_node,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const INSPECT_NODE_JS: &str = include_str!("inspect_node.js");

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InspectDetail {
    Compact,
    Full,
}

fn default_detail() -> InspectDetail {
    InspectDetail::Compact
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InspectNodeParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_ref: Option<NodeRef>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,

    #[serde(default = "default_detail")]
    pub detail: InspectDetail,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub style_names: Vec<String>,
}

#[derive(Default)]
pub struct InspectNodeTool;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InspectNodeOutput {
    pub action: String,
    pub document: DocumentMetadata,
    pub target: TargetEnvelope,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,
    pub identity: InspectIdentity,
    pub accessibility: InspectAccessibility,
    pub form_state: InspectFormState,
    pub layout: InspectLayout,
    pub context: InspectContext,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boundary: Option<InspectBoundary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sections: Option<InspectSections>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InspectIdentity {
    pub tag: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub classes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum BooleanOrMixed {
    Bool(bool),
    Mixed(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InspectAccessibility {
    pub role: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checked: Option<BooleanOrMixed>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expanded: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pressed: Option<BooleanOrMixed>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InspectFormState {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub readonly: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InspectLayout {
    pub bounding_box: InspectBoundingBox,
    pub visible: bool,
    pub visible_in_viewport: bool,
    pub receives_pointer_events: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pointer_events: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InspectBoundingBox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InspectContext {
    pub document_url: String,
    pub frame_depth: usize,
    pub inside_shadow_root: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InspectBoundary {
    pub kind: String,
    pub status: String,
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InspectSections {
    pub text: BoundedTextSection,
    pub html: BoundedTextSection,
    pub attributes: BoundedMapSection,
    pub styles: BoundedMapSection,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BoundedTextSection {
    pub value: String,
    pub truncated: bool,
    pub total_chars: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BoundedMapSection {
    pub values: BTreeMap<String, String>,
    pub truncated: bool,
    pub total_entries: usize,
}

#[derive(Debug, Deserialize)]
pub(crate) struct InspectNodeProbePayload {
    pub success: bool,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub actionable_index: Option<usize>,
    #[serde(default)]
    pub identity: Option<InspectIdentity>,
    #[serde(default)]
    pub accessibility: Option<InspectAccessibility>,
    #[serde(default)]
    pub form_state: Option<InspectFormState>,
    #[serde(default)]
    pub layout: Option<InspectLayout>,
    #[serde(default)]
    pub context: Option<InspectContext>,
    #[serde(default)]
    pub boundary: Option<InspectBoundary>,
    #[serde(default)]
    pub boundaries: Option<Vec<InspectBoundary>>,
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
        "Inspect one node after snapshot. Prefer cursor handles; selector/index/node_ref still work."
    }

    fn execute_typed(
        &self,
        params: InspectNodeParams,
        context: &mut ToolContext,
    ) -> Result<ToolResult> {
        execute_inspect_node(params, context)
    }
}

pub(crate) fn build_inspect_node_js(config: &serde_json::Value) -> String {
    use crate::tools::browser_kernel::render_browser_kernel_script;
    render_browser_kernel_script(INSPECT_NODE_JS, "__INSPECT_CONFIG__", config)
}

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
    use crate::tools::{OPERATION_METRICS_METADATA_KEY, Tool, ToolContext};

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
    }

    #[test]
    fn test_inspect_node_tool_executes_against_fake_backend_and_attaches_metrics() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let tool = InspectNodeTool::default();
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
        assert!(result.metadata.contains_key(OPERATION_METRICS_METADATA_KEY));
    }

    #[test]
    fn test_inspect_node_tool_returns_structured_failure_for_incomplete_probe_payload() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let tool = InspectNodeTool::default();
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
        let missing_fields = data["missing_fields"]
            .as_array()
            .expect("missing_fields should be present");
        assert!(
            missing_fields
                .iter()
                .any(|field| field.as_str() == Some("identity"))
        );
    }
}

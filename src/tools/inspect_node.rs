use crate::dom::{Cursor, DocumentMetadata, NodeRef};
use crate::error::{BrowserError, Result};
use crate::tools::{
    DocumentEnvelopeOptions, TargetEnvelope, TargetResolution, Tool, ToolContext, ToolResult,
    build_document_envelope, resolve_target_with_cursor,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const INSPECT_NODE_JS: &str = include_str!("inspect_node.js");
const DEFAULT_STYLE_NAMES: &[&str] = &[
    "display",
    "visibility",
    "pointer-events",
    "position",
    "z-index",
    "opacity",
    "cursor",
    "overflow",
];
const MAX_STYLE_NAMES: usize = 12;

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
struct InspectNodeProbePayload {
    success: bool,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    actionable_index: Option<usize>,
    #[serde(default)]
    identity: Option<InspectIdentity>,
    #[serde(default)]
    accessibility: Option<InspectAccessibility>,
    #[serde(default)]
    form_state: Option<InspectFormState>,
    #[serde(default)]
    layout: Option<InspectLayout>,
    #[serde(default)]
    context: Option<InspectContext>,
    #[serde(default)]
    boundary: Option<InspectBoundary>,
    #[serde(default)]
    boundaries: Option<Vec<InspectBoundary>>,
    #[serde(default)]
    sections: Option<InspectSections>,
}

impl Tool for InspectNodeTool {
    type Params = InspectNodeParams;
    type Output = InspectNodeOutput;

    fn name(&self) -> &str {
        "inspect_node"
    }

    fn execute_typed(
        &self,
        params: InspectNodeParams,
        context: &mut ToolContext,
    ) -> Result<ToolResult> {
        let InspectNodeParams {
            selector,
            index,
            node_ref,
            cursor,
            detail,
            style_names,
        } = params;
        let target = {
            let dom = context.get_dom()?;
            match resolve_target_with_cursor(
                "inspect_node",
                selector,
                index,
                node_ref,
                cursor,
                Some(dom),
            )? {
                TargetResolution::Resolved(target) => target,
                TargetResolution::Failure(failure) => return Ok(failure),
            }
        };

        let target_index = target
            .cursor
            .as_ref()
            .map(|cursor| cursor.index)
            .or(target.index);
        let styles = if style_names.is_empty() {
            DEFAULT_STYLE_NAMES
                .iter()
                .map(|name| (*name).to_string())
                .collect::<Vec<_>>()
        } else {
            style_names.into_iter().take(MAX_STYLE_NAMES).collect::<Vec<_>>()
        };
        let detail_name = match detail {
            InspectDetail::Compact => "compact",
            InspectDetail::Full => "full",
        };
        let config = serde_json::json!({
            "selector": target.selector,
            "target_index": target_index,
            "detail": detail_name,
            "style_names": styles,
        });
        let inspect_js = INSPECT_NODE_JS.replace("__INSPECT_CONFIG__", &config.to_string());
        let evaluation = context
            .session
            .tab()?
            .evaluate(&inspect_js, false)
            .map_err(|e| BrowserError::ToolExecutionFailed {
                tool: "inspect_node".to_string(),
                reason: e.to_string(),
            })?;
        let payload = decode_probe_payload(evaluation.value)?;

        if !payload.success {
            let error = payload
                .error
                .clone()
                .unwrap_or_else(|| "Node inspection failed".to_string());
            let boundaries = payload.boundaries.unwrap_or_default();
            return Ok(ToolResult::failure_with(
                error.clone(),
                serde_json::json!({
                    "code": payload.code.unwrap_or_else(|| "inspect_failed".to_string()),
                    "error": error,
                    "target": target.to_target_envelope(),
                    "boundaries": boundaries,
                }),
            ));
        }

        let mut envelope =
            build_document_envelope(context, Some(&target), DocumentEnvelopeOptions::minimal())?;
        let mut target_envelope = envelope.target.take().expect("inspect_node should keep target");
        if target_envelope.cursor.is_none() {
            target_envelope.cursor = match payload.actionable_index {
                Some(index) => context.get_dom()?.cursor_for_index(index),
                None => None,
            };
        }
        let cursor = target_envelope.cursor.clone();
        if cursor.is_none() {
            let error = if payload
                .context
                .as_ref()
                .map(|context| context.frame_depth > 0)
                .unwrap_or(false)
            {
                "inspect_node matched an element inside an iframe, but it is not cursor-addressable yet"
                    .to_string()
            } else {
                "inspect_node requires a selector that resolves to an actionable cursor".to_string()
            };
            let code = if payload
                .context
                .as_ref()
                .map(|context| context.frame_depth > 0)
                .unwrap_or(false)
            {
                "unsupported_frame_context"
            } else {
                "selector_not_cursor_addressable"
            };

            return Ok(ToolResult::failure_with(
                error.clone(),
                serde_json::json!({
                    "code": code,
                    "error": error,
                    "target": target_envelope,
                    "context": payload.context,
                    "boundary": payload.boundary,
                }),
            ));
        }

        Ok(ToolResult::success_with(InspectNodeOutput {
            action: "inspect_node".to_string(),
            document: envelope.document,
            target: target_envelope,
            cursor,
            identity: payload.identity.expect("probe should include identity"),
            accessibility: payload
                .accessibility
                .expect("probe should include accessibility"),
            form_state: payload.form_state.expect("probe should include form_state"),
            layout: payload.layout.expect("probe should include layout"),
            context: payload.context.expect("probe should include context"),
            boundary: payload.boundary,
            sections: payload.sections,
        }))
    }
}

fn decode_probe_payload(value: Option<serde_json::Value>) -> Result<InspectNodeProbePayload> {
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
}

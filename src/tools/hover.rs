use crate::dom::{Cursor, NodeRef};
use crate::error::{BrowserError, Result};
use crate::tools::{
    DocumentEnvelope, TargetEnvelope, TargetResolution, Tool, ToolContext, ToolResult,
    actionability::ActionabilityPredicate,
    browser_kernel::render_browser_kernel_script,
    services::interaction::{
        ActionabilityWaitState, DEFAULT_ACTIONABILITY_TIMEOUT_MS, TargetStatus,
        build_actionability_failure, build_interaction_failure, build_interaction_handoff,
        decode_action_result, resolve_interaction_target, wait_for_actionability,
    },
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parameters for the hover tool
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HoverParams {
    /// CSS selector (use either this or index, not both)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,

    /// Element index from DOM tree (use either this or selector, not both)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,

    /// Revision-scoped node reference from the snapshot tool
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_ref: Option<NodeRef>,

    /// Cursor from the snapshot or inspect_node tools
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,
}

/// Tool for hovering over elements
#[derive(Default)]
pub struct HoverTool;

const HOVER_JS: &str = include_str!("hover.js");

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HoverElement {
    pub tag_name: String,
    pub id: String,
    pub class_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HoverOutput {
    #[serde(flatten)]
    pub envelope: DocumentEnvelope,
    pub action: String,
    pub target_before: TargetEnvelope,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_after: Option<TargetEnvelope>,
    pub target_status: TargetStatus,
    pub element: HoverElement,
}

impl Tool for HoverTool {
    type Params = HoverParams;
    type Output = HoverOutput;

    fn name(&self) -> &str {
        "hover"
    }

    fn description(&self) -> &str {
        "Reveal hover state. Usually after snapshot; next snapshot or click."
    }

    fn execute_typed(&self, params: HoverParams, context: &mut ToolContext) -> Result<ToolResult> {
        let HoverParams {
            selector,
            index,
            node_ref,
            cursor,
        } = params;
        let target = match resolve_interaction_target(
            "hover", selector, index, node_ref, cursor, context,
        )? {
            TargetResolution::Resolved(target) => target,
            TargetResolution::Failure(failure) => return Ok(context.finish(failure)),
        };

        let predicates = hover_actionability_predicates();
        match wait_for_actionability(
            context,
            &target,
            predicates,
            DEFAULT_ACTIONABILITY_TIMEOUT_MS,
        )? {
            ActionabilityWaitState::Ready => {}
            ActionabilityWaitState::TimedOut(probe) => {
                return build_actionability_failure(
                    "hover",
                    context.session,
                    &target,
                    &probe,
                    predicates,
                    None,
                )
                .map(|result| context.finish(result));
            }
        }

        let hover_config = serde_json::json!({
            "selector": target.selector,
            "target_index": target.cursor.as_ref().map(|cursor| cursor.index).or(target.index),
        });
        let hover_js = build_hover_js(&hover_config);

        context.record_browser_evaluation();
        let result = context
            .session
            .evaluate(&hover_js, false)
            .map_err(|e| match e {
                BrowserError::EvaluationFailed(reason) => BrowserError::ToolExecutionFailed {
                    tool: "hover".to_string(),
                    reason,
                },
                other => other,
            })?;

        let hover_result = match parse_hover_result(result.value) {
            Ok(result) => result,
            Err(reason) => {
                return Ok(context.finish(ToolResult::failure_with(
                    reason.clone(),
                    serde_json::json!({
                        "code": "invalid_hover_payload",
                        "error": reason,
                        "recovery": {
                            "suggested_tool": "snapshot",
                        }
                    }),
                )));
            }
        };

        match hover_result {
            HoverParseResult::Success(element) => {
                let handoff = build_interaction_handoff(context, &target)?;
                Ok(context.finish(ToolResult::success_with(HoverOutput {
                    envelope: handoff.envelope,
                    action: "hover".to_string(),
                    target_before: handoff.target_before,
                    target_after: handoff.target_after,
                    target_status: handoff.target_status,
                    element,
                })))
            }
            HoverParseResult::Failure { code, error } => build_interaction_failure(
                "hover",
                context.session,
                &target,
                code,
                error,
                Vec::new(),
                None,
            )
            .map(|result| context.finish(result)),
        }
    }
}

fn build_hover_js(config: &serde_json::Value) -> String {
    render_browser_kernel_script(HOVER_JS, "__HOVER_CONFIG__", config)
}

#[derive(Debug)]
enum HoverParseResult {
    Success(HoverElement),
    Failure { code: String, error: String },
}

fn parse_hover_result(
    value: Option<serde_json::Value>,
) -> std::result::Result<HoverParseResult, String> {
    let result_json = decode_action_result(
        value,
        serde_json::json!({
            "success": false,
            "code": "target_detached",
            "error": "Element is no longer present"
        }),
    )
    .map_err(|error| format!("Failed to parse hover result: {}", error))?;

    if result_json["success"].as_bool() == Some(true) {
        Ok(HoverParseResult::Success(HoverElement {
            tag_name: required_hover_string_field(&result_json, "tagName")?.to_string(),
            id: required_hover_string_field(&result_json, "id")?.to_string(),
            class_name: required_hover_string_field(&result_json, "className")?.to_string(),
        }))
    } else {
        Ok(HoverParseResult::Failure {
            code: result_json["code"]
                .as_str()
                .unwrap_or("target_detached")
                .to_string(),
            error: result_json["error"]
                .as_str()
                .unwrap_or("Hover failed")
                .to_string(),
        })
    }
}

fn required_hover_string_field<'a>(
    result_json: &'a serde_json::Value,
    field: &'static str,
) -> std::result::Result<&'a str, String> {
    result_json
        .get(field)
        .and_then(|value| value.as_str())
        .ok_or_else(|| {
            format!("Hover returned an incomplete success payload: missing string field '{field}'")
        })
}

fn hover_actionability_predicates() -> &'static [ActionabilityPredicate] {
    &[
        ActionabilityPredicate::Present,
        ActionabilityPredicate::Visible,
        ActionabilityPredicate::Stable,
        ActionabilityPredicate::ReceivesEvents,
        ActionabilityPredicate::UnobscuredCenter,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::BrowserSession;
    use crate::browser::backend::{ScriptEvaluation, SessionBackend, TabDescriptor};
    use crate::dom::{AriaChild, AriaNode, DocumentMetadata, DomTree};
    use crate::tools::{OPERATION_METRICS_METADATA_KEY, Tool, ToolContext};
    use serde_json::Value;
    use std::time::Duration;

    struct InvalidHoverPayloadBackend;

    impl InvalidHoverPayloadBackend {
        fn dom() -> DomTree {
            let mut root = AriaNode::fragment();
            root.children.push(AriaChild::Node(Box::new(
                AriaNode::new("button", "Fake target")
                    .with_index(0)
                    .with_box(true, Some("pointer".to_string())),
            )));

            let mut dom = DomTree::new(root);
            dom.document = DocumentMetadata {
                document_id: "tab-1".to_string(),
                revision: "fake:1".to_string(),
                url: "https://example.com".to_string(),
                title: "Example".to_string(),
                ready_state: "complete".to_string(),
                frames: Vec::new(),
            };
            dom.replace_selectors(vec!["#fake-target".to_string()]);
            dom
        }
    }

    impl SessionBackend for InvalidHoverPayloadBackend {
        fn navigate(&self, _url: &str) -> crate::error::Result<()> {
            unreachable!("navigate is not used in this test")
        }

        fn wait_for_navigation(&self) -> crate::error::Result<()> {
            unreachable!("wait_for_navigation is not used in this test")
        }

        fn wait_for_document_ready_with_timeout(
            &self,
            _timeout: Duration,
        ) -> crate::error::Result<()> {
            unreachable!("wait_for_document_ready_with_timeout is not used in this test")
        }

        fn document_metadata(&self) -> crate::error::Result<DocumentMetadata> {
            Ok(Self::dom().document)
        }

        fn extract_dom(&self) -> crate::error::Result<DomTree> {
            Ok(Self::dom())
        }

        fn extract_dom_with_prefix(&self, _prefix: &str) -> crate::error::Result<DomTree> {
            Ok(Self::dom())
        }

        fn evaluate(
            &self,
            script: &str,
            _await_promise: bool,
        ) -> crate::error::Result<ScriptEvaluation> {
            if script.contains("\"predicates\"") {
                return Ok(ScriptEvaluation {
                    value: Some(serde_json::json!({
                        "present": true,
                        "visible": true,
                        "stable": true,
                        "in_viewport": true,
                        "receives_events": true,
                        "unobscured_center": true,
                    })),
                    description: None,
                    type_name: Some("Object".to_string()),
                });
            }

            if script.contains("MouseEvent(\"mouseover\"") {
                return Ok(ScriptEvaluation {
                    value: Some(Value::String(
                        serde_json::json!({
                            "success": true,
                            "id": "fake-target",
                            "className": "fake",
                        })
                        .to_string(),
                    )),
                    description: None,
                    type_name: Some("String".to_string()),
                });
            }

            unreachable!("unexpected script in invalid hover payload test: {script}");
        }

        fn capture_screenshot(&self, _full_page: bool) -> crate::error::Result<Vec<u8>> {
            unreachable!("capture_screenshot is not used in this test")
        }

        fn press_key(&self, _key: &str) -> crate::error::Result<()> {
            unreachable!("press_key is not used in this test")
        }

        fn list_tabs(&self) -> crate::error::Result<Vec<TabDescriptor>> {
            unreachable!("list_tabs is not used in this test")
        }

        fn active_tab(&self) -> crate::error::Result<TabDescriptor> {
            unreachable!("active_tab is not used in this test")
        }

        fn open_tab(&self, _url: &str) -> crate::error::Result<TabDescriptor> {
            unreachable!("open_tab is not used in this test")
        }

        fn activate_tab(&self, _tab_id: &str) -> crate::error::Result<()> {
            unreachable!("activate_tab is not used in this test")
        }

        fn close_tab(&self, _tab_id: &str, _with_unload: bool) -> crate::error::Result<()> {
            unreachable!("close_tab is not used in this test")
        }

        fn close(&self) -> crate::error::Result<()> {
            unreachable!("close is not used in this test")
        }
    }

    #[test]
    fn test_parse_hover_result_success() {
        let result = parse_hover_result(Some(serde_json::Value::String(
            r#"{"success":true,"tagName":"BUTTON","id":"save","className":"primary"}"#.to_string(),
        )))
        .expect("hover result should parse");

        match result {
            HoverParseResult::Success(element) => {
                assert_eq!(element.tag_name, "BUTTON");
                assert_eq!(element.id, "save");
                assert_eq!(element.class_name, "primary");
            }
            HoverParseResult::Failure { error, .. } => panic!("unexpected failure: {error}"),
        }
    }

    #[test]
    fn test_parse_hover_result_failure_uses_code_and_error() {
        let result = parse_hover_result(Some(serde_json::json!({
            "success": false,
            "code": "target_detached",
            "error": "Element not found"
        })))
        .expect("hover result should parse");

        match result {
            HoverParseResult::Failure { code, error } => {
                assert_eq!(code, "target_detached");
                assert_eq!(error, "Element not found");
            }
            HoverParseResult::Success(_) => panic!("expected failure"),
        }
    }

    #[test]
    fn test_parse_hover_result_rejects_invalid_json_string() {
        let error = parse_hover_result(Some(serde_json::Value::String("not-json".to_string())))
            .expect_err("invalid JSON should fail");

        assert!(error.contains("Failed to parse hover result"));
        assert!(error.contains("JSON error"));
    }

    #[test]
    fn test_parse_hover_result_rejects_incomplete_success_payload() {
        let error = parse_hover_result(Some(serde_json::json!({
            "success": true,
            "id": "save",
            "className": "primary"
        })))
        .expect_err("incomplete hover success payload should fail");

        assert!(error.contains("missing string field 'tagName'"));
    }

    #[test]
    fn test_hover_tool_returns_structured_failure_for_invalid_payload() {
        let session = BrowserSession::with_test_backend(InvalidHoverPayloadBackend);
        let tool = HoverTool::default();
        let mut context = ToolContext::new(&session);

        let result = tool
            .execute_typed(
                HoverParams {
                    selector: Some("#fake-target".to_string()),
                    index: None,
                    node_ref: None,
                    cursor: None,
                },
                &mut context,
            )
            .expect("invalid hover payload should stay a tool failure");

        assert!(!result.success);
        assert!(
            result
                .error
                .as_deref()
                .unwrap_or_default()
                .contains("missing string field 'tagName'")
        );
        let data = result
            .data
            .expect("invalid hover payload failure should include details");
        assert_eq!(data["code"].as_str(), Some("invalid_hover_payload"));
        assert_eq!(
            data["recovery"]["suggested_tool"].as_str(),
            Some("snapshot")
        );
        let metrics = result.metadata[OPERATION_METRICS_METADATA_KEY]
            .as_object()
            .expect("metrics metadata should be present on failures");
        assert_eq!(metrics["browser_evaluations"].as_u64(), Some(3));
    }

    #[test]
    fn test_hover_js_prefers_selector_before_target_index() {
        let hover_js = build_hover_js(&serde_json::json!({
            "selector": "#save",
            "target_index": 1,
        }));

        assert!(hover_js.contains("function resolveTargetMatch(config, options)"));
        assert!(hover_js.contains("const element = resolveTargetElement(config);"));
        assert!(hover_js.contains("querySelectorAcrossScopes("));
        assert!(hover_js.contains("searchActionableIndex(config.target_index)"));
    }
}

use crate::error::{BrowserError, Result};
use crate::tools::core::structured_tool_failure;
use crate::tools::{
    DocumentActionResult, DocumentEnvelopeOptions, Tool, ToolContext, ToolResult,
    build_document_envelope,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parameters for the scroll tool
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ScrollParams {
    /// Amount to scroll in pixels (positive for down, negative for up).
    /// If not provided, scrolls by one viewport height.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<i32>,
}

/// Tool for scrolling the page
#[derive(Default)]
pub struct ScrollTool;

const SCROLL_JS: &str = include_str!("scroll.js");

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ViewportAfter {
    pub scroll_y: i64,
    pub is_at_top: bool,
    pub is_at_bottom: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ScrollOutput {
    #[serde(flatten)]
    pub result: DocumentActionResult,
    pub scrolled: i64,
    pub is_at_bottom: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub viewport_after: Option<ViewportAfter>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct RawScrollOutput {
    #[serde(alias = "actualScroll")]
    actual_scroll: i64,
    #[serde(alias = "isAtBottom")]
    is_at_bottom: bool,
    #[serde(alias = "scrollY")]
    scroll_y: i64,
    #[serde(alias = "isAtTop")]
    is_at_top: bool,
}

impl Tool for ScrollTool {
    type Params = ScrollParams;
    type Output = ScrollOutput;

    fn name(&self) -> &str {
        "scroll"
    }

    fn description(&self) -> &str {
        "Scroll the page. Returns viewport hints; snapshot only for broader rereads."
    }

    fn execute_typed(&self, params: ScrollParams, context: &mut ToolContext) -> Result<ToolResult> {
        let config = serde_json::json!({
            "amount": params.amount
        });
        let scroll_js = build_scroll_js(&config);

        context.record_browser_evaluation();
        let result = context
            .session
            .evaluate(&scroll_js, true)
            .map_err(|e| match e {
                BrowserError::EvaluationFailed(reason) => BrowserError::ToolExecutionFailed {
                    tool: "scroll".to_string(),
                    reason,
                },
                other => other,
            })?;

        let result_json = match parse_raw_scroll_output(result.value) {
            Ok(result_json) => result_json,
            Err((reason, received_type)) => {
                return Ok(context.finish(structured_tool_failure(
                    "invalid_scroll_payload",
                    reason,
                    None,
                    None,
                    Some(serde_json::json!({
                        "suggested_tool": "snapshot",
                    })),
                    Some(serde_json::json!({
                        "received_type": received_type,
                    })),
                )));
            }
        };

        context.invalidate_dom();
        let envelope = build_document_envelope(context, None, DocumentEnvelopeOptions::minimal())?;

        Ok(context.finish(ToolResult::success_with(build_scroll_output(
            result_json,
            DocumentActionResult::new("scroll", envelope.document),
        ))))
    }
}

fn build_scroll_js(config: &serde_json::Value) -> String {
    // Scroll only needs the page globals, so this stays a deliberate non-kernel exception.
    SCROLL_JS.replace("__SCROLL_CONFIG__", &config.to_string())
}

fn build_scroll_output(result_json: RawScrollOutput, result: DocumentActionResult) -> ScrollOutput {
    let viewport_after = Some(ViewportAfter {
        scroll_y: result_json.scroll_y,
        is_at_top: result_json.is_at_top,
        is_at_bottom: result_json.is_at_bottom,
    });

    let message = if result_json.is_at_bottom {
        format!(
            "Scrolled {} pixels. Reached the bottom of the page.",
            result_json.actual_scroll
        )
    } else {
        format!(
            "Scrolled {} pixels. Did not reach the bottom of the page.",
            result_json.actual_scroll
        )
    };

    ScrollOutput {
        result,
        scrolled: result_json.actual_scroll,
        is_at_bottom: result_json.is_at_bottom,
        message,
        viewport_after,
    }
}

fn parse_raw_scroll_output(
    value: Option<serde_json::Value>,
) -> std::result::Result<RawScrollOutput, (String, &'static str)> {
    match value {
        Some(serde_json::Value::String(json_str)) => {
            serde_json::from_str(&json_str).map_err(|error| {
                (
                    format!("Failed to parse scroll result: {}", error),
                    "string",
                )
            })
        }
        Some(other) => {
            let received_type = value_kind(&other);
            serde_json::from_value(other).map_err(|error| {
                (
                    format!("Failed to deserialize scroll result: {}", error),
                    received_type,
                )
            })
        }
        None => Err(("Scroll returned no data".to_string(), "null")),
    }
}

fn value_kind(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::BrowserSession;
    use crate::browser::backend::{ScriptEvaluation, SessionBackend, TabDescriptor};
    use crate::tools::{OPERATION_METRICS_METADATA_KEY, Tool, ToolContext};
    use crate::{dom::DocumentMetadata, dom::DomTree};
    use serde_json::Value;
    use std::time::Duration;

    struct InvalidScrollPayloadBackend;

    #[test]
    fn test_build_scroll_js_injects_config_without_placeholder() {
        let scroll_js = build_scroll_js(&serde_json::json!({ "amount": 240 }));

        assert!(scroll_js.contains(r#"const config = {"amount":240};"#));
        assert!(!scroll_js.contains("__SCROLL_CONFIG__"));
    }

    impl SessionBackend for InvalidScrollPayloadBackend {
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
            unreachable!("document_metadata is not used in this test")
        }

        fn extract_dom(&self) -> crate::error::Result<DomTree> {
            unreachable!("extract_dom is not used in this test")
        }

        fn extract_dom_with_prefix(&self, _prefix: &str) -> crate::error::Result<DomTree> {
            unreachable!("extract_dom_with_prefix is not used in this test")
        }

        fn evaluate(
            &self,
            _script: &str,
            _await_promise: bool,
        ) -> crate::error::Result<ScriptEvaluation> {
            Ok(ScriptEvaluation {
                value: Some(Value::String("not-json".to_string())),
                description: None,
                type_name: Some("String".to_string()),
            })
        }

        fn capture_screenshot(&self, _full_page: bool) -> crate::error::Result<Vec<u8>> {
            unreachable!("capture_screenshot is not used in this test")
        }

        fn press_key(&self, _key: &str) -> crate::error::Result<()> {
            unreachable!("press_key is not used in this test")
        }

        fn list_tabs(&self) -> crate::error::Result<Vec<TabDescriptor>> {
            Ok(vec![TabDescriptor {
                id: "tab-1".to_string(),
                title: "Test Tab".to_string(),
                url: "about:blank".to_string(),
            }])
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
    fn test_scroll_params_with_amount() {
        let json = serde_json::json!({
            "amount": 500
        });

        let params: ScrollParams = serde_json::from_value(json).unwrap();
        assert_eq!(params.amount, Some(500));
    }

    #[test]
    fn test_scroll_params_negative_amount() {
        let json = serde_json::json!({
            "amount": -300
        });

        let params: ScrollParams = serde_json::from_value(json).unwrap();
        assert_eq!(params.amount, Some(-300));
    }

    #[test]
    fn test_scroll_params_no_amount() {
        let json = serde_json::json!({});

        let params: ScrollParams = serde_json::from_value(json).unwrap();
        assert_eq!(params.amount, None);
    }

    #[test]
    fn test_parse_scroll_output_from_string_payload() {
        let output = build_scroll_output(
            parse_raw_scroll_output(Some(serde_json::Value::String(
                r#"{"actual_scroll":420,"is_at_bottom":true,"scroll_y":860,"is_at_top":false}"#
                    .to_string(),
            )))
            .expect("scroll payload should parse"),
            empty_result(),
        );

        assert_eq!(output.scrolled, 420);
        assert!(output.is_at_bottom);
        assert!(output.message.contains("Reached the bottom"));
        assert_eq!(
            output.viewport_after,
            Some(ViewportAfter {
                scroll_y: 860,
                is_at_top: false,
                is_at_bottom: true,
            })
        );
    }

    #[test]
    fn test_parse_raw_scroll_output_rejects_invalid_payload() {
        let error =
            parse_raw_scroll_output(Some(serde_json::Value::String("not json".to_string())))
                .expect_err("invalid scroll payload should fail");

        assert!(error.0.contains("Failed to parse scroll result"));
        assert_eq!(error.1, "string");
    }

    #[test]
    fn test_scroll_output_serializes_normalized_metric_names_and_adds_viewport_after() {
        let output = ScrollOutput {
            result: empty_result(),
            scrolled: 120,
            is_at_bottom: false,
            message: "Scrolled 120 pixels. Did not reach the bottom of the page.".to_string(),
            viewport_after: Some(ViewportAfter {
                scroll_y: 240,
                is_at_top: false,
                is_at_bottom: false,
            }),
        };

        let value = serde_json::to_value(output).expect("scroll output should serialize");
        assert_eq!(value["scrolled"], serde_json::json!(120));
        assert_eq!(value["is_at_bottom"], serde_json::json!(false));
        assert_eq!(value["action"], serde_json::json!("scroll"));
        assert_eq!(value["viewport_after"]["scroll_y"], serde_json::json!(240));
        assert_eq!(
            value["viewport_after"]["is_at_top"],
            serde_json::json!(false)
        );
    }

    #[test]
    fn test_scroll_tool_returns_structured_failure_for_invalid_payload() {
        let session = BrowserSession::with_test_backend(InvalidScrollPayloadBackend);
        let tool = ScrollTool;
        let mut context = ToolContext::new(&session);

        let result = tool
            .execute_typed(ScrollParams { amount: None }, &mut context)
            .expect("invalid scroll payload should stay a tool failure");

        assert!(!result.success);
        assert!(
            result
                .error
                .as_deref()
                .unwrap_or_default()
                .contains("Failed to parse scroll result")
        );
        let data = result
            .data
            .expect("invalid scroll payload failure should include details");
        assert_eq!(data["code"].as_str(), Some("invalid_scroll_payload"));
        assert_eq!(data["details"]["received_type"].as_str(), Some("string"));
        assert_eq!(
            data["recovery"]["suggested_tool"].as_str(),
            Some("snapshot")
        );
        let metrics = result.metadata[OPERATION_METRICS_METADATA_KEY]
            .as_object()
            .expect("metrics metadata should be present on failures");
        assert_eq!(metrics["browser_evaluations"].as_u64(), Some(1));
    }

    fn empty_result() -> DocumentActionResult {
        DocumentActionResult::new("scroll", crate::dom::DocumentMetadata::default())
    }
}

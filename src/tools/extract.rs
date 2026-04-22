use crate::error::{BrowserError, Result};
use crate::tools::{Tool, ToolContext, ToolResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExtractParams {
    /// CSS selector (optional, defaults to body)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,

    /// Format: "text" or "html"
    #[serde(default = "default_format")]
    pub format: String,
}

fn default_format() -> String {
    "text".to_string()
}

#[derive(Default)]
pub struct ExtractContentTool;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExtractOutput {
    pub content: String,
    pub format: String,
    pub length: usize,
}

impl Tool for ExtractContentTool {
    type Params = ExtractParams;
    type Output = ExtractOutput;

    fn name(&self) -> &str {
        "extract"
    }

    fn description(&self) -> &str {
        "Read page text or HTML when markdown is too lossy for a selector or the whole page."
    }

    fn execute_typed(
        &self,
        params: ExtractParams,
        context: &mut ToolContext,
    ) -> Result<ToolResult> {
        let ExtractParams { selector, format } = params;
        let js_code = build_extract_js(selector.as_deref(), &format);
        context.record_browser_evaluation();
        let result = match context.session.evaluate(&js_code, false) {
            Ok(result) => result,
            Err(BrowserError::EvaluationFailed(reason)) => {
                if let Some(missing_selector) = missing_selector_from_reason(&reason) {
                    return Ok(
                        context.finish(extract_missing_target_failure(&missing_selector, &format))
                    );
                }

                return Err(BrowserError::EvaluationFailed(reason));
            }
            Err(other) => return Err(other),
        };
        let content = match parse_extract_output(result.value, selector.as_deref()) {
            Ok(content) => content,
            Err(ExtractFailure::MissingTarget(missing_selector)) => {
                return Ok(
                    context.finish(extract_missing_target_failure(&missing_selector, &format))
                );
            }
            Err(ExtractFailure::InvalidPayload {
                reason,
                received_type,
            }) => {
                return Ok(context.finish(ToolResult::failure_with(
                    reason.clone(),
                    serde_json::json!({
                        "code": "invalid_extract_payload",
                        "error": reason,
                        "format": format,
                        "selector": selector,
                        "received_type": received_type,
                        "recovery": {
                            "suggested_tool": "snapshot",
                        }
                    }),
                )));
            }
        };

        Ok(context.finish(ToolResult::success_with(ExtractOutput {
            length: content.len(),
            format,
            content,
        })))
    }
}

fn extract_missing_target_failure(selector: &str, format: &str) -> ToolResult {
    let error = format!("Element not found: {}", selector);

    ToolResult::failure_with(
        error.clone(),
        serde_json::json!({
            "code": "element_not_found",
            "error": error,
            "selector": selector,
            "format": format,
            "recovery": {
                "suggested_tool": "snapshot",
            }
        }),
    )
}

fn missing_selector_from_reason(reason: &str) -> Option<String> {
    let (_, selector) = reason.rsplit_once("Element not found: ")?;
    let selector = selector.lines().next().unwrap_or(selector).trim();
    if selector.is_empty() {
        None
    } else {
        Some(selector.to_string())
    }
}

enum ExtractFailure {
    MissingTarget(String),
    InvalidPayload {
        reason: String,
        received_type: &'static str,
    },
}

fn parse_extract_output(
    value: Option<Value>,
    selector: Option<&str>,
) -> std::result::Result<String, ExtractFailure> {
    match value {
        Some(Value::String(content)) => Ok(content),
        Some(other) => {
            let received_type = value_kind(&other);
            Err(ExtractFailure::InvalidPayload {
                reason: format!("Extract returned an unexpected {received_type} payload"),
                received_type,
            })
        }
        None => match selector {
            Some(selector) => Err(ExtractFailure::MissingTarget(selector.to_string())),
            None => Err(ExtractFailure::InvalidPayload {
                reason: "Extract returned no content".to_string(),
                received_type: "null",
            }),
        },
    }
}

fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn build_extract_js(selector: Option<&str>, format: &str) -> String {
    let selector_literal = selector
        .map(|value| serde_json::to_string(value).expect("selector JSON serialization should work"))
        .unwrap_or_else(|| "null".to_string());
    let value_expr = if format == "html" {
        "element ? element.innerHTML : ''"
    } else {
        "element ? (element.innerText || element.textContent || '') : ''"
    };

    format!(
        "(() => {{
            const selector = {selector_literal};
            const element = selector ? document.querySelector(selector) : document.body;
            if (selector && !element) {{
                throw new Error(`Element not found: ${{selector}}`);
            }}
            return {value_expr};
        }})()"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::BrowserSession;
    use crate::browser::backend::{
        FakeSessionBackend, ScriptEvaluation, SessionBackend, TabDescriptor,
    };
    use crate::error::BrowserError;
    use crate::{dom::DocumentMetadata, dom::DomTree};
    use std::time::Duration;

    enum EvaluateOnlyOutcome {
        Success(Value),
        NoValue,
        EvaluationFailed(&'static str),
    }

    struct EvaluateOnlyBackend {
        outcome: EvaluateOnlyOutcome,
    }

    impl SessionBackend for EvaluateOnlyBackend {
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
            match &self.outcome {
                EvaluateOnlyOutcome::Success(value) => Ok(ScriptEvaluation {
                    value: Some(value.clone()),
                    description: None,
                    type_name: Some(value_kind(value).to_string()),
                }),
                EvaluateOnlyOutcome::NoValue => Ok(ScriptEvaluation {
                    value: None,
                    description: None,
                    type_name: Some("undefined".to_string()),
                }),
                EvaluateOnlyOutcome::EvaluationFailed(reason) => {
                    Err(BrowserError::EvaluationFailed((*reason).to_string()))
                }
            }
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
                title: "about:blank".to_string(),
                url: "about:blank".to_string(),
            }])
        }

        fn active_tab(&self) -> crate::error::Result<TabDescriptor> {
            Ok(TabDescriptor {
                id: "tab-1".to_string(),
                title: "about:blank".to_string(),
                url: "about:blank".to_string(),
            })
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
    fn test_extract_tool_supports_selector_text_on_fake_backend() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let tool = ExtractContentTool::default();
        let mut context = ToolContext::new(&session);

        let result = tool
            .execute_typed(
                ExtractParams {
                    selector: Some("#fake-target".to_string()),
                    format: "text".to_string(),
                },
                &mut context,
            )
            .expect("extract should succeed");

        assert!(result.success);
        let data = result.data.expect("extract should include data");
        assert_eq!(data["content"].as_str(), Some("Fake target"));
        assert_eq!(data["format"].as_str(), Some("text"));
    }

    #[test]
    fn test_extract_tool_supports_selector_html_on_fake_backend() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let tool = ExtractContentTool::default();
        let mut context = ToolContext::new(&session);

        let result = tool
            .execute_typed(
                ExtractParams {
                    selector: Some("#fake-target".to_string()),
                    format: "html".to_string(),
                },
                &mut context,
            )
            .expect("extract should succeed");

        assert!(result.success);
        let data = result.data.expect("extract should include data");
        assert_eq!(
            data["content"].as_str(),
            Some(r#"<button id="fake-target" class="fake">Fake target</button>"#)
        );
        assert_eq!(data["format"].as_str(), Some("html"));
    }

    #[test]
    fn test_extract_tool_returns_structured_failure_for_missing_selector() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let tool = ExtractContentTool::default();
        let mut context = ToolContext::new(&session);

        let result = tool
            .execute_typed(
                ExtractParams {
                    selector: Some("#missing".to_string()),
                    format: "text".to_string(),
                },
                &mut context,
            )
            .expect("missing selector should stay a tool failure");

        assert!(!result.success);
        assert_eq!(result.error.as_deref(), Some("Element not found: #missing"));
        let data = result
            .data
            .expect("missing selector should include failure details");
        assert_eq!(data["code"].as_str(), Some("element_not_found"));
        assert_eq!(data["selector"].as_str(), Some("#missing"));
        assert_eq!(data["format"].as_str(), Some("text"));
        assert_eq!(
            data["recovery"]["suggested_tool"].as_str(),
            Some("snapshot")
        );
    }

    #[test]
    fn test_extract_tool_returns_missing_target_failure_when_selector_yields_no_payload() {
        let session = BrowserSession::with_test_backend(EvaluateOnlyBackend {
            outcome: EvaluateOnlyOutcome::NoValue,
        });
        let tool = ExtractContentTool::default();
        let mut context = ToolContext::new(&session);

        let result = tool
            .execute_typed(
                ExtractParams {
                    selector: Some("#missing".to_string()),
                    format: "html".to_string(),
                },
                &mut context,
            )
            .expect("missing selector should stay a tool failure");

        assert!(!result.success);
        assert_eq!(result.error.as_deref(), Some("Element not found: #missing"));
        let data = result
            .data
            .expect("missing selector should include failure details");
        assert_eq!(data["code"].as_str(), Some("element_not_found"));
        assert_eq!(data["selector"].as_str(), Some("#missing"));
        assert_eq!(data["format"].as_str(), Some("html"));
        assert_eq!(
            data["recovery"]["suggested_tool"].as_str(),
            Some("snapshot")
        );
    }

    #[test]
    fn test_extract_tool_preserves_non_missing_selector_evaluation_failures() {
        let session = BrowserSession::with_test_backend(EvaluateOnlyBackend {
            outcome: EvaluateOnlyOutcome::EvaluationFailed(
                "Failed to execute 'querySelector' on 'Document': '[' is not a valid selector.",
            ),
        });
        let tool = ExtractContentTool::default();
        let mut context = ToolContext::new(&session);

        let err = tool
            .execute_typed(
                ExtractParams {
                    selector: Some("[".to_string()),
                    format: "text".to_string(),
                },
                &mut context,
            )
            .expect_err("invalid selector should not be rewritten as element_not_found");

        match err {
            BrowserError::EvaluationFailed(reason) => {
                assert!(reason.contains("not a valid selector"));
            }
            other => panic!("unexpected extract error: {other:?}"),
        }
    }

    #[test]
    fn test_extract_tool_returns_structured_failure_for_invalid_payload_shape() {
        let session = BrowserSession::with_test_backend(EvaluateOnlyBackend {
            outcome: EvaluateOnlyOutcome::Success(serde_json::json!({
                "content": "not-a-string"
            })),
        });
        let tool = ExtractContentTool::default();
        let mut context = ToolContext::new(&session);

        let result = tool
            .execute_typed(
                ExtractParams {
                    selector: Some("#fake-target".to_string()),
                    format: "text".to_string(),
                },
                &mut context,
            )
            .expect("invalid extract payload should stay a tool failure");

        assert!(!result.success);
        assert_eq!(
            result.error.as_deref(),
            Some("Extract returned an unexpected object payload")
        );
        let data = result
            .data
            .expect("invalid extract payload should include details");
        assert_eq!(data["code"].as_str(), Some("invalid_extract_payload"));
        assert_eq!(data["selector"].as_str(), Some("#fake-target"));
        assert_eq!(data["format"].as_str(), Some("text"));
        assert_eq!(data["received_type"].as_str(), Some("object"));
        assert_eq!(
            data["recovery"]["suggested_tool"].as_str(),
            Some("snapshot")
        );
    }
}

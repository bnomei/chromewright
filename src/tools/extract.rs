use crate::error::{BrowserError, Result};
use crate::tools::core::structured_tool_failure;
use crate::tools::{DocumentResult, Tool, ToolContext, ToolResult};
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::de::Deserializer;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::borrow::Cow;

#[derive(Debug, Clone, Serialize)]
pub struct ExtractParams {
    /// CSS selector (optional, defaults to body)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,

    /// Format: "text" or "html"
    pub format: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ExtractFormat {
    Text,
    Html,
}

impl ExtractFormat {
    fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Html => "html",
        }
    }
}

fn default_extract_format() -> ExtractFormat {
    ExtractFormat::Text
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct StrictExtractParams {
    /// Omit `selector` to extract from the whole document body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    /// Output format to return.
    #[serde(default = "default_extract_format")]
    pub format: ExtractFormat,
}

impl From<StrictExtractParams> for ExtractParams {
    fn from(params: StrictExtractParams) -> Self {
        Self {
            selector: params.selector,
            format: params.format.as_str().to_string(),
        }
    }
}

impl<'de> Deserialize<'de> for ExtractParams {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        StrictExtractParams::deserialize(deserializer).map(Into::into)
    }
}

impl JsonSchema for ExtractParams {
    fn schema_name() -> Cow<'static, str> {
        "ExtractParams".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        StrictExtractParams::json_schema(generator)
    }
}

fn parse_extract_format(format: &str) -> Result<ExtractFormat> {
    match format {
        "text" => Ok(ExtractFormat::Text),
        "html" => Ok(ExtractFormat::Html),
        other => Err(BrowserError::InvalidArgument(format!(
            "extract.format must be one of: text, html (received '{other}')"
        ))),
    }
}

#[derive(Default)]
pub struct ExtractContentTool;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExtractOutput {
    #[serde(flatten)]
    pub result: DocumentResult,
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
        let format = parse_extract_format(&format)?;
        let format_label = format.as_str();
        let js_code = build_extract_js(selector.as_deref(), format_label);
        context.record_browser_evaluation();
        let result = match context.session.evaluate(&js_code, false) {
            Ok(result) => result,
            Err(BrowserError::EvaluationFailed(reason)) => {
                if let Some(missing_selector) = missing_selector_from_reason(&reason) {
                    return Ok(context.finish(extract_missing_target_failure(
                        &missing_selector,
                        format_label,
                    )));
                }

                return Err(BrowserError::EvaluationFailed(reason));
            }
            Err(other) => return Err(other),
        };
        let content = match parse_extract_output(result.value, selector.as_deref()) {
            Ok(content) => content,
            Err(ExtractFailure::MissingTarget(missing_selector)) => {
                return Ok(context.finish(extract_missing_target_failure(
                    &missing_selector,
                    format_label,
                )));
            }
            Err(ExtractFailure::InvalidPayload {
                reason,
                received_type,
            }) => {
                return Ok(context.finish(structured_tool_failure(
                    "invalid_extract_payload",
                    reason,
                    None,
                    None,
                    Some(serde_json::json!({
                        "suggested_tool": "snapshot",
                    })),
                    Some(serde_json::json!({
                        "format": format_label,
                        "selector": selector,
                        "received_type": received_type,
                    })),
                )));
            }
        };

        context.record_browser_evaluation();
        let document = context.session.document_metadata()?;

        Ok(context.finish(ToolResult::success_with(ExtractOutput {
            result: DocumentResult::new(document),
            length: content.len(),
            format: format_label.to_string(),
            content,
        })))
    }
}

fn extract_missing_target_failure(selector: &str, format: &str) -> ToolResult {
    let error = format!("Element not found: {}", selector);

    structured_tool_failure(
        "element_not_found",
        error,
        None,
        None,
        Some(serde_json::json!({
            "suggested_tool": "snapshot",
        })),
        Some(serde_json::json!({
            "selector": selector,
            "format": format,
        })),
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
    use schemars::schema_for;
    use serde_json::json;
    use std::time::Duration;

    enum EvaluateOnlyOutcome {
        Success(Value),
        NoValue,
        EvaluationFailed(&'static str),
    }

    struct EvaluateOnlyBackend {
        outcome: EvaluateOnlyOutcome,
    }

    #[test]
    fn test_extract_params_use_enum_schema_and_reject_unknown_format() {
        let params: ExtractParams = serde_json::from_value(json!({
            "selector": "#content",
            "format": "html"
        }))
        .expect("strict extract params should deserialize");
        assert_eq!(params.selector.as_deref(), Some("#content"));
        assert_eq!(params.format, "html");

        let error = serde_json::from_value::<ExtractParams>(json!({
            "selector": "#content",
            "format": "markdown"
        }))
        .expect_err("unknown extract format should be rejected");
        assert!(error.to_string().contains("unknown variant `markdown`"));

        let schema = schema_for!(ExtractParams);
        let schema_json = serde_json::to_value(&schema).expect("schema should serialize");
        let properties = schema_json
            .get("properties")
            .and_then(|value| value.as_object())
            .expect("extract params schema should expose properties");
        let format_property = properties
            .get("format")
            .expect("format property should be present");
        let format_json =
            serde_json::to_string(format_property).expect("format schema should serialize");
        assert!(format_json.contains("$ref") || format_json.contains("enum"));
        let full_schema_json =
            serde_json::to_string(&schema_json).expect("extract schema should serialize");
        assert!(full_schema_json.contains("\"text\""));
        assert!(full_schema_json.contains("\"html\""));
    }

    #[test]
    fn test_extract_tool_rejects_invalid_typed_format_instead_of_coercing() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let tool = ExtractContentTool;
        let mut context = ToolContext::new(&session);

        let error = tool
            .execute_typed(
                ExtractParams {
                    selector: Some("#fake-target".to_string()),
                    format: "markdown".to_string(),
                },
                &mut context,
            )
            .expect_err("invalid typed format should be rejected");

        assert!(matches!(error, BrowserError::InvalidArgument(_)));
        assert!(error.to_string().contains("extract.format"));
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
        let tool = ExtractContentTool;
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
        let tool = ExtractContentTool;
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
        let tool = ExtractContentTool;
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
        assert_eq!(data["details"]["selector"].as_str(), Some("#missing"));
        assert_eq!(data["details"]["format"].as_str(), Some("text"));
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
        let tool = ExtractContentTool;
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
        assert_eq!(data["details"]["selector"].as_str(), Some("#missing"));
        assert_eq!(data["details"]["format"].as_str(), Some("html"));
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
        let tool = ExtractContentTool;
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
        let tool = ExtractContentTool;
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
        assert_eq!(data["details"]["selector"].as_str(), Some("#fake-target"));
        assert_eq!(data["details"]["format"].as_str(), Some("text"));
        assert_eq!(data["details"]["received_type"].as_str(), Some("object"));
        assert_eq!(
            data["recovery"]["suggested_tool"].as_str(),
            Some("snapshot")
        );
    }
}

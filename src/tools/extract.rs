//! MCP tool that extracts raw text or HTML from the document body or a CSS selector.
//!
//! Enforces extract size limits and returns a document envelope for follow-up tools.

use crate::error::{BrowserError, Result};
use crate::tools::core::structured_tool_failure;
use crate::tools::limits::{MAX_EXTRACT_CHARS, validate_extract_chars};
use crate::tools::{DocumentResult, Tool, ToolContext, ToolResult};
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::de::Deserializer;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::borrow::Cow;

/// Optional selector scope and text/html format for raw content extraction.
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

/// Extracts text or HTML from the page or a scoped selector with size limits.
#[derive(Default)]
pub struct ExtractContentTool;

/// Document result carrying extracted content, format label, and character length.
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
            Err(ExtractFailure::ResourceLimit { char_count }) => {
                return Ok(context.finish(extract_resource_limit_failure(
                    format_label,
                    selector.as_deref(),
                    char_count,
                )));
            }
        };
        validate_extract_chars(content.chars().count(), format_label)?;

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

fn extract_resource_limit_failure(
    format: &str,
    selector: Option<&str>,
    char_count: usize,
) -> ToolResult {
    structured_tool_failure(
        "resource_limit_exceeded",
        format!(
            "extract {format} output is {char_count} characters, exceeding the {MAX_EXTRACT_CHARS} character limit"
        ),
        None,
        None,
        Some(serde_json::json!({
            "suggested_tool": "snapshot",
        })),
        Some(serde_json::json!({
            "resource": "extract_chars",
            "limit": format!("{MAX_EXTRACT_CHARS} characters"),
            "actual": format!("{char_count} characters"),
            "format": format,
            "selector": selector,
        })),
    )
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
    ResourceLimit {
        char_count: usize,
    },
}

fn parse_extract_output(
    value: Option<Value>,
    selector: Option<&str>,
) -> std::result::Result<String, ExtractFailure> {
    match value {
        Some(Value::String(payload)) => parse_extract_string_payload(payload),
        Some(other) => parse_extract_structured_payload(other),
        None => match selector {
            Some(selector) => Err(ExtractFailure::MissingTarget(selector.to_string())),
            None => Err(ExtractFailure::InvalidPayload {
                reason: "Extract returned no content".to_string(),
                received_type: "null",
            }),
        },
    }
}

fn parse_extract_string_payload(payload: String) -> std::result::Result<String, ExtractFailure> {
    if let Ok(value) = serde_json::from_str::<Value>(&payload)
        && value.get("success").is_some()
    {
        return parse_extract_structured_payload(value);
    }

    Ok(payload)
}

fn parse_extract_structured_payload(value: Value) -> std::result::Result<String, ExtractFailure> {
    let received_type = value_kind(&value);
    let Some(object) = value.as_object() else {
        return Err(ExtractFailure::InvalidPayload {
            reason: format!("Extract returned an unexpected {received_type} payload"),
            received_type,
        });
    };

    if object.get("success").and_then(Value::as_bool) == Some(false) {
        if object.get("code").and_then(Value::as_str) == Some("resource_limit_exceeded") {
            return Err(ExtractFailure::ResourceLimit {
                char_count: object
                    .get("char_count")
                    .and_then(Value::as_u64)
                    .unwrap_or((MAX_EXTRACT_CHARS + 1) as u64) as usize,
            });
        }

        return Err(ExtractFailure::InvalidPayload {
            reason: object
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("Extract returned an unsuccessful payload")
                .to_string(),
            received_type,
        });
    }

    if object.get("success").and_then(Value::as_bool) != Some(true) {
        return Err(ExtractFailure::InvalidPayload {
            reason: format!("Extract returned an unexpected {received_type} payload"),
            received_type,
        });
    }

    match object.get("content") {
        Some(Value::String(content)) => Ok(content.clone()),
        _ => Err(ExtractFailure::InvalidPayload {
            reason: "Extract returned a structured payload without string content".to_string(),
            received_type,
        }),
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
            const maxChars = {MAX_EXTRACT_CHARS};
            const element = selector ? document.querySelector(selector) : document.body;
            if (selector && !element) {{
                throw new Error(`Element not found: ${{selector}}`);
            }}
            const content = {value_expr};
            if (content.length > maxChars) {{
                return JSON.stringify({{
                    success: false,
                    code: 'resource_limit_exceeded',
                    error: `extract {format} output exceeds the ${{maxChars}} character limit`,
                    char_count: content.length
                }});
            }}
            return JSON.stringify({{
                success: true,
                content
            }});
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
    use crate::tools::limits::MAX_EXTRACT_CHARS;
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

    #[test]
    fn test_extract_tool_returns_structured_failure_for_resource_limit_payload() {
        let session = BrowserSession::with_test_backend(EvaluateOnlyBackend {
            outcome: EvaluateOnlyOutcome::Success(serde_json::Value::String(
                serde_json::json!({
                    "success": false,
                    "code": "resource_limit_exceeded",
                    "error": "extract text output exceeds the character limit",
                    "char_count": MAX_EXTRACT_CHARS + 1,
                })
                .to_string(),
            )),
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
            .expect("resource limit payload should stay a tool failure");

        assert!(!result.success);
        let data = result
            .data
            .expect("resource limit failure should include details");
        assert_eq!(data["code"].as_str(), Some("resource_limit_exceeded"));
        assert_eq!(data["details"]["resource"].as_str(), Some("extract_chars"));
        assert_eq!(data["details"]["format"].as_str(), Some("text"));
        assert_eq!(data["details"]["selector"].as_str(), Some("#fake-target"));
    }

    #[test]
    fn test_extract_tool_defensively_rejects_oversized_legacy_payload() {
        let session = BrowserSession::with_test_backend(EvaluateOnlyBackend {
            outcome: EvaluateOnlyOutcome::Success(serde_json::Value::String(
                "x".repeat(MAX_EXTRACT_CHARS + 1),
            )),
        });
        let tool = ExtractContentTool;
        let mut context = ToolContext::new(&session);

        let error = tool
            .execute_typed(
                ExtractParams {
                    selector: None,
                    format: "text".to_string(),
                },
                &mut context,
            )
            .expect_err("oversized legacy extract payload should fail closed");

        match error {
            BrowserError::ResourceLimitExceeded(details) => {
                assert_eq!(details.resource, "extract_chars");
                assert_eq!(
                    details.actual,
                    format!("{} characters", MAX_EXTRACT_CHARS + 1)
                );
            }
            other => panic!("unexpected extract resource limit error: {other:?}"),
        }
    }
}

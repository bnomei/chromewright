use crate::error::{BrowserError, Result};
use crate::tools::{
    DocumentEnvelope, DocumentEnvelopeOptions, Tool, ToolContext, ToolResult,
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
    pub envelope: DocumentEnvelope,
    pub scrolled: i64,
    #[serde(rename = "isAtBottom")]
    pub is_at_bottom: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub viewport_after: Option<ViewportAfter>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct RawScrollOutput {
    #[serde(default)]
    actual_scroll: i64,
    #[serde(default)]
    is_at_bottom: bool,
    scroll_y: Option<i64>,
    is_at_top: Option<bool>,
}

impl Tool for ScrollTool {
    type Params = ScrollParams;
    type Output = ScrollOutput;

    fn name(&self) -> &str {
        "scroll"
    }

    fn execute_typed(&self, params: ScrollParams, context: &mut ToolContext) -> Result<ToolResult> {
        let config = serde_json::json!({
            "amount": params.amount
        });
        let scroll_js = SCROLL_JS.replace("__SCROLL_CONFIG__", &config.to_string());

        let result = context
            .session
            .tab()?
            .evaluate(&scroll_js, true)
            .map_err(|e| BrowserError::ToolExecutionFailed {
                tool: "scroll".to_string(),
                reason: e.to_string(),
            })?;

        context.invalidate_dom();
        let envelope = build_document_envelope(context, None, DocumentEnvelopeOptions::minimal())?;

        Ok(ToolResult::success_with(parse_scroll_output(
            result.value,
            envelope,
        )))
    }
}

fn parse_scroll_output(
    value: Option<serde_json::Value>,
    envelope: DocumentEnvelope,
) -> ScrollOutput {
    let result_json = parse_raw_scroll_output(value);
    let viewport_after = match (result_json.scroll_y, result_json.is_at_top) {
        (Some(scroll_y), Some(is_at_top)) => Some(ViewportAfter {
            scroll_y,
            is_at_top,
            is_at_bottom: result_json.is_at_bottom,
        }),
        _ => None,
    };

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
        envelope,
        scrolled: result_json.actual_scroll,
        is_at_bottom: result_json.is_at_bottom,
        message,
        viewport_after,
    }
}

fn parse_raw_scroll_output(value: Option<serde_json::Value>) -> RawScrollOutput {
    match value {
        Some(serde_json::Value::String(json_str)) => {
            serde_json::from_str(&json_str).unwrap_or_default()
        }
        Some(other) => serde_json::from_value(other).unwrap_or_default(),
        None => RawScrollOutput::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let output = parse_scroll_output(
            Some(serde_json::Value::String(
                r#"{"actualScroll":420,"isAtBottom":true,"scrollY":860,"isAtTop":false}"#
                    .to_string(),
            )),
            empty_envelope(),
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
    fn test_parse_scroll_output_falls_back_for_invalid_payload() {
        let output = parse_scroll_output(
            Some(serde_json::Value::String("not json".to_string())),
            empty_envelope(),
        );

        assert_eq!(output.scrolled, 0);
        assert!(!output.is_at_bottom);
        assert!(output.message.contains("Did not reach the bottom"));
        assert_eq!(output.viewport_after, None);
    }

    #[test]
    fn test_scroll_output_preserves_existing_metric_names_and_adds_viewport_after() {
        let output = ScrollOutput {
            envelope: empty_envelope(),
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
        assert_eq!(value["isAtBottom"], serde_json::json!(false));
        assert_eq!(value["viewport_after"]["scroll_y"], serde_json::json!(240));
        assert_eq!(
            value["viewport_after"]["is_at_top"],
            serde_json::json!(false)
        );
    }

    fn empty_envelope() -> DocumentEnvelope {
        DocumentEnvelope {
            document: crate::dom::DocumentMetadata::default(),
            target: None,
            snapshot: None,
            nodes: Vec::new(),
            interactive_count: None,
        }
    }
}

//! MCP (Model Context Protocol) server implementation for browser automation
//!
//! This module provides rmcp-compatible tools by wrapping the existing tool implementations.

pub mod handler;
pub use handler::BrowserServer;

use crate::tools::ToolResult as InternalToolResult;
#[cfg(test)]
use crate::tools::{ToolContext, normalize_tool_outcome};
use rmcp::{
    ErrorData as McpError,
    model::{CallToolResult, Content, Meta},
};

fn with_metadata(
    result: CallToolResult,
    metadata: std::collections::HashMap<String, serde_json::Value>,
) -> CallToolResult {
    if metadata.is_empty() {
        return result;
    }

    result.with_meta(Some(Meta(metadata.into_iter().collect())))
}

/// Convert internal ToolResult to MCP CallToolResult
pub(crate) fn convert_result(result: InternalToolResult) -> Result<CallToolResult, McpError> {
    let InternalToolResult {
        success,
        data,
        error,
        metadata,
    } = result;

    if success {
        let result = if let Some(data) = data {
            CallToolResult::structured(data)
        } else {
            CallToolResult::success(vec![Content::text("Success")])
        };

        Ok(with_metadata(result, metadata))
    } else {
        let error_msg = error.unwrap_or_else(|| "Unknown error".to_string());
        let structured_error = match data {
            Some(serde_json::Value::Object(mut object)) => {
                object
                    .entry("error".to_string())
                    .or_insert_with(|| serde_json::Value::String(error_msg.clone()));
                serde_json::Value::Object(object)
            }
            Some(other) => serde_json::json!({
                "error": error_msg,
                "details": other,
            }),
            None => serde_json::json!({
                "error": error_msg,
            }),
        };
        let result = CallToolResult::structured_error(structured_error);

        Ok(with_metadata(result, metadata))
    }
}

#[cfg(test)]
fn convert_tool_outcome(
    outcome: crate::error::Result<InternalToolResult>,
    context: &ToolContext<'_>,
) -> Result<CallToolResult, McpError> {
    let result = normalize_tool_outcome(outcome, context).map_err(mcp_internal_error)?;
    convert_result(result)
}

pub(crate) fn mcp_internal_error(error: impl std::fmt::Display) -> McpError {
    McpError::internal_error(error.to_string(), None)
}

#[cfg(test)]
mod tests {
    use super::{convert_result, convert_tool_outcome};
    use crate::browser::BrowserSession;
    use crate::browser::backend::FakeSessionBackend;
    use crate::error::BrowserError;
    use crate::mcp::BrowserServer;
    use crate::tools::{
        OPERATION_METRICS_METADATA_KEY, ToolContext, ToolResult as InternalToolResult,
    };
    use serde_json::json;
    use std::collections::HashMap;

    const MAX_TOOL_DESCRIPTION_LEN: usize = 96;

    #[test]
    fn test_convert_result_preserves_structured_success() {
        let result = convert_result(
            InternalToolResult::success(Some(json!({
                "url": "https://example.com",
                "title": "Example Domain",
            })))
            .with_metadata("duration_ms", json!(12)),
        )
        .expect("success result should convert");

        assert_eq!(
            result.structured_content,
            Some(json!({
                "url": "https://example.com",
                "title": "Example Domain",
            }))
        );
        assert_eq!(result.is_error, Some(false));
        assert_eq!(
            result.meta.as_ref().unwrap().0.get("duration_ms"),
            Some(&json!(12))
        );

        let text = result
            .content
            .first()
            .and_then(|content| content.as_text())
            .map(|content| content.text.as_str())
            .expect("structured result should keep text fallback");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(text).expect("text fallback should be JSON"),
            json!({
                "url": "https://example.com",
                "title": "Example Domain",
            })
        );
    }

    #[test]
    fn test_convert_result_uses_structured_tool_error() {
        let result = convert_result(
            InternalToolResult::failure("Element not found").with_metadata("tool", json!("click")),
        )
        .expect("tool failures should stay in CallToolResult");

        assert_eq!(
            result.structured_content,
            Some(json!({
                "error": "Element not found",
            }))
        );
        assert_eq!(result.is_error, Some(true));
        assert_eq!(
            result.meta.as_ref().unwrap().0.get("tool"),
            Some(&json!("click"))
        );
    }

    #[test]
    fn test_convert_result_without_data_keeps_text_success() {
        let result = convert_result(InternalToolResult::success(None))
            .expect("empty success should convert");

        assert_eq!(result.structured_content, None);
        assert_eq!(result.is_error, Some(false));

        let text = result
            .content
            .first()
            .and_then(|content| content.as_text())
            .map(|content| content.text.as_ref());
        assert_eq!(text, Some("Success"));
    }

    #[test]
    fn test_convert_tool_outcome_preserves_structured_tool_failures_from_browser_errors() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let mut context = ToolContext::new(&session);
        context.record_browser_evaluation();

        let result = convert_tool_outcome(
            Err(BrowserError::ToolExecutionFailed {
                tool: "close".to_string(),
                reason: "Session close encountered 1 error(s)".to_string(),
            }),
            &context,
        )
        .expect("tool execution failures should stay structured");

        assert_eq!(
            result.structured_content,
            Some(json!({
                "code": "tool_execution_failed",
                "error": "Session close encountered 1 error(s)",
                "tool": "close",
            }))
        );
        assert_eq!(result.is_error, Some(true));
        assert_eq!(
            result
                .meta
                .as_ref()
                .and_then(|meta| meta.0.get(OPERATION_METRICS_METADATA_KEY))
                .and_then(|metrics| metrics.get("browser_evaluations"))
                .and_then(|value| value.as_u64()),
            Some(1)
        );
    }

    #[test]
    fn test_convert_tool_outcome_keeps_internal_errors_for_browser_infra_failures() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let context = ToolContext::new(&session);

        let error = convert_tool_outcome(
            Err(BrowserError::ChromeError(
                "CDP connection dropped".to_string(),
            )),
            &context,
        )
        .expect_err("browser infrastructure failures should remain internal");

        assert!(error.to_string().contains("CDP connection dropped"));
    }

    #[test]
    fn test_mcp_tools_advertise_output_schemas() {
        let tools = BrowserServer::from_session(BrowserSession::with_test_backend(
            FakeSessionBackend::new(),
        ))
        .list_mcp_tools();

        assert!(!tools.is_empty(), "expected MCP tools to be registered");

        let mut missing_output_schema = Vec::new();
        let mut non_object_output_schema = Vec::new();

        for tool in tools {
            match tool.output_schema.as_ref() {
                None => missing_output_schema.push(tool.name.to_string()),
                Some(schema) => {
                    if schema.get("type").and_then(|value| value.as_str()) != Some("object") {
                        non_object_output_schema.push(tool.name.to_string());
                    }
                }
            }
        }

        assert!(
            missing_output_schema.is_empty(),
            "MCP tools missing output_schema: {missing_output_schema:?}"
        );
        assert!(
            non_object_output_schema.is_empty(),
            "MCP tools with non-object output_schema: {non_object_output_schema:?}"
        );
    }

    #[test]
    fn test_mcp_tool_descriptions_are_concise() {
        let tools = BrowserServer::from_session(BrowserSession::with_test_backend(
            FakeSessionBackend::new(),
        ))
        .list_mcp_tools();

        let problems: Vec<String> = tools
            .iter()
            .filter_map(|tool| {
                let description = tool.description.as_deref().unwrap_or("").trim();
                let char_count = description.chars().count();

                if description.is_empty() {
                    Some(format!("{} is missing a description", tool.name))
                } else if char_count > MAX_TOOL_DESCRIPTION_LEN {
                    Some(format!(
                        "{} description is {} chars: {}",
                        tool.name, char_count, description
                    ))
                } else {
                    None
                }
            })
            .collect();

        assert!(
            problems.is_empty(),
            "MCP tool descriptions must stay concise: {problems:?}"
        );
    }

    #[test]
    fn test_mcp_tool_descriptions_keep_orchestration_hints() {
        let descriptions: HashMap<String, String> = BrowserServer::from_session(
            BrowserSession::with_test_backend(FakeSessionBackend::new()),
        )
        .list_mcp_tools()
        .into_iter()
        .map(|tool| {
            (
                tool.name.to_string(),
                tool.description.as_deref().unwrap_or("").to_string(),
            )
        })
        .collect();

        let expectations = [
            ("get_markdown", ["snapshot"].as_slice()),
            ("extract", ["markdown"].as_slice()),
            ("read_links", ["click", "navigate"].as_slice()),
            (
                "snapshot",
                ["cursors", "inspect_node", "click", "wait"].as_slice(),
            ),
            ("inspect_node", ["cursor", "snapshot"].as_slice()),
            ("click", ["snapshot", "wait"].as_slice()),
            ("input", ["snapshot", "press_key"].as_slice()),
            ("select", ["snapshot", "wait"].as_slice()),
            ("new_tab", ["tab_list", "switch_tab"].as_slice()),
            ("tab_list", ["switch_tab"].as_slice()),
            ("switch_tab", ["tab_list", "snapshot"].as_slice()),
        ];

        for (tool_name, keywords) in expectations {
            let description = descriptions
                .get(tool_name)
                .unwrap_or_else(|| panic!("missing description for {tool_name}"));

            for keyword in keywords {
                assert!(
                    description.contains(keyword),
                    "{tool_name} description should mention '{keyword}', got: {description}"
                );
            }
        }
    }

    #[test]
    fn test_mcp_surface_exports_default_extraction_tools() {
        let tool_names: Vec<String> = BrowserServer::from_session(
            BrowserSession::with_test_backend(FakeSessionBackend::new()),
        )
        .list_mcp_tools()
        .into_iter()
        .map(|tool| tool.name.to_string())
        .collect();

        assert!(
            tool_names.iter().any(|name| name == "extract"),
            "extract should be exported via MCP"
        );
        assert!(
            tool_names.iter().any(|name| name == "read_links"),
            "read_links should be exported via MCP"
        );
    }

    #[test]
    fn test_mcp_surface_reflects_operator_tool_registration() {
        let mut session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        session.tool_registry_mut().register_operator_tools();

        let tool_names: Vec<String> = BrowserServer::from_session(session)
            .list_mcp_tools()
            .into_iter()
            .map(|tool| tool.name.to_string())
            .collect();

        assert!(tool_names.iter().any(|name| name == "evaluate"));
        assert!(tool_names.iter().any(|name| name == "screenshot"));
    }
}

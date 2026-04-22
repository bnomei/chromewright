//! MCP (Model Context Protocol) server implementation for browser automation
//!
//! This module provides rmcp-compatible tools by wrapping the existing tool implementations.

pub mod handler;
pub use handler::BrowserServer;

use crate::tools::{
    self, Tool, ToolContext, ToolResult as InternalToolResult, normalize_tool_outcome,
};
use rmcp::{
    ErrorData as McpError,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, Content, Meta},
    tool, tool_router,
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
fn convert_result(result: InternalToolResult) -> Result<CallToolResult, McpError> {
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

fn convert_tool_outcome(
    outcome: crate::error::Result<InternalToolResult>,
    context: &ToolContext<'_>,
) -> Result<CallToolResult, McpError> {
    let result = normalize_tool_outcome(outcome, context)
        .map_err(|error| McpError::internal_error(error.to_string(), None))?;
    convert_result(result)
}

/// Macro to register MCP tools by automatically generating wrapper functions
macro_rules! register_mcp_tools {
    ($($mcp_name:ident => $tool_type:ty, $description:expr);* $(;)?) => {
        #[tool_router]
        impl BrowserServer {
            $(
                #[tool(
                    description = $description,
                    output_schema = rmcp::handler::server::tool::schema_for_type::<<$tool_type as Tool>::Output>()
                )]
                fn $mcp_name(
                    &self,
                    params: Parameters<<$tool_type as Tool>::Params>,
                ) -> Result<CallToolResult, McpError> {
                    let mut context = ToolContext::new(self.session());
                    let tool = <$tool_type>::default();
                    convert_tool_outcome(tool.execute_typed(params.0, &mut context), &context)
                }
            )*
        }
    };
}

// Register all MCP tools using the macro
register_mcp_tools! {
    // ---- Navigation and Browser Flow ----
    browser_navigate => tools::navigate::NavigateTool, "Open a URL. Next: wait or snapshot.";
    browser_go_back => tools::go_back::GoBackTool, "Go back in history. Next: wait or snapshot.";
    browser_go_forward => tools::go_forward::GoForwardTool, "Go forward in history. Next: wait or snapshot.";
    browser_close => tools::close::CloseTool, "Close all open tabs in the current session.";

    // ---- Page Content and Extraction ----
    browser_get_markdown => tools::markdown::GetMarkdownTool, "Read page content as markdown. Extraction only; use snapshot for actions.";
    browser_get_text => tools::extract::ExtractContentTool, "Read text or HTML from the page or a selector. Use when markdown is too lossy.";
    browser_read_links => tools::read_links::ReadLinksTool, "List page links. Next: click or navigate.";
    browser_snapshot => tools::snapshot::SnapshotTool, "Capture page state and node cursors. Next: inspect_node, click, input, select, hover, wait.";
    browser_inspect_node => tools::inspect_node::InspectNodeTool, "Inspect one node after snapshot. Prefer cursor; selector/index/node_ref still work.";

    // ---- Interaction ----
    browser_click => tools::click::ClickTool, "Activate an element. Usually after snapshot; next wait or snapshot.";
    browser_hover => tools::hover::HoverTool, "Reveal hover state. Usually after snapshot; next snapshot or click.";
    browser_select => tools::select::SelectTool, "Choose a dropdown value. Usually after snapshot; next wait or snapshot.";
    browser_input_fill => tools::input::InputTool, "Type into an input. Usually after snapshot; next press_key, click, or wait.";
    browser_press_key => tools::press_key::PressKeyTool, "Press a keyboard key. Returns focus hints; snapshot only for broader rereads.";
    browser_scroll => tools::scroll::ScrollTool, "Scroll the page. Returns viewport hints; snapshot only for broader rereads.";
    browser_wait => tools::wait::WaitTool, "Pause for load, revision change, or node state. Use after actions or before rereading.";

    // ---- Tab Management ----
    browser_new_tab => tools::new_tab::NewTabTool, "Open a URL in a new tab. Next: tab_list, switch_tab, or snapshot.";
    browser_tab_list => tools::tab_list::TabListTool, "List tabs so you can choose a switch_tab target.";
    browser_switch_tab => tools::switch_tab::SwitchTabTool, "Activate a tab by index. Usually after tab_list; next snapshot.";
    browser_close_tab => tools::close_tab::CloseTabTool, "Close the active tab. Next: tab_list or switch_tab if work continues.";
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
            InternalToolResult::failure("Element not found")
                .with_metadata("tool", json!("browser_click")),
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
            Some(&json!("browser_click"))
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
        let tools = BrowserServer::tool_router().list_all();

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
        let tools = BrowserServer::tool_router().list_all();

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
        let descriptions: HashMap<String, String> = BrowserServer::tool_router()
            .list_all()
            .into_iter()
            .map(|tool| {
                (
                    tool.name.to_string(),
                    tool.description.as_deref().unwrap_or("").to_string(),
                )
            })
            .collect();

        let expectations = [
            ("browser_get_markdown", ["snapshot"].as_slice()),
            ("browser_get_text", ["markdown"].as_slice()),
            ("browser_read_links", ["click", "navigate"].as_slice()),
            (
                "browser_snapshot",
                ["cursors", "inspect_node", "click", "wait"].as_slice(),
            ),
            ("browser_inspect_node", ["cursor", "snapshot"].as_slice()),
            ("browser_click", ["snapshot", "wait"].as_slice()),
            ("browser_input_fill", ["snapshot", "press_key"].as_slice()),
            ("browser_select", ["snapshot", "wait"].as_slice()),
            ("browser_new_tab", ["tab_list", "switch_tab"].as_slice()),
            ("browser_tab_list", ["switch_tab"].as_slice()),
            ("browser_switch_tab", ["tab_list", "snapshot"].as_slice()),
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
        let tool_names: Vec<String> = BrowserServer::tool_router()
            .list_all()
            .into_iter()
            .map(|tool| tool.name.to_string())
            .collect();

        assert!(
            tool_names.iter().any(|name| name == "browser_get_text"),
            "browser_get_text should be exported via MCP"
        );
        assert!(
            tool_names.iter().any(|name| name == "browser_read_links"),
            "browser_read_links should be exported via MCP"
        );
    }
}

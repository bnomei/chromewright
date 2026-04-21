//! MCP (Model Context Protocol) server implementation for browser automation
//!
//! This module provides rmcp-compatible tools by wrapping the existing tool implementations.

pub mod handler;
pub use handler::BrowserServer;

use crate::tools::{self, Tool, ToolContext, ToolResult as InternalToolResult};
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

/// Macro to register MCP tools by automatically generating wrapper functions
macro_rules! register_mcp_tools {
    ($($mcp_name:ident => $tool_type:ty, $description:expr);* $(;)?) => {
        #[tool_router]
        impl BrowserServer {
            $(
                #[tool(description = $description)]
                fn $mcp_name(
                    &self,
                    params: Parameters<<$tool_type as Tool>::Params>,
                ) -> Result<CallToolResult, McpError> {
                    let mut context = ToolContext::new(self.session());
                    let tool = <$tool_type>::default();
                    let result = tool.execute_typed(params.0, &mut context)
                        .map_err(|e| McpError::internal_error(e.to_string(), None))?;
                    convert_result(result)
                }
            )*
        }
    };
}

// Register all MCP tools using the macro
register_mcp_tools! {
    // ---- Navigation and Browser Flow ----
    browser_navigate => tools::navigate::NavigateTool, "Navigate to a specified URL in the browser";
    browser_go_back => tools::go_back::GoBackTool, "Navigate back in browser history";
    browser_go_forward => tools::go_forward::GoForwardTool, "Navigate forward in browser history";
    browser_close => tools::close::CloseTool, "Close the browser when the task is complete";

    // ---- Page Content and Extraction ----
    browser_get_markdown => tools::markdown::GetMarkdownTool, "Get the markdown content of the current page (use this tool only for information extraction; for interaction use the snapshot tool instead)";
    browser_snapshot => tools::snapshot::SnapshotTool, "Get a snapshot of the current page with revision-scoped node refs and actionable elements for interaction";
    // browser_get_text => tools::extract::ExtractContentTool, "Extract text or HTML content from the page or an element";

    // ---- Interaction ----
    browser_click => tools::click::ClickTool, "Click on an element specified by CSS selector, index, or snapshot node_ref";
    browser_hover => tools::hover::HoverTool, "Hover over an element specified by CSS selector, index, or snapshot node_ref";
    browser_select => tools::select::SelectTool, "Select an option in a dropdown element by CSS selector, index, or snapshot node_ref";
    browser_input_fill => tools::input::InputTool, "Type text into an input element specified by CSS selector, index, or snapshot node_ref";
    browser_press_key => tools::press_key::PressKeyTool, "Press a key on the keyboard";
    browser_scroll => tools::scroll::ScrollTool, "Scroll the page by a specified amount or to the bottom";
    browser_wait => tools::wait::WaitTool, "Wait for navigation settle, revision changes, or node state predicates on the page";

    // ---- Tab Management ----
    browser_new_tab => tools::new_tab::NewTabTool, "Open a new tab and navigate to the specified URL";
    browser_tab_list => tools::tab_list::TabListTool, "Get the list of all browser tabs with their titles and URLs";
    browser_switch_tab => tools::switch_tab::SwitchTabTool, "Switch to a specific tab by index";
    browser_close_tab => tools::close_tab::CloseTabTool, "Close the current active tab";
}

#[cfg(test)]
mod tests {
    use super::convert_result;
    use crate::tools::ToolResult as InternalToolResult;
    use serde_json::json;

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
}

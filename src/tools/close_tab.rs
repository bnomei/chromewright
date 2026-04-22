use crate::error::{BrowserError, Result};
use crate::tools::{Tool, ToolContext, ToolResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Parameters for the close_tab tool (no parameters needed)
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CloseTabParams {}

/// Tool for closing the current active tab
#[derive(Default)]
pub struct CloseTabTool;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CloseTabOutput {
    pub index: usize,
    pub title: String,
    pub url: String,
    pub message: String,
}

impl Tool for CloseTabTool {
    type Params = CloseTabParams;
    type Output = CloseTabOutput;

    fn name(&self) -> &str {
        "close_tab"
    }

    fn execute_typed(
        &self,
        _params: CloseTabParams,
        context: &mut ToolContext,
    ) -> Result<ToolResult> {
        let tab_count = context.session.tab_overview()?.len();
        if tab_count == 0 {
            return Ok(context.finish(close_tab_failure(
                "no_tabs",
                "No tabs available".to_string(),
                "new_tab",
                tab_count,
            )));
        }

        let closed = match context.session.close_active_tab_summary() {
            Ok(closed) => closed,
            Err(BrowserError::TabOperationFailed(reason))
                if reason.contains("No active tab found") =>
            {
                return Ok(context.finish(close_tab_failure(
                    "no_active_tab",
                    "No active tab found".to_string(),
                    "tab_list",
                    tab_count,
                )));
            }
            Err(other) => return Err(other),
        };
        let message = close_tab_message(closed.index, &closed.title, &closed.url);

        Ok(context.finish(ToolResult::success_with(CloseTabOutput {
            index: closed.index,
            message,
            title: closed.title,
            url: closed.url,
        })))
    }
}

fn close_tab_message(index: usize, title: &str, url: &str) -> String {
    format!("Closed tab [{}]: {} ({})", index, title, url)
}

fn close_tab_failure(
    code: &str,
    error: String,
    suggested_tool: &str,
    tab_count: usize,
) -> ToolResult {
    ToolResult::failure_with(
        error.clone(),
        json!({
            "code": code,
            "error": error,
            "tab_count": tab_count,
            "recovery": {
                "suggested_tool": suggested_tool,
            }
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::BrowserSession;
    use crate::browser::backend::FakeSessionBackend;

    #[test]
    fn test_close_tab_message_includes_index_title_and_url() {
        assert_eq!(
            close_tab_message(3, "Docs", "https://example.com"),
            "Closed tab [3]: Docs (https://example.com)"
        );
    }

    #[test]
    fn test_close_tab_tool_executes_against_fake_backend() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        session
            .open_tab_entry("https://second.example")
            .expect("second tab should open");

        let tool = CloseTabTool::default();
        let mut context = ToolContext::new(&session);
        let result = tool
            .execute_typed(CloseTabParams {}, &mut context)
            .expect("close_tab should succeed");

        assert!(result.success);
        let data = result.data.expect("close_tab should include data");
        assert_eq!(data["index"].as_u64(), Some(1));
        assert_eq!(data["url"].as_str(), Some("https://second.example"));
        assert_eq!(
            session
                .tab_overview()
                .expect("tabs should load")
                .iter()
                .filter(|tab| tab.active)
                .count(),
            1
        );
    }

    #[test]
    fn test_close_tab_tool_returns_structured_failure_when_no_tabs_are_available() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        session.close().expect("session close should succeed");

        let tool = CloseTabTool::default();
        let mut context = ToolContext::new(&session);
        let result = tool
            .execute_typed(CloseTabParams {}, &mut context)
            .expect("close_tab should return a structured failure");

        assert!(!result.success);
        assert_eq!(result.error.as_deref(), Some("No tabs available"));
        let data = result
            .data
            .expect("close_tab failure should include details");
        assert_eq!(data["code"].as_str(), Some("no_tabs"));
        assert_eq!(data["recovery"]["suggested_tool"].as_str(), Some("new_tab"));
    }

    #[test]
    fn test_close_tab_tool_returns_structured_failure_when_active_tab_is_unknown() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::with_no_active_tab());

        let tool = CloseTabTool::default();
        let mut context = ToolContext::new(&session);
        let result = tool
            .execute_typed(CloseTabParams {}, &mut context)
            .expect("close_tab should return a structured failure");

        assert!(!result.success);
        assert_eq!(result.error.as_deref(), Some("No active tab found"));
        let data = result
            .data
            .expect("close_tab failure should include details");
        assert_eq!(data["code"].as_str(), Some("no_active_tab"));
        assert_eq!(
            data["recovery"]["suggested_tool"].as_str(),
            Some("tab_list")
        );
    }
}

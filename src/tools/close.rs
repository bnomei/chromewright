use crate::error::{BrowserError, Result};
use crate::tools::{Tool, ToolContext, ToolResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parameters for the close tool (no parameters needed)
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CloseParams {}

/// Tool for closing the browser
#[derive(Default)]
pub struct CloseTool;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CloseOutput {
    pub closed_tabs: usize,
    pub message: String,
}

impl Tool for CloseTool {
    type Params = CloseParams;
    type Output = CloseOutput;

    fn name(&self) -> &str {
        "close"
    }

    fn execute_typed(&self, _params: CloseParams, context: &mut ToolContext) -> Result<ToolResult> {
        let closed_tabs = context.session.tab_overview()?.len();

        context
            .session
            .close()
            .map_err(|e| BrowserError::ToolExecutionFailed {
                tool: "close".to_string(),
                reason: e.to_string(),
            })?;

        Ok(context.finish(ToolResult::success_with(CloseOutput {
            closed_tabs,
            message: format!("Closed {} tab(s) in the current session", closed_tabs),
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::BrowserSession;
    use crate::browser::backend::FakeSessionBackend;

    #[test]
    fn test_close_tool_closes_fake_backend_tabs() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        session
            .open_tab_entry("https://second.example")
            .expect("second tab should open");

        let tool = CloseTool::default();
        let mut context = ToolContext::new(&session);
        let result = tool
            .execute_typed(CloseParams {}, &mut context)
            .expect("close should succeed");

        assert!(result.success);
        let data = result.data.expect("close should include data");
        assert_eq!(
            data["message"].as_str(),
            Some("Closed 2 tab(s) in the current session")
        );
        assert_eq!(data["closed_tabs"].as_u64(), Some(2));
        assert!(session.tab_overview().expect("tabs should load").is_empty());
    }

    #[test]
    fn test_close_tool_surfaces_partial_close_failures() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::with_close_failures([
            "https://stuck.example",
        ]));
        session
            .open_tab_entry("https://ok.example")
            .expect("ok tab should open");
        session
            .open_tab_entry("https://stuck.example")
            .expect("stuck tab should open");

        let tool = CloseTool::default();
        let mut context = ToolContext::new(&session);
        let err = tool
            .execute_typed(CloseParams {}, &mut context)
            .expect_err("close should fail when one tab cannot close");

        match err {
            BrowserError::ToolExecutionFailed { tool, reason } => {
                assert_eq!(tool, "close");
                assert!(reason.contains("after attempting 3 tab(s)"));
                assert!(reason.contains("stuck.example"));
            }
            other => panic!("unexpected close error: {other:?}"),
        }

        let remaining_tabs = session.tab_overview().expect("tabs should still load");
        assert_eq!(remaining_tabs.len(), 1);
        assert_eq!(remaining_tabs[0].url, "https://stuck.example");
    }
}

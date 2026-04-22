use crate::error::{BrowserError, Result};
use crate::tools::{Tool, ToolContext, ToolResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parameters for the close tool (no parameters needed)
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CloseParams {
    /// Allow closing unmanaged tabs in connected sessions
    #[serde(default)]
    pub confirm_destructive: bool,
}

/// Tool for closing the browser
#[derive(Default)]
pub struct CloseTool;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CloseOutput {
    pub closed_tabs: usize,
    pub skipped_tabs: usize,
    pub scope: String,
    pub session_origin: String,
    pub message: String,
}

impl Tool for CloseTool {
    type Params = CloseParams;
    type Output = CloseOutput;

    fn name(&self) -> &str {
        "close"
    }

    fn description(&self) -> &str {
        "Close session tabs; connected sessions keep unmanaged tabs unless confirm_destructive=true."
    }

    fn execute_typed(&self, params: CloseParams, context: &mut ToolContext) -> Result<ToolResult> {
        let total_tabs = context.session.tab_overview()?.len();
        let session_origin = context.session.session_origin_label().to_string();
        let (closed_tabs, skipped_tabs, scope, message) =
            if context.session.is_connected_session() && !params.confirm_destructive {
                let summary = context
                    .session
                    .close_managed_tabs()
                    .map_err(|e| BrowserError::ToolExecutionFailed {
                        tool: "close".to_string(),
                        reason: e.to_string(),
                    })?;
                (
                    summary.closed_tabs,
                    summary.skipped_tabs,
                    "managed_only".to_string(),
                    format!(
                        "Closed {} session-managed tab(s); left {} unmanaged tab(s) open",
                        summary.closed_tabs, summary.skipped_tabs
                    ),
                )
            } else {
                context
                    .session
                    .close()
                    .map_err(|e| BrowserError::ToolExecutionFailed {
                        tool: "close".to_string(),
                        reason: e.to_string(),
                    })?;
                (
                    total_tabs,
                    0,
                    "all_tabs".to_string(),
                    format!("Closed {} tab(s) in the current session", total_tabs),
                )
            };

        Ok(context.finish(ToolResult::success_with(CloseOutput {
            closed_tabs,
            skipped_tabs,
            scope,
            session_origin,
            message,
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::BrowserSession;
    use crate::browser::SessionOrigin;
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
            .execute_typed(
                CloseParams {
                    confirm_destructive: false,
                },
                &mut context,
            )
            .expect("close should succeed");

        assert!(result.success);
        let data = result.data.expect("close should include data");
        assert_eq!(
            data["message"].as_str(),
            Some("Closed 2 tab(s) in the current session")
        );
        assert_eq!(data["closed_tabs"].as_u64(), Some(2));
        assert_eq!(data["skipped_tabs"].as_u64(), Some(0));
        assert_eq!(data["scope"].as_str(), Some("all_tabs"));
        assert_eq!(data["session_origin"].as_str(), Some("launched"));
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
            .execute_typed(
                CloseParams {
                    confirm_destructive: false,
                },
                &mut context,
            )
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

    #[test]
    fn test_close_tool_connected_session_closes_only_managed_tabs_by_default() {
        let session = BrowserSession::with_test_backend_origin(
            FakeSessionBackend::new(),
            SessionOrigin::Connected,
        );
        session
            .open_tab_entry("https://managed.example")
            .expect("managed tab should open");

        let tool = CloseTool::default();
        let mut context = ToolContext::new(&session);
        let result = tool
            .execute_typed(
                CloseParams {
                    confirm_destructive: false,
                },
                &mut context,
            )
            .expect("connected close should succeed");

        assert!(result.success);
        let data = result.data.expect("close should include data");
        assert_eq!(data["closed_tabs"].as_u64(), Some(1));
        assert_eq!(data["skipped_tabs"].as_u64(), Some(1));
        assert_eq!(data["scope"].as_str(), Some("managed_only"));
        assert_eq!(data["session_origin"].as_str(), Some("connected"));
        let remaining_tabs = session.tab_overview().expect("tabs should still load");
        assert_eq!(remaining_tabs.len(), 1);
        assert_eq!(remaining_tabs[0].url, "about:blank");
    }

    #[test]
    fn test_close_tool_connected_session_can_close_all_tabs_with_confirmation() {
        let session = BrowserSession::with_test_backend_origin(
            FakeSessionBackend::new(),
            SessionOrigin::Connected,
        );
        session
            .open_tab_entry("https://managed.example")
            .expect("managed tab should open");

        let tool = CloseTool::default();
        let mut context = ToolContext::new(&session);
        let result = tool
            .execute_typed(
                CloseParams {
                    confirm_destructive: true,
                },
                &mut context,
            )
            .expect("connected destructive close should succeed");

        assert!(result.success);
        let data = result.data.expect("close should include data");
        assert_eq!(data["closed_tabs"].as_u64(), Some(2));
        assert_eq!(data["skipped_tabs"].as_u64(), Some(0));
        assert_eq!(data["scope"].as_str(), Some("all_tabs"));
        assert_eq!(data["session_origin"].as_str(), Some("connected"));
        assert!(session.tab_overview().expect("tabs should load").is_empty());
    }
}

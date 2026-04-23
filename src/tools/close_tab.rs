use crate::error::{BrowserError, Result};
use crate::tools::core::structured_tool_failure;
use crate::tools::{TabSummary, Tool, ToolContext, ToolResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parameters for the close_tab tool (no parameters needed)
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CloseTabParams {
    /// Allow closing an unmanaged active tab in connected sessions
    #[serde(default)]
    pub confirm_destructive: bool,
}

/// Tool for closing the current active tab
#[derive(Default)]
pub struct CloseTabTool;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CloseTabOutput {
    pub action: String,
    pub closed_tab: TabSummary,
    pub active_tab: Option<TabSummary>,
    pub message: String,
}

impl Tool for CloseTabTool {
    type Params = CloseTabParams;
    type Output = CloseTabOutput;

    fn name(&self) -> &str {
        "close_tab"
    }

    fn description(&self) -> &str {
        "Close the active tab; connected sessions require confirm_destructive for unmanaged tabs."
    }

    fn execute_typed(
        &self,
        params: CloseTabParams,
        context: &mut ToolContext,
    ) -> Result<ToolResult> {
        let tabs = context.session.tab_overview()?;
        let tab_count = tabs.len();
        if tab_count == 0 {
            return Ok(context.finish(close_tab_failure(
                "no_tabs",
                "No tabs available".to_string(),
                "new_tab",
                tab_count,
            )));
        }

        if context.session.is_connected_session()
            && !params.confirm_destructive
            && let Some((index, active)) = tabs.iter().enumerate().find(|(_, tab)| tab.active)
            && !context.session.is_tab_managed(&active.id)?
        {
            return Ok(context.finish(close_tab_confirmation_required(
                index,
                active,
                tab_count,
                context.session.session_origin_label(),
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
        let active_tab = context
            .session
            .tab_overview()?
            .into_iter()
            .enumerate()
            .find(|(_, tab)| tab.active)
            .map(|(index, tab)| TabSummary::from_browser_tab(index, &tab));
        let closed_tab = TabSummary {
            tab_id: closed.id,
            index: closed.index,
            active: false,
            title: closed.title,
            url: closed.url,
        };

        Ok(context.finish(ToolResult::success_with(CloseTabOutput {
            action: "close_tab".to_string(),
            message,
            closed_tab,
            active_tab,
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
    structured_tool_failure(
        code,
        error,
        None,
        None,
        Some(serde_json::json!({
            "suggested_tool": suggested_tool,
        })),
        Some(serde_json::json!({
            "tab_count": tab_count,
        })),
    )
}

fn close_tab_confirmation_required(
    index: usize,
    active: &crate::browser::TabInfo,
    tab_count: usize,
    session_origin: &str,
) -> ToolResult {
    let error = format!(
        "Active tab {} is not managed by this connected session; set confirm_destructive=true to close it",
        active.id
    );

    structured_tool_failure(
        "destructive_confirmation_required",
        error,
        None,
        None,
        Some(serde_json::json!({
            "suggested_tool": "tab_list",
        })),
        Some(serde_json::json!({
            "session_origin": session_origin,
            "tab_count": tab_count,
            "active_tab": {
                "tab_id": active.id,
                "index": index,
                "active": active.active,
                "title": active.title,
                "url": active.url,
            },
            "active_tab_managed": false,
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::BrowserSession;
    use crate::browser::SessionOrigin;
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

        let tool = CloseTabTool;
        let mut context = ToolContext::new(&session);
        let result = tool
            .execute_typed(
                CloseTabParams {
                    confirm_destructive: false,
                },
                &mut context,
            )
            .expect("close_tab should succeed");

        assert!(result.success);
        let data = result.data.expect("close_tab should include data");
        assert_eq!(data["closed_tab"]["tab_id"].as_str(), Some("tab-2"));
        assert_eq!(data["closed_tab"]["index"].as_u64(), Some(1));
        assert_eq!(
            data["closed_tab"]["url"].as_str(),
            Some("https://second.example")
        );
        assert_eq!(data["active_tab"]["tab_id"].as_str(), Some("tab-1"));
        assert_eq!(data["active_tab"]["index"].as_u64(), Some(0));
        assert_eq!(data["active_tab"]["active"].as_bool(), Some(true));
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

        let tool = CloseTabTool;
        let mut context = ToolContext::new(&session);
        let result = tool
            .execute_typed(
                CloseTabParams {
                    confirm_destructive: false,
                },
                &mut context,
            )
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

        let tool = CloseTabTool;
        let mut context = ToolContext::new(&session);
        let result = tool
            .execute_typed(
                CloseTabParams {
                    confirm_destructive: false,
                },
                &mut context,
            )
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

    #[test]
    fn test_close_tab_tool_requires_confirmation_for_unmanaged_active_tab_in_connected_session() {
        let session = BrowserSession::with_test_backend_origin(
            FakeSessionBackend::new(),
            SessionOrigin::Connected,
        );

        let tool = CloseTabTool;
        let mut context = ToolContext::new(&session);
        let result = tool
            .execute_typed(
                CloseTabParams {
                    confirm_destructive: false,
                },
                &mut context,
            )
            .expect("close_tab should return a structured failure");

        assert!(!result.success);
        assert_eq!(
            result.error.as_deref(),
            Some(
                "Active tab tab-1 is not managed by this connected session; set confirm_destructive=true to close it"
            )
        );
        let data = result
            .data
            .expect("close_tab failure should include details");
        assert_eq!(
            data["code"].as_str(),
            Some("destructive_confirmation_required")
        );
        assert_eq!(
            data["details"]["session_origin"].as_str(),
            Some("connected")
        );
        assert_eq!(
            data["details"]["active_tab"]["tab_id"].as_str(),
            Some("tab-1")
        );
        assert_eq!(data["details"]["active_tab_managed"].as_bool(), Some(false));
    }

    #[test]
    fn test_close_tab_tool_connected_session_closes_managed_active_tab_without_confirmation() {
        let session = BrowserSession::with_test_backend_origin(
            FakeSessionBackend::new(),
            SessionOrigin::Connected,
        );
        session
            .open_tab_entry("https://managed.example")
            .expect("managed tab should open");

        let tool = CloseTabTool;
        let mut context = ToolContext::new(&session);
        let result = tool
            .execute_typed(
                CloseTabParams {
                    confirm_destructive: false,
                },
                &mut context,
            )
            .expect("managed active tab should close");

        assert!(result.success);
        let data = result.data.expect("close_tab should include data");
        assert_eq!(data["closed_tab"]["tab_id"].as_str(), Some("tab-2"));
        assert_eq!(data["active_tab"]["tab_id"].as_str(), Some("tab-1"));
    }

    #[test]
    fn test_close_tab_tool_connected_session_can_close_unmanaged_active_tab_with_confirmation() {
        let session = BrowserSession::with_test_backend_origin(
            FakeSessionBackend::new(),
            SessionOrigin::Connected,
        );

        let tool = CloseTabTool;
        let mut context = ToolContext::new(&session);
        let result = tool
            .execute_typed(
                CloseTabParams {
                    confirm_destructive: true,
                },
                &mut context,
            )
            .expect("confirmed destructive close_tab should succeed");

        assert!(result.success);
        let data = result.data.expect("close_tab should include data");
        assert_eq!(data["closed_tab"]["tab_id"].as_str(), Some("tab-1"));
        assert!(data["active_tab"].is_null());
        assert!(session.tab_overview().expect("tabs should load").is_empty());
    }
}

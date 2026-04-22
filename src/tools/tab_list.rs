use crate::error::Result;
use crate::tools::{Tool, ToolContext, ToolResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Information about a browser tab
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TabInfo {
    /// Tab index
    pub index: usize,
    /// Whether this is the active tab
    pub active: bool,
    /// Tab title
    pub title: String,
    /// Tab URL
    pub url: String,
}

/// Parameters for the tab_list tool (no parameters needed)
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TabListParams {}

/// Tool for listing all browser tabs
#[derive(Default)]
pub struct TabListTool;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TabListOutput {
    pub tab_list: Vec<TabInfo>,
    pub count: usize,
    pub summary: String,
}

impl Tool for TabListTool {
    type Params = TabListParams;
    type Output = TabListOutput;

    fn name(&self) -> &str {
        "tab_list"
    }

    fn description(&self) -> &str {
        "List tabs so you can choose a switch_tab target."
    }

    fn execute_typed(
        &self,
        _params: TabListParams,
        context: &mut ToolContext,
    ) -> Result<ToolResult> {
        let tab_list: Vec<TabInfo> = context
            .session
            .tab_overview()?
            .into_iter()
            .enumerate()
            .map(|(index, tab)| TabInfo {
                index,
                active: tab.active,
                title: tab.title,
                url: tab.url,
            })
            .collect();

        let active_index = tab_list.iter().position(|t| t.active);
        let summary = summarize_tab_list(&tab_list, active_index);

        Ok(context.finish(ToolResult::success_with(TabListOutput {
            count: tab_list.len(),
            summary,
            tab_list,
        })))
    }
}

fn summarize_tab_list(tab_list: &[TabInfo], active_index: Option<usize>) -> String {
    if tab_list.is_empty() {
        return "No tabs available".to_string();
    }

    let all_tabs_str = tab_list
        .iter()
        .map(|tab| format!("[{}] Title: {} (URL: {})", tab.index, tab.title, tab.url))
        .collect::<Vec<_>>()
        .join("\n");

    match active_index {
        Some(active_index) => {
            let active_info = &tab_list[active_index];
            format!(
                "Current Tab: [{}] {}\nAll Tabs:\n{}",
                active_index, active_info.title, all_tabs_str
            )
        }
        None => format!(
            "Current Tab: unavailable (active tab could not be determined)\nAll Tabs:\n{}",
            all_tabs_str
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::BrowserSession;
    use crate::browser::backend::{
        FakeSessionBackend, ScriptEvaluation, SessionBackend, TabDescriptor,
    };
    use crate::dom::{DocumentMetadata, DomTree};
    use crate::error::BrowserError;
    use crate::tools::{Tool, ToolContext};
    use std::time::Duration;

    #[test]
    fn test_summarize_tab_list_includes_active_tab_and_all_tabs() {
        let summary = summarize_tab_list(
            &[
                TabInfo {
                    index: 0,
                    active: false,
                    title: "First".to_string(),
                    url: "https://first.example".to_string(),
                },
                TabInfo {
                    index: 1,
                    active: true,
                    title: "Second".to_string(),
                    url: "https://second.example".to_string(),
                },
            ],
            Some(1),
        );

        assert!(summary.contains("Current Tab: [1] Second"));
        assert!(summary.contains("[0] Title: First"));
        assert!(summary.contains("[1] Title: Second"));
    }

    #[test]
    fn test_summarize_tab_list_handles_empty_list() {
        assert_eq!(summarize_tab_list(&[], None), "No tabs available");
    }

    #[test]
    fn test_summarize_tab_list_reports_unknown_active_tab() {
        let summary = summarize_tab_list(
            &[TabInfo {
                index: 0,
                active: false,
                title: "Only".to_string(),
                url: "https://only.example".to_string(),
            }],
            None,
        );

        assert!(summary.contains("Current Tab: unavailable"));
        assert!(summary.contains("[0] Title: Only"));
    }

    #[test]
    fn test_tab_list_tool_does_not_invent_active_tab_when_backend_cannot_determine_one() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::with_no_active_tab());
        let tool = TabListTool::default();
        let mut context = ToolContext::new(&session);

        let result = tool
            .execute_typed(TabListParams {}, &mut context)
            .expect("tab_list should succeed");

        assert!(result.success);
        let data = result.data.expect("tab_list should include data");
        assert_eq!(data["tab_list"][0]["active"].as_bool(), Some(false));
        assert!(
            data["summary"]
                .as_str()
                .expect("summary should be present")
                .contains("Current Tab: unavailable")
        );
    }

    struct ActiveTabFailureBackend;

    impl SessionBackend for ActiveTabFailureBackend {
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
            unreachable!("evaluate is not used in this test")
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
                title: "Only".to_string(),
                url: "https://only.example".to_string(),
            }])
        }

        fn active_tab(&self) -> crate::error::Result<TabDescriptor> {
            Err(BrowserError::TabOperationFailed(
                "Failed to read active tab hint".to_string(),
            ))
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
    fn test_tab_list_tool_propagates_unexpected_active_tab_failures() {
        let session = BrowserSession::with_test_backend(ActiveTabFailureBackend);
        let tool = TabListTool::default();
        let mut context = ToolContext::new(&session);
        let err = tool
            .execute_typed(TabListParams {}, &mut context)
            .expect_err("unexpected active_tab failures should propagate");

        match err {
            BrowserError::TabOperationFailed(reason) => {
                assert!(reason.contains("Failed to read active tab hint"));
            }
            other => panic!("unexpected tab_list error: {other:?}"),
        }
    }
}

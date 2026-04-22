use crate::error::Result;
use crate::tools::{Tool, ToolContext, ToolResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Parameters for the switch_tab tool
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SwitchTabParams {
    /// Tab index to switch to
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,

    /// Stable tab identifier to switch to
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<String>,
}

/// Tool for switching to a specific tab
#[derive(Default)]
pub struct SwitchTabTool;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SwitchTabOutput {
    pub index: usize,
    pub tab_id: String,
    pub title: String,
    pub url: String,
    pub tab: SwitchTabTarget,
    pub active_tab: SwitchTabTarget,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SwitchTabTarget {
    pub tab_id: String,
    pub index: usize,
    pub active: bool,
    pub title: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SwitchTabRequest {
    Index(usize),
    TabId(String),
}

impl Tool for SwitchTabTool {
    type Params = SwitchTabParams;
    type Output = SwitchTabOutput;

    fn name(&self) -> &str {
        "switch_tab"
    }

    fn description(&self) -> &str {
        "Activate a tab by tab_id or index. Usually after tab_list; run snapshot before DOM actions."
    }

    fn execute_typed(
        &self,
        params: SwitchTabParams,
        context: &mut ToolContext,
    ) -> Result<ToolResult> {
        let tabs = context.session.tab_overview()?;
        let requested_index = params.index;
        let requested_tab_id = params.tab_id.clone();
        let request = match parse_switch_request(params) {
            Ok(request) => request,
            Err(failure) => return Ok(context.finish(failure)),
        };

        if tabs.is_empty() {
            return Ok(context.finish(switch_tab_failure(
                "no_tabs",
                "No tabs available".to_string(),
                requested_index,
                requested_tab_id,
                tabs.len(),
            )));
        }

        let (target_index, target_id) = match request {
            SwitchTabRequest::Index(index) => {
                if index >= tabs.len() {
                    return Ok(context.finish(switch_tab_failure(
                        "invalid_tab_index",
                        format!("Invalid tab index: {}. Valid range: 0-{}", index, tabs.len() - 1),
                        Some(index),
                        None,
                        tabs.len(),
                    )));
                }

                (index, tabs[index].id.clone())
            }
            SwitchTabRequest::TabId(tab_id) => {
                let Some(index) = tabs.iter().position(|tab| tab.id == tab_id) else {
                    return Ok(context.finish(switch_tab_failure_with_available_ids(
                        "invalid_tab_id",
                        format!("No tab found for id {}", tab_id),
                        None,
                        Some(tab_id),
                        tabs.len(),
                        &tabs,
                    )));
                };

                (index, tabs[index].id.clone())
            }
        };

        context.session.activate_tab_by_id(&target_id)?;
        let tabs = context.session.tab_overview()?;
        let active_index = tabs
            .iter()
            .position(|tab| tab.active)
            .unwrap_or(target_index);
        let title = tabs[target_index].title.clone();
        let url = tabs[target_index].url.clone();
        let tab = build_switch_tab_target(target_index, &tabs[target_index]);
        let active_tab = build_switch_tab_target(active_index, &tabs[active_index]);

        let summary = format_switch_summary(
            active_index,
            &tabs
                .iter()
                .enumerate()
                .map(|(idx, tab)| TabSummaryLine {
                    index: idx,
                    title: tab.title.clone(),
                    url: tab.url.clone(),
                })
                .collect::<Vec<_>>(),
        );

        Ok(context.finish(ToolResult::success_with(SwitchTabOutput {
            index: target_index,
            tab_id: target_id,
            message: summary,
            title,
            url,
            tab,
            active_tab,
        })))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TabSummaryLine {
    index: usize,
    title: String,
    url: String,
}

fn format_switch_summary(index: usize, tabs: &[TabSummaryLine]) -> String {
    let tab_list_str = tabs
        .iter()
        .map(|tab| format!("[{}] {} ({})", tab.index, tab.title, tab.url))
        .collect::<Vec<_>>()
        .join("\n");

    format!("Switched to tab {}\nAll Tabs:\n{}\n", index, tab_list_str)
}

fn parse_switch_request(
    params: SwitchTabParams,
) -> std::result::Result<SwitchTabRequest, ToolResult> {
    match (params.index, params.tab_id) {
        (Some(index), None) => Ok(SwitchTabRequest::Index(index)),
        (None, Some(tab_id)) if !tab_id.trim().is_empty() => Ok(SwitchTabRequest::TabId(tab_id)),
        (Some(index), Some(tab_id)) => Err(switch_tab_failure(
            "invalid_target_request",
            "Provide exactly one of index or tab_id".to_string(),
            Some(index),
            Some(tab_id),
            0,
        )),
        (None, None) => Err(switch_tab_failure(
            "invalid_target_request",
            "Provide exactly one of index or tab_id".to_string(),
            None,
            None,
            0,
        )),
        (None, Some(tab_id)) => Err(switch_tab_failure(
            "invalid_target_request",
            "tab_id must not be empty".to_string(),
            None,
            Some(tab_id),
            0,
        )),
    }
}

fn build_switch_tab_target(index: usize, tab: &crate::browser::TabInfo) -> SwitchTabTarget {
    SwitchTabTarget {
        tab_id: tab.id.clone(),
        index,
        active: tab.active,
        title: tab.title.clone(),
        url: tab.url.clone(),
    }
}

fn switch_tab_failure(
    code: &str,
    error: String,
    requested_index: Option<usize>,
    requested_tab_id: Option<String>,
    tab_count: usize,
) -> ToolResult {
    let valid_min = (tab_count > 0).then_some(0usize);
    let valid_max = tab_count.checked_sub(1);

    ToolResult::failure_with(
        error.clone(),
        json!({
            "code": code,
            "error": error,
            "requested_index": requested_index,
            "requested_tab_id": requested_tab_id,
            "tab_count": tab_count,
            "valid_min": valid_min,
            "valid_max": valid_max,
            "recovery": {
                "suggested_tool": "tab_list",
            }
        }),
    )
}

fn switch_tab_failure_with_available_ids(
    code: &str,
    error: String,
    requested_index: Option<usize>,
    requested_tab_id: Option<String>,
    tab_count: usize,
    tabs: &[crate::browser::TabInfo],
) -> ToolResult {
    let mut failure = switch_tab_failure(code, error, requested_index, requested_tab_id, tab_count);
    if let Some(data) = failure.data.as_mut() {
        data["available_tab_ids"] = json!(tabs.iter().map(|tab| tab.id.clone()).collect::<Vec<_>>());
    }
    failure
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::BrowserSession;
    use crate::browser::backend::FakeSessionBackend;

    #[test]
    fn test_format_switch_summary_lists_all_tabs() {
        let summary = format_switch_summary(
            2,
            &[
                TabSummaryLine {
                    index: 0,
                    title: "First".to_string(),
                    url: "https://first.example".to_string(),
                },
                TabSummaryLine {
                    index: 2,
                    title: "Second".to_string(),
                    url: "https://second.example".to_string(),
                },
            ],
        );

        assert!(summary.contains("Switched to tab 2"));
        assert!(summary.contains("[0] First"));
        assert!(summary.contains("[2] Second"));
    }

    #[test]
    fn test_switch_tab_tool_executes_against_fake_backend() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        session
            .open_tab_entry("https://second.example")
            .expect("second tab should open");

        let tool = SwitchTabTool::default();
        let mut context = ToolContext::new(&session);
        let result = tool
            .execute_typed(
                SwitchTabParams {
                    index: Some(0),
                    tab_id: None,
                },
                &mut context,
            )
            .expect("switch_tab should succeed");

        assert!(result.success);
        let data = result.data.expect("switch_tab should include data");
        assert_eq!(data["index"].as_u64(), Some(0));
        assert_eq!(data["tab_id"].as_str(), Some("tab-1"));
        assert_eq!(data["url"].as_str(), Some("about:blank"));
        assert_eq!(data["tab"]["tab_id"].as_str(), Some("tab-1"));
        assert_eq!(data["active_tab"]["tab_id"].as_str(), Some("tab-1"));

        let active_index = session
            .tab_overview()
            .expect("tabs should load")
            .iter()
            .position(|tab| tab.active);
        assert_eq!(active_index, Some(0));
    }

    #[test]
    fn test_switch_tab_tool_executes_against_fake_backend_by_tab_id() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        session
            .open_tab_entry("https://second.example")
            .expect("second tab should open");

        let tool = SwitchTabTool::default();
        let mut context = ToolContext::new(&session);
        let result = tool
            .execute_typed(
                SwitchTabParams {
                    index: None,
                    tab_id: Some("tab-1".to_string()),
                },
                &mut context,
            )
            .expect("switch_tab by tab_id should succeed");

        assert!(result.success);
        let data = result.data.expect("switch_tab should include data");
        assert_eq!(data["tab_id"].as_str(), Some("tab-1"));
        assert_eq!(data["active_tab"]["tab_id"].as_str(), Some("tab-1"));
        assert_eq!(data["index"].as_u64(), Some(0));
    }

    #[test]
    fn test_switch_tab_tool_returns_structured_failure_when_no_tabs_are_available() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        session.close().expect("session close should succeed");

        let tool = SwitchTabTool::default();
        let mut context = ToolContext::new(&session);
        let result = tool
            .execute_typed(
                SwitchTabParams {
                    index: Some(0),
                    tab_id: None,
                },
                &mut context,
            )
            .expect("switch_tab should return a structured failure");

        assert!(!result.success);
        assert_eq!(result.error.as_deref(), Some("No tabs available"));
        let data = result
            .data
            .expect("switch_tab failure should include details");
        assert_eq!(data["code"].as_str(), Some("no_tabs"));
        assert_eq!(data["requested_index"].as_u64(), Some(0));
        assert!(data["requested_tab_id"].is_null());
        assert_eq!(data["tab_count"].as_u64(), Some(0));
        assert!(data["valid_min"].is_null());
        assert!(data["valid_max"].is_null());
        assert_eq!(
            data["recovery"]["suggested_tool"].as_str(),
            Some("tab_list")
        );
    }

    #[test]
    fn test_switch_tab_tool_returns_structured_failure_for_invalid_index() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        session
            .open_tab_entry("https://second.example")
            .expect("second tab should open");

        let tool = SwitchTabTool::default();
        let mut context = ToolContext::new(&session);
        let result = tool
            .execute_typed(
                SwitchTabParams {
                    index: Some(999),
                    tab_id: None,
                },
                &mut context,
            )
            .expect("switch_tab should return a structured failure");

        assert!(!result.success);
        assert_eq!(
            result.error.as_deref(),
            Some("Invalid tab index: 999. Valid range: 0-1")
        );
        let data = result
            .data
            .expect("switch_tab failure should include details");
        assert_eq!(data["code"].as_str(), Some("invalid_tab_index"));
        assert_eq!(data["requested_index"].as_u64(), Some(999));
        assert!(data["requested_tab_id"].is_null());
        assert_eq!(data["tab_count"].as_u64(), Some(2));
        assert_eq!(data["valid_min"].as_u64(), Some(0));
        assert_eq!(data["valid_max"].as_u64(), Some(1));
        assert_eq!(
            data["recovery"]["suggested_tool"].as_str(),
            Some("tab_list")
        );
    }

    #[test]
    fn test_switch_tab_tool_returns_structured_failure_for_invalid_tab_id() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        session
            .open_tab_entry("https://second.example")
            .expect("second tab should open");

        let tool = SwitchTabTool::default();
        let mut context = ToolContext::new(&session);
        let result = tool
            .execute_typed(
                SwitchTabParams {
                    index: None,
                    tab_id: Some("tab-999".to_string()),
                },
                &mut context,
            )
            .expect("invalid tab_id should stay a structured failure");

        assert!(!result.success);
        assert_eq!(result.error.as_deref(), Some("No tab found for id tab-999"));
        let data = result
            .data
            .expect("switch_tab failure should include details");
        assert_eq!(data["code"].as_str(), Some("invalid_tab_id"));
        assert!(data["requested_index"].is_null());
        assert_eq!(data["requested_tab_id"].as_str(), Some("tab-999"));
        assert_eq!(data["tab_count"].as_u64(), Some(2));
        assert_eq!(data["available_tab_ids"][0].as_str(), Some("tab-1"));
        assert_eq!(data["available_tab_ids"][1].as_str(), Some("tab-2"));
    }

    #[test]
    fn test_switch_tab_tool_requires_exactly_one_target_handle() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());

        let tool = SwitchTabTool::default();
        let mut context = ToolContext::new(&session);
        let result = tool
            .execute_typed(
                SwitchTabParams {
                    index: Some(0),
                    tab_id: Some("tab-1".to_string()),
                },
                &mut context,
            )
            .expect("ambiguous target request should stay a structured failure");

        assert!(!result.success);
        assert_eq!(result.error.as_deref(), Some("Provide exactly one of index or tab_id"));
        let data = result
            .data
            .expect("switch_tab failure should include details");
        assert_eq!(data["code"].as_str(), Some("invalid_target_request"));
        assert_eq!(data["requested_index"].as_u64(), Some(0));
        assert_eq!(data["requested_tab_id"].as_str(), Some("tab-1"));
    }
}

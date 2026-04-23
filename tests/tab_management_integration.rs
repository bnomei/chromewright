mod common;

use chromewright::tools::{
    CloseTabParams, NewTabParams, SwitchTabParams, TabListParams, Tool, ToolContext,
    close_tab::CloseTabTool, new_tab::NewTabTool, switch_tab::SwitchTabTool, tab_list::TabListTool,
};
use log::info;

#[test]
#[ignore]
fn test_new_tab() {
    use chromewright::tools::{NewTabParams, Tool, ToolContext, new_tab::NewTabTool};

    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    // Navigate to initial page
    common::navigate_and_wait(
        session,
        "data:text/html,<html><body><h1>First Tab</h1></body></html>",
    )
    .expect("Failed to navigate");

    // Get initial tab count
    let initial_tabs = session.list_tabs().expect("Failed to list tabs");
    let initial_count = initial_tabs.len();
    info!("Initial tab count: {}", initial_count);

    // Create tool and context
    let tool = NewTabTool;
    let mut context = ToolContext::new(session);

    // Execute the tool to create a new tab
    let result = tool
        .execute_typed(
            NewTabParams {
                url: "data:text/html,<html><body><h1>Second Tab</h1></body></html>".to_string(),
                allow_unsafe: true,
            },
            &mut context,
        )
        .expect("Failed to execute new_tab tool");

    // Verify the result
    assert!(result.success, "Tool execution should succeed");
    assert!(result.data.is_some());

    let data = result.data.unwrap();
    assert!(
        data["url"].as_str().is_some(),
        "Result should contain url field"
    );
    assert!(
        data["tab"]["tab_id"].as_str().is_some(),
        "Result should contain structured tab metadata"
    );
    assert!(
        data["active_tab"]["tab_id"].as_str().is_some(),
        "Result should contain active_tab metadata"
    );
    assert!(
        data["message"].as_str().is_some(),
        "Result should contain message field"
    );

    info!(
        "New tab result: {}",
        serde_json::to_string_pretty(&data).unwrap()
    );

    common::wait_for_tab_count(session, initial_count + 1).expect("Second tab should appear");

    // Verify tab count increased
    let final_tabs = session.list_tabs().expect("Failed to list tabs");
    let final_count = final_tabs.len();
    info!("Final tab count: {}", final_count);

    assert_eq!(
        final_count,
        initial_count + 1,
        "Tab count should increase by 1"
    );
}

#[test]
#[ignore] // Requires Chrome to be installed
fn test_tab_list() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    // Navigate to a simple page
    common::navigate_and_wait(
        session,
        "data:text/html,<html><body><h1>First Tab</h1></body></html>",
    )
    .expect("Failed to navigate");

    // Create tool and context
    let tool = TabListTool;
    let mut context = ToolContext::new(session);

    // Execute the tool
    let result = tool
        .execute_typed(TabListParams {}, &mut context)
        .expect("Failed to execute tab_list tool");

    // Verify the result
    assert!(result.success, "Tool execution should succeed");
    assert!(result.data.is_some());

    let data = result.data.unwrap();
    let tabs = data["tabs"].as_array().expect("No tabs field");
    let count = data["count"].as_u64().expect("No count field");

    info!("Tab list: {}", serde_json::to_string_pretty(&tabs).unwrap());

    // Should have at least 1 tab
    assert!(count >= 1, "Expected at least 1 tab");
    assert_eq!(tabs.len() as u64, count);

    // Check first tab structure
    let first_tab = &tabs[0];
    assert!(
        first_tab["tab_id"].is_string(),
        "Tab should have stable tab id"
    );
    assert!(first_tab["index"].is_number(), "Tab should have index");
    assert!(
        first_tab["active"].is_boolean(),
        "Tab should have active flag"
    );
    assert!(first_tab["title"].is_string(), "Tab should have title");
    assert!(first_tab["url"].is_string(), "Tab should have url");
    if count > 0 {
        assert!(
            data["active_tab"]["tab_id"].is_string(),
            "tab_list should expose active_tab metadata when one is known"
        );
    }
}

#[test]
#[ignore]
fn test_new_tab_and_switch() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    // Navigate to initial page
    common::navigate_and_wait(
        session,
        "data:text/html,<html><body><h1>First Tab</h1></body></html>",
    )
    .expect("Failed to navigate");

    // Create a new tab
    let new_tab_tool = NewTabTool;
    let mut context = ToolContext::new(session);

    let result = new_tab_tool
        .execute_typed(
            NewTabParams {
                url: "data:text/html,<html><body><h1>Second Tab</h1></body></html>".to_string(),
                allow_unsafe: true,
            },
            &mut context,
        )
        .expect("Failed to execute new_tab tool");

    assert!(result.success, "New tab creation should succeed");

    common::wait_for_tab_count_at_least(session, 2).expect("New tab should be listed");

    // List tabs to verify count increased by 1
    let tab_list_tool = TabListTool;
    let mut context = ToolContext::new(session);

    let result = tab_list_tool
        .execute_typed(TabListParams {}, &mut context)
        .expect("Failed to execute tab_list tool");

    assert!(result.success);
    let data = result.data.unwrap();
    let count = data["count"].as_u64().expect("No count field");

    info!("Tab count after creating new tab: {}", count);
    assert!(count >= 2, "Should have at least 2 tabs, got {}", count);

    // Switch to first tab via stable tab_id
    let switch_tab_tool = SwitchTabTool;
    let mut context = ToolContext::new(session);
    let first_tab_id = data["tabs"][0]["tab_id"]
        .as_str()
        .expect("tab_list should expose the stable tab id")
        .to_string();

    let result = switch_tab_tool
        .execute_typed(
            SwitchTabParams {
                index: None,
                tab_id: Some(first_tab_id.clone()),
            },
            &mut context,
        )
        .expect("Failed to execute switch_tab tool");

    assert!(result.success, "Switch tab should succeed");

    let data = result.data.unwrap();
    assert_eq!(data["tab"]["index"].as_u64(), Some(0));
    assert_eq!(data["tab"]["tab_id"].as_str(), Some(first_tab_id.as_str()));
    assert_eq!(
        data["active_tab"]["tab_id"].as_str(),
        Some(first_tab_id.as_str())
    );
    info!(
        "Switched to tab: {}",
        serde_json::to_string_pretty(&data).unwrap()
    );

    common::wait_for_url_contains(session, "First Tab").expect("First tab should become active");

    let mut context = ToolContext::new(session);
    let result = tab_list_tool
        .execute_typed(TabListParams {}, &mut context)
        .expect("Failed to execute tab_list tool after switching");

    let data = result.data.unwrap();
    let tabs = data["tabs"]
        .as_array()
        .expect("tabs should be present after switching");
    let active_tabs: Vec<_> = tabs
        .iter()
        .filter(|tab| tab["active"].as_bool() == Some(true))
        .collect();

    assert_eq!(active_tabs.len(), 1, "Exactly one tab should be active");
    assert_eq!(active_tabs[0]["index"].as_u64(), Some(0));
}

#[test]
#[ignore]
fn test_switch_tab_invalid_index() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    common::navigate_and_wait(
        session,
        "data:text/html,<html><body><h1>Tab</h1></body></html>",
    )
    .expect("Failed to navigate");

    // Try to switch to invalid index
    let switch_tab_tool = SwitchTabTool;
    let mut context = ToolContext::new(session);

    let result = switch_tab_tool
        .execute_typed(
            SwitchTabParams {
                index: Some(999),
                tab_id: None,
            },
            &mut context,
        )
        .expect("Failed to execute switch_tab tool");

    // Should fail gracefully
    assert!(!result.success, "Should fail for invalid index");
    assert_eq!(
        result.error.as_deref(),
        Some("Invalid tab index: 999. Valid range: 0-0")
    );
    let data = result
        .data
        .expect("invalid index failure should include structured details");
    assert_eq!(data["code"].as_str(), Some("invalid_tab_index"));
    assert_eq!(data["details"]["requested_index"].as_u64(), Some(999));
    assert_eq!(data["details"]["tab_count"].as_u64(), Some(1));
    assert_eq!(data["details"]["valid_min"].as_u64(), Some(0));
    assert_eq!(data["details"]["valid_max"].as_u64(), Some(0));
    assert_eq!(
        data["recovery"]["suggested_tool"].as_str(),
        Some("tab_list")
    );
    info!(
        "Expected structured error: {}",
        serde_json::to_string_pretty(&data).unwrap()
    );
}

#[test]
#[ignore]
fn test_close_tab() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    // Create two tabs
    common::navigate_and_wait(
        session,
        "data:text/html,<html><body><h1>First Tab</h1></body></html>",
    )
    .expect("Failed to navigate");

    let new_tab_tool = NewTabTool;
    let mut context = ToolContext::new(session);

    new_tab_tool
        .execute_typed(
            NewTabParams {
                url: "data:text/html,<html><body><h1>Second Tab</h1></body></html>".to_string(),
                allow_unsafe: true,
            },
            &mut context,
        )
        .expect("Failed to create new tab");

    common::wait_for_tab_count_at_least(session, 2).expect("Second tab should be listed");

    // Verify we have at least 2 tabs
    let tab_list_tool = TabListTool;
    let mut context = ToolContext::new(session);

    let result = tab_list_tool
        .execute_typed(TabListParams {}, &mut context)
        .expect("Failed to execute tab_list tool");

    let count_before = result.data.unwrap()["count"].as_u64().unwrap();
    info!("Tab count before closing: {}", count_before);
    assert!(
        count_before >= 2,
        "Should have at least 2 tabs before closing, got {}",
        count_before
    );

    // Close the active tab (second tab)
    let close_tab_tool = CloseTabTool;
    let mut context = ToolContext::new(session);

    let result = close_tab_tool
        .execute_typed(
            CloseTabParams {
                confirm_destructive: false,
            },
            &mut context,
        )
        .expect("Failed to execute close_tab tool");

    assert!(result.success, "Close tab should succeed");
    let closed_data = result.data.unwrap();
    assert!(
        closed_data["closed_tab"]["tab_id"].is_string(),
        "close_tab should expose structured closed_tab metadata"
    );
    if count_before > 1 {
        assert!(
            closed_data["active_tab"]["tab_id"].is_string(),
            "close_tab should expose the resulting active_tab when one remains"
        );
    }
    info!(
        "Closed tab: {}",
        serde_json::to_string_pretty(&closed_data).unwrap()
    );

    common::wait_for_tab_count(session, (count_before - 1) as usize)
        .expect("Tab count should decrease after close");

    // Verify we now have one less tab
    let mut context = ToolContext::new(session);
    let result = tab_list_tool
        .execute_typed(TabListParams {}, &mut context)
        .expect("Failed to execute tab_list tool");

    let data = result.data.unwrap();
    let count_after = data["count"].as_u64().unwrap();
    let tabs = data["tabs"]
        .as_array()
        .expect("tabs should be present after closing");
    let active_tabs: Vec<_> = tabs
        .iter()
        .filter(|tab| tab["active"].as_bool() == Some(true))
        .collect();
    info!("Tab count after closing: {}", count_after);
    assert_eq!(
        count_after,
        count_before - 1,
        "Should have one less tab after closing"
    );
    assert_eq!(
        active_tabs.len(),
        1,
        "Exactly one remaining tab should be active after closing the current tab"
    );
}

#[test]
#[ignore]
fn test_tab_workflow() {
    // Test a complete workflow: create multiple tabs, switch between them, list them, and close one
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    // Start with first tab
    common::navigate_and_wait(
        session,
        "data:text/html,<html><body><h1>Tab 1</h1></body></html>",
    )
    .expect("Failed to navigate");

    // Create second tab
    let new_tab_tool = NewTabTool;
    let mut context = ToolContext::new(session);

    new_tab_tool
        .execute_typed(
            NewTabParams {
                url: "data:text/html,<html><body><h1>Tab 2</h1></body></html>".to_string(),
                allow_unsafe: true,
            },
            &mut context,
        )
        .expect("Failed to create tab 2");

    common::wait_for_tab_count_at_least(session, 2).expect("Second tab should be listed");

    // Create third tab
    let mut context = ToolContext::new(session);
    new_tab_tool
        .execute_typed(
            NewTabParams {
                url: "data:text/html,<html><body><h1>Tab 3</h1></body></html>".to_string(),
                allow_unsafe: true,
            },
            &mut context,
        )
        .expect("Failed to create tab 3");

    common::wait_for_tab_count_at_least(session, 3).expect("Third tab should be listed");

    // List all tabs
    let tab_list_tool = TabListTool;
    let mut context = ToolContext::new(session);

    let result = tab_list_tool
        .execute_typed(TabListParams {}, &mut context)
        .expect("Failed to list tabs");

    let tab_data = result.data.unwrap();
    let count = tab_data["count"].as_u64().unwrap();
    info!("Total tabs: {}", count);
    assert!(count >= 3, "Should have at least 3 tabs, got {}", count);
    info!("All tabs: {}", tab_data["summary"].as_str().unwrap());
    let second_tab_id = tab_data["tabs"][1]["tab_id"]
        .as_str()
        .expect("tab_list should expose tab ids")
        .to_string();

    // Switch to second tab via stable tab_id
    let switch_tab_tool = SwitchTabTool;
    let mut context = ToolContext::new(session);

    let result = switch_tab_tool
        .execute_typed(
            SwitchTabParams {
                index: None,
                tab_id: Some(second_tab_id.clone()),
            },
            &mut context,
        )
        .expect("Failed to switch to tab 1");

    assert!(result.success);
    let switch_data = result.data.unwrap();
    assert_eq!(switch_data["tab"]["index"].as_u64(), Some(1));
    assert_eq!(
        switch_data["tab"]["tab_id"].as_str(),
        Some(second_tab_id.as_str())
    );
    assert_eq!(
        switch_data["active_tab"]["tab_id"].as_str(),
        Some(second_tab_id.as_str())
    );

    common::wait_for_url_contains(session, "Tab 2").expect("Second tab should become active");

    // Close the current tab (tab 2, index 1)
    let close_tab_tool = CloseTabTool;
    let mut context = ToolContext::new(session);

    let result = close_tab_tool
        .execute_typed(
            CloseTabParams {
                confirm_destructive: false,
            },
            &mut context,
        )
        .expect("Failed to close tab");

    assert!(result.success);
    let closed_data = result.data.unwrap();
    assert!(closed_data["closed_tab"]["tab_id"].is_string());
    assert!(closed_data["active_tab"]["tab_id"].is_string());
    info!("Closed: {}", closed_data["message"].as_str().unwrap());

    common::wait_for_tab_count(session, (count - 1) as usize)
        .expect("Closing a tab should reduce the tab count");

    // List tabs again to verify we have 2 tabs left
    let mut context = ToolContext::new(session);
    let result = tab_list_tool
        .execute_typed(TabListParams {}, &mut context)
        .expect("Failed to list tabs");

    let final_count = result.data.unwrap()["count"].as_u64().unwrap();
    info!("Final tab count: {}", final_count);
    assert_eq!(
        final_count,
        count - 1,
        "Should have one less tab after closing"
    );
}

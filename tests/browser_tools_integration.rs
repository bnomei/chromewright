mod common;

use chromewright::dom::{Cursor, NodeRef};
use chromewright::tools::{
    ClickParams, HoverParams, InputParams, InspectDetail, InspectNodeParams, ScrollParams,
    SelectParams, SnapshotParams, Tool, ToolContext, WaitCondition, WaitParams, click::ClickTool,
    hover::HoverTool, input::InputTool, inspect_node::InspectNodeTool, scroll::ScrollTool,
    select::SelectTool, snapshot::SnapshotTool, wait::WaitTool,
};
use log::info;
use serde_json::Value;

fn snapshot_cursor_for_selector(snapshot_data: &Value, selector: &str) -> Cursor {
    let nodes = snapshot_data["nodes"]
        .as_array()
        .expect("snapshot should return nodes");
    let cursor_value = nodes
        .iter()
        .find(|node| node["cursor"]["selector"].as_str() == Some(selector))
        .unwrap_or_else(|| panic!("expected snapshot cursor for selector {selector}"))["cursor"]
        .clone();

    serde_json::from_value(cursor_value).expect("cursor should deserialize")
}

fn snapshot_node_ref_for_selector(snapshot_data: &Value, selector: &str) -> NodeRef {
    let nodes = snapshot_data["nodes"]
        .as_array()
        .expect("snapshot should return nodes");
    let node_ref_value = nodes
        .iter()
        .find(|node| node["cursor"]["selector"].as_str() == Some(selector))
        .unwrap_or_else(|| panic!("expected snapshot node_ref for selector {selector}"))["node_ref"]
        .clone();

    serde_json::from_value(node_ref_value).expect("node_ref should deserialize")
}

#[test]
#[ignore] // Requires Chrome to be installed
fn test_select_tool() {
    let _guard = common::browser_test_guard();
    let Some(session) = common::launch_or_skip() else {
        return;
    };

    // Create a page with a select dropdown
    let html = r#"
        <!DOCTYPE html>
        <html>
        <body>
            <select id="country">
                <option value="us">United States</option>
                <option value="uk">United Kingdom</option>
                <option value="ca">Canada</option>
            </select>
            <div id="result"></div>
            <script>
                document.getElementById('country').addEventListener('change', function(e) {
                    document.getElementById('result').textContent = 'Selected: ' + e.target.value;
                });
            </script>
        </body>
        </html>
    "#;

    let data_url = format!("data:text/html,{}", html);
    session.navigate(&data_url).expect("Failed to navigate");

    std::thread::sleep(std::time::Duration::from_millis(500));

    // Create tool and context
    let tool = SelectTool::default();
    let mut context = ToolContext::new(&session);

    // Execute the tool to select an option
    let result = tool
        .execute_typed(
            SelectParams {
                selector: Some("#country".to_string()),
                index: None,
                node_ref: None,
                cursor: None,
                value: "uk".to_string(),
            },
            &mut context,
        )
        .expect("Failed to execute select tool");

    // Verify the result
    assert!(result.success, "Tool execution should succeed");
    assert!(result.data.is_some());

    let data = result.data.unwrap();
    info!(
        "Select result: {}",
        serde_json::to_string_pretty(&data).unwrap()
    );

    assert_eq!(data["value"].as_str(), Some("uk"));
    assert_eq!(data["selectedText"].as_str(), Some("United Kingdom"));
    assert_eq!(data["action"].as_str(), Some("select"));
    assert_eq!(data["target_before"]["selector"].as_str(), Some("#country"));
    assert_eq!(data["target_after"]["selector"].as_str(), Some("#country"));
    assert_eq!(data["target_status"].as_str(), Some("same"));
    assert_eq!(data["target"]["selector"].as_str(), Some("#country"));
    assert!(data["document"]["revision"].as_str().is_some());
    assert!(data["snapshot"].is_null());
    assert!(data["nodes"].is_null());
}

#[test]
#[ignore]
fn test_hover_tool() {
    let _guard = common::browser_test_guard();
    let Some(session) = common::launch_or_skip() else {
        return;
    };

    // Create a page with a hoverable element
    let html = r#"
        <!DOCTYPE html>
        <html>
        <body>
            <button id="hover-btn">Hover Me</button>
            <div id="result"></div>
            <script>
                document.getElementById('hover-btn').addEventListener('mouseover', function() {
                    document.getElementById('result').textContent = 'Hovered!';
                });
            </script>
        </body>
        </html>
    "#;

    let data_url = format!("data:text/html,{}", html);
    session.navigate(&data_url).expect("Failed to navigate");

    std::thread::sleep(std::time::Duration::from_millis(500));

    // Create tool and context
    let tool = HoverTool::default();
    let mut context = ToolContext::new(&session);

    // Execute the tool
    let result = tool
        .execute_typed(
            HoverParams {
                selector: Some("#hover-btn".to_string()),
                index: None,
                node_ref: None,
                cursor: None,
            },
            &mut context,
        )
        .expect("Failed to execute hover tool");

    // Verify the result
    assert!(result.success, "Tool execution should succeed");
    assert!(result.data.is_some());

    let data = result.data.unwrap();
    info!(
        "Hover result: {}",
        serde_json::to_string_pretty(&data).unwrap()
    );

    assert_eq!(data["action"].as_str(), Some("hover"));
    assert_eq!(
        data["target_before"]["selector"].as_str(),
        Some("#hover-btn")
    );
    assert_eq!(
        data["target_after"]["selector"].as_str(),
        Some("#hover-btn")
    );
    assert_eq!(data["target_status"].as_str(), Some("same"));
    assert_eq!(data["target"]["selector"].as_str(), Some("#hover-btn"));
    assert_eq!(data["element"]["tagName"].as_str(), Some("BUTTON"));
}

#[test]
#[ignore]
fn test_scroll_tool_with_amount() {
    let _guard = common::browser_test_guard();
    let Some(session) = common::launch_or_skip() else {
        return;
    };

    // Create a long page
    let html = r#"
        <!DOCTYPE html>
        <html>
        <body style="height: 3000px;">
            <h1>Top of page</h1>
            <div style="margin-top: 1000px;">Middle</div>
            <div style="margin-top: 1000px;">Bottom</div>
        </body>
        </html>
    "#;

    let data_url = format!("data:text/html,{}", html);
    session.navigate(&data_url).expect("Failed to navigate");

    std::thread::sleep(std::time::Duration::from_millis(500));

    // Create tool and context
    let tool = ScrollTool::default();
    let mut context = ToolContext::new(&session);

    // Execute the tool to scroll down 500 pixels
    let result = tool
        .execute_typed(ScrollParams { amount: Some(500) }, &mut context)
        .expect("Failed to execute scroll tool");

    // Verify the result
    assert!(result.success, "Tool execution should succeed");
    assert!(result.data.is_some());

    let data = result.data.unwrap();
    info!(
        "Scroll result: {}",
        serde_json::to_string_pretty(&data).unwrap()
    );

    let scrolled = data["scrolled"].as_i64();
    assert!(
        scrolled.is_some() && scrolled.unwrap() > 0,
        "Should have scrolled"
    );
}

#[test]
#[ignore]
fn test_scroll_tool_to_bottom() {
    let _guard = common::browser_test_guard();
    let Some(session) = common::launch_or_skip() else {
        return;
    };

    // Create a page
    let html = r#"
        <!DOCTYPE html>
        <html>
        <body style="height: 2000px;">
            <h1>Top of page</h1>
            <div style="margin-top: 1800px;">Bottom</div>
        </body>
        </html>
    "#;

    let data_url = format!("data:text/html,{}", html);
    session.navigate(&data_url).expect("Failed to navigate");

    std::thread::sleep(std::time::Duration::from_millis(500));

    // Create tool and context
    let tool = ScrollTool::default();
    let mut context = ToolContext::new(&session);

    // Execute the tool multiple times to reach bottom
    for _ in 0..10 {
        let result = tool
            .execute_typed(ScrollParams { amount: None }, &mut context)
            .expect("Failed to execute scroll tool");

        assert!(result.success);

        let data = result.data.as_ref().unwrap();
        let is_at_bottom = data["isAtBottom"].as_bool().unwrap_or(false);

        info!(
            "Scroll iteration: scrolled={}, isAtBottom={}",
            data["scrolled"], is_at_bottom
        );

        if is_at_bottom {
            info!("Reached bottom of page");
            break;
        }

        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

#[test]
#[ignore]
fn test_scroll_tool_returns_compact_viewport_follow_up_state() {
    let _guard = common::browser_test_guard();
    let Some(session) = common::launch_or_skip() else {
        return;
    };

    let html = r#"
        <!DOCTYPE html>
        <html>
        <body style="height: 2800px; margin: 0;">
            <div style="height: 2400px;">Spacer</div>
            <button id="bottom">Bottom button</button>
        </body>
        </html>
    "#;

    let data_url = format!("data:text/html,{}", html);
    session.navigate(&data_url).expect("Failed to navigate");
    std::thread::sleep(std::time::Duration::from_millis(500));

    let tool = ScrollTool::default();
    let mut context = ToolContext::new(&session);
    let result = tool
        .execute_typed(ScrollParams { amount: Some(420) }, &mut context)
        .expect("scroll should succeed");

    assert!(result.success);
    let data = result.data.expect("scroll should include data");
    let scroll_y = data["viewport_after"]["scroll_y"]
        .as_i64()
        .expect("scroll should include viewport_after.scroll_y");
    assert!(scroll_y > 0, "scroll_y should increase after scrolling");
    assert_eq!(data["viewport_after"]["is_at_top"].as_bool(), Some(false));
    assert!(data["document"]["revision"].as_str().is_some());
    assert!(data["target"].is_null());
    assert!(data["snapshot"].is_null());
    assert!(data["nodes"].is_null());
    assert!(data["interactive_count"].is_null());

    let actual_scroll_y = session
        .tab()
        .expect("tab should exist")
        .evaluate("window.scrollY", false)
        .expect("window.scrollY should be readable")
        .value
        .and_then(|value| value.as_i64())
        .expect("window.scrollY should be numeric");
    assert_eq!(scroll_y, actual_scroll_y);
}

#[test]
#[ignore]
fn test_select_with_index() {
    let _guard = common::browser_test_guard();
    let Some(session) = common::launch_or_skip() else {
        return;
    };

    // Create a page with a select dropdown
    let html = r#"
        <!DOCTYPE html>
        <html>
        <body>
            <select id="color">
                <option value="red">Red</option>
                <option value="green">Green</option>
                <option value="blue">Blue</option>
            </select>
        </body>
        </html>
    "#;

    let data_url = format!("data:text/html,{}", html);
    session.navigate(&data_url).expect("Failed to navigate");

    std::thread::sleep(std::time::Duration::from_millis(500));

    let snapshot_tool = SnapshotTool::default();
    let tool = SelectTool::default();
    let mut context = ToolContext::new(&session);

    let snapshot = snapshot_tool
        .execute_typed(SnapshotParams::default(), &mut context)
        .expect("Failed to execute snapshot tool");
    let node_ref: chromewright::dom::NodeRef =
        serde_json::from_value(snapshot.data.unwrap()["nodes"][0]["node_ref"].clone())
            .expect("node_ref should deserialize");

    let result = tool
        .execute_typed(
            SelectParams {
                selector: None,
                index: None,
                node_ref: Some(node_ref),
                cursor: None,
                value: "green".to_string(),
            },
            &mut context,
        )
        .expect("Select with node_ref should succeed");

    assert!(result.success);
    let data = result.data.unwrap();
    assert_eq!(data["selectedText"].as_str(), Some("Green"));
    assert_eq!(data["target_status"].as_str(), Some("same"));
}

#[test]
#[ignore]
fn test_select_tool_reports_rebound_handoff_after_replacement() {
    let _guard = common::browser_test_guard();
    let Some(session) = common::launch_or_skip() else {
        return;
    };

    let html = r#"
        <!DOCTYPE html>
        <html>
        <body>
            <select id="country">
                <option value="us">United States</option>
                <option value="uk">United Kingdom</option>
                <option value="ca">Canada</option>
            </select>
            <div id="status">initial</div>
            <script>
                document.getElementById('country').addEventListener('change', function(event) {
                    const replacement = this.cloneNode(true);
                    replacement.id = 'country';
                    replacement.value = event.target.value;
                    this.replaceWith(replacement);
                    document.getElementById('status').textContent = 'selected:' + replacement.value;
                });
            </script>
        </body>
        </html>
    "#;

    let data_url = format!("data:text/html,{}", html);
    session.navigate(&data_url).expect("Failed to navigate");
    std::thread::sleep(std::time::Duration::from_millis(500));

    let tool = SelectTool::default();
    let mut context = ToolContext::new(&session);
    let result = tool
        .execute_typed(
            SelectParams {
                selector: Some("#country".to_string()),
                index: None,
                node_ref: None,
                cursor: None,
                value: "ca".to_string(),
            },
            &mut context,
        )
        .expect("select should succeed");

    assert!(result.success);
    let data = result.data.expect("select should include data");
    assert_eq!(data["action"].as_str(), Some("select"));
    assert_eq!(data["value"].as_str(), Some("ca"));
    assert_eq!(data["selectedText"].as_str(), Some("Canada"));
    assert_eq!(data["target_before"]["selector"].as_str(), Some("#country"));
    assert_eq!(data["target_after"]["selector"].as_str(), Some("#country"));
    assert_eq!(data["target_status"].as_str(), Some("rebound"));
    assert_eq!(data["target"]["selector"].as_str(), Some("#country"));
    assert_ne!(
        data["target_before"]["node_ref"]["revision"].as_str(),
        data["target_after"]["node_ref"]["revision"].as_str()
    );

    let status = session
        .tab()
        .expect("tab should exist")
        .evaluate("document.getElementById('status').textContent", false)
        .expect("status text should be readable")
        .value
        .and_then(|value| value.as_str().map(str::to_string));
    assert_eq!(status.as_deref(), Some("selected:ca"));

    let selected_value = session
        .tab()
        .expect("tab should exist")
        .evaluate("document.getElementById('country').value", false)
        .expect("selected value should be readable")
        .value
        .and_then(|value| value.as_str().map(str::to_string));
    assert_eq!(selected_value.as_deref(), Some("ca"));
}

#[test]
#[ignore]
fn test_wait_tool_text_contains() {
    let _guard = common::browser_test_guard();
    let Some(session) = common::launch_or_skip() else {
        return;
    };

    let html = r#"
        <!DOCTYPE html>
        <html>
        <body>
            <div id="status" style="display: none;">Loading</div>
            <script>
                setTimeout(() => {
                    document.getElementById('status').textContent = 'Ready now';
                }, 150);
            </script>
        </body>
        </html>
    "#;

    let data_url = format!("data:text/html,{}", html);
    session.navigate(&data_url).expect("Failed to navigate");

    let tool = WaitTool::default();
    let mut context = ToolContext::new(&session);

    let result = tool
        .execute_typed(
            WaitParams {
                selector: Some("#status".to_string()),
                index: None,
                node_ref: None,
                cursor: None,
                condition: WaitCondition::TextContains,
                text: Some("Ready now".to_string()),
                value: None,
                since_revision: None,
                timeout_ms: 5_000,
            },
            &mut context,
        )
        .expect("Wait tool should succeed");

    assert!(result.success);
    let data = result.data.unwrap();
    assert_eq!(data["action"].as_str(), Some("wait"));
    assert_eq!(data["condition"].as_str(), Some("text_contains"));
    assert_eq!(data["target"]["selector"].as_str(), Some("#status"));
    assert!(data["document"]["revision"].as_str().is_some());
}

#[test]
#[ignore]
fn test_wait_tool_reuses_snapshot_node_ref_inside_same_origin_iframe() {
    let _guard = common::browser_test_guard();
    let Some(session) = common::launch_or_skip() else {
        return;
    };

    let html = r#"
        <!DOCTYPE html>
        <html>
        <body>
            <iframe srcdoc="<html><body><button id='inside'>Inside</button></body></html>"></iframe>
        </body>
        </html>
    "#;

    let data_url = format!("data:text/html,{}", html);
    session.navigate(&data_url).expect("Failed to navigate");
    std::thread::sleep(std::time::Duration::from_millis(500));

    let snapshot_tool = SnapshotTool::default();
    let wait_tool = WaitTool::default();
    let mut context = ToolContext::new(&session);

    let snapshot = snapshot_tool
        .execute_typed(SnapshotParams::default(), &mut context)
        .expect("snapshot should succeed");
    let snapshot_data = snapshot.data.expect("snapshot should include data");
    let node_ref = snapshot_node_ref_for_selector(&snapshot_data, "#inside");
    let node_ref_json = serde_json::to_value(&node_ref).expect("node_ref should serialize");

    let result = wait_tool
        .execute_typed(
            WaitParams {
                selector: None,
                index: None,
                node_ref: Some(node_ref),
                cursor: None,
                condition: WaitCondition::Visible,
                text: None,
                value: None,
                since_revision: None,
                timeout_ms: 5_000,
            },
            &mut context,
        )
        .expect("wait should succeed for iframe node_ref");

    assert!(result.success);
    let data = result.data.expect("wait should include data");
    assert_eq!(data["action"].as_str(), Some("wait"));
    assert_eq!(data["condition"].as_str(), Some("visible"));
    assert_eq!(data["target_before"]["method"].as_str(), Some("node_ref"));
    assert_eq!(data["target_before"]["selector"].as_str(), Some("#inside"));
    assert_eq!(data["target_before"]["node_ref"], node_ref_json);
    assert_eq!(data["target_after"]["selector"].as_str(), Some("#inside"));
    assert_eq!(data["target_status"].as_str(), Some("same"));
    assert_eq!(data["target"]["selector"].as_str(), Some("#inside"));
    assert!(data["document"]["revision"].as_str().is_some());
}

#[test]
#[ignore]
fn test_wait_tool_actionable_auto_waits_and_returns_handoff() {
    let _guard = common::browser_test_guard();
    let Some(session) = common::launch_or_skip() else {
        return;
    };

    let html = r#"
        <!DOCTYPE html>
        <html>
        <body style="margin: 0;">
            <button id="save">Save</button>
            <div id="overlay" style="position: fixed; inset: 0; background: rgba(0, 0, 0, 0.01);"></div>
            <script>
                setTimeout(() => document.getElementById('overlay').remove(), 150);
            </script>
        </body>
        </html>
    "#;

    let data_url = format!("data:text/html,{}", html);
    session.navigate(&data_url).expect("Failed to navigate");
    std::thread::sleep(std::time::Duration::from_millis(500));

    let tool = WaitTool::default();
    let mut context = ToolContext::new(&session);
    let result = tool
        .execute_typed(
            WaitParams {
                selector: Some("#save".to_string()),
                index: None,
                node_ref: None,
                cursor: None,
                condition: WaitCondition::Actionable,
                text: None,
                value: None,
                since_revision: None,
                timeout_ms: 5_000,
            },
            &mut context,
        )
        .expect("actionable wait should succeed");

    assert!(result.success);
    let data = result.data.expect("wait should include data");
    assert_eq!(data["condition"].as_str(), Some("actionable"));
    assert_eq!(data["target_before"]["selector"].as_str(), Some("#save"));
    assert_eq!(data["target_after"]["selector"].as_str(), Some("#save"));
    assert_eq!(data["target_status"].as_str(), Some("same"));
    assert_eq!(data["target"]["selector"].as_str(), Some("#save"));
}

#[test]
#[ignore]
fn test_wait_tool_stable_waits_for_layout_settle() {
    let _guard = common::browser_test_guard();
    let Some(session) = common::launch_or_skip() else {
        return;
    };

    let html = r#"
        <!DOCTYPE html>
        <html>
        <body style="margin: 0;">
            <button id="moving" style="position: absolute; top: 10px;">Moving</button>
            <script>
                const moving = document.getElementById('moving');
                let ticks = 0;
                const interval = setInterval(() => {
                    ticks += 1;
                    moving.style.top = `${10 + ticks * 8}px`;
                    if (ticks >= 6) {
                        clearInterval(interval);
                    }
                }, 30);
            </script>
        </body>
        </html>
    "#;

    let data_url = format!("data:text/html,{}", html);
    session.navigate(&data_url).expect("Failed to navigate");
    std::thread::sleep(std::time::Duration::from_millis(500));

    let tool = WaitTool::default();
    let mut context = ToolContext::new(&session);
    let result = tool
        .execute_typed(
            WaitParams {
                selector: Some("#moving".to_string()),
                index: None,
                node_ref: None,
                cursor: None,
                condition: WaitCondition::Stable,
                text: None,
                value: None,
                since_revision: None,
                timeout_ms: 5_000,
            },
            &mut context,
        )
        .expect("stable wait should succeed");

    assert!(result.success);
    let data = result.data.expect("wait should include data");
    assert_eq!(data["condition"].as_str(), Some("stable"));
    assert_eq!(data["target_status"].as_str(), Some("same"));
}

#[test]
#[ignore]
fn test_wait_tool_receives_events_waits_for_overlay_to_clear() {
    let _guard = common::browser_test_guard();
    let Some(session) = common::launch_or_skip() else {
        return;
    };

    let html = r#"
        <!DOCTYPE html>
        <html>
        <body style="margin: 0;">
            <button id="save" style="position: absolute; top: 20px; left: 20px;">Save</button>
            <div id="overlay" style="position: fixed; inset: 0; background: rgba(0, 0, 0, 0.01);"></div>
            <script>
                setTimeout(() => document.getElementById('overlay').remove(), 150);
            </script>
        </body>
        </html>
    "#;

    let data_url = format!("data:text/html,{}", html);
    session.navigate(&data_url).expect("Failed to navigate");
    std::thread::sleep(std::time::Duration::from_millis(500));

    let tool = WaitTool::default();
    let mut context = ToolContext::new(&session);
    let result = tool
        .execute_typed(
            WaitParams {
                selector: Some("#save".to_string()),
                index: None,
                node_ref: None,
                cursor: None,
                condition: WaitCondition::ReceivesEvents,
                text: None,
                value: None,
                since_revision: None,
                timeout_ms: 5_000,
            },
            &mut context,
        )
        .expect("receives_events wait should succeed");

    assert!(result.success);
    let data = result.data.expect("wait should include data");
    assert_eq!(data["condition"].as_str(), Some("receives_events"));
    assert_eq!(data["target_status"].as_str(), Some("same"));
}

#[test]
#[ignore]
fn test_click_tool_auto_waits_and_reports_rebound_handoff() {
    let _guard = common::browser_test_guard();
    let Some(session) = common::launch_or_skip() else {
        return;
    };

    let html = r#"
        <!DOCTYPE html>
        <html>
        <body style="margin: 0;">
            <div id="status">waiting</div>
            <button id="save" onclick="
                const replacement = document.createElement('button');
                replacement.id = 'save';
                replacement.textContent = 'Save v2';
                replacement.dataset.version = '2';
                this.replaceWith(replacement);
                document.getElementById('status').textContent = 'clicked';
            ">Save</button>
            <div id="overlay" style="position: fixed; inset: 0; background: rgba(0, 0, 0, 0.01);"></div>
            <script>
                setTimeout(() => document.getElementById('overlay').remove(), 150);
            </script>
        </body>
        </html>
    "#;

    let data_url = format!("data:text/html,{}", html);
    session.navigate(&data_url).expect("Failed to navigate");
    std::thread::sleep(std::time::Duration::from_millis(500));

    let tool = ClickTool::default();
    let mut context = ToolContext::new(&session);
    let result = tool
        .execute_typed(
            ClickParams {
                selector: Some("#save".to_string()),
                index: None,
                node_ref: None,
                cursor: None,
            },
            &mut context,
        )
        .expect("click should succeed after bounded auto-wait");

    assert!(result.success);
    let data = result.data.expect("click should include data");
    assert_eq!(data["action"].as_str(), Some("click"));
    assert_eq!(data["target_before"]["selector"].as_str(), Some("#save"));
    assert_eq!(data["target_after"]["selector"].as_str(), Some("#save"));
    assert_eq!(data["target_status"].as_str(), Some("rebound"));
    assert_ne!(
        data["target_before"]["node_ref"]["revision"].as_str(),
        data["target_after"]["node_ref"]["revision"].as_str()
    );
    assert_eq!(data["target"]["selector"].as_str(), Some("#save"));

    let status = session
        .tab()
        .expect("tab should exist")
        .evaluate("document.getElementById('status').textContent", false)
        .expect("status text should be readable")
        .value
        .and_then(|value| value.as_str().map(str::to_string));
    assert_eq!(status.as_deref(), Some("clicked"));
}

#[test]
#[ignore]
fn test_click_tool_hidden_target_returns_structured_failure() {
    let _guard = common::browser_test_guard();
    let Some(session) = common::launch_or_skip() else {
        return;
    };

    let html = r#"
        <!DOCTYPE html>
        <html>
        <body>
            <button id="hidden" style="display: none;">Hidden</button>
        </body>
        </html>
    "#;

    let data_url = format!("data:text/html,{}", html);
    session.navigate(&data_url).expect("Failed to navigate");
    std::thread::sleep(std::time::Duration::from_millis(500));

    let tool = ClickTool::default();
    let mut context = ToolContext::new(&session);
    let result = tool
        .execute_typed(
            ClickParams {
                selector: Some("#hidden".to_string()),
                index: None,
                node_ref: None,
                cursor: None,
            },
            &mut context,
        )
        .expect("click should return a structured failure");

    assert!(!result.success);
    let data = result.data.expect("failure should include data");
    assert_eq!(data["code"].as_str(), Some("target_not_visible"));
    assert_eq!(data["target_before"]["selector"].as_str(), Some("#hidden"));
    assert_eq!(
        data["recovery"]["suggested_tool"].as_str(),
        Some("inspect_node")
    );
}

#[test]
#[ignore]
fn test_click_tool_offscreen_target_auto_scrolls_into_view() {
    let _guard = common::browser_test_guard();
    let Some(session) = common::launch_or_skip() else {
        return;
    };

    let html = r#"
        <!DOCTYPE html>
        <html>
        <body style="margin: 0; min-height: 2400px;">
            <div id="status">waiting</div>
            <button
                id="offscreen"
                style="position: absolute; top: 1600px; left: 24px;"
                onclick="document.getElementById('status').textContent = 'clicked';"
            >
                Offscreen action
            </button>
        </body>
        </html>
    "#;

    let data_url = format!("data:text/html,{}", html);
    session.navigate(&data_url).expect("Failed to navigate");
    std::thread::sleep(std::time::Duration::from_millis(500));

    let tool = ClickTool::default();
    let mut context = ToolContext::new(&session);
    let result = tool
        .execute_typed(
            ClickParams {
                selector: Some("#offscreen".to_string()),
                index: None,
                node_ref: None,
                cursor: None,
            },
            &mut context,
        )
        .expect("click should succeed for offscreen targets");

    assert!(result.success);
    let data = result.data.expect("click should include data");
    assert_eq!(data["action"].as_str(), Some("click"));
    assert_eq!(
        data["target_before"]["selector"].as_str(),
        Some("#offscreen")
    );
    assert_eq!(
        data["target_after"]["selector"].as_str(),
        Some("#offscreen")
    );
    assert_eq!(data["target_status"].as_str(), Some("same"));

    let scroll_y = session
        .tab()
        .expect("tab should exist")
        .evaluate("window.scrollY", false)
        .expect("window.scrollY should be readable")
        .value
        .and_then(|value| value.as_i64())
        .expect("window.scrollY should be numeric");
    assert!(
        scroll_y > 0,
        "click should scroll the offscreen target into view"
    );

    let status = session
        .tab()
        .expect("tab should exist")
        .evaluate("document.getElementById('status').textContent", false)
        .expect("status text should be readable")
        .value
        .and_then(|value| value.as_str().map(str::to_string));
    assert_eq!(status.as_deref(), Some("clicked"));
}

#[test]
#[ignore]
fn test_input_tool_disabled_target_returns_structured_failure() {
    let _guard = common::browser_test_guard();
    let Some(session) = common::launch_or_skip() else {
        return;
    };

    let html = r#"
        <!DOCTYPE html>
        <html>
        <body>
            <input id="query" type="text" disabled value="draft">
        </body>
        </html>
    "#;

    let data_url = format!("data:text/html,{}", html);
    session.navigate(&data_url).expect("Failed to navigate");
    std::thread::sleep(std::time::Duration::from_millis(500));

    let tool = InputTool::default();
    let mut context = ToolContext::new(&session);
    let result = tool
        .execute_typed(
            InputParams {
                selector: Some("#query".to_string()),
                index: None,
                node_ref: None,
                cursor: None,
                text: "next".to_string(),
                clear: false,
            },
            &mut context,
        )
        .expect("input should return a structured failure");

    assert!(!result.success);
    let data = result.data.expect("failure should include data");
    assert_eq!(data["code"].as_str(), Some("target_not_enabled"));
    assert_eq!(data["target_before"]["selector"].as_str(), Some("#query"));
}

#[test]
#[ignore]
fn test_hover_tool_obscured_target_returns_structured_failure() {
    let _guard = common::browser_test_guard();
    let Some(session) = common::launch_or_skip() else {
        return;
    };

    let html = r#"
        <!DOCTYPE html>
        <html>
        <body style="margin: 0;">
            <button id="hover-btn" style="position: absolute; top: 20px; left: 20px;">Hover Me</button>
            <div id="overlay" style="position: fixed; inset: 0; background: rgba(0, 0, 0, 0.01);"></div>
        </body>
        </html>
    "#;

    let data_url = format!("data:text/html,{}", html);
    session.navigate(&data_url).expect("Failed to navigate");
    std::thread::sleep(std::time::Duration::from_millis(500));

    let tool = HoverTool::default();
    let mut context = ToolContext::new(&session);
    let result = tool
        .execute_typed(
            HoverParams {
                selector: Some("#hover-btn".to_string()),
                index: None,
                node_ref: None,
                cursor: None,
            },
            &mut context,
        )
        .expect("hover should return a structured failure");

    assert!(!result.success);
    let data = result.data.expect("failure should include data");
    assert_eq!(data["code"].as_str(), Some("target_obscured"));
    assert_eq!(
        data["target_before"]["selector"].as_str(),
        Some("#hover-btn")
    );
    let failed_predicates = data["failed_predicates"]
        .as_array()
        .expect("failed_predicates should be present");
    assert!(failed_predicates.iter().any(|value| {
        matches!(
            value.as_str(),
            Some("receives_events") | Some("unobscured_center")
        )
    }));
}

#[test]
#[ignore]
fn test_inspect_node_with_snapshot_cursor() {
    let _guard = common::browser_test_guard();
    let Some(session) = common::launch_or_skip() else {
        return;
    };

    let html = r#"
        <!DOCTYPE html>
        <html>
        <body>
            <button id="save">Save</button>
        </body>
        </html>
    "#;

    let data_url = format!("data:text/html,{}", html);
    session.navigate(&data_url).expect("Failed to navigate");
    std::thread::sleep(std::time::Duration::from_millis(500));
    session
        .tab()
        .expect("tab should exist")
        .evaluate("document.getElementById('save').focus();", false)
        .expect("button should be focusable");

    let snapshot_tool = SnapshotTool::default();
    let inspect_tool = InspectNodeTool::default();
    let mut context = ToolContext::new(&session);

    let snapshot = snapshot_tool
        .execute_typed(SnapshotParams::default(), &mut context)
        .expect("Failed to execute snapshot tool");
    let snapshot_data = snapshot.data.expect("snapshot should include data");
    let cursor = snapshot_cursor_for_selector(&snapshot_data, "#save");
    let cursor_json = serde_json::to_value(&cursor).expect("cursor should serialize");

    let result = inspect_tool
        .execute_typed(
            InspectNodeParams {
                selector: None,
                index: None,
                node_ref: None,
                cursor: Some(cursor),
                detail: InspectDetail::Compact,
                style_names: Vec::new(),
            },
            &mut context,
        )
        .expect("inspect_node should succeed");

    assert!(result.success);
    let data = result.data.unwrap();
    assert_eq!(data["action"].as_str(), Some("inspect_node"));
    assert_eq!(data["identity"]["tag"].as_str(), Some("button"));
    assert_eq!(data["accessibility"]["role"].as_str(), Some("button"));
    assert_eq!(data["accessibility"]["active"].as_bool(), Some(true));
    assert_eq!(data["target"]["method"].as_str(), Some("cursor"));
    assert_eq!(data["cursor"]["selector"].as_str(), Some("#save"));
    assert_eq!(data["cursor"], cursor_json);
    assert_eq!(data["target"]["selector"].as_str(), Some("#save"));
    assert_eq!(data["target"]["cursor"], data["cursor"]);
    assert_eq!(data["layout"]["visible"].as_bool(), Some(true));
    assert_eq!(data["context"]["inside_shadow_root"].as_bool(), Some(false));
    assert!(data["sections"].is_null());
}

#[test]
#[ignore]
fn test_inspect_node_handles_shadow_dom() {
    let _guard = common::browser_test_guard();
    let Some(session) = common::launch_or_skip() else {
        return;
    };

    let html = r#"
        <!DOCTYPE html>
        <html>
        <body>
            <div id="host"></div>
            <script>
                const host = document.getElementById('host');
                const root = host.attachShadow({ mode: 'open' });
                root.innerHTML = '<button id="shadow-save" disabled>Shadow Save</button>';
            </script>
        </body>
        </html>
    "#;

    let data_url = format!("data:text/html,{}", html);
    session.navigate(&data_url).expect("Failed to navigate");
    std::thread::sleep(std::time::Duration::from_millis(500));

    let snapshot_tool = SnapshotTool::default();
    let inspect_tool = InspectNodeTool::default();
    let mut context = ToolContext::new(&session);

    let snapshot = snapshot_tool
        .execute_typed(SnapshotParams::default(), &mut context)
        .expect("Failed to execute snapshot tool");
    let snapshot_data = snapshot.data.expect("snapshot should include data");
    let cursor = snapshot_cursor_for_selector(&snapshot_data, "#shadow-save");

    let result = inspect_tool
        .execute_typed(
            InspectNodeParams {
                selector: None,
                index: None,
                node_ref: None,
                cursor: Some(cursor),
                detail: InspectDetail::Full,
                style_names: Vec::new(),
            },
            &mut context,
        )
        .expect("inspect_node should succeed for shadow DOM nodes");

    assert!(result.success);
    let data = result.data.unwrap();
    assert_eq!(data["identity"]["tag"].as_str(), Some("button"));
    assert_eq!(data["accessibility"]["disabled"].as_bool(), Some(true));
    assert_eq!(data["context"]["inside_shadow_root"].as_bool(), Some(true));
    assert!(
        data["sections"]["html"]["value"]
            .as_str()
            .unwrap_or_default()
            .contains("shadow-save")
    );
    assert_eq!(
        data["sections"]["attributes"]["values"]["id"].as_str(),
        Some("shadow-save")
    );
}

#[test]
#[ignore]
fn test_inspect_node_compact_surface_covers_focus_disabled_and_viewport_visibility() {
    let _guard = common::browser_test_guard();
    let Some(session) = common::launch_or_skip() else {
        return;
    };

    let html = r#"
        <!DOCTYPE html>
        <html>
        <body style="margin: 0;">
            <label for="query">Search</label>
            <input id="query" type="text" value="draft" placeholder="Search docs" readonly>
            <button id="disabled-save" disabled style="pointer-events: none;">Save disabled</button>
            <button id="offscreen" style="position: absolute; top: 1600px;">Offscreen action</button>
        </body>
        </html>
    "#;

    let data_url = format!("data:text/html,{}", html);
    session.navigate(&data_url).expect("Failed to navigate");
    std::thread::sleep(std::time::Duration::from_millis(500));
    session
        .tab()
        .expect("tab should exist")
        .evaluate("document.getElementById('query').focus();", false)
        .expect("input should be focusable");

    let inspect_tool = InspectNodeTool::default();
    let mut context = ToolContext::new(&session);

    let focused = inspect_tool
        .execute_typed(
            InspectNodeParams {
                selector: Some("#query".to_string()),
                index: None,
                node_ref: None,
                cursor: None,
                detail: InspectDetail::Compact,
                style_names: Vec::new(),
            },
            &mut context,
        )
        .expect("focused input inspection should succeed");
    assert!(focused.success);
    let focused_data = focused.data.expect("focused input should include data");
    assert_eq!(
        focused_data["accessibility"]["active"].as_bool(),
        Some(true)
    );
    assert_eq!(
        focused_data["accessibility"]["role"].as_str(),
        Some("textbox")
    );
    assert_eq!(focused_data["form_state"]["value"].as_str(), Some("draft"));
    assert_eq!(
        focused_data["form_state"]["placeholder"].as_str(),
        Some("Search docs")
    );
    assert_eq!(focused_data["form_state"]["readonly"].as_bool(), Some(true));
    assert!(focused_data["sections"].is_null());

    let disabled = inspect_tool
        .execute_typed(
            InspectNodeParams {
                selector: Some("#disabled-save".to_string()),
                index: None,
                node_ref: None,
                cursor: None,
                detail: InspectDetail::Compact,
                style_names: Vec::new(),
            },
            &mut context,
        )
        .expect("disabled button inspection should succeed");
    assert!(disabled.success);
    let disabled_data = disabled.data.expect("disabled button should include data");
    assert_eq!(
        disabled_data["accessibility"]["disabled"].as_bool(),
        Some(true)
    );
    assert_eq!(
        disabled_data["form_state"]["disabled"].as_bool(),
        Some(true)
    );
    assert_eq!(
        disabled_data["layout"]["receives_pointer_events"].as_bool(),
        Some(false)
    );
    assert_eq!(
        disabled_data["layout"]["pointer_events"].as_str(),
        Some("none")
    );
    assert!(disabled_data["sections"].is_null());

    let offscreen = inspect_tool
        .execute_typed(
            InspectNodeParams {
                selector: Some("#offscreen".to_string()),
                index: None,
                node_ref: None,
                cursor: None,
                detail: InspectDetail::Compact,
                style_names: Vec::new(),
            },
            &mut context,
        )
        .expect("offscreen button inspection should succeed");
    assert!(offscreen.success);
    let offscreen_data = offscreen
        .data
        .expect("offscreen button should include data");
    assert_eq!(offscreen_data["layout"]["visible"].as_bool(), Some(true));
    assert_eq!(
        offscreen_data["layout"]["visible_in_viewport"].as_bool(),
        Some(false)
    );
    assert!(offscreen_data["sections"].is_null());
}

#[test]
#[ignore]
fn test_inspect_node_selector_normalizes_to_cursor() {
    let _guard = common::browser_test_guard();
    let Some(session) = common::launch_or_skip() else {
        return;
    };

    let html = r#"
        <!DOCTYPE html>
        <html>
        <body>
            <button id="publish">Publish</button>
        </body>
        </html>
    "#;

    let data_url = format!("data:text/html,{}", html);
    session.navigate(&data_url).expect("Failed to navigate");
    std::thread::sleep(std::time::Duration::from_millis(500));

    let inspect_tool = InspectNodeTool::default();
    let mut context = ToolContext::new(&session);

    let result = inspect_tool
        .execute_typed(
            InspectNodeParams {
                selector: Some("#publish".to_string()),
                index: None,
                node_ref: None,
                cursor: None,
                detail: InspectDetail::Compact,
                style_names: Vec::new(),
            },
            &mut context,
        )
        .expect("inspect_node should succeed for actionable selector");

    assert!(result.success);
    let data = result.data.unwrap();
    assert_eq!(data["cursor"]["selector"].as_str(), Some("#publish"));
    assert_eq!(data["target"]["selector"].as_str(), Some("#publish"));
}

#[test]
#[ignore]
fn test_inspect_node_bounds_heavy_fields() {
    let _guard = common::browser_test_guard();
    let Some(session) = common::launch_or_skip() else {
        return;
    };

    let long_text = "x".repeat(3000);
    let html = format!(
        r#"
        <!DOCTYPE html>
        <html>
        <body>
            <button id="heavy" data-extra="{long_text}">{long_text}</button>
        </body>
        </html>
    "#
    );

    let data_url = format!("data:text/html,{}", html);
    session.navigate(&data_url).expect("Failed to navigate");
    std::thread::sleep(std::time::Duration::from_millis(500));

    let inspect_tool = InspectNodeTool::default();
    let mut context = ToolContext::new(&session);

    let result = inspect_tool
        .execute_typed(
            InspectNodeParams {
                selector: Some("#heavy".to_string()),
                index: None,
                node_ref: None,
                cursor: None,
                detail: InspectDetail::Full,
                style_names: vec![
                    "display".to_string(),
                    "visibility".to_string(),
                    "pointer-events".to_string(),
                    "position".to_string(),
                    "z-index".to_string(),
                    "opacity".to_string(),
                    "cursor".to_string(),
                    "overflow".to_string(),
                    "color".to_string(),
                    "background-color".to_string(),
                    "font-size".to_string(),
                    "font-weight".to_string(),
                    "line-height".to_string(),
                ],
            },
            &mut context,
        )
        .expect("inspect_node should succeed for actionable selector");

    assert!(result.success);
    let data = result.data.unwrap();
    assert_eq!(data["sections"]["text"]["truncated"].as_bool(), Some(true));
    assert_eq!(data["sections"]["html"]["truncated"].as_bool(), Some(true));
    assert_eq!(
        data["sections"]["styles"]["total_entries"].as_u64(),
        Some(12)
    );
    assert!(
        data["sections"]["styles"]["values"]
            .as_object()
            .map(|styles| styles.len() <= 12)
            .unwrap_or(false)
    );
}

#[test]
#[ignore]
fn test_inspect_node_reports_cross_origin_iframe_boundary() {
    let _guard = common::browser_test_guard();
    let Some(session) = common::launch_or_skip() else {
        return;
    };

    let html = r#"
        <!DOCTYPE html>
        <html>
        <body>
            <iframe src="data:text/html,%3Cbutton%20id%3D%22inside%22%3EInside%3C%2Fbutton%3E"></iframe>
        </body>
        </html>
    "#;

    let data_url = format!("data:text/html,{}", html);
    session.navigate(&data_url).expect("Failed to navigate");
    std::thread::sleep(std::time::Duration::from_millis(500));

    let inspect_tool = InspectNodeTool::default();
    let mut context = ToolContext::new(&session);

    let result = inspect_tool
        .execute_typed(
            InspectNodeParams {
                selector: Some("#inside".to_string()),
                index: None,
                node_ref: None,
                cursor: None,
                detail: InspectDetail::Compact,
                style_names: Vec::new(),
            },
            &mut context,
        )
        .expect("inspect_node should return a structured failure");

    assert!(!result.success);
    let data = result.data.unwrap();
    assert_eq!(data["code"].as_str(), Some("cross_origin_frame_boundary"));
    assert_eq!(
        data["boundaries"][0]["status"].as_str(),
        Some("cross_origin")
    );
}

#[test]
#[ignore]
fn test_inspect_node_handles_same_origin_iframe() {
    let _guard = common::browser_test_guard();
    let Some(session) = common::launch_or_skip() else {
        return;
    };

    let html = r#"
        <!DOCTYPE html>
        <html>
        <body>
            <iframe srcdoc="<html><body><button id='inside'>Inside</button></body></html>"></iframe>
        </body>
        </html>
    "#;

    let data_url = format!("data:text/html,{}", html);
    session.navigate(&data_url).expect("Failed to navigate");
    std::thread::sleep(std::time::Duration::from_millis(500));

    let inspect_tool = InspectNodeTool::default();
    let mut context = ToolContext::new(&session);

    let result = inspect_tool
        .execute_typed(
            InspectNodeParams {
                selector: Some("#inside".to_string()),
                index: None,
                node_ref: None,
                cursor: None,
                detail: InspectDetail::Compact,
                style_names: Vec::new(),
            },
            &mut context,
        )
        .expect("inspect_node should succeed for same-origin iframe content");

    assert!(result.success);
    let data = result.data.unwrap();
    assert_eq!(data["identity"]["tag"].as_str(), Some("button"));
    assert_eq!(data["cursor"]["selector"].as_str(), Some("#inside"));
    assert_eq!(data["context"]["frame_depth"].as_u64(), Some(1));
}

#[test]
#[ignore]
fn test_inspect_node_reuses_snapshot_cursor_inside_same_origin_iframe() {
    let _guard = common::browser_test_guard();
    let Some(session) = common::launch_or_skip() else {
        return;
    };

    let html = r#"
        <!DOCTYPE html>
        <html>
        <body>
            <iframe srcdoc="<html><body><button id='inside'>Inside</button></body></html>"></iframe>
        </body>
        </html>
    "#;

    let data_url = format!("data:text/html,{}", html);
    session.navigate(&data_url).expect("Failed to navigate");
    std::thread::sleep(std::time::Duration::from_millis(500));

    let snapshot_tool = SnapshotTool::default();
    let inspect_tool = InspectNodeTool::default();
    let mut context = ToolContext::new(&session);

    let snapshot = snapshot_tool
        .execute_typed(SnapshotParams::default(), &mut context)
        .expect("snapshot should succeed");
    let snapshot_data = snapshot.data.expect("snapshot should include data");
    let cursor = snapshot_cursor_for_selector(&snapshot_data, "#inside");
    let cursor_json = serde_json::to_value(&cursor).expect("cursor should serialize");

    let result = inspect_tool
        .execute_typed(
            InspectNodeParams {
                selector: None,
                index: None,
                node_ref: None,
                cursor: Some(cursor),
                detail: InspectDetail::Compact,
                style_names: Vec::new(),
            },
            &mut context,
        )
        .expect("iframe cursor inspection should succeed");

    assert!(result.success);
    let data = result.data.expect("inspect_node should include data");
    assert_eq!(data["target"]["method"].as_str(), Some("cursor"));
    assert_eq!(data["cursor"], cursor_json);
    assert_eq!(data["target"]["cursor"], data["cursor"]);
    assert_eq!(data["cursor"]["selector"].as_str(), Some("#inside"));
    assert_eq!(data["context"]["frame_depth"].as_u64(), Some(1));
    assert_eq!(data["identity"]["tag"].as_str(), Some("button"));
}

mod common;

use chromewright::tools::{
    HoverParams, InspectDetail, InspectNodeParams, ScrollParams, SelectParams, SnapshotParams,
    Tool, ToolContext, WaitCondition, WaitParams, hover::HoverTool,
    inspect_node::InspectNodeTool, scroll::ScrollTool, select::SelectTool,
    snapshot::SnapshotTool, wait::WaitTool,
};
use log::info;

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
                value: "green".to_string(),
            },
            &mut context,
        )
        .expect("Select with node_ref should succeed");

    assert!(result.success);
    let data = result.data.unwrap();
    assert_eq!(data["selectedText"].as_str(), Some("Green"));
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
            <div id="status">Loading</div>
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
    assert!(data["document"]["revision"].as_str().is_some());
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
    let cursor: chromewright::dom::Cursor =
        serde_json::from_value(snapshot.data.unwrap()["nodes"][0]["cursor"].clone())
            .expect("cursor should deserialize");

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
    assert_eq!(data["cursor"]["selector"].as_str(), Some("#save"));
    assert_eq!(data["target"]["selector"].as_str(), Some("#save"));
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
    let cursor: chromewright::dom::Cursor =
        serde_json::from_value(snapshot.data.unwrap()["nodes"][0]["cursor"].clone())
            .expect("cursor should deserialize");

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
    assert_eq!(data["sections"]["styles"]["total_entries"].as_u64(), Some(12));
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

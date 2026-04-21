use browser_use::tools::{
    HoverParams, ScrollParams, SelectParams, SnapshotParams, Tool, ToolContext, WaitCondition,
    WaitParams, hover::HoverTool, scroll::ScrollTool, select::SelectTool, snapshot::SnapshotTool,
    wait::WaitTool,
};
use browser_use::{BrowserSession, LaunchOptions};
use log::info;

fn launch_or_skip() -> Option<BrowserSession> {
    match BrowserSession::launch(LaunchOptions::new().headless(true)) {
        Ok(session) => Some(session),
        Err(err)
            if err
                .to_string()
                .contains("didn't give us a WebSocket URL before we timed out")
                || err
                    .to_string()
                    .contains("Could not auto detect a chrome executable")
                || err
                    .to_string()
                    .contains("Running as root without --no-sandbox is not supported") =>
        {
            eprintln!(
                "Skipping browser integration test due to environment: {}",
                err
            );
            None
        }
        Err(err) => panic!("Unexpected launch failure: {}", err),
    }
}

#[test]
#[ignore] // Requires Chrome to be installed
fn test_select_tool() {
    let session = BrowserSession::launch(LaunchOptions::new().headless(true))
        .expect("Failed to launch browser");

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
    let session = BrowserSession::launch(LaunchOptions::new().headless(true))
        .expect("Failed to launch browser");

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
    let session = BrowserSession::launch(LaunchOptions::new().headless(true))
        .expect("Failed to launch browser");

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
    let session = BrowserSession::launch(LaunchOptions::new().headless(true))
        .expect("Failed to launch browser");

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
    let session = BrowserSession::launch(LaunchOptions::new().headless(true))
        .expect("Failed to launch browser");

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
    let node_ref: browser_use::dom::NodeRef =
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
    let Some(session) = launch_or_skip() else {
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

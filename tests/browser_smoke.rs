mod common;

use serde_json::{Value, json};

fn encoded_html_url(html: &str) -> String {
    format!("data:text/html,{}", urlencoding::encode(html))
}

fn snapshot_cursor_for_selector(snapshot_data: &Value, selector: &str) -> Value {
    snapshot_data["nodes"]
        .as_array()
        .expect("snapshot should return nodes")
        .iter()
        .find(|node| node["cursor"]["selector"].as_str() == Some(selector))
        .unwrap_or_else(|| panic!("expected snapshot cursor for selector {selector}"))["cursor"]
        .clone()
}

#[test]
fn smoke_navigate_tool() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();
    let url = encoded_html_url("<html><body><h1>Smoke Nav</h1></body></html>");

    let result = session
        .execute_tool(
            "navigate",
            json!({
                "url": url,
                "wait_for_load": true,
                "allow_unsafe": true,
            }),
        )
        .expect("navigate should execute");

    assert!(result.success);
    let data = result.data.expect("navigate should include data");
    assert_eq!(data["action"].as_str(), Some("navigate"));
    assert_eq!(data["document"]["ready_state"].as_str(), Some("complete"));
    assert!(
        data["document"]["url"]
            .as_str()
            .unwrap_or_default()
            .contains("Smoke%20Nav")
    );
}

#[test]
fn smoke_snapshot_and_inspect() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    common::navigate_encoded_html(
        session,
        r#"
            <html>
            <body>
                <button id="save">Save</button>
            </body>
            </html>
        "#,
    )
    .expect("failed to navigate");

    let snapshot = session
        .execute_tool("snapshot", json!({}))
        .expect("snapshot should execute");
    assert!(snapshot.success);
    let snapshot_data = snapshot.data.expect("snapshot should include data");
    let cursor = snapshot_cursor_for_selector(&snapshot_data, "#save");

    let inspect = session
        .execute_tool(
            "inspect_node",
            json!({
                "cursor": cursor,
                "detail": "compact",
            }),
        )
        .expect("inspect_node should execute");

    assert!(inspect.success);
    let data = inspect.data.expect("inspect_node should include data");
    assert_eq!(data["action"].as_str(), Some("inspect_node"));
    assert_eq!(data["identity"]["tag"].as_str(), Some("button"));
    assert_eq!(data["target"]["selector"].as_str(), Some("#save"));
}

#[test]
fn smoke_click_and_wait() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    common::navigate_encoded_html(
        session,
        r#"
            <html>
            <body>
                <div id="status">waiting</div>
                <button id="save" onclick="document.getElementById('status').textContent='clicked'">
                    Save
                </button>
            </body>
            </html>
        "#,
    )
    .expect("failed to navigate");

    let click = session
        .execute_tool(
            "click",
            json!({
                "selector": "#save",
            }),
        )
        .expect("click should execute");
    assert!(click.success);

    let wait = session
        .execute_tool(
            "wait",
            json!({
                "selector": "#status",
                "condition": "text_contains",
                "text": "clicked",
                "timeout_ms": 5_000,
            }),
        )
        .expect("wait should execute");
    assert!(wait.success);

    let status = common::evaluate(session, "document.getElementById('status').textContent")
        .expect("status text should be readable");
    assert_eq!(status.as_str(), Some("clicked"));
}

#[test]
fn smoke_get_markdown() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    common::navigate_encoded_html(
        session,
        r#"
            <html>
            <head><title>Smoke Article</title></head>
            <body>
                <main>
                    <h1>Smoke Article</h1>
                    <p>Ship the smoke test.</p>
                </main>
            </body>
            </html>
        "#,
    )
    .expect("failed to navigate");

    let result = session
        .execute_tool("get_markdown", json!({}))
        .expect("get_markdown should execute");

    assert!(result.success);
    let data = result.data.expect("get_markdown should include data");
    let markdown = data["markdown"].as_str().unwrap_or_default();
    assert!(markdown.contains("Smoke Article"));
    assert!(markdown.contains("Ship the smoke test."));
}

#[test]
fn smoke_tab_workflow() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    common::navigate_encoded_html(session, "<html><body><h1>First Tab</h1></body></html>")
        .expect("failed to navigate");

    let second_tab_url = encoded_html_url("<html><body><h1>Second Tab</h1></body></html>");
    let new_tab = session
        .execute_tool(
            "new_tab",
            json!({
                "url": second_tab_url,
                "allow_unsafe": true,
            }),
        )
        .expect("new_tab should execute");
    assert!(new_tab.success);

    let tabs = session
        .execute_tool("tab_list", json!({}))
        .expect("tab_list should execute");
    assert!(tabs.success);
    let tab_data = tabs.data.expect("tab_list should include data");
    assert!(
        tab_data["count"].as_u64().unwrap_or_default() >= 2,
        "expected at least two tabs"
    );

    let switched = session
        .execute_tool("switch_tab", json!({ "index": 0 }))
        .expect("switch_tab should execute");
    assert!(switched.success);

    common::wait_for_url_contains(session, "First%20Tab").expect("first tab should become active");
}

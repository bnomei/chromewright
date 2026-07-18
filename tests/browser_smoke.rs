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
#[cfg(feature = "tui")]
fn smoke_semantic_capture_uses_main_revision_from_browser_metadata() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    common::navigate_encoded_html(
        session,
        r#"
            <html>
            <head><title>Semantic Revision</title></head>
            <body><main><h1 id="title">Semantic Revision</h1></main></body>
            </html>
        "#,
    )
    .expect("failed to navigate");

    let semantic =
        chromewright::extract_semantic_document(session).expect("semantic capture should succeed");
    let metadata = session
        .document_metadata()
        .expect("document metadata should succeed");

    assert_eq!(semantic.document.document_id, metadata.document_id);
    assert_eq!(semantic.document.url, metadata.url);
    assert_eq!(semantic.document.title, metadata.title);
    assert_eq!(
        metadata.revision.split('|').next(),
        Some(semantic.document.revision.as_str()),
        "semantic capture must retain the browser metadata main-frame revision"
    );
}

#[test]
#[cfg(feature = "tui")]
fn smoke_tui_operates_idless_semantic_controls_and_rejects_stale_refs() {
    use chromewright::SemanticKind;
    use chromewright::tui::{PageDriver, SessionPageDriver};

    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();
    common::navigate_encoded_html(
        session,
        r##"
            <html><body>
              <input name="email">
              <select name="plan"><option value="free">Free</option><option value="pro">Pro</option></select>
              <button onclick="window.__tuiClicked = true">Save</button>
              <a href="#done" onclick="window.__tuiLinkClicked = true; event.preventDefault()">Continue</a>
              <div class="widget"></div>
              <script>
                const shadow = document.querySelector('.widget').attachShadow({ mode: 'open' });
                shadow.innerHTML = `
                  <button onclick="window.__tuiShadowFirstClicked = true">Shadow First</button>
                  <button onclick="window.__tuiShadowClicked = true">Shadow Save</button>
                  <input placeholder="Shadow name">
                `;
              </script>
            </body></html>
        "##,
    )
    .expect("failed to navigate");

    let mut driver = SessionPageDriver::new(session);
    let document = driver.capture_semantic().expect("semantic capture");
    let input_ref = document
        .components()
        .find(|component| component.kind == SemanticKind::Input)
        .expect("id-less input")
        .semantic_ref
        .clone();
    driver
        .fill_control(&document, &input_ref, "user@example.com")
        .expect("fill id-less input");
    assert_eq!(
        common::evaluate(session, "document.querySelector('input').value")
            .expect("input value")
            .as_str(),
        Some("user@example.com")
    );

    let document = driver.capture_semantic().expect("capture after input");
    let select_ref = document
        .components()
        .find(|component| component.kind == SemanticKind::Select)
        .expect("id-less select")
        .semantic_ref
        .clone();
    driver
        .fill_control(&document, &select_ref, "pro")
        .expect("select id-less option");
    assert_eq!(
        common::evaluate(session, "document.querySelector('select').value")
            .expect("select value")
            .as_str(),
        Some("pro")
    );

    let document = driver.capture_semantic().expect("capture after select");
    let button_ref = document
        .components()
        .find(|component| component.kind == SemanticKind::Button)
        .expect("id-less button")
        .semantic_ref
        .clone();
    driver
        .activate_ref(&document, &button_ref, false)
        .expect("activate id-less button");
    assert_eq!(
        common::evaluate(session, "window.__tuiClicked === true").expect("button marker"),
        Value::Bool(true)
    );

    let document = driver.capture_semantic().expect("capture after button");
    let shadow_button_ref = document
        .components()
        .find(|component| {
            component.kind == SemanticKind::Button
                && component
                    .text
                    .as_deref()
                    .or(component.label.as_deref())
                    .is_some_and(|text| text.contains("Shadow Save"))
        })
        .expect("id-less shadow button")
        .semantic_ref
        .clone();
    driver
        .activate_ref(&document, &shadow_button_ref, false)
        .expect("activate id-less shadow button");
    assert_eq!(
        common::evaluate(session, "window.__tuiShadowClicked === true")
            .expect("shadow button marker"),
        Value::Bool(true)
    );

    let document = driver
        .capture_semantic()
        .expect("capture after shadow click");
    let shadow_input_ref = document
        .components()
        .find(|component| {
            component.kind == SemanticKind::Input
                && component
                    .attrs
                    .placeholder
                    .as_deref()
                    .is_some_and(|text| text == "Shadow name")
        })
        .expect("id-less shadow input")
        .semantic_ref
        .clone();
    driver
        .activate_ref(&document, &shadow_input_ref, false)
        .expect("focus id-less shadow input");
    assert_eq!(
        common::evaluate(
            session,
            "document.querySelector('.widget').shadowRoot.activeElement.placeholder"
        )
        .expect("shadow focus marker")
        .as_str(),
        Some("Shadow name")
    );

    let stale_shadow_document = driver
        .capture_semantic()
        .expect("capture before shadow mutation");
    let stale_shadow_ref = stale_shadow_document
        .components()
        .find(|component| {
            component.kind == SemanticKind::Button
                && component
                    .text
                    .as_deref()
                    .or(component.label.as_deref())
                    .is_some_and(|text| text.contains("Shadow Save"))
        })
        .expect("shadow button before mutation")
        .semantic_ref
        .clone();
    common::evaluate(
        session,
        "document.querySelector('.widget').shadowRoot.children[1].replaceWith(document.createElement('button'))",
    )
    .expect("mutate open shadow root");
    let stale_shadow_error = driver
        .activate_ref(&stale_shadow_document, &stale_shadow_ref, false)
        .expect_err("stale shadow interaction must fail");
    assert!(
        stale_shadow_error.to_string().contains("stale"),
        "{stale_shadow_error}"
    );

    let stale_document = driver.capture_semantic().expect("capture before mutation");
    let stale_button_ref = stale_document
        .components()
        .find(|component| component.kind == SemanticKind::Button)
        .expect("button before mutation")
        .semantic_ref
        .clone();
    common::evaluate(
        session,
        "document.body.appendChild(document.createElement('div'))",
    )
    .expect("mutate document");
    let stale_error = driver
        .activate_ref(&stale_document, &stale_button_ref, false)
        .expect_err("stale semantic interaction must fail");
    assert!(stale_error.to_string().contains("stale"), "{stale_error}");

    let document = driver.capture_semantic().expect("capture after mutation");
    let link_ref = document
        .components()
        .find(|component| component.kind == SemanticKind::Link)
        .expect("id-less link")
        .semantic_ref
        .clone();
    driver
        .activate_ref(&document, &link_ref, false)
        .expect("follow id-less link");
    assert_eq!(
        common::evaluate(session, "window.__tuiLinkClicked === true").expect("link marker"),
        Value::Bool(true)
    );
}

#[test]
#[cfg(feature = "tui")]
fn smoke_tui_controller_publishes_loading_then_atomic_ready_on_reload() {
    use chromewright::tui::{Controller, Lifecycle, PageDriver, SessionPageDriver, SharedTuiState};

    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    common::navigate_encoded_html(
        browser.session(),
        r#"<html><head><title>TUI lifecycle</title></head><body>
            <main>ready</main>
            <form onsubmit="event.preventDefault(); document.querySelector('output').textContent = this.elements[0].value">
              <input name="message"><button>Submit</button>
            </form>
            <output></output>
        </body></html>"#,
    )
    .expect("failed to navigate");
    let (_guard, session) = browser.into_shared();
    let shared = SharedTuiState::new(session.clone());
    let mut controller = Controller::with_shared(shared.clone());
    let mut driver = SessionPageDriver::new(&session);

    controller.bootstrap();
    assert!(matches!(
        controller.state.lifecycle,
        Lifecycle::Loading { .. }
    ));
    assert!(matches!(shared.lifecycle(), Lifecycle::Loading { .. }));
    controller.acknowledge_loading_frame();
    controller
        .perform_pending_page_action(&mut driver)
        .expect("initial semantic capture");
    assert!(matches!(controller.state.lifecycle, Lifecycle::Ready));

    let input_ref = controller
        .state
        .document()
        .expect("published semantic document")
        .components()
        .find(|component| component.kind == chromewright::SemanticKind::Input)
        .expect("form input")
        .semantic_ref
        .clone();
    controller
        .submit_form_input(&input_ref, "submitted through controller")
        .expect("queue form submission");
    assert!(matches!(
        controller.state.lifecycle,
        Lifecycle::Loading { .. }
    ));
    assert!(matches!(shared.lifecycle(), Lifecycle::Loading { .. }));
    controller.acknowledge_loading_frame();
    controller
        .perform_pending_page_action(&mut driver)
        .expect("submit form and recapture");
    assert!(matches!(controller.state.lifecycle, Lifecycle::Ready));
    assert_eq!(
        common::evaluate(&session, "document.querySelector('output').textContent")
            .expect("submitted form marker")
            .as_str(),
        Some("submitted through controller")
    );

    let before = controller.state.revision().to_string();
    controller.reload();
    assert!(matches!(
        controller.state.lifecycle,
        Lifecycle::Loading { .. }
    ));
    assert_eq!(
        controller.state.revision(),
        before,
        "Loading must retain the last complete page"
    );
    controller.acknowledge_loading_frame();
    controller
        .perform_pending_page_action(&mut driver)
        .expect("reload and recapture");

    assert!(matches!(controller.state.lifecycle, Lifecycle::Ready));
    let page = controller.state.page.as_ref().expect("published page");
    assert_eq!(page.revision, page.document.document.revision);
    assert_eq!(page.url, page.document.document.url);
    assert_eq!(page.title, page.document.document.title);
    assert_eq!(
        shared
            .active()
            .expect("shared published page")
            .document
            .revision,
        page.revision
    );
    // Exercise the trait import in this live path and prove the published
    // document remains immediately capturable from the same browser session.
    assert_eq!(
        driver
            .capture_semantic()
            .expect("post-ready capture")
            .document
            .document_id,
        page.document.document.document_id
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
                "target": {
                    "kind": "cursor",
                    "cursor": cursor,
                },
                "detail": "compact",
            }),
        )
        .expect("inspect_node should execute");

    assert!(
        inspect.success,
        "inspect_node failed: error={:?}, data={:?}",
        inspect.error, inspect.data
    );
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
                "target": "#save",
            }),
        )
        .expect("click should execute");
    assert!(
        click.success,
        "click failed: error={:?}, data={:?}",
        click.error, click.data
    );

    let wait = session
        .execute_tool(
            "wait",
            json!({
                "target": "#status",
                "condition": "text_contains",
                "text": "clicked",
                "timeout_ms": 5_000,
            }),
        )
        .expect("wait should execute");
    assert!(
        wait.success,
        "wait failed: error={:?}, data={:?}",
        wait.error, wait.data
    );

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
    let tabs = tab_data["tabs"]
        .as_array()
        .expect("tab_list should include tabs");
    let first_tab = tabs
        .iter()
        .find(|tab| {
            tab["url"]
                .as_str()
                .is_some_and(|url| url.contains("First%20Tab"))
        })
        .expect("tab_list should include the first tab");
    let first_tab_id = first_tab["tab_id"]
        .as_str()
        .expect("tab_list should expose stable tab ids")
        .to_string();

    let switched = session
        .execute_tool("switch_tab", json!({ "tab_id": first_tab_id }))
        .expect("switch_tab should execute");
    assert!(switched.success);
    let switched_data = switched.data.expect("switch_tab should include data");
    assert_eq!(
        switched_data["tab"]["tab_id"].as_str(),
        Some(first_tab_id.as_str())
    );
    assert_eq!(
        switched_data["active_tab"]["tab_id"].as_str(),
        Some(first_tab_id.as_str())
    );

    common::wait_for_url_contains(session, "First%20Tab").expect("first tab should become active");

    let close_tab = session
        .execute_tool("close_tab", json!({}))
        .expect("close_tab should execute in launched mode");
    assert!(close_tab.success);
    let close_tab_data = close_tab.data.expect("close_tab should include data");
    assert_eq!(
        close_tab_data["closed_tab"]["tab_id"].as_str(),
        Some(first_tab_id.as_str())
    );

    let close = session
        .execute_tool("close", json!({}))
        .expect("close should execute in launched mode");
    assert!(close.success);
    let close_data = close.data.expect("close should include data");
    assert_eq!(close_data["scope"].as_str(), Some("all_tabs"));
    assert_eq!(close_data["session_origin"].as_str(), Some("launched"));
}

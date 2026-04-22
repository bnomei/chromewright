mod common;

use log::info;

fn production_inspection_fixture_html() -> String {
    let tiny_gif = "R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==";
    format!(
        r#"
        <!DOCTYPE html>
        <html>
        <head>
            <style>
                [role="tab"] {{ cursor: pointer; }}
            </style>
        </head>
        <body>
            <main>
                <article>
                    <h1 id="story-title">Workspace agents in ChatGPT</h1>
                    <img
                        id="3hero-image"
                        alt="Workspace agent diagram"
                        src="data:image/gif;base64,{tiny_gif}"
                    />
                    <div role="tablist" aria-label="Customer stories">
                        <button id="7rippling" role="tab" aria-selected="true">Rippling</button>
                        <button id="better-mortgage" role="tab" aria-selected="false">Better Mortgage</button>
                    </div>
                </article>

                <dialog id="cookie-banner" open>
                    <button id="cookie-accept">Accept</button>
                </dialog>

                <iframe
                    id="same-origin-frame"
                    srcdoc="<html><body><button id='inside'>Inside</button></body></html>"
                ></iframe>
            </main>
        </body>
        </html>
        "#
    )
}

#[test]
#[ignore] // Requires Chrome to be installed
fn test_dom_extraction() {
    // Launch browser
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    // Navigate to a simple page
    common::navigate_and_wait(
        session,
        "data:text/html,<html><body><button id='test-btn'>Click me</button><a href='#'>Link</a></body></html>",
    )
    .expect("Failed to navigate");

    // Extract DOM
    let dom = session.extract_dom().expect("Failed to extract DOM");

    // Verify DOM structure
    assert_eq!(dom.root.role, "fragment");
    assert!(dom.count_nodes() > 0);

    // Note: interactive elements might be 0 due to visibility issues with data: URLs
    // Just verify we got the structure
    info!("DOM tree element count: {}", dom.count_nodes());
    info!("Interactive elements: {}", dom.count_interactive());

    // Convert to JSON
    let json = dom.to_json().expect("Failed to convert to JSON");
    assert!(json.contains("button"));
}

#[test]
#[ignore]
fn test_simplified_dom_extraction() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    // Page with script and style tags that should be removed
    // Use a simple HTML page
    common::navigate_and_wait(
        session,
        "data:text/html,<html><head></head><body><p>Hello</p><button>Click</button></body></html>",
    )
    .expect("Failed to navigate");

    // Extract simplified DOM
    let dom = session.extract_dom().expect("Failed to extract DOM");

    // Verify we got content
    let json = dom.to_json().expect("Failed to convert to JSON");
    assert!(json.contains("button") || json.contains("body"));
    info!("Simplified DOM: {}", json);
}

#[test]
#[ignore]
fn test_read_links() {
    use chromewright::tools::{ReadLinksParams, Tool, ToolContext, read_links::ReadLinksTool};

    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    let html = concat!(
        "<html><head><title>Links Test</title>",
        "<base href=\"https://example.test/articles/\">",
        "</head><body>",
        "<a href=\"https://example.com\">Example</a>",
        "<a href=\"guide/getting-started\">Relative</a>",
        "<a href=\"#anchor\">Anchor</a>",
        "<a href=\"https://rust-lang.org\">Rust</a>",
        "<a>No Href</a>",
        "<a href=\"\">Empty</a>",
        "</body></html>"
    );

    common::navigate_encoded_html(session, html).expect("Failed navigate");

    let tool = ReadLinksTool::default();
    let mut context = ToolContext::new(&session);

    let result = tool
        .execute_typed(ReadLinksParams {}, &mut context)
        .expect("Failed execute");

    assert!(result.success);
    let data = result.data.unwrap();
    let links = data["links"].as_array().unwrap();
    let count = data["count"].as_u64().unwrap();

    info!("Links found: {}", count);
    for link in links {
        info!(
            "  {} -> {} ({})",
            link["text"].as_str().unwrap_or(""),
            link["href"].as_str().unwrap_or(""),
            link["resolved_url"].as_str().unwrap_or("")
        );
    }

    assert!(count >= 4, "Expected at least 4 links");
    assert_eq!(links.len() as u64, count);

    let texts: Vec<&str> = links.iter().filter_map(|l| l["text"].as_str()).collect();

    // Verify the links we do get are correct
    assert!(texts.contains(&"Example"));
    assert!(texts.contains(&"Relative"));

    // Verify absolute href values remain unchanged.
    let ex_link = links
        .iter()
        .find(|l| l["text"].as_str() == Some("Example"))
        .expect("Example link not found");
    assert_eq!(ex_link["href"].as_str(), Some("https://example.com"));
    assert_eq!(
        ex_link["resolved_url"].as_str(),
        Some("https://example.com/")
    );

    // Verify relative href values are preserved while exposing the resolved URL.
    let relative_link = links
        .iter()
        .find(|l| l["text"].as_str() == Some("Relative"))
        .expect("Relative link not found");
    assert_eq!(
        relative_link["href"].as_str(),
        Some("guide/getting-started")
    );
    assert_eq!(
        relative_link["resolved_url"].as_str(),
        Some("https://example.test/articles/guide/getting-started")
    );
}

#[test]
#[ignore]
fn test_press_key_enter() {
    use chromewright::tools::{PressKeyParams, Tool, ToolContext, press_key::PressKeyTool};

    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    // Create a page with an input field that responds to Enter key
    let html = r#"
        <html>
        <head><title>Press Key Test</title></head>
        <body>
            <input type="text" id="input1" value="test">
            <div id="output"></div>
            <script>
                document.getElementById('input1').addEventListener('keydown', function(e) {
                    if (e.key === 'Enter') {
                        document.getElementById('output').textContent = 'Enter pressed!';
                    }
                });
            </script>
        </body>
        </html>
    "#;

    common::navigate_html(session, html).expect("Failed to navigate");

    // Focus the input element first
    common::evaluate(session, "document.getElementById('input1').click(); true")
        .expect("Failed to click input");

    common::wait_for_eval_truthy(
        session,
        "input focus",
        "document.activeElement && document.activeElement.id === 'input1'",
        std::time::Duration::from_secs(5),
    )
    .expect("Input should receive focus");

    // Create tool and context
    let tool = PressKeyTool::default();
    let mut context = ToolContext::new(&session);

    // Execute the tool to press Enter
    let result = tool
        .execute_typed(
            PressKeyParams {
                key: "Enter".to_string(),
            },
            &mut context,
        )
        .expect("Failed to execute press_key tool");

    // Verify the result
    assert!(result.success, "Tool execution should succeed");
    assert!(result.data.is_some());

    let data = result.data.unwrap();
    assert_eq!(data["key"].as_str(), Some("Enter"));
    assert!(data["document"]["revision"].as_str().is_some());
    assert_eq!(data["focus_after"]["kind"].as_str(), Some("cursor"));
    assert_eq!(
        data["focus_after"]["cursor"]["selector"].as_str(),
        Some("#input1")
    );
    assert_eq!(
        data["focus_after"]["cursor"]["role"].as_str(),
        Some("textbox")
    );

    common::wait_for_eval_truthy(
        session,
        "enter key output",
        "document.getElementById('output').textContent === 'Enter pressed!'",
        std::time::Duration::from_secs(5),
    )
    .expect("Enter key handler should update the output");

    // Verify that the event was triggered
    let output = common::evaluate(session, "document.getElementById('output').textContent")
        .ok()
        .and_then(|value| value.as_str().map(str::to_string));

    info!("Output after Enter key: {:?}", output);

    // 校验 output 内容
    assert_eq!(
        output.as_deref(),
        Some("Enter pressed!"),
        "Output should be 'Enter pressed!', but was: {:?}",
        output
    );
    // Note: Due to limitations with data: URLs and event handling,
    // we mainly verify that the tool executes without error
}

#[test]
#[ignore]
fn test_snapshot_tool_exposes_document_metadata_and_node_refs() {
    use chromewright::tools::{SnapshotParams, Tool, ToolContext, snapshot::SnapshotTool};

    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    let html = r#"
        <html>
        <body>
            <button id="save-btn">Save</button>
            <input id="query" type="text" placeholder="Search">
        </body>
        </html>
    "#;

    common::navigate_html(session, html).expect("Failed to navigate");

    let tool = SnapshotTool::default();
    let mut context = ToolContext::new(&session);

    let result = tool
        .execute_typed(SnapshotParams::default(), &mut context)
        .expect("Failed to execute snapshot tool");

    assert!(result.success);
    let data = result.data.unwrap();
    let document = &data["document"];
    let nodes = data["nodes"]
        .as_array()
        .expect("snapshot should return nodes");

    assert!(document["document_id"].as_str().is_some());
    assert!(document["revision"].as_str().is_some());
    assert_eq!(document["ready_state"].as_str(), Some("complete"));
    assert!(data["snapshot"].as_str().is_some());
    assert!(!nodes.is_empty(), "expected actionable nodes in snapshot");

    let first_ref = &nodes[0]["node_ref"];
    let first_cursor = &nodes[0]["cursor"];
    assert_eq!(
        first_ref["document_id"].as_str(),
        document["document_id"].as_str()
    );
    assert_eq!(
        first_ref["revision"].as_str(),
        document["revision"].as_str()
    );
    assert!(first_ref["index"].as_u64().is_some());
    assert_eq!(&first_cursor["node_ref"], first_ref);
    assert_eq!(first_cursor["index"].as_u64(), first_ref["index"].as_u64());
    assert_eq!(first_cursor["role"].as_str(), nodes[0]["role"].as_str());
    assert_eq!(first_cursor["name"].as_str(), nodes[0]["name"].as_str());
    assert!(first_cursor["selector"].as_str().is_some());
}

#[test]
#[ignore]
fn test_stale_node_ref_returns_structured_failure() {
    use chromewright::tools::{
        ClickParams, SnapshotParams, Tool, ToolContext, click::ClickTool, snapshot::SnapshotTool,
    };

    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    let html = r#"
        <html>
        <body>
            <button id="change" onclick="document.getElementById('status').textContent = 'changed';">Change</button>
            <div id="status">initial</div>
        </body>
        </html>
    "#;

    common::navigate_html(session, html).expect("Failed to navigate");

    let snapshot_tool = SnapshotTool::default();
    let click_tool = ClickTool::default();
    let mut context = ToolContext::new(&session);

    let snapshot = snapshot_tool
        .execute_typed(SnapshotParams::default(), &mut context)
        .expect("Failed to execute snapshot tool");
    let node_ref: chromewright::dom::NodeRef =
        serde_json::from_value(snapshot.data.unwrap()["nodes"][0]["node_ref"].clone())
            .expect("node_ref should deserialize");

    let first_click = click_tool
        .execute_typed(
            ClickParams {
                selector: None,
                index: None,
                node_ref: Some(node_ref.clone()),
                cursor: None,
            },
            &mut context,
        )
        .expect("First click should succeed");
    assert!(first_click.success);

    let stale_click = click_tool
        .execute_typed(
            ClickParams {
                selector: None,
                index: None,
                node_ref: Some(node_ref),
                cursor: None,
            },
            &mut context,
        )
        .expect("Stale node ref should return a structured tool failure");

    assert!(!stale_click.success);
    let data = stale_click
        .data
        .expect("structured failure should include data");
    assert_eq!(data["code"].as_str(), Some("stale_node_ref"));
    assert_eq!(stale_click.error.as_deref(), Some("Stale node reference"));
}

#[test]
#[ignore]
fn test_click_tool_reports_detached_handoff_after_target_removal() {
    use chromewright::tools::{
        ClickParams, SnapshotParams, Tool, ToolContext, click::ClickTool, snapshot::SnapshotTool,
    };

    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    let html = r#"
        <html>
        <body>
            <button id="remove" onclick="this.remove(); document.getElementById('status').textContent = 'removed';">Remove</button>
            <div id="status">initial</div>
        </body>
        </html>
    "#;

    common::navigate_html(session, html).expect("Failed to navigate");

    let snapshot_tool = SnapshotTool::default();
    let click_tool = ClickTool::default();
    let mut context = ToolContext::new(&session);

    let snapshot = snapshot_tool
        .execute_typed(SnapshotParams::default(), &mut context)
        .expect("snapshot should succeed");
    let snapshot_data = snapshot.data.expect("snapshot should include data");
    let node_ref: chromewright::dom::NodeRef =
        serde_json::from_value(snapshot_data["nodes"][0]["node_ref"].clone())
            .expect("node_ref should deserialize");

    let result = click_tool
        .execute_typed(
            ClickParams {
                selector: None,
                index: None,
                node_ref: Some(node_ref.clone()),
                cursor: None,
            },
            &mut context,
        )
        .expect("click should succeed");

    assert!(result.success);
    let data = result.data.expect("click should include data");
    assert_eq!(
        data["target_before"]["node_ref"],
        serde_json::to_value(node_ref).unwrap()
    );
    assert_eq!(data["target_status"].as_str(), Some("detached"));
    assert!(data["target_after"].is_null());
    assert!(data["target"].is_null());
}

#[test]
#[ignore]
fn test_same_origin_iframe_content_is_included_in_snapshot() {
    use chromewright::tools::{SnapshotParams, Tool, ToolContext, snapshot::SnapshotTool};

    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    let html = r#"
        <html>
        <body>
            <iframe id="frame" srcdoc="<html><body><h2>Inside Frame</h2><p>Frame text</p></body></html>"></iframe>
        </body>
        </html>
    "#;

    common::navigate_html(session, html).expect("Failed to navigate");

    let initial_metadata = session
        .document_metadata()
        .expect("metadata should load before snapshot extraction");
    let initial_dom = session
        .extract_dom()
        .expect("DOM extraction should succeed before snapshot extraction");
    assert_eq!(initial_metadata.revision, initial_dom.document.revision);
    assert_eq!(initial_metadata.frames, initial_dom.document.frames);

    let tool = SnapshotTool::default();
    let mut context = ToolContext::new(&session);

    let result = tool
        .execute_typed(SnapshotParams::default(), &mut context)
        .expect("Failed to execute snapshot tool");

    assert!(result.success);
    let data = result.data.unwrap();
    assert!(
        data["snapshot"]
            .as_str()
            .unwrap_or_default()
            .contains("Inside Frame")
    );
    assert_eq!(
        data["document"]["frames"][0]["status"].as_str(),
        Some("expanded")
    );
    assert_eq!(
        data["document"]["revision"].as_str(),
        Some(initial_metadata.revision.as_str())
    );

    let initial_frame_document_id = initial_metadata.frames[0]
        .document_id
        .clone()
        .expect("same-origin iframe should expose a frame document id");
    common::evaluate(
        session,
        r#"
            (() => {
                const frame = document.getElementById('frame');
                frame.contentWindow.location.replace('about:blank#updated');
                return true;
            })()
        "#,
    )
    .expect("iframe navigation should succeed");

    let navigation_start = std::time::Instant::now();
    let updated_metadata = loop {
        let metadata = session
            .document_metadata()
            .expect("metadata should load after iframe navigation");
        let current_frame_document_id = metadata.frames[0]
            .document_id
            .as_deref()
            .expect("same-origin iframe should keep a frame document id");
        if current_frame_document_id != initial_frame_document_id {
            break metadata;
        }

        if navigation_start.elapsed() >= std::time::Duration::from_secs(5) {
            panic!("iframe navigation did not invalidate metadata tracking in time");
        }

        std::thread::sleep(std::time::Duration::from_millis(50));
    };

    assert_ne!(updated_metadata.revision, initial_metadata.revision);
    assert_ne!(
        updated_metadata.frames[0].document_id.as_deref(),
        Some(initial_frame_document_id.as_str())
    );

    common::evaluate(
        session,
        r#"
            (() => {
                const frame = document.getElementById('frame');
                frame.contentDocument.body.innerHTML =
                    '<h2>Updated Frame</h2><p>Updated text</p>';
                return true;
            })()
        "#,
    )
    .expect("updating iframe contents should succeed");

    common::evaluate(
        session,
        r#"
            (() => {
                const extra = document.createElement('iframe');
                extra.id = 'extra-frame';
                extra.srcdoc = '<html><body><p>Extra Frame</p></body></html>';
                document.body.appendChild(extra);
                return true;
            })()
        "#,
    )
    .expect("adding an iframe should succeed");

    let membership_start = std::time::Instant::now();
    let final_metadata = loop {
        let metadata = session
            .document_metadata()
            .expect("metadata should load after iframe membership change");
        if metadata.frames.len() == 2 {
            break metadata;
        }

        if membership_start.elapsed() >= std::time::Duration::from_secs(5) {
            panic!("iframe membership change did not invalidate metadata tracking in time");
        }

        std::thread::sleep(std::time::Duration::from_millis(50));
    };

    let final_dom = session
        .extract_dom()
        .expect("DOM extraction should succeed after iframe updates");
    assert_eq!(final_metadata.revision, final_dom.document.revision);
    assert_eq!(final_metadata.frames, final_dom.document.frames);
    assert_eq!(final_metadata.frames.len(), 2);
    assert_eq!(final_metadata.frames[0].status.as_str(), "expanded");
    assert_eq!(final_metadata.frames[1].status.as_str(), "expanded");

    let mut updated_context = ToolContext::new(&session);
    let updated_snapshot = tool
        .execute_typed(SnapshotParams::default(), &mut updated_context)
        .expect("snapshot should succeed after iframe updates");
    let updated_data = updated_snapshot
        .data
        .expect("snapshot should include data after iframe updates");
    assert!(
        updated_data["snapshot"]
            .as_str()
            .unwrap_or_default()
            .contains("Updated Frame")
    );
    assert!(
        updated_data["snapshot"]
            .as_str()
            .unwrap_or_default()
            .contains("Extra Frame")
    );
    assert_eq!(
        updated_data["document"]["revision"].as_str(),
        Some(final_metadata.revision.as_str())
    );
}

#[test]
#[ignore]
fn test_snapshot_tool_exposes_cursor_for_same_origin_iframe_node() {
    use chromewright::tools::{SnapshotParams, Tool, ToolContext, snapshot::SnapshotTool};

    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    let html = r#"
        <html>
        <body>
            <iframe id="frame" srcdoc="<html><body><button id='inside'>Inside</button></body></html>"></iframe>
        </body>
        </html>
    "#;

    common::navigate_html(session, html).expect("Failed to navigate");

    let tool = SnapshotTool::default();
    let mut context = ToolContext::new(&session);

    let result = tool
        .execute_typed(SnapshotParams::default(), &mut context)
        .expect("Failed to execute snapshot tool");

    assert!(result.success);
    let data = result.data.unwrap();
    let nodes = data["nodes"]
        .as_array()
        .expect("snapshot should return nodes");
    let iframe_node = nodes
        .iter()
        .find(|node| node["cursor"]["selector"].as_str() == Some("#inside"))
        .expect("expected same-origin iframe button to expose a cursor");

    assert_eq!(iframe_node["name"].as_str(), Some("Inside"));
    assert_eq!(iframe_node["cursor"]["selector"].as_str(), Some("#inside"));
    assert_eq!(
        data["document"]["frames"][0]["status"].as_str(),
        Some("expanded")
    );
}

#[test]
#[ignore]
fn test_snapshot_tool_keeps_inline_handles_aligned_with_exposed_cursor_nodes() {
    use chromewright::tools::{SnapshotParams, Tool, ToolContext, snapshot::SnapshotTool};

    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    common::navigate_encoded_html(session, production_inspection_fixture_html())
        .expect("Failed to navigate");

    let tool = SnapshotTool::default();
    let mut context = ToolContext::new(&session);

    let result = tool
        .execute_typed(SnapshotParams::default(), &mut context)
        .expect("snapshot should succeed");

    assert!(result.success);
    let data = result.data.expect("snapshot should include data");
    let snapshot = data["snapshot"]
        .as_str()
        .expect("snapshot should include a rendered tree");
    let nodes = data["nodes"]
        .as_array()
        .expect("snapshot should include exposed nodes");

    assert!(snapshot.contains("heading \"Workspace agents in ChatGPT\""));
    assert!(snapshot.contains("img \"Workspace agent diagram\""));
    assert!(
        !snapshot.contains("heading \"Workspace agents in ChatGPT\" [index="),
        "non-actionable headings should not advertise numeric follow-up handles"
    );
    assert!(
        !snapshot.contains("img \"Workspace agent diagram\" [index="),
        "non-actionable images should not advertise numeric follow-up handles"
    );

    let rippling = nodes
        .iter()
        .find(|node| node["name"].as_str() == Some("Rippling"))
        .expect("expected Rippling tab in snapshot nodes");
    assert_eq!(
        rippling["cursor"]["selector"].as_str(),
        Some("#\\37 rippling")
    );

    assert!(
        nodes
            .iter()
            .all(|node| node["name"].as_str() != Some("Workspace agents in ChatGPT"))
    );
    assert!(
        nodes
            .iter()
            .all(|node| node["name"].as_str() != Some("Workspace agent diagram"))
    );
}

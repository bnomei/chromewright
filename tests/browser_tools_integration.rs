mod common;

use chromewright::BrowserSession;
use chromewright::dom::{Cursor, NodeRef};
use chromewright::tools::{
    ClickParams, HoverParams, InputParams, InspectDetail, InspectNodeParams, ScrollParams,
    SelectParams, SnapshotParams, Tool, ToolContext, WaitCondition, WaitParams, click::ClickTool,
    hover::HoverTool, input::InputTool, inspect_node::InspectNodeTool, scroll::ScrollTool,
    select::SelectTool, snapshot::SnapshotTool, wait::WaitTool,
};
use log::info;
use serde_json::{Value, json};

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

fn snapshot_node_by_name<'a>(snapshot_data: &'a Value, name: &str) -> &'a Value {
    snapshot_data["nodes"]
        .as_array()
        .expect("snapshot should return nodes")
        .iter()
        .find(|node| node["name"].as_str() == Some(name))
        .unwrap_or_else(|| panic!("expected snapshot node named {name}"))
}

fn production_inspection_fixture_html() -> String {
    let tiny_gif = "R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==";
    format!(
        r#"
        <!DOCTYPE html>
        <html>
        <head>
            <style>
                body {{ font-family: sans-serif; margin: 0; padding: 24px; }}
                article {{ max-width: 720px; }}
                img.hero {{ display: block; width: 320px; height: 180px; }}
                [role="tab"] {{ cursor: pointer; }}
                dialog[open] {{ position: fixed; right: 16px; bottom: 16px; }}
            </style>
        </head>
        <body>
            <main>
                <article>
                    <h1 id="story-title">Workspace agents in ChatGPT</h1>
                    <p>Production-style fixture for inspection reliability.</p>
                    <img
                        id="3hero-image"
                        class="hero visual"
                        alt="Workspace agent diagram"
                        src="data:image/gif;base64,{tiny_gif}"
                    />
                    <div role="tablist" aria-label="Customer stories">
                        <button id="7rippling" role="tab" aria-selected="true">Rippling</button>
                        <button id="better-mortgage" role="tab" aria-selected="false">Better Mortgage</button>
                        <button id="softbank-corp" role="tab" aria-selected="false">SoftBank Corp.</button>
                    </div>
                    <section aria-labelledby="7rippling">
                        <h2 id="3compliance-api">Compliance API</h2>
                        <p>Selected panel details.</p>
                    </section>
                </article>

                <dialog id="cookie-banner" open>
                    <form method="dialog">
                        <p>Cookies</p>
                        <button id="cookie-manage">Manage</button>
                        <button id="cookie-dismiss">Dismiss</button>
                        <button id="cookie-accept">Accept</button>
                    </form>
                </dialog>

                <iframe
                    id="same-origin-frame"
                    srcdoc="<html><body><button id='inside'>Inside</button></body></html>"
                ></iframe>
                <iframe
                    id="cross-origin-frame"
                    src="data:text/html,%3Cbutton%20id%3D%22outside%22%3EOutside%3C%2Fbutton%3E"
                ></iframe>
            </main>
        </body>
        </html>
        "#
    )
}

fn execute_screenshot(session: &BrowserSession, params: Value) -> Value {
    let result = session
        .execute_tool("screenshot", params)
        .expect("screenshot should execute");
    assert!(
        result.success,
        "screenshot should succeed: {:?}",
        result.error
    );
    result.data.expect("screenshot should include data")
}

fn execute_screenshot_failure(session: &BrowserSession, params: Value) -> Value {
    let result = session
        .execute_tool("screenshot", params)
        .expect("screenshot should execute");
    assert!(
        !result.success,
        "screenshot should fail with structured output"
    );
    result.data.expect("failed screenshot should include data")
}

fn remove_artifact(path: &std::path::Path) {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => panic!("failed to remove artifact {}: {}", path.display(), err),
    }
}

fn clip_rect(data: &Value) -> (f64, f64, f64, f64) {
    assert_eq!(
        data["clip"]["coordinate_space"].as_str(),
        Some("viewport_css_pixels")
    );

    (
        data["clip"]["x"]
            .as_f64()
            .expect("clip x should be returned"),
        data["clip"]["y"]
            .as_f64()
            .expect("clip y should be returned"),
        data["clip"]["width"]
            .as_f64()
            .expect("clip width should be returned"),
        data["clip"]["height"]
            .as_f64()
            .expect("clip height should be returned"),
    )
}

fn assert_screenshot_scale_metadata(
    data: &Value,
    expected_scale: &str,
    css_width: f64,
    css_height: f64,
    device_pixel_ratio: f64,
    pixel_scale: f64,
    tolerance: f64,
) {
    assert_eq!(data["scale"].as_str(), Some(expected_scale));
    assert_close(
        data["css_width"]
            .as_f64()
            .expect("css_width should be returned"),
        css_width,
        tolerance,
        "css width",
    );
    assert_close(
        data["css_height"]
            .as_f64()
            .expect("css_height should be returned"),
        css_height,
        tolerance,
        "css height",
    );
    assert_close(
        data["device_pixel_ratio"]
            .as_f64()
            .expect("device_pixel_ratio should be returned"),
        device_pixel_ratio,
        0.05,
        "device pixel ratio",
    );
    assert_close(
        data["pixel_scale"]
            .as_f64()
            .expect("pixel_scale should be returned"),
        pixel_scale,
        0.05,
        "pixel scale",
    );
}

fn assert_close(actual: f64, expected: f64, tolerance: f64, label: &str) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{label} expected {expected} +/- {tolerance}, got {actual}"
    );
}

fn rect_for_selector(session: &BrowserSession, selector: &str) -> (f64, f64, f64, f64) {
    let selector_json = serde_json::to_string(selector).expect("selector should serialize");
    let value = common::evaluate(
        session,
        &format!(
            r#"(() => {{
                const selector = {selector_json};
                let element = null;
                try {{
                    element = document.querySelector(selector);
                }} catch (_error) {{
                    if (selector.startsWith('#')) {{
                        element = document.getElementById(selector.slice(1));
                    }}
                }}

                if (!element) {{
                    return null;
                }}

                const rect = element.getBoundingClientRect();
                return [rect.x, rect.y, rect.width, rect.height];
            }})()"#
        ),
    )
    .expect("bounding box should be readable");

    let rect = value
        .as_array()
        .expect("bounding box evaluation should return an array");
    (
        rect[0].as_f64().expect("x should be numeric"),
        rect[1].as_f64().expect("y should be numeric"),
        rect[2].as_f64().expect("width should be numeric"),
        rect[3].as_f64().expect("height should be numeric"),
    )
}

fn viewport_metrics(session: &BrowserSession) -> (f64, f64, f64, f64) {
    let value = common::evaluate(
        session,
        r#"(() => [
            window.innerWidth,
            window.innerHeight,
            window.devicePixelRatio || 1,
            Math.max(
                document.documentElement.scrollHeight,
                document.body ? document.body.scrollHeight : 0
            )
        ])()"#,
    )
    .expect("viewport metrics should be readable");

    let metrics = value
        .as_array()
        .expect("viewport metrics should return an array");
    (
        metrics[0].as_f64().expect("innerWidth should be numeric"),
        metrics[1].as_f64().expect("innerHeight should be numeric"),
        metrics[2]
            .as_f64()
            .expect("devicePixelRatio should be numeric"),
        metrics[3].as_f64().expect("scrollHeight should be numeric"),
    )
}

#[test]
#[ignore]
fn test_set_viewport_tool_emulates_breakpoint_and_snapshot_scope_reports_viewport() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    common::navigate_encoded_html(
        session,
        r#"
            <!DOCTYPE html>
            <html>
            <body style="margin: 0;">
                <main style="height: 2000px;">
                    <button id="buy">Buy now</button>
                </main>
            </body>
            </html>
        "#,
    )
    .expect("Failed to navigate");

    let baseline = viewport_metrics(session);

    let set_viewport = session
        .execute_tool(
            "set_viewport",
            json!({
                "width": 412,
                "height": 915,
                "device_scale_factor": 2.0,
                "mobile": true,
                "touch": true
            }),
        )
        .expect("set_viewport should execute");
    assert!(set_viewport.success, "set_viewport should succeed");
    let data = set_viewport.data.expect("set_viewport should include data");
    assert_eq!(data["action"].as_str(), Some("set_viewport"));
    assert_eq!(data["reset"].as_bool(), Some(false));
    assert_close(
        data["viewport_after"]["width"]
            .as_f64()
            .expect("viewport_after width should be returned"),
        412.0,
        1.0,
        "viewport width",
    );
    assert_close(
        data["viewport_after"]["height"]
            .as_f64()
            .expect("viewport_after height should be returned"),
        915.0,
        1.0,
        "viewport height",
    );
    assert_close(
        data["viewport_after"]["device_pixel_ratio"]
            .as_f64()
            .expect("viewport_after dpr should be returned"),
        2.0,
        0.1,
        "viewport dpr",
    );

    let applied = viewport_metrics(session);
    assert_close(applied.0, 412.0, 1.0, "applied innerWidth");
    assert_close(applied.1, 915.0, 1.0, "applied innerHeight");
    assert_close(applied.2, 2.0, 0.1, "applied dpr");

    let snapshot = session
        .execute_tool("snapshot", json!({}))
        .expect("snapshot should execute after set_viewport");
    assert!(snapshot.success, "snapshot should succeed");
    let snapshot_data = snapshot.data.expect("snapshot should include data");
    assert_close(
        snapshot_data["scope"]["viewport"]["width"]
            .as_f64()
            .expect("scope.viewport.width should be returned"),
        412.0,
        1.0,
        "snapshot scope viewport width",
    );
    assert_close(
        snapshot_data["scope"]["viewport"]["height"]
            .as_f64()
            .expect("scope.viewport.height should be returned"),
        915.0,
        1.0,
        "snapshot scope viewport height",
    );
    assert_close(
        snapshot_data["scope"]["viewport"]["device_pixel_ratio"]
            .as_f64()
            .expect("scope.viewport.dpr should be returned"),
        2.0,
        0.1,
        "snapshot scope viewport dpr",
    );

    let reset = session
        .execute_tool("set_viewport", json!({ "reset": true }))
        .expect("set_viewport reset should execute");
    assert!(reset.success, "set_viewport reset should succeed");

    let reset_metrics = viewport_metrics(session);
    assert_close(reset_metrics.0, baseline.0, 2.0, "reset innerWidth");
    assert_close(reset_metrics.1, baseline.1, 2.0, "reset innerHeight");
    assert_close(reset_metrics.2, baseline.2, 0.2, "reset dpr");
}

#[test]
#[ignore]
fn test_screenshot_tool_captures_viewport_artifact() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    common::navigate_encoded_html(
        session,
        r#"
            <!DOCTYPE html>
            <html>
            <body style="margin: 0;">
                <div style="height: 1800px; background: linear-gradient(#102030, #d0e0f0);"></div>
            </body>
            </html>
        "#,
    )
    .expect("Failed to navigate");

    let active_tab_id = session
        .list_tabs()
        .expect("tabs should list")
        .into_iter()
        .find(|tab| tab.active)
        .expect("one tab should be active")
        .id;
    let (inner_width, inner_height, dpr, scroll_height) = viewport_metrics(session);
    assert!(scroll_height > inner_height + 200.0);

    let data = execute_screenshot(session, json!({ "mode": "viewport" }));
    let artifact_path = common::assert_png_screenshot_artifact(&data);

    assert_eq!(data["mode"].as_str(), Some("viewport"));
    assert_eq!(data["tab_id"].as_str(), Some(active_tab_id.as_str()));
    assert!(data["clip"].is_null());
    assert_screenshot_scale_metadata(&data, "device", inner_width, inner_height, dpr, dpr, 2.0);
    assert_eq!(data["revealed_from_offscreen"].as_bool(), Some(false));
    assert_close(
        data["width"].as_u64().expect("width should be returned") as f64,
        inner_width * dpr,
        2.0,
        "viewport width",
    );
    assert_close(
        data["height"].as_u64().expect("height should be returned") as f64,
        inner_height * dpr,
        2.0,
        "viewport height",
    );

    remove_artifact(&artifact_path);
}

#[test]
#[ignore]
fn test_screenshot_tool_captures_full_page_artifact() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    common::navigate_encoded_html(
        session,
        r#"
            <!DOCTYPE html>
            <html>
            <body style="margin: 0;">
                <div style="height: 2200px; background: linear-gradient(#f7d794, #2d98da);"></div>
            </body>
            </html>
        "#,
    )
    .expect("Failed to navigate");

    let active_tab_id = session
        .list_tabs()
        .expect("tabs should list")
        .into_iter()
        .find(|tab| tab.active)
        .expect("one tab should be active")
        .id;
    let (inner_width, inner_height, dpr, scroll_height) = viewport_metrics(session);
    assert!(scroll_height > inner_height + 400.0);

    let data = execute_screenshot(session, json!({ "mode": "full_page" }));
    let artifact_path = common::assert_png_screenshot_artifact(&data);
    let screenshot_height = data["height"].as_u64().expect("height should be returned") as f64;

    assert_eq!(data["mode"].as_str(), Some("full_page"));
    assert_eq!(data["tab_id"].as_str(), Some(active_tab_id.as_str()));
    assert!(data["clip"].is_null());
    assert_screenshot_scale_metadata(&data, "device", inner_width, scroll_height, dpr, dpr, 4.0);
    assert_eq!(data["revealed_from_offscreen"].as_bool(), Some(false));
    assert_close(
        data["width"].as_u64().expect("width should be returned") as f64,
        inner_width * dpr,
        2.0,
        "full-page width",
    );
    assert!(
        screenshot_height > inner_height * dpr + 200.0,
        "full-page capture should extend beyond the viewport"
    );
    assert_close(
        screenshot_height,
        scroll_height * dpr,
        4.0,
        "full-page height",
    );

    remove_artifact(&artifact_path);
}

#[test]
#[ignore]
fn test_screenshot_tool_captures_non_active_tab_by_tab_id() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    common::navigate_encoded_html(
        session,
        r#"
            <!DOCTYPE html>
            <html>
            <body style="margin: 0;">
                <div style="height: 400px; background: #f6e58d;">Active tab</div>
            </body>
            </html>
        "#,
    )
    .expect("Failed to navigate first tab");

    let first_tab_id = session
        .list_tabs()
        .expect("tabs should list")
        .into_iter()
        .find(|tab| tab.active)
        .expect("one tab should be active")
        .id;

    let captured_tab = session
        .open_tab(&common::encoded_html_url(
            r#"
                <!DOCTYPE html>
                <html>
                <body style="margin: 0;">
                    <div style="height: 1200px; background: #7ed6df;">Captured tab</div>
                </body>
                </html>
            "#,
        ))
        .expect("captured tab should open");
    common::wait_for_document_ready(session).expect("captured tab should finish loading");

    session
        .activate_tab(&first_tab_id)
        .expect("original tab should reactivate");
    common::wait_for_document_ready(session).expect("original tab should be ready");

    let before_capture_active_id = session
        .list_tabs()
        .expect("tabs should list")
        .into_iter()
        .find(|tab| tab.active)
        .expect("one tab should be active")
        .id;
    assert_eq!(before_capture_active_id, first_tab_id);

    let data = execute_screenshot(
        session,
        json!({
            "mode": "viewport",
            "tab_id": captured_tab.id,
        }),
    );
    let artifact_path = common::assert_png_screenshot_artifact(&data);

    assert_eq!(data["mode"].as_str(), Some("viewport"));
    assert_eq!(data["tab_id"].as_str(), Some(captured_tab.id.as_str()));
    let (inner_width, inner_height, dpr, _scroll_height) = viewport_metrics(session);
    assert_screenshot_scale_metadata(&data, "device", inner_width, inner_height, dpr, dpr, 2.0);

    let after_capture_active_id = session
        .list_tabs()
        .expect("tabs should still list")
        .into_iter()
        .find(|tab| tab.active)
        .expect("one tab should remain active")
        .id;
    assert_eq!(after_capture_active_id, first_tab_id);

    remove_artifact(&artifact_path);
}

#[test]
#[ignore]
fn test_screenshot_tool_captures_heading_and_image_targets() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    common::navigate_encoded_html(session, production_inspection_fixture_html())
        .expect("Failed to navigate");

    let heading = execute_screenshot(
        session,
        json!({
            "mode": "element",
            "target": {
                "kind": "selector",
                "selector": "h1",
            },
        }),
    );
    let heading_artifact = common::assert_png_screenshot_artifact(&heading);
    let (heading_x, heading_y, heading_width, heading_height) = clip_rect(&heading);
    let (expected_hx, expected_hy, expected_hw, expected_hh) = rect_for_selector(session, "h1");

    assert_eq!(heading["mode"].as_str(), Some("element"));
    let (_inner_width, _inner_height, dpr, _scroll_height) = viewport_metrics(session);
    assert_screenshot_scale_metadata(&heading, "device", expected_hw, expected_hh, dpr, dpr, 2.0);
    assert_eq!(heading["revealed_from_offscreen"].as_bool(), Some(false));
    assert_close(heading_x, expected_hx, 1.0, "heading clip x");
    assert_close(heading_y, expected_hy, 1.0, "heading clip y");
    assert_close(heading_width, expected_hw, 1.0, "heading clip width");
    assert_close(heading_height, expected_hh, 1.0, "heading clip height");

    let image = execute_screenshot(
        session,
        json!({
            "mode": "element",
            "target": {
                "kind": "selector",
                "selector": "#3hero-image",
            },
        }),
    );
    let image_artifact = common::assert_png_screenshot_artifact(&image);
    let (image_x, image_y, image_width, image_height) = clip_rect(&image);
    let (expected_ix, expected_iy, expected_iw, expected_ih) =
        rect_for_selector(session, "#3hero-image");

    assert_eq!(image["mode"].as_str(), Some("element"));
    assert_screenshot_scale_metadata(&image, "device", expected_iw, expected_ih, dpr, dpr, 2.0);
    assert_eq!(image["revealed_from_offscreen"].as_bool(), Some(false));
    assert_close(image_x, expected_ix, 1.0, "image clip x");
    assert_close(image_y, expected_iy, 1.0, "image clip y");
    assert_close(image_width, expected_iw, 1.0, "image clip width");
    assert_close(image_height, expected_ih, 1.0, "image clip height");

    remove_artifact(&heading_artifact);
    remove_artifact(&image_artifact);
}

#[test]
#[ignore]
fn test_screenshot_tool_rebinds_stale_cursor_for_element_capture() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    common::navigate_encoded_html(session, production_inspection_fixture_html())
        .expect("Failed to navigate");

    let snapshot_tool = SnapshotTool;
    let mut context = ToolContext::new(session);
    let snapshot = snapshot_tool
        .execute_typed(SnapshotParams::default(), &mut context)
        .expect("snapshot should succeed");
    let snapshot_data = snapshot.data.expect("snapshot should include data");
    let tab_node = snapshot_node_by_name(&snapshot_data, "Rippling");
    let stale_cursor_value = tab_node["cursor"].clone();
    let stale_cursor: Cursor =
        serde_json::from_value(stale_cursor_value.clone()).expect("cursor should deserialize");
    let stale_revision = stale_cursor.node_ref.revision.clone();

    common::evaluate(
        session,
        "document.getElementById('3compliance-api').setAttribute('data-screenshot-pass', '1'); true",
    )
    .expect("revision bump should succeed");
    common::wait_until(
        "document revision to change after screenshot target mutation",
        std::time::Duration::from_secs(2),
        || Ok(session.document_metadata()?.revision != stale_revision),
    )
    .expect("document revision should change after mutation");

    let data = execute_screenshot(
        session,
        json!({
            "mode": "element",
            "target": {
                "kind": "cursor",
                "cursor": stale_cursor_value,
            },
        }),
    );
    let artifact_path = common::assert_png_screenshot_artifact(&data);
    let (clip_x, clip_y, clip_width, clip_height) = clip_rect(&data);
    let (expected_x, expected_y, expected_width, expected_height) =
        rect_for_selector(session, stale_cursor.selector.as_str());

    assert_eq!(data["mode"].as_str(), Some("element"));
    let (_inner_width, _inner_height, dpr, _scroll_height) = viewport_metrics(session);
    assert_screenshot_scale_metadata(
        &data,
        "device",
        expected_width,
        expected_height,
        dpr,
        dpr,
        2.0,
    );
    assert_ne!(
        session
            .document_metadata()
            .expect("document metadata should be readable")
            .revision,
        stale_revision
    );
    assert_close(clip_x, expected_x, 1.0, "rebound clip x");
    assert_close(clip_y, expected_y, 1.0, "rebound clip y");
    assert_close(clip_width, expected_width, 1.0, "rebound clip width");
    assert_close(clip_height, expected_height, 1.0, "rebound clip height");

    remove_artifact(&artifact_path);
}

#[test]
#[ignore]
fn test_screenshot_tool_captures_precise_region() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    common::navigate_encoded_html(
        session,
        r#"
            <!DOCTYPE html>
            <html>
            <body style="margin: 0;">
                <div
                    id="region-box"
                    style="
                        position: absolute;
                        left: 40px;
                        top: 60px;
                        width: 120px;
                        height: 90px;
                        background: linear-gradient(90deg, #eb4d4b 50%, #22a6b3 50%);
                    "
                ></div>
            </body>
            </html>
        "#,
    )
    .expect("Failed to navigate");

    let data = execute_screenshot(
        session,
        json!({
            "mode": "region",
            "region": {
                "x": 40.0,
                "y": 60.0,
                "width": 120.0,
                "height": 90.0,
            },
        }),
    );
    let artifact_path = common::assert_png_screenshot_artifact(&data);
    let (clip_x, clip_y, clip_width, clip_height) = clip_rect(&data);
    let (box_x, box_y, box_width, box_height) = rect_for_selector(session, "#region-box");

    assert_eq!(data["mode"].as_str(), Some("region"));
    let (_inner_width, _inner_height, dpr, _scroll_height) = viewport_metrics(session);
    assert_screenshot_scale_metadata(&data, "device", 120.0, 90.0, dpr, dpr, 2.0);
    assert_eq!(data["width"].as_u64(), Some((120.0 * dpr).round() as u64));
    assert_eq!(data["height"].as_u64(), Some((90.0 * dpr).round() as u64));
    assert_eq!(data["revealed_from_offscreen"].as_bool(), Some(false));
    assert_eq!(clip_x, 40.0);
    assert_eq!(clip_y, 60.0);
    assert_eq!(clip_width, 120.0);
    assert_eq!(clip_height, 90.0);
    assert_close(clip_x, box_x, 0.1, "region clip x");
    assert_close(clip_y, box_y, 0.1, "region clip y");
    assert_close(clip_width, box_width, 0.1, "region clip width");
    assert_close(clip_height, box_height, 0.1, "region clip height");

    remove_artifact(&artifact_path);
}

#[test]
#[ignore]
fn test_screenshot_tool_css_scale_normalizes_region_capture() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    common::navigate_encoded_html(
        session,
        r#"
            <!DOCTYPE html>
            <html>
            <body style="margin: 0;">
                <div
                    id="region-box"
                    style="
                        position: absolute;
                        left: 24px;
                        top: 36px;
                        width: 140px;
                        height: 88px;
                        background: linear-gradient(90deg, #ffbe76 50%, #6ab04c 50%);
                    "
                ></div>
            </body>
            </html>
        "#,
    )
    .expect("Failed to navigate");

    let (_inner_width, _inner_height, dpr, _scroll_height) = viewport_metrics(session);
    assert!(dpr >= 1.0);

    let data = execute_screenshot(
        session,
        json!({
            "mode": "region",
            "scale": "css",
            "region": {
                "x": 24.0,
                "y": 36.0,
                "width": 140.0,
                "height": 88.0,
            },
        }),
    );
    let artifact_path = common::assert_png_screenshot_artifact(&data);

    assert_eq!(data["mode"].as_str(), Some("region"));
    assert_screenshot_scale_metadata(&data, "css", 140.0, 88.0, dpr, 1.0, 2.0);
    assert_eq!(data["width"].as_u64(), Some(140));
    assert_eq!(data["height"].as_u64(), Some(88));
    assert_eq!(data["revealed_from_offscreen"].as_bool(), Some(false));

    remove_artifact(&artifact_path);
}

#[test]
#[ignore]
fn test_screenshot_tool_reveals_offscreen_element_before_capture() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    common::navigate_encoded_html(
        session,
        r#"
            <!DOCTYPE html>
            <html>
            <body style="margin: 0; min-height: 2600px;">
                <button
                    id="offscreen-shot"
                    style="position: absolute; top: 1700px; left: 24px; width: 180px; height: 44px;"
                >
                    Offscreen screenshot
                </button>
            </body>
            </html>
        "#,
    )
    .expect("Failed to navigate");

    let data = execute_screenshot(
        session,
        json!({
            "mode": "element",
            "target": {
                "kind": "selector",
                "selector": "#offscreen-shot",
            },
        }),
    );
    let artifact_path = common::assert_png_screenshot_artifact(&data);
    let (_clip_x, _clip_y, clip_width, clip_height) = clip_rect(&data);
    let (_inner_width, _inner_height, dpr, _scroll_height) = viewport_metrics(session);

    assert_eq!(data["mode"].as_str(), Some("element"));
    assert_eq!(data["revealed_from_offscreen"].as_bool(), Some(true));
    assert_screenshot_scale_metadata(&data, "device", clip_width, clip_height, dpr, dpr, 2.0);

    let scroll_y = common::evaluate(session, "window.scrollY")
        .expect("window.scrollY should be readable")
        .as_f64()
        .expect("window.scrollY should be numeric");
    assert!(
        scroll_y > 0.0,
        "element capture should scroll the target into view"
    );

    remove_artifact(&artifact_path);
}

#[test]
#[ignore]
fn test_screenshot_tool_fails_structurally_for_hidden_offscreen_element() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    common::navigate_encoded_html(
        session,
        r#"
            <!DOCTYPE html>
            <html>
            <body style="margin: 0; min-height: 2600px;">
                <button
                    id="hidden-shot"
                    style="position: absolute; top: 1700px; left: 24px; width: 180px; height: 44px; display: none;"
                >
                    Hidden screenshot
                </button>
            </body>
            </html>
        "#,
    )
    .expect("Failed to navigate");

    let data = execute_screenshot_failure(
        session,
        json!({
            "mode": "element",
            "target": {
                "kind": "selector",
                "selector": "#hidden-shot",
            },
        }),
    );

    assert_eq!(data["code"].as_str(), Some("target_not_visible"));
    assert_eq!(
        data["details"]["viewport_state"]["reveal_attempted"].as_bool(),
        Some(false)
    );
    assert_eq!(
        data["details"]["viewport_state"]["visible"].as_bool(),
        Some(false)
    );
    assert_eq!(
        data["recovery"]["suggested_tool"].as_str(),
        Some("inspect_node")
    );
}

#[test]
#[ignore]
fn test_read_links_tool_returns_raw_and_resolved_urls_via_registry() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    let html = r#"
        <!DOCTYPE html>
        <html>
        <head>
            <title>Read Links Registry Test</title>
            <base href="https://example.test/docs/">
        </head>
        <body>
            <a href="guide">Guide</a>
            <a href="https://www.rust-lang.org/">Rust</a>
        </body>
        </html>
    "#;

    common::navigate_encoded_html(session, html).expect("Failed to navigate");

    let result = session
        .execute_tool("read_links", json!({}))
        .expect("read_links should execute");

    assert!(result.success, "read_links should succeed");
    let data = result.data.expect("read_links should return data");
    let links = data["links"]
        .as_array()
        .expect("read_links should return a links array");
    assert_eq!(data["count"].as_u64(), Some(2));

    let guide_link = links
        .iter()
        .find(|link| link["text"].as_str() == Some("Guide"))
        .expect("Guide link should be returned");
    assert_eq!(guide_link["href"].as_str(), Some("guide"));
    assert_eq!(
        guide_link["resolved_url"].as_str(),
        Some("https://example.test/docs/guide")
    );

    let rust_link = links
        .iter()
        .find(|link| link["text"].as_str() == Some("Rust"))
        .expect("Rust link should be returned");
    assert_eq!(
        rust_link["href"].as_str(),
        Some("https://www.rust-lang.org/")
    );
    assert_eq!(
        rust_link["resolved_url"].as_str(),
        Some("https://www.rust-lang.org/")
    );
}

#[test]
#[ignore] // Requires Chrome to be installed
fn test_select_tool() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

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

    common::navigate_html(session, html).expect("Failed to navigate");

    // Create tool and context
    let tool = SelectTool;
    let mut context = ToolContext::new(session);

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
    assert_eq!(data["selected_text"].as_str(), Some("United Kingdom"));
    assert_eq!(data["action"].as_str(), Some("select"));
    assert_eq!(data["target_before"]["selector"].as_str(), Some("#country"));
    assert_eq!(data["target_after"]["selector"].as_str(), Some("#country"));
    assert_eq!(data["target_status"].as_str(), Some("same"));
    assert!(data.get("target").is_none());
    assert!(data["document"]["revision"].as_str().is_some());
    assert!(data["snapshot"].is_null());
    assert!(data["nodes"].is_null());
}

#[test]
#[ignore]
fn test_hover_tool() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

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

    common::navigate_html(session, html).expect("Failed to navigate");

    // Create tool and context
    let tool = HoverTool;
    let mut context = ToolContext::new(session);

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
    assert!(data.get("target").is_none());
    assert_eq!(data["element"]["tag_name"].as_str(), Some("BUTTON"));
}

#[test]
#[ignore]
fn test_scroll_tool_with_amount() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

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

    common::navigate_html(session, html).expect("Failed to navigate");

    // Create tool and context
    let tool = ScrollTool;
    let mut context = ToolContext::new(session);

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
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

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

    common::navigate_html(session, html).expect("Failed to navigate");

    // Create tool and context
    let tool = ScrollTool;
    let mut context = ToolContext::new(session);

    // Execute the tool multiple times to reach bottom
    for _ in 0..10 {
        let result = tool
            .execute_typed(ScrollParams { amount: None }, &mut context)
            .expect("Failed to execute scroll tool");

        assert!(result.success);

        let data = result.data.as_ref().unwrap();
        let is_at_bottom = data["is_at_bottom"].as_bool().unwrap_or(false);

        info!(
            "Scroll iteration: scrolled={}, is_at_bottom={}",
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
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    let html = r#"
        <!DOCTYPE html>
        <html>
        <body style="height: 2800px; margin: 0;">
            <div style="height: 2400px;">Spacer</div>
            <button id="bottom">Bottom button</button>
        </body>
        </html>
    "#;

    common::navigate_html(session, html).expect("Failed to navigate");

    let tool = ScrollTool;
    let mut context = ToolContext::new(session);
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
    assert!(data.get("target").is_none());
    assert!(data["snapshot"].is_null());
    assert!(data["nodes"].is_null());
    assert!(data["global_interactive_count"].is_null());

    let actual_scroll_y = common::evaluate(session, "window.scrollY")
        .expect("window.scrollY should be readable")
        .as_f64()
        .expect("window.scrollY should be numeric");
    assert_eq!(scroll_y, actual_scroll_y.round() as i64);
}

#[test]
#[ignore]
fn test_select_with_index() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

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

    common::navigate_html(session, html).expect("Failed to navigate");

    let snapshot_tool = SnapshotTool;
    let tool = SelectTool;
    let mut context = ToolContext::new(session);

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
    assert_eq!(data["selected_text"].as_str(), Some("Green"));
    assert_eq!(data["target_status"].as_str(), Some("same"));
}

#[test]
#[ignore]
fn test_select_tool_reports_rebound_handoff_after_replacement() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

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

    common::navigate_html(session, html).expect("Failed to navigate");

    let tool = SelectTool;
    let mut context = ToolContext::new(session);
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
    assert_eq!(data["selected_text"].as_str(), Some("Canada"));
    assert_eq!(data["target_before"]["selector"].as_str(), Some("#country"));
    assert_eq!(data["target_after"]["selector"].as_str(), Some("#country"));
    assert_eq!(data["target_status"].as_str(), Some("rebound"));
    assert!(data.get("target").is_none());
    assert_ne!(
        data["target_before"]["node_ref"]["revision"].as_str(),
        data["target_after"]["node_ref"]["revision"].as_str()
    );

    let status = common::evaluate(session, "document.getElementById('status').textContent")
        .expect("status text should be readable")
        .as_str()
        .map(str::to_string);
    assert_eq!(status.as_deref(), Some("selected:ca"));

    let selected_value = common::evaluate(session, "document.getElementById('country').value")
        .expect("selected value should be readable")
        .as_str()
        .map(str::to_string);
    assert_eq!(selected_value.as_deref(), Some("ca"));
}

#[test]
#[ignore]
fn test_wait_tool_text_contains() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

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

    common::navigate_html(session, html).expect("Failed to navigate");

    let tool = WaitTool;
    let mut context = ToolContext::new(session);

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
    assert!(data.get("target").is_none());
    assert!(data["document"]["revision"].as_str().is_some());
}

#[test]
#[ignore]
fn test_wait_tool_reuses_snapshot_node_ref_inside_same_origin_iframe() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    let html = r#"
        <!DOCTYPE html>
        <html>
        <body>
            <iframe srcdoc="<html><body><button id='inside'>Inside</button></body></html>"></iframe>
        </body>
        </html>
    "#;

    common::navigate_html(session, html).expect("Failed to navigate");

    let snapshot_tool = SnapshotTool;
    let wait_tool = WaitTool;
    let mut context = ToolContext::new(session);

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
    assert!(data.get("target").is_none());
    assert!(data["document"]["revision"].as_str().is_some());
}

#[test]
#[ignore]
fn test_wait_tool_actionable_auto_waits_and_returns_handoff() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

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

    common::navigate_html(session, html).expect("Failed to navigate");

    let tool = WaitTool;
    let mut context = ToolContext::new(session);
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
    assert!(data.get("target").is_none());
}

#[test]
#[ignore]
fn test_wait_tool_stable_waits_for_layout_settle() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

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

    common::navigate_html(session, html).expect("Failed to navigate");

    let tool = WaitTool;
    let mut context = ToolContext::new(session);
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
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

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

    common::navigate_html(session, html).expect("Failed to navigate");

    let tool = WaitTool;
    let mut context = ToolContext::new(session);
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
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

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

    common::navigate_html(session, html).expect("Failed to navigate");

    let tool = ClickTool;
    let mut context = ToolContext::new(session);
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
    assert!(data.get("target").is_none());

    let status = common::evaluate(session, "document.getElementById('status').textContent")
        .expect("status text should be readable")
        .as_str()
        .map(str::to_string);
    assert_eq!(status.as_deref(), Some("clicked"));
}

#[test]
#[ignore]
fn test_click_tool_reports_rebound_handoff_for_same_element_hidden_after_click() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    let html = r#"
        <!DOCTYPE html>
        <html>
        <body>
            <div id="status">waiting</div>
            <button
                id="toc-toggle"
                aria-expanded="false"
                onclick="
                    this.setAttribute('aria-expanded', 'true');
                    this.style.display = 'none';
                    document.getElementById('status').textContent = 'collapsed';
                "
            >
                Contents
            </button>
        </body>
        </html>
    "#;

    common::navigate_html(session, html).expect("Failed to navigate");

    let tool = ClickTool;
    let mut context = ToolContext::new(session);
    let result = tool
        .execute_typed(
            ClickParams {
                selector: Some("#toc-toggle".to_string()),
                index: None,
                node_ref: None,
                cursor: None,
            },
            &mut context,
        )
        .expect("click should succeed for same-element hidden-after-click transitions");

    assert!(result.success);
    let data = result.data.expect("click should include data");
    assert_eq!(data["action"].as_str(), Some("click"));
    assert_eq!(
        data["target_before"]["selector"].as_str(),
        Some("#toc-toggle")
    );
    assert_eq!(
        data["target_after"]["selector"].as_str(),
        Some("#toc-toggle")
    );
    assert_eq!(data["target_after"]["method"].as_str(), Some("css"));
    assert!(data["target_after"]["cursor"].is_null());
    assert!(data["target_after"]["node_ref"].is_null());
    assert_eq!(data["target_status"].as_str(), Some("rebound"));
    assert!(data.get("target").is_none());

    let status = common::evaluate(session, "document.getElementById('status').textContent")
        .expect("status text should be readable")
        .as_str()
        .map(str::to_string);
    assert_eq!(status.as_deref(), Some("collapsed"));

    let display = common::evaluate(
        session,
        "getComputedStyle(document.getElementById('toc-toggle')).display",
    )
    .expect("toggle display should be readable")
    .as_str()
    .map(str::to_string);
    assert_eq!(display.as_deref(), Some("none"));

    let expanded = common::evaluate(
        session,
        "document.getElementById('toc-toggle').getAttribute('aria-expanded')",
    )
    .expect("toggle aria-expanded should be readable")
    .as_str()
    .map(str::to_string);
    assert_eq!(expanded.as_deref(), Some("true"));
}

#[test]
#[ignore]
fn test_click_tool_hidden_target_returns_structured_failure() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    let html = r#"
        <!DOCTYPE html>
        <html>
        <body>
            <button id="hidden" style="display: none;">Hidden</button>
        </body>
        </html>
    "#;

    common::navigate_html(session, html).expect("Failed to navigate");

    let tool = ClickTool;
    let mut context = ToolContext::new(session);
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
    assert_eq!(data["target"]["selector"].as_str(), Some("#hidden"));
    assert_eq!(
        data["recovery"]["suggested_tool"].as_str(),
        Some("inspect_node")
    );
}

#[test]
#[ignore]
fn test_click_tool_reports_rebound_handoff_for_tab_selection_state_change() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    let html = r#"
        <!DOCTYPE html>
        <html>
        <body>
            <div role="tablist" aria-label="Customers">
                <button
                    id="tab-rippling"
                    role="tab"
                    aria-selected="true"
                    onclick="
                        document.getElementById('tab-rippling').setAttribute('aria-selected', 'true');
                        document.getElementById('tab-better').setAttribute('aria-selected', 'false');
                        document.getElementById('panel').textContent = 'Rippling';
                    "
                >
                    Rippling
                </button>
                <button
                    id="tab-better"
                    role="tab"
                    aria-selected="false"
                    onclick="
                        document.getElementById('tab-rippling').setAttribute('aria-selected', 'false');
                        document.getElementById('tab-better').setAttribute('aria-selected', 'true');
                        document.getElementById('panel').textContent = 'Better Mortgage';
                    "
                >
                    Better Mortgage
                </button>
            </div>
            <div id="panel">Rippling</div>
        </body>
        </html>
    "#;

    common::navigate_html(session, html).expect("Failed to navigate");

    let tool = ClickTool;
    let mut context = ToolContext::new(session);
    let result = tool
        .execute_typed(
            ClickParams {
                selector: Some("#tab-better".to_string()),
                index: None,
                node_ref: None,
                cursor: None,
            },
            &mut context,
        )
        .expect("click should succeed for tab selection state changes");

    assert!(result.success);
    let data = result.data.expect("click should include data");
    assert_eq!(
        data["target_before"]["selector"].as_str(),
        Some("#tab-better")
    );
    assert_eq!(
        data["target_after"]["selector"].as_str(),
        Some("#tab-better")
    );
    assert_eq!(
        data["target_after"]["cursor"]["selector"].as_str(),
        Some("#tab-better")
    );
    assert_eq!(data["target_status"].as_str(), Some("rebound"));
    assert!(data.get("target").is_none());

    let selected = common::evaluate(
        session,
        "document.getElementById('tab-better').getAttribute('aria-selected')",
    )
    .expect("selected state should be readable")
    .as_str()
    .map(str::to_string);
    assert_eq!(selected.as_deref(), Some("true"));

    let panel = common::evaluate(session, "document.getElementById('panel').textContent")
        .expect("panel text should be readable")
        .as_str()
        .map(str::to_string);
    assert_eq!(panel.as_deref(), Some("Better Mortgage"));
}

#[test]
#[ignore]
fn test_click_tool_rebinds_stale_snapshot_cursor_via_selector_for_tab_selection() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    let html = r#"
        <!DOCTYPE html>
        <html>
        <body>
            <div role="tablist" aria-label="Customers">
                <button
                    id="tab-rippling"
                    role="tab"
                    aria-selected="true"
                    onclick="
                        document.getElementById('tab-rippling').setAttribute('aria-selected', 'true');
                        document.getElementById('tab-better').setAttribute('aria-selected', 'false');
                        document.getElementById('panel').textContent = 'Rippling';
                    "
                >
                    Rippling
                </button>
                <button
                    id="tab-better"
                    role="tab"
                    aria-selected="false"
                    onclick="
                        document.getElementById('tab-rippling').setAttribute('aria-selected', 'false');
                        document.getElementById('tab-better').setAttribute('aria-selected', 'true');
                        document.getElementById('panel').textContent = 'Better Mortgage';
                    "
                >
                    Better Mortgage
                </button>
            </div>
            <div id="panel">Rippling</div>
        </body>
        </html>
    "#;

    common::navigate_html(session, html).expect("Failed to navigate");

    let snapshot_tool = SnapshotTool;
    let click_tool = ClickTool;
    let mut context = ToolContext::new(session);

    let snapshot = snapshot_tool
        .execute_typed(SnapshotParams::default(), &mut context)
        .expect("snapshot should succeed");
    let snapshot_data = snapshot.data.expect("snapshot should include data");
    let stale_cursor = snapshot_cursor_for_selector(&snapshot_data, "#tab-better");
    let stale_cursor_json =
        serde_json::to_value(&stale_cursor).expect("cursor should serialize for assertions");

    common::evaluate(
        session,
        "document.getElementById('panel').setAttribute('data-pre-click', 'stale'); true",
    )
    .expect("pre-click mutation should succeed");

    let result = click_tool
        .execute_typed(
            ClickParams {
                selector: None,
                index: None,
                node_ref: None,
                cursor: Some(stale_cursor),
            },
            &mut context,
        )
        .expect("click should succeed after stale cursor rebound");

    assert!(result.success);
    let data = result.data.expect("click should include data");
    assert_eq!(data["target_before"]["method"].as_str(), Some("cursor"));
    assert_eq!(
        data["target_before"]["resolution_status"].as_str(),
        Some("selector_rebound")
    );
    assert_eq!(
        data["target_before"]["recovered_from"].as_str(),
        Some("cursor")
    );
    assert_eq!(
        data["target_before"]["selector"].as_str(),
        Some("#tab-better")
    );
    assert_eq!(
        data["target_before"]["cursor"]["selector"].as_str(),
        Some("#tab-better")
    );
    assert_ne!(
        data["target_before"]["cursor"]["node_ref"]["revision"].as_str(),
        stale_cursor_json["node_ref"]["revision"].as_str()
    );
    assert_eq!(
        data["target_after"]["selector"].as_str(),
        Some("#tab-better")
    );
    assert_eq!(data["target_status"].as_str(), Some("rebound"));
    assert!(data.get("target").is_none());

    let selected = common::evaluate(
        session,
        "document.getElementById('tab-better').getAttribute('aria-selected')",
    )
    .expect("selected state should be readable")
    .as_str()
    .map(str::to_string);
    assert_eq!(selected.as_deref(), Some("true"));
}

#[test]
#[ignore]
fn test_click_tool_offscreen_target_auto_scrolls_into_view() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

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

    common::navigate_html(session, html).expect("Failed to navigate");

    let tool = ClickTool;
    let mut context = ToolContext::new(session);
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

    let scroll_y = common::evaluate(session, "window.scrollY")
        .expect("window.scrollY should be readable")
        .as_f64()
        .expect("window.scrollY should be numeric");
    assert!(
        scroll_y > 0.0,
        "click should scroll the offscreen target into view"
    );

    let status = common::evaluate(session, "document.getElementById('status').textContent")
        .expect("status text should be readable")
        .as_str()
        .map(str::to_string);
    assert_eq!(status.as_deref(), Some("clicked"));
}

#[test]
#[ignore]
fn test_wait_tool_reports_rebound_handoff_for_same_element_hidden_after_state_change() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    let html = r#"
        <!DOCTYPE html>
        <html>
        <body>
            <button id="toc-toggle" aria-expanded="false">Collapsed</button>
            <script>
                setTimeout(() => {
                    const toggle = document.getElementById('toc-toggle');
                    toggle.textContent = 'Expanded';
                    toggle.setAttribute('aria-expanded', 'true');
                    toggle.style.display = 'none';
                }, 150);
            </script>
        </body>
        </html>
    "#;

    common::navigate_html(session, html).expect("Failed to navigate");

    let tool = WaitTool;
    let mut context = ToolContext::new(session);
    let result = tool
        .execute_typed(
            WaitParams {
                selector: Some("#toc-toggle".to_string()),
                index: None,
                node_ref: None,
                cursor: None,
                condition: WaitCondition::TextContains,
                text: Some("Expanded".to_string()),
                value: None,
                since_revision: None,
                timeout_ms: 5_000,
            },
            &mut context,
        )
        .expect("wait should succeed for hidden-after-state-change transitions");

    assert!(result.success);
    let data = result.data.expect("wait should include data");
    assert_eq!(data["condition"].as_str(), Some("text_contains"));
    assert_eq!(
        data["target_before"]["selector"].as_str(),
        Some("#toc-toggle")
    );
    assert_eq!(
        data["target_after"]["selector"].as_str(),
        Some("#toc-toggle")
    );
    assert_eq!(data["target_after"]["method"].as_str(), Some("css"));
    assert!(data["target_after"]["cursor"].is_null());
    assert_eq!(data["target_status"].as_str(), Some("rebound"));
    assert!(data.get("target").is_none());

    let expanded = common::evaluate(
        session,
        "document.getElementById('toc-toggle').getAttribute('aria-expanded')",
    )
    .expect("aria-expanded should be readable")
    .as_str()
    .map(str::to_string);
    assert_eq!(expanded.as_deref(), Some("true"));
}

#[test]
#[ignore]
fn test_input_tool_disabled_target_returns_structured_failure() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    let html = r#"
        <!DOCTYPE html>
        <html>
        <body>
            <input id="query" type="text" disabled value="draft">
        </body>
        </html>
    "#;

    common::navigate_html(session, html).expect("Failed to navigate");

    let tool = InputTool;
    let mut context = ToolContext::new(session);
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
    assert_eq!(data["target"]["selector"].as_str(), Some("#query"));
}

#[test]
#[ignore]
fn test_hover_tool_obscured_target_returns_structured_failure() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    let html = r#"
        <!DOCTYPE html>
        <html>
        <body style="margin: 0;">
            <button id="hover-btn" style="position: absolute; top: 20px; left: 20px;">Hover Me</button>
            <div id="overlay" style="position: fixed; inset: 0; background: rgba(0, 0, 0, 0.01);"></div>
        </body>
        </html>
    "#;

    common::navigate_html(session, html).expect("Failed to navigate");

    let tool = HoverTool;
    let mut context = ToolContext::new(session);
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
    assert_eq!(data["target"]["selector"].as_str(), Some("#hover-btn"));
    let failed_predicates = data["details"]["failed_predicates"]
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
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    let html = r#"
        <!DOCTYPE html>
        <html>
        <body>
            <button id="save">Save</button>
        </body>
        </html>
    "#;

    common::navigate_html(session, html).expect("Failed to navigate");
    common::evaluate(session, "document.getElementById('save').focus(); true")
        .expect("button should be focusable");

    let snapshot_tool = SnapshotTool;
    let inspect_tool = InspectNodeTool;
    let mut context = ToolContext::new(session);

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
    assert_eq!(data["target"]["cursor"]["selector"].as_str(), Some("#save"));
    assert_eq!(data["target"]["cursor"], cursor_json);
    assert_eq!(data["target"]["selector"].as_str(), Some("#save"));
    assert_eq!(data["layout"]["visible"].as_bool(), Some(true));
    assert_eq!(data["context"]["inside_shadow_root"].as_bool(), Some(false));
    assert!(data["sections"].is_null());
}

#[test]
#[ignore]
fn test_inspect_node_rebinds_stale_snapshot_cursor_via_selector_in_production_fixture() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    common::navigate_encoded_html(session, production_inspection_fixture_html())
        .expect("Failed to navigate");

    let snapshot_tool = SnapshotTool;
    let inspect_tool = InspectNodeTool;
    let mut context = ToolContext::new(session);

    let snapshot = snapshot_tool
        .execute_typed(SnapshotParams::default(), &mut context)
        .expect("snapshot should succeed");
    let snapshot_data = snapshot.data.expect("snapshot should include data");
    let tab_node = snapshot_node_by_name(&snapshot_data, "Rippling");
    let stale_cursor_value = tab_node["cursor"].clone();
    let stale_cursor: Cursor =
        serde_json::from_value(stale_cursor_value.clone()).expect("cursor should deserialize");

    common::evaluate(
        session,
        "document.getElementById('3compliance-api').setAttribute('data-inspect-pass', '1'); true",
    )
    .expect("revision bump should succeed");

    let result = inspect_tool
        .execute_typed(
            InspectNodeParams {
                selector: None,
                index: None,
                node_ref: None,
                cursor: Some(stale_cursor),
                detail: InspectDetail::Compact,
                style_names: Vec::new(),
            },
            &mut context,
        )
        .expect("inspect_node should succeed after stale cursor rebound");

    assert!(result.success);
    let data = result.data.expect("inspect_node should include data");
    assert_eq!(data["action"].as_str(), Some("inspect_node"));
    assert_eq!(data["target"]["method"].as_str(), Some("cursor"));
    assert_eq!(
        data["target"]["resolution_status"].as_str(),
        Some("selector_rebound")
    );
    assert_eq!(data["target"]["recovered_from"].as_str(), Some("cursor"));
    assert_eq!(data["target"]["selector"].as_str(), Some("#\\37 rippling"));
    assert_eq!(
        data["target"]["cursor"]["selector"].as_str(),
        Some("#\\37 rippling")
    );
    assert_ne!(
        data["target"]["cursor"]["node_ref"]["revision"].as_str(),
        stale_cursor_value["node_ref"]["revision"].as_str()
    );
    assert_eq!(data["identity"]["id"].as_str(), Some("7rippling"));
    assert_eq!(data["accessibility"]["selected"].as_bool(), Some(true));
}

#[test]
#[ignore]
fn test_inspect_node_stale_snapshot_cursor_without_live_selector_returns_recovery_hints() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    let html = r#"
        <!DOCTYPE html>
        <html>
        <body>
            <button id="save">Save</button>
        </body>
        </html>
    "#;

    common::navigate_html(session, html).expect("Failed to navigate");

    let snapshot_tool = SnapshotTool;
    let inspect_tool = InspectNodeTool;
    let mut context = ToolContext::new(session);

    let snapshot = snapshot_tool
        .execute_typed(SnapshotParams::default(), &mut context)
        .expect("snapshot should succeed");
    let snapshot_data = snapshot.data.expect("snapshot should include data");
    let stale_cursor = snapshot_cursor_for_selector(&snapshot_data, "#save");
    let stale_cursor_json =
        serde_json::to_value(&stale_cursor).expect("cursor should serialize for assertions");

    common::evaluate(session, "document.getElementById('save').remove(); true")
        .expect("removing target should succeed");

    let result = inspect_tool
        .execute_typed(
            InspectNodeParams {
                selector: None,
                index: None,
                node_ref: None,
                cursor: Some(stale_cursor),
                detail: InspectDetail::Compact,
                style_names: Vec::new(),
            },
            &mut context,
        )
        .expect("inspect_node should return a structured stale failure");

    assert!(!result.success);
    let data = result.data.expect("failure should include data");
    assert_eq!(data["code"].as_str(), Some("stale_node_ref"));
    assert_eq!(data["details"]["provided"], stale_cursor_json["node_ref"]);
    assert_eq!(
        data["details"]["resolution"]["status"].as_str(),
        Some("unrecoverable_stale")
    );
    assert_eq!(
        data["details"]["resolution"]["recovered_from"].as_str(),
        Some("cursor")
    );
    assert_eq!(
        data["details"]["resolution"]["selector_rebound_attempted"].as_bool(),
        Some(true)
    );
    assert_eq!(
        data["recovery"]["suggested_tool"].as_str(),
        Some("snapshot")
    );
    assert_eq!(
        data["recovery"]["suggested_selector"].as_str(),
        Some("#save")
    );
    assert_ne!(
        data["document"]["revision"].as_str(),
        stale_cursor_json["node_ref"]["revision"].as_str()
    );
}

#[test]
#[ignore]
fn test_inspect_node_handles_shadow_dom() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

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

    common::navigate_html(session, html).expect("Failed to navigate");

    let snapshot_tool = SnapshotTool;
    let inspect_tool = InspectNodeTool;
    let mut context = ToolContext::new(session);

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
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

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

    common::navigate_html(session, html).expect("Failed to navigate");
    common::evaluate(session, "document.getElementById('query').focus(); true")
        .expect("input should be focusable");

    let inspect_tool = InspectNodeTool;
    let mut context = ToolContext::new(session);

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
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    let html = r#"
        <!DOCTYPE html>
        <html>
        <body>
            <button id="publish">Publish</button>
        </body>
        </html>
    "#;

    common::navigate_html(session, html).expect("Failed to navigate");

    let inspect_tool = InspectNodeTool;
    let mut context = ToolContext::new(session);

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
    assert_eq!(
        data["target"]["cursor"]["selector"].as_str(),
        Some("#publish")
    );
    assert_eq!(data["target"]["selector"].as_str(), Some("#publish"));
}

#[test]
#[ignore]
fn test_inspect_node_bounds_heavy_fields() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

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

    common::navigate_html(session, html).expect("Failed to navigate");

    let inspect_tool = InspectNodeTool;
    let mut context = ToolContext::new(session);

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
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    let html = r#"
        <!DOCTYPE html>
        <html>
        <body>
            <iframe src="data:text/html,%3Cbutton%20id%3D%22inside%22%3EInside%3C%2Fbutton%3E"></iframe>
        </body>
        </html>
    "#;

    common::navigate_html(session, html).expect("Failed to navigate");

    let inspect_tool = InspectNodeTool;
    let mut context = ToolContext::new(session);

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
        data["details"]["boundaries"][0]["status"].as_str(),
        Some("cross_origin")
    );
}

#[test]
#[ignore]
fn test_inspect_node_handles_same_origin_iframe() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    let html = r#"
        <!DOCTYPE html>
        <html>
        <body>
            <iframe srcdoc="<html><body><button id='inside'>Inside</button></body></html>"></iframe>
        </body>
        </html>
    "#;

    common::navigate_html(session, html).expect("Failed to navigate");

    let inspect_tool = InspectNodeTool;
    let mut context = ToolContext::new(session);

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
    assert_eq!(
        data["target"]["cursor"]["selector"].as_str(),
        Some("#inside")
    );
    assert_eq!(data["context"]["frame_depth"].as_u64(), Some(1));
}

#[test]
#[ignore]
fn test_inspect_node_reuses_snapshot_cursor_inside_same_origin_iframe() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    let html = r#"
        <!DOCTYPE html>
        <html>
        <body>
            <iframe srcdoc="<html><body><button id='inside'>Inside</button></body></html>"></iframe>
        </body>
        </html>
    "#;

    common::navigate_html(session, html).expect("Failed to navigate");

    let snapshot_tool = SnapshotTool;
    let inspect_tool = InspectNodeTool;
    let mut context = ToolContext::new(session);

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
    assert_eq!(data["target"]["cursor"], cursor_json);
    assert_eq!(
        data["target"]["cursor"]["selector"].as_str(),
        Some("#inside")
    );
    assert_eq!(data["context"]["frame_depth"].as_u64(), Some(1));
    assert_eq!(data["identity"]["tag"].as_str(), Some("button"));
}

#[test]
#[ignore]
fn test_inspect_node_supports_non_actionable_and_overlay_targets_in_production_fixture() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    common::navigate_encoded_html(session, production_inspection_fixture_html())
        .expect("Failed to navigate");

    let inspect_tool = InspectNodeTool;
    let mut context = ToolContext::new(session);

    let heading = inspect_tool
        .execute_typed(
            InspectNodeParams {
                selector: Some("h1".to_string()),
                index: None,
                node_ref: None,
                cursor: None,
                detail: InspectDetail::Compact,
                style_names: Vec::new(),
            },
            &mut context,
        )
        .expect("heading inspection should succeed");

    assert!(heading.success);
    let heading_data = heading
        .data
        .expect("heading inspection should include data");
    assert_eq!(heading_data["identity"]["tag"].as_str(), Some("h1"));
    assert_eq!(
        heading_data["accessibility"]["role"].as_str(),
        Some("heading")
    );
    assert!(heading_data["target"]["cursor"].is_null());
    assert_eq!(heading_data["target"]["selector"].as_str(), Some("h1"));

    let image = inspect_tool
        .execute_typed(
            InspectNodeParams {
                selector: Some("#3hero-image".to_string()),
                index: None,
                node_ref: None,
                cursor: None,
                detail: InspectDetail::Full,
                style_names: Vec::new(),
            },
            &mut context,
        )
        .expect("numeric-id image inspection should succeed");

    assert!(image.success);
    let image_data = image.data.expect("image inspection should include data");
    assert_eq!(image_data["identity"]["tag"].as_str(), Some("img"));
    assert_eq!(image_data["accessibility"]["role"].as_str(), Some("img"));
    assert_eq!(
        image_data["accessibility"]["name"].as_str(),
        Some("Workspace agent diagram")
    );
    assert!(image_data["target"]["cursor"].is_null());
    assert_eq!(
        image_data["target"]["selector"].as_str(),
        Some("#3hero-image")
    );
    assert!(
        image_data["sections"]["html"]["value"]
            .as_str()
            .unwrap_or_default()
            .contains("3hero-image")
    );

    let overlay = inspect_tool
        .execute_typed(
            InspectNodeParams {
                selector: Some("#cookie-accept".to_string()),
                index: None,
                node_ref: None,
                cursor: None,
                detail: InspectDetail::Compact,
                style_names: Vec::new(),
            },
            &mut context,
        )
        .expect("overlay inspection should succeed");

    assert!(overlay.success);
    let overlay_data = overlay
        .data
        .expect("overlay inspection should include data");
    assert_eq!(
        overlay_data["identity"]["id"].as_str(),
        Some("cookie-accept")
    );
    assert_eq!(
        overlay_data["accessibility"]["name"].as_str(),
        Some("Accept")
    );
    assert_eq!(overlay_data["context"]["frame_depth"].as_u64(), Some(0));
}

#[test]
#[ignore]
fn test_inspect_node_keeps_snapshot_cursor_consistent_for_production_tabset() {
    let Some(browser) = common::browser_or_skip() else {
        return;
    };
    let session = browser.session();

    common::navigate_encoded_html(session, production_inspection_fixture_html())
        .expect("Failed to navigate");

    let snapshot_tool = SnapshotTool;
    let inspect_tool = InspectNodeTool;
    let mut context = ToolContext::new(session);

    let snapshot = snapshot_tool
        .execute_typed(SnapshotParams::default(), &mut context)
        .expect("snapshot should succeed");
    let snapshot_data = snapshot.data.expect("snapshot should include data");
    let tab_node = snapshot_node_by_name(&snapshot_data, "Rippling");
    let cursor_value = tab_node["cursor"].clone();
    let cursor: Cursor = serde_json::from_value(cursor_value.clone()).expect("cursor should parse");

    assert_eq!(cursor.selector, "#\\37 rippling");

    let result = inspect_tool
        .execute_typed(
            InspectNodeParams {
                selector: Some(cursor.selector.clone()),
                index: None,
                node_ref: None,
                cursor: None,
                detail: InspectDetail::Full,
                style_names: Vec::new(),
            },
            &mut context,
        )
        .expect("tab inspection should succeed");

    assert!(result.success);
    let data = result.data.expect("inspect_node should include data");
    assert_eq!(
        data["target"]["selector"].as_str(),
        Some(cursor.selector.as_str())
    );
    assert_eq!(data["target"]["cursor"], cursor_value);
    assert_eq!(data["identity"]["id"].as_str(), Some("7rippling"));
    assert_eq!(data["identity"]["tag"].as_str(), Some("button"));
    assert_eq!(data["accessibility"]["name"].as_str(), Some("Rippling"));
    assert_eq!(data["accessibility"]["selected"].as_bool(), Some(true));
    assert!(
        data["sections"]["html"]["value"]
            .as_str()
            .unwrap_or_default()
            .contains("id=\"7rippling\"")
    );
}

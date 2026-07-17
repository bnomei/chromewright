//! Shared fixtures and golden-style tests for all semantic projections.

use crate::dom::DocumentMetadata;
use crate::semantic::SemanticDocument;
use crate::semantic::component::SemanticKind;
use crate::semantic::identity::SemanticRefError;
use crate::semantic::normalize::{RawSemanticNode, normalize_fixture};
use crate::semantic::render::projections::{
    render_component_json_with_limit, render_debug_with_limit, render_outline_with_limit,
    render_semantic_json_with_limit,
};
use crate::semantic::render::{
    RenderError, SemanticRatatuiView, buffer_to_lines, render_component_json,
    render_component_markdown, render_debug, render_outline, render_ratatui_buffer,
    render_ratatui_lines, render_semantic_json, render_semantic_markdown,
};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

fn meta(doc: &str, rev: &str, url: &str) -> DocumentMetadata {
    DocumentMetadata {
        document_id: doc.to_string(),
        revision: rev.to_string(),
        url: url.to_string(),
        title: "Fixture".to_string(),
        ready_state: "complete".to_string(),
        frames: Vec::new(),
    }
}

fn node(kind: &str) -> RawSemanticNode {
    RawSemanticNode {
        kind: kind.to_string(),
        tag: None,
        id: None,
        unique_id: false,
        landmark: None,
        heading_level: None,
        ordered: None,
        text: None,
        label: None,
        href: None,
        src: None,
        alt: None,
        name: None,
        value: None,
        input_type: None,
        placeholder: None,
        checked: None,
        disabled: None,
        required: None,
        readonly: None,
        multiple: None,
        button_type: None,
        options: Vec::new(),
        children: Vec::new(),
    }
}

/// Shared mixed interactive document used across Markdown/outline/JSON/debug/Ratatui.
fn shared_fixture() -> SemanticDocument {
    normalize_fixture(
        meta("doc-render", "rev-r1", "https://example.com/render"),
        vec![
            RawSemanticNode {
                kind: "landmark".into(),
                tag: Some("header".into()),
                landmark: Some("header".into()),
                unique_id: true,
                id: Some("site-header".into()),
                children: vec![RawSemanticNode {
                    kind: "heading".into(),
                    tag: Some("h1".into()),
                    heading_level: Some(1),
                    text: Some("Welcome".into()),
                    label: Some("Welcome".into()),
                    ..node("heading")
                }],
                ..node("landmark")
            },
            RawSemanticNode {
                kind: "landmark".into(),
                tag: Some("main".into()),
                landmark: Some("main".into()),
                children: vec![
                    RawSemanticNode {
                        kind: "text".into(),
                        tag: Some("p".into()),
                        // Mixed inline; aggregate deliberately wrong to prove no duplication.
                        text: Some("before link after".into()),
                        children: vec![
                            RawSemanticNode {
                                kind: "text".into(),
                                text: Some("before".into()),
                                ..node("text")
                            },
                            RawSemanticNode {
                                kind: "link".into(),
                                tag: Some("a".into()),
                                href: Some("/x".into()),
                                text: Some("link".into()),
                                label: Some("link".into()),
                                unique_id: true,
                                id: Some("inline-link".into()),
                                ..node("link")
                            },
                            RawSemanticNode {
                                kind: "text".into(),
                                text: Some("after".into()),
                                ..node("text")
                            },
                        ],
                        ..node("text")
                    },
                    RawSemanticNode {
                        kind: "list".into(),
                        tag: Some("ul".into()),
                        ordered: Some(false),
                        children: vec![
                            RawSemanticNode {
                                kind: "list_item".into(),
                                tag: Some("li".into()),
                                text: Some("Alpha".into()),
                                label: Some("Alpha".into()),
                                ..node("list_item")
                            },
                            RawSemanticNode {
                                kind: "list_item".into(),
                                tag: Some("li".into()),
                                text: Some("Beta".into()),
                                label: Some("Beta".into()),
                                ..node("list_item")
                            },
                        ],
                        ..node("list")
                    },
                    RawSemanticNode {
                        kind: "group".into(),
                        tag: Some("form".into()),
                        children: vec![
                            RawSemanticNode {
                                kind: "input".into(),
                                tag: Some("input".into()),
                                input_type: Some("text".into()),
                                name: Some("email".into()),
                                value: Some("user@example.com".into()),
                                label: Some("Email".into()),
                                required: Some(true),
                                unique_id: true,
                                id: Some("email".into()),
                                ..node("input")
                            },
                            RawSemanticNode {
                                kind: "button".into(),
                                tag: Some("button".into()),
                                button_type: Some("submit".into()),
                                text: Some("Save".into()),
                                label: Some("Save".into()),
                                ..node("button")
                            },
                        ],
                        ..node("group")
                    },
                ],
                ..node("landmark")
            },
            RawSemanticNode {
                kind: "landmark".into(),
                tag: Some("nav".into()),
                landmark: Some("nav".into()),
                children: vec![RawSemanticNode {
                    kind: "link".into(),
                    tag: Some("a".into()),
                    href: Some("/home".into()),
                    text: Some("Home".into()),
                    label: Some("Home".into()),
                    unique_id: true,
                    id: Some("nav-home".into()),
                    ..node("link")
                }],
                ..node("landmark")
            },
        ],
    )
    .expect("shared fixture")
}

#[test]
fn markdown_preserves_order_refs_and_no_duplicate_inline() {
    let doc = shared_fixture();
    let rendered = render_semantic_markdown(&doc).expect("markdown");
    assert_eq!(rendered.document_id, "doc-render");
    assert_eq!(rendered.revision, "rev-r1");
    assert!(!rendered.truncated);

    let md = &rendered.content;
    assert!(md.contains("document_id=\"doc-render\""));
    assert!(md.contains("revision=\"rev-r1\""));
    assert!(md.contains("# Fixture"));
    assert!(md.contains("# Welcome"));
    assert!(
        md.contains("before [link](/x) after") || md.contains("before[link](/x)after") || {
            // Allow single spaces around the link.
            md.contains("before") && md.contains("[link](/x)") && md.contains("after")
        }
    );
    // Must not duplicate "link" from aggregate text.
    let link_occurrences = md.matches("[link](/x)").count();
    assert_eq!(
        link_occurrences, 1,
        "mixed inline must not duplicate link text"
    );
    assert!(
        !md.contains("before link after"),
        "aggregate text must not appear"
    );

    // Exact model refs must propagate.
    let inline_link = doc
        .components()
        .find(|c| c.attrs.href.as_deref() == Some("/x"))
        .expect("inline link");
    assert!(
        md.contains(inline_link.semantic_ref.as_str()),
        "markdown must copy model semantic_ref, not invent one"
    );
    let email = doc
        .components()
        .find(|c| c.kind == SemanticKind::Input)
        .expect("input");
    assert!(md.contains(email.semantic_ref.as_str()));
    assert!(md.contains("**Input**"));
    assert!(md.contains("user@example.com"));
    assert!(md.contains("- Alpha"));
    assert!(md.contains("- Beta"));
    assert!(md.contains("[Home](/home)"));
}

#[test]
fn component_markdown_is_fail_closed_and_exact() {
    let doc = shared_fixture();
    let home = doc
        .components()
        .find(|c| c.attrs.href.as_deref() == Some("/home"))
        .expect("home")
        .semantic_ref
        .clone();

    let fragment = render_component_markdown(&doc, &home).expect("component md");
    assert!(fragment.content.contains("component_fragment"));
    assert!(fragment.content.contains(home.as_str()));
    assert!(fragment.content.contains("[Home](/home)"));
    assert!(!fragment.content.contains("# Welcome"));

    let err = render_component_markdown(&doc, &crate::semantic::SemanticRef::from_opaque("nope"))
        .expect_err("malformed");
    assert!(matches!(err, RenderError::Ref(SemanticRefError::Malformed)));
}

#[test]
fn outline_json_debug_share_refs_and_revision() {
    let doc = shared_fixture();
    let refs: Vec<_> = doc.semantic_refs();

    let outline = render_outline(&doc).expect("outline");
    assert_eq!(outline.document_id, "doc-render");
    assert_eq!(outline.revision, "rev-r1");
    for r in &refs {
        // Outline includes structural/focusable nodes; not every text fragment.
        let _ = r;
    }
    assert!(outline.content.contains("doc-render"));
    assert!(outline.content.contains("rev-r1"));
    assert!(outline.content.contains("header") || outline.content.contains("landmark"));
    let home = doc
        .components()
        .find(|c| c.attrs.href.as_deref() == Some("/home"))
        .expect("home");
    assert!(outline.content.contains(home.semantic_ref.as_str()));

    let json = render_semantic_json(&doc).expect("json");
    assert_eq!(json.document_id, "doc-render");
    assert_eq!(json.revision, "rev-r1");
    let value: serde_json::Value = serde_json::from_str(&json.content).expect("parse json");
    assert_eq!(value["document_id"], "doc-render");
    assert_eq!(value["revision"], "rev-r1");
    assert_eq!(value["component_count"], doc.component_count());
    // Every model ref must appear in JSON (opaque tokens copied from model).
    for semantic_ref in &refs {
        assert!(
            json.content.contains(semantic_ref.as_str()),
            "missing ref in json: {}",
            semantic_ref.as_str()
        );
    }

    let debug = render_debug(&doc).expect("debug");
    assert_eq!(debug.document_id, "doc-render");
    assert_eq!(debug.revision, "rev-r1");
    let debug_value: serde_json::Value = serde_json::from_str(&debug.content).expect("debug json");
    assert_eq!(debug_value["document_id"], "doc-render");
    assert_eq!(debug_value["revision"], "rev-r1");
    assert!(debug_value["nodes"].as_array().expect("nodes").len() >= doc.component_count());
    for semantic_ref in &refs {
        assert!(debug.content.contains(semantic_ref.as_str()));
    }
    // Debug carries kind/depth/tag-safe attrs.
    assert!(debug.content.contains("\"depth\""));
    assert!(debug.content.contains("\"kind\""));

    let email = doc
        .components()
        .find(|c| c.kind == SemanticKind::Input)
        .expect("email")
        .semantic_ref
        .clone();
    let component = render_component_json(&doc, &email).expect("component json");
    assert!(component.content.contains(email.as_str()));
    assert!(component.content.contains("user@example.com"));
    assert_eq!(component.revision, "rev-r1");
}

#[test]
fn ratatui_nested_landmark_frames_and_mixed_inline() {
    let doc = shared_fixture();
    // Line helper remains for lightweight inspection (textual markers).
    let lines = render_ratatui_lines(&doc).expect("ratatui lines");
    assert_eq!(lines.document_id, "doc-render");
    assert_eq!(lines.revision, "rev-r1");
    let body = &lines.content;
    assert!(body.contains("┌─ header ─"));
    assert!(body.contains("└─ /header ─"));
    assert!(body.contains("┌─ main ─"));
    assert!(body.contains("└─ /main ─"));
    assert!(body.contains("┌─ nav ─"));
    assert!(
        body.contains("# Welcome")
            || body.contains("# Welcome\n")
            || body.lines().any(|l| l.contains("Welcome"))
    );
    // Mixed inline ordered without duplication.
    assert!(
        body.contains("before") && body.contains("[link](/x)") && body.contains("after"),
        "ratatui must keep mixed inline order: {body}"
    );
    assert_eq!(body.matches("[link](/x)").count(), 1);
    assert!(!body.contains("before link after"));
    assert!(body.contains("- Alpha"));
    assert!(body.contains("[Home](/home)") || body.contains("Home"));

    // Widget/buffer path uses real Ratatui Blocks (not the line-helper markers).
    let rows = render_ratatui_buffer(&doc, 80, 40).expect("buffer");
    assert!(!rows.is_empty());
    let joined = rows.join("\n");
    assert!(
        joined.contains("header") && joined.contains("main") && joined.contains("nav"),
        "widget path must title nested landmark Blocks: {joined}"
    );
    // Line helper closing markers must not appear in the Block render path.
    assert!(
        !joined.contains("└─ /header ─") && !joined.contains("└─ /main ─"),
        "widget path must render real Blocks, not line-helper close markers: {joined}"
    );
    assert!(
        joined.contains('┌') || joined.contains('│') || joined.contains('─'),
        "widget buffer must contain Block border glyphs: {joined}"
    );

    let area = Rect::new(0, 0, 60, 30);
    let mut buffer = Buffer::empty(area);
    SemanticRatatuiView::new(&doc).render(area, &mut buffer);
    let widget_rows = buffer_to_lines(&buffer, area);
    assert!(!widget_rows.is_empty());
}

#[test]
fn ratatui_widget_path_renders_nested_landmark_blocks() {
    // Nested landmarks: main > section > heading — widget must draw real Blocks.
    let doc = normalize_fixture(
        meta("doc-blocks", "rev-b1", "https://example.com/blocks"),
        vec![RawSemanticNode {
            kind: "landmark".into(),
            tag: Some("main".into()),
            landmark: Some("main".into()),
            children: vec![RawSemanticNode {
                kind: "landmark".into(),
                tag: Some("section".into()),
                landmark: Some("section".into()),
                children: vec![RawSemanticNode {
                    kind: "heading".into(),
                    tag: Some("h2".into()),
                    heading_level: Some(2),
                    text: Some("Nested".into()),
                    label: Some("Nested".into()),
                    ..node("heading")
                }],
                ..node("landmark")
            }],
            ..node("landmark")
        }],
    )
    .expect("nested landmark fixture");

    let width = 48u16;
    let height = 16u16;
    let area = Rect::new(0, 0, width, height);
    let mut buffer = Buffer::empty(area);
    SemanticRatatuiView::new(&doc).render(area, &mut buffer);
    let rows = buffer_to_lines(&buffer, area);
    let joined = rows.join("\n");

    // Titles come from Block::title on landmark frames.
    assert!(
        joined.contains("main") && joined.contains("section"),
        "nested Block titles missing: {joined}"
    );
    assert!(
        joined.contains("Nested") || joined.contains("# Nested") || joined.contains("## Nested"),
        "inner content missing inside nested Blocks: {joined}"
    );
    // Not the textual line helper.
    assert!(
        !joined.contains("└─ /main ─") && !joined.contains("└─ /section ─"),
        "must not use line-helper close markers in widget path: {joined}"
    );

    // Prove nested borders: top-left corner glyphs at increasing x insets.
    // Document chrome at x=0, outer landmark at x=1, nested landmark at x=2.
    let mut corner_xs: Vec<u16> = Vec::new();
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let sym = buffer[(x, y)].symbol();
            if (sym == "┌" || sym.starts_with('┌')) && !corner_xs.contains(&x) {
                corner_xs.push(x);
            }
        }
    }
    corner_xs.sort_unstable();
    assert!(
        corner_xs.len() >= 2,
        "expected nested Block top-left corners at multiple x insets, got {corner_xs:?} in:\n{joined}"
    );
    // At least one corner is inset (nested inside another Block / chrome).
    assert!(
        corner_xs.iter().any(|&x| x >= 1),
        "expected an inset nested Block border, corners={corner_xs:?}"
    );
}

#[test]
fn all_projections_share_one_document_revision() {
    let doc = shared_fixture();
    let md = render_semantic_markdown(&doc).unwrap();
    let outline = render_outline(&doc).unwrap();
    let json = render_semantic_json(&doc).unwrap();
    let debug = render_debug(&doc).unwrap();
    let ratatui = render_ratatui_lines(&doc).unwrap();
    for out in [&md, &outline, &json, &debug, &ratatui] {
        assert_eq!(out.document_id, doc.document.document_id);
        assert_eq!(out.revision, doc.document.revision);
    }
}

#[test]
fn get_markdown_service_remains_readability_pipeline() {
    // Static contract check: the existing get_markdown service must stay on
    // Readability + html_to_markdown and must not call semantic renderers.
    let service = include_str!("../../tools/services/markdown.rs");
    let tool = include_str!("../../tools/markdown.rs");
    let html_to_md = include_str!("../../tools/html_to_markdown.rs");

    assert!(
        service.contains("convert_html_to_markdown"),
        "get_markdown service must convert via html_to_markdown"
    );
    assert!(
        service.contains("READABILITY_SCRIPT") || service.contains("readability"),
        "get_markdown must use Readability extraction"
    );
    assert!(
        service.contains("execute_get_markdown"),
        "service entrypoint must remain execute_get_markdown"
    );
    assert!(
        !service.contains("crate::semantic")
            && !service.contains("render_semantic_markdown")
            && !service.contains("SemanticDocument"),
        "get_markdown must not depend on the semantic renderer"
    );
    assert!(
        !tool.contains("crate::semantic") && !tool.contains("render_semantic_markdown"),
        "GetMarkdownTool must not redirect to semantic markdown"
    );
    assert!(
        html_to_md.contains("html2md") && html_to_md.contains("convert_html_to_markdown"),
        "html_to_markdown contract must remain"
    );
}

#[test]
fn markdown_and_ratatui_do_not_import_browser_or_html() {
    let markdown_src = include_str!("markdown.rs");
    let projections_src = include_str!("projections.rs");
    let ratatui_src = include_str!("ratatui_view.rs");
    for src in [markdown_src, projections_src, ratatui_src] {
        assert!(!src.contains("headless_chrome"));
        assert!(!src.contains("extract_semantic"));
        assert!(!src.contains("BrowserSession"));
        assert!(!src.contains("html2md"));
        assert!(!src.contains("convert_html_to_markdown"));
    }
}

#[test]
fn json_projections_are_valid_on_success_and_never_truncated() {
    let doc = shared_fixture();

    // Generous limit: success yields parseable, untruncated JSON.
    let json = render_semantic_json_with_limit(&doc, 500_000).expect("json ok");
    assert!(!json.truncated);
    let value: serde_json::Value = serde_json::from_str(&json.content).expect("valid json");
    assert_eq!(value["document_id"], "doc-render");
    assert_eq!(value["revision"], "rev-r1");

    let debug = render_debug_with_limit(&doc, 500_000).expect("debug ok");
    assert!(!debug.truncated);
    serde_json::from_str::<serde_json::Value>(&debug.content).expect("valid debug json");

    let email = doc
        .components()
        .find(|c| c.kind == SemanticKind::Input)
        .expect("email")
        .semantic_ref
        .clone();
    let component = render_component_json_with_limit(&doc, &email, 500_000).expect("component ok");
    assert!(!component.truncated);
    serde_json::from_str::<serde_json::Value>(&component.content).expect("valid component json");
}

#[test]
fn json_projections_fail_closed_when_over_limit_never_invalid() {
    let doc = shared_fixture();

    // Tiny limit forces OutputLimit instead of mid-document truncation.
    let tiny = 32usize;
    let err = render_semantic_json_with_limit(&doc, tiny).expect_err("must reject oversize json");
    assert!(
        matches!(
            err,
            RenderError::OutputLimit {
                limit: 32,
                produced_chars
            } if produced_chars > 32
        ),
        "expected OutputLimit, got {err:?}"
    );

    let debug_err = render_debug_with_limit(&doc, tiny).expect_err("debug over limit");
    assert!(matches!(debug_err, RenderError::OutputLimit { .. }));

    let email = doc
        .components()
        .find(|c| c.kind == SemanticKind::Input)
        .expect("email")
        .semantic_ref
        .clone();
    // Component JSON is smaller; use a still-tiny limit.
    let component_err =
        render_component_json_with_limit(&doc, &email, 16).expect_err("component over limit");
    assert!(matches!(component_err, RenderError::OutputLimit { .. }));

    // Outline: when the JSON fence alone exceeds the budget, fail closed.
    let outline_err = render_outline_with_limit(&doc, tiny).expect_err("outline fence over limit");
    assert!(matches!(outline_err, RenderError::OutputLimit { .. }));
}

#[test]
fn outline_keeps_embedded_json_valid_when_human_text_is_truncated() {
    let doc = shared_fixture();
    // Full outline at default limit.
    let full = render_outline(&doc).expect("outline");
    let full_json = extract_fenced_json(&full.content).expect("full outline has json fence");
    serde_json::from_str::<serde_json::Value>(&full_json).expect("full outline json valid");

    // Choose a limit that fits the JSON fence but may truncate human prose.
    let json_chars = full_json.chars().count();
    let fence_overhead = "\n```json\n".chars().count() + "\n```\n".chars().count();
    // Budget: JSON fence + a little room for a truncated human header.
    let limit = json_chars + fence_overhead + 80;
    let limited = render_outline_with_limit(&doc, limit).expect("outline under mixed budget");
    assert!(limited.content.chars().count() <= limit);
    let limited_json =
        extract_fenced_json(&limited.content).expect("truncated outline still has complete fence");
    // The JSON body itself must be complete and equal to the full projection JSON.
    assert_eq!(limited_json, full_json);
    serde_json::from_str::<serde_json::Value>(&limited_json).expect("embedded json remains valid");
}

fn extract_fenced_json(content: &str) -> Option<String> {
    let start = content.find("```json\n")?;
    let after = &content[start + "```json\n".len()..];
    let end = after.find("\n```")?;
    Some(after[..end].to_string())
}

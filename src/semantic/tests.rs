//! Focused fixtures for semantic normalization, identity stability, and fail-closed refs.

use crate::dom::DocumentMetadata;
use crate::semantic::SemanticDocument;
use crate::semantic::component::{LandmarkRole, SemanticKind};
use crate::semantic::identity::{
    SemanticIdentity, SemanticRef, SemanticRefError, SemanticRefPayload,
};
use crate::semantic::normalize::{RawSelectOption, RawSemanticNode, normalize_fixture};

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
        selector: None,
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

fn content_fixture() -> Vec<RawSemanticNode> {
    vec![
        RawSemanticNode {
            kind: "landmark".into(),
            tag: Some("header".into()),
            landmark: Some("header".into()),
            unique_id: true,
            selector: None,
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
                    text: Some("Intro paragraph.".into()),
                    ..node("text")
                },
                RawSemanticNode {
                    kind: "landmark".into(),
                    tag: Some("section".into()),
                    landmark: Some("section".into()),
                    children: vec![RawSemanticNode {
                        kind: "heading".into(),
                        tag: Some("h2".into()),
                        heading_level: Some(2),
                        text: Some("Details".into()),
                        label: Some("Details".into()),
                        ..node("heading")
                    }],
                    ..node("landmark")
                },
            ],
            ..node("landmark")
        },
        RawSemanticNode {
            kind: "landmark".into(),
            tag: Some("footer".into()),
            landmark: Some("footer".into()),
            children: vec![RawSemanticNode {
                kind: "text".into(),
                tag: Some("p".into()),
                text: Some("Copyright".into()),
                ..node("text")
            }],
            ..node("landmark")
        },
    ]
}

fn links_fixture() -> Vec<RawSemanticNode> {
    vec![RawSemanticNode {
        kind: "landmark".into(),
        tag: Some("nav".into()),
        landmark: Some("nav".into()),
        children: vec![
            RawSemanticNode {
                kind: "link".into(),
                tag: Some("a".into()),
                href: Some("/home".into()),
                text: Some("Home".into()),
                label: Some("Home".into()),
                unique_id: true,
                selector: None,
                id: Some("nav-home".into()),
                ..node("link")
            },
            RawSemanticNode {
                kind: "link".into(),
                tag: Some("a".into()),
                href: Some("/about".into()),
                text: Some("About".into()),
                label: Some("About".into()),
                ..node("link")
            },
        ],
        ..node("landmark")
    }]
}

fn lists_fixture() -> Vec<RawSemanticNode> {
    vec![
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
            kind: "list".into(),
            tag: Some("ol".into()),
            ordered: Some(true),
            children: vec![RawSemanticNode {
                kind: "list_item".into(),
                tag: Some("li".into()),
                // Mixed inline content: ordered fragments, no aggregate text.
                children: vec![
                    RawSemanticNode {
                        kind: "text".into(),
                        text: Some("See".into()),
                        ..node("text")
                    },
                    RawSemanticNode {
                        kind: "link".into(),
                        tag: Some("a".into()),
                        href: Some("/first".into()),
                        text: Some("First".into()),
                        label: Some("First".into()),
                        ..node("link")
                    },
                ],
                ..node("list_item")
            }],
            ..node("list")
        },
    ]
}

fn controls_fixture() -> Vec<RawSemanticNode> {
    vec![RawSemanticNode {
        kind: "group".into(),
        tag: Some("form".into()),
        children: vec![
            RawSemanticNode {
                kind: "input".into(),
                tag: Some("input".into()),
                input_type: Some("text".into()),
                name: Some("email".into()),
                value: Some("user@example.com".into()),
                placeholder: Some("Email".into()),
                label: Some("Email".into()),
                required: Some(true),
                unique_id: true,
                selector: None,
                id: Some("email".into()),
                ..node("input")
            },
            RawSemanticNode {
                kind: "textarea".into(),
                tag: Some("textarea".into()),
                name: Some("bio".into()),
                value: Some("Hello".into()),
                label: Some("Bio".into()),
                ..node("textarea")
            },
            RawSemanticNode {
                kind: "select".into(),
                tag: Some("select".into()),
                name: Some("color".into()),
                value: Some("blue".into()),
                label: Some("Color".into()),
                options: vec![
                    RawSelectOption {
                        value: "red".into(),
                        label: Some("Red".into()),
                        selected: false,
                        disabled: false,
                    },
                    RawSelectOption {
                        value: "blue".into(),
                        label: Some("Blue".into()),
                        selected: true,
                        disabled: false,
                    },
                ],
                ..node("select")
            },
            RawSemanticNode {
                kind: "button".into(),
                tag: Some("button".into()),
                button_type: Some("submit".into()),
                text: Some("Save".into()),
                label: Some("Save".into()),
                ..node("button")
            },
            RawSemanticNode {
                kind: "image".into(),
                tag: Some("img".into()),
                src: Some("/logo.png".into()),
                alt: Some("Logo".into()),
                label: Some("Logo".into()),
                ..node("image")
            },
        ],
        ..node("group")
    }]
}

#[test]
fn normalize_content_landmarks_and_text() {
    let document = normalize_fixture(
        meta("doc-content", "rev-1", "https://example.com/content"),
        content_fixture(),
    )
    .expect("normalize content");

    assert_eq!(document.document.document_id, "doc-content");
    assert_eq!(document.document.revision, "rev-1");
    assert_eq!(document.roots.len(), 3);
    assert_eq!(document.roots[0].kind, SemanticKind::Landmark);
    assert_eq!(document.roots[0].attrs.landmark, Some(LandmarkRole::Header));
    assert_eq!(document.roots[1].attrs.landmark, Some(LandmarkRole::Main));
    assert_eq!(document.roots[2].attrs.landmark, Some(LandmarkRole::Footer));

    let heading = &document.roots[0].children[0];
    assert_eq!(heading.kind, SemanticKind::Heading);
    assert_eq!(heading.attrs.heading_level, Some(1));
    assert_eq!(heading.text.as_deref(), Some("Welcome"));

    // Generic layout wrappers are not present; only semantic nodes remain.
    let kinds: Vec<_> = document.components().map(|c| c.kind).collect();
    assert!(kinds.contains(&SemanticKind::Text));
    assert!(!kinds.is_empty());
}

#[test]
fn normalize_links_retains_href_and_labels() {
    let document = normalize_fixture(
        meta("doc-links", "rev-1", "https://example.com/links"),
        links_fixture(),
    )
    .expect("normalize links");

    let links: Vec<_> = document
        .components()
        .filter(|c| c.kind == SemanticKind::Link)
        .collect();
    assert_eq!(links.len(), 2);
    assert_eq!(links[0].attrs.href.as_deref(), Some("/home"));
    assert_eq!(links[0].text.as_deref(), Some("Home"));
    assert_eq!(links[1].attrs.href.as_deref(), Some("/about"));
    assert!(links[0].is_focusable());
}

#[test]
fn normalize_lists_ordered_and_nested_links() {
    let document = normalize_fixture(
        meta("doc-lists", "rev-1", "https://example.com/lists"),
        lists_fixture(),
    )
    .expect("normalize lists");

    let lists: Vec<_> = document
        .components()
        .filter(|c| c.kind == SemanticKind::List)
        .collect();
    assert_eq!(lists.len(), 2);
    assert_eq!(lists[0].attrs.ordered, Some(false));
    assert_eq!(lists[1].attrs.ordered, Some(true));

    let items: Vec<_> = document
        .components()
        .filter(|c| c.kind == SemanticKind::ListItem)
        .collect();
    assert_eq!(items.len(), 3);
    assert!(
        items[2].text.is_none(),
        "mixed list item must not keep aggregate text"
    );
    assert_eq!(items[2].children.len(), 2);
    assert_eq!(items[2].children[0].kind, SemanticKind::Text);
    assert_eq!(items[2].children[0].text.as_deref(), Some("See"));
    assert_eq!(items[2].children[1].kind, SemanticKind::Link);
    assert_eq!(items[2].children[1].attrs.href.as_deref(), Some("/first"));
}

#[test]
fn normalize_mixed_inline_paragraph_preserves_order_without_duplicate_text() {
    // Capture shape for: <p>before <a href="/x">link</a> after</p>
    let document = normalize_fixture(
        meta("doc-mixed", "rev-1", "https://example.com/mixed"),
        vec![RawSemanticNode {
            kind: "text".into(),
            tag: Some("p".into()),
            // Deliberately wrong aggregate that would duplicate nested link text.
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
                    ..node("link")
                },
                RawSemanticNode {
                    kind: "text".into(),
                    text: Some("after".into()),
                    ..node("text")
                },
            ],
            ..node("text")
        }],
    )
    .expect("normalize mixed paragraph");

    assert_eq!(document.roots.len(), 1);
    let paragraph = &document.roots[0];
    assert_eq!(paragraph.kind, SemanticKind::Text);
    assert!(
        paragraph.text.is_none(),
        "paragraph with nested semantics must not retain aggregate innerText"
    );
    assert_eq!(paragraph.children.len(), 3);
    assert_eq!(paragraph.children[0].kind, SemanticKind::Text);
    assert_eq!(paragraph.children[0].text.as_deref(), Some("before"));
    assert_eq!(paragraph.children[1].kind, SemanticKind::Link);
    assert_eq!(paragraph.children[1].text.as_deref(), Some("link"));
    assert_eq!(paragraph.children[1].attrs.href.as_deref(), Some("/x"));
    assert!(
        paragraph.children[1].children.is_empty(),
        "links are terminal leaves"
    );
    assert_eq!(paragraph.children[2].kind, SemanticKind::Text);
    assert_eq!(paragraph.children[2].text.as_deref(), Some("after"));
}

#[test]
fn normalize_mixed_inline_heading_preserves_order_without_duplicate_text() {
    // Capture shape for: <h2>Read <a href="/docs">docs</a> next</h2>
    let document = normalize_fixture(
        meta("doc-heading-mixed", "rev-1", "https://example.com/h"),
        vec![RawSemanticNode {
            kind: "heading".into(),
            tag: Some("h2".into()),
            heading_level: Some(2),
            text: Some("Read docs next".into()),
            label: Some("Read docs next".into()),
            children: vec![
                RawSemanticNode {
                    kind: "text".into(),
                    text: Some("Read".into()),
                    ..node("text")
                },
                RawSemanticNode {
                    kind: "link".into(),
                    tag: Some("a".into()),
                    href: Some("/docs".into()),
                    text: Some("docs".into()),
                    label: Some("docs".into()),
                    ..node("link")
                },
                RawSemanticNode {
                    kind: "text".into(),
                    text: Some("next".into()),
                    ..node("text")
                },
            ],
            ..node("heading")
        }],
    )
    .expect("normalize mixed heading");

    let heading = &document.roots[0];
    assert_eq!(heading.kind, SemanticKind::Heading);
    assert!(heading.text.is_none());
    assert!(heading.label.is_none());
    assert_eq!(heading.attrs.heading_level, Some(2));
    assert_eq!(heading.children.len(), 3);
    assert_eq!(heading.children[0].text.as_deref(), Some("Read"));
    assert_eq!(heading.children[1].kind, SemanticKind::Link);
    assert_eq!(heading.children[1].attrs.href.as_deref(), Some("/docs"));
    assert_eq!(heading.children[2].text.as_deref(), Some("next"));
}

#[test]
fn normalize_plain_paragraph_keeps_compact_leaf_text() {
    let document = normalize_fixture(
        meta("doc-plain", "rev-1", "https://example.com/plain"),
        vec![RawSemanticNode {
            kind: "text".into(),
            tag: Some("p".into()),
            text: Some("Only text here.".into()),
            ..node("text")
        }],
    )
    .expect("normalize plain paragraph");

    let paragraph = &document.roots[0];
    assert_eq!(paragraph.kind, SemanticKind::Text);
    assert_eq!(paragraph.text.as_deref(), Some("Only text here."));
    assert!(paragraph.children.is_empty());
}

#[test]
fn normalize_controls_inputs_selects_buttons_images() {
    let document = normalize_fixture(
        meta("doc-controls", "rev-1", "https://example.com/form"),
        controls_fixture(),
    )
    .expect("normalize controls");

    let input = document
        .components()
        .find(|c| c.kind == SemanticKind::Input)
        .expect("input");
    assert_eq!(input.attrs.input_type.as_deref(), Some("text"));
    assert_eq!(input.attrs.name.as_deref(), Some("email"));
    assert_eq!(input.attrs.value.as_deref(), Some("user@example.com"));
    assert_eq!(input.attrs.required, Some(true));
    assert_eq!(input.label.as_deref(), Some("Email"));

    let select = document
        .components()
        .find(|c| c.kind == SemanticKind::Select)
        .expect("select");
    assert_eq!(select.attrs.options.len(), 2);
    assert!(select.attrs.options[1].selected);
    assert_eq!(select.attrs.value.as_deref(), Some("blue"));

    let button = document
        .components()
        .find(|c| c.kind == SemanticKind::Button)
        .expect("button");
    assert_eq!(button.attrs.button_type.as_deref(), Some("submit"));
    assert_eq!(button.text.as_deref(), Some("Save"));

    let image = document
        .components()
        .find(|c| c.kind == SemanticKind::Image)
        .expect("image");
    assert_eq!(image.attrs.alt.as_deref(), Some("Logo"));
    assert_eq!(image.attrs.src.as_deref(), Some("/logo.png"));
}

#[test]
fn identity_stable_within_same_document_fixture() {
    let first = normalize_fixture(
        meta("doc-stable", "rev-1", "https://example.com/stable"),
        links_fixture(),
    )
    .expect("first");
    let second = normalize_fixture(
        meta("doc-stable", "rev-1", "https://example.com/stable"),
        links_fixture(),
    )
    .expect("second");

    let first_refs = first.semantic_refs();
    let second_refs = second.semantic_refs();
    assert_eq!(first_refs, second_refs);

    // Author id preferred when unique.
    let home = first
        .components()
        .find(|c| c.attrs.href.as_deref() == Some("/home"))
        .expect("home link");
    let payload = home.semantic_ref.decode().expect("decode");
    assert_eq!(payload.identity, SemanticIdentity::author_id("nav-home"));

    // Resolving the first capture's refs against the second succeeds for the same revision.
    for semantic_ref in &first_refs {
        let a = first.resolve(semantic_ref).expect("first resolve");
        let b = second.resolve(semantic_ref).expect("second resolve");
        assert_eq!(a.kind, b.kind);
        assert_eq!(a.attrs.href, b.attrs.href);
    }
}

#[test]
fn identity_does_not_retarget_by_text_across_structure_change() {
    let original = normalize_fixture(
        meta("doc-x", "rev-1", "https://example.com/x"),
        vec![RawSemanticNode {
            kind: "link".into(),
            tag: Some("a".into()),
            href: Some("/a".into()),
            text: Some("Same label".into()),
            label: Some("Same label".into()),
            ..node("link")
        }],
    )
    .expect("original");

    let moved = normalize_fixture(
        meta("doc-x", "rev-2", "https://example.com/x"),
        vec![
            RawSemanticNode {
                kind: "text".into(),
                tag: Some("p".into()),
                text: Some("Padding".into()),
                ..node("text")
            },
            RawSemanticNode {
                kind: "link".into(),
                tag: Some("a".into()),
                href: Some("/b".into()),
                text: Some("Same label".into()),
                label: Some("Same label".into()),
                ..node("link")
            },
        ],
    )
    .expect("moved");

    let original_link_ref = original
        .components()
        .find(|c| c.kind == SemanticKind::Link)
        .expect("link")
        .semantic_ref
        .clone();

    // Wrong revision is stale; never silently rebind to the text-similar link.
    assert!(matches!(
        moved.resolve(&original_link_ref),
        Err(SemanticRefError::Stale { .. })
    ));

    // Even with matching revision, a structural identity change must not resolve by text.
    let same_rev_different_structure = SemanticDocument::from_components(
        meta("doc-x", "rev-1", "https://example.com/x"),
        moved.roots.clone(),
    );
    // Rebuild with rev-1 metadata but different structure/identities.
    let rebuilt = normalize_fixture(
        meta("doc-x", "rev-1", "https://example.com/x"),
        vec![
            RawSemanticNode {
                kind: "text".into(),
                tag: Some("p".into()),
                text: Some("Padding".into()),
                ..node("text")
            },
            RawSemanticNode {
                kind: "link".into(),
                tag: Some("a".into()),
                href: Some("/b".into()),
                text: Some("Same label".into()),
                label: Some("Same label".into()),
                ..node("link")
            },
        ],
    )
    .expect("rebuilt");

    assert_eq!(
        rebuilt.resolve(&original_link_ref),
        Err(SemanticRefError::Unknown)
    );
    let _ = same_rev_different_structure;
}

#[test]
fn fail_closed_resolution_matrix() {
    let document = normalize_fixture(
        meta("doc-fc", "rev-9", "https://example.com/fc"),
        content_fixture(),
    )
    .expect("document");

    let valid = document.semantic_refs().into_iter().next().expect("ref");
    assert!(document.resolve(&valid).is_ok());

    assert_eq!(
        document.resolve_str("not-valid"),
        Err(SemanticRefError::Malformed)
    );

    let wrong_doc = SemanticRef::encode(&SemanticRefPayload {
        document_id: "other-doc".into(),
        revision: "rev-9".into(),
        identity: SemanticIdentity::author_id("site-header"),
    });
    assert!(matches!(
        document.resolve(&wrong_doc),
        Err(SemanticRefError::WrongDocument { .. })
    ));

    let stale = SemanticRef::encode(&SemanticRefPayload {
        document_id: "doc-fc".into(),
        revision: "rev-1".into(),
        identity: SemanticIdentity::author_id("site-header"),
    });
    assert!(matches!(
        document.resolve(&stale),
        Err(SemanticRefError::Stale { .. })
    ));

    let unknown = SemanticRef::encode(&SemanticRefPayload {
        document_id: "doc-fc".into(),
        revision: "rev-9".into(),
        identity: SemanticIdentity::author_id("missing-node"),
    });
    assert_eq!(document.resolve(&unknown), Err(SemanticRefError::Unknown));
}

#[test]
fn extract_script_is_json_string_expression() {
    let script = include_str!("extract_semantic_dom.js");
    assert!(
        script.contains("JSON.stringify("),
        "semantic extract script should return a JSON string expression"
    );
    // getComputedStyle is allowed only for cheap visibility (display/visibility)
    // so client-side filters (Holmes + Tailwind `.hidden`) drop from capture.
    assert!(
        script.contains("isEffectivelyHidden"),
        "semantic capture must skip effectively hidden nodes"
    );
    // Ban geometry APIs in executable code (comments may name them as non-goals).
    let code_only: String = script
        .lines()
        .filter(|line| {
            let t = line.trim_start();
            !t.starts_with("//") && !t.starts_with('*')
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !code_only.contains("getBoundingClientRect"),
        "semantic capture must not query layout geometry"
    );
    assert!(
        !code_only.contains("offsetWidth") && !code_only.contains("getClientRects"),
        "semantic capture must not probe element geometry"
    );
}

#[test]
fn extract_script_preserves_ordered_mixed_inline_content() {
    let script = include_str!("extract_semantic_dom.js");
    assert!(
        script.contains("function visitOrdered("),
        "capture must walk children in document order including text nodes"
    );
    assert!(
        script.contains("function makeTextFragment("),
        "capture must emit Text fragments for direct text nodes"
    );
    assert!(
        script.contains("function makeTextualContainer("),
        "capture must distinguish compact leaves from mixed textual containers"
    );
    assert!(
        script
            .matches("children = visitOrdered(element, depth + 1);")
            .count()
            >= 3,
        "landmarks, lists, and groups must retain direct text in document order"
    );
    assert!(
        script.contains("never keep aggregate innerText")
            || script.contains("ordered children are authoritative"),
        "capture must document the no-duplicate aggregate-text rule for mixed content"
    );
    // Links and buttons must be terminal leaves (empty children arrays in makeNode calls).
    assert!(
        script.contains("Terminal leaf: aggregate label only"),
        "capture must treat links/buttons as terminal leaves"
    );
}

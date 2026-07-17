//! Addressable semantic content lines for scrolling, selection, and copy.
//!
//! Built from the shared `SemanticDocument` without re-parsing HTML. Each line
//! carries the owning component's exact `semantic_ref` when addressable.

use crate::semantic::{
    SemanticComponent, SemanticDocument, SemanticKind, SemanticRatatuiView, SemanticRef,
};
use std::collections::HashSet;

/// One display line in the TUI content pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentLine {
    pub text: String,
    pub semantic_ref: Option<SemanticRef>,
    pub kind: Option<SemanticKind>,
    /// True when this is the first line of a collapsible block.
    pub block_start: bool,
}

/// Flatten a document into content lines, honoring collapsed refs.
pub fn build_content_lines(
    document: &SemanticDocument,
    collapsed: &HashSet<SemanticRef>,
) -> Vec<ContentLine> {
    let mut lines = Vec::new();
    for root in &document.roots {
        push_component(root, 0, collapsed, &mut lines);
    }
    lines
}

fn push_component(
    component: &SemanticComponent,
    depth: usize,
    collapsed: &HashSet<SemanticRef>,
    lines: &mut Vec<ContentLine>,
) {
    let indent = "  ".repeat(depth);
    let is_collapsed = collapsed.contains(&component.semantic_ref);

    match component.kind {
        SemanticKind::Landmark => {
            let role = landmark_name(component);
            let marker = if is_collapsed { "▸" } else { "▾" };
            lines.push(ContentLine {
                text: format!("{indent}{marker} [{role}]"),
                semantic_ref: Some(component.semantic_ref.clone()),
                kind: Some(component.kind),
                block_start: true,
            });
            if !is_collapsed {
                for child in &component.children {
                    push_component(child, depth + 1, collapsed, lines);
                }
            }
        }
        SemanticKind::Group => {
            if let Some(label) = component.label.as_deref().filter(|s| !s.is_empty()) {
                lines.push(ContentLine {
                    text: format!("{indent}[{label}]"),
                    semantic_ref: Some(component.semantic_ref.clone()),
                    kind: Some(component.kind),
                    block_start: true,
                });
            }
            if !is_collapsed {
                for child in &component.children {
                    push_component(child, depth + 1, collapsed, lines);
                }
            }
        }
        SemanticKind::Heading => {
            let level = component.attrs.heading_level.unwrap_or(2).clamp(1, 6);
            let hashes = "#".repeat(level as usize);
            let text = display_text(component);
            lines.push(ContentLine {
                text: format!("{indent}{hashes} {text}"),
                semantic_ref: Some(component.semantic_ref.clone()),
                kind: Some(component.kind),
                block_start: true,
            });
        }
        SemanticKind::Text => {
            let text = display_text(component);
            if !text.is_empty() {
                lines.push(ContentLine {
                    text: format!("{indent}{text}"),
                    semantic_ref: Some(component.semantic_ref.clone()),
                    kind: Some(component.kind),
                    block_start: true,
                });
            }
        }
        SemanticKind::List => {
            let ordered = component.attrs.ordered.unwrap_or(false);
            let marker = if is_collapsed { "▸" } else { "▾" };
            lines.push(ContentLine {
                text: format!("{indent}{marker} {}", if ordered { "ol" } else { "ul" }),
                semantic_ref: Some(component.semantic_ref.clone()),
                kind: Some(component.kind),
                block_start: true,
            });
            if !is_collapsed {
                let mut index = 1usize;
                for child in &component.children {
                    if child.kind == SemanticKind::ListItem {
                        let bullet = if ordered {
                            format!("{index}.")
                        } else {
                            "-".into()
                        };
                        lines.push(ContentLine {
                            text: format!("{indent}  {bullet} {}", display_text(child)),
                            semantic_ref: Some(child.semantic_ref.clone()),
                            kind: Some(child.kind),
                            block_start: true,
                        });
                        if !collapsed.contains(&child.semantic_ref) {
                            for nested in &child.children {
                                push_component(nested, depth + 2, collapsed, lines);
                            }
                        }
                        index += 1;
                    } else {
                        push_component(child, depth + 1, collapsed, lines);
                    }
                }
            }
        }
        SemanticKind::ListItem => {
            lines.push(ContentLine {
                text: format!("{indent}- {}", display_text(component)),
                semantic_ref: Some(component.semantic_ref.clone()),
                kind: Some(component.kind),
                block_start: true,
            });
            if !is_collapsed {
                for child in &component.children {
                    push_component(child, depth + 1, collapsed, lines);
                }
            }
        }
        SemanticKind::Link => {
            let label = display_text(component);
            let href = component.attrs.href.as_deref().unwrap_or("");
            lines.push(ContentLine {
                text: format!("{indent}[{label}]({href})"),
                semantic_ref: Some(component.semantic_ref.clone()),
                kind: Some(component.kind),
                block_start: true,
            });
        }
        SemanticKind::Image => {
            let alt = component
                .attrs
                .alt
                .as_deref()
                .or(component.label.as_deref())
                .unwrap_or("");
            let src = component.attrs.src.as_deref().unwrap_or("");
            lines.push(ContentLine {
                text: format!("{indent}![{alt}]({src})"),
                semantic_ref: Some(component.semantic_ref.clone()),
                kind: Some(component.kind),
                block_start: true,
            });
        }
        SemanticKind::Input => {
            let name = component.attrs.name.as_deref().unwrap_or("");
            let value = component.attrs.value.as_deref().unwrap_or("");
            let input_type = component.attrs.input_type.as_deref().unwrap_or("text");
            lines.push(ContentLine {
                text: format!("{indent}[input {input_type} name={name} value={value}]"),
                semantic_ref: Some(component.semantic_ref.clone()),
                kind: Some(component.kind),
                block_start: true,
            });
        }
        SemanticKind::Textarea => {
            let name = component.attrs.name.as_deref().unwrap_or("");
            let value = component.attrs.value.as_deref().unwrap_or("");
            lines.push(ContentLine {
                text: format!("{indent}[textarea name={name} value={value}]"),
                semantic_ref: Some(component.semantic_ref.clone()),
                kind: Some(component.kind),
                block_start: true,
            });
        }
        SemanticKind::Select => {
            let name = component.attrs.name.as_deref().unwrap_or("");
            let value = component.attrs.value.as_deref().unwrap_or("");
            lines.push(ContentLine {
                text: format!("{indent}[select name={name} value={value}]"),
                semantic_ref: Some(component.semantic_ref.clone()),
                kind: Some(component.kind),
                block_start: true,
            });
        }
        SemanticKind::Button => {
            let label = display_text(component);
            lines.push(ContentLine {
                text: format!("{indent}[button {label}]"),
                semantic_ref: Some(component.semantic_ref.clone()),
                kind: Some(component.kind),
                block_start: true,
            });
        }
    }
}

fn landmark_name(component: &SemanticComponent) -> String {
    component
        .attrs
        .landmark
        .map(|r| format!("{r:?}").to_ascii_lowercase())
        .or_else(|| component.label.clone())
        .unwrap_or_else(|| "landmark".into())
}

fn display_text(component: &SemanticComponent) -> String {
    component
        .text
        .as_deref()
        .or(component.label.as_deref())
        .unwrap_or("")
        .to_string()
}

/// First content line index for a semantic_ref, if present.
pub fn line_index_of(lines: &[ContentLine], semantic_ref: &SemanticRef) -> Option<usize> {
    lines
        .iter()
        .position(|l| l.semantic_ref.as_ref().is_some_and(|r| r == semantic_ref))
}

/// Collect focusable controls in document order.
pub fn focusable_refs(document: &SemanticDocument) -> Vec<SemanticRef> {
    document
        .components()
        .filter(|c| {
            c.is_focusable()
                && matches!(
                    c.kind,
                    SemanticKind::Input
                        | SemanticKind::Textarea
                        | SemanticKind::Select
                        | SemanticKind::Button
                        | SemanticKind::Link
                )
                && !c.attrs.disabled.unwrap_or(false)
        })
        .map(|c| c.semantic_ref.clone())
        .collect()
}

/// Form controls only (for `gi` / Tab form traversal).
pub fn form_control_refs(document: &SemanticDocument) -> Vec<SemanticRef> {
    document
        .components()
        .filter(|c| {
            matches!(
                c.kind,
                SemanticKind::Input
                    | SemanticKind::Textarea
                    | SemanticKind::Select
                    | SemanticKind::Button
            ) && !c.attrs.disabled.unwrap_or(false)
        })
        .map(|c| c.semantic_ref.clone())
        .collect()
}

/// Links in document order (for hints over the full document; viewport filter applied later).
#[allow(dead_code)]
pub fn link_components(document: &SemanticDocument) -> Vec<&SemanticComponent> {
    document
        .components()
        .filter(|c| c.kind == SemanticKind::Link)
        .collect()
}

/// Rendered plain text for a block (copy target).
pub fn rendered_block_text(
    document: &SemanticDocument,
    semantic_ref: &SemanticRef,
) -> Option<String> {
    let component = document.resolve(semantic_ref).ok()?;
    let collapsed = HashSet::new();
    let mut lines = Vec::new();
    push_component(component, 0, &collapsed, &mut lines);
    Some(
        lines
            .into_iter()
            .map(|l| l.text)
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// Search content lines for a query; returns matching semantic_refs in order (exact ref ownership).
pub fn search_refs(lines: &[ContentLine], query: &str) -> Vec<SemanticRef> {
    if query.is_empty() {
        return Vec::new();
    }
    let q = query.to_ascii_lowercase();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for line in lines {
        if line.text.to_ascii_lowercase().contains(&q)
            && let Some(r) = &line.semantic_ref
            && seen.insert(r.clone())
        {
            out.push(r.clone());
        }
    }
    out
}

/// Ensure chrome/render helpers never emit shortcut legend text.
#[cfg(test)]
pub fn contains_shortcut_legend(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    // Heuristic: classic legend fragments must not appear in chrome.
    lower.contains("keys:")
        || lower.contains("shortcuts:")
        || lower.contains("key bindings")
        || (lower.contains("j/k") && lower.contains("scroll"))
}

/// Optional helper: lightweight lines from the pure Phase 3 ratatui view (no scroll state).
#[allow(dead_code)]
pub fn ratatui_inspection_lines(document: &SemanticDocument) -> Vec<String> {
    SemanticRatatuiView::new(document).lines()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::DocumentMetadata;
    use crate::semantic::normalize::{RawSemanticNode, normalize_fixture};

    fn meta() -> DocumentMetadata {
        DocumentMetadata {
            document_id: "doc".into(),
            revision: "1".into(),
            url: "https://example.com/".into(),
            title: "Example".into(),
            ready_state: "complete".into(),
            frames: vec![],
        }
    }

    #[test]
    fn content_lines_carry_exact_refs() {
        let doc = normalize_fixture(
            meta(),
            vec![RawSemanticNode {
                kind: "link".into(),
                tag: Some("a".into()),
                id: Some("home".into()),
                unique_id: true,
                text: Some("Home".into()),
                href: Some("/".into()),
                landmark: None,
                heading_level: None,
                ordered: None,
                label: None,
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
                options: vec![],
                children: vec![],
            }],
        )
        .expect("doc");
        let lines = build_content_lines(&doc, &HashSet::new());
        assert!(!lines.is_empty());
        let r = lines[0].semantic_ref.as_ref().expect("ref");
        assert!(doc.resolve(r).is_ok());
    }

    #[test]
    fn ordinary_press_content_is_not_a_shortcut_legend() {
        assert!(!contains_shortcut_legend("Press releases"));
    }
}

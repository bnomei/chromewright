//! Addressable semantic content lines for scrolling, selection, and copy.
//!
//! Built from the published SemanticDocument without re-parsing HTML. Each line
//! carries the owning component's exact `semantic_ref` when addressable so
//! selection, hints, search, and collapse stay revision-scoped.

use crate::semantic::{
    SemanticComponent, SemanticDocument, SemanticKind, SemanticRatatuiView, SemanticRef,
};
use std::collections::HashSet;

/// One display line in the TUI content pane.
///
/// `semantic_ref` is the selection/copy/hint identity for that line when the
/// component is addressable; structural padding lines may omit it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentLine {
    pub text: String,
    pub semantic_ref: Option<SemanticRef>,
    pub kind: Option<SemanticKind>,
    /// True when this is the first line of a collapsible block.
    pub block_start: bool,
}

/// Flatten a SemanticDocument into content lines, honoring collapsed exact refs.
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

/// Soft-wrap content lines to `width` columns (Unicode scalar count).
///
/// Prefers breaks at whitespace; falls back to a hard split when a single token
/// exceeds the width. Continuation rows keep the same `semantic_ref` / `kind`
/// and set `block_start = false` so selection still addresses the component.
///
/// `width == 0` is treated as 1 so callers never produce empty geometry.
pub fn wrap_content_lines(lines: &[ContentLine], width: usize) -> Vec<ContentLine> {
    let width = width.max(1);
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        wrap_one_line(line, width, &mut out);
    }
    out
}

fn wrap_one_line(line: &ContentLine, width: usize, out: &mut Vec<ContentLine>) {
    let chars: Vec<char> = line.text.chars().collect();
    if chars.is_empty() {
        out.push(ContentLine {
            text: String::new(),
            semantic_ref: line.semantic_ref.clone(),
            kind: line.kind,
            block_start: line.block_start,
        });
        return;
    }

    let mut start = 0usize;
    let mut first = true;
    while start < chars.len() {
        let remaining = chars.len() - start;
        if remaining <= width {
            push_wrapped_segment(line, &chars[start..], first, out);
            break;
        }

        // Prefer the last whitespace break inside the window.
        let window = &chars[start..start + width];
        let break_at = window
            .iter()
            .rposition(|c| c.is_whitespace())
            .filter(|&idx| idx > 0)
            .unwrap_or(width);

        push_wrapped_segment(line, &chars[start..start + break_at], first, out);
        first = false;
        start += break_at;
        // Drop a single leading space on the next row after a word break.
        if start < chars.len() && chars[start] == ' ' {
            start += 1;
        }
    }
}

fn push_wrapped_segment(
    line: &ContentLine,
    segment: &[char],
    first: bool,
    out: &mut Vec<ContentLine>,
) {
    out.push(ContentLine {
        text: segment.iter().collect(),
        semantic_ref: line.semantic_ref.clone(),
        kind: line.kind,
        block_start: first && line.block_start,
    });
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
                selector: None,
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

    #[test]
    fn wrap_breaks_on_spaces_and_preserves_refs() {
        let line = ContentLine {
            text: "hello beautiful world".into(),
            semantic_ref: Some(SemanticRef::from_opaque("r1")),
            kind: Some(SemanticKind::Text),
            block_start: true,
        };
        // "hello " is 6; width 10 → "hello" then "beautiful" then "world"
        let wrapped = wrap_content_lines(std::slice::from_ref(&line), 10);
        assert!(wrapped.len() >= 2);
        assert_eq!(wrapped[0].text, "hello");
        assert!(wrapped[0].block_start);
        for row in &wrapped {
            assert_eq!(row.semantic_ref.as_ref().map(|r| r.as_str()), Some("r1"));
        }
        assert!(!wrapped[1].block_start);
        assert!(wrapped.iter().all(|r| r.text.chars().count() <= 10));
    }

    #[test]
    fn wrap_hard_splits_overlong_tokens() {
        let line = ContentLine {
            text: "abcdefghij".into(),
            semantic_ref: None,
            kind: Some(SemanticKind::Text),
            block_start: true,
        };
        let wrapped = wrap_content_lines(std::slice::from_ref(&line), 4);
        assert_eq!(
            wrapped
                .iter()
                .map(|l| l.text.as_str())
                .collect::<Vec<_>>(),
            vec!["abcd", "efgh", "ij"]
        );
    }
}

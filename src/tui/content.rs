//! Addressable semantic content lines for scrolling, selection, and copy.
//!
//! Built from the published SemanticDocument without re-parsing HTML. Each line
//! carries the owning component's exact `semantic_ref` when addressable so
//! selection, hints, search, and collapse stay revision-scoped.

use crate::semantic::{
    SemanticComponent, SemanticDocument, SemanticKind, SemanticRatatuiView, SemanticRef,
};
use std::collections::{HashMap, HashSet};

/// Live form values overlaid on content lines (staged Tab values + active edit).
///
/// Keys are exact `semantic_ref`s. When present they replace the captured
/// `attrs.value` / checked state for display only (DOM write still happens on
/// submit).
pub type FormValueOverlay = HashMap<SemanticRef, String>;

/// One display line in the TUI content pane.
///
/// `semantic_ref` is the selection/copy/hint identity for that line when the
/// component is addressable; structural padding lines may omit it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentLine {
    pub text: String,
    pub semantic_ref: Option<SemanticRef>,
    pub kind: Option<SemanticKind>,
    /// Heading level 1–6 when `kind` is Heading (for theme hierarchy).
    pub heading_level: Option<u8>,
    /// True when this is the first line of a collapsible block.
    pub block_start: bool,
}

/// How structural containers are projected into content lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ContentProjection {
    /// Markdown-like: hide landmark/list/group chrome, fully flat indent.
    /// Default reading mode.
    #[default]
    Prose,
    /// DOM-like: show `▾ [main]`, `▾ ol`, group labels, and depth indent.
    Structure,
}

impl ContentProjection {
    pub fn is_prose(self) -> bool {
        matches!(self, Self::Prose)
    }

    pub fn is_structure(self) -> bool {
        matches!(self, Self::Structure)
    }
}

/// Flatten a SemanticDocument into content lines, honoring collapsed exact refs.
///
/// Defaults to structure projection for callers/tests that want DOM chrome.
#[cfg_attr(not(test), allow(dead_code))]
pub fn build_content_lines(
    document: &SemanticDocument,
    collapsed: &HashSet<SemanticRef>,
) -> Vec<ContentLine> {
    build_content_lines_with(document, collapsed, ContentProjection::Structure)
}

/// Flatten with an explicit projection (prose vs structure).
pub fn build_content_lines_with(
    document: &SemanticDocument,
    collapsed: &HashSet<SemanticRef>,
    projection: ContentProjection,
) -> Vec<ContentLine> {
    build_content_lines_with_overlay(document, collapsed, projection, None, None)
}

/// Flatten with optional live form overlay (staged values + active edit buffer).
pub fn build_content_lines_with_overlay(
    document: &SemanticDocument,
    collapsed: &HashSet<SemanticRef>,
    projection: ContentProjection,
    form_values: Option<&FormValueOverlay>,
    editing_ref: Option<&SemanticRef>,
) -> Vec<ContentLine> {
    let mut lines = Vec::new();
    push_siblings(&document.roots, 0, collapsed, projection, &mut lines);
    if form_values.is_some() || editing_ref.is_some() {
        apply_form_value_overlay(&mut lines, document, form_values, editing_ref);
    }
    lines
}

/// Walk siblings with lookahead so heading spacing can see the next node.
fn push_siblings(
    children: &[SemanticComponent],
    depth: usize,
    collapsed: &HashSet<SemanticRef>,
    projection: ContentProjection,
    lines: &mut Vec<ContentLine>,
) {
    for (index, child) in children.iter().enumerate() {
        let next = children.get(index + 1);
        push_component(child, depth, collapsed, projection, next, lines);
    }
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
            heading_level: line.heading_level,
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
        heading_level: line.heading_level,
        block_start: first && line.block_start,
    });
}

fn push_line(
    lines: &mut Vec<ContentLine>,
    text: String,
    component: &SemanticComponent,
    block_start: bool,
) {
    lines.push(ContentLine {
        text,
        semantic_ref: Some(component.semantic_ref.clone()),
        kind: Some(component.kind),
        heading_level: component.attrs.heading_level,
        block_start,
    });
}

/// Blank spacer line for prose readability.
///
/// Never carries a `semantic_ref`, so j/k / search / hints skip these rows.
fn push_blank_lines(lines: &mut Vec<ContentLine>, count: usize) {
    for _ in 0..count {
        lines.push(ContentLine {
            text: String::new(),
            semantic_ref: None,
            kind: None,
            heading_level: None,
            block_start: false,
        });
    }
}

fn has_selectable_line(lines: &[ContentLine]) -> bool {
    lines.iter().any(|l| l.semantic_ref.is_some())
}

/// Blank (non-selectable) lines already trailing at the end of `lines`.
fn trailing_blank_count(lines: &[ContentLine]) -> usize {
    lines
        .iter()
        .rev()
        .take_while(|l| l.semantic_ref.is_none() && l.text.is_empty())
        .count()
}

/// Ensure at least `needed` blank lines before the next content line (prose only).
fn ensure_blank_before(lines: &mut Vec<ContentLine>, needed: usize) {
    if needed == 0 || !has_selectable_line(lines) {
        return;
    }
    let have = trailing_blank_count(lines);
    if have < needed {
        push_blank_lines(lines, needed - have);
    }
}

fn heading_level_of(component: &SemanticComponent) -> Option<u8> {
    if component.kind == SemanticKind::Heading {
        Some(component.attrs.heading_level.unwrap_or(2).clamp(1, 6))
    } else {
        None
    }
}

/// Spacing above a heading in prose: h1/h2 → 2, h3+ → 1 (none at document start).
fn blanks_before_heading(level: u8) -> usize {
    match level {
        1 | 2 => 2,
        _ => 1,
    }
}

fn push_component(
    component: &SemanticComponent,
    depth: usize,
    collapsed: &HashSet<SemanticRef>,
    projection: ContentProjection,
    next_sibling: Option<&SemanticComponent>,
    lines: &mut Vec<ContentLine>,
) {
    // Prose is fully flat; structure keeps DOM-like depth indent.
    let depth = match projection {
        ContentProjection::Prose => 0,
        ContentProjection::Structure => depth,
    };
    let indent = "  ".repeat(depth);
    let is_collapsed = collapsed.contains(&component.semantic_ref);

    match component.kind {
        SemanticKind::Landmark => {
            if projection.is_structure() {
                let role = landmark_name(component);
                let marker = if is_collapsed { "▸" } else { "▾" };
                push_line(
                    lines,
                    format!("{indent}{marker} [{role}]"),
                    component,
                    true,
                );
            }
            // Prose: omit landmark chrome; still walk children (collapse only in structure).
            if projection.is_prose() || !is_collapsed {
                push_siblings(
                    &component.children,
                    depth + 1,
                    collapsed,
                    projection,
                    lines,
                );
            }
        }
        SemanticKind::Group => {
            if projection.is_structure()
                && let Some(label) = component.label.as_deref().filter(|s| !s.is_empty())
            {
                push_line(lines, format!("{indent}[{label}]"), component, true);
            }
            if projection.is_prose() || !is_collapsed {
                push_siblings(
                    &component.children,
                    depth + 1,
                    collapsed,
                    projection,
                    lines,
                );
            }
        }
        SemanticKind::Heading => {
            let level = component.attrs.heading_level.unwrap_or(2).clamp(1, 6);
            let hashes = "#".repeat(level as usize);
            let text = display_text(component);
            // Heading breathing room is prose-only; structure stays dense/DOM-like.
            // Spacers never get a semantic_ref (not selectable).
            if projection.is_prose() {
                ensure_blank_before(lines, blanks_before_heading(level));
            }
            push_line(lines, format!("{indent}{hashes} {text}"), component, true);
            // Two blank lines after h1 unless the next sibling is an h2
            // (h2 already brings two lines of leading space).
            if projection.is_prose() && level == 1 {
                let next_is_h2 = next_sibling.is_some_and(|n| heading_level_of(n) == Some(2));
                if !next_is_h2 {
                    push_blank_lines(lines, 2usize.saturating_sub(trailing_blank_count(lines)));
                }
            }
        }
        SemanticKind::Text => {
            let text = display_text(component);
            if !text.is_empty() {
                push_line(lines, format!("{indent}{text}"), component, true);
            }
        }
        SemanticKind::List => {
            let ordered = component.attrs.ordered.unwrap_or(false);
            if projection.is_structure() {
                let marker = if is_collapsed { "▸" } else { "▾" };
                push_line(
                    lines,
                    format!(
                        "{indent}{marker} {}",
                        if ordered { "ol" } else { "ul" }
                    ),
                    component,
                    true,
                );
            }
            if projection.is_prose() || !is_collapsed {
                let mut index = 1usize;
                let children = &component.children;
                for (child_index, child) in children.iter().enumerate() {
                    let next = children.get(child_index + 1);
                    if child.kind == SemanticKind::ListItem {
                        let bullet = if ordered {
                            format!("{index}.")
                        } else {
                            "-".into()
                        };
                        // Fully flat in prose (no nested list indent).
                        push_line(
                            lines,
                            format!("{indent}{bullet} {}", display_text(child)),
                            child,
                            true,
                        );
                        let item_collapsed = collapsed.contains(&child.semantic_ref);
                        if projection.is_prose() || !item_collapsed {
                            push_siblings(
                                &child.children,
                                depth + 1,
                                collapsed,
                                projection,
                                lines,
                            );
                        }
                        index += 1;
                        let _ = next; // list items use sibling walk only for nested non-items
                    } else {
                        push_component(child, depth + 1, collapsed, projection, next, lines);
                    }
                }
            }
        }
        SemanticKind::ListItem => {
            // Reached when a list item is a root or nested outside List handling.
            push_line(
                lines,
                format!("{indent}- {}", display_text(component)),
                component,
                true,
            );
            if projection.is_prose() || !is_collapsed {
                push_siblings(
                    &component.children,
                    depth + 1,
                    collapsed,
                    projection,
                    lines,
                );
            }
        }
        SemanticKind::Link => {
            let label = display_text(component);
            let href = fold_media_url(component.attrs.href.as_deref().unwrap_or(""));
            push_line(lines, format!("{indent}[{label}]({href})"), component, true);
        }
        SemanticKind::Image => {
            let alt = component
                .attrs
                .alt
                .as_deref()
                .or(component.label.as_deref())
                .unwrap_or("");
            let src = fold_media_url(component.attrs.src.as_deref().unwrap_or(""));
            push_line(lines, format!("{indent}![{alt}]({src})"), component, true);
        }
        SemanticKind::Input
        | SemanticKind::Textarea
        | SemanticKind::Select
        | SemanticKind::Button => {
            let text = format_control_line(component, indent.as_str(), None, false);
            push_line(lines, text, component, true);
        }
    }
}

/// First visible descendant ref in document order under `root` that appears in `lines`.
pub fn first_visible_descendant_ref(
    document: &SemanticDocument,
    root: &SemanticRef,
    lines: &[ContentLine],
) -> Option<SemanticRef> {
    let component = document.resolve(root).ok()?;
    let mut found = None;
    component.walk(&mut |c| {
        if found.is_some() {
            return;
        }
        if line_index_of(lines, &c.semantic_ref).is_some() {
            found = Some(c.semantic_ref.clone());
        }
    });
    found
}

/// Exact `root` plus every descendant ref (document order walk).
///
/// Used so agent attention on a prose-hidden landmark still paints/scrolls via
/// its visible children.
pub fn subtree_refs(document: &SemanticDocument, root: &SemanticRef) -> HashSet<SemanticRef> {
    let mut out = HashSet::new();
    let Ok(component) = document.resolve(root) else {
        return out;
    };
    component.walk(&mut |c| {
        out.insert(c.semantic_ref.clone());
    });
    out
}

fn landmark_name(component: &SemanticComponent) -> String {
    component
        .attrs
        .landmark
        .map(|r| format!("{r:?}").to_ascii_lowercase())
        .or_else(|| component.label.clone())
        .unwrap_or_else(|| "landmark".into())
}

/// Rewrite control lines using live staged/edit values when present.
fn apply_form_value_overlay(
    lines: &mut [ContentLine],
    document: &SemanticDocument,
    form_values: Option<&FormValueOverlay>,
    editing_ref: Option<&SemanticRef>,
) {
    for line in lines.iter_mut() {
        let Some(r) = line.semantic_ref.as_ref() else {
            continue;
        };
        let Ok(component) = document.resolve(r) else {
            continue;
        };
        if !matches!(
            component.kind,
            SemanticKind::Input
                | SemanticKind::Textarea
                | SemanticKind::Select
                | SemanticKind::Button
        ) {
            continue;
        }
        // Preserve leading indent from the existing line.
        let indent_len = line.text.chars().take_while(|c| *c == ' ').count();
        let indent: String = line.text.chars().take(indent_len).collect();
        let live = form_values.and_then(|m| m.get(r)).map(String::as_str);
        let editing = editing_ref.is_some_and(|e| e == r);
        line.text = format_control_line(component, &indent, live, editing);
    }
}

/// Compact inline rendering for form controls.
///
/// `live_value` overrides the captured attr when the operator has staged/edited
/// text. `editing` marks the active field with a cursor.
fn format_control_line(
    component: &SemanticComponent,
    indent: &str,
    live_value: Option<&str>,
    editing: bool,
) -> String {
    let name = component.attrs.name.as_deref().unwrap_or("");
    let label = component
        .label
        .as_deref()
        .or(component.text.as_deref())
        .unwrap_or("")
        .trim();
    let captured = component.attrs.value.as_deref().unwrap_or("");
    let value = live_value.unwrap_or(captured);
    let cursor = if editing { "█" } else { "" };
    let name_bit = if name.is_empty() {
        String::new()
    } else {
        format!(" {name}")
    };

    match component.kind {
        SemanticKind::Button => {
            let text = if label.is_empty() { "Button" } else { label };
            format!("{indent}[{text}]")
        }
        SemanticKind::Select => {
            let shown = if value.is_empty() {
                select_display_label(component).unwrap_or_else(|| "…".into())
            } else {
                select_label_for_value(component, value).unwrap_or_else(|| value.to_string())
            };
            if editing {
                format!("{indent}[select{name_bit}: {shown}{cursor} ▾]")
            } else {
                format!("{indent}[select{name_bit}: {shown} ▾]")
            }
        }
        SemanticKind::Textarea => {
            let shown = if value.is_empty() && !editing {
                "…".to_string()
            } else {
                format!("{value}{cursor}")
            };
            if label.is_empty() {
                format!("{indent}[textarea{name_bit}: {shown}]")
            } else {
                format!("{indent}[textarea {label}{name_bit}: {shown}]")
            }
        }
        SemanticKind::Input => {
            let input_type = component
                .attrs
                .input_type
                .as_deref()
                .unwrap_or("text")
                .to_ascii_lowercase();
            match input_type.as_str() {
                "checkbox" => {
                    let checked = live_checked(component, live_value);
                    let mark = if checked { "☑" } else { "☐" };
                    let tail = if label.is_empty() {
                        name_bit
                    } else {
                        format!(" {label}")
                    };
                    format!("{indent}{mark}{tail}")
                }
                "radio" => {
                    let checked = live_checked(component, live_value);
                    let mark = if checked { "●" } else { "○" };
                    let tail = if label.is_empty() {
                        name_bit
                    } else {
                        format!(" {label}")
                    };
                    format!("{indent}{mark}{tail}")
                }
                "submit" | "button" | "reset" => {
                    let text = if !value.is_empty() {
                        value.to_string()
                    } else if !label.is_empty() {
                        label.to_string()
                    } else {
                        input_type.to_string()
                    };
                    format!("{indent}[{text}]")
                }
                "password" => {
                    let dots: String = value.chars().map(|_| '•').collect();
                    let shown = if editing {
                        format!("{dots}{cursor}")
                    } else if dots.is_empty() {
                        "…".into()
                    } else {
                        dots
                    };
                    format!("{indent}[password{name_bit}: {shown}]")
                }
                other => {
                    let shown = if value.is_empty() && !editing {
                        component
                            .attrs
                            .placeholder
                            .as_deref()
                            .filter(|p| !p.is_empty())
                            .map(|p| format!("({p})"))
                            .unwrap_or_else(|| "…".into())
                    } else {
                        format!("{value}{cursor}")
                    };
                    let type_bit = if other == "text" || other.is_empty() {
                        String::new()
                    } else {
                        format!(" {other}")
                    };
                    if label.is_empty() {
                        format!("{indent}[input{type_bit}{name_bit}: {shown}]")
                    } else {
                        format!("{indent}[input {label}{type_bit}{name_bit}: {shown}]")
                    }
                }
            }
        }
        _ => format!("{indent}[control]"),
    }
}

fn live_checked(component: &SemanticComponent, live_value: Option<&str>) -> bool {
    if let Some(v) = live_value {
        return matches!(v, "true" | "1" | "on" | "yes");
    }
    component.attrs.checked == Some(true)
}

fn select_display_label(component: &SemanticComponent) -> Option<String> {
    component
        .attrs
        .options
        .iter()
        .find(|o| o.selected)
        .map(|o| {
            o.label
                .clone()
                .filter(|l| !l.is_empty())
                .unwrap_or_else(|| o.value.clone())
        })
}

fn select_label_for_value(component: &SemanticComponent, value: &str) -> Option<String> {
    component.attrs.options.iter().find(|o| o.value == value).map(|o| {
        o.label
            .clone()
            .filter(|l| !l.is_empty())
            .unwrap_or_else(|| o.value.clone())
    })
}

fn display_text(component: &SemanticComponent) -> String {
    component
        .text
        .as_deref()
        .or(component.label.as_deref())
        .unwrap_or("")
        .to_string()
}

/// Collapse inline `data:` / base64 media URLs so content lines stay readable.
///
/// Examples:
/// - `data:image/png;base64,iVBOR…` → `base64,…`
/// - `data:image/svg+xml,…` → `data:…`
/// - ordinary `https://…` / relative paths unchanged
fn fold_media_url(src: &str) -> String {
    let trimmed = src.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("data:") {
        if lower.contains(";base64,") || lower.contains(";base64;") {
            return "base64,…".into();
        }
        return "data:…".into();
    }
    // Bare base64 blobs occasionally appear without a data: prefix.
    if trimmed.len() > 64 && looks_like_base64_blob(trimmed) {
        return "base64,…".into();
    }
    trimmed.to_string()
}

fn looks_like_base64_blob(s: &str) -> bool {
    let mut saw_payload = false;
    for (i, b) in s.bytes().enumerate() {
        let ok = b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'=' | b'\n' | b'\r');
        if !ok {
            return false;
        }
        if b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/') {
            saw_payload = true;
        }
        // Padding only at end.
        if b == b'=' && i + 2 < s.len() {
            // allow == at end; reject = mid-string roughly
            let rest = &s.as_bytes()[i..];
            if !rest.iter().all(|c| matches!(c, b'=' | b'\n' | b'\r')) {
                return false;
            }
            break;
        }
    }
    saw_payload
}

/// First content line index for a semantic_ref, if present.
pub fn line_index_of(lines: &[ContentLine], semantic_ref: &SemanticRef) -> Option<usize> {
    lines
        .iter()
        .position(|l| l.semantic_ref.as_ref().is_some_and(|r| r == semantic_ref))
}

/// Top-of-viewport anchor used to restore scroll after a same-document patch.
///
/// `semantic_ref` is the first addressable line at or below the current top.
/// `wrap_row` is how many display rows of that block sit above the viewport
/// edge (0 when the block start is on-screen). After recapture, restore with
/// `scroll_y = line_index_of(new_lines, rebound) + wrap_row`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewportTopAnchor {
    pub semantic_ref: SemanticRef,
    pub wrap_row: usize,
}

/// Capture the topmost visible addressable line for scroll-stable patches.
///
/// Skips non-addressable rows (prose heading spacers). When the top row is a
/// soft-wrap continuation, walks upward to the block start and records the
/// wrap offset so the same continuation can be restored after recapture.
pub fn top_viewport_anchor(lines: &[ContentLine], scroll_y: usize) -> Option<ViewportTopAnchor> {
    if lines.is_empty() {
        return None;
    }
    let start = scroll_y.min(lines.len().saturating_sub(1));
    let mut idx = start;
    while idx < lines.len() && lines[idx].semantic_ref.is_none() {
        idx += 1;
    }
    let line = lines.get(idx)?;
    let semantic_ref = line.semantic_ref.clone()?;
    // Distance from this component's first display row to the viewport top.
    let block_start = line_index_of(lines, &semantic_ref).unwrap_or(idx);
    let wrap_row = start.saturating_sub(block_start);
    Some(ViewportTopAnchor {
        semantic_ref,
        wrap_row,
    })
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
    // Links and images copy their URL (resolved against the page when relative),
    // not the full markdown-ish element rendering.
    if let Some(url) = copy_url_for_component(document, component) {
        return Some(url);
    }
    let collapsed = HashSet::new();
    let mut lines = Vec::new();
    // Copy uses structure projection so container labels remain available.
    push_component(
        component,
        0,
        &collapsed,
        ContentProjection::Structure,
        None,
        &mut lines,
    );
    Some(
        lines
            .into_iter()
            .map(|l| l.text)
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// Clipboard payload for link/image selection: resolved href or src.
///
/// Returns `None` for other kinds (caller falls back to rendered block text).
/// Empty/missing URLs also return `None`. Data/base64 media stays full so the
/// clipboard can carry the real URL (display folding is separate).
fn copy_url_for_component(
    document: &SemanticDocument,
    component: &SemanticComponent,
) -> Option<String> {
    let raw = match component.kind {
        SemanticKind::Link => component.attrs.href.as_deref(),
        SemanticKind::Image => component.attrs.src.as_deref(),
        _ => None,
    }?
    .trim();
    if raw.is_empty() {
        return None;
    }
    Some(resolve_copy_url(document.document.url.as_str(), raw))
}

/// Resolve a possibly-relative link/image URL against the document base.
fn resolve_copy_url(base: &str, href: &str) -> String {
    if href.starts_with("http://")
        || href.starts_with("https://")
        || href.starts_with("about:")
        || href.starts_with("data:")
        || href.starts_with("file:")
        || href.starts_with("mailto:")
        || href.starts_with("tel:")
        || href.starts_with("blob:")
    {
        return href.to_string();
    }
    if href.starts_with("//") {
        let scheme = if base.starts_with("https") {
            "https:"
        } else {
            "http:"
        };
        return format!("{scheme}{href}");
    }
    if href.starts_with('#') {
        // Fragment-only: keep page URL and replace/append fragment.
        if let Some(hash) = base.find('#') {
            return format!("{}{href}", &base[..hash]);
        }
        return format!("{base}{href}");
    }
    if href.starts_with('/')
        && let Some(origin_end) = base.find("://")
    {
        let after = &base[origin_end + 3..];
        let host_end = after
            .find('/')
            .map(|i| origin_end + 3 + i)
            .unwrap_or(base.len());
        return format!("{}{}", &base[..host_end], href);
    }
    // Relative to current path directory.
    if let Some(slash) = base.rfind('/') {
        return format!("{}{}", &base[..=slash], href);
    }
    href.to_string()
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

/// Max chars for inspect label/text/value snippets (grapheme-ish: Unicode scalar values).
const INSPECT_CLIP: usize = 48;

/// Compact developer inspect panel body for the TUI (`i`).
///
/// CSS-selector-first layout, 1–3 lines, no `None`/`Some` noise:
/// 1. identity (`tag#id`) + short quoted label
/// 2. kind-specific action fields (omitted when empty)
/// 3. opaque ref + document revision
///
/// Panel title is the full DOM path from [`format_inspect_dom_path`].
pub fn format_inspect_panel(component: &SemanticComponent, revision: &str) -> String {
    let mut lines = Vec::with_capacity(3);

    let identity = inspect_identity(component);
    let label = inspect_label(component);
    lines.push(match label {
        Some(q) => format!("{identity}  {q}"),
        None => identity,
    });

    if let Some(action) = inspect_action_line(component) {
        lines.push(action);
    }

    lines.push(format!(
        "{}  rev={}",
        component.semantic_ref.as_str(),
        revision
    ));

    lines.join("\n")
}

/// Full CSS-selector-style path from document root to `component` (inclusive).
///
/// Example: `main > form#signup > input#email`
pub fn format_inspect_dom_path(
    document: &SemanticDocument,
    component: &SemanticComponent,
) -> String {
    let mut segments = Vec::new();
    for ancestor_ref in document.ancestor_refs(&component.semantic_ref) {
        if let Ok(ancestor) = document.resolve(&ancestor_ref) {
            segments.push(inspect_path_segment(ancestor));
        }
    }
    segments.push(inspect_path_segment(component));
    if segments.is_empty() {
        inspect_path_segment(component)
    } else {
        segments.join(" > ")
    }
}

/// Truncate a DOM path from the left so the leaf stays visible.
///
/// `main > … > form#x > input#y` when the full path exceeds `max_chars`.
pub fn truncate_inspect_dom_path(path: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let count = path.chars().count();
    if count <= max_chars {
        return path.to_string();
    }
    if max_chars <= 1 {
        return "…".to_string();
    }
    // Prefer keeping trailing segments after a leading ellipsis.
    let ellipsis = "…";
    let budget = max_chars.saturating_sub(ellipsis.chars().count() + 1); // "… "
    if budget == 0 {
        return "…".to_string();
    }
    let parts: Vec<&str> = path.split(" > ").collect();
    if parts.is_empty() {
        return clip_inspect(path, max_chars);
    }
    let mut kept: Vec<&str> = Vec::new();
    let mut used = 0usize;
    for part in parts.iter().rev() {
        let add = part.chars().count() + if kept.is_empty() { 0 } else { 3 }; // " > "
        if used + add > budget && !kept.is_empty() {
            break;
        }
        if used + add > budget && kept.is_empty() {
            // Single segment still too long: hard clip the leaf.
            return format!("{ellipsis} {}", clip_inspect(part, budget.saturating_sub(1)));
        }
        kept.push(part);
        used += add;
    }
    kept.reverse();
    format!("{ellipsis} {}", kept.join(" > "))
}

/// Path segment without kind annotations (tag#id only).
fn inspect_path_segment(component: &SemanticComponent) -> String {
    let mut id = String::new();
    if let Some(tag) = component.attrs.tag.as_deref().filter(|t| !t.is_empty()) {
        id.push_str(tag);
    } else {
        id.push_str(inspect_kind_fallback(component));
    }
    if let Some(eid) = component
        .attrs
        .element_id
        .as_deref()
        .filter(|s| !s.is_empty())
    {
        id.push('#');
        id.push_str(eid);
    }
    id
}

fn inspect_identity(component: &SemanticComponent) -> String {
    let mut id = String::new();
    if let Some(tag) = component.attrs.tag.as_deref().filter(|t| !t.is_empty()) {
        id.push_str(tag);
    } else {
        id.push_str(inspect_kind_fallback(component));
    }
    if let Some(eid) = component
        .attrs
        .element_id
        .as_deref()
        .filter(|s| !s.is_empty())
    {
        id.push('#');
        id.push_str(eid);
    }
    // When tag already conveys kind (a/input/button/…), skip redundant kind token.
    if inspect_kind_needed(component) {
        id.push(' ');
        id.push_str(inspect_kind_token(component));
    }
    id
}

fn inspect_kind_fallback(component: &SemanticComponent) -> &'static str {
    match component.kind {
        SemanticKind::Landmark => landmark_role_str(component).unwrap_or("landmark"),
        SemanticKind::Heading => "heading",
        SemanticKind::Text => "text",
        SemanticKind::List => "list",
        SemanticKind::ListItem => "li",
        SemanticKind::Link => "a",
        SemanticKind::Image => "img",
        SemanticKind::Input => "input",
        SemanticKind::Textarea => "textarea",
        SemanticKind::Select => "select",
        SemanticKind::Button => "button",
        SemanticKind::Group => "group",
    }
}

fn inspect_kind_token(component: &SemanticComponent) -> &'static str {
    match component.kind {
        SemanticKind::Landmark => landmark_role_str(component).unwrap_or("landmark"),
        SemanticKind::Heading => "heading",
        SemanticKind::Text => "text",
        SemanticKind::List => {
            if component.attrs.ordered == Some(true) {
                "ol"
            } else {
                "list"
            }
        }
        SemanticKind::ListItem => "li",
        SemanticKind::Link => "link",
        SemanticKind::Image => "image",
        SemanticKind::Input => "input",
        SemanticKind::Textarea => "textarea",
        SemanticKind::Select => "select",
        SemanticKind::Button => "button",
        SemanticKind::Group => "group",
    }
}

fn inspect_kind_needed(component: &SemanticComponent) -> bool {
    let tag = component.attrs.tag.as_deref().unwrap_or("").to_ascii_lowercase();
    // No tag → identity already used the kind fallback; do not double-annotate.
    if tag.is_empty() {
        return false;
    }
    match component.kind {
        SemanticKind::Link => tag != "a" && tag != "link",
        SemanticKind::Image => tag != "img" && tag != "image",
        SemanticKind::Input => tag != "input",
        SemanticKind::Textarea => tag != "textarea",
        SemanticKind::Select => tag != "select",
        SemanticKind::Button => tag != "button",
        SemanticKind::ListItem => tag != "li",
        SemanticKind::Heading => {
            // h1–h6 already encode level.
            !(tag.len() == 2
                && tag.as_bytes()[0].eq_ignore_ascii_case(&b'h')
                && tag.as_bytes()[1].is_ascii_digit())
        }
        SemanticKind::Landmark => {
            let role = landmark_role_str(component).unwrap_or("");
            tag != role
                && tag != "nav"
                && tag != "main"
                && tag != "aside"
                && tag != "header"
                && tag != "footer"
                && tag != "section"
        }
        // Tag alone is enough for prose/containers.
        SemanticKind::Text | SemanticKind::List | SemanticKind::Group => false,
    }
}

fn landmark_role_str(component: &SemanticComponent) -> Option<&'static str> {
    use crate::semantic::LandmarkRole;
    match component.attrs.landmark? {
        LandmarkRole::Main => Some("main"),
        LandmarkRole::Aside => Some("aside"),
        LandmarkRole::Header => Some("header"),
        LandmarkRole::Nav => Some("nav"),
        LandmarkRole::Section => Some("section"),
        LandmarkRole::Footer => Some("footer"),
    }
}

fn inspect_label(component: &SemanticComponent) -> Option<String> {
    let raw = component
        .label
        .as_deref()
        .or(component.text.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    Some(format!("\"{}\"", clip_inspect(raw, INSPECT_CLIP)))
}

fn inspect_action_line(component: &SemanticComponent) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    match component.kind {
        SemanticKind::Link => {
            if let Some(href) = non_empty(&component.attrs.href) {
                parts.push(format!("href={}", fold_media_url(href)));
            }
        }
        SemanticKind::Input | SemanticKind::Textarea => {
            if let Some(t) = non_empty(&component.attrs.input_type) {
                parts.push(format!("type={t}"));
            }
            if let Some(n) = non_empty(&component.attrs.name) {
                parts.push(format!("name={n}"));
            }
            // Always show value key for controls when present (including empty string).
            if let Some(v) = component.attrs.value.as_deref() {
                parts.push(format!("value={}", clip_inspect(v, INSPECT_CLIP)));
            }
            if let Some(ph) = non_empty(&component.attrs.placeholder) {
                parts.push(format!("ph={}", clip_inspect(ph, INSPECT_CLIP)));
            }
            push_flags(&mut parts, component);
        }
        SemanticKind::Select => {
            if let Some(n) = non_empty(&component.attrs.name) {
                parts.push(format!("name={n}"));
            }
            if let Some(v) = component.attrs.value.as_deref() {
                parts.push(format!("value={}", clip_inspect(v, INSPECT_CLIP)));
            }
            if !component.attrs.options.is_empty() {
                parts.push(format!("options={}", component.attrs.options.len()));
            }
            push_flags(&mut parts, component);
        }
        SemanticKind::Button => {
            if let Some(t) = non_empty(&component.attrs.button_type) {
                parts.push(format!("type={t}"));
            }
            if let Some(n) = non_empty(&component.attrs.name) {
                parts.push(format!("name={n}"));
            }
            push_flags(&mut parts, component);
        }
        SemanticKind::Image => {
            if let Some(alt) = non_empty(&component.attrs.alt) {
                parts.push(format!("alt={}", clip_inspect(alt, INSPECT_CLIP)));
            }
            if let Some(src) = non_empty(&component.attrs.src) {
                parts.push(format!("src={}", fold_media_url(src)));
            }
        }
        SemanticKind::Heading => {
            if let Some(level) = component.attrs.heading_level {
                let tag = component.attrs.tag.as_deref().unwrap_or("");
                // Only emit level when tag didn't already encode it.
                let tag_encodes = tag.len() == 2
                    && tag.as_bytes()[0].eq_ignore_ascii_case(&b'h')
                    && tag.as_bytes()[1].is_ascii_digit();
                if !tag_encodes {
                    parts.push(format!("h{level}"));
                }
            }
        }
        SemanticKind::Landmark => {
            if let Some(role) = landmark_role_str(component) {
                let tag = component
                    .attrs
                    .tag
                    .as_deref()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if tag != role {
                    parts.push(format!("landmark={role}"));
                }
            }
        }
        SemanticKind::List => {
            if component.attrs.ordered == Some(true) {
                parts.push("ordered".into());
            }
        }
        SemanticKind::Text | SemanticKind::ListItem | SemanticKind::Group => {}
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("  "))
    }
}

fn push_flags(parts: &mut Vec<String>, component: &SemanticComponent) {
    if component.attrs.required == Some(true) {
        parts.push("required".into());
    }
    if component.attrs.disabled == Some(true) {
        parts.push("disabled".into());
    }
    if component.attrs.readonly == Some(true) {
        parts.push("readonly".into());
    }
    if component.attrs.checked == Some(true) {
        parts.push("checked".into());
    }
    if component.attrs.multiple == Some(true) {
        parts.push("multiple".into());
    }
}

fn non_empty(value: &Option<String>) -> Option<&str> {
    value.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

fn clip_inspect(value: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let count = value.chars().count();
    if count <= max_chars {
        return value.to_string();
    }
    if max_chars <= 1 {
        return "…".to_string();
    }
    let mut out: String = value.chars().take(max_chars - 1).collect();
    out.push('…');
    out
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

    fn heading_node(level: u8, id: &str, text: &str) -> RawSemanticNode {
        RawSemanticNode {
            kind: "heading".into(),
            tag: Some(format!("h{level}")),
            id: Some(id.into()),
            unique_id: true,
            selector: None,
            text: Some(text.into()),
            href: None,
            landmark: None,
            heading_level: Some(level),
            ordered: None,
            label: Some(text.into()),
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
        }
    }

    fn text_node(id: &str, text: &str) -> RawSemanticNode {
        RawSemanticNode {
            kind: "text".into(),
            tag: Some("p".into()),
            id: Some(id.into()),
            unique_id: true,
            selector: None,
            text: Some(text.into()),
            href: None,
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
        }
    }

    #[test]
    fn prose_heading_spacing_and_non_selectable_blanks() {
        let doc = normalize_fixture(
            meta(),
            vec![
                heading_node(1, "h1", "Title"),
                text_node("p1", "body"),
                heading_node(2, "h2", "Section"),
                text_node("p2", "more"),
                heading_node(3, "h3", "Sub"),
            ],
        )
        .expect("doc");
        let empty = HashSet::new();
        let prose = build_content_lines_with(&doc, &empty, ContentProjection::Prose);
        let structure = build_content_lines_with(&doc, &empty, ContentProjection::Structure);

        // Structure: dense, no blank spacers.
        assert!(
            structure
                .iter()
                .all(|l| l.semantic_ref.is_some() || !l.text.is_empty()),
            "structure must not insert blank spacers: {structure:?}"
        );

        // Helper: index of selectable heading lines in prose.
        let idx = |needle: &str| {
            prose
                .iter()
                .position(|l| l.text.contains(needle))
                .unwrap_or_else(|| panic!("missing {needle} in {prose:?}"))
        };
        let h1 = idx("# Title");
        let body = idx("body");
        let h2 = idx("## Section");
        let h3 = idx("### Sub");

        // h1 at start: no leading blanks.
        assert_eq!(h1, 0);
        // 2 blanks after h1 before body.
        assert_eq!(body - h1 - 1, 2);
        // 2 blanks before h2.
        assert_eq!(h2 - body - 1, 2);
        // 1 blank before h3.
        let more = idx("more");
        assert_eq!(h3 - more - 1, 1);

        // Blank spacers are never selectable.
        for line in &prose {
            if line.text.is_empty() {
                assert!(line.semantic_ref.is_none());
            }
        }

        // h1 immediately followed by h2: no extra after-h1 blanks (h2 provides leading).
        let h1h2 = normalize_fixture(
            meta(),
            vec![heading_node(1, "a", "A"), heading_node(2, "b", "B")],
        )
        .expect("doc");
        let lines = build_content_lines_with(&h1h2, &empty, ContentProjection::Prose);
        let i1 = lines.iter().position(|l| l.text.contains("# A")).unwrap();
        let i2 = lines.iter().position(|l| l.text.contains("## B")).unwrap();
        // Only the 2 leading blanks for h2 between them (not 2 after + 2 before).
        assert_eq!(i2 - i1 - 1, 2);
    }

    #[test]
    fn form_controls_render_compact_inline_values() {
        let doc = normalize_fixture(
            meta(),
            vec![
                RawSemanticNode {
                    kind: "input".into(),
                    tag: Some("input".into()),
                    id: Some("email".into()),
                    unique_id: true,
                    selector: None,
                    text: None,
                    href: None,
                    landmark: None,
                    heading_level: None,
                    ordered: None,
                    label: Some("Email".into()),
                    src: None,
                    alt: None,
                    name: Some("email".into()),
                    value: Some("a@b.co".into()),
                    input_type: Some("email".into()),
                    placeholder: None,
                    checked: None,
                    disabled: None,
                    required: None,
                    readonly: None,
                    multiple: None,
                    button_type: None,
                    options: vec![],
                    children: vec![],
                },
                RawSemanticNode {
                    kind: "input".into(),
                    tag: Some("input".into()),
                    id: Some("ok".into()),
                    unique_id: true,
                    selector: None,
                    text: None,
                    href: None,
                    landmark: None,
                    heading_level: None,
                    ordered: None,
                    label: Some("Agree".into()),
                    src: None,
                    alt: None,
                    name: Some("ok".into()),
                    value: None,
                    input_type: Some("checkbox".into()),
                    placeholder: None,
                    checked: Some(true),
                    disabled: None,
                    required: None,
                    readonly: None,
                    multiple: None,
                    button_type: None,
                    options: vec![],
                    children: vec![],
                },
            ],
        )
        .expect("doc");
        let lines = build_content_lines_with(&doc, &HashSet::new(), ContentProjection::Prose);
        let joined = lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n");
        assert!(joined.contains("a@b.co"), "{joined}");
        assert!(joined.contains("Email") || joined.contains("email"), "{joined}");
        assert!(joined.contains("☑") || joined.contains("Agree"), "{joined}");

        // Overlay live edit value
        let email = doc.roots[0].semantic_ref.clone();
        let mut overlay = FormValueOverlay::new();
        overlay.insert(email.clone(), "live@x".into());
        let lines = build_content_lines_with_overlay(
            &doc,
            &HashSet::new(),
            ContentProjection::Prose,
            Some(&overlay),
            Some(&email),
        );
        let joined = lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n");
        assert!(joined.contains("live@x"), "{joined}");
        assert!(joined.contains('█'), "cursor while editing: {joined}");
    }

    #[test]
    fn copy_y_on_link_returns_resolved_href() {

        let doc = normalize_fixture(
            meta(),
            vec![RawSemanticNode {
                kind: "link".into(),
                tag: Some("a".into()),
                id: Some("svc".into()),
                unique_id: true,
                selector: None,
                text: Some("SERVICE".into()),
                href: Some("/en/#service".into()),
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
        let r = doc.roots[0].semantic_ref.clone();
        let text = rendered_block_text(&doc, &r).expect("copy");
        assert_eq!(text, "https://example.com/en/#service");
        assert!(!text.contains("SERVICE"));
        assert!(!text.contains("]("));
    }

    #[test]
    fn copy_y_on_image_returns_src_url() {
        let doc = normalize_fixture(
            meta(),
            vec![RawSemanticNode {
                kind: "image".into(),
                tag: Some("img".into()),
                id: None,
                unique_id: false,
                selector: None,
                text: None,
                href: None,
                landmark: None,
                heading_level: None,
                ordered: None,
                label: None,
                src: Some("https://cdn.example/a.png".into()),
                alt: Some("logo".into()),
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
        let r = doc.roots[0].semantic_ref.clone();
        let text = rendered_block_text(&doc, &r).expect("copy");
        assert_eq!(text, "https://cdn.example/a.png");
        assert!(!text.contains("logo"));
        assert!(!text.contains("!["));
    }

    #[test]
    fn copy_y_on_text_still_returns_rendered_block() {
        let doc = normalize_fixture(
            meta(),
            vec![RawSemanticNode {
                kind: "text".into(),
                tag: Some("p".into()),
                id: Some("p1".into()),
                unique_id: true,
                selector: None,
                text: Some("hello body".into()),
                href: None,
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
        let r = doc.roots[0].semantic_ref.clone();
        let text = rendered_block_text(&doc, &r).expect("copy");
        assert!(text.contains("hello body"), "{text}");
    }

    #[test]
    fn fold_media_url_collapses_base64_data_uris() {
        assert_eq!(
            fold_media_url("data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAAB"),
            "base64,…"
        );
        assert_eq!(fold_media_url("data:image/svg+xml,<svg/>"), "data:…");
        assert_eq!(
            fold_media_url("https://cdn.example/a.png"),
            "https://cdn.example/a.png"
        );
        assert_eq!(fold_media_url("/img/logo.png"), "/img/logo.png");
    }

    #[test]
    fn image_lines_fold_base64_src() {
        let doc = normalize_fixture(
            meta(),
            vec![RawSemanticNode {
                kind: "image".into(),
                tag: Some("img".into()),
                id: None,
                unique_id: false,
                selector: None,
                text: None,
                href: None,
                landmark: None,
                heading_level: None,
                ordered: None,
                label: None,
                src: Some("data:image/png;base64,AAAA".into()),
                alt: Some("logo".into()),
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
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "![logo](base64,…)");
        assert!(!lines[0].text.contains("AAAA"));
    }

    #[test]
    fn format_inspect_panel_link_is_compact() {
        let doc = normalize_fixture(
            meta(),
            vec![RawSemanticNode {
                kind: "link".into(),
                tag: Some("a".into()),
                id: Some("pricing".into()),
                unique_id: true,
                selector: None,
                text: Some("Start free trial".into()),
                href: Some("/signup".into()),
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
        let c = doc.roots.first().expect("root");
        let text = format_inspect_panel(c, doc.document.revision.as_str());
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3, "{text}");
        assert_eq!(lines[0], "a#pricing  \"Start free trial\"");
        assert_eq!(lines[1], "href=/signup");
        assert!(lines[2].starts_with(c.semantic_ref.as_str()), "{text}");
        assert!(lines[2].contains("rev=1"), "{text}");
        assert!(!text.contains("Some("));
        assert!(!text.contains("None"));
    }

    #[test]
    fn format_inspect_panel_input_shows_flags_not_defaults() {
        let doc = normalize_fixture(
            meta(),
            vec![RawSemanticNode {
                kind: "input".into(),
                tag: Some("input".into()),
                id: Some("email".into()),
                unique_id: true,
                selector: None,
                text: None,
                href: None,
                landmark: None,
                heading_level: None,
                ordered: None,
                label: Some("Email".into()),
                src: None,
                alt: None,
                name: Some("email".into()),
                value: Some("".into()),
                input_type: Some("email".into()),
                placeholder: Some("you@example.com".into()),
                checked: None,
                disabled: None,
                required: Some(true),
                readonly: None,
                multiple: None,
                button_type: None,
                options: vec![],
                children: vec![],
            }],
        )
        .expect("doc");
        let c = doc.roots.first().expect("root");
        let text = format_inspect_panel(c, "main:19");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "input#email  \"Email\"");
        assert!(lines[1].contains("type=email"), "{text}");
        assert!(lines[1].contains("name=email"), "{text}");
        assert!(lines[1].contains("ph=you@example.com"), "{text}");
        assert!(lines[1].contains("required"), "{text}");
        assert!(!lines[1].contains("disabled"), "{text}");
        // Empty value may be dropped by normalize; only assert when retained.
        if c.attrs.value.is_some() {
            assert!(lines[1].contains("value="), "{text}");
        }
        assert!(lines[2].contains("rev=main:19"), "{text}");
    }

    #[test]
    fn format_inspect_panel_heading_omits_redundant_action() {
        let doc = normalize_fixture(
            meta(),
            vec![RawSemanticNode {
                kind: "heading".into(),
                tag: Some("h2".into()),
                id: Some("install".into()),
                unique_id: true,
                selector: None,
                text: Some("Install".into()),
                href: None,
                landmark: None,
                heading_level: Some(2),
                ordered: None,
                label: Some("Install".into()),
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
        let c = doc.roots.first().expect("root");
        let text = format_inspect_panel(c, "1");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "{text}");
        assert_eq!(lines[0], "h2#install  \"Install\"");
        assert!(lines[1].contains("rev=1"), "{text}");
    }

    #[test]
    fn format_inspect_dom_path_includes_ancestors() {
        let doc = normalize_fixture(
            meta(),
            vec![RawSemanticNode {
                kind: "landmark".into(),
                tag: Some("main".into()),
                id: None,
                unique_id: false,
                selector: None,
                text: None,
                href: None,
                landmark: Some("main".into()),
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
                children: vec![RawSemanticNode {
                    kind: "input".into(),
                    tag: Some("input".into()),
                    id: Some("email".into()),
                    unique_id: true,
                    selector: None,
                    text: None,
                    href: None,
                    landmark: None,
                    heading_level: None,
                    ordered: None,
                    label: Some("Email".into()),
                    src: None,
                    alt: None,
                    name: Some("email".into()),
                    value: None,
                    input_type: Some("email".into()),
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
            }],
        )
        .expect("doc");
        let input = doc
            .components()
            .find(|c| c.attrs.element_id.as_deref() == Some("email"))
            .expect("input");
        let path = format_inspect_dom_path(&doc, input);
        assert_eq!(path, "main > input#email", "{path}");
        let truncated = truncate_inspect_dom_path("main > section > form#signup > input#email", 22);
        assert!(truncated.starts_with('…'), "{truncated}");
        assert!(truncated.contains("input#email"), "{truncated}");
        assert!(truncated.chars().count() <= 22, "{truncated}");
    }

    #[test]
    fn format_inspect_panel_folds_data_urls_and_clips_long_text() {
        let long = "x".repeat(80);
        let doc = normalize_fixture(
            meta(),
            vec![RawSemanticNode {
                kind: "image".into(),
                tag: Some("img".into()),
                id: None,
                unique_id: false,
                selector: None,
                text: None,
                href: None,
                landmark: None,
                heading_level: None,
                ordered: None,
                label: None,
                src: Some("data:image/png;base64,AAAA".into()),
                alt: Some(long.clone()),
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
        let c = doc.roots.first().expect("root");
        let text = format_inspect_panel(c, "1");
        assert!(text.contains("src=base64,…"), "{text}");
        assert!(text.contains("alt="), "{text}");
        assert!(!text.contains("AAAA"), "{text}");
        // clipped alt ends with ellipsis and is bounded
        let alt_line = text.lines().nth(1).unwrap_or("");
        assert!(alt_line.contains('…'), "{text}");
        assert!(alt_line.len() < long.len(), "{text}");
    }

    #[test]
    fn wrap_breaks_on_spaces_and_preserves_refs() {
        let line = ContentLine {
            text: "hello beautiful world".into(),
            semantic_ref: Some(SemanticRef::from_opaque("r1")),
            kind: Some(SemanticKind::Text),
            heading_level: None,
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
    fn prose_hides_landmark_and_list_chrome_structure_shows_them() {
        let doc = normalize_fixture(
            meta(),
            vec![RawSemanticNode {
                kind: "landmark".into(),
                tag: Some("main".into()),
                id: None,
                unique_id: false,
                selector: None,
                text: None,
                href: None,
                landmark: Some("main".into()),
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
                children: vec![
                    RawSemanticNode {
                        kind: "heading".into(),
                        tag: Some("h1".into()),
                        id: Some("t".into()),
                        unique_id: true,
                        selector: None,
                        text: Some("Title".into()),
                        href: None,
                        landmark: None,
                        heading_level: Some(1),
                        ordered: None,
                        label: Some("Title".into()),
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
                    },
                    RawSemanticNode {
                        kind: "list".into(),
                        tag: Some("ul".into()),
                        id: None,
                        unique_id: false,
                        selector: None,
                        text: None,
                        href: None,
                        landmark: None,
                        heading_level: None,
                        ordered: Some(false),
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
                        children: vec![RawSemanticNode {
                            kind: "list_item".into(),
                            tag: Some("li".into()),
                            id: None,
                            unique_id: false,
                            selector: None,
                            text: Some("one".into()),
                            href: None,
                            landmark: None,
                            heading_level: None,
                            ordered: None,
                            label: Some("one".into()),
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
                    },
                ],
            }],
        )
        .expect("doc");
        let empty = HashSet::new();
        let structure = build_content_lines_with(&doc, &empty, ContentProjection::Structure);
        let prose = build_content_lines_with(&doc, &empty, ContentProjection::Prose);
        assert!(
            structure.iter().any(|l| l.text.contains("[main]")),
            "structure shows landmark: {structure:?}"
        );
        assert!(
            structure.iter().any(|l| l.text.contains("ul")),
            "structure shows list chrome"
        );
        assert!(
            !prose.iter().any(|l| l.text.contains("[main]")),
            "prose hides landmark"
        );
        assert!(
            !prose.iter().any(|l| l.text.contains("▾") || l.text == "ul"),
            "prose hides list chrome"
        );
        assert!(prose.iter().any(|l| l.text.contains("# Title")));
        assert!(prose.iter().any(|l| l.text.contains("- one") || l.text.contains("one")));
        // Fully flat: no leading indent spaces on prose lines.
        assert!(prose.iter().all(|l| !l.text.starts_with(' ')));
    }

    #[test]
    fn wrap_hard_splits_overlong_tokens() {
        let line = ContentLine {
            text: "abcdefghij".into(),
            semantic_ref: None,
            kind: Some(SemanticKind::Text),
            heading_level: None,
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

    #[test]
    fn top_viewport_anchor_skips_spacers_and_records_wrap_row() {
        let r1 = SemanticRef::from_opaque("r1");
        let r2 = SemanticRef::from_opaque("r2");
        let lines = vec![
            ContentLine {
                text: String::new(),
                semantic_ref: None,
                kind: None,
                heading_level: None,
                block_start: false,
            },
            ContentLine {
                text: "hello".into(),
                semantic_ref: Some(r1.clone()),
                kind: Some(SemanticKind::Text),
                heading_level: None,
                block_start: true,
            },
            ContentLine {
                text: "world continued".into(),
                semantic_ref: Some(r1.clone()),
                kind: Some(SemanticKind::Text),
                heading_level: None,
                block_start: false,
            },
            ContentLine {
                text: "next".into(),
                semantic_ref: Some(r2.clone()),
                kind: Some(SemanticKind::Text),
                heading_level: None,
                block_start: true,
            },
        ];
        // scroll_y=0 lands on spacer → first addressable is r1, wrap_row 0
        let a0 = top_viewport_anchor(&lines, 0).expect("a0");
        assert_eq!(a0.semantic_ref, r1);
        assert_eq!(a0.wrap_row, 0);
        // scroll_y=2 is wrap continuation of r1
        let a2 = top_viewport_anchor(&lines, 2).expect("a2");
        assert_eq!(a2.semantic_ref, r1);
        assert_eq!(a2.wrap_row, 1);
        let a3 = top_viewport_anchor(&lines, 3).expect("a3");
        assert_eq!(a3.semantic_ref, r2);
        assert_eq!(a3.wrap_row, 0);
    }
}

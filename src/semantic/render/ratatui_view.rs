//! Ratatui projection: nested landmark frames around semantic content.
//!
//! Meaningful landmarks render as nested Ratatui [`Block`] widgets (bordered
//! frames titled with the landmark role) directly around their children. This
//! is not an inspector/sidebar tree. A separate line helper remains available
//! for lightweight inspection; the widget/buffer path uses real Blocks.

use crate::semantic::component::{LandmarkRole, SemanticComponent, SemanticKind};
use crate::semantic::document::SemanticDocument;
use crate::semantic::render::bounds::{RenderError, RenderedOutput, bound_output};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

/// Deterministic Ratatui view of a [`SemanticDocument`].
///
/// Nested landmarks become real bordered [`Block`]s titled with the landmark
/// role. Content uses normal text; opaque refs are not shown in the visual
/// frame (they remain available via other projections and later TUI chrome).
#[derive(Debug, Clone, Copy)]
pub struct SemanticRatatuiView<'a> {
    document: &'a SemanticDocument,
}

impl<'a> SemanticRatatuiView<'a> {
    /// Borrow a document for widget or line-oriented Ratatui projection.
    pub fn new(document: &'a SemanticDocument) -> Self {
        Self { document }
    }

    /// Flatten the document into display lines (lightweight inspection helper).
    ///
    /// This path uses textual markers and is **not** the widget layout path.
    /// Prefer [`render_ratatui_buffer`] / [`Widget::render`] for real Blocks.
    pub fn lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if !self.document.document.title.is_empty() {
            lines.push(self.document.document.title.clone());
            lines.push(String::new());
        }
        for root in &self.document.roots {
            push_component_lines(root, 0, &mut lines);
        }
        lines
    }
}

impl Widget for SemanticRatatuiView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        let chrome = Block::default().borders(Borders::ALL).title(format!(
            "{} · rev {}",
            truncate_title(&self.document.document.title, area.width as usize),
            truncate_title(&self.document.document.revision, 24)
        ));
        let inner = chrome.inner(area);
        chrome.render(area, buf);
        render_stack(&self.document.roots, inner, buf);
    }
}

/// Render the document into a fixed-size buffer and return cell text rows.
///
/// Uses the widget path (nested landmark [`Block`]s), not the line helper.
pub fn render_ratatui_buffer(
    document: &SemanticDocument,
    width: u16,
    height: u16,
) -> Result<Vec<String>, RenderError> {
    let area = Rect::new(0, 0, width.max(1), height.max(1));
    let mut buffer = Buffer::empty(area);
    SemanticRatatuiView::new(document).render(area, &mut buffer);
    Ok(buffer_to_lines(&buffer, area))
}

/// Line-oriented projection used by golden tests (no absolute terminal required).
pub fn render_ratatui_lines(document: &SemanticDocument) -> Result<RenderedOutput, RenderError> {
    let view = SemanticRatatuiView::new(document);
    let mut content = view.lines().join("\n");
    if !content.ends_with('\n') {
        content.push('\n');
    }
    Ok(bound_output(
        content,
        &document.document.document_id,
        &document.document.revision,
    ))
}

/// Extract printable rows from a ratatui buffer for assertions.
pub fn buffer_to_lines(buffer: &Buffer, area: Rect) -> Vec<String> {
    let mut rows = Vec::with_capacity(area.height as usize);
    for y in area.top()..area.bottom() {
        let mut row = String::with_capacity(area.width as usize);
        for x in area.left()..area.right() {
            let cell = &buffer[(x, y)];
            let symbol = cell.symbol();
            if symbol.is_empty() {
                row.push(' ');
            } else {
                // ratatui may return multi-byte symbols for box-drawing.
                row.push_str(symbol);
            }
        }
        // Trim trailing spaces for stable golden comparisons.
        let trimmed = row.trim_end().to_string();
        rows.push(trimmed);
    }
    // Drop trailing empty rows for stability.
    while rows.last().is_some_and(|r| r.is_empty()) {
        rows.pop();
    }
    rows
}

/// Vertically stack siblings within `area`, rendering landmarks as Blocks.
fn render_stack(components: &[SemanticComponent], area: Rect, buf: &mut Buffer) {
    if area.width == 0 || area.height == 0 || components.is_empty() {
        return;
    }

    let heights: Vec<u16> = components.iter().map(estimate_height).collect();
    let total_needed: u32 = heights.iter().map(|&h| u32::from(h)).sum();
    let available = u32::from(area.height);

    // Scale heights down proportionally when content exceeds the viewport.
    let scaled: Vec<u16> = if total_needed <= available || total_needed == 0 {
        heights
    } else {
        let mut remaining_budget = available;
        let mut remaining_need = total_needed;
        heights
            .iter()
            .enumerate()
            .map(|(i, &h)| {
                if i + 1 == heights.len() {
                    remaining_budget as u16
                } else {
                    let share = u32::from(h)
                        .checked_mul(remaining_budget)
                        .and_then(|n| n.checked_div(remaining_need))
                        .unwrap_or(0);
                    let share = share.max(if h > 0 { 1 } else { 0 }).min(remaining_budget);
                    remaining_budget = remaining_budget.saturating_sub(share);
                    remaining_need = remaining_need.saturating_sub(u32::from(h));
                    share as u16
                }
            })
            .collect()
    };

    let mut y = area.y;
    let bottom = area.bottom();
    for (component, height) in components.iter().zip(scaled) {
        if y >= bottom || height == 0 {
            break;
        }
        let h = height.min(bottom.saturating_sub(y));
        let child_area = Rect {
            x: area.x,
            y,
            width: area.width,
            height: h,
        };
        render_component(component, child_area, buf);
        y = y.saturating_add(h);
    }
}

fn render_component(component: &SemanticComponent, area: Rect, buf: &mut Buffer) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    match component.kind {
        SemanticKind::Landmark => {
            let role = landmark_label(component);
            let block = Block::default().borders(Borders::ALL).title(role);
            let inner = block.inner(area);
            block.render(area, buf);
            render_stack(&component.children, inner, buf);
        }
        SemanticKind::Group => {
            // Groups are not landmark frames; stack children, optionally label.
            if let Some(label) = component.label.as_deref().filter(|s| !s.is_empty())
                && area.height >= 1
            {
                let label_area = Rect {
                    x: area.x,
                    y: area.y,
                    width: area.width,
                    height: 1,
                };
                Paragraph::new(Line::from(Span::raw(format!("[{label}]")))).render(label_area, buf);
                let rest = Rect {
                    x: area.x,
                    y: area.y.saturating_add(1),
                    width: area.width,
                    height: area.height.saturating_sub(1),
                };
                render_stack(&component.children, rest, buf);
                return;
            }
            render_stack(&component.children, area, buf);
        }
        _ => {
            let lines = content_lines(component);
            let text_lines: Vec<Line> = lines
                .into_iter()
                .map(|s| Line::from(Span::raw(s)))
                .collect();
            Paragraph::new(text_lines).render(area, buf);
        }
    }
}

fn estimate_height(component: &SemanticComponent) -> u16 {
    match component.kind {
        SemanticKind::Landmark => {
            // Top + bottom border plus children.
            let children: u16 = component
                .children
                .iter()
                .map(estimate_height)
                .fold(0u16, |acc, h| acc.saturating_add(h));
            children.saturating_add(2).max(3)
        }
        SemanticKind::Group => {
            let label = u16::from(component.label.as_ref().is_some_and(|s| !s.is_empty()));
            let children: u16 = component
                .children
                .iter()
                .map(estimate_height)
                .fold(0u16, |acc, h| acc.saturating_add(h));
            children.saturating_add(label).max(1)
        }
        _ => {
            let n = content_lines(component).len();
            (n as u16).max(1)
        }
    }
}

fn content_lines(component: &SemanticComponent) -> Vec<String> {
    let mut lines = Vec::new();
    push_content_only(component, &mut lines);
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Emit non-landmark content lines (landmarks are handled as Blocks).
fn push_content_only(component: &SemanticComponent, lines: &mut Vec<String>) {
    match component.kind {
        SemanticKind::Landmark => {
            // Should not be called for landmarks in the widget path; defensive.
            for child in &component.children {
                push_content_only(child, lines);
            }
        }
        SemanticKind::Heading => {
            let level = component.attrs.heading_level.unwrap_or(2).clamp(1, 6);
            let hashes = "#".repeat(level as usize);
            lines.push(format!("{} {}", hashes, inline_display(component)));
        }
        SemanticKind::Text => {
            let text = inline_display(component);
            if !text.is_empty() {
                lines.push(text);
            }
        }
        SemanticKind::List => {
            let ordered = component.attrs.ordered.unwrap_or(false);
            let mut index = 1usize;
            for child in &component.children {
                if child.kind == SemanticKind::ListItem {
                    let marker = if ordered {
                        format!("{index}.")
                    } else {
                        "-".to_string()
                    };
                    lines.push(format!("{} {}", marker, inline_display(child)));
                    for nested in &child.children {
                        if nested.kind == SemanticKind::List {
                            // Indent nested list lines.
                            let mut nested_lines = Vec::new();
                            push_content_only(nested, &mut nested_lines);
                            for line in nested_lines {
                                lines.push(format!("  {line}"));
                            }
                        }
                    }
                    index += 1;
                } else {
                    push_content_only(child, lines);
                }
            }
        }
        SemanticKind::ListItem => {
            lines.push(format!("- {}", inline_display(component)));
        }
        SemanticKind::Link => {
            let label = display_text(component);
            let href = component.attrs.href.as_deref().unwrap_or("");
            lines.push(format!("[{label}]({href})"));
        }
        SemanticKind::Image => {
            let alt = component
                .attrs
                .alt
                .as_deref()
                .or(component.label.as_deref())
                .unwrap_or("");
            let src = component.attrs.src.as_deref().unwrap_or("");
            lines.push(format!("![{alt}]({src})"));
        }
        SemanticKind::Input => {
            let name = component.attrs.name.as_deref().unwrap_or("");
            let value = component.attrs.value.as_deref().unwrap_or("");
            let input_type = component.attrs.input_type.as_deref().unwrap_or("text");
            lines.push(format!("[input {input_type} name={name} value={value}]"));
        }
        SemanticKind::Textarea => {
            let name = component.attrs.name.as_deref().unwrap_or("");
            let value = component.attrs.value.as_deref().unwrap_or("");
            lines.push(format!("[textarea name={name} value={value}]"));
        }
        SemanticKind::Select => {
            let name = component.attrs.name.as_deref().unwrap_or("");
            let value = component.attrs.value.as_deref().unwrap_or("");
            lines.push(format!("[select name={name} value={value}]"));
        }
        SemanticKind::Button => {
            let label = display_text(component);
            lines.push(format!("[button {label}]"));
        }
        SemanticKind::Group => {
            if let Some(label) = component.label.as_deref().filter(|s| !s.is_empty()) {
                lines.push(format!("[{label}]"));
            }
            for child in &component.children {
                push_content_only(child, lines);
            }
        }
    }
}

// --- line helper (inspection only; not the widget Block path) ---

fn push_component_lines(component: &SemanticComponent, depth: usize, lines: &mut Vec<String>) {
    match component.kind {
        SemanticKind::Landmark => {
            let role = landmark_label(component);
            let indent = "  ".repeat(depth);
            lines.push(format!("{indent}┌─ {role} ─"));
            for child in &component.children {
                push_component_lines(child, depth + 1, lines);
            }
            lines.push(format!("{indent}└─ /{role} ─"));
        }
        SemanticKind::Heading => {
            let level = component.attrs.heading_level.unwrap_or(2).clamp(1, 6);
            let hashes = "#".repeat(level as usize);
            let text = inline_display(component);
            lines.push(format!("{}{} {}", "  ".repeat(depth), hashes, text));
        }
        SemanticKind::Text => {
            let text = inline_display(component);
            if !text.is_empty() {
                lines.push(format!("{}{}", "  ".repeat(depth), text));
            }
        }
        SemanticKind::List => {
            let ordered = component.attrs.ordered.unwrap_or(false);
            let mut index = 1usize;
            for child in &component.children {
                if child.kind == SemanticKind::ListItem {
                    let marker = if ordered {
                        format!("{index}.")
                    } else {
                        "-".to_string()
                    };
                    let text = inline_display(child);
                    lines.push(format!("{}{} {}", "  ".repeat(depth), marker, text));
                    for nested in &child.children {
                        if nested.kind == SemanticKind::List {
                            push_component_lines(nested, depth + 1, lines);
                        }
                    }
                    index += 1;
                } else {
                    push_component_lines(child, depth, lines);
                }
            }
        }
        SemanticKind::ListItem => {
            let text = inline_display(component);
            lines.push(format!("{}- {}", "  ".repeat(depth), text));
        }
        SemanticKind::Link => {
            let label = display_text(component);
            let href = component.attrs.href.as_deref().unwrap_or("");
            lines.push(format!("{}[{label}]({href})", "  ".repeat(depth)));
        }
        SemanticKind::Image => {
            let alt = component
                .attrs
                .alt
                .as_deref()
                .or(component.label.as_deref())
                .unwrap_or("");
            let src = component.attrs.src.as_deref().unwrap_or("");
            lines.push(format!("{}![{alt}]({src})", "  ".repeat(depth)));
        }
        SemanticKind::Input => {
            let name = component.attrs.name.as_deref().unwrap_or("");
            let value = component.attrs.value.as_deref().unwrap_or("");
            let input_type = component.attrs.input_type.as_deref().unwrap_or("text");
            lines.push(format!(
                "{}[input {input_type} name={name} value={value}]",
                "  ".repeat(depth)
            ));
        }
        SemanticKind::Textarea => {
            let name = component.attrs.name.as_deref().unwrap_or("");
            let value = component.attrs.value.as_deref().unwrap_or("");
            lines.push(format!(
                "{}[textarea name={name} value={value}]",
                "  ".repeat(depth)
            ));
        }
        SemanticKind::Select => {
            let name = component.attrs.name.as_deref().unwrap_or("");
            let value = component.attrs.value.as_deref().unwrap_or("");
            lines.push(format!(
                "{}[select name={name} value={value}]",
                "  ".repeat(depth)
            ));
        }
        SemanticKind::Button => {
            let label = display_text(component);
            lines.push(format!("{}[button {label}]", "  ".repeat(depth)));
        }
        SemanticKind::Group => {
            let indent = "  ".repeat(depth);
            if let Some(label) = component.label.as_deref().filter(|s| !s.is_empty()) {
                lines.push(format!("{indent}[{label}]"));
            }
            for child in &component.children {
                push_component_lines(child, depth, lines);
            }
        }
    }
}

fn inline_display(component: &SemanticComponent) -> String {
    if component.children.is_empty() {
        return display_text(component).to_string();
    }
    let mut parts = Vec::new();
    for child in &component.children {
        match child.kind {
            SemanticKind::Text => {
                if child.children.is_empty() {
                    if let Some(t) = child.text.as_deref() {
                        parts.push(t.to_string());
                    }
                } else {
                    parts.push(inline_display(child));
                }
            }
            SemanticKind::Link => {
                let label = display_text(child);
                let href = child.attrs.href.as_deref().unwrap_or("");
                parts.push(format!("[{label}]({href})"));
            }
            SemanticKind::Image => {
                let alt = child.attrs.alt.as_deref().unwrap_or("");
                let src = child.attrs.src.as_deref().unwrap_or("");
                parts.push(format!("![{alt}]({src})"));
            }
            SemanticKind::List => {}
            _ => {
                let t = display_text(child);
                if !t.is_empty() {
                    parts.push(t.to_string());
                }
            }
        }
    }
    join_inline_parts(&parts)
}

fn join_inline_parts(parts: &[String]) -> String {
    let mut out = String::new();
    for part in parts {
        if part.is_empty() {
            continue;
        }
        if out.is_empty() {
            out.push_str(part);
            continue;
        }
        let need_space = match (out.chars().next_back(), part.chars().next()) {
            (Some(a), Some(b)) if !a.is_whitespace() && !b.is_whitespace() => {
                !matches!(a, '[' | '(') && !matches!(b, ']' | ')' | ',' | '.' | ';' | ':')
            }
            _ => false,
        };
        if need_space {
            out.push(' ');
        }
        out.push_str(part);
    }
    out
}

fn display_text(component: &SemanticComponent) -> &str {
    component
        .text
        .as_deref()
        .or(component.label.as_deref())
        .or(component.attrs.value.as_deref())
        .or(component.attrs.alt.as_deref())
        .unwrap_or("")
}

fn landmark_label(component: &SemanticComponent) -> &'static str {
    match component.attrs.landmark {
        Some(LandmarkRole::Main) => "main",
        Some(LandmarkRole::Aside) => "aside",
        Some(LandmarkRole::Header) => "header",
        Some(LandmarkRole::Nav) => "nav",
        Some(LandmarkRole::Section) => "section",
        Some(LandmarkRole::Footer) => "footer",
        None => "landmark",
    }
}

fn truncate_title(value: &str, max_chars: usize) -> String {
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

//! Terminal chrome + content rendering (no shortcut legends).
//!
//! Draws lifecycle/mode chrome, addressable content lines (with independent
//! human selection vs agent attention styles), and status. Never renders key
//! binding legends in the UI. Colors come from [`crate::tui::theme::TuiTheme`].

use crate::tui::controller::Controller;
use crate::tui::state::{InputKind, InteractionMode, Lifecycle};
use crate::tui::theme::TuiTheme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

/// Draw one frame of the terminal browser.
pub fn draw(frame: &mut Frame, controller: &Controller) {
    let theme = TuiTheme::new();
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // chrome (single browser-like bar)
            Constraint::Min(1),    // content
            Constraint::Length(1), // status
        ])
        .split(area);

    draw_chrome(frame, chunks[0], controller, &theme);
    draw_content(frame, chunks[1], controller, &theme);
    draw_status(frame, chunks[2], controller, &theme);

    if let Some(inspect) = &controller.state.view.inspect_text {
        let title = controller
            .state
            .view
            .inspect_title
            .as_deref()
            .unwrap_or("");
        draw_inspect_under_selection(frame, chunks[1], controller, inspect, title, &theme);
    }
}

/// Single-line browser chrome: history · location · title · lifecycle/mode (color only).
fn draw_chrome(frame: &mut Frame, area: Rect, controller: &Controller, theme: &TuiTheme) {
    let state = &controller.state;
    let width = area.width as usize;
    if width == 0 {
        return;
    }

    let lifecycle_style = match &state.lifecycle {
        Lifecycle::Ready => theme.chrome_ready(),
        Lifecycle::Loading { .. } => theme.chrome_loading(),
        Lifecycle::Error { .. } => theme.chrome_error(),
    };

    // Left: back/forward + location (or active prompt).
    let mut spans: Vec<Span> = Vec::new();
    spans.push(Span::styled(
        if state.can_go_back { "◀" } else { "◁" },
        if state.can_go_back {
            theme.chrome_hist_enabled()
        } else {
            theme.chrome_hist_disabled()
        },
    ));
    spans.push(Span::raw(" "));
    spans.push(Span::styled(
        if state.can_go_forward { "▶" } else { "▷" },
        if state.can_go_forward {
            theme.chrome_hist_enabled()
        } else {
            theme.chrome_hist_disabled()
        },
    ));
    spans.push(Span::raw(" "));

    let (location, location_style) = match &state.mode {
        InteractionMode::Input(InputKind::Url { buffer }) => {
            (format!("URL {buffer}"), theme.chrome_mode())
        }
        InteractionMode::Input(InputKind::Search { buffer }) => {
            (format!("/{buffer}"), theme.chrome_mode())
        }
        InteractionMode::Input(InputKind::Form { buffer, .. }) => {
            (format!("IN {buffer}"), theme.chrome_mode())
        }
        InteractionMode::Hint(_) => (
            format!("hint {}", state.view.hint_buffer),
            theme.chrome_mode(),
        ),
        InteractionMode::Normal => {
            let url = state.url();
            if url.is_empty() {
                (String::new(), theme.muted())
            } else {
                (url.to_string(), theme.muted())
            }
        }
    };

    // Right cluster: lifecycle + non-Normal mode only (no wrap/structure flags).
    let mut right_parts: Vec<(&str, Style)> = Vec::new();
    let life = match &state.lifecycle {
        Lifecycle::Ready => "ready",
        Lifecycle::Loading { .. } => "load",
        Lifecycle::Error { .. } => "err",
    };
    right_parts.push((life, lifecycle_style));
    // Mode only when not Normal (keeps the bar quiet while browsing).
    let mode = state.mode_label();
    if mode != "Normal" {
        right_parts.push((mode, theme.chrome_mode()));
    }

    let right_plain: String = right_parts
        .iter()
        .map(|(t, _)| *t)
        .collect::<Vec<_>>()
        .join(" ");
    let right_width = if right_plain.is_empty() {
        0
    } else {
        right_plain.chars().count() + 1 // leading space
    };

    // Middle: prefer URL; append title in dim text when space remains.
    let left_plain_prefix = 4usize; // "◀ ▶ " roughly 4 cells
    let budget = width.saturating_sub(left_plain_prefix + right_width);
    let title = state.title();
    let mut mid = location;
    if !title.is_empty()
        && matches!(state.mode, InteractionMode::Normal)
        && mid.chars().count() + 3 + title.chars().count() <= budget
    {
        mid = format!("{mid} · {title}");
    }
    mid = truncate(&mid, budget);

    spans.push(Span::styled(mid, location_style));

    // Pad then right cluster so flags sit at the trailing edge.
    let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let pad = width.saturating_sub(used + right_plain.chars().count());
    if pad > 0 && !right_parts.is_empty() {
        spans.push(Span::raw(" ".repeat(pad)));
    } else if !right_parts.is_empty() {
        spans.push(Span::raw(" "));
    }
    for (i, (text, style)) in right_parts.into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(text.to_string(), style));
    }

    // Hard clip if we still overflow (narrow terminals).
    let plain: String = spans.iter().map(|s| s.content.as_ref()).collect();
    let line = if plain.chars().count() > width {
        Line::from(Span::styled(truncate(&plain, width), theme.base()))
    } else {
        Line::from(spans)
    };
    frame.render_widget(Paragraph::new(line), area);
}

fn draw_content(frame: &mut Frame, area: Rect, controller: &Controller, theme: &TuiTheme) {
    let state = &controller.state;
    let lines = controller.content_lines();
    let scroll = state.view.scroll_y;
    let height = area.height as usize;
    let width = area.width as usize;
    let hscroll = if state.view.wrap {
        0
    } else {
        state.view.scroll_x
    };

    let mut text_lines = Vec::new();
    for line in lines.iter().skip(scroll).take(height) {
        let mut text: String = line.text.chars().skip(hscroll).collect();
        if text.chars().count() > width {
            text = text.chars().take(width).collect();
        }

        let selected = state
            .view
            .selection
            .as_ref()
            .zip(line.semantic_ref.as_ref())
            .is_some_and(|(s, r)| s == r);
        // Paint root + descendants so prose-hidden containers still spotlight kids.
        let agent_attention = line.semantic_ref.as_ref().is_some_and(|r| {
            state.view.attention_paint.contains(r)
                || state.view.attention.as_ref().is_some_and(|a| a == r)
        });

        let mut spans = Vec::new();
        if let Some(ref_r) = &line.semantic_ref
            && let Some(hint) = controller.hints.iter().find(|h| &h.semantic_ref == ref_r)
        {
            let label = format!("[{}] ", hint.label);
            spans.push(Span::styled(label, theme.hint_label()));
            let label_len = format!("[{}] ", hint.label).chars().count();
            let remain = width.saturating_sub(label_len);
            if text.chars().count() > remain {
                text = text.chars().take(remain).collect();
            }
        }

        let style = theme.line_style(line.kind, line.heading_level, selected, agent_attention);
        // Pad attention rows to the full viewport width so the magenta bar is a
        // solid strip, not only under the glyph run (easy to miss on short h2s).
        if agent_attention {
            let used: usize = spans
                .iter()
                .map(|s| s.content.chars().count())
                .sum::<usize>()
                + text.chars().count();
            if used < width {
                text.push_str(&" ".repeat(width - used));
            }
        }
        spans.push(Span::styled(text, style));
        text_lines.push(Line::from(spans));
    }

    if text_lines.is_empty() {
        let (empty, style) = match &state.lifecycle {
            Lifecycle::Loading { action } => {
                (format!("Loading {action}…"), theme.status_loading())
            }
            Lifecycle::Error { message, .. } => (
                format!("(error retained prior page) {message}"),
                theme.status_error(),
            ),
            Lifecycle::Ready if state.page.is_none() => ("No document".into(), theme.muted()),
            Lifecycle::Ready => (String::new(), theme.base()),
        };
        text_lines.push(Line::from(Span::styled(empty, style)));
    }

    frame.render_widget(Paragraph::new(text_lines).style(theme.base()), area);
}

fn draw_status(frame: &mut Frame, area: Rect, controller: &Controller, theme: &TuiTheme) {
    let state = &controller.state;
    let mut spans = Vec::new();
    match &state.lifecycle {
        Lifecycle::Loading { action } => {
            spans.push(Span::styled(
                format!("loading:{action}"),
                theme.status_loading(),
            ));
        }
        Lifecycle::Error { action, message } => {
            spans.push(Span::styled(
                format!("error:{action}: {message}"),
                theme.status_error(),
            ));
        }
        Lifecycle::Ready => {
            spans.push(Span::styled(
                format!("rev {}", state.revision()),
                theme.muted(),
            ));
        }
    }
    if let Some(msg) = &state.view.status_message {
        if !spans.is_empty() {
            spans.push(Span::raw(" │ "));
        }
        let style = if msg.starts_with("dismissed:") || msg.contains("not found") {
            theme.muted()
        } else if msg.starts_with("wrap:") {
            theme.chrome_wrap()
        } else {
            theme.status_ok()
        };
        spans.push(Span::styled(msg.clone(), style));
    }
    if let Some(fb) = &state.clipboard_fallback {
        if !spans.is_empty() {
            spans.push(Span::raw(" │ "));
        }
        let preview: String = fb.chars().take(32).collect();
        spans.push(Span::styled(
            format!("clip-fallback:{preview}"),
            theme.muted(),
        ));
    }
    let plain: String = spans.iter().map(|s| s.content.as_ref()).collect();
    let line = if plain.chars().count() > area.width as usize {
        Line::from(Span::styled(
            truncate(&plain, area.width as usize),
            theme.base(),
        ))
    } else {
        Line::from(spans)
    };
    frame.render_widget(Paragraph::new(line), area);
}

/// Draw the inspect panel just below the last visible line of the selection.
///
/// Falls back to the bottom of the content area when the selection is off-screen
/// or near the bottom (panel grows upward so it stays in the content pane).
fn draw_inspect_under_selection(
    frame: &mut Frame,
    content_area: Rect,
    controller: &Controller,
    inspect: &str,
    title: &str,
    theme: &TuiTheme,
) {
    let lines = controller.content_lines();
    let scroll = controller.state.view.scroll_y;
    let vh = content_area.height as usize;

    let body_lines = inspect.lines().count().max(1);
    // Border (2) + body, capped so we never cover the whole content pane.
    let panel_h = ((body_lines + 2) as u16)
        .min(content_area.height.saturating_sub(1).max(3))
        .max(3);
    let width = content_area.width.saturating_sub(2).max(10);
    let x = content_area.x.saturating_add(1);

    // Last content-line index of the selection (handles wrap continuations).
    let last_abs = controller
        .state
        .view
        .selection
        .as_ref()
        .and_then(|sel| Controller::last_line_index_of(&lines, sel));

    let y = if let Some(abs) = last_abs {
        if abs < scroll {
            // Selection above viewport: dock under content top.
            content_area.y
        } else {
            let row_in_view = abs - scroll;
            if row_in_view >= vh {
                // Selection below viewport: dock above content bottom.
                content_area
                    .y
                    .saturating_add(content_area.height.saturating_sub(panel_h))
            } else {
                let below = content_area.y.saturating_add((row_in_view as u16).saturating_add(1));
                let max_y = content_area
                    .y
                    .saturating_add(content_area.height.saturating_sub(panel_h));
                if below > max_y {
                    // Not enough room below the element: sit just above the bottom.
                    max_y
                } else {
                    below
                }
            }
        }
    } else {
        content_area
            .y
            .saturating_add(content_area.height.saturating_sub(panel_h))
    };

    let rect = Rect {
        x,
        y,
        width,
        height: panel_h,
    };
    // Title is the full DOM path; truncate from the left so the leaf stays visible.
    let title_budget = width.saturating_sub(2).max(1);
    let title_text = if title.is_empty() {
        "inspect".to_string()
    } else {
        crate::tui::content::truncate_inspect_dom_path(title, title_budget as usize)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title_text)
        .border_style(theme.chrome_mode())
        .title_style(theme.chrome_mode());
    let inner = block.inner(rect);
    frame.render_widget(Clear, rect);
    frame.render_widget(block, rect);
    frame.render_widget(
        Paragraph::new(inspect.to_string())
            .wrap(Wrap { trim: true })
            .style(theme.base()),
        inner,
    );
}

fn truncate(s: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let count = s.chars().count();
    if count <= width {
        s.to_string()
    } else if width <= 1 {
        "…".into()
    } else {
        let mut out: String = s.chars().take(width - 1).collect();
        out.push('…');
        out
    }
}

/// Build chrome text for tests (no terminal). Single browser-like bar.
#[allow(dead_code)]
pub fn chrome_lines(controller: &Controller) -> Vec<String> {
    let state = &controller.state;
    let back = if state.can_go_back { "◀" } else { "◁" };
    let fwd = if state.can_go_forward { "▶" } else { "▷" };
    let life = match &state.lifecycle {
        Lifecycle::Ready => "ready",
        Lifecycle::Loading { .. } => "load",
        Lifecycle::Error { .. } => "err",
    };
    let mut flags = vec![life];
    let mode = state.mode_label();
    if mode != "Normal" {
        flags.push(mode);
    }
    let mid = if state.title().is_empty() {
        state.url().to_string()
    } else if state.url().is_empty() {
        state.title().to_string()
    } else {
        format!("{} · {}", state.url(), state.title())
    };
    vec![format!("{back} {fwd} {mid}  {}", flags.join(" "))]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::DocumentMetadata;
    use crate::semantic::SemanticDocument;
    use crate::tui::content::contains_shortcut_legend;

    #[test]
    fn chrome_has_no_shortcut_legend() {
        let mut ctl = Controller::new();
        let doc = SemanticDocument::empty(DocumentMetadata {
            document_id: "d".into(),
            revision: "1".into(),
            url: "https://example.com/".into(),
            title: "T".into(),
            ready_state: "complete".into(),
            frames: vec![],
        })
        .unwrap();
        ctl.state.publish_page(doc);
        for line in chrome_lines(&ctl) {
            assert!(!contains_shortcut_legend(&line), "legend leaked: {line}");
        }
    }

    #[test]
    fn horizontal_scroll_is_character_aligned() {
        assert_eq!(truncate("éclair", 3), "éc…");
    }
}

//! Terminal chrome + content rendering (no shortcut legends).
//!
//! Draws lifecycle/mode chrome, addressable content lines (with independent
//! human selection vs agent attention styles), and status. Never renders key
//! binding legends in the UI.

use crate::tui::controller::Controller;
use crate::tui::state::{InputKind, InteractionMode, Lifecycle};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

/// Draw one frame of the terminal browser.
pub fn draw(frame: &mut Frame, controller: &Controller) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // chrome
            Constraint::Min(1),    // content
            Constraint::Length(1), // status
        ])
        .split(area);

    draw_chrome(frame, chunks[0], controller);
    draw_content(frame, chunks[1], controller);
    draw_status(frame, chunks[2], controller);

    if let Some(inspect) = &controller.state.view.inspect_text {
        draw_inspect_overlay(frame, area, inspect);
    }
}

fn draw_chrome(frame: &mut Frame, area: Rect, controller: &Controller) {
    let state = &controller.state;
    let mode = state.mode_label();
    let lifecycle = state.lifecycle.status_label();
    let hist = format!(
        "←{} →{}",
        if state.can_go_back { "●" } else { "○" },
        if state.can_go_forward { "●" } else { "○" }
    );
    let wrap = if state.view.wrap { " wrap" } else { "" };

    let url_line = match &state.mode {
        InteractionMode::Input(InputKind::Url { buffer }) => format!("URL> {buffer}"),
        InteractionMode::Input(InputKind::Search { buffer }) => format!("/{buffer}"),
        InteractionMode::Input(InputKind::Form { buffer, .. }) => format!("IN> {buffer}"),
        InteractionMode::Hint(_) => {
            format!("hint: {}", state.view.hint_buffer)
        }
        InteractionMode::Normal => state.url().to_string(),
    };

    let title = state.title();
    let line1 = format!("[{mode}] [{lifecycle}]{wrap} {hist}  {title}");
    let line2 = truncate(&url_line, area.width as usize);

    let para = Paragraph::new(vec![
        Line::from(Span::styled(
            truncate(&line1, area.width as usize),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(line2),
    ]);
    frame.render_widget(para, area);
}

fn draw_content(frame: &mut Frame, area: Rect, controller: &Controller) {
    let state = &controller.state;
    let lines = controller.content_lines();
    let scroll = state.view.scroll_y;
    let height = area.height as usize;
    let width = area.width as usize;
    // Horizontal pan only applies when wrap is off; wrapped lines already fit.
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
        let agent_attention = state
            .view
            .attention
            .as_ref()
            .zip(line.semantic_ref.as_ref())
            .is_some_and(|(a, r)| a == r);

        // Overlay hint labels for links
        if let Some(ref_r) = &line.semantic_ref
            && let Some(hint) = controller.hints.iter().find(|h| &h.semantic_ref == ref_r)
        {
            text = format!("[{}] {}", hint.label, text);
            if text.chars().count() > width {
                text = text.chars().take(width).collect();
            }
        }

        // Human selection and agent attention are independent: selection wins
        // for reverse video; attention uses a distinct underline/bold highlight.
        let style = if selected {
            Style::default().add_modifier(Modifier::REVERSED)
        } else if agent_attention {
            Style::default().add_modifier(Modifier::UNDERLINED | Modifier::BOLD)
        } else {
            Style::default()
        };
        text_lines.push(Line::from(Span::styled(text, style)));
    }

    if text_lines.is_empty() {
        let empty = match &state.lifecycle {
            Lifecycle::Loading { action } => format!("Loading {action}…"),
            Lifecycle::Error { message, .. } => format!("(error retained prior page) {message}"),
            Lifecycle::Ready if state.page.is_none() => "No document".into(),
            Lifecycle::Ready => String::new(),
        };
        text_lines.push(Line::from(empty));
    }

    frame.render_widget(Paragraph::new(text_lines), area);
}

fn draw_status(frame: &mut Frame, area: Rect, controller: &Controller) {
    let state = &controller.state;
    let mut parts = Vec::new();
    match &state.lifecycle {
        Lifecycle::Loading { action } => parts.push(format!("loading:{action}")),
        Lifecycle::Error { action, message } => {
            parts.push(format!("error:{action}: {message}"));
        }
        Lifecycle::Ready => parts.push(format!("rev {}", state.revision())),
    }
    if let Some(msg) = &state.view.status_message {
        parts.push(msg.clone());
    }
    if let Some(fb) = &state.clipboard_fallback {
        let preview: String = fb.chars().take(32).collect();
        parts.push(format!("clip-fallback:{preview}"));
    }
    let text = truncate(&parts.join(" │ "), area.width as usize);
    frame.render_widget(Paragraph::new(text), area);
}

fn draw_inspect_overlay(frame: &mut Frame, area: Rect, inspect: &str) {
    let width = area.width.saturating_sub(4).max(10);
    let height = 6u16.min(area.height.saturating_sub(2)).max(3);
    let x = area.x + 2;
    let y = area.y + area.height.saturating_sub(height + 1) / 2;
    let rect = Rect {
        x,
        y,
        width,
        height,
    };
    let block = Block::default().borders(Borders::ALL).title("inspect");
    let inner = block.inner(rect);
    frame.render_widget(Clear, rect);
    frame.render_widget(block, rect);
    frame.render_widget(
        Paragraph::new(inspect.to_string()).wrap(Wrap { trim: true }),
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

/// Build chrome text lines for tests (no terminal).
#[allow(dead_code)]
pub fn chrome_lines(controller: &Controller) -> Vec<String> {
    let state = &controller.state;
    let mode = state.mode_label();
    let lifecycle = state.lifecycle.status_label();
    let hist = format!(
        "back={} forward={}",
        state.can_go_back, state.can_go_forward
    );
    vec![
        format!("[{mode}] [{lifecycle}] {hist} {}", state.title()),
        state.url().to_string(),
    ]
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
            title: "Example".into(),
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

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
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

/// Draw one frame of the terminal browser.
pub fn draw(frame: &mut Frame, controller: &Controller) {
    let theme = TuiTheme::new();
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // chrome
            Constraint::Min(1),    // content
            Constraint::Length(1), // status
        ])
        .split(area);

    draw_chrome(frame, chunks[0], controller, &theme);
    draw_content(frame, chunks[1], controller, &theme);
    draw_status(frame, chunks[2], controller, &theme);

    if let Some(inspect) = &controller.state.view.inspect_text {
        draw_inspect_overlay(frame, area, inspect, &theme);
    }
}

fn draw_chrome(frame: &mut Frame, area: Rect, controller: &Controller, theme: &TuiTheme) {
    let state = &controller.state;
    let mode = state.mode_label();
    let lifecycle = state.lifecycle.status_label();
    let lifecycle_style = match &state.lifecycle {
        Lifecycle::Ready => theme.chrome_ready(),
        Lifecycle::Loading { .. } => theme.chrome_loading(),
        Lifecycle::Error { .. } => theme.chrome_error(),
    };

    let mut line1_spans = vec![
        Span::styled(format!("[{mode}]"), theme.chrome_mode()),
        Span::raw(" "),
        Span::styled(format!("[{lifecycle}]"), lifecycle_style),
    ];
    if state.view.wrap {
        line1_spans.push(Span::styled(" wrap", theme.chrome_wrap()));
    }
    if state.view.projection.is_structure() {
        line1_spans.push(Span::styled(" struct", theme.chrome_wrap()));
    }
    line1_spans.push(Span::raw(" "));
    line1_spans.push(Span::styled(
        if state.can_go_back { "←●" } else { "←○" },
        if state.can_go_back {
            theme.chrome_hist_enabled()
        } else {
            theme.chrome_hist_disabled()
        },
    ));
    line1_spans.push(Span::raw(" "));
    line1_spans.push(Span::styled(
        if state.can_go_forward { "→●" } else { "→○" },
        if state.can_go_forward {
            theme.chrome_hist_enabled()
        } else {
            theme.chrome_hist_disabled()
        },
    ));
    line1_spans.push(Span::raw("  "));
    line1_spans.push(Span::styled(state.title().to_string(), theme.chrome_title()));

    let url_line = match &state.mode {
        InteractionMode::Input(InputKind::Url { buffer }) => format!("URL> {buffer}"),
        InteractionMode::Input(InputKind::Search { buffer }) => format!("/{buffer}"),
        InteractionMode::Input(InputKind::Form { buffer, .. }) => format!("IN> {buffer}"),
        InteractionMode::Hint(_) => format!("hint: {}", state.view.hint_buffer),
        InteractionMode::Normal => state.url().to_string(),
    };
    let url_style = match &state.mode {
        InteractionMode::Input(_) | InteractionMode::Hint(_) => theme.chrome_mode(),
        InteractionMode::Normal => theme.muted(),
    };

    let line1_text: String = line1_spans.iter().map(|s| s.content.as_ref()).collect();
    let line1 = if line1_text.chars().count() > area.width as usize {
        // Fall back to truncated plain line when the chrome overflows.
        Line::from(Span::styled(
            truncate(&line1_text, area.width as usize),
            theme.chrome_title(),
        ))
    } else {
        Line::from(line1_spans)
    };

    let para = Paragraph::new(vec![
        line1,
        Line::from(Span::styled(
            truncate(&url_line, area.width as usize),
            url_style,
        )),
    ]);
    frame.render_widget(para, area);
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
        let agent_attention = state
            .view
            .attention
            .as_ref()
            .zip(line.semantic_ref.as_ref())
            .is_some_and(|(a, r)| a == r);

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

fn draw_inspect_overlay(frame: &mut Frame, area: Rect, inspect: &str, theme: &TuiTheme) {
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
    let block = Block::default()
        .borders(Borders::ALL)
        .title("inspect")
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

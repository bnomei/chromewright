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

/// Draw one frame of the terminal browser using built-in theme defaults.
#[allow(dead_code)] // Convenience for callers/tests that do not load a config theme.
pub fn draw(frame: &mut Frame, controller: &Controller) {
    draw_with_theme(frame, controller, &TuiTheme::new());
}

/// Draw one frame with an explicit theme (from `tui.toml` `[theme]` overlay).
pub fn draw_with_theme(frame: &mut Frame, controller: &Controller, theme: &TuiTheme) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // chrome (single browser-like bar)
            Constraint::Min(1),    // content
            Constraint::Length(1), // status
        ])
        .split(area);

    draw_chrome(frame, chunks[0], controller, theme);
    draw_content(frame, chunks[1], controller, theme);
    draw_status(frame, chunks[2], controller, theme);

    if let Some(inspect) = &controller.state.view.inspect_text {
        let title = controller
            .state
            .view
            .inspect_title
            .as_deref()
            .unwrap_or("");
        draw_inspect_under_selection(frame, chunks[1], controller, inspect, title, theme);
    }
}

/// Split a markdown-style link line so only the URL is underlined.
///
/// Expects content shaped like `[label](url)` (optional indent). When the
/// pattern is missing (wrap continuations, odd capture), falls back to a
/// single non-underlined span with `base`.
fn link_line_spans(text: &str, base: Style) -> Vec<Span<'static>> {
    use ratatui::style::Modifier;
    if let Some((prefix, url, suffix)) = split_markdown_link_url(text) {
        let url_style = base.add_modifier(Modifier::UNDERLINED);
        let mut out = Vec::with_capacity(3);
        if !prefix.is_empty() {
            out.push(Span::styled(prefix.to_string(), base));
        }
        if !url.is_empty() {
            out.push(Span::styled(url.to_string(), url_style));
        }
        if !suffix.is_empty() {
            out.push(Span::styled(suffix.to_string(), base));
        }
        if out.is_empty() {
            out.push(Span::styled(text.to_string(), base));
        }
        out
    } else {
        // No `](url)` segment visible — keep link color without underline.
        vec![Span::styled(text.to_string(), base)]
    }
}

/// Locate `](url)` in a link display line. Returns `(before_url, url, after_url)`.
fn split_markdown_link_url(text: &str) -> Option<(&str, &str, &str)> {
    let open = text.find("](")?;
    let url_start = open + 2;
    let rest = text.get(url_start..)?;
    let close_rel = rest.find(')')?;
    let url = &rest[..close_rel];
    let suffix = &rest[close_rel..]; // includes `)`
    let prefix = &text[..url_start]; // includes `](`
    Some((prefix, url, suffix))
}

/// Single-line browser chrome: tabs · history · location · title · lifecycle/mode (color only).
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

    // Left: tab ordinal · back/forward + location (or active prompt).
    let mut spans: Vec<Span> = Vec::new();
    let tab_label = state
        .tab_position
        .map(|(index, count)| format!("{index}/{count}"));
    if let Some(ref label) = tab_label {
        spans.push(Span::styled(label.clone(), theme.muted()));
        spans.push(Span::raw(" "));
    }
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

    // Search + link-hint live in the footer cmdline, not the location bar.
    let (location, location_style) = match &state.mode {
        InteractionMode::Input(InputKind::Url { buffer }) => {
            (format!("URL {buffer}"), theme.chrome_mode())
        }
        InteractionMode::Input(InputKind::Form { buffer, .. }) => {
            (format!("IN {buffer}"), theme.chrome_mode())
        }
        InteractionMode::Input(InputKind::Search { .. })
        | InteractionMode::Hint(_)
        | InteractionMode::Normal => {
            let url = state.url();
            if url.is_empty() {
                (String::new(), theme.muted())
            } else {
                (url.to_string(), theme.muted())
            }
        }
    };

    // Right cluster: lifecycle glyph + non-Normal mode only (no wrap/structure flags).
    // Search / Hint are shown in the footer cmdline, not here.
    let mut right_parts: Vec<(&str, Style)> = Vec::new();
    right_parts.push((lifecycle_glyph(&state.lifecycle), lifecycle_style));
    let mode = state.mode_label();
    if mode != "Normal" && mode != "Search" && mode != "Hint" && mode != "Hint+" {
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
    // "2/5 ◀ ▶ " or "◀ ▶ " depending on whether tab position is known.
    let left_plain_prefix = tab_label
        .as_ref()
        .map(|l| l.chars().count() + 1 + 4)
        .unwrap_or(4);
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
        // One label per target: only the block-start row (first line of a
        // multi-line / wrapped link or control), never wrap continuations.
        if line.block_start
            && let Some(ref_r) = &line.semantic_ref
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
        let is_form_control = matches!(
            line.kind,
            Some(
                crate::semantic::SemanticKind::Input
                    | crate::semantic::SemanticKind::Textarea
                    | crate::semantic::SemanticKind::Select
                    | crate::semantic::SemanticKind::Button
            )
        );
        // Pad attention / selected form rows to the full viewport width so the
        // reverse or magenta bar is a solid strip (not a gap after short values).
        if agent_attention || (selected && is_form_control) {
            let used: usize = spans
                .iter()
                .map(|s| s.content.chars().count())
                .sum::<usize>()
                + text.chars().count();
            if used < width {
                text.push_str(&" ".repeat(width - used));
            }
        }
        // Links: underline only the URL inside `](…)` so labels stay clean.
        if line.kind == Some(crate::semantic::SemanticKind::Link) {
            spans.extend(link_line_spans(&text, style));
        } else {
            spans.push(Span::styled(text, style));
        }
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

    // Footer cmdline: search (`/…`) or link-hint (`f …` / `F …`). Other status
    // rides after it. When neither owns the bar, show lifecycle/rev as usual.
    let cmdline = footer_cmdline(state, theme);
    let cmdline_active = cmdline.is_some();
    if let Some((cmd_text, cmd_style)) = cmdline {
        spans.push(Span::styled(cmd_text, cmd_style));
    } else {
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
    }

    // When cmdline owns the bar, still surface lifecycle errors/loading on the right.
    if cmdline_active {
        match &state.lifecycle {
            Lifecycle::Loading { action } => {
                if !spans.is_empty() {
                    spans.push(Span::raw(" │ "));
                }
                spans.push(Span::styled(
                    format!("loading:{action}"),
                    theme.status_loading(),
                ));
            }
            Lifecycle::Error { action, message } => {
                if !spans.is_empty() {
                    spans.push(Span::raw(" │ "));
                }
                spans.push(Span::styled(
                    format!("error:{action}: {message}"),
                    theme.status_error(),
                ));
            }
            Lifecycle::Ready => {}
        }
    }

    if let Some(msg) = &state.view.status_message {
        // Avoid duplicating match counts already shown as `/{q}  n/m`.
        let redundant = msg.starts_with("search:")
            || (msg == "pattern not found" && !state.view.search_query.is_empty());
        if !redundant {
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

/// Footer cmdline owner: link-hint or search (hint wins while active).
fn footer_cmdline(
    state: &crate::tui::state::TuiState,
    theme: &TuiTheme,
) -> Option<(String, Style)> {
    if let Some(line) = hint_status_line(state, theme) {
        return Some(line);
    }
    search_status_line(state, theme)
}

/// Link-hint indicator for the footer (`f` / `F`).
///
/// - Follow: `f` or `f as` while typing the two-key label
/// - New tab: `F` or `F as`
fn hint_status_line(
    state: &crate::tui::state::TuiState,
    theme: &TuiTheme,
) -> Option<(String, Style)> {
    use crate::tui::state::HintMode;
    let prefix = match &state.mode {
        InteractionMode::Hint(HintMode::Follow) => 'f',
        InteractionMode::Hint(HintMode::NewTab) => 'F',
        _ => return None,
    };
    let buf = state.view.hint_buffer.as_str();
    let text = if buf.is_empty() {
        prefix.to_string()
    } else {
        format!("{prefix} {buf}")
    };
    Some((text, theme.chrome_mode()))
}

/// Vim-style search indicator for the footer.
///
/// - While typing: `/{buffer}`
/// - After submit with matches: `/{query}  n/m`
/// - After submit with no matches: `/{query}  0/0`
fn search_status_line(
    state: &crate::tui::state::TuiState,
    theme: &TuiTheme,
) -> Option<(String, Style)> {
    if let InteractionMode::Input(InputKind::Search { buffer }) = &state.mode {
        return Some((format!("/{buffer}"), theme.chrome_mode()));
    }
    let query = state.view.search_query.as_str();
    if query.is_empty() {
        return None;
    }
    let total = state.view.search_matches.len();
    let text = if total == 0 {
        format!("/{query}  0/0")
    } else {
        let n = state.view.search_index.saturating_add(1).min(total);
        format!("/{query}  {n}/{total}")
    };
    let style = if total == 0 {
        theme.status_error()
    } else {
        theme.chrome_mode()
    };
    Some((text, style))
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

/// Single-character lifecycle marker for the header (color carries the rest).
///
/// - Ready: `●`
/// - Loading: `◐`
/// - Error: `✕`
fn lifecycle_glyph(lifecycle: &Lifecycle) -> &'static str {
    match lifecycle {
        Lifecycle::Ready => "●",
        Lifecycle::Loading { .. } => "◐",
        Lifecycle::Error { .. } => "✕",
    }
}

/// Build chrome text for tests (no terminal). Single browser-like bar.
#[allow(dead_code)]
pub fn chrome_lines(controller: &Controller) -> Vec<String> {
    let state = &controller.state;
    let tabs = state
        .tab_position
        .map(|(i, n)| format!("{i}/{n} "))
        .unwrap_or_default();
    let back = if state.can_go_back { "◀" } else { "◁" };
    let fwd = if state.can_go_forward { "▶" } else { "▷" };
    let life = lifecycle_glyph(&state.lifecycle);
    let mut flags = vec![life];
    let mode = state.mode_label();
    // Search / Hint are footer-only, not header chrome.
    if mode != "Normal" && mode != "Search" && mode != "Hint" && mode != "Hint+" {
        flags.push(mode);
    }
    let mid = if state.title().is_empty() {
        state.url().to_string()
    } else if state.url().is_empty() {
        state.title().to_string()
    } else {
        format!("{} · {}", state.url(), state.title())
    };
    vec![format!("{tabs}{back} {fwd} {mid}  {}", flags.join(" "))]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::DocumentMetadata;
    use crate::semantic::SemanticDocument;
    use crate::semantic::SemanticRef;
    use crate::tui::content::contains_shortcut_legend;
    use crate::tui::state::InputKind;

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
            assert!(
                line.contains('●'),
                "ready lifecycle should be a single glyph: {line}"
            );
            assert!(!line.contains("ready"), "no verbose ready text: {line}");
        }
    }

    #[test]
    fn chrome_shows_tab_position_left_of_history() {
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
        ctl.state.set_tab_position(Some((2, 5)));
        ctl.state.set_history_availability(true, false);
        let line = &chrome_lines(&ctl)[0];
        assert!(
            line.starts_with("2/5 ◀ ▷") || line.contains("2/5 ◀"),
            "tab ordinal should sit left of history arrows: {line}"
        );
        let tabs_at = line.find("2/5").expect("tab label");
        let back_at = line.find('◀').expect("back arrow");
        assert!(tabs_at < back_at, "2/5 before ◀: {line}");
    }

    #[test]
    fn lifecycle_glyphs_are_single_chars() {
        assert_eq!(lifecycle_glyph(&Lifecycle::Ready), "●");
        assert_eq!(
            lifecycle_glyph(&Lifecycle::Loading {
                action: "navigate".into()
            }),
            "◐"
        );
        assert_eq!(
            lifecycle_glyph(&Lifecycle::Error {
                action: "navigate".into(),
                message: "boom".into()
            }),
            "✕"
        );
        for g in ["●", "◐", "✕"] {
            assert_eq!(g.chars().count(), 1);
        }
    }

    #[test]
    fn search_status_shows_prompt_while_typing() {
        let mut ctl = Controller::new();
        ctl.state.mode = InteractionMode::Input(InputKind::Search {
            buffer: "leo".into(),
        });
        let theme = TuiTheme::new();
        let (text, _) = search_status_line(&ctl.state, &theme).expect("search prompt");
        assert_eq!(text, "/leo");
    }

    #[test]
    fn search_status_stays_while_pattern_active() {
        let mut ctl = Controller::new();
        ctl.state.view.search_query = "space".into();
        ctl.state.view.search_matches = vec![
            SemanticRef::from_opaque("r1"),
            SemanticRef::from_opaque("r2"),
            SemanticRef::from_opaque("r3"),
        ];
        ctl.state.view.search_index = 1;
        let theme = TuiTheme::new();
        let (text, _) = search_status_line(&ctl.state, &theme).expect("active search");
        assert_eq!(text, "/space  2/3");
    }

    #[test]
    fn search_status_absent_without_query() {
        let ctl = Controller::new();
        let theme = TuiTheme::new();
        assert!(search_status_line(&ctl.state, &theme).is_none());
    }

    #[test]
    fn split_markdown_link_url_isolates_href() {
        let (pre, url, suf) = split_markdown_link_url("  [Click here](https://example.com/x)").unwrap();
        assert_eq!(pre, "  [Click here](");
        assert_eq!(url, "https://example.com/x");
        assert_eq!(suf, ")");
        assert!(split_markdown_link_url("plain text").is_none());
        assert!(split_markdown_link_url("](no-open").is_none());
    }

    #[test]
    fn link_line_spans_underline_only_url() {
        use ratatui::style::{Color, Modifier, Style};
        let base = Style::default().fg(Color::Blue);
        let spans = link_line_spans("[Go](/path)", base);
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].content.as_ref(), "[Go](");
        assert!(!spans[0].style.add_modifier.contains(Modifier::UNDERLINED));
        assert_eq!(spans[1].content.as_ref(), "/path");
        assert!(spans[1].style.add_modifier.contains(Modifier::UNDERLINED));
        assert_eq!(spans[2].content.as_ref(), ")");
        assert!(!spans[2].style.add_modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn hint_status_shows_in_footer_not_empty() {
        use crate::tui::state::HintMode;
        let mut ctl = Controller::new();
        let theme = TuiTheme::new();
        ctl.state.mode = InteractionMode::Hint(HintMode::Follow);
        ctl.state.view.hint_buffer.clear();
        let (text, _) = hint_status_line(&ctl.state, &theme).expect("hint f");
        assert_eq!(text, "f");
        ctl.state.view.hint_buffer = "as".into();
        let (text, _) = hint_status_line(&ctl.state, &theme).expect("hint f as");
        assert_eq!(text, "f as");
        ctl.state.mode = InteractionMode::Hint(HintMode::NewTab);
        ctl.state.view.hint_buffer = "aa".into();
        let (text, _) = footer_cmdline(&ctl.state, &theme).expect("hint F");
        assert_eq!(text, "F aa");
    }

    #[test]
    fn horizontal_scroll_is_character_aligned() {
        assert_eq!(truncate("éclair", 3), "éc…");
    }
}

//! Terminal chrome + content rendering (no shortcut legends).
//!
//! Draws an immutable, frame-local [`RenderModel`] assembled by the controller.
//! This boundary keeps browser coordination and pending work outside rendering.
//! It contains lifecycle/mode chrome, addressable content lines (with independent
//! human selection vs agent attention styles), and status. Never renders key
//! binding legends in the UI. Colors come from [`crate::tui::theme::TuiTheme`].

use crate::tui::config::TuiLayout;
use crate::tui::content::ContentLine;
use crate::tui::hints::LinkHint;
use crate::tui::state::{InputKind, InteractionMode, Lifecycle};
use crate::tui::theme::TuiTheme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use std::time::{Duration, Instant};

/// How often the Loading lifecycle glyph advances one quarter-turn.
pub const LOADING_SPINNER_INTERVAL: Duration = Duration::from_millis(250);

/// Quarter-circle frames for the Loading header glyph (`◐` → `◓` → `◑` → `◒`).
const LOADING_SPINNER_FRAMES: &[&str] = &["◐", "◓", "◑", "◒"];

/// Owned inputs needed to paint one frame. It deliberately contains no browser,
/// coordinator, history, pending operation, or mutation surface.
#[derive(Debug, Clone)]
pub struct RenderModel {
    pub(crate) state: crate::tui::state::TuiState,
    pub(crate) content_lines: Vec<ContentLine>,
    pub(crate) hints: Vec<LinkHint>,
    pub(crate) url_completion_ghost: Option<String>,
    pub(crate) selected_last_line: Option<usize>,
}

/// Draw one frame of the terminal browser using built-in theme/layout defaults.
#[allow(dead_code)] // Convenience for callers/tests that do not load a config theme.
pub fn draw(frame: &mut Frame, model: &RenderModel) {
    draw_with_theme(frame, model, &TuiTheme::new(), TuiLayout::defaults());
}

/// Draw one frame with an explicit theme (from `tui.toml` `[theme]` overlay)
/// and content-pane layout padding (from `[layout]`).
///
/// Header and footer stay full terminal width; only the content region is inset.
pub fn draw_with_theme(
    frame: &mut Frame,
    model: &RenderModel,
    theme: &TuiTheme,
    layout: TuiLayout,
) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // chrome (single browser-like bar)
            Constraint::Min(1),    // content
            Constraint::Length(1), // status
        ])
        .split(area);

    // Reserve one right-edge column for an Amp-style block scrollbar on the
    // full content band (outside padding / max-width column).
    let band = chunks[1];
    let (content_outer, scrollbar_col) = split_scrollbar_column(band);
    let content_area = layout.content_rect(content_outer, model.state.view.full_width);

    draw_chrome(frame, chunks[0], model, theme);
    if content_area.width > 0 && content_area.height > 0 {
        draw_content(frame, content_area, model, theme);
    }
    if let Some(sb) = scrollbar_col {
        draw_content_scrollbar(
            frame,
            sb,
            model.content_lines.len(),
            model.state.view.scroll_y,
            model.state.view.viewport_height.max(1),
            theme,
        );
    }
    draw_status(frame, chunks[2], model, theme);

    if let Some(inspect) = &model.state.view.inspect_text
        && content_area.width > 0
        && content_area.height > 0
    {
        let title = model.state.view.inspect_title.as_deref().unwrap_or("");
        draw_inspect_under_selection(frame, content_area, model, inspect, title, theme);
    }
}

/// Split a 1-col scrollbar track off the right edge of the content band.
fn split_scrollbar_column(band: Rect) -> (Rect, Option<Rect>) {
    if band.width < 2 {
        return (band, None);
    }
    let content = Rect {
        x: band.x,
        y: band.y,
        width: band.width.saturating_sub(1),
        height: band.height,
    };
    let bar = Rect {
        x: band.x.saturating_add(band.width.saturating_sub(1)),
        y: band.y,
        width: 1,
        height: band.height,
    };
    (content, Some(bar))
}

/// Amp-like vertical scrollbar: solid colored block cells only (no │/▐ glyphs).
///
/// Track is a dim background strip; thumb is a brighter block segment sized by
/// the visible fraction of content. When content fits, the thumb fills the track.
fn draw_content_scrollbar(
    frame: &mut Frame,
    area: Rect,
    content_len: usize,
    scroll_y: usize,
    viewport_height: usize,
    theme: &TuiTheme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let track_h = area.height as usize;
    let vh = viewport_height.max(1);
    let (thumb_start, thumb_len) = scrollbar_thumb(content_len, scroll_y, vh, track_h);

    let track_style = theme.scrollbar_track();
    let thumb_style = theme.scrollbar_thumb();
    let mut rows = Vec::with_capacity(track_h);
    for row in 0..track_h {
        let on_thumb = row >= thumb_start && row < thumb_start.saturating_add(thumb_len);
        let style = if on_thumb { thumb_style } else { track_style };
        // Space + background only — solid blocks, no line art.
        rows.push(Line::from(Span::styled(" ", style)));
    }
    frame.render_widget(Paragraph::new(rows), area);
}

/// Thumb start row and height within a track of `track_h` rows.
fn scrollbar_thumb(
    content_len: usize,
    scroll_y: usize,
    viewport_height: usize,
    track_h: usize,
) -> (usize, usize) {
    if track_h == 0 {
        return (0, 0);
    }
    if content_len <= viewport_height {
        return (0, track_h);
    }
    // Thumb height ~ visible fraction; at least 1 cell.
    let thumb_len = ((viewport_height * track_h) / content_len.max(1)).clamp(1, track_h);
    let max_scroll = content_len.saturating_sub(viewport_height);
    let max_start = track_h.saturating_sub(thumb_len);
    let thumb_start = (scroll_y.min(max_scroll).saturating_mul(max_start))
        .checked_div(max_scroll)
        .unwrap_or(0);
    (thumb_start, thumb_len)
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
fn draw_chrome(frame: &mut Frame, area: Rect, model: &RenderModel, theme: &TuiTheme) {
    let state = &model.state;
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
    // URL mode may show a dim ghost suffix from local history (Tab accepts).
    let url_ghost = model.url_completion_ghost.clone();
    let (location, location_style, ghost_suffix) = match &state.mode {
        InteractionMode::Input(InputKind::Url { buffer }) => {
            (format!("URL {buffer}"), theme.chrome_mode(), url_ghost)
        }
        InteractionMode::Input(InputKind::Form { buffer, .. }) => {
            (format!("IN {buffer}"), theme.chrome_mode(), None)
        }
        InteractionMode::Input(InputKind::Search { .. })
        | InteractionMode::Hint(_)
        | InteractionMode::Normal => {
            let url = state.url();
            if url.is_empty() {
                (String::new(), theme.muted(), None)
            } else {
                (url.to_string(), theme.muted(), None)
            }
        }
    };

    // Right cluster: lifecycle glyph only. URL/IN buffers already show on the
    // left; Search/Hint live in the footer — no redundant mode label top-right.
    let right_parts: Vec<(&str, Style)> =
        vec![(lifecycle_glyph(&state.lifecycle), lifecycle_style)];
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
    // Leave room for a short ghost completion when editing the URL.
    let ghost_budget = ghost_suffix
        .as_ref()
        .map(|g| g.chars().count().min(budget / 2))
        .unwrap_or(0);
    let mid_budget = budget.saturating_sub(ghost_budget);
    mid = truncate(&mid, mid_budget);

    spans.push(Span::styled(mid, location_style));
    if let Some(ghost) = ghost_suffix.filter(|g| !g.is_empty()) {
        let ghost = truncate(&ghost, ghost_budget.max(1));
        if !ghost.is_empty() {
            spans.push(Span::styled(ghost, theme.muted()));
        }
    }

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

fn draw_content(frame: &mut Frame, area: Rect, model: &RenderModel, theme: &TuiTheme) {
    let state = &model.state;
    let lines = &model.content_lines;
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
            && let Some(hint) = model.hints.iter().find(|h| &h.semantic_ref == ref_r)
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
        // solid strip. Do **not** full-width-pad form controls — that left a long
        // reverse trail and odd trailing cells without bg.
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
            Lifecycle::Loading { action } => (format!("Loading {action}…"), theme.status_loading()),
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

fn draw_status(frame: &mut Frame, area: Rect, model: &RenderModel, theme: &TuiTheme) {
    let state = &model.state;
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
/// - After submit with matches: `/{query}  n/m` (clear with Esc in Normal mode)
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
    model: &RenderModel,
    inspect: &str,
    title: &str,
    theme: &TuiTheme,
) {
    let scroll = model.state.view.scroll_y;
    let vh = content_area.height as usize;

    let body_lines = inspect.lines().count().max(1);
    // Border (2) + body, capped so we never cover the whole content pane.
    let panel_h = ((body_lines + 2) as u16)
        .min(content_area.height.saturating_sub(1).max(3))
        .max(3);
    let width = content_area.width.saturating_sub(2).max(10);
    let x = content_area.x.saturating_add(1);

    // Last content-line index of the selection (handles wrap continuations).
    let last_abs = model.selected_last_line;

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
                let below = content_area
                    .y
                    .saturating_add((row_in_view as u16).saturating_add(1));
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
/// - Loading: spinning quarter-circle (`◐◓◑◒`), 250 ms per frame
/// - Error: `✕`
fn lifecycle_glyph(lifecycle: &Lifecycle) -> &'static str {
    match lifecycle {
        Lifecycle::Ready => "●",
        Lifecycle::Loading { .. } => loading_spinner_glyph(Instant::now()),
        Lifecycle::Error { .. } => "✕",
    }
}

/// Loading spinner frame for `now` (wall clock; advances every
/// [`LOADING_SPINNER_INTERVAL`]).
pub fn loading_spinner_glyph(now: Instant) -> &'static str {
    loading_spinner_glyph_at_ms(spinner_clock_ms(now))
}

fn spinner_clock_ms(now: Instant) -> u128 {
    // Process-local origin so the sequence is stable for the TUI lifetime.
    static ORIGIN: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    let origin = ORIGIN.get_or_init(Instant::now);
    now.duration_since(*origin).as_millis()
}

fn loading_spinner_glyph_at_ms(ms: u128) -> &'static str {
    let idx = (ms / LOADING_SPINNER_INTERVAL.as_millis()) as usize % LOADING_SPINNER_FRAMES.len();
    LOADING_SPINNER_FRAMES[idx]
}

/// Build chrome text for tests (no terminal). Single browser-like bar.
#[allow(dead_code)]
pub fn chrome_lines(model: &RenderModel) -> Vec<String> {
    let state = &model.state;
    let tabs = state
        .tab_position
        .map(|(i, n)| format!("{i}/{n} "))
        .unwrap_or_default();
    let back = if state.can_go_back { "◀" } else { "◁" };
    let fwd = if state.can_go_forward { "▶" } else { "▷" };
    let life = lifecycle_glyph(&state.lifecycle);
    let mid = if state.title().is_empty() {
        state.url().to_string()
    } else if state.url().is_empty() {
        state.title().to_string()
    } else {
        format!("{} · {}", state.url(), state.title())
    };
    vec![format!("{tabs}{back} {fwd} {mid}  {life}")]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::DocumentMetadata;
    use crate::semantic::SemanticDocument;
    use crate::semantic::SemanticRef;
    use crate::tui::content::contains_shortcut_legend;
    use crate::tui::state::InputKind;

    fn model() -> RenderModel {
        RenderModel {
            state: crate::tui::state::TuiState::new(),
            content_lines: Vec::new(),
            hints: Vec::new(),
            url_completion_ghost: None,
            selected_last_line: None,
        }
    }

    #[test]
    fn chrome_has_no_shortcut_legend() {
        let mut model = model();
        let doc = SemanticDocument::empty(DocumentMetadata {
            document_id: "d".into(),
            revision: "1".into(),
            url: "https://example.com/".into(),
            title: "T".into(),
            ready_state: "complete".into(),
            frames: vec![],
        })
        .unwrap();
        model.state.publish_page(doc);
        for line in chrome_lines(&model) {
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
        let mut model = model();
        let doc = SemanticDocument::empty(DocumentMetadata {
            document_id: "d".into(),
            revision: "1".into(),
            url: "https://example.com/".into(),
            title: "T".into(),
            ready_state: "complete".into(),
            frames: vec![],
        })
        .unwrap();
        model.state.publish_page(doc);
        model.state.set_tab_position(Some((2, 5)));
        model.state.set_history_availability(true, false);
        let line = &chrome_lines(&model)[0];
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
        let loading = lifecycle_glyph(&Lifecycle::Loading {
            action: "navigate".into(),
        });
        assert!(
            LOADING_SPINNER_FRAMES.contains(&loading),
            "loading should be a spinner frame, got {loading}"
        );
        assert_eq!(
            lifecycle_glyph(&Lifecycle::Error {
                action: "navigate".into(),
                message: "boom".into()
            }),
            "✕"
        );
        for g in LOADING_SPINNER_FRAMES.iter().chain(["●", "✕"].iter()) {
            assert_eq!(g.chars().count(), 1);
        }
    }

    #[test]
    fn loading_spinner_advances_every_250ms() {
        assert_eq!(loading_spinner_glyph_at_ms(0), "◐");
        assert_eq!(loading_spinner_glyph_at_ms(249), "◐");
        assert_eq!(loading_spinner_glyph_at_ms(250), "◓");
        assert_eq!(loading_spinner_glyph_at_ms(500), "◑");
        assert_eq!(loading_spinner_glyph_at_ms(750), "◒");
        assert_eq!(loading_spinner_glyph_at_ms(1000), "◐");
    }

    #[test]
    fn search_status_shows_prompt_while_typing() {
        let mut model = model();
        model.state.mode = InteractionMode::Input(InputKind::Search {
            buffer: "leo".into(),
        });
        let theme = TuiTheme::new();
        let (text, _) = search_status_line(&model.state, &theme).expect("search prompt");
        assert_eq!(text, "/leo");
    }

    #[test]
    fn search_status_stays_while_pattern_active() {
        let mut model = model();
        model.state.view.search_query = "space".into();
        model.state.view.search_matches = vec![
            SemanticRef::from_opaque("r1"),
            SemanticRef::from_opaque("r2"),
            SemanticRef::from_opaque("r3"),
        ];
        model.state.view.search_index = 1;
        let theme = TuiTheme::new();
        let (text, _) = search_status_line(&model.state, &theme).expect("active search");
        assert_eq!(text, "/space  2/3");
    }

    #[test]
    fn search_status_absent_without_query() {
        let model = model();
        let theme = TuiTheme::new();
        assert!(search_status_line(&model.state, &theme).is_none());
    }

    #[test]
    fn escape_in_normal_clears_sticky_search_footer() {
        let mut ctl = crate::tui::controller::Controller::new();
        ctl.state.view.search_query = "space".into();
        ctl.state.view.search_matches = vec![
            SemanticRef::from_opaque("r1"),
            SemanticRef::from_opaque("r2"),
        ];
        ctl.state.view.search_index = 1;
        ctl.state.mode = InteractionMode::Normal;
        let theme = TuiTheme::new();
        assert!(search_status_line(&ctl.state, &theme).is_some());
        ctl.escape();
        assert!(ctl.state.view.search_query.is_empty());
        assert!(ctl.state.view.search_matches.is_empty());
        assert!(search_status_line(&ctl.state, &theme).is_none());
    }

    #[test]
    fn escape_while_typing_search_keeps_prior_pattern() {
        let mut ctl = crate::tui::controller::Controller::new();
        ctl.state.view.search_query = "prior".into();
        ctl.state.view.search_matches = vec![SemanticRef::from_opaque("r1")];
        ctl.state.mode = InteractionMode::Input(InputKind::Search {
            buffer: "new".into(),
        });
        ctl.escape();
        assert_eq!(ctl.state.view.search_query, "prior");
        assert_eq!(ctl.state.view.search_matches.len(), 1);
        assert!(matches!(ctl.state.mode, InteractionMode::Normal));
    }

    #[test]
    fn split_markdown_link_url_isolates_href() {
        let (pre, url, suf) =
            split_markdown_link_url("  [Click here](https://example.com/x)").unwrap();
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
        let mut model = model();
        let theme = TuiTheme::new();
        model.state.mode = InteractionMode::Hint(HintMode::Follow);
        model.state.view.hint_buffer.clear();
        let (text, _) = hint_status_line(&model.state, &theme).expect("hint f");
        assert_eq!(text, "f");
        model.state.view.hint_buffer = "as".into();
        let (text, _) = hint_status_line(&model.state, &theme).expect("hint f as");
        assert_eq!(text, "f as");
        model.state.mode = InteractionMode::Hint(HintMode::NewTab);
        model.state.view.hint_buffer = "aa".into();
        let (text, _) = footer_cmdline(&model.state, &theme).expect("hint F");
        assert_eq!(text, "F aa");
    }

    #[test]
    fn horizontal_scroll_is_character_aligned() {
        assert_eq!(truncate("éclair", 3), "éc…");
    }

    #[test]
    fn scrollbar_thumb_fills_when_content_fits() {
        assert_eq!(scrollbar_thumb(10, 0, 20, 15), (0, 15));
    }

    #[test]
    fn scrollbar_thumb_moves_with_scroll() {
        // 100 lines, 10 visible, 20-row track → thumb height 2, max_start 18
        let (start0, len) = scrollbar_thumb(100, 0, 10, 20);
        assert_eq!(len, 2);
        assert_eq!(start0, 0);
        let (start_mid, _) = scrollbar_thumb(100, 45, 10, 20);
        assert!(start_mid > 0 && start_mid < 18);
        let (start_end, _) = scrollbar_thumb(100, 90, 10, 20);
        assert_eq!(start_end, 18);
    }

    #[test]
    fn split_scrollbar_column_reserves_right_edge() {
        let band = Rect {
            x: 0,
            y: 1,
            width: 80,
            height: 20,
        };
        let (content, sb) = split_scrollbar_column(band);
        assert_eq!(content.width, 79);
        let sb = sb.expect("scrollbar");
        assert_eq!(sb.x, 79);
        assert_eq!(sb.width, 1);
        assert_eq!(sb.height, 20);
    }
}

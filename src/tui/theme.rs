//! Terminal-native TUI theme (Raymon-inspired role styles).
//!
//! Defaults use the terminal's ANSI-16 colors and leave base fg/bg unset so
//! light/dark Base16 themes inherit correctly. Selection is applied last as
//! reverse video so it stays visible over kind colors.

use crate::semantic::SemanticKind;
use ratatui::style::{Color, Modifier, Style};

/// Semantic style roles for chrome and content rendering.
#[derive(Debug, Clone, Default)]
pub struct TuiTheme;

impl TuiTheme {
    pub fn new() -> Self {
        Self
    }

    /// Base style: do not force fg/bg (terminal default).
    pub fn base(&self) -> Style {
        Style::default()
    }

    /// Full-width white fill for header/footer chrome bars.
    ///
    /// Solid white background separates chrome from the content pane. Text on
    /// the bar uses gray / black / green only — no per-span backgrounds.
    pub fn chrome_bar(&self) -> Style {
        Style::default().fg(Color::Black).bg(Color::White)
    }

    /// Accent text on the white chrome bar (green).
    pub fn chrome_mode(&self) -> Style {
        self.chrome_bar().fg(Color::Green).add_modifier(Modifier::BOLD)
    }

    pub fn chrome_ready(&self) -> Style {
        self.chrome_bar().fg(Color::Green).add_modifier(Modifier::BOLD)
    }

    pub fn chrome_loading(&self) -> Style {
        // Yellow on white is hard to read; use black bold for loading on the bar.
        self.chrome_bar().fg(Color::Black).add_modifier(Modifier::BOLD)
    }

    pub fn chrome_error(&self) -> Style {
        // Keep error legible on white without a second background color.
        self.chrome_bar().fg(Color::Black).add_modifier(Modifier::BOLD)
    }

    pub fn chrome_wrap(&self) -> Style {
        self.chrome_bar().fg(Color::DarkGray)
    }

    pub fn chrome_hist_enabled(&self) -> Style {
        self.chrome_bar().fg(Color::Green).add_modifier(Modifier::BOLD)
    }

    pub fn chrome_hist_disabled(&self) -> Style {
        self.chrome_bar().fg(Color::DarkGray)
    }

    /// Muted text in the content pane.
    pub fn muted(&self) -> Style {
        self.base().fg(Color::DarkGray)
    }

    /// Muted / secondary text on the white chrome bar.
    pub fn bar_muted(&self) -> Style {
        self.chrome_bar().fg(Color::DarkGray)
    }

    /// Error text in the content pane.
    pub fn status_error(&self) -> Style {
        self.base()
            .fg(Color::Red)
            .add_modifier(Modifier::BOLD)
    }

    /// Loading text in the content pane.
    pub fn status_loading(&self) -> Style {
        self.base().fg(Color::Yellow)
    }

    /// Error text on the white chrome bar.
    pub fn bar_status_error(&self) -> Style {
        self.chrome_error()
    }

    /// Loading text on the white chrome bar.
    pub fn bar_status_loading(&self) -> Style {
        self.chrome_loading()
    }

    /// OK / positive text on the white chrome bar.
    pub fn bar_status_ok(&self) -> Style {
        self.chrome_ready()
    }

    /// Scrollbar track (grey line on white gutter).
    pub fn scrollbar_track(&self) -> Style {
        Style::default().fg(Color::DarkGray).bg(Color::White)
    }

    /// Scrollbar thumb (darker mark on white gutter).
    pub fn scrollbar_thumb(&self) -> Style {
        Style::default().fg(Color::Black).bg(Color::White)
    }

    pub fn attention_overlay(&self) -> Style {
        // Background fill is the readable cue: fg-only magenta was invisible on
        // cyan h2s (and images already use magenta). Selection still wins via
        // reverse when both apply to the same line.
        Style::default()
            .fg(Color::Black)
            .bg(Color::Magenta)
            .add_modifier(Modifier::BOLD)
    }

    /// Applied last so selection remains visible over kind colors.
    pub fn selection_overlay(&self) -> Style {
        Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
    }

    pub fn hint_label(&self) -> Style {
        self.base()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    }

    /// Style for a content line from its semantic kind and optional heading level.
    pub fn content_style(&self, kind: Option<SemanticKind>, heading_level: Option<u8>) -> Style {
        match kind {
            Some(SemanticKind::Heading) => self.heading(heading_level.unwrap_or(2)),
            Some(SemanticKind::Link) => self
                .base()
                .fg(Color::LightBlue)
                .add_modifier(Modifier::UNDERLINED),
            Some(SemanticKind::Image) => self.base().fg(Color::Magenta),
            Some(SemanticKind::Landmark) => self
                .base()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
            Some(SemanticKind::Group) => self.base().fg(Color::Cyan),
            Some(SemanticKind::List) => self.muted(),
            Some(SemanticKind::ListItem) => self.base(),
            Some(SemanticKind::Input)
            | Some(SemanticKind::Textarea)
            | Some(SemanticKind::Select)
            | Some(SemanticKind::Button) => self
                .base()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
            Some(SemanticKind::Text) | None => self.base(),
        }
    }

    fn heading(&self, level: u8) -> Style {
        match level {
            1 => self
                .base()
                .fg(Color::LightBlue)
                .add_modifier(Modifier::BOLD),
            2 => self.base().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            3 => self.base().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            _ => self.base().fg(Color::Cyan),
        }
    }

    /// Layer kind → attention → selection (selection wins).
    pub fn line_style(
        &self,
        kind: Option<SemanticKind>,
        heading_level: Option<u8>,
        selected: bool,
        attention: bool,
    ) -> Style {
        let mut style = self.content_style(kind, heading_level);
        if attention {
            style = style.patch(self.attention_overlay());
        }
        if selected {
            style = style.patch(self.selection_overlay());
        }
        style
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_overlay_includes_reverse() {
        let theme = TuiTheme::new();
        let style = theme.line_style(Some(SemanticKind::Link), None, true, true);
        assert!(style.add_modifier.contains(Modifier::REVERSED));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn chrome_bar_is_white_with_dark_text() {
        let theme = TuiTheme::new();
        assert_eq!(theme.chrome_bar().bg, Some(Color::White));
        assert_eq!(theme.chrome_bar().fg, Some(Color::Black));
        assert_eq!(theme.chrome_mode().bg, Some(Color::White));
        assert_eq!(theme.chrome_mode().fg, Some(Color::Green));
        assert_eq!(theme.bar_muted().bg, Some(Color::White));
        assert_eq!(theme.bar_muted().fg, Some(Color::DarkGray));
        // No reverse-video on chrome; content stays on default terminal bg.
        assert!(!theme.chrome_bar().add_modifier.contains(Modifier::REVERSED));
        assert_ne!(theme.base().bg, Some(Color::White));
        assert_eq!(theme.scrollbar_track().bg, Some(Color::White));
        assert_eq!(theme.scrollbar_track().fg, Some(Color::DarkGray));
        assert_eq!(theme.scrollbar_thumb().bg, Some(Color::White));
        assert_eq!(theme.scrollbar_thumb().fg, Some(Color::Black));
    }

    #[test]
    fn attention_overlay_uses_magenta_background() {
        let theme = TuiTheme::new();
        // Attention alone: solid magenta bar (not fg-only).
        let style = theme.line_style(Some(SemanticKind::Heading), Some(2), false, true);
        assert_eq!(style.bg, Some(Color::Magenta));
        assert_eq!(style.fg, Some(Color::Black));
        assert!(style.add_modifier.contains(Modifier::BOLD));
        // Selection still wins when both apply.
        let both = theme.line_style(Some(SemanticKind::Heading), Some(2), true, true);
        assert!(both.add_modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn heading_levels_differ() {
        let theme = TuiTheme::new();
        let h1 = theme.content_style(Some(SemanticKind::Heading), Some(1));
        let h6 = theme.content_style(Some(SemanticKind::Heading), Some(6));
        assert_ne!(h1.fg, h6.fg);
    }
}

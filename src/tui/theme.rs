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

    /// Full-width inverted fill for header/footer chrome bars.
    ///
    /// Reverse video separates chrome from the content pane in both light and
    /// dark terminals without hard-coding absolute bar colors.
    pub fn chrome_bar(&self) -> Style {
        Style::default().add_modifier(Modifier::REVERSED)
    }

    /// Content-pane style patched onto the inverted chrome bar.
    pub fn on_bar(&self, style: Style) -> Style {
        style.add_modifier(Modifier::REVERSED)
    }

    pub fn chrome_mode(&self) -> Style {
        self.on_bar(
            self.base()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )
    }

    pub fn chrome_ready(&self) -> Style {
        self.on_bar(self.base().fg(Color::Green).add_modifier(Modifier::BOLD))
    }

    pub fn chrome_loading(&self) -> Style {
        self.on_bar(self.base().fg(Color::Yellow).add_modifier(Modifier::BOLD))
    }

    pub fn chrome_error(&self) -> Style {
        self.on_bar(
            self.base()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD),
        )
    }

    pub fn chrome_wrap(&self) -> Style {
        self.on_bar(self.base().fg(Color::Cyan))
    }

    pub fn chrome_hist_enabled(&self) -> Style {
        self.on_bar(self.base().fg(Color::Green).add_modifier(Modifier::BOLD))
    }

    pub fn chrome_hist_disabled(&self) -> Style {
        self.on_bar(self.base().add_modifier(Modifier::DIM))
    }

    /// Muted text in the content pane (not bar-inverted).
    pub fn muted(&self) -> Style {
        self.base().fg(Color::DarkGray)
    }

    /// Muted text on the inverted chrome bar.
    pub fn bar_muted(&self) -> Style {
        self.on_bar(self.base().add_modifier(Modifier::DIM))
    }

    /// Error text in the content pane (not bar-inverted).
    pub fn status_error(&self) -> Style {
        self.base()
            .fg(Color::Red)
            .add_modifier(Modifier::BOLD)
    }

    /// Loading text in the content pane (not bar-inverted).
    pub fn status_loading(&self) -> Style {
        self.base().fg(Color::Yellow)
    }

    /// Error text on the inverted chrome bar.
    pub fn bar_status_error(&self) -> Style {
        self.chrome_error()
    }

    /// Loading text on the inverted chrome bar.
    pub fn bar_status_loading(&self) -> Style {
        self.chrome_loading()
    }

    /// OK / positive text on the inverted chrome bar.
    pub fn bar_status_ok(&self) -> Style {
        self.chrome_ready()
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
    fn chrome_bar_is_reversed_content_is_not() {
        let theme = TuiTheme::new();
        assert!(theme.chrome_bar().add_modifier.contains(Modifier::REVERSED));
        assert!(theme.chrome_mode().add_modifier.contains(Modifier::REVERSED));
        assert!(theme.bar_muted().add_modifier.contains(Modifier::REVERSED));
        // Content-pane styles must stay non-inverted so bars read as separate strips.
        assert!(!theme.base().add_modifier.contains(Modifier::REVERSED));
        assert!(!theme.muted().add_modifier.contains(Modifier::REVERSED));
        assert!(!theme.status_loading().add_modifier.contains(Modifier::REVERSED));
        assert!(
            !theme
                .content_style(Some(SemanticKind::Text), None)
                .add_modifier
                .contains(Modifier::REVERSED)
        );
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

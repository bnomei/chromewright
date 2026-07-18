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

    pub fn chrome_title(&self) -> Style {
        self.base().add_modifier(Modifier::BOLD)
    }

    pub fn chrome_mode(&self) -> Style {
        self.base()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    }

    pub fn chrome_ready(&self) -> Style {
        self.base().fg(Color::Green)
    }

    pub fn chrome_loading(&self) -> Style {
        self.base().fg(Color::Yellow)
    }

    pub fn chrome_error(&self) -> Style {
        self.base()
            .fg(Color::Red)
            .add_modifier(Modifier::BOLD)
    }

    pub fn chrome_wrap(&self) -> Style {
        self.base().fg(Color::Cyan)
    }

    pub fn chrome_hist_enabled(&self) -> Style {
        self.base().fg(Color::Green)
    }

    pub fn chrome_hist_disabled(&self) -> Style {
        self.muted()
    }

    pub fn muted(&self) -> Style {
        self.base().fg(Color::DarkGray)
    }

    pub fn status_error(&self) -> Style {
        self.chrome_error()
    }

    pub fn status_loading(&self) -> Style {
        self.chrome_loading()
    }

    pub fn status_ok(&self) -> Style {
        self.chrome_ready()
    }

    pub fn attention_overlay(&self) -> Style {
        Style::default().add_modifier(Modifier::UNDERLINED | Modifier::BOLD)
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
    fn heading_levels_differ() {
        let theme = TuiTheme::new();
        let h1 = theme.content_style(Some(SemanticKind::Heading), Some(1));
        let h6 = theme.content_style(Some(SemanticKind::Heading), Some(6));
        assert_ne!(h1.fg, h6.fg);
    }
}

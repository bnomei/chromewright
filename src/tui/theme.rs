//! Terminal-native TUI theme (ANSI-16 role palette).
//!
//! Defaults use the terminal's 16-color palette and leave base fg/bg unset so
//! light/dark themes inherit correctly. Selection is applied last as reverse
//! video so it stays visible over kind colors.
//!
//! Heading levels and links use a clearer ladder for markdown-like reading
//! (inspired by md-tui's role idea, not a 1:1 color match). Optional
//! `[theme]` keys in `tui.toml` override individual roles.

use crate::semantic::SemanticKind;
use ratatui::style::{Color, Modifier, Style};
use std::collections::HashMap;
use std::fmt;

/// Named content/chrome roles that can be overridden in TOML.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThemeRole {
    Link,
    H1,
    H2,
    H3,
    H4,
    H5,
    H6,
    Landmark,
    Group,
    List,
    Image,
    FormControl,
    HintLabel,
    Muted,
    ChromeReady,
    ChromeLoading,
    ChromeError,
    ChromeMode,
    AttentionFg,
    AttentionBg,
}

impl ThemeRole {
    pub fn name(self) -> &'static str {
        match self {
            Self::Link => "link",
            Self::H1 => "h1",
            Self::H2 => "h2",
            Self::H3 => "h3",
            Self::H4 => "h4",
            Self::H5 => "h5",
            Self::H6 => "h6",
            Self::Landmark => "landmark",
            Self::Group => "group",
            Self::List => "list",
            Self::Image => "image",
            Self::FormControl => "form_control",
            Self::HintLabel => "hint_label",
            Self::Muted => "muted",
            Self::ChromeReady => "chrome_ready",
            Self::ChromeLoading => "chrome_loading",
            Self::ChromeError => "chrome_error",
            Self::ChromeMode => "chrome_mode",
            Self::AttentionFg => "attention_fg",
            Self::AttentionBg => "attention_bg",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "link" => Some(Self::Link),
            "h1" => Some(Self::H1),
            "h2" => Some(Self::H2),
            "h3" => Some(Self::H3),
            "h4" => Some(Self::H4),
            "h5" => Some(Self::H5),
            "h6" => Some(Self::H6),
            "landmark" => Some(Self::Landmark),
            "group" => Some(Self::Group),
            "list" => Some(Self::List),
            "image" => Some(Self::Image),
            "form_control" | "form" => Some(Self::FormControl),
            "hint_label" | "hint" => Some(Self::HintLabel),
            "muted" => Some(Self::Muted),
            "chrome_ready" => Some(Self::ChromeReady),
            "chrome_loading" => Some(Self::ChromeLoading),
            "chrome_error" => Some(Self::ChromeError),
            "chrome_mode" => Some(Self::ChromeMode),
            "attention_fg" => Some(Self::AttentionFg),
            "attention_bg" => Some(Self::AttentionBg),
            _ => None,
        }
    }

    fn all() -> &'static [ThemeRole] {
        &[
            Self::Link,
            Self::H1,
            Self::H2,
            Self::H3,
            Self::H4,
            Self::H5,
            Self::H6,
            Self::Landmark,
            Self::Group,
            Self::List,
            Self::Image,
            Self::FormControl,
            Self::HintLabel,
            Self::Muted,
            Self::ChromeReady,
            Self::ChromeLoading,
            Self::ChromeError,
            Self::ChromeMode,
            Self::AttentionFg,
            Self::AttentionBg,
        ]
    }
}

/// ANSI-16 role colors. `None` means "use terminal default" for that channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemePalette {
    colors: HashMap<ThemeRole, Color>,
}

impl Default for ThemePalette {
    fn default() -> Self {
        Self::defaults()
    }
}

impl ThemePalette {
    /// Built-in palette: clearer hierarchy within ANSI-16, not a 1:1 md-tui copy.
    pub fn defaults() -> Self {
        let mut colors = HashMap::new();
        // Content — spread the ladder so prose is scannable.
        colors.insert(ThemeRole::Link, Color::Blue);
        colors.insert(ThemeRole::H1, Color::LightBlue);
        colors.insert(ThemeRole::H2, Color::Green);
        colors.insert(ThemeRole::H3, Color::Magenta);
        colors.insert(ThemeRole::H4, Color::Cyan);
        colors.insert(ThemeRole::H5, Color::Yellow);
        colors.insert(ThemeRole::H6, Color::LightRed);
        colors.insert(ThemeRole::Landmark, Color::Green);
        colors.insert(ThemeRole::Group, Color::DarkGray);
        colors.insert(ThemeRole::List, Color::DarkGray);
        // Images are low-priority chrome in the semantic view — keep them muted.
        colors.insert(ThemeRole::Image, Color::DarkGray);
        // Browser-only roles stay distinct from each other.
        colors.insert(ThemeRole::FormControl, Color::LightCyan);
        colors.insert(ThemeRole::HintLabel, Color::Yellow);
        colors.insert(ThemeRole::Muted, Color::DarkGray);
        // Chrome / status
        colors.insert(ThemeRole::ChromeReady, Color::Green);
        colors.insert(ThemeRole::ChromeLoading, Color::Yellow);
        colors.insert(ThemeRole::ChromeError, Color::Red);
        colors.insert(ThemeRole::ChromeMode, Color::Green);
        colors.insert(ThemeRole::AttentionFg, Color::Black);
        colors.insert(ThemeRole::AttentionBg, Color::Magenta);
        Self { colors }
    }

    pub fn get(&self, role: ThemeRole) -> Option<Color> {
        self.colors.get(&role).copied()
    }

    pub fn set(&mut self, role: ThemeRole, color: Color) {
        self.colors.insert(role, color);
    }

    /// Overlay `role = "color"` entries. Unknown roles or colors fail closed.
    pub fn overlay_from_map(
        &self,
        overrides: &HashMap<String, String>,
    ) -> Result<Self, ThemeError> {
        let mut next = self.clone();
        for (name, spec) in overrides {
            let role = ThemeRole::from_name(name)
                .ok_or_else(|| ThemeError::UnknownRole(name.clone()))?;
            let color = parse_color(spec)?;
            next.set(role, color);
        }
        Ok(next)
    }
}

/// Semantic style roles for chrome and content rendering.
#[derive(Debug, Clone)]
pub struct TuiTheme {
    palette: ThemePalette,
}

impl Default for TuiTheme {
    fn default() -> Self {
        Self::new()
    }
}

impl TuiTheme {
    pub fn new() -> Self {
        Self {
            palette: ThemePalette::defaults(),
        }
    }

    pub fn with_palette(palette: ThemePalette) -> Self {
        Self { palette }
    }

    pub fn palette(&self) -> &ThemePalette {
        &self.palette
    }

    fn fg(&self, role: ThemeRole) -> Style {
        match self.palette.get(role) {
            Some(c) => self.base().fg(c),
            None => self.base(),
        }
    }

    /// Base style: do not force fg/bg (terminal default).
    pub fn base(&self) -> Style {
        Style::default()
    }

    pub fn chrome_mode(&self) -> Style {
        self.fg(ThemeRole::ChromeMode).add_modifier(Modifier::BOLD)
    }

    pub fn chrome_ready(&self) -> Style {
        self.fg(ThemeRole::ChromeReady)
    }

    pub fn chrome_loading(&self) -> Style {
        self.fg(ThemeRole::ChromeLoading)
    }

    pub fn chrome_error(&self) -> Style {
        self.fg(ThemeRole::ChromeError).add_modifier(Modifier::BOLD)
    }

    pub fn chrome_wrap(&self) -> Style {
        self.fg(ThemeRole::Group)
    }

    pub fn chrome_hist_enabled(&self) -> Style {
        self.fg(ThemeRole::ChromeReady)
    }

    pub fn chrome_hist_disabled(&self) -> Style {
        self.muted()
    }

    pub fn muted(&self) -> Style {
        self.fg(ThemeRole::Muted)
    }

    /// Scrollbar track: solid dim block (background fill, no glyphs).
    pub fn scrollbar_track(&self) -> Style {
        // DarkGray bg reads as a quiet rail on both light and dark terminals.
        Style::default().bg(Color::DarkGray).fg(Color::DarkGray)
    }

    /// Scrollbar thumb: brighter solid block (Amp-style, no │/▐ line art).
    pub fn scrollbar_thumb(&self) -> Style {
        Style::default().bg(Color::Gray).fg(Color::Gray)
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
        // Background fill is the readable cue: fg-only was invisible on
        // colored headings (and image lines are muted gray). Selection still
        // wins via reverse when both apply.
        let mut style = Style::default().add_modifier(Modifier::BOLD);
        if let Some(fg) = self.palette.get(ThemeRole::AttentionFg) {
            style = style.fg(fg);
        }
        if let Some(bg) = self.palette.get(ThemeRole::AttentionBg) {
            style = style.bg(bg);
        }
        style
    }

    /// Applied last so selection remains visible over kind colors.
    pub fn selection_overlay(&self) -> Style {
        Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
    }

    /// Jump-label chrome (`[aa]`) in hint mode — distinct from form field content.
    pub fn hint_label(&self) -> Style {
        self.fg(ThemeRole::HintLabel).add_modifier(Modifier::BOLD)
    }

    /// Form controls (input/textarea/select/button) in the content pane.
    pub fn form_control(&self) -> Style {
        self.fg(ThemeRole::FormControl).add_modifier(Modifier::BOLD)
    }

    /// Style for a content line from its semantic kind and optional heading level.
    pub fn content_style(&self, kind: Option<SemanticKind>, heading_level: Option<u8>) -> Style {
        match kind {
            Some(SemanticKind::Heading) => self.heading(heading_level.unwrap_or(2)),
            // Link color only; underline is applied to the URL span in render
            // (`[label](url)`), not the whole line.
            Some(SemanticKind::Link) => self.fg(ThemeRole::Link),
            Some(SemanticKind::Image) => self.fg(ThemeRole::Image),
            Some(SemanticKind::Landmark) => {
                self.fg(ThemeRole::Landmark).add_modifier(Modifier::BOLD)
            }
            Some(SemanticKind::Group) => self.fg(ThemeRole::Group),
            Some(SemanticKind::List) => self.fg(ThemeRole::List),
            Some(SemanticKind::ListItem) => self.base(),
            Some(SemanticKind::Input)
            | Some(SemanticKind::Textarea)
            | Some(SemanticKind::Select)
            | Some(SemanticKind::Button) => self.form_control(),
            Some(SemanticKind::Text) | None => self.base(),
        }
    }

    fn heading(&self, level: u8) -> Style {
        let role = match level {
            1 => ThemeRole::H1,
            2 => ThemeRole::H2,
            3 => ThemeRole::H3,
            4 => ThemeRole::H4,
            5 => ThemeRole::H5,
            _ => ThemeRole::H6,
        };
        let style = self.fg(role);
        // H1–H3 stay bold for scan weight; lower levels are color-only.
        if level <= 3 {
            style.add_modifier(Modifier::BOLD)
        } else {
            style
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

/// Parse a TOML color name into a ratatui [`Color`].
///
/// Accepts ANSI names (`blue`, `lightcyan`, …), `reset`/`default` (maps to
/// `Reset`), and `#rrggbb` hex. Empty string is rejected (use `reset`).
pub fn parse_color(spec: &str) -> Result<Color, ThemeError> {
    let s = spec.trim();
    if s.is_empty() {
        return Err(ThemeError::InvalidColor(spec.to_string()));
    }
    let lower = s.to_ascii_lowercase();
    match lower.as_str() {
        "reset" | "default" | "none" => Ok(Color::Reset),
        "black" => Ok(Color::Black),
        "red" => Ok(Color::Red),
        "green" => Ok(Color::Green),
        "yellow" => Ok(Color::Yellow),
        "blue" => Ok(Color::Blue),
        "magenta" | "purple" => Ok(Color::Magenta),
        "cyan" => Ok(Color::Cyan),
        "gray" | "grey" | "darkgray" | "darkgrey" => Ok(Color::DarkGray),
        "white" => Ok(Color::White),
        "lightred" => Ok(Color::LightRed),
        "lightgreen" => Ok(Color::LightGreen),
        "lightyellow" => Ok(Color::LightYellow),
        "lightblue" => Ok(Color::LightBlue),
        "lightmagenta" => Ok(Color::LightMagenta),
        "lightcyan" => Ok(Color::LightCyan),
        "lightgray" | "lightgrey" => Ok(Color::Gray),
        hex if hex.starts_with('#') && hex.len() == 7 => {
            let r = u8::from_str_radix(&hex[1..3], 16)
                .map_err(|_| ThemeError::InvalidColor(spec.to_string()))?;
            let g = u8::from_str_radix(&hex[3..5], 16)
                .map_err(|_| ThemeError::InvalidColor(spec.to_string()))?;
            let b = u8::from_str_radix(&hex[5..7], 16)
                .map_err(|_| ThemeError::InvalidColor(spec.to_string()))?;
            Ok(Color::Rgb(r, g, b))
        }
        _ => Err(ThemeError::InvalidColor(spec.to_string())),
    }
}

/// Theme configuration errors (fail startup when loading TOML).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeError {
    UnknownRole(String),
    InvalidColor(String),
}

impl fmt::Display for ThemeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownRole(name) => {
                write!(
                    f,
                    "unknown theme role '{name}' (valid: {})",
                    ThemeRole::all()
                        .iter()
                        .map(|r| r.name())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            Self::InvalidColor(spec) => write!(
                f,
                "invalid theme color '{spec}' (use ANSI names like blue/lightcyan, reset, or #rrggbb)"
            ),
        }
    }
}

impl std::error::Error for ThemeError {}

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
    fn attention_overlay_uses_magenta_background() {
        let theme = TuiTheme::new();
        let style = theme.line_style(Some(SemanticKind::Heading), Some(2), false, true);
        assert_eq!(style.bg, Some(Color::Magenta));
        assert_eq!(style.fg, Some(Color::Black));
        assert!(style.add_modifier.contains(Modifier::BOLD));
        let both = theme.line_style(Some(SemanticKind::Heading), Some(2), true, true);
        assert!(both.add_modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn heading_levels_use_distinct_ladder() {
        let theme = TuiTheme::new();
        let h1 = theme.content_style(Some(SemanticKind::Heading), Some(1));
        let h2 = theme.content_style(Some(SemanticKind::Heading), Some(2));
        let h3 = theme.content_style(Some(SemanticKind::Heading), Some(3));
        let h6 = theme.content_style(Some(SemanticKind::Heading), Some(6));
        assert_eq!(h1.fg, Some(Color::LightBlue));
        assert_eq!(h2.fg, Some(Color::Green));
        assert_eq!(h3.fg, Some(Color::Magenta));
        assert_eq!(h6.fg, Some(Color::LightRed));
        assert_ne!(h1.fg, h2.fg);
        assert_ne!(h2.fg, h3.fg);
        assert_ne!(h3.fg, h6.fg);
    }

    #[test]
    fn link_is_blue_without_full_line_underline() {
        let theme = TuiTheme::new();
        let link = theme.content_style(Some(SemanticKind::Link), None);
        assert_eq!(link.fg, Some(Color::Blue));
        // Underline is applied only to the URL span in render, not the whole line.
        assert!(!link.add_modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn hint_labels_differ_from_form_controls() {
        let theme = TuiTheme::new();
        let hint = theme.hint_label();
        let input = theme.content_style(Some(SemanticKind::Input), None);
        let button = theme.content_style(Some(SemanticKind::Button), None);
        assert_eq!(hint.fg, Some(Color::Yellow));
        assert_eq!(input.fg, Some(Color::LightCyan));
        assert_eq!(button.fg, Some(Color::LightCyan));
        assert_ne!(hint.fg, input.fg);
    }

    #[test]
    fn palette_overlay_replaces_named_roles() {
        let mut overrides = HashMap::new();
        overrides.insert("h2".into(), "yellow".into());
        overrides.insert("link".into(), "lightblue".into());
        let palette = ThemePalette::defaults()
            .overlay_from_map(&overrides)
            .expect("overlay");
        let theme = TuiTheme::with_palette(palette);
        assert_eq!(
            theme.content_style(Some(SemanticKind::Heading), Some(2)).fg,
            Some(Color::Yellow)
        );
        assert_eq!(
            theme.content_style(Some(SemanticKind::Link), None).fg,
            Some(Color::LightBlue)
        );
        // Untouched roles stay default.
        assert_eq!(
            theme.content_style(Some(SemanticKind::Heading), Some(3)).fg,
            Some(Color::Magenta)
        );
    }

    #[test]
    fn unknown_role_and_color_fail() {
        let mut bad_role = HashMap::new();
        bad_role.insert("not_a_role".into(), "blue".into());
        assert!(matches!(
            ThemePalette::defaults().overlay_from_map(&bad_role),
            Err(ThemeError::UnknownRole(_))
        ));
        let mut bad_color = HashMap::new();
        bad_color.insert("h1".into(), "blurple".into());
        assert!(matches!(
            ThemePalette::defaults().overlay_from_map(&bad_color),
            Err(ThemeError::InvalidColor(_))
        ));
    }

    #[test]
    fn parse_color_accepts_ansi_and_hex() {
        assert_eq!(parse_color("blue").unwrap(), Color::Blue);
        assert_eq!(parse_color("LightCyan").unwrap(), Color::LightCyan);
        assert_eq!(parse_color("reset").unwrap(), Color::Reset);
        assert_eq!(parse_color("#ff00aa").unwrap(), Color::Rgb(255, 0, 170));
    }
}

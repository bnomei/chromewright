//! TOML configuration: keymap + theme + layout overlays.
//!
//! Loads Action-name keymap bindings, optional `[theme]` color roles, and
//! optional `[layout]` content padding onto built-in defaults. Explicit paths
//! must exist and parse; a missing XDG default file is valid and keeps built-ins.

use crate::tui::keymap::{KeymapError, TuiKeymap};
use crate::tui::theme::{ThemeError, ThemePalette, TuiTheme};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Maximum accepted content padding on either axis (cols or rows per side).
pub const MAX_CONTENT_PADDING: u16 = 32;

/// Content-pane padding (header/footer chrome stay full width).
///
/// `content_padding_x` is applied left and right; `content_padding_y` top and
/// bottom. Defaults are 1 column horizontal and 0 rows vertical.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TuiLayout {
    pub content_padding_x: u16,
    pub content_padding_y: u16,
}

impl TuiLayout {
    /// Built-in defaults: 1 col left/right, 0 row top/bottom.
    pub const fn defaults() -> Self {
        Self {
            content_padding_x: 1,
            content_padding_y: 0,
        }
    }

    /// Inner content size after applying symmetric padding to a terminal content
    /// region of `outer_width` × `outer_height` (chrome already excluded).
    ///
    /// Returns at least 1×1 so wrap/selection math never sees a zero viewport.
    pub fn content_viewport_size(self, outer_width: u16, outer_height: u16) -> (usize, usize) {
        let pad_x = self.content_padding_x.saturating_mul(2);
        let pad_y = self.content_padding_y.saturating_mul(2);
        let width = outer_width.saturating_sub(pad_x).max(1) as usize;
        let height = outer_height.saturating_sub(pad_y).max(1) as usize;
        (width, height)
    }

    /// Inset `outer` by the configured padding. Saturates safely when padding
    /// consumes the whole area (returns a 0×0 rect at the inner origin).
    pub fn inset_content_rect(self, outer: ratatui::layout::Rect) -> ratatui::layout::Rect {
        let pad_x = self.content_padding_x;
        let pad_y = self.content_padding_y;
        let x = outer.x.saturating_add(pad_x);
        let y = outer.y.saturating_add(pad_y);
        let width = outer.width.saturating_sub(pad_x.saturating_mul(2));
        let height = outer.height.saturating_sub(pad_y.saturating_mul(2));
        ratatui::layout::Rect {
            x,
            y,
            width,
            height,
        }
    }
}

impl Default for TuiLayout {
    fn default() -> Self {
        Self::defaults()
    }
}

/// Loaded TUI configuration (keymap + theme + layout overlays).
#[derive(Debug, Clone)]
pub struct TuiConfig {
    pub keymap: TuiKeymap,
    pub theme: TuiTheme,
    pub layout: TuiLayout,
    /// Path that was loaded, if any file was read.
    pub loaded_from: Option<PathBuf>,
}

impl TuiConfig {
    /// Defaults with no file loaded.
    pub fn defaults() -> Self {
        Self {
            keymap: TuiKeymap::defaults(),
            theme: TuiTheme::new(),
            layout: TuiLayout::defaults(),
            loaded_from: None,
        }
    }
}

/// Resolve configuration path precedence: explicit CLI path, else XDG default.
///
/// Missing default path is valid (returns defaults). Explicit path must exist and parse.
pub fn load_tui_config(explicit: Option<&Path>) -> Result<TuiConfig, ConfigError> {
    match explicit {
        Some(path) => load_from_path(path, true),
        None => {
            let default = default_config_path();
            if default.as_ref().is_some_and(|p| p.is_file()) {
                load_from_path(default.as_ref().unwrap(), false)
            } else {
                Ok(TuiConfig::defaults())
            }
        }
    }
}

/// XDG config path: `$XDG_CONFIG_HOME/chromewright/tui.toml` or `~/.config/chromewright/tui.toml`.
pub fn default_config_path() -> Option<PathBuf> {
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(root.join("chromewright").join("tui.toml"))
}

fn load_from_path(path: &Path, required: bool) -> Result<TuiConfig, ConfigError> {
    let raw = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(err) if !required && err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(TuiConfig::defaults());
        }
        Err(err) => {
            return Err(ConfigError::Io {
                path: path.to_path_buf(),
                message: err.to_string(),
            });
        }
    };

    let file: ConfigFile = toml::from_str(&raw).map_err(|err| ConfigError::Parse {
        path: path.to_path_buf(),
        message: err.to_string(),
    })?;

    let keymap_overrides = file.keymap.unwrap_or_default();
    let keymap = TuiKeymap::defaults()
        .overlay_from_map(&keymap_overrides)
        .map_err(|err| ConfigError::Keymap {
            path: path.to_path_buf(),
            message: err.to_string(),
        })?;

    let theme_overrides = file.theme.unwrap_or_default();
    let palette = ThemePalette::defaults()
        .overlay_from_map(&theme_overrides)
        .map_err(|err| ConfigError::Theme {
            path: path.to_path_buf(),
            message: err.to_string(),
        })?;

    let layout = parse_layout(file.layout.as_ref()).map_err(|message| ConfigError::Layout {
        path: path.to_path_buf(),
        message,
    })?;

    Ok(TuiConfig {
        keymap,
        theme: TuiTheme::with_palette(palette),
        layout,
        loaded_from: Some(path.to_path_buf()),
    })
}

fn parse_layout(raw: Option<&LayoutFile>) -> Result<TuiLayout, String> {
    let Some(raw) = raw else {
        return Ok(TuiLayout::defaults());
    };

    let mut layout = TuiLayout::defaults();
    if let Some(x) = raw.content_padding_x {
        layout.content_padding_x = validate_padding("content_padding_x", x)?;
    }
    if let Some(y) = raw.content_padding_y {
        layout.content_padding_y = validate_padding("content_padding_y", y)?;
    }
    Ok(layout)
}

fn validate_padding(name: &str, value: i64) -> Result<u16, String> {
    if value < 0 {
        return Err(format!("{name} must be >= 0, got {value}"));
    }
    if value > i64::from(MAX_CONTENT_PADDING) {
        return Err(format!(
            "{name} must be <= {MAX_CONTENT_PADDING}, got {value}"
        ));
    }
    Ok(value as u16)
}

#[derive(Debug, Clone, serde::Deserialize, Default)]
struct ConfigFile {
    #[serde(default)]
    keymap: Option<HashMap<String, String>>,
    #[serde(default)]
    theme: Option<HashMap<String, String>>,
    #[serde(default)]
    layout: Option<LayoutFile>,
}

#[derive(Debug, Clone, serde::Deserialize, Default)]
struct LayoutFile {
    #[serde(default)]
    content_padding_x: Option<i64>,
    #[serde(default)]
    content_padding_y: Option<i64>,
}

/// Configuration load failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    Io { path: PathBuf, message: String },
    Parse { path: PathBuf, message: String },
    Keymap { path: PathBuf, message: String },
    Theme { path: PathBuf, message: String },
    Layout { path: PathBuf, message: String },
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, message } => {
                write!(f, "failed to read config {}: {message}", path.display())
            }
            Self::Parse { path, message } => {
                write!(f, "failed to parse config {}: {message}", path.display())
            }
            Self::Keymap { path, message } => {
                write!(f, "invalid keymap in config {}: {message}", path.display())
            }
            Self::Theme { path, message } => {
                write!(f, "invalid theme in config {}: {message}", path.display())
            }
            Self::Layout { path, message } => {
                write!(f, "invalid layout in config {}: {message}", path.display())
            }
        }
    }
}

impl std::error::Error for ConfigError {}

impl From<KeymapError> for ConfigError {
    fn from(err: KeymapError) -> Self {
        Self::Keymap {
            path: PathBuf::from("<memory>"),
            message: err.to_string(),
        }
    }
}

impl From<ThemeError> for ConfigError {
    fn from(err: ThemeError) -> Self {
        Self::Theme {
            path: PathBuf::from("<memory>"),
            message: err.to_string(),
        }
    }
}

/// Example config content for documentation / discovery (not rendered in the TUI).
pub fn example_config_toml() -> &'static str {
    r#"# chromewright tui config
# Only list keys you want to change. Unknown names abort startup.

[keymap]
# reload = "R"
# open_url = "O"
# quit = "ctrl-q"

# Optional content-pane padding (header/footer stay full width).
# Defaults: 1 column left/right, 0 rows top/bottom. Max 32 per side.
[layout]
# content_padding_x = 1
# content_padding_y = 0

# Optional color roles (ANSI names, reset, or #rrggbb). Defaults already
# use a clear heading ladder within the terminal 16-color palette.
[theme]
# link = "blue"
# h1 = "lightblue"
# h2 = "green"
# h3 = "magenta"
# h4 = "cyan"
# h5 = "yellow"
# h6 = "lightred"
# form_control = "lightcyan"
# hint_label = "yellow"
# attention_bg = "magenta"
"#
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::action::Action;
    use crate::tui::keymap::KeySequence;
    use ratatui::layout::Rect;
    use ratatui::style::Color;
    use std::io::Write;

    #[test]
    fn missing_default_uses_defaults() {
        let path = PathBuf::from("/tmp/chromewright-tui-config-does-not-exist-xyz.toml");
        let cfg = load_from_path(&path, false).expect("missing optional");
        assert!(cfg.loaded_from.is_none());
        assert_eq!(cfg.layout, TuiLayout::defaults());
        assert_eq!(cfg.layout.content_padding_x, 1);
        assert_eq!(cfg.layout.content_padding_y, 0);
        assert_eq!(
            cfg.keymap.resolve_sequence(&KeySequence::chars("j")),
            Some(Action::ScrollDown)
        );
    }

    #[test]
    fn explicit_missing_path_fails() {
        let path = PathBuf::from("/tmp/chromewright-tui-config-does-not-exist-xyz.toml");
        let err = load_tui_config(Some(&path)).expect_err("required");
        assert!(matches!(err, ConfigError::Io { .. }));
    }

    #[test]
    fn explicit_path_takes_precedence_and_overlays() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("tui.toml");
        let mut f = fs::File::create(&path).expect("create");
        writeln!(f, "[keymap]").unwrap();
        writeln!(f, "reload = \"R\"").unwrap();
        drop(f);

        let cfg = load_tui_config(Some(&path)).expect("load");
        assert_eq!(cfg.loaded_from.as_deref(), Some(path.as_path()));
        assert_eq!(
            cfg.keymap.resolve_sequence(&KeySequence::chars("R")),
            Some(Action::Reload)
        );
        assert_eq!(cfg.keymap.resolve_sequence(&KeySequence::chars("r")), None);
        assert_eq!(cfg.layout, TuiLayout::defaults());
    }

    #[test]
    fn theme_overlay_from_toml() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("tui.toml");
        fs::write(&path, "[theme]\nh2 = \"yellow\"\nlink = \"lightblue\"\n").expect("write");
        let cfg = load_tui_config(Some(&path)).expect("load");
        assert_eq!(
            cfg.theme
                .content_style(Some(crate::semantic::SemanticKind::Heading), Some(2))
                .fg,
            Some(Color::Yellow)
        );
        assert_eq!(
            cfg.theme
                .content_style(Some(crate::semantic::SemanticKind::Link), None)
                .fg,
            Some(Color::LightBlue)
        );
    }

    #[test]
    fn layout_overlay_from_toml() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("tui.toml");
        fs::write(
            &path,
            "[layout]\ncontent_padding_x = 2\ncontent_padding_y = 1\n",
        )
        .expect("write");
        let cfg = load_tui_config(Some(&path)).expect("load");
        assert_eq!(
            cfg.layout,
            TuiLayout {
                content_padding_x: 2,
                content_padding_y: 1,
            }
        );
    }

    #[test]
    fn layout_partial_keeps_other_default() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("tui.toml");
        fs::write(&path, "[layout]\ncontent_padding_x = 0\n").expect("write");
        let cfg = load_tui_config(Some(&path)).expect("load");
        assert_eq!(cfg.layout.content_padding_x, 0);
        assert_eq!(cfg.layout.content_padding_y, 0);
    }

    #[test]
    fn layout_negative_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("tui.toml");
        fs::write(&path, "[layout]\ncontent_padding_x = -1\n").expect("write");
        let err = load_tui_config(Some(&path)).expect_err("layout");
        assert!(matches!(err, ConfigError::Layout { .. }));
    }

    #[test]
    fn layout_excessive_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("tui.toml");
        fs::write(&path, "[layout]\ncontent_padding_y = 33\n").expect("write");
        let err = load_tui_config(Some(&path)).expect_err("layout");
        assert!(matches!(err, ConfigError::Layout { .. }));
    }

    #[test]
    fn unknown_theme_role_fails_startup() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("tui.toml");
        fs::write(&path, "[theme]\nnot_a_role = \"blue\"\n").expect("write");
        let err = load_tui_config(Some(&path)).expect_err("theme");
        assert!(matches!(err, ConfigError::Theme { .. }));
    }

    #[test]
    fn malformed_toml_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("bad.toml");
        fs::write(&path, "keymap = [not valid").expect("write");
        let err = load_tui_config(Some(&path)).expect_err("parse");
        assert!(matches!(err, ConfigError::Parse { .. }));
    }

    #[test]
    fn unknown_action_fails_startup() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("tui.toml");
        fs::write(&path, "[keymap]\nfrobnicate = \"z\"\n").expect("write");
        let err = load_tui_config(Some(&path)).expect_err("keymap");
        assert!(matches!(err, ConfigError::Keymap { .. }));
    }

    #[test]
    fn inset_content_rect_applies_symmetric_padding() {
        let layout = TuiLayout {
            content_padding_x: 1,
            content_padding_y: 0,
        };
        let outer = Rect {
            x: 0,
            y: 1,
            width: 80,
            height: 20,
        };
        let inner = layout.inset_content_rect(outer);
        assert_eq!(
            inner,
            Rect {
                x: 1,
                y: 1,
                width: 78,
                height: 20,
            }
        );
        assert_eq!(layout.content_viewport_size(80, 20), (78, 20));
    }

    #[test]
    fn inset_content_rect_saturates_when_padding_exceeds_area() {
        let layout = TuiLayout {
            content_padding_x: 10,
            content_padding_y: 5,
        };
        let outer = Rect {
            x: 2,
            y: 3,
            width: 8,
            height: 4,
        };
        let inner = layout.inset_content_rect(outer);
        assert_eq!(
            inner,
            Rect {
                x: 12,
                y: 8,
                width: 0,
                height: 0,
            }
        );
        // Viewport size never reports zero (wrap/selection need a positive box).
        assert_eq!(layout.content_viewport_size(8, 4), (1, 1));
    }
}

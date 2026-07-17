//! TOML keymap configuration: explicit `--config` path or XDG default.

use crate::tui::keymap::{KeymapError, TuiKeymap};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Loaded TUI configuration (keymap overlay only for Phase 4).
#[derive(Debug, Clone)]
pub struct TuiConfig {
    pub keymap: TuiKeymap,
    /// Path that was loaded, if any file was read.
    pub loaded_from: Option<PathBuf>,
}

impl TuiConfig {
    /// Defaults with no file loaded.
    pub fn defaults() -> Self {
        Self {
            keymap: TuiKeymap::defaults(),
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

    let overrides = file.keymap.unwrap_or_default();
    let keymap = TuiKeymap::defaults()
        .overlay_from_map(&overrides)
        .map_err(|err| ConfigError::Keymap {
            path: path.to_path_buf(),
            message: err.to_string(),
        })?;

    Ok(TuiConfig {
        keymap,
        loaded_from: Some(path.to_path_buf()),
    })
}

#[derive(Debug, Clone, serde::Deserialize, Default)]
struct ConfigFile {
    #[serde(default)]
    keymap: Option<HashMap<String, String>>,
}

/// Configuration load failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    Io { path: PathBuf, message: String },
    Parse { path: PathBuf, message: String },
    Keymap { path: PathBuf, message: String },
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

/// Example config content for documentation / discovery (not rendered in the TUI).
pub fn example_config_toml() -> &'static str {
    r#"# chromewright tui keymap overlay
# Only list actions you want to rebind. Unknown actions abort startup.

[keymap]
# reload = "R"
# open_url = "O"
# quit = "ctrl-q"
"#
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::action::Action;
    use crate::tui::keymap::KeySequence;
    use std::io::Write;

    #[test]
    fn missing_default_uses_defaults() {
        // Explicit None path with no forced file: always defaults when default path absent
        // We call load with a guaranteed-missing explicit path as non-required via load_from_path.
        let path = PathBuf::from("/tmp/chromewright-tui-config-does-not-exist-xyz.toml");
        let cfg = load_from_path(&path, false).expect("missing optional");
        assert!(cfg.loaded_from.is_none());
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
}

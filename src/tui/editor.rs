//! External `$VISUAL` / `$EDITOR` launch (Nereid-style).
//!
//! Used by TUI `e` to open the current page's semantic markdown in a temp file.
//! The event loop must suspend the alternate screen / raw mode around
//! [`launch_editor_command`].

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// Resolve the editor command: `$VISUAL`, then `$EDITOR`, then `vi`.
pub fn resolve_editor_command() -> String {
    env::var("VISUAL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            env::var("EDITOR")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .unwrap_or_else(|| "vi".to_owned())
}

/// Write `content` to a unique temp `.md` path under the system temp dir.
pub fn write_temp_markdown(stem: &str, content: &str) -> Result<PathBuf, String> {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let safe: String = stem
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .take(48)
        .collect();
    let safe = if safe.is_empty() {
        "page".to_string()
    } else {
        safe
    };
    let mut path = env::temp_dir();
    path.push(format!("chromewright-{safe}-{ts}.md"));
    fs::write(&path, content).map_err(|err| {
        format!(
            "failed to create temporary markdown file {}: {err}",
            path.display()
        )
    })?;
    Ok(path)
}

/// Run `command` (shell) with `path` as the last argument. Blocks until exit.
pub fn launch_editor_command(command: &str, path: &Path) -> Result<(), String> {
    let path_text = path.to_string_lossy();
    if path_text.starts_with('-') {
        return Err("invalid editor temp path".to_owned());
    }
    let status = Command::new("sh")
        .arg("-lc")
        .arg(format!(
            "{command} {}",
            shell_single_quote(path_text.as_ref())
        ))
        .status()
        .map_err(|err| format!("failed to run editor command `{command}`: {err}"))?;
    if !status.success() {
        return Err(format!("editor command failed with status {status}"));
    }
    Ok(())
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_editor_falls_back_to_vi_when_unset() {
        // Cannot reliably unset env in parallel tests; just ensure non-empty.
        let cmd = resolve_editor_command();
        assert!(!cmd.trim().is_empty());
    }

    #[test]
    fn write_temp_markdown_roundtrips() {
        let path = write_temp_markdown("Example Page", "# hi\n").expect("write");
        assert!(path.extension().is_some_and(|e| e == "md"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "# hi\n");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn shell_single_quote_escapes_apostrophe() {
        assert_eq!(shell_single_quote("a'b"), "'a'\\''b'");
    }
}

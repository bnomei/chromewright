//! Local URL bar history for Tab autocomplete.
//!
//! Managed headless profiles do not expose Chrome's omnibox history. This store
//! records URLs the TUI successfully opened and suggests prefix matches while
//! editing the location bar. Persists under the XDG data directory when possible.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

/// Maximum retained URLs (most recent first).
const MAX_ENTRIES: usize = 200;

/// In-memory URL history with optional on-disk persistence.
#[derive(Debug, Clone, Default)]
pub struct UrlHistory {
    /// Most recent first; unique (case-sensitive).
    entries: Vec<String>,
}

impl UrlHistory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load from the default data path if present; otherwise empty.
    pub fn load_default() -> Self {
        let mut h = Self::new();
        if let Some(path) = default_history_path()
            && let Ok(file) = fs::File::open(&path)
        {
            let reader = BufReader::new(file);
            for line in reader.lines().map_while(Result::ok) {
                let line = line.trim().to_string();
                if !line.is_empty() && !h.entries.iter().any(|e| e == &line) {
                    h.entries.push(line);
                }
                if h.entries.len() >= MAX_ENTRIES {
                    break;
                }
            }
        }
        h
    }

    /// Persist to the default data path (best-effort).
    pub fn save_default(&self) {
        let Some(path) = default_history_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let Ok(mut file) = fs::File::create(&path) else {
            return;
        };
        for url in &self.entries {
            let _ = writeln!(file, "{url}");
        }
    }

    /// Record a successful navigation URL (most recent first, unique).
    pub fn record(&mut self, url: &str) {
        let url = url.trim();
        if url.is_empty() || url == "about:blank" {
            return;
        }
        self.entries.retain(|e| e != url);
        self.entries.insert(0, url.to_string());
        if self.entries.len() > MAX_ENTRIES {
            self.entries.truncate(MAX_ENTRIES);
        }
        self.save_default();
    }

    /// All entries matching `prefix` (case-insensitive), most recent first.
    pub fn matches(&self, prefix: &str) -> Vec<&str> {
        let prefix = prefix.trim();
        if prefix.is_empty() {
            return self.entries.iter().map(String::as_str).take(20).collect();
        }
        let lower = prefix.to_ascii_lowercase();
        self.entries
            .iter()
            .filter(|u| u.to_ascii_lowercase().starts_with(&lower))
            .map(String::as_str)
            .collect()
    }

    /// Best single completion for `prefix` (first match that is a strict extension).
    pub fn ghost_suffix(&self, prefix: &str) -> Option<String> {
        let prefix = prefix.trim();
        if prefix.is_empty() {
            return None;
        }
        let lower = prefix.to_ascii_lowercase();
        for url in &self.entries {
            let ul = url.to_ascii_lowercase();
            if ul.starts_with(&lower) && url.len() > prefix.len() {
                return Some(url[prefix.len()..].to_string());
            }
        }
        None
    }

    /// Cycle to the next/previous match relative to `current` buffer.
    ///
    /// Returns the full URL to put in the buffer, or `None` if no matches.
    pub fn complete(&self, current: &str, forward: bool) -> Option<String> {
        let matches = self.matches(current);
        if matches.is_empty() {
            if current.is_empty() {
                return self.entries.first().cloned();
            }
            return None;
        }
        // Already a full history hit (or the only prefix match equals current):
        // widen the stem so Tab can cycle siblings (e.g. /blog ↔ /docs).
        if matches.contains(&current) {
            let wider = if matches.len() == 1 {
                let stem = shorten_for_cycle(current);
                let w = self.matches(&stem);
                if w.len() > 1 { w } else { matches }
            } else {
                matches
            };
            return cycle_match(&wider, current, forward)
                .or_else(|| wider.first().copied())
                .map(str::to_string);
        }
        // Prefer first strict extension over exact prefix-only.
        matches
            .iter()
            .find(|m| m.len() > current.len())
            .or_else(|| matches.first())
            .map(|s| (*s).to_string())
    }

    #[cfg(test)]
    pub fn entries(&self) -> &[String] {
        &self.entries
    }
}

fn cycle_match<'a>(matches: &[&'a str], current: &str, forward: bool) -> Option<&'a str> {
    let idx = matches.iter().position(|m| *m == current)?;
    let next = if forward {
        (idx + 1) % matches.len()
    } else if idx == 0 {
        matches.len() - 1
    } else {
        idx - 1
    };
    Some(matches[next])
}

/// When the buffer is already a full URL, drop the last path segment so Tab can
/// cycle siblings under the same prefix.
fn shorten_for_cycle(current: &str) -> String {
    if let Some(idx) = current.rfind('/')
        && idx + 1 < current.len()
    {
        return current[..=idx].to_string();
    }
    if current.len() > 1 {
        current[..current.len() - 1].to_string()
    } else {
        String::new()
    }
}

/// `$XDG_DATA_HOME/chromewright/url_history` or `~/.local/share/chromewright/url_history`.
pub fn default_history_path() -> Option<PathBuf> {
    let root = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local").join("share"))
        })?;
    Some(root.join("chromewright").join("url_history"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_dedupes_and_orders_recent_first() {
        let mut h = UrlHistory::new();
        h.record("https://a.example/");
        h.record("https://b.example/");
        h.record("https://a.example/");
        assert_eq!(
            h.entries(),
            &["https://a.example/".to_string(), "https://b.example/".to_string()]
        );
    }

    #[test]
    fn complete_accepts_prefix_and_cycles() {
        let mut h = UrlHistory::new();
        h.record("https://example.com/docs");
        h.record("https://example.com/blog");
        h.record("https://other.test/");
        let first = h.complete("https://ex", true).unwrap();
        assert!(first.starts_with("https://example.com/"));
        let second = h.complete(&first, true).unwrap();
        assert_ne!(first, second);
        assert!(second.starts_with("https://example.com/"));
        let back = h.complete(&second, false).unwrap();
        assert_eq!(back, first);
    }

    #[test]
    fn ghost_suffix_extends_prefix() {
        let mut h = UrlHistory::new();
        h.record("https://example.com/path");
        assert_eq!(
            h.ghost_suffix("https://ex").as_deref(),
            Some("ample.com/path")
        );
        assert_eq!(h.ghost_suffix("https://example.com/path"), None);
    }
}

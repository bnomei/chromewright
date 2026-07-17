//! Deterministic Vimari-compatible key bindings with TOML overlay support.

use crate::tui::action::Action;
use std::collections::HashMap;
use std::fmt;

/// One physical key chord used in a binding sequence.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyChord {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
}

/// Subset of keys the TUI understands (independent of crossterm for unit tests).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum KeyCode {
    Char(char),
    Esc,
    Enter,
    Tab,
    BackTab,
    Backspace,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    F(u8),
}

/// Modifier bitflags for a chord.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct KeyModifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl KeyModifiers {
    pub const NONE: Self = Self {
        ctrl: false,
        alt: false,
        shift: false,
    };

    pub const CTRL: Self = Self {
        ctrl: true,
        alt: false,
        shift: false,
    };

    pub const SHIFT: Self = Self {
        ctrl: false,
        alt: false,
        shift: true,
    };
}

/// Ordered sequence of chords bound to one action (e.g. `gg`, `gi`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeySequence(pub Vec<KeyChord>);

impl KeySequence {
    pub fn single(code: KeyCode) -> Self {
        Self(vec![KeyChord {
            code,
            modifiers: KeyModifiers::NONE,
        }])
    }

    pub fn with_modifiers(code: KeyCode, modifiers: KeyModifiers) -> Self {
        Self(vec![KeyChord { code, modifiers }])
    }

    pub fn chars(chars: &str) -> Self {
        Self(
            chars
                .chars()
                .map(|c| KeyChord {
                    code: KeyCode::Char(c),
                    modifiers: KeyModifiers::NONE,
                })
                .collect(),
        )
    }

    pub fn parse(spec: &str) -> Result<Self, KeymapError> {
        let trimmed = spec.trim();
        if trimmed.is_empty() {
            return Err(KeymapError::EmptyBinding);
        }
        // Crossterm reports Shift-Tab as the dedicated BackTab key code and
        // the event normalizer intentionally removes the redundant modifier.
        if trimmed.eq_ignore_ascii_case("shift-tab") {
            return Ok(Self::single(KeyCode::BackTab));
        }

        // Multi-key letter sequences without separators: "gg", "gi"
        if trimmed.chars().all(|c| c.is_ascii_alphanumeric()) && trimmed.len() > 1 {
            // Prefer multi-char sequences only when not a known special token.
            if !matches!(
                trimmed.to_ascii_lowercase().as_str(),
                "esc" | "enter" | "tab" | "space" | "backspace" | "up" | "down" | "left" | "right"
            ) {
                return Ok(Self::chars(trimmed));
            }
        }

        // Chord forms: "ctrl-c", "shift-tab", "C-c", single keys.
        let parts: Vec<&str> = trimmed
            .split(['-', '+', ' '])
            .filter(|p| !p.is_empty())
            .collect();
        if parts.is_empty() {
            return Err(KeymapError::EmptyBinding);
        }

        let mut modifiers = KeyModifiers::NONE;
        let mut code_part = parts[parts.len() - 1];
        for part in &parts[..parts.len() - 1] {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "c" if part.eq_ignore_ascii_case("c") && parts.len() > 2 => {
                    // ambiguous bare "c" as modifier only when multi-part like C-c style handled below
                    modifiers.ctrl = true;
                }
                "ctrl" | "control" => modifiers.ctrl = true,
                "alt" | "a" if part.eq_ignore_ascii_case("alt") => modifiers.alt = true,
                "alt" => modifiers.alt = true,
                "shift" | "s" if part.eq_ignore_ascii_case("shift") => modifiers.shift = true,
                "shift" => modifiers.shift = true,
                "c" => modifiers.ctrl = true,
                other => {
                    return Err(KeymapError::InvalidBinding(format!(
                        "unknown modifier '{other}' in binding '{spec}'"
                    )));
                }
            }
        }

        // Emacs-style "C-c"
        if parts.len() == 2 && parts[0].eq_ignore_ascii_case("C") {
            modifiers.ctrl = true;
            code_part = parts[1];
        }

        let code = parse_key_code(code_part)?;
        Ok(Self::with_modifiers(code, modifiers))
    }
}

fn parse_key_code(token: &str) -> Result<KeyCode, KeymapError> {
    let lower = token.to_ascii_lowercase();
    match lower.as_str() {
        "esc" | "escape" => Ok(KeyCode::Esc),
        "enter" | "return" | "ret" => Ok(KeyCode::Enter),
        "tab" => Ok(KeyCode::Tab),
        "backtab" | "shift-tab" => Ok(KeyCode::BackTab),
        "backspace" | "bs" => Ok(KeyCode::Backspace),
        "up" => Ok(KeyCode::Up),
        "down" => Ok(KeyCode::Down),
        "left" => Ok(KeyCode::Left),
        "right" => Ok(KeyCode::Right),
        "home" => Ok(KeyCode::Home),
        "end" => Ok(KeyCode::End),
        "pageup" | "pgup" => Ok(KeyCode::PageUp),
        "pagedown" | "pgdn" => Ok(KeyCode::PageDown),
        "space" => Ok(KeyCode::Char(' ')),
        s if s.len() == 1 => {
            let c = token.chars().next().expect("len 1");
            Ok(KeyCode::Char(c))
        }
        s if s.starts_with('f') && s.len() <= 3 => {
            let n: u8 = s[1..].parse().map_err(|_| {
                KeymapError::InvalidBinding(format!("invalid function key '{token}'"))
            })?;
            Ok(KeyCode::F(n))
        }
        _ => Err(KeymapError::InvalidBinding(format!(
            "unknown key '{token}'"
        ))),
    }
}

/// Action-to-binding map. Defaults are Vimari-compatible; TOML overlays replace named actions.
#[derive(Debug, Clone)]
pub struct TuiKeymap {
    /// Primary binding for each action (first wins for reverse lookup).
    bindings: HashMap<Action, KeySequence>,
    /// Reverse map from full sequences to actions.
    sequence_index: HashMap<KeySequence, Action>,
}

impl TuiKeymap {
    /// Built-in Vimari-compatible defaults.
    pub fn defaults() -> Self {
        let mut map = HashMap::new();
        insert(&mut map, Action::LinkHintsFollow, KeySequence::chars("f"));
        insert(&mut map, Action::LinkHintsNewTab, KeySequence::chars("F"));
        insert(&mut map, Action::ScrollDown, KeySequence::chars("j"));
        insert(&mut map, Action::ScrollUp, KeySequence::chars("k"));
        insert(&mut map, Action::ScrollLeft, KeySequence::chars("h"));
        insert(&mut map, Action::ScrollRight, KeySequence::chars("l"));
        insert(&mut map, Action::HalfPageUp, KeySequence::chars("u"));
        insert(&mut map, Action::HalfPageDown, KeySequence::chars("d"));
        insert(&mut map, Action::GoTop, KeySequence::chars("gg"));
        insert(&mut map, Action::GoBottom, KeySequence::chars("G"));
        insert(&mut map, Action::FocusFirstInput, KeySequence::chars("gi"));
        insert(&mut map, Action::HistoryBack, KeySequence::chars("H"));
        insert(&mut map, Action::HistoryForward, KeySequence::chars("L"));
        insert(&mut map, Action::Reload, KeySequence::chars("r"));
        insert(&mut map, Action::NextTab, KeySequence::chars("w"));
        insert(&mut map, Action::PrevTab, KeySequence::chars("q"));
        insert(&mut map, Action::CloseTab, KeySequence::chars("x"));
        insert(&mut map, Action::NewTab, KeySequence::chars("t"));
        insert(&mut map, Action::OpenUrl, KeySequence::chars("o"));
        insert(&mut map, Action::Search, KeySequence::chars("/"));
        insert(
            &mut map,
            Action::Collapse,
            KeySequence::single(KeyCode::Char(' ')),
        );
        insert(&mut map, Action::Inspect, KeySequence::chars("i"));
        insert(&mut map, Action::CopyBlock, KeySequence::chars("y"));
        insert(&mut map, Action::CopyRef, KeySequence::chars("Y"));
        insert(&mut map, Action::TabNext, KeySequence::single(KeyCode::Tab));
        insert(
            &mut map,
            Action::TabPrev,
            KeySequence::single(KeyCode::BackTab),
        );
        insert(
            &mut map,
            Action::Confirm,
            KeySequence::single(KeyCode::Enter),
        );
        insert(&mut map, Action::Escape, KeySequence::single(KeyCode::Esc));
        insert(
            &mut map,
            Action::Quit,
            KeySequence::with_modifiers(KeyCode::Char('c'), KeyModifiers::CTRL),
        );
        Self::from_bindings(map).expect("default keymap is valid")
    }

    fn from_bindings(bindings: HashMap<Action, KeySequence>) -> Result<Self, KeymapError> {
        let mut sequence_index = HashMap::new();
        for (action, seq) in &bindings {
            if let Some(existing) = sequence_index.insert(seq.clone(), *action) {
                return Err(KeymapError::ConflictingBinding {
                    sequence: format_sequence(seq),
                    first: existing.name().to_string(),
                    second: action.name().to_string(),
                });
            }
        }
        Ok(Self {
            bindings,
            sequence_index,
        })
    }

    /// Overlay action bindings from a TOML table of `action = "binding"`.
    pub fn overlay_from_map(
        &self,
        overrides: &HashMap<String, String>,
    ) -> Result<Self, KeymapError> {
        let mut bindings = self.bindings.clone();
        for (action_name, binding_spec) in overrides {
            let action = Action::from_name(action_name)
                .ok_or_else(|| KeymapError::UnknownAction(action_name.clone()))?;
            let sequence = KeySequence::parse(binding_spec)?;
            bindings.insert(action, sequence);
        }
        Self::from_bindings(bindings)
    }

    /// Binding for an action, if configured.
    pub fn binding(&self, action: Action) -> Option<&KeySequence> {
        self.bindings.get(&action)
    }

    /// Resolve a complete chord sequence to an action.
    pub fn resolve_sequence(&self, sequence: &KeySequence) -> Option<Action> {
        self.sequence_index.get(sequence).copied()
    }

    /// Whether any binding starts with this prefix (incomplete multi-key).
    pub fn has_prefix(&self, prefix: &KeySequence) -> bool {
        if prefix.0.is_empty() {
            return false;
        }
        self.sequence_index
            .keys()
            .any(|seq| seq.0.len() > prefix.0.len() && seq.0.starts_with(&prefix.0))
    }

    /// All action→binding pairs in deterministic action-name order.
    pub fn entries(&self) -> Vec<(Action, KeySequence)> {
        let mut entries: Vec<_> = self.bindings.iter().map(|(a, s)| (*a, s.clone())).collect();
        entries.sort_by_key(|(a, _)| a.name());
        entries
    }
}

fn insert(map: &mut HashMap<Action, KeySequence>, action: Action, seq: KeySequence) {
    map.insert(action, seq);
}

fn format_sequence(seq: &KeySequence) -> String {
    seq.0
        .iter()
        .map(|c| {
            let mut s = String::new();
            if c.modifiers.ctrl {
                s.push_str("ctrl-");
            }
            if c.modifiers.alt {
                s.push_str("alt-");
            }
            if c.modifiers.shift {
                s.push_str("shift-");
            }
            match &c.code {
                KeyCode::Char(' ') => s.push_str("space"),
                KeyCode::Char(ch) => s.push(*ch),
                KeyCode::Esc => s.push_str("esc"),
                KeyCode::Enter => s.push_str("enter"),
                KeyCode::Tab => s.push_str("tab"),
                KeyCode::BackTab => s.push_str("backtab"),
                KeyCode::Backspace => s.push_str("backspace"),
                other => s.push_str(&format!("{other:?}")),
            }
            s
        })
        .collect::<Vec<_>>()
        .join("")
}

/// Keymap configuration errors (fail startup).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeymapError {
    EmptyBinding,
    InvalidBinding(String),
    UnknownAction(String),
    ConflictingBinding {
        sequence: String,
        first: String,
        second: String,
    },
}

impl fmt::Display for KeymapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyBinding => write!(f, "empty key binding"),
            Self::InvalidBinding(msg) => write!(f, "invalid key binding: {msg}"),
            Self::UnknownAction(name) => write!(f, "unknown keymap action '{name}'"),
            Self::ConflictingBinding {
                sequence,
                first,
                second,
            } => write!(
                f,
                "conflicting binding '{sequence}' for actions '{first}' and '{second}'"
            ),
        }
    }
}

impl std::error::Error for KeymapError {}

/// Incremental multi-key resolver used by the normal-mode dispatcher.
#[derive(Debug, Clone, Default)]
pub struct KeyResolver {
    pending: Vec<KeyChord>,
}

#[allow(dead_code)]
impl KeyResolver {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.pending.clear();
    }

    pub fn pending(&self) -> &[KeyChord] {
        &self.pending
    }

    /// Feed one chord. Returns a completed action, wait-for-more, or reject.
    pub fn push(&mut self, chord: KeyChord, keymap: &TuiKeymap) -> KeyResolveResult {
        self.pending.push(chord);
        let sequence = KeySequence(self.pending.clone());
        if let Some(action) = keymap.resolve_sequence(&sequence) {
            self.pending.clear();
            return KeyResolveResult::Action(action);
        }
        if keymap.has_prefix(&sequence) {
            return KeyResolveResult::Pending;
        }
        // Reject: if we had a multi-key pending, drop and do not re-interpret the last key
        // (fail closed for ambiguous prefixes).
        self.pending.clear();
        KeyResolveResult::Unbound
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyResolveResult {
    Action(Action),
    Pending,
    Unbound,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_include_vimari_core() {
        let km = TuiKeymap::defaults();
        assert_eq!(
            km.resolve_sequence(&KeySequence::chars("f")),
            Some(Action::LinkHintsFollow)
        );
        assert_eq!(
            km.resolve_sequence(&KeySequence::chars("F")),
            Some(Action::LinkHintsNewTab)
        );
        assert_eq!(
            km.resolve_sequence(&KeySequence::chars("gg")),
            Some(Action::GoTop)
        );
        assert_eq!(
            km.resolve_sequence(&KeySequence::chars("gi")),
            Some(Action::FocusFirstInput)
        );
        assert_eq!(
            km.resolve_sequence(&KeySequence::chars("o")),
            Some(Action::OpenUrl)
        );
        assert_eq!(
            km.resolve_sequence(&KeySequence::with_modifiers(
                KeyCode::Char('c'),
                KeyModifiers::CTRL
            )),
            Some(Action::Quit)
        );
    }

    #[test]
    fn multi_key_resolver_waits_for_gg() {
        let km = TuiKeymap::defaults();
        let mut r = KeyResolver::new();
        assert_eq!(
            r.push(
                KeyChord {
                    code: KeyCode::Char('g'),
                    modifiers: KeyModifiers::NONE
                },
                &km
            ),
            KeyResolveResult::Pending
        );
        assert_eq!(
            r.push(
                KeyChord {
                    code: KeyCode::Char('g'),
                    modifiers: KeyModifiers::NONE
                },
                &km
            ),
            KeyResolveResult::Action(Action::GoTop)
        );
    }

    #[test]
    fn overlay_replaces_named_action_only() {
        let km = TuiKeymap::defaults();
        let mut overrides = HashMap::new();
        overrides.insert("reload".into(), "R".into());
        let km = km.overlay_from_map(&overrides).expect("overlay");
        assert_eq!(
            km.resolve_sequence(&KeySequence::chars("R")),
            Some(Action::Reload)
        );
        assert_eq!(km.resolve_sequence(&KeySequence::chars("r")), None);
        assert_eq!(
            km.resolve_sequence(&KeySequence::chars("j")),
            Some(Action::ScrollDown)
        );
    }

    #[test]
    fn unknown_action_fails() {
        let km = TuiKeymap::defaults();
        let mut overrides = HashMap::new();
        overrides.insert("not_a_real_action".into(), "z".into());
        let err = km.overlay_from_map(&overrides).expect_err("unknown");
        assert!(matches!(err, KeymapError::UnknownAction(_)));
    }

    #[test]
    fn conflicting_bindings_fail() {
        let km = TuiKeymap::defaults();
        let mut overrides = HashMap::new();
        // Bind scroll_up to j which is already scroll_down
        overrides.insert("scroll_up".into(), "j".into());
        let err = km.overlay_from_map(&overrides).expect_err("conflict");
        assert!(matches!(err, KeymapError::ConflictingBinding { .. }));
    }
}

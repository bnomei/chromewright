//! Key event → action dispatch with Normal / Input / Hint mode boundaries.

use crate::tui::action::Action;
use crate::tui::clipboard::{ClipboardResult, copy_status, copy_text};
use crate::tui::controller::Controller;
use crate::tui::keymap::{
    KeyChord, KeyCode, KeyModifiers, KeyResolveResult, KeyResolver, TuiKeymap,
};
use crate::tui::state::{HintMode, InputKind, InteractionMode};

/// Outcome of handling one key event for the terminal event loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchOutcome {
    /// Keep the loop running without forcing an immediate full redraw.
    Continue,
    /// User requested quit (`Ctrl-c` / Quit action).
    Quit,
    /// Page-changing work was requested; caller should draw Loading then perform it.
    Redraw,
}

/// Routes key chords to named [`Action`]s under Normal / Input / Hint mode boundaries.
///
/// Holds multi-key resolver state so sequences like `gg` and `gi` complete across events.
pub struct Dispatcher {
    pub keymap: TuiKeymap,
    resolver: KeyResolver,
}

impl Dispatcher {
    pub fn new(keymap: TuiKeymap) -> Self {
        Self {
            keymap,
            resolver: KeyResolver::new(),
        }
    }

    pub fn handle_key(&mut self, controller: &mut Controller, chord: KeyChord) -> DispatchOutcome {
        // Loading: ignore normal commands (Escape still clears error after load fails).
        if controller.state.lifecycle.is_loading() {
            return DispatchOutcome::Continue;
        }

        match &controller.state.mode {
            InteractionMode::Normal => self.handle_normal(controller, chord),
            InteractionMode::Input(_) => self.handle_input(controller, chord),
            InteractionMode::Hint(_) => self.handle_hint(controller, chord),
        }
    }

    fn handle_normal(&mut self, controller: &mut Controller, chord: KeyChord) -> DispatchOutcome {
        // Escape clears error / inspect even without action map.
        if chord.code == KeyCode::Esc {
            self.resolver.clear();
            controller.escape();
            return DispatchOutcome::Redraw;
        }

        match self.resolver.push(chord, &self.keymap) {
            KeyResolveResult::Pending => DispatchOutcome::Continue,
            KeyResolveResult::Unbound => DispatchOutcome::Continue,
            KeyResolveResult::Action(action) => self.dispatch_action(controller, action),
        }
    }

    fn handle_input(&mut self, controller: &mut Controller, chord: KeyChord) -> DispatchOutcome {
        // Normal commands must not fire while editing.
        if let Some(action) = self
            .keymap
            .resolve_sequence(&crate::tui::keymap::KeySequence(vec![chord.clone()]))
        {
            match action {
                Action::Escape => {
                    self.resolver.clear();
                    controller.escape();
                    return DispatchOutcome::Redraw;
                }
                Action::Confirm => return self.confirm_input(controller),
                Action::TabNext => {
                    controller.tab_focus(true);
                    return DispatchOutcome::Redraw;
                }
                Action::TabPrev => {
                    controller.tab_focus(false);
                    return DispatchOutcome::Redraw;
                }
                Action::Quit => return self.dispatch_action(controller, action),
                _ => {}
            }
        }
        match chord.code {
            KeyCode::Backspace => {
                if let InteractionMode::Input(kind) = &mut controller.state.mode {
                    match kind {
                        InputKind::Url { buffer }
                        | InputKind::Search { buffer }
                        | InputKind::Form { buffer, .. } => {
                            buffer.pop();
                        }
                    }
                }
                DispatchOutcome::Redraw
            }
            KeyCode::Char(ch) if !chord.modifiers.ctrl && !chord.modifiers.alt => {
                if let InteractionMode::Input(kind) = &mut controller.state.mode {
                    match kind {
                        InputKind::Url { buffer }
                        | InputKind::Search { buffer }
                        | InputKind::Form { buffer, .. } => {
                            buffer.push(ch);
                        }
                    }
                }
                DispatchOutcome::Redraw
            }
            _ => DispatchOutcome::Continue,
        }
    }

    fn handle_hint(&mut self, controller: &mut Controller, chord: KeyChord) -> DispatchOutcome {
        if self
            .keymap
            .resolve_sequence(&crate::tui::keymap::KeySequence(vec![chord.clone()]))
            == Some(Action::Escape)
        {
            self.resolver.clear();
            controller.escape();
            return DispatchOutcome::Redraw;
        }
        let KeyCode::Char(ch) = chord.code else {
            return DispatchOutcome::Continue;
        };
        if let Some((r, new_tab)) = controller.hint_type_char(ch) {
            match controller.follow_link(&r, new_tab) {
                Ok(()) => {
                    // The controller restores chained hint mode only after the
                    // deferred action settles and publishes its fresh capture.
                }
                Err(_) => {
                    // Error lifecycle already set; leave hint mode via enter_error.
                }
            }
            return DispatchOutcome::Redraw;
        }
        DispatchOutcome::Redraw
    }

    fn confirm_input(&mut self, controller: &mut Controller) -> DispatchOutcome {
        let mode = controller.state.mode.clone();
        match mode {
            InteractionMode::Input(InputKind::Url { buffer }) => {
                let url = buffer.trim().to_string();
                controller.state.mode = InteractionMode::Normal;
                if !url.is_empty() {
                    controller.navigate_to(&url);
                }
                DispatchOutcome::Redraw
            }
            InteractionMode::Input(InputKind::Search { buffer }) => {
                controller.apply_search(&buffer);
                DispatchOutcome::Redraw
            }
            InteractionMode::Input(InputKind::Form {
                semantic_ref,
                buffer,
            }) => {
                let _ = controller.submit_form_input(&semantic_ref, &buffer);
                DispatchOutcome::Redraw
            }
            _ => DispatchOutcome::Continue,
        }
    }

    fn dispatch_action(&mut self, controller: &mut Controller, action: Action) -> DispatchOutcome {
        // Double-check mode boundary for normal actions.
        if !controller.state.allows_normal_commands()
            && !matches!(action, Action::Escape | Action::Quit)
        {
            return DispatchOutcome::Continue;
        }

        match action {
            Action::Quit => {
                controller.state.should_quit = true;
                DispatchOutcome::Quit
            }
            Action::Escape => {
                controller.escape();
                DispatchOutcome::Redraw
            }
            Action::ScrollDown => {
                controller.scroll_down();
                DispatchOutcome::Redraw
            }
            Action::ScrollUp => {
                controller.scroll_up();
                DispatchOutcome::Redraw
            }
            Action::ScrollLeft => {
                controller.scroll_left();
                DispatchOutcome::Redraw
            }
            Action::ScrollRight => {
                controller.scroll_right();
                DispatchOutcome::Redraw
            }
            Action::HalfPageUp => {
                controller.half_page_up();
                DispatchOutcome::Redraw
            }
            Action::HalfPageDown => {
                controller.half_page_down();
                DispatchOutcome::Redraw
            }
            Action::GoTop => {
                controller.go_top();
                DispatchOutcome::Redraw
            }
            Action::GoBottom => {
                controller.go_bottom();
                DispatchOutcome::Redraw
            }
            Action::OpenUrl => {
                controller.enter_url_input();
                DispatchOutcome::Redraw
            }
            Action::Search => {
                controller.enter_search();
                DispatchOutcome::Redraw
            }
            Action::Collapse => {
                controller.toggle_collapse();
                DispatchOutcome::Redraw
            }
            Action::Inspect => {
                controller.inspect_selection();
                DispatchOutcome::Redraw
            }
            Action::CopyBlock => {
                if let Some(text) = controller.copy_block_text() {
                    let result = copy_text(&text);
                    apply_clipboard(controller, result, "block");
                } else {
                    controller.state.view.set_status("nothing to copy");
                }
                DispatchOutcome::Redraw
            }
            Action::CopyRef => {
                if let Some(text) = controller.copy_ref_text() {
                    let result = copy_text(&text);
                    apply_clipboard(controller, result, "semantic_ref");
                } else {
                    controller.state.view.set_status("nothing to copy");
                }
                DispatchOutcome::Redraw
            }
            Action::FocusFirstInput => {
                controller.focus_first_input();
                DispatchOutcome::Redraw
            }
            Action::TabNext => {
                controller.tab_focus(true);
                DispatchOutcome::Redraw
            }
            Action::TabPrev => {
                controller.tab_focus(false);
                DispatchOutcome::Redraw
            }
            Action::LinkHintsFollow => {
                controller.enter_hint_mode(HintMode::Follow);
                DispatchOutcome::Redraw
            }
            Action::LinkHintsNewTab => {
                controller.enter_hint_mode(HintMode::NewTab);
                DispatchOutcome::Redraw
            }
            Action::HistoryBack => {
                controller.history_back();
                DispatchOutcome::Redraw
            }
            Action::HistoryForward => {
                controller.history_forward();
                DispatchOutcome::Redraw
            }
            Action::Reload => {
                controller.reload();
                DispatchOutcome::Redraw
            }
            Action::NextTab => {
                controller.next_tab();
                DispatchOutcome::Redraw
            }
            Action::PrevTab => {
                controller.prev_tab();
                DispatchOutcome::Redraw
            }
            Action::CloseTab => {
                controller.close_tab();
                DispatchOutcome::Redraw
            }
            Action::NewTab => {
                controller.new_tab();
                DispatchOutcome::Redraw
            }
            Action::Confirm => {
                let _ = controller.activate_selection();
                DispatchOutcome::Redraw
            }
        }
    }
}

fn apply_clipboard(controller: &mut Controller, result: ClipboardResult, kind: &str) {
    match &result {
        ClipboardResult::Fallback { text } => {
            controller.state.clipboard_fallback = Some(text.clone());
        }
        ClipboardResult::Copied => {
            controller.state.clipboard_fallback = None;
        }
    }
    controller.state.view.set_status(copy_status(&result, kind));
}

/// Convert crossterm key event into our KeyChord (feature-gated callers).
pub fn chord_from_crossterm(
    code: crossterm::event::KeyCode,
    modifiers: crossterm::event::KeyModifiers,
) -> Option<KeyChord> {
    use crossterm::event::KeyCode as CCode;
    let code = match code {
        CCode::Char(c) => KeyCode::Char(c),
        CCode::Esc => KeyCode::Esc,
        CCode::Enter => KeyCode::Enter,
        CCode::Tab => KeyCode::Tab,
        CCode::BackTab => KeyCode::BackTab,
        CCode::Backspace => KeyCode::Backspace,
        CCode::Up => KeyCode::Up,
        CCode::Down => KeyCode::Down,
        CCode::Left => KeyCode::Left,
        CCode::Right => KeyCode::Right,
        CCode::Home => KeyCode::Home,
        CCode::End => KeyCode::End,
        CCode::PageUp => KeyCode::PageUp,
        CCode::PageDown => KeyCode::PageDown,
        CCode::F(n) => KeyCode::F(n),
        _ => return None,
    };
    let mut normalized_modifiers = modifiers;
    if matches!(code, KeyCode::Char(c) if c.is_ascii_uppercase()) {
        normalized_modifiers.remove(crossterm::event::KeyModifiers::SHIFT);
    }
    if matches!(code, KeyCode::BackTab) {
        normalized_modifiers.remove(crossterm::event::KeyModifiers::SHIFT);
    }
    Some(KeyChord {
        code,
        modifiers: KeyModifiers {
            ctrl: normalized_modifiers.contains(crossterm::event::KeyModifiers::CONTROL),
            alt: normalized_modifiers.contains(crossterm::event::KeyModifiers::ALT),
            shift: normalized_modifiers.contains(crossterm::event::KeyModifiers::SHIFT),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::DocumentMetadata;
    use crate::semantic::SemanticDocument;
    use crate::semantic::normalize::{RawSemanticNode, normalize_fixture};
    use crate::tui::keymap::KeySequence;

    fn empty_doc() -> SemanticDocument {
        SemanticDocument::empty(DocumentMetadata {
            document_id: "d".into(),
            revision: "1".into(),
            url: "https://example.com/".into(),
            title: "T".into(),
            ready_state: "complete".into(),
            frames: vec![],
        })
        .unwrap()
    }

    #[test]
    fn normal_commands_do_not_fire_in_url_input() {
        let mut ctl = Controller::new();
        ctl.state.publish_page(empty_doc());
        ctl.enter_url_input();
        assert!(ctl.state.is_input_mode());

        let mut dispatcher = Dispatcher::new(TuiKeymap::defaults());
        // Typing 'r' must not reload
        let _ = dispatcher.handle_key(
            &mut ctl,
            KeyChord {
                code: KeyCode::Char('r'),
                modifiers: KeyModifiers::NONE,
            },
        );
        assert!(!ctl.has_pending_page_action());
        assert!(ctl.state.is_input_mode());
        if let InteractionMode::Input(InputKind::Url { buffer }) = &ctl.state.mode {
            assert!(buffer.ends_with('r') || buffer.contains('r'));
        } else {
            panic!("expected url input");
        }
    }

    #[test]
    fn escape_leaves_input_without_navigation() {
        let mut ctl = Controller::new();
        ctl.state.publish_page(empty_doc());
        ctl.enter_url_input();
        let mut dispatcher = Dispatcher::new(TuiKeymap::defaults());
        dispatcher.handle_key(
            &mut ctl,
            KeyChord {
                code: KeyCode::Esc,
                modifiers: KeyModifiers::NONE,
            },
        );
        assert!(matches!(ctl.state.mode, InteractionMode::Normal));
        assert!(!ctl.has_pending_page_action());
    }

    #[test]
    fn reload_action_from_keymap() {
        let doc = empty_doc();
        let mut ctl = Controller::new();
        ctl.state.publish_page(doc.clone());
        let mut dispatcher = Dispatcher::new(TuiKeymap::defaults());
        let outcome = dispatcher.dispatch_action(&mut ctl, Action::Reload);
        assert_eq!(outcome, DispatchOutcome::Redraw);
        assert!(ctl.state.lifecycle.is_loading());
        assert!(ctl.has_pending_page_action());
    }

    #[test]
    fn hint_escape_ends_chain() {
        let doc = normalize_fixture(
            DocumentMetadata {
                document_id: "d".into(),
                revision: "1".into(),
                url: "https://example.com/".into(),
                title: "T".into(),
                ready_state: "complete".into(),
                frames: vec![],
            },
            vec![RawSemanticNode {
                kind: "link".into(),
                tag: Some("a".into()),
                id: Some("h".into()),
                unique_id: true,
                text: Some("Home".into()),
                href: Some("/".into()),
                landmark: None,
                heading_level: None,
                ordered: None,
                label: None,
                src: None,
                alt: None,
                name: None,
                value: None,
                input_type: None,
                placeholder: None,
                checked: None,
                disabled: None,
                required: None,
                readonly: None,
                multiple: None,
                button_type: None,
                options: vec![],
                children: vec![],
            }],
        )
        .unwrap();
        let mut ctl = Controller::new();
        ctl.state.view.viewport_height = 20;
        ctl.state.publish_page(doc);
        ctl.enter_hint_mode(HintMode::Follow);
        assert!(ctl.state.is_hint_mode());
        let mut dispatcher = Dispatcher::new(TuiKeymap::defaults());
        dispatcher.handle_key(
            &mut ctl,
            KeyChord {
                code: KeyCode::Esc,
                modifiers: KeyModifiers::NONE,
            },
        );
        assert!(matches!(ctl.state.mode, InteractionMode::Normal));
    }

    #[test]
    fn defaults_resolve_o_to_open_url() {
        let km = TuiKeymap::defaults();
        assert_eq!(
            km.resolve_sequence(&KeySequence::chars("o")),
            Some(Action::OpenUrl)
        );
    }

    #[test]
    fn escape_clears_pending_prefix_and_does_not_promote_error() {
        let mut ctl = Controller::new();
        ctl.state.publish_page(empty_doc());
        ctl.state.enter_error("reload", "nope");
        let mut dispatcher = Dispatcher::new(TuiKeymap::defaults());
        dispatcher.handle_key(&mut ctl, KeySequence::chars("g").0[0].clone());
        dispatcher.handle_key(
            &mut ctl,
            KeyChord {
                code: KeyCode::Esc,
                modifiers: KeyModifiers::NONE,
            },
        );
        assert!(matches!(
            ctl.state.lifecycle,
            crate::tui::state::Lifecycle::Error { .. }
        ));
        assert!(dispatcher.resolver.pending().is_empty());
    }

    #[test]
    fn error_state_blocks_confirm_and_other_semantic_actions() {
        let mut ctl = Controller::new();
        ctl.state.publish_page(empty_doc());
        ctl.state.enter_error("reload", "nope");
        let mut dispatcher = Dispatcher::new(TuiKeymap::defaults());

        assert_eq!(
            dispatcher.dispatch_action(&mut ctl, Action::Confirm),
            DispatchOutcome::Continue
        );
        assert!(!ctl.has_pending_page_action());
    }

    #[test]
    fn configured_quit_replaces_ctrl_c() {
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("quit".into(), "ctrl-q".into());
        let keymap = TuiKeymap::defaults().overlay_from_map(&overrides).unwrap();
        let mut ctl = Controller::new();
        ctl.state.publish_page(empty_doc());
        let mut dispatcher = Dispatcher::new(keymap);
        dispatcher.handle_key(
            &mut ctl,
            KeyChord {
                code: KeyCode::Char('c'),
                modifiers: KeyModifiers::CTRL,
            },
        );
        assert!(!ctl.state.should_quit);
        assert_eq!(
            dispatcher.handle_key(
                &mut ctl,
                KeyChord {
                    code: KeyCode::Char('q'),
                    modifiers: KeyModifiers::CTRL
                }
            ),
            DispatchOutcome::Quit
        );
    }

    #[test]
    fn crossterm_shifted_vimari_keys_normalize_to_uppercase_bindings() {
        use crossterm::event::{KeyCode as CCode, KeyModifiers as CMods};
        let chord = chord_from_crossterm(CCode::Char('F'), CMods::SHIFT).unwrap();
        assert_eq!(chord.modifiers, KeyModifiers::NONE);
        let backtab = chord_from_crossterm(CCode::BackTab, CMods::SHIFT).unwrap();
        assert_eq!(backtab.code, KeyCode::BackTab);
        assert_eq!(backtab.modifiers, KeyModifiers::NONE);
    }
}

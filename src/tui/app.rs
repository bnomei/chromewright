//! Terminal event loop for `chromewright tui`.

use crate::browser::BrowserSession;
use crate::tui::config::TuiConfig;
use crate::tui::controller::Controller;
use crate::tui::dispatch::{DispatchOutcome, Dispatcher, chord_from_crossterm};
use crate::tui::driver::SessionPageDriver;
use crate::tui::render;
use crossterm::cursor::{Hide, Show};
use crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::io::{self, Stdout};
use std::path::PathBuf;
use std::time::Duration;

/// Options for launching the terminal browser.
#[derive(Debug, Clone, Default)]
pub struct TuiOptions {
    /// Explicit keymap config path (`--config`); XDG default when None.
    pub config: Option<PathBuf>,
}

/// Run the interactive terminal browser against a shared session.
pub fn run_tui(session: &BrowserSession, options: TuiOptions) -> Result<(), String> {
    let config = crate::tui::config::load_tui_config(options.config.as_deref())
        .map_err(|e| e.to_string())?;
    run_tui_with_config(session, config)
}

pub fn run_tui_with_config(session: &BrowserSession, config: TuiConfig) -> Result<(), String> {
    let mut terminal = TerminalGuard::setup().map_err(|e| e.to_string())?;
    let result = run_loop(session, config, terminal.terminal_mut());
    let restore_result = terminal.restore().map_err(|e| e.to_string());

    // Do not mask the application failure with a secondary cleanup failure.
    // Drop retries any terminal-state operation that reported an error.
    match result {
        Err(application_error) => Err(application_error),
        Ok(()) => restore_result,
    }
}

/// Records precisely which terminal modes are active so both partial setup and
/// normal teardown can reverse them in a safe order.
#[derive(Debug, Default)]
struct TerminalLifecycle {
    raw_mode: bool,
    alternate_screen: bool,
    mouse_capture: bool,
    bracketed_paste: bool,
    cursor_hidden: bool,
}

trait TerminalControl {
    fn enable_raw_mode(&mut self) -> io::Result<()>;
    fn disable_raw_mode(&mut self) -> io::Result<()>;
    fn enter_alternate_screen(&mut self) -> io::Result<()>;
    fn leave_alternate_screen(&mut self) -> io::Result<()>;
    fn enable_mouse_capture(&mut self) -> io::Result<()>;
    fn disable_mouse_capture(&mut self) -> io::Result<()>;
    fn enable_bracketed_paste(&mut self) -> io::Result<()>;
    fn disable_bracketed_paste(&mut self) -> io::Result<()>;
    fn hide_cursor(&mut self) -> io::Result<()>;
    fn show_cursor(&mut self) -> io::Result<()>;
}

impl TerminalLifecycle {
    fn enable<C: TerminalControl>(&mut self, control: &mut C) -> io::Result<()> {
        self.raw_mode = true;
        control.enable_raw_mode()?;
        self.alternate_screen = true;
        control.enter_alternate_screen()?;
        self.mouse_capture = true;
        control.enable_mouse_capture()?;
        self.bracketed_paste = true;
        control.enable_bracketed_paste()?;
        self.cursor_hidden = true;
        control.hide_cursor()?;
        Ok(())
    }

    /// Attempt every restoration operation even if an earlier one fails. State
    /// is retained after a failed operation so Drop can make a final retry.
    fn restore<C: TerminalControl>(&mut self, control: &mut C) -> io::Result<()> {
        let mut first_error = None;
        if self.cursor_hidden {
            Self::record(
                control.show_cursor(),
                &mut self.cursor_hidden,
                &mut first_error,
            );
        }
        if self.bracketed_paste {
            Self::record(
                control.disable_bracketed_paste(),
                &mut self.bracketed_paste,
                &mut first_error,
            );
        }
        if self.mouse_capture {
            Self::record(
                control.disable_mouse_capture(),
                &mut self.mouse_capture,
                &mut first_error,
            );
        }
        if self.alternate_screen {
            Self::record(
                control.leave_alternate_screen(),
                &mut self.alternate_screen,
                &mut first_error,
            );
        }
        if self.raw_mode {
            Self::record(
                control.disable_raw_mode(),
                &mut self.raw_mode,
                &mut first_error,
            );
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn record(result: io::Result<()>, active: &mut bool, first_error: &mut Option<io::Error>) {
        match result {
            Ok(()) => *active = false,
            Err(error) if first_error.is_none() => *first_error = Some(error),
            Err(_) => {}
        }
    }
}

struct CrosstermTerminalControl<'a> {
    terminal: &'a mut Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalControl for CrosstermTerminalControl<'_> {
    fn enable_raw_mode(&mut self) -> io::Result<()> {
        enable_raw_mode()
    }

    fn disable_raw_mode(&mut self) -> io::Result<()> {
        disable_raw_mode()
    }

    fn enter_alternate_screen(&mut self) -> io::Result<()> {
        execute!(self.terminal.backend_mut(), EnterAlternateScreen)
    }

    fn leave_alternate_screen(&mut self) -> io::Result<()> {
        execute!(self.terminal.backend_mut(), LeaveAlternateScreen)
    }

    fn enable_mouse_capture(&mut self) -> io::Result<()> {
        execute!(self.terminal.backend_mut(), EnableMouseCapture)
    }

    fn disable_mouse_capture(&mut self) -> io::Result<()> {
        execute!(self.terminal.backend_mut(), DisableMouseCapture)
    }

    fn enable_bracketed_paste(&mut self) -> io::Result<()> {
        execute!(self.terminal.backend_mut(), EnableBracketedPaste)
    }

    fn disable_bracketed_paste(&mut self) -> io::Result<()> {
        execute!(self.terminal.backend_mut(), DisableBracketedPaste)
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        execute!(self.terminal.backend_mut(), Hide)
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        execute!(self.terminal.backend_mut(), Show)
    }
}

/// RAII terminal guard. Its Drop implementation runs during early returns and
/// unwinding, without catching or replacing the original application panic.
struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    lifecycle: TerminalLifecycle,
}

impl TerminalGuard {
    fn setup() -> io::Result<Self> {
        let terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
        let mut guard = Self {
            terminal,
            lifecycle: TerminalLifecycle::default(),
        };
        {
            let mut control = CrosstermTerminalControl {
                terminal: &mut guard.terminal,
            };
            guard.lifecycle.enable(&mut control)?;
        }
        Ok(guard)
    }

    fn terminal_mut(&mut self) -> &mut Terminal<CrosstermBackend<Stdout>> {
        &mut self.terminal
    }

    fn restore(&mut self) -> io::Result<()> {
        let mut control = CrosstermTerminalControl {
            terminal: &mut self.terminal,
        };
        self.lifecycle.restore(&mut control)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

fn run_loop(
    session: &BrowserSession,
    config: TuiConfig,
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
) -> Result<(), String> {
    let mut controller = Controller::new();
    let mut dispatcher = Dispatcher::new(config.keymap);
    let mut driver = SessionPageDriver::new(session);

    // Bootstrap follows the same draw-before-browser-work lifecycle as every
    // later page transition.
    controller.bootstrap();

    loop {
        let size = terminal.size().map_err(|e| e.to_string())?;
        // Content area is total height minus chrome (2) and status (1).
        let content_h = size.height.saturating_sub(3) as usize;
        controller.set_viewport(size.width as usize, content_h.max(1));

        terminal
            .draw(|frame| render::draw(frame, &controller))
            .map_err(|e| e.to_string())?;

        // `Terminal::draw` succeeded while lifecycle is Loading, so browser
        // work may now start. No action is executed in the key dispatcher.
        if controller.has_pending_page_action() {
            controller.acknowledge_loading_frame();
            let _ = controller.perform_pending_page_action(&mut driver);
            continue;
        }

        if controller.state.should_quit {
            break;
        }

        if event::poll(Duration::from_millis(100)).map_err(|e| e.to_string())? {
            match event::read().map_err(|e| e.to_string())? {
                Event::Key(key)
                    if key.kind == KeyEventKind::Press || key.kind == KeyEventKind::Repeat =>
                {
                    if let Some(chord) = chord_from_crossterm(key.code, key.modifiers) {
                        match dispatcher.handle_key(&mut controller, chord) {
                            DispatchOutcome::Quit => break,
                            DispatchOutcome::Continue | DispatchOutcome::Redraw => {}
                        }
                    }
                }
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakeTerminalControl {
        calls: Vec<&'static str>,
        fail: Option<&'static str>,
    }

    impl FakeTerminalControl {
        fn call(&mut self, name: &'static str) -> io::Result<()> {
            self.calls.push(name);
            if self.fail == Some(name) {
                return Err(io::Error::other("injected terminal failure"));
            }
            Ok(())
        }
    }

    impl TerminalControl for FakeTerminalControl {
        fn enable_raw_mode(&mut self) -> io::Result<()> {
            self.call("enable_raw")
        }
        fn disable_raw_mode(&mut self) -> io::Result<()> {
            self.call("disable_raw")
        }
        fn enter_alternate_screen(&mut self) -> io::Result<()> {
            self.call("enter_alt")
        }
        fn leave_alternate_screen(&mut self) -> io::Result<()> {
            self.call("leave_alt")
        }
        fn enable_mouse_capture(&mut self) -> io::Result<()> {
            self.call("enable_mouse")
        }
        fn disable_mouse_capture(&mut self) -> io::Result<()> {
            self.call("disable_mouse")
        }
        fn enable_bracketed_paste(&mut self) -> io::Result<()> {
            self.call("enable_paste")
        }
        fn disable_bracketed_paste(&mut self) -> io::Result<()> {
            self.call("disable_paste")
        }
        fn hide_cursor(&mut self) -> io::Result<()> {
            self.call("hide_cursor")
        }
        fn show_cursor(&mut self) -> io::Result<()> {
            self.call("show_cursor")
        }
    }

    #[test]
    fn terminal_lifecycle_restores_every_enabled_mode_in_reverse_order() {
        let mut lifecycle = TerminalLifecycle::default();
        let mut terminal = FakeTerminalControl::default();
        lifecycle.enable(&mut terminal).expect("setup");
        lifecycle.restore(&mut terminal).expect("restore");
        assert_eq!(
            terminal.calls,
            [
                "enable_raw",
                "enter_alt",
                "enable_mouse",
                "enable_paste",
                "hide_cursor",
                "show_cursor",
                "disable_paste",
                "disable_mouse",
                "leave_alt",
                "disable_raw",
            ]
        );
    }

    #[test]
    fn terminal_restore_continues_after_a_failure_and_retains_retry_state() {
        let mut lifecycle = TerminalLifecycle {
            raw_mode: true,
            alternate_screen: true,
            mouse_capture: true,
            bracketed_paste: true,
            cursor_hidden: true,
        };
        let mut terminal = FakeTerminalControl {
            fail: Some("disable_paste"),
            ..Default::default()
        };
        assert!(lifecycle.restore(&mut terminal).is_err());
        assert_eq!(
            terminal.calls,
            [
                "show_cursor",
                "disable_paste",
                "disable_mouse",
                "leave_alt",
                "disable_raw"
            ]
        );
        assert!(lifecycle.bracketed_paste);
        assert!(!lifecycle.raw_mode && !lifecycle.alternate_screen && !lifecycle.mouse_capture);
    }

    #[test]
    fn partial_setup_failure_restores_every_successfully_enabled_mode() {
        let mut lifecycle = TerminalLifecycle::default();
        let mut terminal = FakeTerminalControl {
            fail: Some("enable_paste"),
            ..Default::default()
        };
        assert!(lifecycle.enable(&mut terminal).is_err());
        lifecycle.restore(&mut terminal).expect("partial cleanup");
        assert_eq!(
            terminal.calls,
            [
                "enable_raw",
                "enter_alt",
                "enable_mouse",
                "enable_paste",
                "disable_paste",
                "disable_mouse",
                "leave_alt",
                "disable_raw",
            ]
        );
    }
}

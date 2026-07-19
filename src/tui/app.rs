//! Terminal event loop for `chromewright tui`.
//!
//! Owns terminal setup/teardown, registers `tui_*` tools on the unique
//! BrowserSession Arc, starts the loopback Companion, and drives the
//! draw → acknowledge Loading → perform page action cycle. Does not hard-code
//! key bindings; those come from Keymap → Action via the dispatcher.

use crate::browser::BrowserSession;
use crate::tui::PageCoordinator;
use crate::tui::config::TuiConfig;
use crate::tui::controller::{Controller, PreparedPageAction};
use crate::tui::dispatch::{DispatchOutcome, Dispatcher, chord_from_crossterm};
use crate::tui::driver::SessionPageDriver;
use crate::tui::render::{self, LOADING_SPINNER_INTERVAL};
use crate::tui::shared::SharedTuiState;
use crossterm::cursor::{Hide, Show};
use crossterm::event::{self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::io::{self, Stdout};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// Entry options for `chromewright tui`: keymap overlay and companion bind settings.
#[derive(Debug, Clone, Default)]
pub struct TuiOptions {
    /// Explicit keymap config path (`--config`); XDG default when None.
    pub config: Option<PathBuf>,
    /// Loopback companion port (`0` = ephemeral).
    pub companion_port: u16,
    /// Absolute HTTP path the companion nests under (e.g. `/mcp`).
    pub companion_path: String,
}

/// Load keymap config from [`TuiOptions`] and run the interactive terminal browser.
///
/// Shares the caller's `BrowserSession` (no second browser process). Returns when
/// the user quits or terminal setup/teardown fails.
pub fn run_tui(session: Arc<BrowserSession>, options: TuiOptions) -> Result<(), String> {
    let config = crate::tui::config::load_tui_config(options.config.as_deref())
        .map_err(|e| e.to_string())?;
    run_tui_with_config_and_companion(
        session,
        config,
        options.companion_port,
        options.companion_path,
    )
}

/// Run the terminal browser with a pre-loaded [`TuiConfig`] (keymap overlay already resolved).
///
/// Owns the Ratatui event loop: Loading → action dispatch → recapture → Ready | Error.
/// Application errors are preferred over secondary terminal restore failures.
pub fn run_tui_with_config(session: Arc<BrowserSession>, config: TuiConfig) -> Result<(), String> {
    run_tui_with_config_and_companion(session, config, 0, "/mcp".into())
}

fn run_tui_with_config_and_companion(
    mut session: Arc<BrowserSession>,
    config: TuiConfig,
    port: u16,
    path: String,
) -> Result<(), String> {
    let mut terminal = TerminalGuard::setup().map_err(|e| e.to_string())?;
    // Register TUI tools while the BrowserSession Arc is still uniquely owned.
    // The shared coordination state is bound to that same Arc immediately
    // afterwards, so no second session or disconnected state is created.
    let shared = SharedTuiState::unbound();
    let session_mut = Arc::get_mut(&mut session).ok_or("TUI session is already shared")?;
    for name in crate::tools::tui::NAMES {
        session_mut
            .tool_registry_mut()
            .register(crate::tools::tui::TuiTool::with_shared(
                name,
                shared.clone(),
            ));
    }
    let coordinator = Arc::new(PageCoordinator::new(session.clone(), shared.clone()));
    shared.activate_runtime();
    let companion = crate::tui::companion::start(coordinator.clone(), path, port)?;
    let result = run_loop(coordinator, config, &mut terminal);
    shared.deactivate_runtime();
    let restore_result = terminal.restore().map_err(|e| e.to_string());
    companion.stop();

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
    bracketed_paste: bool,
    cursor_hidden: bool,
}

trait TerminalControl {
    fn enable_raw_mode(&mut self) -> io::Result<()>;
    fn disable_raw_mode(&mut self) -> io::Result<()>;
    fn enter_alternate_screen(&mut self) -> io::Result<()>;
    fn leave_alternate_screen(&mut self) -> io::Result<()>;
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

    /// Leave alt-screen/raw mode, run `f`, then re-enter. Used for `$EDITOR`.
    fn run_external<T, F>(&mut self, f: F) -> Result<T, String>
    where
        F: FnOnce() -> Result<T, String>,
    {
        {
            let mut control = CrosstermTerminalControl {
                terminal: &mut self.terminal,
            };
            self.lifecycle
                .restore(&mut control)
                .map_err(|e| format!("terminal suspend failed: {e}"))?;
        }
        let result = f();
        {
            let mut control = CrosstermTerminalControl {
                terminal: &mut self.terminal,
            };
            self.lifecycle
                .enable(&mut control)
                .map_err(|e| format!("terminal resume failed: {e}"))?;
        }
        // Clear leftover editor paint before the next Ratatui frame.
        let _ = self.terminal.clear();
        result
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

fn run_loop(
    coordinator: Arc<PageCoordinator>,
    config: TuiConfig,
    terminal_guard: &mut TerminalGuard,
) -> Result<(), String> {
    let mut controller = Controller::with_coordinator(coordinator.clone());
    let mut dispatcher = Dispatcher::new(config.keymap);
    let theme = config.theme;
    let layout = config.layout;
    let mut in_flight = None;
    let mut quit_requested = false;

    // Bootstrap follows the same draw-before-browser-work lifecycle as every
    // later page transition.
    controller.bootstrap();

    let result = (|| loop {
        poll_worker(&mut in_flight, &mut controller);
        controller.synchronize_companion_state();
        {
            let terminal = terminal_guard.terminal_mut();
            let size = terminal.size().map_err(|e| e.to_string())?;
            // Content outer box is total height minus chrome (1) and status (1).
            // Reserve 1 col for the right-edge scrollbar (matches render), then
            // padding + optional max-width for the markdown viewport.
            let outer_h = size.height.saturating_sub(2);
            let band_w = size.width.saturating_sub(1).max(1); // scrollbar column
            let (vw, vh) =
                layout.content_viewport_size(band_w, outer_h, controller.state.view.full_width);
            controller.set_viewport(vw, vh);

            terminal
                .draw(|frame| render::draw_with_theme(frame, &controller, &theme, layout))
                .map_err(|e| e.to_string())?;
            // Keep the hardware cursor hidden; form edit uses an in-band soft caret.
            // Some backends re-show or park a block at the last cell after draw.
            let _ = crossterm::execute!(
                terminal.backend_mut(),
                crossterm::cursor::Hide,
                crossterm::cursor::MoveTo(0, 0)
            );
        }

        // `Terminal::draw` succeeded while lifecycle is Loading, so browser
        // work may now start. No action is executed in the key dispatcher.
        if controller.has_pending_page_action() && in_flight.is_none() {
            controller.acknowledge_loading_frame();
            let job = controller
                .take_page_action_job()
                .expect("just acknowledged");
            let session = coordinator.session().clone();
            in_flight = Some(spawn_worker(job, move |job| {
                let mut driver = SessionPageDriver::new(&session);
                Controller::execute_page_action_job(job, &mut driver)
            }));
            continue;
        }

        if can_open_editor(
            in_flight.is_some(),
            coordinator.shared().page_action_in_progress(),
        ) && controller.take_edit_external()
        {
            run_edit_external(terminal_guard, &mut controller);
            continue;
        }

        if should_exit(
            quit_requested || controller.state.should_quit,
            in_flight.is_some() || coordinator.shared().page_action_in_progress(),
        ) {
            break Ok(());
        }

        // While Loading (e.g. companion-driven), wake often enough to advance
        // the spinner; otherwise a calmer 100 ms poll is fine.
        let poll_for = if controller.state.lifecycle.is_loading() {
            LOADING_SPINNER_INTERVAL
        } else {
            Duration::from_millis(100)
        };
        if event::poll(poll_for).map_err(|e| e.to_string())? {
            match event::read().map_err(|e| e.to_string())? {
                Event::Key(key)
                    if key.kind == KeyEventKind::Press || key.kind == KeyEventKind::Repeat =>
                {
                    if let Some(chord) = chord_from_crossterm(key.code, key.modifiers) {
                        match dispatcher.handle_key(&mut controller, chord) {
                            DispatchOutcome::Quit => quit_requested = true,
                            DispatchOutcome::Continue | DispatchOutcome::Redraw => {}
                        }
                    }
                }
                Event::Resize(_, _) => {}
                Event::Paste(text) => {
                    let _ = dispatcher.handle_paste(&mut controller, &text);
                }
                _ => {}
            }
        }
    })();

    // No return path may let terminal restoration race a browser worker. Finish
    // and reconcile the transaction first, while retaining the primary error.
    let cleanup = finish_worker_blocking(&mut in_flight, &mut controller);
    match result {
        Err(error) => Err(error),
        Ok(()) => cleanup,
    }
}

struct InFlightPageAction {
    receiver: Receiver<PreparedPageAction>,
    join: JoinHandle<()>,
    ticket: crate::tui::shared::PageActionTicket,
    action: String,
}

fn spawn_worker(
    job: crate::tui::controller::PageActionJob,
    worker: impl FnOnce(crate::tui::controller::PageActionJob) -> PreparedPageAction + Send + 'static,
) -> InFlightPageAction {
    let (ticket, action) = job.failure_context();
    let (sender, receiver) = mpsc::sync_channel(1);
    let join = thread::spawn(move || {
        let prepared = worker(job);
        let _ = sender.send(prepared);
    });
    InFlightPageAction {
        receiver,
        join,
        ticket,
        action,
    }
}

fn finish_received(
    worker: InFlightPageAction,
    received: Result<PreparedPageAction, ()>,
    controller: &mut Controller,
) -> Result<(), String> {
    let joined = worker.join.join();
    match (received, joined) {
        (Ok(prepared), Ok(())) => controller.complete_page_action(prepared),
        _ => {
            controller.fail_abandoned_page_action(
                worker.ticket,
                worker.action,
                "browser worker disconnected or panicked".into(),
            );
            Err("browser worker disconnected or panicked".into())
        }
    }
}

fn poll_worker(in_flight: &mut Option<InFlightPageAction>, controller: &mut Controller) {
    let received = match in_flight.as_ref().map(|worker| worker.receiver.try_recv()) {
        Some(Ok(prepared)) => Some(Ok(prepared)),
        Some(Err(TryRecvError::Disconnected)) => Some(Err(())),
        _ => None,
    };
    if let Some(received) = received {
        let worker = in_flight.take().expect("worker exists");
        let _ = finish_received(worker, received, controller);
    }
}

fn finish_worker_blocking(
    in_flight: &mut Option<InFlightPageAction>,
    controller: &mut Controller,
) -> Result<(), String> {
    let Some(worker) = in_flight.take() else {
        return Ok(());
    };
    let received = worker.receiver.recv().map_err(|_| ());
    finish_received(worker, received, controller)
}

fn can_open_editor(terminal_worker: bool, shared_loading: bool) -> bool {
    !terminal_worker && !shared_loading
}

fn should_exit(quit_requested: bool, work_in_flight: bool) -> bool {
    quit_requested && !work_in_flight
}

/// Suspend the TUI, open page markdown in `$VISUAL`/`$EDITOR`/`vi`, resume.
fn run_edit_external(terminal_guard: &mut TerminalGuard, controller: &mut Controller) {
    let Some(markdown) = controller.export_page_markdown() else {
        controller.state.view.set_status("nothing to edit");
        return;
    };
    let stem = controller.export_page_stem();
    let result = terminal_guard.run_external(|| {
        let path = crate::tui::editor::write_temp_markdown(&stem, &markdown)?;
        let editor = crate::tui::editor::resolve_editor_command();
        let launch = crate::tui::editor::launch_editor_command(&editor, &path);
        let _ = std::fs::remove_file(&path);
        launch?;
        Ok(())
    });
    match result {
        Ok(()) => controller.state.view.set_status("editor closed"),
        Err(msg) => controller.state.view.set_status(format!("editor: {msg}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::backend::FakeSessionBackend;
    use crate::tui::state::Lifecycle;

    fn worker_fixture() -> (Controller, Arc<BrowserSession>) {
        let session = Arc::new(BrowserSession::with_test_backend(FakeSessionBackend::new()));
        let coordinator = Arc::new(PageCoordinator::new(session.clone(), SharedTuiState::new()));
        (Controller::with_coordinator(coordinator), session)
    }

    fn acknowledged_bootstrap(
        controller: &mut Controller,
    ) -> crate::tui::controller::PageActionJob {
        controller.bootstrap();
        controller.acknowledge_loading_frame();
        controller.take_page_action_job().expect("acknowledged job")
    }

    #[test]
    fn acknowledged_job_runs_on_worker_and_completes_ready() {
        let (mut controller, session) = worker_fixture();
        let job = acknowledged_bootstrap(&mut controller);
        let mut worker = Some(spawn_worker(job, move |job| {
            let mut driver = SessionPageDriver::new(&session);
            Controller::execute_page_action_job(job, &mut driver)
        }));

        finish_worker_blocking(&mut worker, &mut controller).expect("worker completion");
        assert!(worker.is_none());
        assert!(matches!(controller.state.lifecycle, Lifecycle::Ready));
    }

    #[test]
    fn panicked_worker_is_joined_and_retains_previous_document() {
        let (mut controller, session) = worker_fixture();
        let initial = acknowledged_bootstrap(&mut controller);
        let mut initial_worker = Some(spawn_worker(initial, move |job| {
            let mut driver = SessionPageDriver::new(&session);
            Controller::execute_page_action_job(job, &mut driver)
        }));
        finish_worker_blocking(&mut initial_worker, &mut controller).expect("bootstrap");
        let revision = controller
            .state
            .document()
            .expect("document")
            .document
            .revision
            .clone();

        controller.reload();
        controller.acknowledge_loading_frame();
        let job = controller.take_page_action_job().expect("reload job");
        let mut worker = Some(spawn_worker(job, |_| panic!("injected worker panic")));
        assert!(finish_worker_blocking(&mut worker, &mut controller).is_err());
        assert!(worker.is_none());
        assert!(matches!(
            controller.state.lifecycle,
            Lifecycle::Error { .. }
        ));
        assert_eq!(
            controller.state.document().unwrap().document.revision,
            revision
        );
    }

    #[test]
    fn blocking_cleanup_waits_for_worker_signal() {
        let (mut controller, session) = worker_fixture();
        let job = acknowledged_bootstrap(&mut controller);
        let (release_tx, release_rx) = mpsc::channel();
        let worker = spawn_worker(job, move |job| {
            release_rx.recv().expect("release");
            let mut driver = SessionPageDriver::new(&session);
            Controller::execute_page_action_job(job, &mut driver)
        });
        let (done_tx, done_rx) = mpsc::channel();
        let cleanup = thread::spawn(move || {
            let mut worker = Some(worker);
            let result = finish_worker_blocking(&mut worker, &mut controller);
            done_tx.send((result, controller)).unwrap();
        });
        assert!(matches!(done_rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
        release_tx.send(()).unwrap();
        let (result, controller) = done_rx.recv().unwrap();
        cleanup.join().unwrap();
        result.expect("cleanup completion");
        assert!(matches!(controller.state.lifecycle, Lifecycle::Ready));
    }

    #[test]
    fn editor_requires_both_worker_and_shared_lifecycle_idle() {
        assert!(!can_open_editor(true, false));
        assert!(!can_open_editor(false, true));
        assert!(!can_open_editor(true, true));
        assert!(can_open_editor(false, false));
    }

    #[test]
    fn quit_waits_for_in_flight_browser_work() {
        assert!(!should_exit(true, true));
        assert!(should_exit(true, false));
        assert!(!should_exit(false, false));
    }

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
                "enable_paste",
                "hide_cursor",
                "show_cursor",
                "disable_paste",
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
            ["show_cursor", "disable_paste", "leave_alt", "disable_raw"]
        );
        assert!(lifecycle.bracketed_paste);
        assert!(!lifecycle.raw_mode && !lifecycle.alternate_screen);
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
                "enable_paste",
                "disable_paste",
                "leave_alt",
                "disable_raw",
            ]
        );
    }
}

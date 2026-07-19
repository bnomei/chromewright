//! Terminal browser (`chromewright tui`): lifecycle, keymap, and semantic navigation.
//!
//! Compiled only with the default-enabled, optional `tui` feature. Shares one
//! [`BrowserSession`](crate::browser::BrowserSession) and one SemanticDocument
//! with its loopback MCP Companion, `tui_*` tools, and bounded revisioned
//! resources via [`SharedTuiState`]. Standard stdio MCP remains separate.
//!
//! Layers: app event loop, controller lifecycle, dispatch/keymap
//! (KeyChord → Action), content lines, and managed_headless
//! BrowserSessionPolicy for `--headless tui`.

mod action;
mod app;
mod clipboard;
mod companion;
mod config;
mod content;
mod controller;
mod coordinator;
mod dispatch;
mod driver;
mod editor;
mod hints;
mod keymap;
mod managed_headless;
mod render;
mod shared;
mod state;
mod theme;
mod url_history;

pub use action::Action;
pub use app::{TuiOptions, run_tui, run_tui_with_config};
pub use config::{TuiConfig, TuiLayout, default_config_path, example_config_toml, load_tui_config};
pub use controller::Controller;
pub use coordinator::PageCoordinator;
pub use driver::{FakePageDriver, PageDriver, SessionPageDriver};
pub use keymap::{KeyChord, KeyCode, KeyModifiers, KeySequence, TuiKeymap};
pub use managed_headless::{BrowserSessionPolicy, ManagedHeadlessSession};
pub use shared::{
    Attention, CoordinationError, CoordinationSnapshot, DEFAULT_REVISION_RETENTION,
    MAX_ATTENTION_MESSAGE_CHARS, PageActionTicket, SharedTuiState,
};
pub use state::{HintMode, InputKind, InteractionMode, Lifecycle, PublishedPage, TuiState};
pub use theme::{ThemePalette, ThemeRole, TuiTheme};

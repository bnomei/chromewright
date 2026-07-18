//! Terminal browser (`chromewright tui`): lifecycle, keymap, and semantic navigation.
//!
//! Compiled only with the opt-in `tui` feature. Shares one `BrowserSession` and one
//! semantic document with its Phase 5 loopback MCP companion, `tui_*` tools,
//! and bounded revisioned resources. Standard stdio MCP remains separate.

mod action;
mod app;
mod clipboard;
mod companion;
mod config;
mod content;
mod controller;
mod dispatch;
mod driver;
mod hints;
mod keymap;
mod render;
mod shared;
mod state;

pub use action::Action;
pub use app::{TuiOptions, run_tui, run_tui_with_config};
pub use config::{TuiConfig, default_config_path, example_config_toml, load_tui_config};
pub use controller::Controller;
pub use driver::{FakePageDriver, PageDriver, SessionPageDriver};
pub use keymap::{KeyChord, KeyCode, KeyModifiers, KeySequence, TuiKeymap};
pub use shared::{
    Attention, CoordinationError, CoordinationSnapshot, DEFAULT_REVISION_RETENTION,
    MAX_ATTENTION_MESSAGE_CHARS, SharedTuiState,
};
pub use state::{HintMode, InputKind, InteractionMode, Lifecycle, PublishedPage, TuiState};

//! Terminal browser (`chromewright tui`): lifecycle, keymap, and semantic navigation.
//!
//! Compiled only with the opt-in `tui` feature. Shares one `BrowserSession` and one
//! semantic document with later Phase 5 MCP surfaces. Does not host MCP tools or
//! resources (Phase 5).

mod action;
mod app;
mod clipboard;
mod config;
mod content;
mod controller;
mod dispatch;
mod driver;
mod hints;
mod keymap;
mod render;
mod state;

pub use action::Action;
pub use app::{TuiOptions, run_tui, run_tui_with_config};
pub use config::{TuiConfig, default_config_path, example_config_toml, load_tui_config};
pub use controller::Controller;
pub use driver::{FakePageDriver, PageDriver, SessionPageDriver};
pub use keymap::{KeyChord, KeyCode, KeyModifiers, KeySequence, TuiKeymap};
pub use state::{HintMode, InputKind, InteractionMode, Lifecycle, PublishedPage, TuiState};

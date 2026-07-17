//! Named TUI actions resolved by the keymap (never hard-coded in the event loop).

use serde::{Deserialize, Serialize};

/// Semantic actions the terminal browser can perform.
///
/// Key bindings map to these names via [`crate::tui::keymap::TuiKeymap`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// `f` — enter link-hint mode; follow in current tab.
    LinkHintsFollow,
    /// `F` — enter link-hint mode; open target in a new tab.
    LinkHintsNewTab,
    /// `j` — scroll / move selection down one block.
    ScrollDown,
    /// `k` — scroll / move selection up one block.
    ScrollUp,
    /// `h` — horizontal scroll left when content overflows.
    ScrollLeft,
    /// `l` — horizontal scroll right when content overflows.
    ScrollRight,
    /// `u` — half-page up.
    HalfPageUp,
    /// `d` — half-page down.
    HalfPageDown,
    /// `gg` — jump to document top.
    GoTop,
    /// `G` — jump to document bottom.
    GoBottom,
    /// `gi` — focus first focusable form control.
    FocusFirstInput,
    /// `H` — browser history back.
    HistoryBack,
    /// `L` — browser history forward.
    HistoryForward,
    /// `r` — reload active page.
    Reload,
    /// `w` — next tab.
    NextTab,
    /// `q` — previous tab.
    PrevTab,
    /// `x` — close current tab.
    CloseTab,
    /// `t` — open a new tab.
    NewTab,
    /// `o` — open URL entry prompt.
    OpenUrl,
    /// `/` — forward search by exact semantic content.
    Search,
    /// `Space` — collapse / expand selected block.
    Collapse,
    /// `i` — inspect selected component metadata.
    Inspect,
    /// `y` — copy rendered block text (OSC 52).
    CopyBlock,
    /// `Y` — copy opaque semantic_ref (OSC 52).
    CopyRef,
    /// `Tab` — next focusable control.
    TabNext,
    /// `Shift-Tab` — previous focusable control.
    TabPrev,
    /// `Enter` — confirm URL/search/input submission.
    Confirm,
    /// `Escape` — leave prompt, hint, inspect, or clear error overlay.
    Escape,
    /// `Ctrl-c` — quit the TUI.
    Quit,
}

impl Action {
    /// Stable snake_case name used in TOML configuration.
    pub fn name(self) -> &'static str {
        match self {
            Self::LinkHintsFollow => "link_hints_follow",
            Self::LinkHintsNewTab => "link_hints_new_tab",
            Self::ScrollDown => "scroll_down",
            Self::ScrollUp => "scroll_up",
            Self::ScrollLeft => "scroll_left",
            Self::ScrollRight => "scroll_right",
            Self::HalfPageUp => "half_page_up",
            Self::HalfPageDown => "half_page_down",
            Self::GoTop => "go_top",
            Self::GoBottom => "go_bottom",
            Self::FocusFirstInput => "focus_first_input",
            Self::HistoryBack => "history_back",
            Self::HistoryForward => "history_forward",
            Self::Reload => "reload",
            Self::NextTab => "next_tab",
            Self::PrevTab => "prev_tab",
            Self::CloseTab => "close_tab",
            Self::NewTab => "new_tab",
            Self::OpenUrl => "open_url",
            Self::Search => "search",
            Self::Collapse => "collapse",
            Self::Inspect => "inspect",
            Self::CopyBlock => "copy_block",
            Self::CopyRef => "copy_ref",
            Self::TabNext => "tab_next",
            Self::TabPrev => "tab_prev",
            Self::Confirm => "confirm",
            Self::Escape => "escape",
            Self::Quit => "quit",
        }
    }

    /// Parse a configuration action name.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "link_hints_follow" => Some(Self::LinkHintsFollow),
            "link_hints_new_tab" => Some(Self::LinkHintsNewTab),
            "scroll_down" => Some(Self::ScrollDown),
            "scroll_up" => Some(Self::ScrollUp),
            "scroll_left" => Some(Self::ScrollLeft),
            "scroll_right" => Some(Self::ScrollRight),
            "half_page_up" => Some(Self::HalfPageUp),
            "half_page_down" => Some(Self::HalfPageDown),
            "go_top" => Some(Self::GoTop),
            "go_bottom" => Some(Self::GoBottom),
            "focus_first_input" => Some(Self::FocusFirstInput),
            "history_back" => Some(Self::HistoryBack),
            "history_forward" => Some(Self::HistoryForward),
            "reload" => Some(Self::Reload),
            "next_tab" => Some(Self::NextTab),
            "prev_tab" => Some(Self::PrevTab),
            "close_tab" => Some(Self::CloseTab),
            "new_tab" => Some(Self::NewTab),
            "open_url" => Some(Self::OpenUrl),
            "search" => Some(Self::Search),
            "collapse" => Some(Self::Collapse),
            "inspect" => Some(Self::Inspect),
            "copy_block" => Some(Self::CopyBlock),
            "copy_ref" => Some(Self::CopyRef),
            "tab_next" => Some(Self::TabNext),
            "tab_prev" => Some(Self::TabPrev),
            "confirm" => Some(Self::Confirm),
            "escape" => Some(Self::Escape),
            "quit" => Some(Self::Quit),
            _ => None,
        }
    }

    /// All actions in deterministic declaration order.
    pub fn all() -> &'static [Action] {
        &ALL_ACTIONS
    }

    /// Whether performing this action may change the active page document.
    pub fn is_page_changing(self) -> bool {
        matches!(
            self,
            Self::HistoryBack
                | Self::HistoryForward
                | Self::Reload
                | Self::OpenUrl
                | Self::Confirm
                | Self::LinkHintsFollow
                | Self::LinkHintsNewTab
                | Self::NextTab
                | Self::PrevTab
                | Self::CloseTab
                | Self::NewTab
        )
    }
}

const ALL_ACTIONS: [Action; 29] = [
    Action::LinkHintsFollow,
    Action::LinkHintsNewTab,
    Action::ScrollDown,
    Action::ScrollUp,
    Action::ScrollLeft,
    Action::ScrollRight,
    Action::HalfPageUp,
    Action::HalfPageDown,
    Action::GoTop,
    Action::GoBottom,
    Action::FocusFirstInput,
    Action::HistoryBack,
    Action::HistoryForward,
    Action::Reload,
    Action::NextTab,
    Action::PrevTab,
    Action::CloseTab,
    Action::NewTab,
    Action::OpenUrl,
    Action::Search,
    Action::Collapse,
    Action::Inspect,
    Action::CopyBlock,
    Action::CopyRef,
    Action::TabNext,
    Action::TabPrev,
    Action::Confirm,
    Action::Escape,
    Action::Quit,
];

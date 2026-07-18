//! Terminal browser state: lifecycle, interaction modes, viewport, and chrome fields.
//!
//! [`TuiState`] is the local UI mirror of page readiness and interaction mode.
//! Shared companion coordination lives in [`crate::tui::shared::SharedTuiState`];
//! both use the same Loading → Ready | Error lifecycle vocabulary so neither
//! can claim Ready while the other still owns a page action.

use crate::semantic::{SemanticDocument, SemanticRef};
use crate::tui::content::ContentProjection;
use std::collections::HashSet;

/// Page lifecycle: Ready → Loading → Ready | Error (atomic publish on success).
///
/// Shared by the terminal controller and companion tools. Loading blocks normal
/// key actions. Error retains the last published page and blocks normal keys
/// until Escape dismisses it back to Ready (without requiring a new capture).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lifecycle {
    /// Settled document is published; normal commands and scrolling apply.
    Ready,
    /// Page-changing work is in flight; normal key actions are ignored.
    Loading { action: String },
    /// Last action failed; previous page (if any) is retained for recovery.
    Error { action: String, message: String },
}

impl Lifecycle {
    pub fn is_loading(&self) -> bool {
        matches!(self, Self::Loading { .. })
    }

    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready)
    }

    /// Short chrome/status label for the lifecycle state (not a key legend).
    pub fn status_label(&self) -> &str {
        match self {
            Self::Ready => "Ready",
            Self::Loading { .. } => "Loading",
            Self::Error { .. } => "Error",
        }
    }
}

/// Interaction mode while lifecycle is Ready (or Error, until Escape dismisses it).
///
/// The dispatcher routes keys by mode so Normal action maps never fire during
/// URL/search/form editing or two-key hint selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InteractionMode {
    /// Keymap-driven action dispatch (Vimari-style).
    Normal,
    /// URL bar, forward search, or form-control editing.
    Input(InputKind),
    /// Two-key link hint selection (chained until Escape).
    Hint(HintMode),
}

/// Kind of text-input overlay active while [`InteractionMode::Input`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputKind {
    /// Navigate-to URL prompt (`o` / OpenUrl).
    Url { buffer: String },
    /// Forward search by content (`/` / Search); matches bind exact `semantic_ref`s.
    Search { buffer: String },
    /// Form control value editing bound to an exact `semantic_ref`.
    Form {
        semantic_ref: SemanticRef,
        buffer: String,
    },
}

/// Whether a resolved link hint opens in the current tab or a new tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HintMode {
    /// Follow link in the current tab (`f`).
    Follow,
    /// Open link in a new tab (`F`).
    NewTab,
}

/// Snapshot published only after wait/settle + capture + reconciliation.
///
/// Chrome fields (url/title/revision) are copied from the SemanticDocument so
/// the UI never shows metadata from a different capture than the body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedPage {
    pub document: SemanticDocument,
    pub url: String,
    pub title: String,
    pub revision: String,
}

impl PublishedPage {
    /// Derive chrome fields from a complete semantic capture.
    pub fn from_document(document: SemanticDocument) -> Self {
        let url = document.document.url.clone();
        let title = document.document.title.clone();
        let revision = document.document.revision.clone();
        Self {
            document,
            url,
            title,
            revision,
        }
    }
}

/// Viewport and selection state keyed by exact `semantic_ref`.
///
/// Human selection and agent attention are independent exact refs. Collapse and
/// search match lists also store exact refs so recapture can rebind survivors
/// without fuzzy identity.
#[derive(Debug, Clone, Default)]
pub struct ViewState {
    /// Vertical scroll offset in content lines.
    pub scroll_y: usize,
    /// Horizontal scroll offset in columns (ignored while word-wrap is on).
    pub scroll_x: usize,
    /// Viewport height in lines (content area).
    pub viewport_height: usize,
    /// Viewport width in columns.
    pub viewport_width: usize,
    /// Soft-wrap long content lines to the viewport width (off by default).
    pub wrap: bool,
    /// Content projection: prose (default) hides structural chrome; structure shows it.
    pub projection: ContentProjection,
    /// Currently selected addressable component (exact ref).
    pub selection: Option<SemanticRef>,
    /// Agent attention spotlight (exact ref), independent of human selection.
    pub attention: Option<SemanticRef>,
    /// Refs painted for the current attention (root + descendants). Empty when clear.
    ///
    /// Lets prose-hidden containers (landmarks) still show a spotlight on their
    /// visible children without retargeting human selection.
    pub attention_paint: HashSet<SemanticRef>,
    /// Collapsed component refs (exact).
    pub collapsed: HashSet<SemanticRef>,
    /// Forward-search matches (exact refs, document order).
    pub search_matches: Vec<SemanticRef>,
    pub search_index: usize,
    /// Last search query (for status).
    pub search_query: String,
    /// Inspection overlay body (no key legends).
    pub inspect_text: Option<String>,
    /// Inspection panel title: full DOM path (`main > form#x > input#email`).
    pub inspect_title: Option<String>,
    /// When true, inspect stays open and refreshes as selection moves (j/k, tab, …).
    pub inspect_follow: bool,
    /// Transient status (anchor changed, clipboard fallback, etc.).
    pub status_message: Option<String>,
    /// Pending two-key hint buffer.
    pub hint_buffer: String,
}

impl ViewState {
    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some(msg.into());
    }

    pub fn clear_status(&mut self) {
        self.status_message = None;
    }
}

/// Full terminal browser state shared by controller and renderer.
///
/// Local only: does not own the BrowserSession. History affordances come from
/// the active browser tab; page content is the last successfully published
/// SemanticDocument (retained across Error).
#[derive(Debug, Clone)]
pub struct TuiState {
    pub lifecycle: Lifecycle,
    pub mode: InteractionMode,
    /// Last successfully published page (retained on Error).
    pub page: Option<PublishedPage>,
    pub view: ViewState,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub should_quit: bool,
    /// Clipboard fallback text when OSC 52 is unavailable.
    pub clipboard_fallback: Option<String>,
}

impl Default for TuiState {
    fn default() -> Self {
        Self {
            lifecycle: Lifecycle::Ready,
            mode: InteractionMode::Normal,
            page: None,
            view: ViewState::default(),
            can_go_back: false,
            can_go_forward: false,
            should_quit: false,
            clipboard_fallback: None,
        }
    }
}

impl TuiState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Chrome mode label shown beside lifecycle (not a shortcut legend).
    pub fn mode_label(&self) -> &'static str {
        match &self.mode {
            InteractionMode::Normal => "Normal",
            InteractionMode::Input(InputKind::Url { .. }) => "URL",
            InteractionMode::Input(InputKind::Search { .. }) => "Search",
            InteractionMode::Input(InputKind::Form { .. }) => "Input",
            InteractionMode::Hint(HintMode::Follow) => "Hint",
            InteractionMode::Hint(HintMode::NewTab) => "Hint+",
        }
    }

    pub fn is_input_mode(&self) -> bool {
        matches!(self.mode, InteractionMode::Input(_))
    }

    pub fn is_hint_mode(&self) -> bool {
        matches!(self.mode, InteractionMode::Hint(_))
    }

    /// Whether keymap-driven Normal actions may run.
    ///
    /// Requires both Normal mode and Ready lifecycle. Error retains the page
    /// for display; Escape dismisses Error back to Ready so scrolling and
    /// further page actions can resume. Quit remains available while Error.
    pub fn allows_normal_commands(&self) -> bool {
        matches!(self.mode, InteractionMode::Normal) && self.lifecycle.is_ready()
    }

    pub fn url(&self) -> &str {
        self.page.as_ref().map(|p| p.url.as_str()).unwrap_or("")
    }

    pub fn title(&self) -> &str {
        self.page.as_ref().map(|p| p.title.as_str()).unwrap_or("")
    }

    pub fn revision(&self) -> &str {
        self.page
            .as_ref()
            .map(|p| p.revision.as_str())
            .unwrap_or("")
    }

    /// Active SemanticDocument when a page has been published.
    pub fn document(&self) -> Option<&SemanticDocument> {
        self.page.as_ref().map(|p| &p.document)
    }

    /// Atomically publish document + url + title + revision and mark Ready.
    pub fn publish_page(&mut self, document: SemanticDocument) {
        self.page = Some(PublishedPage::from_document(document));
        self.lifecycle = Lifecycle::Ready;
    }

    /// Enter Loading and leave transient Input/Hint modes so keys cannot race the capture.
    pub fn enter_loading(&mut self, action: impl Into<String>) {
        self.lifecycle = Lifecycle::Loading {
            action: action.into(),
        };
        // Leave transient input/hint modes during page-changing work.
        if !matches!(self.mode, InteractionMode::Normal) {
            self.mode = InteractionMode::Normal;
            self.view.hint_buffer.clear();
        }
    }

    /// Enter Error while retaining `page` for recovery; clear transient modes.
    pub fn enter_error(&mut self, action: impl Into<String>, message: impl Into<String>) {
        self.lifecycle = Lifecycle::Error {
            action: action.into(),
            message: message.into(),
        };
        self.mode = InteractionMode::Normal;
        self.view.hint_buffer.clear();
    }

    /// Clear the published page and view after the browser has no open tabs.
    pub fn clear_session(&mut self) {
        self.lifecycle = Lifecycle::Ready;
        self.mode = InteractionMode::Normal;
        self.page = None;
        self.view = ViewState {
            viewport_height: self.view.viewport_height,
            viewport_width: self.view.viewport_width,
            wrap: self.view.wrap,
            projection: self.view.projection,
            inspect_follow: false,
            ..ViewState::default()
        };
        self.can_go_back = false;
        self.can_go_forward = false;
        self.clipboard_fallback = None;
        self.view.set_status("no open tabs — press t for a new tab");
    }

    /// Dismiss Error back to Ready while retaining the last published page.
    ///
    /// Returns `true` when an Error was cleared. Loading and Ready are left
    /// unchanged. The prior error message is kept as a transient status so the
    /// operator can still see why the last action failed after recovery.
    pub fn clear_error(&mut self) -> bool {
        match std::mem::replace(&mut self.lifecycle, Lifecycle::Ready) {
            Lifecycle::Error { message, .. } => {
                self.mode = InteractionMode::Normal;
                self.view.hint_buffer.clear();
                self.view.inspect_text = None;
                self.view.inspect_title = None;
                self.view.inspect_follow = false;
                self.view.set_status(format!("dismissed: {message}"));
                true
            }
            other => {
                self.lifecycle = other;
                false
            }
        }
    }

    /// Set chrome affordances from the active browser tab, never from a local
    /// approximation of global navigation history.
    pub fn set_history_availability(&mut self, can_go_back: bool, can_go_forward: bool) {
        self.can_go_back = can_go_back;
        self.can_go_forward = can_go_forward;
    }

    /// Reconcile selection/collapse/search by exact identity after recapture.
    ///
    /// Surviving selected anchor restores viewport-relative position; otherwise
    /// scroll is clamped and an identity-change status is set.
    pub fn reconcile_after_capture(
        &mut self,
        new_document: SemanticDocument,
        previous_selection: Option<SemanticRef>,
        previous_scroll_y: usize,
        anchor_offset_in_viewport: usize,
        content_line_of: impl Fn(&SemanticDocument, &SemanticRef) -> Option<usize>,
    ) {
        let mut collapsed = HashSet::new();
        for old in &self.view.collapsed {
            if let Ok(rebound) = new_document.rebind_surviving(old) {
                collapsed.insert(rebound);
            }
        }

        let mut search_matches = Vec::new();
        for old in &self.view.search_matches {
            if let Ok(rebound) = new_document.rebind_surviving(old) {
                search_matches.push(rebound);
            }
        }
        let search_index = self
            .view
            .search_index
            .min(search_matches.len().saturating_sub(1));

        // The anchor must be measured against the reconciled collapsed layout,
        // not the old or fully expanded projection.
        self.view.collapsed = collapsed;

        let mut selection = None;
        let mut scroll_y = previous_scroll_y;
        let mut status = None;

        if let Some(prev) = previous_selection {
            match new_document.rebind_surviving(&prev) {
                Ok(rebound) => {
                    selection = Some(rebound.clone());
                    if let Some(line) = content_line_of(&new_document, &rebound) {
                        // Restore at the same viewport-relative offset.
                        scroll_y = line.saturating_sub(anchor_offset_in_viewport);
                    }
                }
                Err(_) => {
                    status = Some("anchor changed".to_string());
                    // Clamp later once content height is known; keep absolute scroll for now.
                }
            }
        }

        self.view.search_matches = search_matches;
        self.view.search_index = search_index;
        self.view.selection = selection;
        self.view.scroll_y = scroll_y;
        if let Some(msg) = status {
            self.view.set_status(msg);
        } else {
            // Clear only anchor messages; keep clipboard notes until next action.
            if self.view.status_message.as_deref() == Some("anchor changed") {
                self.view.clear_status();
            }
        }

        self.publish_page(new_document);
    }

    /// Keep vertical scroll within the current content length and viewport height.
    pub fn clamp_scroll(&mut self, content_len: usize) {
        let max_scroll = content_len.saturating_sub(self.view.viewport_height.max(1));
        if self.view.scroll_y > max_scroll {
            self.view.scroll_y = max_scroll;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_commands_blocked_in_input_and_loading() {
        let mut s = TuiState::new();
        assert!(s.allows_normal_commands());

        s.mode = InteractionMode::Input(InputKind::Url {
            buffer: String::new(),
        });
        assert!(!s.allows_normal_commands());

        s.mode = InteractionMode::Normal;
        s.enter_loading("reload");
        assert!(!s.allows_normal_commands());
    }

    #[test]
    fn error_retains_last_page() {
        use crate::dom::DocumentMetadata;
        use crate::semantic::SemanticDocument;

        let doc = SemanticDocument::empty(DocumentMetadata {
            document_id: "d".into(),
            revision: "1".into(),
            url: "https://example.com/".into(),
            title: "T".into(),
            ready_state: "complete".into(),
            frames: vec![],
        })
        .expect("empty");
        let mut s = TuiState::new();
        s.publish_page(doc);
        assert_eq!(s.url(), "https://example.com/");
        s.enter_error("navigate", "boom");
        assert_eq!(s.url(), "https://example.com/");
        assert!(matches!(s.lifecycle, Lifecycle::Error { .. }));
    }

    #[test]
    fn error_blocks_semantic_actions_until_dismissed() {
        let mut state = TuiState::new();
        state.enter_error("reload", "failed");
        assert!(!state.allows_normal_commands());
        assert!(state.clear_error());
        assert!(state.lifecycle.is_ready());
        assert!(state.allows_normal_commands());
        assert_eq!(
            state.view.status_message.as_deref(),
            Some("dismissed: failed")
        );
    }

    #[test]
    fn clear_error_is_noop_when_ready_or_loading() {
        let mut state = TuiState::new();
        assert!(!state.clear_error());
        state.enter_loading("navigate");
        assert!(!state.clear_error());
        assert!(state.lifecycle.is_loading());
    }
}

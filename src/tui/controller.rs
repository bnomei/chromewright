//! Lifecycle controller: Loading → browser action → settle → capture → atomic Ready | Error.
//!
//! Owns local [`TuiState`] (viewport, selection, modes) while coordinating page
//! actions with [`SharedTuiState`] so the companion and terminal share one
//! Loading lock and one published SemanticDocument. Browser work is always
//! deferred until the event loop has drawn a Loading frame.

use crate::semantic::{SemanticDocument, SemanticRef};
use crate::tui::content::{
    build_content_lines, focusable_refs, form_control_refs, line_index_of, rendered_block_text,
    search_refs,
};
use crate::tui::driver::PageDriver;
use crate::tui::hints::{HintMatch, LinkHint, assign_hints, match_hint};
use crate::tui::shared::SharedTuiState;
use crate::tui::state::{HintMode, InputKind, InteractionMode, Lifecycle, TuiState};

/// Orchestrates page-changing actions and pure view updates against [`TuiState`].
///
/// Page mutations go through a deferred queue: claim Loading on
/// [`SharedTuiState`], draw Loading, then [`Self::perform_pending_page_action`].
/// Pure view ops (scroll, collapse, search) never touch the browser driver.
pub struct Controller {
    pub state: TuiState,
    pub shared: SharedTuiState,
    /// Active link hints when in Hint mode (exact `semantic_ref` targets).
    pub hints: Vec<LinkHint>,
    /// Browser work that may begin only after the event loop has successfully
    /// drawn a Loading frame.
    pending_page_action: Option<PendingPageAction>,
}

#[derive(Debug)]
struct PendingPageAction {
    action: String,
    operation: PageOperation,
    previous_selection: Option<SemanticRef>,
    previous_scroll: usize,
    anchor_offset: usize,
    loading_frame_drawn: bool,
    hint_mode_after_success: Option<HintMode>,
}

#[derive(Debug)]
enum PageOperation {
    Bootstrap,
    Navigate(String),
    HistoryBack,
    HistoryForward,
    Reload,
    NextTab,
    PrevTab,
    CloseTab,
    NewTab,
    Follow {
        document: SemanticDocument,
        semantic_ref: SemanticRef,
        new_tab: bool,
    },
    SubmitForm {
        document: SemanticDocument,
        semantic_ref: SemanticRef,
        text: String,
    },
    Activate {
        document: SemanticDocument,
        semantic_ref: SemanticRef,
    },
}

impl Controller {
    #[cfg(test)]
    pub fn new() -> Self {
        Self::with_shared(SharedTuiState::new(std::sync::Arc::new(
            crate::browser::BrowserSession::with_test_backend(
                crate::browser::backend::FakeSessionBackend::new(),
            ),
        )))
    }

    #[cfg(not(test))]
    pub fn new() -> Self {
        panic!("Controller::new requires a shared TUI runtime")
    }

    /// Production constructor: share coordination state with the companion and tools.
    pub fn with_shared(shared: SharedTuiState) -> Self {
        Self {
            state: TuiState::new(),
            shared,
            hints: Vec::new(),
            pending_page_action: None,
        }
    }

    /// Test helper that seeds local state; still requires a shared runtime via [`Self::new`].
    pub fn with_state(state: TuiState) -> Self {
        Self {
            state,
            ..Self::new()
        }
    }

    /// Seed both local state and shared coordination (tests / recovery harnesses).
    pub fn with_state_and_shared(state: TuiState, shared: SharedTuiState) -> Self {
        Self {
            state,
            shared,
            hints: Vec::new(),
            pending_page_action: None,
        }
    }

    /// Schedule the initial capture. The app must draw Loading, acknowledge it,
    /// then call [`Self::perform_pending_page_action`].
    pub fn bootstrap(&mut self) {
        self.queue_page_action("bootstrap", PageOperation::Bootstrap, None);
    }

    /// Queue URL navigation; browser work remains deferred until a Loading
    /// frame has been rendered and acknowledged.
    pub fn navigate_to(&mut self, url: &str) {
        self.queue_page_action("navigate", PageOperation::Navigate(url.to_string()), None);
    }

    /// Queue browser history back for the active tab (not a local history stack).
    pub fn history_back(&mut self) {
        self.queue_page_action("history_back", PageOperation::HistoryBack, None);
    }

    /// Queue browser history forward for the active tab.
    pub fn history_forward(&mut self) {
        self.queue_page_action("history_forward", PageOperation::HistoryForward, None);
    }

    /// Queue a full page reload and semantic recapture.
    pub fn reload(&mut self) {
        self.queue_page_action("reload", PageOperation::Reload, None);
    }

    /// Queue activation of the next browser tab (document-changing).
    pub fn next_tab(&mut self) {
        self.queue_page_action("next_tab", PageOperation::NextTab, None);
    }

    /// Queue activation of the previous browser tab (document-changing).
    pub fn prev_tab(&mut self) {
        self.queue_page_action("prev_tab", PageOperation::PrevTab, None);
    }

    /// Queue close of the active tab and recapture of whatever remains active.
    pub fn close_tab(&mut self) {
        self.queue_page_action("close_tab", PageOperation::CloseTab, None);
    }

    /// Queue opening a blank tab and switching to it.
    pub fn new_tab(&mut self) {
        self.queue_page_action("new_tab", PageOperation::NewTab, None);
    }

    /// Queue a link follow by exact `semantic_ref` (current tab or new tab).
    ///
    /// Resolves the ref against the published document before entering Loading
    /// so stale targets never claim the shared lifecycle lock.
    pub fn follow_link(&mut self, semantic_ref: &SemanticRef, new_tab: bool) -> Result<(), String> {
        let action = if new_tab {
            "hint_new_tab"
        } else {
            "hint_follow"
        };
        let document = self
            .state
            .document()
            .cloned()
            .ok_or_else(|| "no document".to_string())?;
        // Fail closed on stale/unknown refs before entering Loading.
        document.resolve(semantic_ref).map_err(|e| e.to_string())?;
        self.queue_page_action(
            action,
            PageOperation::Follow {
                document,
                semantic_ref: semantic_ref.clone(),
                new_tab,
            },
            Some(if new_tab {
                HintMode::NewTab
            } else {
                HintMode::Follow
            }),
        );
        Ok(())
    }

    /// Queue form submission bound to an exact ref; recapture afterward.
    pub fn submit_form_input(
        &mut self,
        semantic_ref: &SemanticRef,
        text: &str,
    ) -> Result<(), String> {
        let document = self
            .state
            .document()
            .cloned()
            .ok_or_else(|| "no document".to_string())?;
        document.resolve(semantic_ref).map_err(|e| e.to_string())?;
        self.queue_page_action(
            "form_submit",
            PageOperation::SubmitForm {
                document,
                semantic_ref: semantic_ref.clone(),
                text: text.to_string(),
            },
            None,
        );
        Ok(())
    }

    /// Queue activation of the currently selected exact focusable ref.
    pub fn activate_selection(&mut self) -> Result<(), String> {
        let document = self
            .state
            .document()
            .cloned()
            .ok_or_else(|| "no document".to_string())?;
        let semantic_ref = self
            .state
            .view
            .selection
            .clone()
            .ok_or_else(|| "no selection".to_string())?;
        let component = document.resolve(&semantic_ref).map_err(|e| e.to_string())?;
        if !component.is_focusable() {
            return Err("selected component is not focusable".into());
        }
        self.queue_page_action(
            "activate",
            PageOperation::Activate {
                document,
                semantic_ref,
            },
            None,
        );
        Ok(())
    }

    fn queue_page_action(
        &mut self,
        action: impl Into<String>,
        operation: PageOperation,
        hint_mode_after_success: Option<HintMode>,
    ) {
        let action = action.into();
        // The terminal is one participant in the same lifecycle as the
        // companion. Do not replace a deferred action, and publish Loading
        // before browser work can be acknowledged by the event loop.
        if self.pending_page_action.is_some() {
            return;
        }
        if let Err(error) = self.shared.begin_page_action(action.clone()) {
            self.state.enter_error(&action, error.to_string());
            return;
        }
        let prev_sel = self.state.view.selection.clone();
        let prev_scroll = self.state.view.scroll_y;
        let prev_doc = self.state.document().cloned();
        let anchor_offset = prev_doc
            .as_ref()
            .map(|d| self.anchor_offset_in_viewport(d, prev_sel.as_ref()))
            .unwrap_or(0);

        self.state.enter_loading(&action);
        self.pending_page_action = Some(PendingPageAction {
            action,
            operation,
            previous_selection: prev_sel,
            previous_scroll: prev_scroll,
            anchor_offset,
            loading_frame_drawn: false,
            hint_mode_after_success,
        });
    }

    /// True when browser work is waiting on an actual terminal Loading frame.
    pub fn has_pending_page_action(&self) -> bool {
        self.pending_page_action.is_some()
    }

    /// Reconcile companion-owned lifecycle and agent attention.
    ///
    /// Local page transitions win for lifecycle until their pending action
    /// completes. Agent attention is independent and is always applied without
    /// mutating human selection.
    pub fn synchronize_companion_state(&mut self) {
        if self.pending_page_action.is_none() {
            match self.shared.lifecycle() {
                Lifecycle::Loading { action } => self.state.enter_loading(action),
                Lifecycle::Error { action, message } => self.state.enter_error(action, message),
                Lifecycle::Ready => {
                    if let Ok(document) = self.shared.active() {
                        if self.state.document().is_none_or(|current| {
                            current.document.document_id != document.document.document_id
                                || current.document.revision != document.document.revision
                        }) {
                            let prev_sel = self.state.view.selection.clone();
                            let prev_scroll = self.state.view.scroll_y;
                            let anchor_offset =
                                self.anchor_offset_in_viewport(&document, prev_sel.as_ref());
                            let collapsed = self.state.view.collapsed.clone();
                            let content_line_of =
                                move |document: &SemanticDocument, r: &SemanticRef| {
                                    let lines = build_content_lines(document, &collapsed);
                                    line_index_of(&lines, r)
                                };
                            self.state.reconcile_after_capture(
                                document,
                                prev_sel,
                                prev_scroll,
                                anchor_offset,
                                content_line_of,
                            );
                        }
                        self.state.lifecycle = Lifecycle::Ready;
                    }
                }
            }
        }
        self.sync_attention_from_shared();
    }

    /// Apply agent attention from shared state: highlight + scroll into view,
    /// without changing human selection.
    fn sync_attention_from_shared(&mut self) {
        let attention = self.shared.attention();
        let next = attention.semantic_ref.clone();
        let changed = self.state.view.attention != next;
        let previous_selection = self.state.view.selection.clone();
        self.state.view.attention = next.clone();
        if !changed {
            return;
        }
        let Some(reference) = next else {
            return;
        };
        let lines = self.content_lines();
        if let Some(line_idx) = line_index_of(&lines, &reference) {
            self.ensure_visible(line_idx, lines.len());
        }
        // Attention scroll must never retarget human selection.
        self.state.view.selection = previous_selection;
    }

    /// Record that `Terminal::draw` completed while this transition was Loading.
    pub fn acknowledge_loading_frame(&mut self) {
        if let Some(pending) = &mut self.pending_page_action
            && self.state.lifecycle.is_loading()
        {
            pending.loading_frame_drawn = true;
        }
    }

    /// Perform deferred browser work after a successfully rendered Loading frame.
    ///
    /// Settle → capture SemanticDocument → post-capture metadata barrier →
    /// atomic Ready via [`TuiState::reconcile_after_capture`] and
    /// [`SharedTuiState::publish_with_selection`]. On failure publishes Error
    /// while retaining the last valid page. Rejects work if no Loading frame
    /// was acknowledged (draw-before-browser invariant).
    pub fn perform_pending_page_action<D: PageDriver>(
        &mut self,
        driver: &mut D,
    ) -> Result<(), String> {
        let Some(pending) = self.pending_page_action.take() else {
            return Ok(());
        };
        if !pending.loading_frame_drawn {
            let message = "browser action rejected before a Loading frame was rendered".to_string();
            self.state.enter_error(&pending.action, message.clone());
            self.shared
                .fail_page_action(pending.action.clone(), message.clone());
            return Err(message);
        }

        let result = (|| -> Result<SemanticDocument, String> {
            match pending.operation {
                PageOperation::Bootstrap => {}
                PageOperation::Navigate(url) => driver.navigate(&url).map_err(|e| e.to_string())?,
                PageOperation::HistoryBack => driver.go_back().map_err(|e| e.to_string())?,
                PageOperation::HistoryForward => driver.go_forward().map_err(|e| e.to_string())?,
                PageOperation::Reload => driver.reload().map_err(|e| e.to_string())?,
                PageOperation::NextTab => driver.next_tab().map_err(|e| e.to_string())?,
                PageOperation::PrevTab => driver.prev_tab().map_err(|e| e.to_string())?,
                PageOperation::CloseTab => driver.close_active_tab().map_err(|e| e.to_string())?,
                PageOperation::NewTab => {
                    driver.open_tab("about:blank").map_err(|e| e.to_string())?
                }
                PageOperation::Follow {
                    document,
                    semantic_ref,
                    new_tab,
                } => {
                    let _ = driver
                        .activate_ref(&document, &semantic_ref, new_tab)
                        .map_err(|e| e.to_string())?;
                }
                PageOperation::SubmitForm {
                    document,
                    semantic_ref,
                    text,
                } => {
                    let _ = driver
                        .fill_control(&document, &semantic_ref, &text)
                        .map_err(|e| e.to_string())?;
                }
                PageOperation::Activate {
                    document,
                    semantic_ref,
                } => {
                    let _ = driver
                        .activate_ref(&document, &semantic_ref, false)
                        .map_err(|e| e.to_string())?;
                }
            }
            driver.wait_settle().map_err(|e| e.to_string())?;
            // Capture first, then read metadata as the freshness barrier. A
            // hydrated page can mutate between settling and capture, so a
            // pre-capture metadata read would reject a valid later snapshot.
            // The post-capture metadata must instead describe exactly what we
            // captured before it can be published.
            let doc = driver.capture_semantic().map_err(|e| e.to_string())?;
            let metadata = driver.document_metadata().map_err(|e| e.to_string())?;
            if metadata.document_id.is_empty()
                || metadata.revision.is_empty()
                || metadata.ready_state != "complete"
            {
                return Err("browser did not provide stable complete document metadata".into());
            }
            if !crate::semantic::capture_matches_document_metadata(&doc.document, &metadata) {
                return Err("semantic capture metadata changed during publication".into());
            }
            // Atomic consistency: url/title/revision come only from this document.
            Ok(doc)
        })();
        match result {
            Ok(doc) => {
                self.finish_capture(
                    doc,
                    pending.previous_selection,
                    pending.previous_scroll,
                    pending.anchor_offset,
                );
                self.refresh_history_availability(driver);
                if let Some(mode) = pending.hint_mode_after_success {
                    self.enter_hint_mode(mode);
                }
                Ok(())
            }
            Err(msg) => {
                // Retain last valid page; never publish partial update as Ready.
                self.state.enter_error(&pending.action, msg.clone());
                self.shared
                    .fail_page_action(pending.action.clone(), msg.clone());
                Err(msg)
            }
        }
    }

    fn finish_capture(
        &mut self,
        doc: SemanticDocument,
        prev_sel: Option<SemanticRef>,
        prev_scroll: usize,
        anchor_offset: usize,
    ) {
        let collapsed = self.state.view.collapsed.clone();
        let content_line_of = move |document: &SemanticDocument, r: &SemanticRef| {
            let lines = build_content_lines(document, &collapsed);
            line_index_of(&lines, r)
        };
        self.state.reconcile_after_capture(
            doc,
            prev_sel,
            prev_scroll,
            anchor_offset,
            content_line_of,
        );
        if let Some(document) = self.state.document() {
            let lines = build_content_lines(document, &self.state.view.collapsed);
            self.state.clamp_scroll(lines.len());
            // Ensure selection exists
            if self.state.view.selection.is_none()
                && let Some(first) = lines.iter().find_map(|l| l.semantic_ref.clone())
            {
                self.state.view.selection = Some(first);
            }
        }
        // Inspection text describes the prior capture and must never survive a
        // recapture unless explicitly regenerated for the new exact ref.
        self.state.view.inspect_text = None;
        self.hints.clear();
        let document = self.state.document().expect("capture published").clone();
        let selection = self.state.view.selection.clone();
        let _ = self.shared.publish_with_selection(document, selection);
    }

    /// Push the current human selection into shared coordination for companion tools.
    pub fn publish_selection(&self) {
        if let Some(selection) = self.state.view.selection.clone() {
            let _ = self.shared.set_selection(selection);
        }
    }

    fn refresh_history_availability<D: PageDriver>(&mut self, driver: &mut D) {
        let (back, forward) = driver.history_availability().unwrap_or((false, false));
        self.state.set_history_availability(back, forward);
    }

    fn anchor_offset_in_viewport(
        &self,
        document: &SemanticDocument,
        selection: Option<&SemanticRef>,
    ) -> usize {
        let Some(sel) = selection else {
            return 0;
        };
        let lines = build_content_lines(document, &self.state.view.collapsed);
        let Some(line) = line_index_of(&lines, sel) else {
            return 0;
        };
        line.saturating_sub(self.state.view.scroll_y)
    }

    // --- Pure view operations (no driver) ---

    /// Flatten the active SemanticDocument into addressable content lines.
    pub fn content_lines(&self) -> Vec<crate::tui::content::ContentLine> {
        match self.state.document() {
            Some(doc) => build_content_lines(doc, &self.state.view.collapsed),
            None => Vec::new(),
        }
    }

    /// Move selection to the next addressable content line (and keep it visible).
    pub fn scroll_down(&mut self) {
        self.move_selection(1);
    }

    /// Move selection to the previous addressable content line (and keep it visible).
    pub fn scroll_up(&mut self) {
        self.move_selection(-1);
    }

    fn move_selection(&mut self, delta: isize) {
        let lines = self.content_lines();
        if lines.is_empty() {
            return;
        }
        let positions: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter_map(|(i, l)| l.semantic_ref.as_ref().map(|_| i))
            .collect();
        if positions.is_empty() {
            return;
        }
        let current = self
            .state
            .view
            .selection
            .as_ref()
            .and_then(|s| line_index_of(&lines, s))
            .and_then(|line| positions.iter().position(|&p| p == line))
            .unwrap_or(0);
        let next = (current as isize + delta).clamp(0, positions.len() as isize - 1) as usize;
        let line_idx = positions[next];
        self.state.view.selection = lines[line_idx].semantic_ref.clone();
        self.ensure_visible(line_idx, lines.len());
    }

    /// Scroll down by half the content viewport without changing selection.
    pub fn half_page_down(&mut self) {
        let h = self.state.view.viewport_height.max(1) / 2;
        self.state.view.scroll_y = self.state.view.scroll_y.saturating_add(h);
        let len = self.content_lines().len();
        self.state.clamp_scroll(len);
    }

    /// Scroll up by half the content viewport without changing selection.
    pub fn half_page_up(&mut self) {
        let h = self.state.view.viewport_height.max(1) / 2;
        self.state.view.scroll_y = self.state.view.scroll_y.saturating_sub(h);
    }

    /// Jump to the first content line and select its `semantic_ref` when present.
    pub fn go_top(&mut self) {
        self.state.view.scroll_y = 0;
        let lines = self.content_lines();
        if let Some(r) = lines.iter().find_map(|l| l.semantic_ref.clone()) {
            self.state.view.selection = Some(r);
        }
    }

    /// Jump to the last content line and select its `semantic_ref` when present.
    pub fn go_bottom(&mut self) {
        let lines = self.content_lines();
        let len = lines.len();
        self.state.view.scroll_y = len.saturating_sub(self.state.view.viewport_height.max(1));
        if let Some(r) = lines.iter().rev().find_map(|l| l.semantic_ref.clone()) {
            self.state.view.selection = Some(r);
        }
    }

    /// Horizontal pan left when a content line overflows the viewport width.
    pub fn scroll_left(&mut self) {
        self.state.view.scroll_x = self.state.view.scroll_x.saturating_sub(4);
    }

    /// Horizontal pan right when a content line overflows the viewport width.
    pub fn scroll_right(&mut self) {
        self.state.view.scroll_x = self.state.view.scroll_x.saturating_add(4);
    }

    fn ensure_visible(&mut self, line_idx: usize, content_len: usize) {
        let vh = self.state.view.viewport_height.max(1);
        if line_idx < self.state.view.scroll_y {
            self.state.view.scroll_y = line_idx;
        } else if line_idx >= self.state.view.scroll_y + vh {
            self.state.view.scroll_y = line_idx + 1 - vh;
        }
        self.state.clamp_scroll(content_len);
    }

    /// Collapse or expand the selected block by exact `semantic_ref` only (never rebind).
    pub fn toggle_collapse(&mut self) {
        let Some(sel) = self.state.view.selection.clone() else {
            return;
        };
        // Exact ref only — never rebind.
        if let Some(doc) = self.state.document()
            && doc.resolve(&sel).is_err()
        {
            self.state.view.set_status("stale selection");
            return;
        }
        if !self.state.view.collapsed.remove(&sel) {
            self.state.view.collapsed.insert(sel);
        }
    }

    /// Open the inspect overlay for the selected component (no key legends).
    pub fn inspect_selection(&mut self) {
        let Some(doc) = self.state.document() else {
            return;
        };
        let Some(sel) = &self.state.view.selection else {
            self.state.view.set_status("no selection");
            return;
        };
        match doc.resolve(sel) {
            Ok(c) => {
                let text = format!(
                    "kind={:?} ref={} label={:?} text={:?} href={:?} name={:?} value={:?}",
                    c.kind,
                    c.semantic_ref.as_str(),
                    c.label,
                    c.text,
                    c.attrs.href,
                    c.attrs.name,
                    c.attrs.value
                );
                self.state.view.inspect_text = Some(text);
            }
            Err(e) => {
                self.state.view.inspect_text = None;
                self.state.view.set_status(format!("inspect failed: {e}"));
            }
        }
    }

    /// Rendered plain text for the selected block (clipboard payload for `y`).
    pub fn copy_block_text(&mut self) -> Option<String> {
        let doc = self.state.document()?;
        let sel = self.state.view.selection.as_ref()?;
        doc.resolve(sel).ok()?;
        rendered_block_text(doc, sel)
    }

    /// Opaque `semantic_ref` string for the selection (clipboard payload for `Y`).
    pub fn copy_ref_text(&mut self) -> Option<String> {
        let doc = self.state.document()?;
        let sel = self.state.view.selection.as_ref()?;
        doc.resolve(sel).ok()?;
        Some(sel.as_str().to_string())
    }

    /// Enter URL-bar input with an empty buffer (never prefill the current URL).
    pub fn enter_url_input(&mut self) {
        if self.state.lifecycle.is_loading() {
            return;
        }
        // Opening a location prompt starts a new navigation. Reusing the
        // current URL both hides the mode change and causes typed URLs to be
        // appended to it rather than replacing it.
        self.state.mode = InteractionMode::Input(InputKind::Url {
            buffer: String::new(),
        });
    }

    /// Enter forward-search input mode (`/`).
    pub fn enter_search(&mut self) {
        if self.state.lifecycle.is_loading() {
            return;
        }
        self.state.mode = InteractionMode::Input(InputKind::Search {
            buffer: String::new(),
        });
    }

    /// Apply a forward search: match content lines by text, own exact `semantic_ref`s.
    ///
    /// Empty query with a previous pattern repeats forward (Vim `/` + Enter).
    /// New matches start after the current selection and wrap.
    pub fn apply_search(&mut self, query: &str) {
        if query.is_empty() && !self.state.view.search_query.is_empty() {
            self.state.mode = InteractionMode::Normal;
            self.repeat_search(true);
            return;
        }
        let lines = self.content_lines();
        let matches = search_refs(&lines, query);
        let current_line = self
            .state
            .view
            .selection
            .as_ref()
            .and_then(|selection| line_index_of(&lines, selection));
        self.state.view.search_query = query.to_string();
        self.state.view.search_matches = matches;
        self.state.view.search_index = current_line
            .and_then(|line| {
                self.state
                    .view
                    .search_matches
                    .iter()
                    .position(|semantic_ref| {
                        line_index_of(&lines, semantic_ref)
                            .is_some_and(|match_line| match_line > line)
                    })
            })
            .unwrap_or(0);
        if self.state.view.search_matches.is_empty() {
            self.state.view.set_status("pattern not found");
        } else {
            self.select_search_match(&lines);
        }
        self.state.mode = InteractionMode::Normal;
    }

    /// Advance (`n`) or reverse (`N`) within the last search match list.
    pub fn repeat_search(&mut self, forward: bool) {
        if self.state.view.search_query.is_empty() {
            self.state.view.set_status("no previous search");
            return;
        }
        if self.state.view.search_matches.is_empty() {
            self.state.view.set_status("pattern not found");
            return;
        }

        let count = self.state.view.search_matches.len();
        self.state.view.search_index = if forward {
            (self.state.view.search_index + 1) % count
        } else {
            (self.state.view.search_index + count - 1) % count
        };
        let lines = self.content_lines();
        self.select_search_match(&lines);
    }

    fn select_search_match(&mut self, lines: &[crate::tui::content::ContentLine]) {
        let Some(selected) = self
            .state
            .view
            .search_matches
            .get(self.state.view.search_index)
            .cloned()
        else {
            return;
        };
        self.state.view.selection = Some(selected.clone());
        if let Some(line) = line_index_of(lines, &selected) {
            self.ensure_visible(line, lines.len());
        }
        self.state.view.set_status(format!(
            "search: {}/{}",
            self.state.view.search_index + 1,
            self.state.view.search_matches.len()
        ));
    }

    /// Focus the first form control (`gi`) and enter form-input mode.
    pub fn focus_first_input(&mut self) {
        let Some(doc) = self.state.document() else {
            return;
        };
        let controls = form_control_refs(doc);
        if let Some(first) = controls.first() {
            self.begin_form_edit(first.clone());
        } else {
            self.state.view.set_status("no form controls");
        }
    }

    /// Cycle focusable controls (Tab / Shift-Tab); form fields enter edit mode.
    ///
    /// Leaving a form field clears Input mode so Enter cannot submit a control
    /// that is no longer the visible selection.
    pub fn tab_focus(&mut self, forward: bool) {
        let Some(doc) = self.state.document() else {
            return;
        };
        let controls = focusable_refs(doc);
        if controls.is_empty() {
            return;
        }
        let current = self.state.view.selection.as_ref();
        let idx = current
            .and_then(|c| controls.iter().position(|r| r == c))
            .unwrap_or(if forward { usize::MAX } else { 0 });
        let next = if forward {
            if idx == usize::MAX {
                0
            } else {
                (idx + 1) % controls.len()
            }
        } else if idx == 0 || idx == usize::MAX {
            controls.len() - 1
        } else {
            idx - 1
        };
        let r = controls[next].clone();
        // For form controls enter input mode; for links just select.
        if let Ok(c) = doc.resolve(&r) {
            use crate::semantic::SemanticKind;
            if matches!(
                c.kind,
                SemanticKind::Input | SemanticKind::Textarea | SemanticKind::Select
            ) {
                self.begin_form_edit(r);
                return;
            }
        }
        self.state.view.selection = Some(r.clone());
        // Leaving a form field must also relinquish its editable ownership.
        // Otherwise Enter could submit the previously focused input while a
        // link or button is visibly selected.
        self.state.mode = InteractionMode::Normal;
        let lines = self.content_lines();
        if let Some(idx) = line_index_of(&lines, &r) {
            self.ensure_visible(idx, lines.len());
        }
    }

    fn begin_form_edit(&mut self, semantic_ref: SemanticRef) {
        let buffer = self
            .state
            .document()
            .and_then(|d| d.resolve(&semantic_ref).ok())
            .and_then(|c| c.attrs.value.clone())
            .unwrap_or_default();
        self.state.view.selection = Some(semantic_ref.clone());
        self.state.mode = InteractionMode::Input(InputKind::Form {
            semantic_ref,
            buffer,
        });
    }

    /// Enter two-key hint mode over viewport-visible links (`f` / `F`).
    ///
    /// Labels are deterministic for the current scroll window. Chained follows
    /// re-enter this mode after a successful recapture until Escape.
    pub fn enter_hint_mode(&mut self, mode: HintMode) {
        if self.state.lifecycle.is_loading() {
            return;
        }
        let Some(doc) = self.state.document() else {
            return;
        };
        let links: Vec<_> = doc
            .components()
            .filter(|c| c.kind == crate::semantic::SemanticKind::Link)
            .cloned()
            .collect();
        let lines = self.content_lines();
        self.hints = assign_hints(
            &lines,
            self.state.view.scroll_y,
            self.state.view.viewport_height.max(1),
            &links,
        );
        self.state.view.hint_buffer.clear();
        self.state.mode = InteractionMode::Hint(mode);
        if self.hints.is_empty() {
            self.state.view.set_status("no visible links");
            self.state.mode = InteractionMode::Normal;
        }
    }

    /// Feed a character while in hint mode.
    ///
    /// Returns `(semantic_ref, new_tab)` when a two-key label completes exactly;
    /// partial prefixes wait; ambiguous/unknown labels fail closed and clear the buffer.
    pub fn hint_type_char(&mut self, ch: char) -> Option<(SemanticRef, bool)> {
        if !matches!(self.state.mode, InteractionMode::Hint(_)) {
            return None;
        }
        self.state.view.hint_buffer.push(ch);
        match match_hint(&self.hints, &self.state.view.hint_buffer) {
            HintMatch::Exact(r) => {
                let new_tab = matches!(self.state.mode, InteractionMode::Hint(HintMode::NewTab));
                let target = r.clone();
                // Chained follow: stay in hint mode until Escape (recompute after navigation).
                self.state.view.hint_buffer.clear();
                Some((target, new_tab))
            }
            HintMatch::Partial => None,
            HintMatch::None => {
                self.state.view.hint_buffer.clear();
                self.state.view.set_status("no matching hint");
                None
            }
        }
    }

    /// Leave Input/Hint/inspect overlays and return to Normal mode (does not clear Error lifecycle).
    pub fn escape(&mut self) {
        self.state.mode = InteractionMode::Normal;
        self.state.view.hint_buffer.clear();
        self.state.view.inspect_text = None;
        self.hints.clear();
    }

    /// Update content-area dimensions after a terminal resize and clamp scroll.
    pub fn set_viewport(&mut self, width: usize, height: usize) {
        self.state.view.viewport_width = width;
        self.state.view.viewport_height = height;
        let len = self.content_lines().len();
        self.state.clamp_scroll(len);
    }
}

impl Default for Controller {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::DocumentMetadata;
    use crate::semantic::normalize::{RawSemanticNode, normalize_fixture};
    use crate::tui::driver::FakePageDriver;
    use crate::tui::state::Lifecycle;

    fn meta(rev: &str, url: &str) -> DocumentMetadata {
        DocumentMetadata {
            document_id: "doc".into(),
            revision: rev.into(),
            url: url.into(),
            title: format!("Title {rev}"),
            ready_state: "complete".into(),
            frames: vec![],
        }
    }

    fn link_doc(rev: &str, url: &str, id: &str, href: &str) -> SemanticDocument {
        normalize_fixture(
            meta(rev, url),
            vec![RawSemanticNode {
                kind: "link".into(),
                tag: Some("a".into()),
                id: Some(id.into()),
                unique_id: true,
                selector: None,
                text: Some("Go".into()),
                href: Some(href.into()),
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
        .expect("doc")
    }

    fn text_doc(rev: &str, id: &str, text: &str) -> SemanticDocument {
        normalize_fixture(meta(rev, "https://example.com/"), vec![raw_text(id, text)]).expect("doc")
    }

    fn raw_text(id: &str, text: &str) -> RawSemanticNode {
        RawSemanticNode {
            kind: "text".into(),
            tag: Some("p".into()),
            id: Some(id.into()),
            unique_id: true,
            selector: None,
            text: Some(text.into()),
            href: None,
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
        }
    }

    fn search_doc() -> SemanticDocument {
        normalize_fixture(
            meta("1", "https://example.com/"),
            vec![
                raw_text("first", "needle one"),
                raw_text("middle", "other"),
                raw_text("last", "needle two"),
            ],
        )
        .expect("search doc")
    }

    fn render_loading_and_run(
        ctl: &mut Controller,
        driver: &mut FakePageDriver,
    ) -> Result<(), String> {
        assert!(ctl.state.lifecycle.is_loading());
        assert!(ctl.has_pending_page_action());
        ctl.acknowledge_loading_frame();
        ctl.perform_pending_page_action(driver)
    }

    #[test]
    fn open_url_starts_with_an_empty_location_buffer() {
        let mut ctl = Controller::new();
        ctl.state.publish_page(text_doc("1", "t", "one"));

        ctl.enter_url_input();

        assert!(matches!(
            ctl.state.mode,
            InteractionMode::Input(InputKind::Url { ref buffer }) if buffer.is_empty()
        ));
    }

    #[test]
    fn vim_search_starts_after_selection_and_n_wraps_both_directions() {
        let document = search_doc();
        let refs = document.semantic_refs();
        let mut ctl = Controller::new();
        ctl.state.publish_page(document);
        ctl.state.view.selection = Some(refs[0].clone());

        ctl.apply_search("needle");
        assert_eq!(ctl.state.view.selection.as_ref(), Some(&refs[2]));
        assert_eq!(ctl.state.view.search_index, 1);

        ctl.repeat_search(true);
        assert_eq!(ctl.state.view.selection.as_ref(), Some(&refs[0]));
        assert_eq!(ctl.state.view.search_index, 0);

        ctl.repeat_search(false);
        assert_eq!(ctl.state.view.selection.as_ref(), Some(&refs[2]));
        assert_eq!(ctl.state.view.search_index, 1);
    }

    #[test]
    fn empty_search_repeats_the_previous_pattern() {
        let document = search_doc();
        let refs = document.semantic_refs();
        let mut ctl = Controller::new();
        ctl.state.publish_page(document);

        ctl.apply_search("needle");
        assert_eq!(ctl.state.view.selection.as_ref(), Some(&refs[0]));
        ctl.apply_search("");
        assert_eq!(ctl.state.view.selection.as_ref(), Some(&refs[2]));
        assert_eq!(ctl.state.view.search_query, "needle");
    }

    #[test]
    fn lifecycle_loading_to_ready_is_atomic() {
        let d1 = text_doc("1", "t", "one");
        let d2 = text_doc("2", "t", "two");
        let mut driver = FakePageDriver::new(vec![d1.clone(), d2.clone()]);
        let mut ctl = Controller::new();
        ctl.state.publish_page(d1);
        assert_eq!(ctl.state.revision(), "1");

        ctl.navigate_to("https://example.com/two");
        render_loading_and_run(&mut ctl, &mut driver).expect("nav");
        assert!(ctl.state.lifecycle.is_ready());
        assert_eq!(ctl.state.revision(), "2");
        assert_eq!(ctl.state.title(), "Title 2");
        assert_eq!(ctl.state.url(), d2.document.url);
        // document/url/title/revision all from same published snapshot
        let page = ctl.state.page.as_ref().unwrap();
        assert_eq!(page.document.document.revision, page.revision);
        assert_eq!(page.document.document.url, page.url);
        assert_eq!(page.document.document.title, page.title);
    }

    #[test]
    fn local_transition_publishes_shared_loading_and_serializes_companion_refresh() {
        let d1 = text_doc("1", "t", "one");
        let d2 = text_doc("2", "t", "two");
        let mut driver = FakePageDriver::new(vec![d1.clone(), d2]);
        let mut ctl = Controller::new();
        ctl.shared.activate_runtime();
        ctl.state.publish_page(d1);
        ctl.navigate_to("https://example.com/two");

        assert!(matches!(ctl.shared.lifecycle(), Lifecycle::Loading { .. }));
        assert_eq!(
            ctl.shared.refresh(),
            Err(crate::tui::CoordinationError::RefreshInProgress)
        );

        render_loading_and_run(&mut ctl, &mut driver).expect("terminal navigation");
        assert!(ctl.shared.lifecycle().is_ready());
        assert_eq!(
            ctl.shared
                .active()
                .expect("shared capture")
                .document
                .revision,
            "2"
        );
    }

    #[test]
    fn error_retains_last_valid_render() {
        let d1 = text_doc("1", "t", "one");
        let mut driver = FakePageDriver::new(vec![d1.clone()]);
        driver.fail_next = Some("network down".into());
        let mut ctl = Controller::new();
        ctl.state.publish_page(d1);
        ctl.navigate_to("https://example.com/x");
        let err = render_loading_and_run(&mut ctl, &mut driver).expect_err("fail");
        assert!(err.contains("network"));
        assert!(matches!(ctl.state.lifecycle, Lifecycle::Error { .. }));
        assert!(matches!(ctl.shared.lifecycle(), Lifecycle::Error { .. }));
        assert_eq!(ctl.state.revision(), "1");
        assert_eq!(ctl.state.url(), "https://example.com/");
    }

    #[test]
    fn capture_failure_is_error_not_ready() {
        let d1 = text_doc("1", "t", "one");
        let mut driver = FakePageDriver::new(vec![d1.clone()]);
        driver.fail_capture = Some("capture blew up".into());
        let mut ctl = Controller::new();
        ctl.state.publish_page(d1);
        ctl.reload();
        let _ = render_loading_and_run(&mut ctl, &mut driver);
        assert!(matches!(ctl.state.lifecycle, Lifecycle::Error { .. }));
        assert_eq!(ctl.state.revision(), "1");
    }

    #[test]
    fn capture_accepts_hydration_change_when_post_capture_metadata_matches() {
        let before_capture = text_doc("1", "old", "before hydration");
        let captured = text_doc("2", "new", "after hydration");
        let mut driver = FakePageDriver::new(vec![before_capture, captured.clone()]);
        // This mutation would make the old pre-capture barrier observe rev 1
        // and reject the valid rev 2 capture. The post-capture barrier sees
        // the captured document's complete metadata instead.
        driver.advance_page_on_capture = true;
        let mut ctl = Controller::new();

        ctl.bootstrap();
        render_loading_and_run(&mut ctl, &mut driver).expect("post-hydration capture");

        assert!(ctl.state.lifecycle.is_ready());
        assert_eq!(ctl.state.revision(), "2");
        assert_eq!(ctl.state.document(), Some(&captured));
    }

    #[test]
    fn capture_rejects_mismatched_post_capture_metadata() {
        let captured = text_doc("2", "new", "captured document");
        let stale_metadata = meta("1", "https://example.com/");
        let mut driver = FakePageDriver::new(vec![captured.clone()]);
        driver.metadata_responses.push(stale_metadata);
        let mut ctl = Controller::new();
        ctl.state
            .publish_page(text_doc("0", "old", "last valid render"));

        ctl.reload();
        let error = render_loading_and_run(&mut ctl, &mut driver)
            .expect_err("post-capture mismatch must fail closed");

        assert_eq!(
            error,
            "semantic capture metadata changed during publication"
        );
        assert!(matches!(ctl.state.lifecycle, Lifecycle::Error { .. }));
        assert_eq!(ctl.state.revision(), "0");
    }

    #[test]
    fn idle_shared_state_cannot_clear_a_local_capture_error() {
        let d1 = text_doc("1", "t", "one");
        let mut driver = FakePageDriver::new(vec![d1.clone()]);
        driver.fail_capture = Some("capture blew up".into());
        let mut ctl = Controller::new();
        ctl.state.publish_page(d1);
        ctl.reload();
        let _ = render_loading_and_run(&mut ctl, &mut driver);

        ctl.synchronize_companion_state();
        assert!(matches!(ctl.state.lifecycle, Lifecycle::Error { .. }));
    }

    #[test]
    fn surviving_anchor_restores_viewport_offset() {
        let d1 = text_doc("1", "anchor", "hello");
        let d2 = text_doc("2", "anchor", "hello again");
        let mut ctl = Controller::new();
        ctl.state.publish_page(d1.clone());
        let sel = d1.semantic_refs().into_iter().next().unwrap();
        ctl.state.view.selection = Some(sel.clone());
        ctl.state.view.scroll_y = 0;
        ctl.state.view.viewport_height = 10;

        let mut driver = FakePageDriver::new(vec![d1, d2]);
        // Advance fake page index via navigate
        driver.page_index = 0;
        ctl.reload();
        render_loading_and_run(&mut ctl, &mut driver).expect("reload");
        // Identity survives → selection rebound
        let new_sel = ctl.state.view.selection.as_ref().unwrap();
        assert!(ctl.state.document().unwrap().resolve(new_sel).is_ok());
        assert_ne!(new_sel, &sel);
    }

    #[test]
    fn missing_anchor_sets_status() {
        let d1 = text_doc("1", "gone", "hello");
        let d2 = text_doc("2", "other", "world");
        let mut ctl = Controller::new();
        ctl.state.publish_page(d1.clone());
        ctl.state.view.selection = Some(d1.semantic_refs().into_iter().next().unwrap());
        let mut driver = FakePageDriver::new(vec![d1, d2]);
        ctl.navigate_to("https://example.com/");
        render_loading_and_run(&mut ctl, &mut driver).expect("nav");
        assert_eq!(
            ctl.state.view.status_message.as_deref(),
            Some("anchor changed")
        );
    }

    #[test]
    fn follow_uses_exact_ref_only() {
        let d1 = link_doc("1", "https://example.com/", "home", "/next");
        let d2 = link_doc("2", "https://example.com/next", "home", "/");
        let r = d1.semantic_refs().into_iter().next().unwrap();
        let mut driver = FakePageDriver::new(vec![d1.clone(), d2]);
        let mut ctl = Controller::new();
        ctl.state.publish_page(d1);
        ctl.follow_link(&r, false).expect("schedule follow");
        render_loading_and_run(&mut ctl, &mut driver).expect("follow");
        assert_eq!(driver.activated.len(), 1);
        assert_eq!(driver.activated[0].0, r.as_str());
        assert!(!driver.activated[0].1);
    }

    #[test]
    fn f_and_f_new_tab_differ() {
        let d1 = link_doc("1", "https://example.com/", "home", "/x");
        let r = d1.semantic_refs().into_iter().next().unwrap();
        let mut driver = FakePageDriver::new(vec![d1.clone()]);
        let mut ctl = Controller::new();
        ctl.state.publish_page(d1);
        ctl.follow_link(&r, true).expect("schedule new tab");
        render_loading_and_run(&mut ctl, &mut driver).expect("new tab");
        assert!(driver.activated[0].1);
        assert_eq!(driver.open_tabs.len(), 1);
    }

    #[test]
    fn stale_ref_follow_fails_closed() {
        let d1 = link_doc("1", "https://example.com/", "home", "/x");
        let d2 = link_doc("2", "https://example.com/", "other", "/y");
        let stale = d1.semantic_refs().into_iter().next().unwrap();
        let mut ctl = Controller::new();
        ctl.state.publish_page(d2);
        let driver = FakePageDriver::new(vec![]);
        let err = ctl.follow_link(&stale, false);
        assert!(err.is_err());
        assert!(driver.activated.is_empty());
    }

    #[test]
    fn browser_work_cannot_begin_before_loading_frame_acknowledgement() {
        let d1 = text_doc("1", "t", "one");
        let d2 = text_doc("2", "t", "two");
        let mut driver = FakePageDriver::new(vec![d1.clone(), d2]);
        let mut ctl = Controller::new();
        ctl.state.publish_page(d1);

        ctl.navigate_to("https://example.com/two");
        assert!(ctl.state.lifecycle.is_loading());
        assert!(ctl.has_pending_page_action());
        assert!(driver.navigate_calls.is_empty());
        let err = ctl
            .perform_pending_page_action(&mut driver)
            .expect_err("must reject");
        assert!(err.contains("Loading frame"));
        assert!(driver.navigate_calls.is_empty());
        assert!(matches!(ctl.state.lifecycle, Lifecycle::Error { .. }));
    }

    #[test]
    fn agent_attention_scrolls_into_view_without_mutating_selection() {
        // Build a tall document so attention can force a scroll away from selection.
        let mut roots = Vec::new();
        for index in 0..40 {
            roots.push(RawSemanticNode {
                kind: "text".into(),
                tag: Some("p".into()),
                id: Some(format!("line-{index}")),
                unique_id: true,
                selector: None,
                text: Some(format!("line {index}")),
                href: None,
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
            });
        }
        let document = normalize_fixture(meta("1", "https://example.com/"), roots).expect("doc");
        let refs = document.semantic_refs();
        let selection = refs.first().cloned().expect("first");
        let attention = refs.last().cloned().expect("last");

        let mut ctl = Controller::new();
        ctl.shared.activate_runtime();
        ctl.state.publish_page(document.clone());
        ctl.shared.publish(document);
        ctl.state.view.selection = Some(selection.clone());
        ctl.state.view.viewport_height = 5;
        ctl.state.view.scroll_y = 0;
        let selection_before = ctl.state.view.selection.clone();
        let scroll_before = ctl.state.view.scroll_y;

        ctl.shared
            .set_attention(attention.clone(), Some("look here".into()))
            .expect("attention");
        ctl.synchronize_companion_state();

        assert_eq!(ctl.state.view.selection, selection_before);
        assert_eq!(ctl.state.view.attention.as_ref(), Some(&attention));
        assert!(
            ctl.state.view.scroll_y > scroll_before,
            "attention should scroll the spotlight into view"
        );
    }
}

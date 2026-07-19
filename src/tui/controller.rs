//! Lifecycle controller: Loading → browser action → settle → capture → atomic Ready | Error.
//!
//! Owns local [`TuiState`] (viewport, selection, modes) while coordinating page
//! actions with [`SharedTuiState`] so the companion and terminal share one
//! Loading lock and one published SemanticDocument. Browser work is always
//! deferred until the event loop has drawn a Loading frame.

use crate::semantic::{FragmentResolution, SemanticDocument, SemanticRef};
use crate::tui::content::{
    build_content_lines_with, first_visible_descendant_ref, focusable_refs, form_control_refs,
    line_index_of, rendered_block_text, search_refs, subtree_refs, top_viewport_anchor,
    ContentProjection, ViewportTopAnchor,
};
use crate::tui::driver::PageDriver;
use crate::tui::hints::{HintMatch, LinkHint, assign_hints, match_hint};
use crate::tui::shared::SharedTuiState;
use crate::tui::state::{HintMode, InputKind, InteractionMode, Lifecycle, TuiState};

/// Rows kept above a `#fragment` target when scrolling it into view.
const FRAGMENT_VIEWPORT_TOP_MARGIN: usize = 5;

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
    /// Write every staged field value into the live DOM, then click the submit control.
    SubmitForm {
        document: SemanticDocument,
        /// Submit button / `input type=submit` that is clicked after fields are written.
        semantic_ref: SemanticRef,
        /// All staged field values to apply before the click.
        values: Vec<(SemanticRef, String)>,
    },
    Activate {
        document: SemanticDocument,
        semantic_ref: SemanticRef,
    },
}

/// Result of performing a deferred page operation.
enum PageActionOutcome {
    /// Fresh semantic capture to publish (navigation, form submit, real navigation).
    Captured(Box<SemanticDocument>),
    /// Same-document activation: recapture DOM. Scroll policy depends on whether
    /// this was an in-page `#` jump vs a plain in-page click (clipboard copy).
    Patched {
        document: Box<SemanticDocument>,
        top_anchor: Option<ViewportTopAnchor>,
        previous_selection: Option<SemanticRef>,
        /// When set, jump to this fragment after publish (from link `href` or
        /// post-click URL). When `None`, pin the top viewport line.
        jump_fragment: Option<String>,
    },
    /// Focus-only / no-op: keep the published page without capturing.
    Retained,
}

/// Compare URLs for same-document activate short-circuit (ignore trailing slash / fragment).
fn urls_match_for_activate(a: &str, b: &str) -> bool {
    fn normalize(url: &str) -> String {
        let without_fragment = url.split('#').next().unwrap_or(url);
        without_fragment.trim_end_matches('/').to_string()
    }
    normalize(a) == normalize(b)
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
        // `f` (current tab) leaves Hint mode after the follow settles — the
        // destination page should start in Normal. `F` (new tab) keeps Hint
        // mode so multiple background opens can be chained without re-pressing F.
        let hint_after = if new_tab {
            Some(HintMode::NewTab)
        } else {
            None
        };
        self.queue_page_action(
            action,
            PageOperation::Follow {
                document,
                semantic_ref: semantic_ref.clone(),
                new_tab,
            },
            hint_after,
        );
        Ok(())
    }

    /// Stash the current form field buffer without writing to the browser.
    ///
    /// Called when Tab leaves a field so multi-field values survive until Enter.
    pub fn stash_form_field(&mut self, semantic_ref: &SemanticRef, text: &str) {
        self.state
            .view
            .pending_form_values
            .insert(semantic_ref.clone(), text.to_string());
    }

    /// Commit the active text-field buffer into staged values and leave edit mode.
    ///
    /// Does **not** submit the form. Submission is only via Enter on a submit
    /// control ([`Self::activate_selection`] when the selection is a submit).
    pub fn commit_form_field(&mut self, semantic_ref: &SemanticRef, text: &str) {
        self.stash_form_field(semantic_ref, text);
        self.state.mode = InteractionMode::Normal;
        self.state.view.set_status("field saved (Enter on submit to send)");
    }

    /// Queue form submission: write all staged field values, click submit, recapture.
    ///
    /// `submit_ref` must be a submit button / `input type=submit`. Forms without
    /// an explicit submit control are not supported.
    pub fn submit_form_via_button(&mut self, submit_ref: &SemanticRef) -> Result<(), String> {
        let document = self
            .state
            .document()
            .cloned()
            .ok_or_else(|| "no document".to_string())?;
        let component = document
            .resolve(submit_ref)
            .map_err(|e| e.to_string())?;
        if !is_submit_control(component) {
            return Err("form submit requires a submit button".into());
        }
        let values: Vec<(SemanticRef, String)> = self
            .state
            .view
            .pending_form_values
            .iter()
            .map(|(r, v)| (r.clone(), v.clone()))
            .collect();
        self.queue_page_action(
            "form_submit",
            PageOperation::SubmitForm {
                document,
                semantic_ref: submit_ref.clone(),
                values,
            },
            None,
        );
        Ok(())
    }

    /// Queue activation of the currently selected exact focusable ref.
    ///
    /// Submit buttons flush staged field values then click. Other controls
    /// activate without `requestSubmit` from text fields.
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
        // Explicit submit control: write staged fields, then click the button.
        if is_submit_control(component) {
            return self.submit_form_via_button(&semantic_ref);
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
                            let wrap = self.state.view.wrap;
                            let projection = self.state.view.projection;
                            let width = self.state.view.viewport_width.max(1);
                            let content_line_of =
                                move |document: &SemanticDocument, r: &SemanticRef| {
                                    let lines = build_content_lines_with(
                                        document,
                                        &collapsed,
                                        projection,
                                    );
                                    let lines = if wrap {
                                        crate::tui::content::wrap_content_lines(&lines, width)
                                    } else {
                                        lines
                                    };
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
    ///
    /// In prose mode landmarks/lists often have no content line. Attention still
    /// paints the whole subtree and scrolls to the first visible descendant.
    fn sync_attention_from_shared(&mut self) {
        let attention = self.shared.attention();
        let next = attention.semantic_ref.clone();
        let changed = self.state.view.attention != next;
        let previous_selection = self.state.view.selection.clone();
        self.state.view.attention = next.clone();

        // Rebuild paint set every sync so projection/collapse changes stay correct.
        self.state.view.attention_paint.clear();
        if let (Some(reference), Some(document)) = (next.as_ref(), self.state.document()) {
            self.state.view.attention_paint = subtree_refs(document, reference);
        }

        if !changed {
            // Still restore selection in case a prior path touched it (defensive).
            self.state.view.selection = previous_selection;
            return;
        }

        let Some(reference) = next else {
            self.state.view.set_status("attention cleared");
            self.state.view.selection = previous_selection;
            return;
        };

        let lines = self.content_lines();
        let scroll_target = line_index_of(&lines, &reference).or_else(|| {
            let document = self.state.document()?;
            let desc = first_visible_descendant_ref(document, &reference, &lines)?;
            line_index_of(&lines, &desc)
        });
        if let Some(line_idx) = scroll_target {
            // Top-ish placement so the spotlight is obvious, not just barely on-screen.
            self.scroll_line_with_top_margin(line_idx, lines.len(), 5);
        }

        if let Some(message) = attention.message.filter(|m| !m.is_empty()) {
            self.state.view.set_status(format!("attention: {message}"));
        } else {
            self.state.view.set_status("attention set");
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

        let result = (|| -> Result<Option<PageActionOutcome>, String> {
            match pending.operation {
                PageOperation::Bootstrap => {}
                PageOperation::Navigate(url) => {
                    // Empty session (no open tabs): open a tab at the URL instead
                    // of navigating a missing active page. Matches "o → type URL"
                    // recovery after the last tab was closed.
                    if !driver.has_open_tabs().map_err(|e| e.to_string())? {
                        driver.open_tab(&url).map_err(|e| e.to_string())?;
                    } else {
                        driver.navigate(&url).map_err(|e| e.to_string())?;
                    }
                }
                PageOperation::HistoryBack => driver.go_back().map_err(|e| e.to_string())?,
                PageOperation::HistoryForward => driver.go_forward().map_err(|e| e.to_string())?,
                PageOperation::Reload => driver.reload().map_err(|e| e.to_string())?,
                PageOperation::NextTab => driver.next_tab().map_err(|e| e.to_string())?,
                PageOperation::PrevTab => driver.prev_tab().map_err(|e| e.to_string())?,
                PageOperation::CloseTab => {
                    driver.close_active_tab().map_err(|e| e.to_string())?;
                    if !driver.has_open_tabs().map_err(|e| e.to_string())? {
                        // Last tab closed: clear retained UI instead of recapturing
                        // a missing active page (which would Error and look stuck).
                        return Ok(None);
                    }
                }
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
                    values,
                } => {
                    // Write every staged field, then click the submit control
                    // (never requestSubmit from a text field).
                    for (field_ref, field_text) in &values {
                        driver
                            .set_control_value(&document, field_ref, field_text)
                            .map_err(|e| e.to_string())?;
                    }
                    let _ = driver
                        .activate_ref(&document, &semantic_ref, false)
                        .map_err(|e| e.to_string())?;
                }
                PageOperation::Activate {
                    document,
                    semantic_ref,
                } => {
                    // Snapshot identity + top viewport line before the click so
                    // JS-only actions (clipboard copy) can recapture without
                    // jumping the markdown view when the page did not navigate.
                    // Same-page `#fragment` links also look "same document" once
                    // the fragment is stripped — those must jump, not pin top.
                    let before_id = document.document.document_id.clone();
                    let before_url = document.document.url.clone();
                    // Prefer the link's own href fragment when present so we
                    // still jump if the post-click capture is slow to show #hash.
                    let link_fragment = document
                        .resolve(&semantic_ref)
                        .ok()
                        .and_then(link_href_fragment);
                    let top_anchor = {
                        let lines = self.lines_for_document(&document);
                        top_viewport_anchor(&lines, self.state.view.scroll_y)
                    };
                    let previous_selection = self.state.view.selection.clone();
                    let page_changing = driver
                        .activate_ref(&document, &semantic_ref, false)
                        .map_err(|e| e.to_string())?;
                    if !page_changing {
                        // Focus-only (text inputs): nothing to settle/capture.
                        return Ok(Some(PageActionOutcome::Retained));
                    }
                    let same_document = driver
                        .document_metadata()
                        .ok()
                        .is_some_and(|meta| {
                            meta.document_id == before_id
                                && urls_match_for_activate(&meta.url, &before_url)
                        });
                    if same_document {
                        // Same-doc patch path: settle + capture; scroll policy
                        // decided in finish_same_document_patch (fragment vs top).
                        driver.wait_settle().map_err(|e| e.to_string())?;
                        let doc = Self::capture_with_metadata_barrier(driver)?;
                        let jump_fragment = link_fragment
                            .or_else(|| url_fragment(&doc.document.url))
                            .filter(|f| !f.is_empty());
                        return Ok(Some(PageActionOutcome::Patched {
                            document: Box::new(doc),
                            top_anchor,
                            previous_selection,
                            jump_fragment,
                        }));
                    }
                }
            }
            driver.wait_settle().map_err(|e| e.to_string())?;
            let doc = Self::capture_with_metadata_barrier(driver)?;
            Ok(Some(PageActionOutcome::Captured(Box::new(doc))))
        })();
        match result {
            Ok(Some(PageActionOutcome::Captured(doc))) => {
                self.finish_capture(
                    *doc,
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
            Ok(Some(PageActionOutcome::Patched {
                document,
                top_anchor,
                previous_selection,
                jump_fragment,
            })) => {
                self.finish_same_document_patch(
                    *document,
                    top_anchor,
                    previous_selection,
                    jump_fragment,
                );
                self.refresh_history_availability(driver);
                Ok(())
            }
            Ok(Some(PageActionOutcome::Retained)) => {
                // Keep published page + selection; just leave Loading.
                self.state.lifecycle = Lifecycle::Ready;
                self.shared.finish_page_action_retained();
                self.state.view.set_status("activated");
                Ok(())
            }
            Ok(None) => {
                self.clear_empty_session();
                Ok(())
            }
            Err(msg) => {
                // Retain last valid page; never publish partial update as Ready.
                log::error!("tui page action '{}' failed: {msg}", pending.action);
                self.state.enter_error(&pending.action, msg.clone());
                self.shared
                    .fail_page_action(pending.action.clone(), msg.clone());
                Err(msg)
            }
        }
    }

    /// Browser has zero tabs: drop retained page so chrome/content clear and
    /// lifecycle stays Ready for recovery via `t` (blank tab) or `o` (URL → new tab).
    fn clear_empty_session(&mut self) {
        self.pending_page_action = None;
        self.hints.clear();
        self.state.clear_session();
        self.shared.clear_session();
    }

    /// Capture + post-capture metadata barrier with short retries.
    ///
    /// Shared by navigate-style finishes and same-document patches.
    fn capture_with_metadata_barrier<D: PageDriver>(
        driver: &mut D,
    ) -> Result<SemanticDocument, String> {
        // Capture first, then read metadata as the freshness barrier. A
        // hydrated page can mutate between settling and capture, so a
        // pre-capture metadata read would reject a valid later snapshot.
        // The post-capture metadata must instead describe exactly what we
        // captured before it can be published. Dynamic sites often keep
        // mutating briefly after settle — retry only the metadata barrier
        // (hard capture failures still fail closed immediately).
        const METADATA_RETRY_ATTEMPTS: usize = 4;
        let mut last_err = String::new();
        for attempt in 0..METADATA_RETRY_ATTEMPTS {
            if attempt > 0 {
                let _ = driver.wait_settle();
            }
            let doc = driver.capture_semantic().map_err(|e| e.to_string())?;
            let metadata = driver.document_metadata().map_err(|e| e.to_string())?;
            if metadata.document_id.is_empty()
                || metadata.revision.is_empty()
                || metadata.ready_state != "complete"
            {
                last_err = "browser did not provide stable complete document metadata".into();
                continue;
            }
            if !crate::semantic::capture_matches_document_metadata(&doc.document, &metadata) {
                last_err = "semantic capture metadata changed during publication".into();
                continue;
            }
            return Ok(doc);
        }
        Err(last_err)
    }

    /// Publish a same-document recapture.
    ///
    /// - If `jump_fragment` is set (in-page `#section` link): jump to that
    ///   target with the usual top margin — do **not** pin the old top.
    /// - Otherwise (clipboard copy, label toggles): pin the pre-click top
    ///   viewport line and never `ensure_visible(selection)`.
    fn finish_same_document_patch(
        &mut self,
        doc: SemanticDocument,
        top_anchor: Option<ViewportTopAnchor>,
        previous_selection: Option<SemanticRef>,
        jump_fragment: Option<String>,
    ) {
        let prev_scroll = self.state.view.scroll_y;

        // Rebind collapse / search against the new revision.
        let mut collapsed = std::collections::HashSet::new();
        for old in &self.state.view.collapsed {
            if let Ok(rebound) = doc.rebind_surviving(old) {
                collapsed.insert(rebound);
            }
        }
        let mut search_matches = Vec::new();
        for old in &self.state.view.search_matches {
            if let Ok(rebound) = doc.rebind_surviving(old) {
                search_matches.push(rebound);
            }
        }
        let search_index = self
            .state
            .view
            .search_index
            .min(search_matches.len().saturating_sub(1));

        // Selection: rebind if the identity survives; fragment path may replace it.
        let selection = previous_selection.and_then(|prev| doc.rebind_surviving(&prev).ok());

        // Attention: drop if it no longer resolves by identity.
        let attention = self
            .state
            .view
            .attention
            .as_ref()
            .and_then(|a| doc.rebind_surviving(a).ok());

        // Staged form values: rebind keys that still exist.
        let mut pending_form = std::collections::HashMap::new();
        for (old_ref, value) in self.state.view.pending_form_values.drain() {
            if let Ok(rebound) = doc.rebind_surviving(&old_ref) {
                pending_form.insert(rebound, value);
            }
        }

        self.state.view.collapsed = collapsed;
        self.state.view.search_matches = search_matches;
        self.state.view.search_index = search_index;
        self.state.view.selection = selection;
        self.state.view.attention = attention.clone();
        self.state.view.attention_paint.clear();
        self.state.view.pending_form_values = pending_form;
        self.state.view.inspect_text = None;
        self.state.view.inspect_title = None;
        self.hints.clear();

        self.state.publish_page(doc);

        // Rebuild attention paint without scrolling (sync_attention_from_shared
        // would move the viewport when attention is set).
        if let (Some(reference), Some(document)) =
            (attention.as_ref(), self.state.document())
        {
            self.state.view.attention_paint = subtree_refs(document, reference);
        }

        if let Some(fragment) = jump_fragment.filter(|f| !f.is_empty()) {
            // In-page `#` navigation: select + scroll to the fragment target.
            // Prefer the explicit fragment (from link href) over whatever the
            // capture URL happened to contain.
            if let Some(document) = self.state.document().cloned() {
                self.select_fragment_target(&document, &fragment);
            }
        } else {
            // Plain same-doc click: restore scroll from the top-of-viewport anchor.
            let lines = self
                .state
                .document()
                .map(|d| self.lines_for_document(d))
                .unwrap_or_default();
            let scroll_y = if let Some(anchor) = top_anchor.as_ref() {
                if let Some(document) = self.state.document() {
                    if let Ok(rebound) = document.rebind_surviving(&anchor.semantic_ref) {
                        if let Some(block_start) = line_index_of(&lines, &rebound) {
                            block_start.saturating_add(anchor.wrap_row)
                        } else {
                            prev_scroll
                        }
                    } else {
                        prev_scroll
                    }
                } else {
                    prev_scroll
                }
            } else {
                prev_scroll
            };
            self.state.view.scroll_y = scroll_y;
            self.state.clamp_scroll(lines.len());
            // Intentionally no ensure_visible(selection) — that would yank the view.
        }

        let document = self.state.document().expect("patch published").clone();
        let selection = self.state.view.selection.clone();
        let _ = self.shared.publish_with_selection(document, selection);
        self.refresh_inspect_panel();
    }

    fn finish_capture(
        &mut self,
        doc: SemanticDocument,
        prev_sel: Option<SemanticRef>,
        prev_scroll: usize,
        anchor_offset: usize,
    ) {
        let collapsed = self.state.view.collapsed.clone();
        let wrap = self.state.view.wrap;
        let projection = self.state.view.projection;
        let width = self.state.view.viewport_width.max(1);
        let content_line_of = move |document: &SemanticDocument, r: &SemanticRef| {
            let lines = build_content_lines_with(document, &collapsed, projection);
            let lines = if wrap {
                crate::tui::content::wrap_content_lines(&lines, width)
            } else {
                lines
            };
            line_index_of(&lines, r)
        };
        self.state.reconcile_after_capture(
            doc,
            prev_sel,
            prev_scroll,
            anchor_offset,
            content_line_of,
        );
        if let Some(document) = self.state.document().cloned() {
            // Prefer fragment target from the new URL when present. This sets
            // selection + scroll (top + margin). Do not call ensure_visible
            // afterward — that would pin the target to the bottom of the pane.
            if url_fragment(&document.document.url).is_some() {
                self.apply_fragment_from_url(&document);
            } else {
                let lines = self.lines_for_document(&document);
                self.state.clamp_scroll(lines.len());
                self.ensure_selection_visible_in_lines(&document, &lines);
                if let Some(sel) = self.state.view.selection.clone()
                    && let Some(idx) = line_index_of(&lines, &sel)
                {
                    self.ensure_visible(idx, lines.len());
                }
            }
        }
        // Drop stale inspect text; sticky follow rebuilds for the new selection.
        self.state.view.inspect_text = None;
        self.state.view.inspect_title = None;
        // Staged form values belonged to the previous capture's refs.
        self.state.view.clear_pending_form_values();
        self.hints.clear();
        let document = self.state.document().expect("capture published").clone();
        let selection = self.state.view.selection.clone();
        let _ = self.shared.publish_with_selection(document, selection);
        self.refresh_inspect_panel();
    }

    /// After a capture, if the document URL has a fragment, move selection to it.
    fn apply_fragment_from_url(&mut self, document: &SemanticDocument) {
        let Some(fragment) = url_fragment(&document.document.url) else {
            return;
        };
        self.select_fragment_target(document, &fragment);
    }

    /// Expand collapsed ancestors, select the fragment target, and scroll it into view.
    ///
    /// The target line is pinned near the **top** of the content viewport with
    /// [`FRAGMENT_VIEWPORT_TOP_MARGIN`] rows of prior context above it (not the
    /// bottom). If the exact ref has no display line (e.g. hidden landmark in
    /// prose mode), selection jumps to the first visible descendant line.
    fn select_fragment_target(&mut self, document: &SemanticDocument, fragment: &str) {
        match document.resolve_fragment(fragment) {
            FragmentResolution::Target(target) => {
                for ancestor in document.ancestor_refs(&target) {
                    self.state.view.collapsed.remove(&ancestor);
                }
                // Also expand the target itself if it was collapsed as a landmark/list.
                self.state.view.collapsed.remove(&target);

                let lines = self.lines_for_document(document);
                // Prefer the target's own line; otherwise the first visible
                // descendant (prose hides pure containers).
                let (select_ref, line_idx) =
                    if let Some(idx) = line_index_of(&lines, &target) {
                        (target, idx)
                    } else if let Some(desc) =
                        first_visible_descendant_ref(document, &target, &lines)
                    {
                        let idx = line_index_of(&lines, &desc).unwrap_or(0);
                        (desc, idx)
                    } else {
                        self.state
                            .view
                            .set_status("fragment target not represented");
                        return;
                    };

                self.state.view.selection = Some(select_ref);
                self.scroll_line_with_top_margin(
                    line_idx,
                    lines.len(),
                    FRAGMENT_VIEWPORT_TOP_MARGIN,
                );
            }
            FragmentResolution::Top => {
                self.state.view.scroll_y = 0;
                let lines = self.lines_for_document(document);
                if let Some(r) = lines.iter().find_map(|l| l.semantic_ref.clone()) {
                    self.state.view.selection = Some(r);
                }
            }
            FragmentResolution::NotFound => {
                self.state
                    .view
                    .set_status("fragment target not represented");
            }
        }
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
        let tab_position = driver.tab_position().unwrap_or(None);
        self.state.set_tab_position(tab_position);
    }

    fn anchor_offset_in_viewport(
        &self,
        document: &SemanticDocument,
        selection: Option<&SemanticRef>,
    ) -> usize {
        let Some(sel) = selection else {
            return 0;
        };
        let lines = self.lines_for_document(document);
        let Some(line) = line_index_of(&lines, sel) else {
            return 0;
        };
        line.saturating_sub(self.state.view.scroll_y)
    }

    /// Content lines for a document under the current projection + collapse + wrap.
    fn lines_for_document(
        &self,
        document: &SemanticDocument,
    ) -> Vec<crate::tui::content::ContentLine> {
        let (overlay, editing) = self.form_display_overlay();
        let lines = crate::tui::content::build_content_lines_with_overlay(
            document,
            &self.state.view.collapsed,
            self.state.view.projection,
            overlay.as_ref(),
            editing.as_ref(),
        );
        if self.state.view.wrap {
            crate::tui::content::wrap_content_lines(&lines, self.state.view.viewport_width.max(1))
        } else {
            lines
        }
    }

    /// Staged + active edit values for inline form display.
    fn form_display_overlay(
        &self,
    ) -> (
        Option<crate::tui::content::FormValueOverlay>,
        Option<SemanticRef>,
    ) {
        let mut map = self.state.view.pending_form_values.clone();
        let editing = if let InteractionMode::Input(InputKind::Form {
            semantic_ref,
            buffer,
        }) = &self.state.mode
        {
            map.insert(semantic_ref.clone(), buffer.clone());
            Some(semantic_ref.clone())
        } else {
            None
        };
        let overlay = if map.is_empty() { None } else { Some(map) };
        (overlay, editing)
    }

    /// If selection is missing or has no line in the current projection, pick a
    /// visible ref (first descendant under the old selection, else first line).
    fn ensure_selection_visible_in_lines(
        &mut self,
        document: &SemanticDocument,
        lines: &[crate::tui::content::ContentLine],
    ) {
        if let Some(sel) = self.state.view.selection.clone() {
            if line_index_of(lines, &sel).is_some() {
                return;
            }
            if let Some(next) = first_visible_descendant_ref(document, &sel, lines) {
                self.state.view.selection = Some(next);
                return;
            }
        }
        if self.state.view.selection.is_none()
            && let Some(first) = lines.iter().find_map(|l| l.semantic_ref.clone())
        {
            self.state.view.selection = Some(first);
        }
    }
}

/// Fragment portion of a URL without the leading `#`, if any.
fn url_fragment(url: &str) -> Option<String> {
    let hash = url.find('#')?;
    let frag = &url[hash + 1..];
    if frag.is_empty() {
        None
    } else {
        Some(frag.to_string())
    }
}

/// Fragment from a link component's `href` (`#sec` or `…#sec`), if any.
fn link_href_fragment(component: &crate::semantic::SemanticComponent) -> Option<String> {
    use crate::semantic::SemanticKind;
    if component.kind != SemanticKind::Link {
        return None;
    }
    url_fragment(component.attrs.href.as_deref()?)
}

// Re-open Controller impl block for pure view operations.
impl Controller {

    // --- Pure view operations (no driver) ---

    /// Flatten the active SemanticDocument into addressable content lines.
    ///
    /// When word-wrap is enabled, long lines are soft-wrapped to the current
    /// viewport width so scroll, selection, search, and hints share one list.
    pub fn content_lines(&self) -> Vec<crate::tui::content::ContentLine> {
        match self.state.document() {
            Some(doc) => self.lines_for_document(doc),
            None => Vec::new(),
        }
    }

    /// Toggle soft word-wrap. On by default; enabling clears horizontal pan.
    pub fn toggle_wrap(&mut self) {
        self.state.view.wrap = !self.state.view.wrap;
        if self.state.view.wrap {
            self.state.view.scroll_x = 0;
            self.state.view.set_status("wrap: on");
        } else {
            self.state.view.set_status("wrap: off");
        }
        self.after_projection_change();
    }

    /// Toggle prose (default) vs structure content projection.
    ///
    /// Prose hides landmark/list/group chrome and flattens indent. Structure is
    /// the DOM-like outline. When leaving structure, selection jumps to the
    /// first visible descendant if the current ref has no prose line.
    pub fn toggle_structure(&mut self) {
        self.state.view.projection = match self.state.view.projection {
            ContentProjection::Prose => ContentProjection::Structure,
            ContentProjection::Structure => ContentProjection::Prose,
        };
        match self.state.view.projection {
            ContentProjection::Prose => self.state.view.set_status("mode: prose"),
            ContentProjection::Structure => self.state.view.set_status("mode: structure"),
        }
        self.after_projection_change();
    }

    fn after_projection_change(&mut self) {
        let Some(document) = self.state.document().cloned() else {
            return;
        };
        let lines = self.lines_for_document(&document);
        self.state.clamp_scroll(lines.len());
        self.ensure_selection_visible_in_lines(&document, &lines);
        if let Some(sel) = self.state.view.selection.clone()
            && let Some(idx) = line_index_of(&lines, &sel)
        {
            self.ensure_visible(idx, lines.len());
        }
        self.refresh_inspect_panel();
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
        // One stop per addressable component. Soft-wrap continuations share the
        // same semantic_ref; including every row would trap j/k on the first
        // continuation because line_index_of always returns the block start.
        let mut seen = std::collections::HashSet::new();
        let positions: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter_map(|(i, l)| {
                let r = l.semantic_ref.as_ref()?;
                if seen.insert(r.clone()) {
                    Some(i)
                } else {
                    None
                }
            })
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
        self.refresh_inspect_panel();
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

    /// Move selection down by about half a viewport of addressable stops (`Ctrl-d`).
    pub fn page_select_down(&mut self) {
        let steps = self.page_select_step();
        self.move_selection(steps as isize);
    }

    /// Move selection up by about half a viewport of addressable stops (`Ctrl-u`).
    pub fn page_select_up(&mut self) {
        let steps = self.page_select_step();
        self.move_selection(-(steps as isize));
    }

    fn page_select_step(&self) -> usize {
        // At least one stop so Ctrl-d/u always move when possible.
        (self.state.view.viewport_height.max(1) / 2).max(1)
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
    ///
    /// No-op while word-wrap is on (lines already fit the viewport).
    pub fn scroll_left(&mut self) {
        if self.state.view.wrap {
            return;
        }
        self.state.view.scroll_x = self.state.view.scroll_x.saturating_sub(4);
    }

    /// Horizontal pan right when a content line overflows the viewport width.
    ///
    /// No-op while word-wrap is on (lines already fit the viewport).
    pub fn scroll_right(&mut self) {
        if self.state.view.wrap {
            return;
        }
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

    /// Place `line_idx` `margin` rows below the **top** of the viewport.
    ///
    /// Used for `#fragment` jumps so the anchor sits near the top with a little
    /// prior context above it. Near the document end, [`TuiState::clamp_scroll`]
    /// may leave the target lower in the viewport; we never pin it to the bottom
    /// via [`Self::ensure_visible`] (that path is for j/k only).
    fn scroll_line_with_top_margin(&mut self, line_idx: usize, content_len: usize, margin: usize) {
        // Prefer: scroll_y + margin == line_idx  →  target at row `margin` from top.
        self.state.view.scroll_y = line_idx.saturating_sub(margin);
        self.state.clamp_scroll(content_len);
        // If the viewport is shorter than the margin, still keep the target in view
        // by scrolling it to the top rather than the bottom.
        let vh = self.state.view.viewport_height.max(1);
        if line_idx < self.state.view.scroll_y {
            self.state.view.scroll_y = line_idx;
            self.state.clamp_scroll(content_len);
        } else if line_idx >= self.state.view.scroll_y + vh {
            // Target fell below the viewport after clamp (should be rare); pull
            // it to the top of the viewport, not the bottom.
            self.state.view.scroll_y = line_idx;
            self.state.clamp_scroll(content_len);
        }
    }

    /// Collapse or expand the selected block by exact `semantic_ref` only (never rebind).
    ///
    /// Structure mode only: prose hides containers, so collapse is unavailable.
    pub fn toggle_collapse(&mut self) {
        if self.state.view.projection.is_prose() {
            self.state
                .view
                .set_status("collapse needs structure mode (zs)");
            return;
        }
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
    ///
    /// Sticky until Escape: while `inspect_follow` is set, selection changes
    /// (j/k, tab, search, …) refresh the panel under the new target.
    pub fn inspect_selection(&mut self) {
        self.state.view.inspect_follow = true;
        self.refresh_inspect_panel();
    }

    /// Rebuild inspect text for the current selection when follow mode is on.
    fn refresh_inspect_panel(&mut self) {
        if !self.state.view.inspect_follow {
            return;
        }
        let Some(doc) = self.state.document() else {
            self.state.view.inspect_text = None;
            self.state.view.inspect_title = None;
            return;
        };
        let Some(sel) = &self.state.view.selection else {
            self.state.view.inspect_text = Some("(no selection)".into());
            self.state.view.inspect_title = None;
            return;
        };
        match doc.resolve(sel) {
            Ok(c) => {
                let text = crate::tui::content::format_inspect_panel(
                    c,
                    doc.document.revision.as_str(),
                );
                let title = crate::tui::content::format_inspect_dom_path(doc, c);
                self.state.view.inspect_text = Some(text);
                self.state.view.inspect_title = Some(title);
            }
            Err(e) => {
                self.state.view.inspect_text =
                    Some(format!("inspect failed: {e}"));
                self.state.view.inspect_title = None;
            }
        }
    }

    /// Last content-line index belonging to `semantic_ref` (for wrap continuations).
    pub fn last_line_index_of(
        lines: &[crate::tui::content::ContentLine],
        semantic_ref: &SemanticRef,
    ) -> Option<usize> {
        lines.iter().enumerate().rev().find_map(|(i, l)| {
            l.semantic_ref
                .as_ref()
                .filter(|r| *r == semantic_ref)
                .map(|_| i)
        })
    }

    /// Clipboard payload for `y`: link/image URL when selected, else rendered block text.
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

    /// Enter URL-bar input with an empty buffer (`o` — type a fresh location).
    pub fn enter_url_input(&mut self) {
        if self.state.lifecycle.is_loading() {
            return;
        }
        // Empty buffer so typed characters replace rather than append to the
        // current address (see also `enter_url_edit` for prefilled editing).
        self.state.mode = InteractionMode::Input(InputKind::Url {
            buffer: String::new(),
        });
    }

    /// Enter URL-bar input prefilled with the current page URL (`O`).
    ///
    /// The buffer ends at the last character so further typing extends the
    /// address (backspace edits from the end). Empty when there is no page yet.
    pub fn enter_url_edit(&mut self) {
        if self.state.lifecycle.is_loading() {
            return;
        }
        let buffer = self.state.url().to_string();
        self.state.mode = InteractionMode::Input(InputKind::Url { buffer });
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
        // Match count lives in the footer (`/{query}  n/m`); clear stale status.
        if self
            .state
            .view
            .status_message
            .as_deref()
            .is_some_and(|m| m.starts_with("search:"))
        {
            self.state.view.clear_status();
        }
        self.refresh_inspect_panel();
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

    /// Start editing the current selection when it is a text-like form control.
    ///
    /// Returns `true` when form-input mode was entered. Checkboxes, radios,
    /// buttons, selects, and non-controls return `false` so callers can
    /// activate / cycle instead.
    pub fn edit_selection_if_form(&mut self) -> bool {
        let Some(sel) = self.state.view.selection.clone() else {
            return false;
        };
        let Some(doc) = self.state.document() else {
            return false;
        };
        let Ok(component) = doc.resolve(&sel) else {
            return false;
        };
        if !is_text_editable_control(component) {
            return false;
        }
        self.begin_form_edit(sel);
        true
    }

    /// Cycle a selected `<select>` through its options (local staged value).
    ///
    /// Returns `true` when the selection was a select and the staged value moved.
    /// Enter after cycling still goes through form submit / button activate.
    pub fn cycle_select_if_selected(&mut self) -> bool {
        let Some(sel) = self.state.view.selection.clone() else {
            return false;
        };
        let (options, captured_value) = {
            let Some(doc) = self.state.document() else {
                return false;
            };
            let Ok(component) = doc.resolve(&sel) else {
                return false;
            };
            if component.kind != crate::semantic::SemanticKind::Select {
                return false;
            }
            let options: Vec<(String, String)> = component
                .attrs
                .options
                .iter()
                .map(|o| {
                    (
                        o.value.clone(),
                        o.label
                            .clone()
                            .filter(|l| !l.is_empty())
                            .unwrap_or_else(|| o.value.clone()),
                    )
                })
                .collect();
            (options, component.attrs.value.clone())
        };
        if options.is_empty() {
            self.state.view.set_status("select has no options");
            return true;
        }
        let current = self
            .state
            .view
            .pending_form_values
            .get(&sel)
            .cloned()
            .or(captured_value)
            .unwrap_or_default();
        let idx = options
            .iter()
            .position(|(v, _)| v == &current)
            .unwrap_or(usize::MAX);
        let next = if idx == usize::MAX {
            0
        } else {
            (idx + 1) % options.len()
        };
        let (next_val, label) = options[next].clone();
        self.state.view.pending_form_values.insert(sel, next_val);
        self.state.view.set_status(format!("select: {label}"));
        true
    }

    /// Cycle focusable controls (Tab / Shift-Tab); form fields enter edit mode.
    ///
    /// Leaving a form field stashes its buffer so multi-field values survive
    /// until Enter on a submit button sends the form. Leaving a
    /// field for a non-form control clears Input mode so Enter cannot submit
    /// the previously focused input while a link or button is selected.
    pub fn tab_focus(&mut self, forward: bool) {
        // Leaving a form field: stash before any other mutation.
        let leaving_form = if let InteractionMode::Input(InputKind::Form {
            semantic_ref,
            buffer,
        }) = &self.state.mode
        {
            Some((semantic_ref.clone(), buffer.clone()))
        } else {
            None
        };
        if let Some((ref_left, text_left)) = leaving_form {
            self.stash_form_field(&ref_left, &text_left);
        }

        let (next_ref, next_is_form) = {
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
            let is_form = doc.resolve(&r).ok().is_some_and(|c| {
                use crate::semantic::SemanticKind;
                matches!(
                    c.kind,
                    SemanticKind::Input | SemanticKind::Textarea | SemanticKind::Select
                )
            });
            (r, is_form)
        };

        if next_is_form {
            self.begin_form_edit(next_ref);
            return;
        }
        self.state.view.selection = Some(next_ref.clone());
        self.state.mode = InteractionMode::Normal;
        let lines = self.content_lines();
        if let Some(idx) = line_index_of(&lines, &next_ref) {
            self.ensure_visible(idx, lines.len());
        }
        self.refresh_inspect_panel();
    }

    fn begin_form_edit(&mut self, semantic_ref: SemanticRef) {
        // Prefer a value already staged this capture; else the captured attr.
        let buffer = self
            .state
            .view
            .pending_form_values
            .get(&semantic_ref)
            .cloned()
            .or_else(|| {
                self.state
                    .document()
                    .and_then(|d| d.resolve(&semantic_ref).ok())
                    .and_then(|c| c.attrs.value.clone())
            })
            .unwrap_or_default();
        self.state.view.selection = Some(semantic_ref.clone());
        self.state.mode = InteractionMode::Input(InputKind::Form {
            semantic_ref,
            buffer,
        });
        self.refresh_inspect_panel();
    }

    /// Enter two-key hint mode over viewport-visible links (`f` / `F`).
    ///
    /// Labels are deterministic for the current scroll window. After a
    /// successful current-tab follow (`f`), mode returns to Normal. New-tab
    /// follows (`F`) may re-enter Hint mode for chaining until Escape.
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

    /// Leave Input/Hint/inspect overlays and return to Normal mode.
    ///
    /// When lifecycle is Error, also dismiss Error → Ready (local + shared) so
    /// the retained page becomes interactive again. Loading is never cleared
    /// here; only Escape after a failed page action recovers the session.
    pub fn escape(&mut self) {
        if self.state.clear_error() {
            let _ = self.shared.clear_error();
        } else {
            self.state.mode = InteractionMode::Normal;
            self.state.view.hint_buffer.clear();
            self.state.view.inspect_text = None;
            self.state.view.inspect_title = None;
            self.state.view.inspect_follow = false;
            // Discard unstaged form typing without writing to the page.
            self.state.view.clear_pending_form_values();
        }
        self.hints.clear();
    }

    /// Update content-area dimensions after a terminal resize and clamp scroll.
    ///
    /// When word-wrap is on, width changes rewrite the line grid, so the current
    /// selection is re-scrolled into view after clamping.
    pub fn set_viewport(&mut self, width: usize, height: usize) {
        self.state.view.viewport_width = width;
        self.state.view.viewport_height = height;
        let lines = self.content_lines();
        self.state.clamp_scroll(lines.len());
        if self.state.view.wrap
            && let Some(sel) = self.state.view.selection.as_ref()
            && let Some(idx) = line_index_of(&lines, sel)
        {
            self.ensure_visible(idx, lines.len());
        }
    }
}

/// Explicit submit controls (form send only happens through these).
fn is_submit_control(component: &crate::semantic::SemanticComponent) -> bool {
    use crate::semantic::SemanticKind;
    match component.kind {
        SemanticKind::Button => {
            let t = component
                .attrs
                .button_type
                .as_deref()
                .unwrap_or("submit")
                .to_ascii_lowercase();
            // HTML default for <button> is submit.
            t == "submit" || t.is_empty()
        }
        SemanticKind::Input => {
            let t = component
                .attrs
                .input_type
                .as_deref()
                .unwrap_or("text")
                .to_ascii_lowercase();
            t == "submit" || t == "image"
        }
        _ => false,
    }
}

/// Text-like controls that open form-input mode.
///
/// Checkboxes/radios toggle via activate; selects cycle via
/// [`Controller::cycle_select_if_selected`]; submit buttons activate/send.
fn is_text_editable_control(component: &crate::semantic::SemanticComponent) -> bool {
    use crate::semantic::SemanticKind;
    match component.kind {
        SemanticKind::Textarea => true,
        SemanticKind::Input => {
            let t = component
                .attrs
                .input_type
                .as_deref()
                .unwrap_or("text")
                .to_ascii_lowercase();
            !matches!(
                t.as_str(),
                "checkbox"
                    | "radio"
                    | "submit"
                    | "button"
                    | "reset"
                    | "image"
                    | "file"
                    | "hidden"
                    | "range"
                    | "color"
            )
        }
        // Select uses Enter to cycle options, not free-text edit.
        SemanticKind::Select => false,
        _ => false,
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
    fn edit_url_prefills_current_address() {
        let mut ctl = Controller::new();
        ctl.state
            .publish_page(text_doc("1", "t", "one"));
        assert_eq!(ctl.state.url(), "https://example.com/");
        ctl.enter_url_edit();
        match &ctl.state.mode {
            InteractionMode::Input(InputKind::Url { buffer }) => {
                assert_eq!(buffer, "https://example.com/");
            }
            other => panic!("expected URL input, got {other:?}"),
        }
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
    fn finish_capture_selects_fragment_target_from_url() {
        let heading = normalize_fixture(
            meta("1", "https://example.com/page#sec"),
            vec![RawSemanticNode {
                kind: "heading".into(),
                tag: Some("h2".into()),
                id: Some("sec".into()),
                unique_id: true,
                selector: None,
                text: Some("Section".into()),
                href: None,
                landmark: None,
                heading_level: Some(2),
                ordered: None,
                label: Some("Section".into()),
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
        .expect("doc");
        // Ensure element_id survived normalize
        let target = heading.semantic_refs()[0].clone();
        assert_eq!(
            heading.resolve_fragment("sec"),
            FragmentResolution::Target(target.clone())
        );

        let mut ctl = Controller::new();
        ctl.set_viewport(80, 20);
        // Seed a different page first so finish_capture path is exercised via publish.
        ctl.state.publish_page(text_doc("0", "prev", "before"));
        ctl.state.view.selection = ctl
            .state
            .document()
            .unwrap()
            .semantic_refs()
            .into_iter()
            .next();
        ctl.state.view.scroll_y = 5;

        // Simulate finish_capture body: reconcile then apply fragment.
        let collapsed = ctl.state.view.collapsed.clone();
        let content_line_of = move |document: &SemanticDocument, r: &SemanticRef| {
            let lines = build_content_lines_with(
                document,
                &collapsed,
                ContentProjection::Prose,
            );
            line_index_of(&lines, r)
        };
        ctl.state.reconcile_after_capture(
            heading.clone(),
            ctl.state.view.selection.clone(),
            5,
            0,
            content_line_of,
        );
        ctl.apply_fragment_from_url(&heading);
        assert_eq!(ctl.state.view.selection.as_ref(), Some(&target));
        // Single-line doc: margin clamps to top.
        assert_eq!(ctl.state.view.scroll_y, 0);
    }

    #[test]
    fn fragment_target_scrolls_with_top_margin() {
        let mut nodes = Vec::new();
        for i in 0..20 {
            nodes.push(RawSemanticNode {
                kind: "text".into(),
                tag: Some("p".into()),
                id: Some(format!("p{i}")),
                unique_id: true,
                selector: None,
                text: Some(format!("paragraph {i}")),
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
        // Put the fragment target mid-document.
        nodes[12].id = Some("sec".into());
        let doc = normalize_fixture(meta("1", "https://example.com/page#sec"), nodes).expect("doc");
        let target = doc
            .components()
            .find(|c| c.attrs.element_id.as_deref() == Some("sec"))
            .expect("target")
            .semantic_ref
            .clone();
        let mut ctl = Controller::new();
        // Viewport shorter than content so a non-zero scroll offset is allowed.
        ctl.set_viewport(80, 10);
        ctl.state.publish_page(doc.clone());
        ctl.apply_fragment_from_url(&doc);
        assert_eq!(ctl.state.view.selection.as_ref(), Some(&target));
        let lines = ctl.content_lines();
        let idx = line_index_of(&lines, &target).expect("line");
        assert!(idx >= FRAGMENT_VIEWPORT_TOP_MARGIN);
        // Target sits `margin` rows below the top of the viewport.
        assert_eq!(
            ctl.state.view.scroll_y,
            idx.saturating_sub(FRAGMENT_VIEWPORT_TOP_MARGIN)
        );
        let offset_from_top = idx - ctl.state.view.scroll_y;
        assert_eq!(offset_from_top, FRAGMENT_VIEWPORT_TOP_MARGIN);
        // Must not be pinned to the bottom row of the viewport.
        assert!(offset_from_top + 1 < ctl.state.view.viewport_height);
    }

    #[test]
    fn fragment_on_hidden_container_selects_visible_descendant_near_top() {
        let doc = normalize_fixture(
            meta("1", "https://example.com/page#wrap"),
            vec![RawSemanticNode {
                kind: "landmark".into(),
                tag: Some("main".into()),
                id: Some("wrap".into()),
                unique_id: true,
                selector: None,
                text: None,
                href: None,
                landmark: Some("main".into()),
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
                children: {
                    let mut kids = Vec::new();
                    for i in 0..15 {
                        kids.push(RawSemanticNode {
                            kind: "text".into(),
                            tag: Some("p".into()),
                            id: Some(format!("p{i}")),
                            unique_id: true,
                            selector: None,
                            text: Some(format!("para {i}")),
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
                    kids
                },
            }],
        )
        .expect("doc");
        let first_child = doc.semantic_refs()[1].clone();
        let mut ctl = Controller::new();
        ctl.set_viewport(80, 10);
        // Default prose hides the landmark chrome.
        assert!(ctl.state.view.projection.is_prose());
        ctl.state.publish_page(doc.clone());
        ctl.apply_fragment_from_url(&doc);
        // Selection is the first visible descendant, not the hidden landmark.
        assert_eq!(ctl.state.view.selection.as_ref(), Some(&first_child));
        let lines = ctl.content_lines();
        let idx = line_index_of(&lines, &first_child).expect("line");
        assert_eq!(
            ctl.state.view.scroll_y,
            idx.saturating_sub(FRAGMENT_VIEWPORT_TOP_MARGIN)
        );
    }

    #[test]
    fn toggle_wrap_reflows_lines_and_disables_horizontal_pan() {
        let long = "word ".repeat(20);
        let doc = text_doc("1", "body", long.trim());
        let mut ctl = Controller::new();
        ctl.state.publish_page(doc);
        ctl.set_viewport(20, 10);
        // Wrap is on by default.
        assert!(ctl.state.view.wrap);
        let wrapped = ctl.content_lines().len();
        assert!(
            ctl.content_lines()
                .iter()
                .all(|l| l.text.chars().count() <= 20)
        );
        ctl.scroll_right();
        assert_eq!(ctl.state.view.scroll_x, 0, "h-pan disabled while wrapped");
        ctl.toggle_wrap();
        assert!(!ctl.state.view.wrap);
        assert_eq!(ctl.state.view.status_message.as_deref(), Some("wrap: off"));
        let unwrapped = ctl.content_lines().len();
        assert!(wrapped > unwrapped);
        ctl.state.view.scroll_x = 12;
        ctl.toggle_wrap();
        assert!(ctl.state.view.wrap);
        assert_eq!(ctl.state.view.scroll_x, 0);
        assert_eq!(ctl.state.view.status_message.as_deref(), Some("wrap: on"));
        assert_eq!(ctl.content_lines().len(), wrapped);
    }

    #[test]
    fn closing_last_tab_clears_ui_and_new_tab_recovers() {
        let d1 = text_doc("1", "only", "solo tab");
        let mut driver = FakePageDriver::new(vec![d1.clone()]);
        driver.open_tab_count = 1;
        let mut ctl = Controller::new();
        ctl.state.publish_page(d1);
        ctl.close_tab();
        render_loading_and_run(&mut ctl, &mut driver).expect("close last");
        assert!(ctl.state.page.is_none());
        assert!(ctl.state.lifecycle.is_ready());
        assert!(ctl.state.tab_position.is_none());
        assert_eq!(
            ctl.state.view.status_message.as_deref(),
            Some("no open tabs — press t or o to continue")
        );
        assert!(driver.tab_ops.contains(&"close"));

        ctl.new_tab();
        render_loading_and_run(&mut ctl, &mut driver).expect("new tab");
        assert!(ctl.state.lifecycle.is_ready());
        assert!(ctl.state.page.is_some());
        assert_eq!(ctl.state.url(), "about:blank");
        assert!(!driver.open_tabs.is_empty());
        assert_eq!(ctl.state.tab_position, Some((1, 1)));
    }

    #[test]
    fn capture_refreshes_tab_position_for_chrome() {
        let d1 = text_doc("1", "a", "first");
        let mut driver = FakePageDriver::new(vec![d1.clone()]);
        driver.open_tab_count = 5;
        driver.active_tab_index = 2;
        let mut ctl = Controller::new();
        ctl.state.publish_page(d1);
        ctl.reload();
        render_loading_and_run(&mut ctl, &mut driver).expect("reload");
        assert_eq!(ctl.state.tab_position, Some((2, 5)));
    }

    #[test]
    fn navigate_with_no_tabs_opens_tab_at_url() {
        let target = SemanticDocument::empty(meta("2", "https://example.com/recover")).unwrap();
        let mut driver = FakePageDriver::new(vec![target]);
        driver.open_tab_count = 0;
        driver.active_tab_index = 0;
        let mut ctl = Controller::new();
        // Empty session UI (last tab closed).
        ctl.state.clear_session();
        assert!(ctl.state.page.is_none());

        ctl.navigate_to("https://example.com/recover");
        render_loading_and_run(&mut ctl, &mut driver).expect("navigate empty");
        assert!(ctl.state.lifecycle.is_ready());
        assert_eq!(ctl.state.url(), "https://example.com/recover");
        assert_eq!(driver.open_tab_count, 1);
        assert_eq!(
            driver.open_tabs.last().map(String::as_str),
            Some("https://example.com/recover")
        );
        assert!(
            driver.navigate_calls.is_empty(),
            "should open_tab, not navigate on missing active page"
        );
        assert_eq!(ctl.state.tab_position, Some((1, 1)));
    }

    #[test]
    fn zs_toggles_prose_and_rebinds_hidden_container_selection() {
        let doc = normalize_fixture(
            meta("1", "https://example.com/"),
            vec![RawSemanticNode {
                kind: "landmark".into(),
                tag: Some("main".into()),
                id: None,
                unique_id: false,
                selector: None,
                text: None,
                href: None,
                landmark: Some("main".into()),
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
                children: vec![RawSemanticNode {
                    kind: "text".into(),
                    tag: Some("p".into()),
                    id: Some("p1".into()),
                    unique_id: true,
                    selector: None,
                    text: Some("hello".into()),
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
                }],
            }],
        )
        .expect("doc");
        let landmark = doc.semantic_refs()[0].clone();
        let text_ref = doc.semantic_refs()[1].clone();
        let mut ctl = Controller::new();
        ctl.state.publish_page(doc);
        ctl.set_viewport(80, 20);
        // Enter structure and select the landmark container.
        ctl.toggle_structure();
        assert!(ctl.state.view.projection.is_structure());
        ctl.state.view.selection = Some(landmark);
        assert!(
            ctl.content_lines()
                .iter()
                .any(|l| l.text.contains("[main]"))
        );
        // Back to prose: landmark chrome gone; selection jumps to child text.
        ctl.toggle_structure();
        assert!(ctl.state.view.projection.is_prose());
        assert_eq!(ctl.state.view.selection.as_ref(), Some(&text_ref));
        assert!(!ctl.content_lines().iter().any(|l| l.text.contains("[main]")));
        // Collapse disabled in prose.
        ctl.toggle_collapse();
        assert_eq!(
            ctl.state.view.status_message.as_deref(),
            Some("collapse needs structure mode (zs)")
        );
    }

    #[test]
    fn j_moves_past_wrapped_block_to_next_component() {
        let long = "word ".repeat(30);
        let doc = normalize_fixture(
            meta("1", "https://example.com/"),
            vec![
                RawSemanticNode {
                    kind: "text".into(),
                    tag: Some("p".into()),
                    id: Some("a".into()),
                    unique_id: true,
                    selector: None,
                    text: Some(long.trim().into()),
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
                },
                RawSemanticNode {
                    kind: "text".into(),
                    tag: Some("p".into()),
                    id: Some("b".into()),
                    unique_id: true,
                    selector: None,
                    text: Some("second".into()),
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
                },
            ],
        )
        .expect("doc");
        let refs = doc.semantic_refs();
        assert_eq!(refs.len(), 2);
        let mut ctl = Controller::new();
        ctl.state.publish_page(doc);
        ctl.set_viewport(12, 20);
        ctl.state.view.selection = Some(refs[0].clone());
        ctl.toggle_wrap();
        let wrapped = ctl.content_lines();
        assert!(
            wrapped
                .iter()
                .filter(|l| l.semantic_ref.as_ref() == Some(&refs[0]))
                .count()
                > 1,
            "first block must occupy multiple wrap rows"
        );
        // j must leave the wrapped first block, not stall on continuations.
        ctl.scroll_down();
        assert_eq!(ctl.state.view.selection.as_ref(), Some(&refs[1]));
        ctl.scroll_up();
        assert_eq!(ctl.state.view.selection.as_ref(), Some(&refs[0]));
    }

    #[test]
    fn ctrl_d_moves_selection_by_half_page() {
        let mut nodes = Vec::new();
        for i in 0..30 {
            nodes.push(raw_text(&format!("p{i}"), &format!("line {i}")));
        }
        let doc = normalize_fixture(meta("1", "https://example.com/"), nodes).expect("doc");
        let refs = doc.semantic_refs();
        let mut ctl = Controller::new();
        ctl.set_viewport(80, 10); // half page = 5 stops
        ctl.state.publish_page(doc);
        ctl.state.view.selection = Some(refs[0].clone());
        ctl.page_select_down();
        assert_eq!(ctl.state.view.selection.as_ref(), Some(&refs[5]));
        ctl.page_select_up();
        assert_eq!(ctl.state.view.selection.as_ref(), Some(&refs[0]));
        // Plain d only pans the view.
        let sel = ctl.state.view.selection.clone();
        let scroll_before = ctl.state.view.scroll_y;
        ctl.half_page_down();
        assert_eq!(ctl.state.view.selection, sel);
        assert!(ctl.state.view.scroll_y >= scroll_before);
    }

    #[test]
    fn inspect_follows_selection_until_escape() {
        let doc = normalize_fixture(
            meta("1", "https://example.com/"),
            vec![
                raw_text("a", "first"),
                raw_text("b", "second"),
            ],
        )
        .expect("doc");
        let refs = doc.semantic_refs();
        let mut ctl = Controller::new();
        ctl.set_viewport(80, 20);
        ctl.state.publish_page(doc);
        ctl.state.view.selection = Some(refs[0].clone());
        ctl.inspect_selection();
        assert!(ctl.state.view.inspect_follow);
        let first = ctl.state.view.inspect_text.clone().expect("inspect open");
        assert!(first.contains("\"first\"") || first.contains("first"), "{first}");
        assert!(first.contains("rev="), "{first}");
        assert!(!first.contains("Some("), "{first}");
        ctl.scroll_down();
        assert_eq!(ctl.state.view.selection.as_ref(), Some(&refs[1]));
        let second = ctl.state.view.inspect_text.clone().expect("still open");
        assert_ne!(first, second);
        assert!(second.contains("\"second\"") || second.contains("second"), "{second}");
        let title = ctl
            .state
            .view
            .inspect_title
            .clone()
            .expect("dom path title");
        assert!(!title.is_empty(), "{title}");
        ctl.escape();
        assert!(!ctl.state.view.inspect_follow);
        assert!(ctl.state.view.inspect_text.is_none());
        assert!(ctl.state.view.inspect_title.is_none());
    }

    #[test]
    fn follow_uses_exact_ref_only() {
        let d1 = link_doc("1", "https://example.com/", "home", "/next");
        let d2 = link_doc("2", "https://example.com/next", "home", "/");
        let r = d1.semantic_refs().into_iter().next().unwrap();
        let mut driver = FakePageDriver::new(vec![d1.clone(), d2]);
        let mut ctl = Controller::new();
        ctl.state.publish_page(d1);
        ctl.enter_hint_mode(HintMode::Follow);
        assert!(ctl.state.is_hint_mode());
        ctl.follow_link(&r, false).expect("schedule follow");
        render_loading_and_run(&mut ctl, &mut driver).expect("follow");
        assert_eq!(driver.activated.len(), 1);
        assert_eq!(driver.activated[0].0, r.as_str());
        assert!(!driver.activated[0].1);
        // `f` auto-closes hint mode after a successful follow.
        assert!(
            matches!(ctl.state.mode, InteractionMode::Normal),
            "expected Normal after f follow, got {:?}",
            ctl.state.mode
        );
    }

    #[test]
    fn f_and_f_new_tab_differ() {
        let d1 = link_doc("1", "https://example.com/", "home", "/x");
        let r = d1.semantic_refs().into_iter().next().unwrap();
        let mut driver = FakePageDriver::new(vec![d1.clone()]);
        let mut ctl = Controller::new();
        ctl.state.publish_page(d1);
        ctl.enter_hint_mode(HintMode::NewTab);
        ctl.follow_link(&r, true).expect("schedule new tab");
        render_loading_and_run(&mut ctl, &mut driver).expect("new tab");
        assert!(driver.activated[0].1);
        assert_eq!(driver.open_tabs.len(), 1);
        // `F` keeps hint mode for chaining.
        assert!(
            matches!(ctl.state.mode, InteractionMode::Hint(HintMode::NewTab)),
            "expected Hint(NewTab) after F follow, got {:?}",
            ctl.state.mode
        );
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
        assert!(ctl.state.view.attention_paint.contains(&attention));
        assert_eq!(
            ctl.state.view.status_message.as_deref(),
            Some("attention: look here")
        );
    }

    #[test]
    fn agent_attention_on_prose_hidden_landmark_paints_and_scrolls_descendants() {
        // Landmark has no prose line; attention must still reach the child heading.
        let mut roots = Vec::new();
        for index in 0..25 {
            roots.push(raw_text(&format!("pad-{index}"), &format!("pad {index}")));
        }
        roots.push(RawSemanticNode {
                    kind: "landmark".into(),
                    tag: Some("section".into()),
                    id: Some("block".into()),
                    unique_id: true,
                    selector: None,
                    text: None,
                    href: None,
                    landmark: Some("section".into()),
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
                    children: {
                        let mut kids = Vec::new();
                        for index in 0..30 {
                            kids.push(RawSemanticNode {
                                kind: "heading".into(),
                                tag: Some("h2".into()),
                                id: Some(format!("h-{index}")),
                                unique_id: true,
                                selector: None,
                                text: Some(format!("Heading {index}")),
                                href: None,
                                landmark: None,
                                heading_level: Some(2),
                                ordered: None,
                                label: Some(format!("Heading {index}")),
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
                        kids
                    },
                });
        let document = normalize_fixture(meta("1", "https://example.com/"), roots).expect("doc");
        let section = document
            .components()
            .find(|c| c.attrs.element_id.as_deref() == Some("block"))
            .expect("section")
            .semantic_ref
            .clone();
        let first_heading = document
            .components()
            .find(|c| c.attrs.element_id.as_deref() == Some("h-0"))
            .expect("first heading")
            .semantic_ref
            .clone();
        let pad0 = document
            .components()
            .find(|c| c.attrs.element_id.as_deref() == Some("pad-0"))
            .expect("pad")
            .semantic_ref
            .clone();

        let mut ctl = Controller::new();
        ctl.shared.activate_runtime();
        ctl.state.publish_page(document.clone());
        ctl.shared.publish(document);
        // Default projection is prose — landmark chrome hidden.
        assert!(ctl.state.view.projection.is_prose());
        ctl.state.view.selection = Some(pad0.clone());
        ctl.set_viewport(80, 6);
        ctl.state.view.scroll_y = 0;
        let scroll_before = ctl.state.view.scroll_y;

        // Prose lines must not include the section ref itself.
        let lines = ctl.content_lines();
        assert!(
            line_index_of(&lines, &section).is_none(),
            "section must be prose-hidden"
        );
        let heading_line = line_index_of(&lines, &first_heading).expect("heading line");
        assert!(heading_line > 10, "heading should sit below padding");

        ctl.shared
            .set_attention(section.clone(), Some("section spotlight".into()))
            .expect("attention");
        ctl.synchronize_companion_state();

        assert_eq!(ctl.state.view.selection.as_ref(), Some(&pad0));
        assert_eq!(ctl.state.view.attention.as_ref(), Some(&section));
        assert!(
            ctl.state.view.attention_paint.contains(&section),
            "paint set includes root"
        );
        assert!(
            ctl.state.view.attention_paint.contains(&first_heading),
            "paint set includes descendant heading"
        );
        assert!(
            ctl.state.view.scroll_y > scroll_before,
            "should scroll toward first visible descendant under section; scroll={} before={}",
            ctl.state.view.scroll_y,
            scroll_before
        );
        // Target should be near top of viewport (margin 5).
        assert!(
            ctl.state.view.scroll_y <= heading_line,
            "scroll {} should not pass heading {}",
            ctl.state.view.scroll_y,
            heading_line
        );
    }

    fn form_input(id: &str, name: &str, value: &str) -> RawSemanticNode {
        RawSemanticNode {
            kind: "input".into(),
            tag: Some("input".into()),
            id: Some(id.into()),
            unique_id: true,
            selector: None,
            text: None,
            href: None,
            landmark: None,
            heading_level: None,
            ordered: None,
            label: Some(name.into()),
            src: None,
            alt: None,
            name: Some(name.into()),
            value: Some(value.into()),
            input_type: Some("text".into()),
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

    #[test]
    fn enter_on_selected_input_starts_form_edit() {
        let document = normalize_fixture(
            meta("1", "https://example.com/form"),
            vec![form_input("email", "email", "pre")],
        )
        .expect("doc");
        let email = document
            .components()
            .find(|c| c.attrs.element_id.as_deref() == Some("email"))
            .unwrap()
            .semantic_ref
            .clone();
        let mut ctl = Controller::new();
        ctl.state.publish_page(document);
        ctl.state.view.selection = Some(email.clone());
        assert!(ctl.edit_selection_if_form());
        match &ctl.state.mode {
            InteractionMode::Input(InputKind::Form {
                semantic_ref,
                buffer,
            }) => {
                assert_eq!(semantic_ref, &email);
                assert_eq!(buffer, "pre");
            }
            other => panic!("expected form edit, got {other:?}"),
        }
    }

    #[test]
    fn multi_field_form_stashes_on_tab_and_submits_only_via_button() {
        let document = normalize_fixture(
            meta("1", "https://example.com/form"),
            vec![
                form_input("email", "email", ""),
                form_input("name", "name", ""),
                RawSemanticNode {
                    kind: "button".into(),
                    tag: Some("button".into()),
                    id: Some("go".into()),
                    unique_id: true,
                    selector: None,
                    text: Some("Send".into()),
                    href: None,
                    landmark: None,
                    heading_level: None,
                    ordered: None,
                    label: Some("Send".into()),
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
                    button_type: Some("submit".into()),
                    options: vec![],
                    children: vec![],
                },
            ],
        )
        .expect("doc");
        let email = document
            .components()
            .find(|c| c.attrs.element_id.as_deref() == Some("email"))
            .unwrap()
            .semantic_ref
            .clone();
        let name = document
            .components()
            .find(|c| c.attrs.element_id.as_deref() == Some("name"))
            .unwrap()
            .semantic_ref
            .clone();
        let submit = document
            .components()
            .find(|c| c.attrs.element_id.as_deref() == Some("go"))
            .unwrap()
            .semantic_ref
            .clone();

        let mut driver = FakePageDriver::new(vec![document.clone()]);
        let mut ctl = Controller::new();
        ctl.shared.activate_runtime();
        ctl.state.publish_page(document.clone());
        ctl.shared.publish(document);

        // Edit first field and Tab away — stash only.
        ctl.state.view.selection = Some(email.clone());
        ctl.begin_form_edit(email.clone());
        match &mut ctl.state.mode {
            InteractionMode::Input(InputKind::Form { buffer, .. }) => {
                *buffer = "a@b.co".into();
            }
            other => panic!("expected form input, got {other:?}"),
        }
        ctl.tab_focus(true);
        assert_eq!(
            ctl.state.view.pending_form_values.get(&email).map(String::as_str),
            Some("a@b.co")
        );

        // Enter on text field commits value, does NOT submit.
        match &mut ctl.state.mode {
            InteractionMode::Input(InputKind::Form { buffer, .. }) => {
                *buffer = "Ada".into();
            }
            other => panic!("expected second form field, got {other:?}"),
        }
        ctl.commit_form_field(&name, "Ada");
        assert!(matches!(ctl.state.mode, InteractionMode::Normal));
        assert!(!ctl.has_pending_page_action());
        assert_eq!(
            ctl.state.view.pending_form_values.get(&name).map(String::as_str),
            Some("Ada")
        );
        assert!(driver.filled.is_empty(), "no DOM write until submit button");

        // Enter on submit button: write all staged + click.
        ctl.state.view.selection = Some(submit.clone());
        ctl.activate_selection().expect("submit");
        assert!(ctl.state.lifecycle.is_loading());
        ctl.acknowledge_loading_frame();
        ctl.perform_pending_page_action(&mut driver).expect("run");

        assert!(
            driver.filled.iter().any(|(r, v)| r == email.as_str() && v == "a@b.co"),
            "email written on submit: {:?}",
            driver.filled
        );
        assert!(
            driver.filled.iter().any(|(r, v)| r == name.as_str() && v == "Ada"),
            "name written on submit: {:?}",
            driver.filled
        );
        assert!(
            driver.activated.iter().any(|(r, _)| r == submit.as_str()),
            "submit button clicked: {:?}",
            driver.activated
        );
        assert!(
            ctl.state.view.pending_form_values.is_empty(),
            "pending cleared after capture"
        );
    }

    fn copy_button_node(text: &str) -> RawSemanticNode {
        RawSemanticNode {
            kind: "button".into(),
            tag: Some("button".into()),
            id: Some("copy".into()),
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
            button_type: Some("button".into()),
            options: vec![],
            children: vec![],
        }
    }

    #[test]
    fn same_page_hash_link_jumps_to_fragment() {
        // Enter on `#sec` must scroll to the heading, not pin the old top line
        // (same-document patch path used to break this).
        let url = "https://example.com/docs";
        let mut nodes = Vec::new();
        for i in 0..10 {
            nodes.push(raw_text(&format!("p{i}"), &format!("paragraph {i}")));
        }
        nodes.push(RawSemanticNode {
            kind: "heading".into(),
            tag: Some("h2".into()),
            id: Some("sec".into()),
            unique_id: true,
            selector: None,
            text: Some("Target section".into()),
            href: None,
            landmark: None,
            heading_level: Some(2),
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
        nodes.push(RawSemanticNode {
            kind: "link".into(),
            tag: Some("a".into()),
            id: Some("toc".into()),
            unique_id: true,
            selector: None,
            text: Some("Jump".into()),
            href: Some("#sec".into()),
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
        let doc = normalize_fixture(meta("main:1", url), nodes).expect("doc");
        let heading = doc
            .components()
            .find(|c| c.attrs.element_id.as_deref() == Some("sec"))
            .unwrap()
            .semantic_ref
            .clone();
        let link = doc
            .components()
            .find(|c| c.attrs.element_id.as_deref() == Some("toc"))
            .unwrap()
            .semantic_ref
            .clone();

        // After click, same doc with fragment in URL (browser hash update).
        let mut after_meta = meta("main:2", url);
        after_meta.url = format!("{url}#sec");
        let after_nodes = {
            // Rebuild identical tree under new revision/url for capture.
            let mut n = Vec::new();
            for i in 0..10 {
                n.push(raw_text(&format!("p{i}"), &format!("paragraph {i}")));
            }
            n.push(RawSemanticNode {
                kind: "heading".into(),
                tag: Some("h2".into()),
                id: Some("sec".into()),
                unique_id: true,
                selector: None,
                text: Some("Target section".into()),
                href: None,
                landmark: None,
                heading_level: Some(2),
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
            n.push(RawSemanticNode {
                kind: "link".into(),
                tag: Some("a".into()),
                id: Some("toc".into()),
                unique_id: true,
                selector: None,
                text: Some("Jump".into()),
                href: Some("#sec".into()),
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
            n
        };
        let after = normalize_fixture(after_meta, after_nodes).expect("after");

        let mut driver = FakePageDriver::new(vec![doc.clone(), after]);
        driver.advance_page_on_capture = true;
        let mut ctl = Controller::new();
        ctl.shared.activate_runtime();
        ctl.state.publish_page(doc.clone());
        ctl.shared.publish(doc);
        ctl.set_viewport(40, 5);
        ctl.state.view.scroll_y = 0;
        ctl.state.view.selection = Some(link.clone());

        ctl.activate_selection().expect("activate hash link");
        ctl.acknowledge_loading_frame();
        ctl.perform_pending_page_action(&mut driver)
            .expect("run");

        assert!(ctl.state.lifecycle.is_ready());
        let heading_after = ctl
            .state
            .document()
            .unwrap()
            .rebind_surviving(&heading)
            .expect("heading survives");
        assert_eq!(
            ctl.state.view.selection.as_ref(),
            Some(&heading_after),
            "selection should move to fragment target"
        );
        let lines = ctl.content_lines();
        let heading_line = line_index_of(&lines, &heading_after).expect("heading line");
        // Top margin places the target a few rows below the top of the viewport.
        assert!(
            ctl.state.view.scroll_y <= heading_line,
            "scroll should be at or above heading"
        );
        assert!(
            heading_line < ctl.state.view.scroll_y + ctl.state.view.viewport_height.max(1),
            "heading must be visible after # jump: scroll={} heading={line} vh={}",
            ctl.state.view.scroll_y,
            ctl.state.view.viewport_height,
            line = heading_line
        );
        assert!(
            ctl.state.view.scroll_y > 0 || heading_line < 5,
            "should have scrolled away from document top for a mid-page target"
        );
    }

    #[test]
    fn js_button_activate_patches_dom_and_keeps_top_line() {
        // Same-document type=button (clipboard copy): recapture the flipped
        // label but pin the topmost visible line so the markdown view does not
        // jump toward the button.
        let url = "https://getkirby.com/docs/guide";
        let mut before_nodes = Vec::new();
        for i in 0..12 {
            before_nodes.push(raw_text(&format!("p{i}"), &format!("paragraph {i} body text")));
        }
        before_nodes.push(copy_button_node("Copy"));
        let before = normalize_fixture(meta("main:1", url), before_nodes).expect("before");

        let mut after_nodes = Vec::new();
        for i in 0..12 {
            after_nodes.push(raw_text(&format!("p{i}"), &format!("paragraph {i} body text")));
        }
        after_nodes.push(copy_button_node("Copied!"));
        let after = normalize_fixture(meta("main:2", url), after_nodes).expect("after");

        let top_ref = before
            .components()
            .find(|c| c.attrs.element_id.as_deref() == Some("p0"))
            .unwrap()
            .semantic_ref
            .clone();
        let mid_ref = before
            .components()
            .find(|c| c.attrs.element_id.as_deref() == Some("p3"))
            .unwrap()
            .semantic_ref
            .clone();
        let button = before
            .components()
            .find(|c| c.attrs.element_id.as_deref() == Some("copy"))
            .unwrap()
            .semantic_ref
            .clone();

        let mut driver = FakePageDriver::new(vec![before.clone(), after]);
        driver.advance_page_on_capture = true;
        let mut ctl = Controller::new();
        ctl.shared.activate_runtime();
        ctl.state.publish_page(before.clone());
        ctl.shared.publish(before);
        ctl.set_viewport(24, 5);

        // Scroll so p3 is the topmost visible line; select the copy button below.
        let lines = ctl.content_lines();
        let top_idx = line_index_of(&lines, &mid_ref).expect("mid line");
        ctl.state.view.scroll_y = top_idx;
        ctl.state.view.selection = Some(button.clone());
        let top_before = top_viewport_anchor(&lines, ctl.state.view.scroll_y)
            .expect("top anchor")
            .semantic_ref;

        ctl.activate_selection().expect("activate");
        assert!(ctl.state.lifecycle.is_loading());
        ctl.acknowledge_loading_frame();
        ctl.perform_pending_page_action(&mut driver)
            .expect("run activate");

        assert!(ctl.state.lifecycle.is_ready());
        assert_eq!(ctl.state.url(), url);
        assert_eq!(ctl.state.revision(), "main:2");
        assert!(
            driver.capture_calls >= 1,
            "same-doc button should recapture to pick up Copied!"
        );
        // Label updated in the published view.
        let lines_after = ctl.content_lines();
        assert!(
            lines_after.iter().any(|l| l.text.contains("Copied!")),
            "patched label missing: {:?}",
            lines_after.iter().map(|l| &l.text).collect::<Vec<_>>()
        );
        // Top of viewport still the same identity (p3), not yanked to the button.
        let top_after = top_viewport_anchor(&lines_after, ctl.state.view.scroll_y)
            .expect("top after")
            .semantic_ref;
        let top_after_rebound = ctl
            .state
            .document()
            .unwrap()
            .rebind_surviving(&top_before)
            .expect("top identity survives");
        assert_eq!(
            top_after, top_after_rebound,
            "top line should stay pinned after patch"
        );
        // Selection rebinds to the button on the new revision.
        let button_after = ctl
            .state
            .document()
            .unwrap()
            .rebind_surviving(&button)
            .expect("button survives");
        assert_eq!(ctl.state.view.selection.as_ref(), Some(&button_after));
        // Sanity: top was not p0 (we scrolled).
        assert_ne!(
            ctl.state
                .document()
                .unwrap()
                .rebind_surviving(&top_ref)
                .ok()
                .as_ref(),
            Some(&top_after)
        );
        assert!(
            driver
                .activated
                .iter()
                .any(|(r, _)| r == button.as_str()),
            "button was clicked: {:?}",
            driver.activated
        );
    }
}

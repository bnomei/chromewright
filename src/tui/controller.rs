//! Lifecycle controller: Loading → browser action → settle → capture → atomic Ready | Error.

use crate::semantic::{SemanticDocument, SemanticRef};
use crate::tui::content::{
    build_content_lines, focusable_refs, form_control_refs, line_index_of, rendered_block_text,
    search_refs,
};
use crate::tui::driver::PageDriver;
use crate::tui::hints::{HintMatch, LinkHint, assign_hints, match_hint};
use crate::tui::state::{HintMode, InputKind, InteractionMode, TuiState};

/// Orchestrates page-changing actions and pure view updates against [`TuiState`].
pub struct Controller {
    pub state: TuiState,
    /// Active link hints when in Hint mode.
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
    pub fn new() -> Self {
        Self {
            state: TuiState::new(),
            hints: Vec::new(),
            pending_page_action: None,
        }
    }

    pub fn with_state(state: TuiState) -> Self {
        Self {
            state,
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

    pub fn history_back(&mut self) {
        self.queue_page_action("history_back", PageOperation::HistoryBack, None);
    }

    pub fn history_forward(&mut self) {
        self.queue_page_action("history_forward", PageOperation::HistoryForward, None);
    }

    pub fn reload(&mut self) {
        self.queue_page_action("reload", PageOperation::Reload, None);
    }

    pub fn next_tab(&mut self) {
        self.queue_page_action("next_tab", PageOperation::NextTab, None);
    }

    pub fn prev_tab(&mut self) {
        self.queue_page_action("prev_tab", PageOperation::PrevTab, None);
    }

    pub fn close_tab(&mut self) {
        self.queue_page_action("close_tab", PageOperation::CloseTab, None);
    }

    pub fn new_tab(&mut self) {
        self.queue_page_action("new_tab", PageOperation::NewTab, None);
    }

    /// Queue a link follow by exact semantic_ref.
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

    /// Record that `Terminal::draw` completed while this transition was Loading.
    pub fn acknowledge_loading_frame(&mut self) {
        if let Some(pending) = &mut self.pending_page_action
            && self.state.lifecycle.is_loading()
        {
            pending.loading_frame_drawn = true;
        }
    }

    /// Perform deferred browser work after a successfully rendered Loading frame.
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
            // Read metadata after settle as an explicit freshness barrier. The
            // semantic capture carries the atomically published values, but
            // this call ensures a cached/stale browser handoff is not treated
            // as a settled page.
            let metadata = driver.document_metadata().map_err(|e| e.to_string())?;
            if metadata.document_id.is_empty()
                || metadata.revision.is_empty()
                || metadata.ready_state != "complete"
            {
                return Err("browser did not provide stable complete document metadata".into());
            }
            let doc = driver.capture_semantic().map_err(|e| e.to_string())?;
            if doc.document.document_id != metadata.document_id
                || doc.document.revision != metadata.revision
                || doc.document.url != metadata.url
                || doc.document.title != metadata.title
            {
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

    pub fn content_lines(&self) -> Vec<crate::tui::content::ContentLine> {
        match self.state.document() {
            Some(doc) => build_content_lines(doc, &self.state.view.collapsed),
            None => Vec::new(),
        }
    }

    pub fn scroll_down(&mut self) {
        self.move_selection(1);
    }

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

    pub fn half_page_down(&mut self) {
        let h = self.state.view.viewport_height.max(1) / 2;
        self.state.view.scroll_y = self.state.view.scroll_y.saturating_add(h);
        let len = self.content_lines().len();
        self.state.clamp_scroll(len);
    }

    pub fn half_page_up(&mut self) {
        let h = self.state.view.viewport_height.max(1) / 2;
        self.state.view.scroll_y = self.state.view.scroll_y.saturating_sub(h);
    }

    pub fn go_top(&mut self) {
        self.state.view.scroll_y = 0;
        let lines = self.content_lines();
        if let Some(r) = lines.iter().find_map(|l| l.semantic_ref.clone()) {
            self.state.view.selection = Some(r);
        }
    }

    pub fn go_bottom(&mut self) {
        let lines = self.content_lines();
        let len = lines.len();
        self.state.view.scroll_y = len.saturating_sub(self.state.view.viewport_height.max(1));
        if let Some(r) = lines.iter().rev().find_map(|l| l.semantic_ref.clone()) {
            self.state.view.selection = Some(r);
        }
    }

    pub fn scroll_left(&mut self) {
        self.state.view.scroll_x = self.state.view.scroll_x.saturating_sub(4);
    }

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

    pub fn copy_block_text(&mut self) -> Option<String> {
        let doc = self.state.document()?;
        let sel = self.state.view.selection.as_ref()?;
        doc.resolve(sel).ok()?;
        rendered_block_text(doc, sel)
    }

    pub fn copy_ref_text(&mut self) -> Option<String> {
        let doc = self.state.document()?;
        let sel = self.state.view.selection.as_ref()?;
        doc.resolve(sel).ok()?;
        Some(sel.as_str().to_string())
    }

    pub fn enter_url_input(&mut self) {
        if self.state.lifecycle.is_loading() {
            return;
        }
        let buffer = self.state.url().to_string();
        self.state.mode = InteractionMode::Input(InputKind::Url { buffer });
    }

    pub fn enter_search(&mut self) {
        if self.state.lifecycle.is_loading() {
            return;
        }
        self.state.mode = InteractionMode::Input(InputKind::Search {
            buffer: String::new(),
        });
    }

    pub fn apply_search(&mut self, query: &str) {
        let lines = self.content_lines();
        let matches = search_refs(&lines, query);
        self.state.view.search_query = query.to_string();
        self.state.view.search_matches = matches;
        self.state.view.search_index = 0;
        if let Some(first) = self.state.view.search_matches.first().cloned() {
            self.state.view.selection = Some(first.clone());
            if let Some(idx) = line_index_of(&lines, &first) {
                self.ensure_visible(idx, lines.len());
            }
            self.state.view.set_status(format!(
                "search: {}/{}",
                1,
                self.state.view.search_matches.len()
            ));
        } else {
            self.state.view.set_status("no matches");
        }
        self.state.mode = InteractionMode::Normal;
    }

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

    /// Feed a character while in hint mode. Returns a ref to follow when complete.
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

    pub fn escape(&mut self) {
        self.state.mode = InteractionMode::Normal;
        self.state.view.hint_buffer.clear();
        self.state.view.inspect_text = None;
        self.hints.clear();
    }

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
        normalize_fixture(
            meta(rev, "https://example.com/"),
            vec![RawSemanticNode {
                kind: "text".into(),
                tag: Some("p".into()),
                id: Some(id.into()),
                unique_id: true,
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
            }],
        )
        .expect("doc")
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
}

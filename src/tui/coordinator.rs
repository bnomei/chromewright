//! Sole owner of companion browser mutation transactions and publication barriers.

use crate::browser::BrowserSession;
use crate::semantic::{SemanticDocument, SemanticRef};
use crate::tui::driver::{PageDriver, SessionPageDriver};
use crate::tui::shared::{CoordinationError, PageActionTicket, RefreshPage, SharedTuiState};
use std::sync::Arc;

/// Structurally couples the one companion browser session to its publication store.
pub struct PageCoordinator {
    session: Arc<BrowserSession>,
    shared: SharedTuiState,
}

impl PageCoordinator {
    pub fn new(session: Arc<BrowserSession>, shared: SharedTuiState) -> Self {
        Self { session, shared }
    }

    pub fn session(&self) -> &Arc<BrowserSession> {
        &self.session
    }
    pub fn shared(&self) -> &SharedTuiState {
        &self.shared
    }

    pub fn begin(&self, action: impl Into<String>) -> Result<PageActionTicket, CoordinationError> {
        self.shared.begin_page_action(action)
    }

    pub fn capture_with_metadata_barrier<D: PageDriver>(
        driver: &mut D,
    ) -> Result<SemanticDocument, String> {
        let mut last_err = String::new();
        for attempt in 0..4 {
            if attempt > 0 {
                let _ = driver.wait_settle();
            }
            let document = driver.capture_semantic().map_err(|e| e.to_string())?;
            let metadata = driver.document_metadata().map_err(|e| e.to_string())?;
            if metadata.document_id.is_empty()
                || metadata.revision.is_empty()
                || metadata.ready_state != "complete"
            {
                last_err = "browser did not provide stable complete document metadata".into();
                continue;
            }
            if !crate::semantic::capture_matches_document_metadata(&document.document, &metadata) {
                last_err = "semantic capture metadata changed during publication".into();
                continue;
            }
            return Ok(document);
        }
        Err(last_err)
    }

    pub fn finalize_browser_mutation(
        &self,
        ticket: PageActionTicket,
        action: &str,
    ) -> Result<RefreshPage, CoordinationError> {
        let result = (|| {
            let mut driver = SessionPageDriver::new(&self.session);
            driver
                .wait_settle()
                .map_err(|_| CoordinationError::RefreshFailed)?;
            Self::capture_with_metadata_barrier(&mut driver)
                .map_err(|_| CoordinationError::RefreshFailed)
        })();
        match result {
            Ok(document) => {
                let page = RefreshPage {
                    document_id: document.document.document_id.clone(),
                    revision: document.document.revision.clone(),
                    url: document.document.url.clone(),
                    title: document.document.title.clone(),
                };
                self.shared.commit_page_action(ticket, document, None)?;
                Ok(page)
            }
            Err(error) => {
                let _ = self
                    .shared
                    .fail_page_action(ticket, action, error.to_string());
                Err(error)
            }
        }
    }

    pub fn refresh(&self) -> Result<RefreshPage, CoordinationError> {
        if !self.shared.runtime_active() {
            return Err(CoordinationError::RuntimeRequired);
        }
        let ticket = self.begin("refresh").map_err(|e| {
            if e == CoordinationError::ActionInProgress {
                CoordinationError::RefreshInProgress
            } else {
                e
            }
        })?;
        if self.session.evaluate("location.reload()", false).is_err() {
            let _ = self.shared.fail_page_action(
                ticket,
                "refresh",
                CoordinationError::RefreshFailed.to_string(),
            );
            return Err(CoordinationError::RefreshFailed);
        }
        self.finalize_browser_mutation(ticket, "refresh")
    }

    pub fn commit(
        &self,
        ticket: PageActionTicket,
        document: SemanticDocument,
        selection: Option<SemanticRef>,
    ) -> Result<(), CoordinationError> {
        self.shared
            .commit_page_action(ticket, document, selection)
            .map(|_| ())
    }
    pub fn fail(
        &self,
        ticket: PageActionTicket,
        action: &str,
        message: String,
    ) -> Result<(), CoordinationError> {
        self.shared.fail_page_action(ticket, action, message)
    }
    pub fn retain(&self, ticket: PageActionTicket) -> Result<(), CoordinationError> {
        self.shared.finish_page_action_retained(ticket)
    }
    pub fn clear(&self, ticket: PageActionTicket) -> Result<(), CoordinationError> {
        self.shared.clear_session(ticket)
    }
}

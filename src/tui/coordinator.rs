//! Sole owner of companion browser mutation transactions and publication barriers.

use crate::browser::BrowserSession;
use crate::semantic::{SemanticDocument, SemanticRef};
use crate::tui::driver::{PageDriver, SessionPageDriver};
use crate::tui::shared::{CoordinationError, PageActionTicket, RefreshPage, SharedTuiState};
use std::sync::{Arc, Condvar, Mutex};

#[derive(Debug)]
struct CompanionRequests {
    state: Mutex<CompanionRequestState>,
    drained: Condvar,
}

#[derive(Debug)]
struct CompanionRequestState {
    accepting: bool,
    active: usize,
}

/// Keeps one accepted companion tool request counted until its blocking work ends.
pub(crate) struct CompanionRequestGuard {
    requests: Arc<CompanionRequests>,
}

impl Drop for CompanionRequestGuard {
    fn drop(&mut self) {
        let mut state = self.requests.state.lock().unwrap();
        state.active = state.active.saturating_sub(1);
        if state.active == 0 {
            self.requests.drained.notify_all();
        }
    }
}

/// Fails a claimed companion page transaction if unwinding skips normal resolution.
pub(crate) struct CompanionPageTransaction {
    shared: SharedTuiState,
    ticket: PageActionTicket,
    action: String,
}

impl CompanionPageTransaction {
    pub(crate) fn ticket(&self) -> PageActionTicket {
        self.ticket
    }
}

impl Drop for CompanionPageTransaction {
    fn drop(&mut self) {
        let _ = self.shared.fail_page_action(
            self.ticket,
            &self.action,
            "companion page action aborted",
        );
    }
}

/// Result of successfully finalizing a companion browser mutation.
pub enum FinalizeOutcome {
    Published(RefreshPage),
    Cleared,
}

/// Structurally couples the one companion browser session to its publication store.
pub struct PageCoordinator {
    session: Arc<BrowserSession>,
    shared: SharedTuiState,
    companion_requests: Arc<CompanionRequests>,
}

impl PageCoordinator {
    pub fn new(session: Arc<BrowserSession>, shared: SharedTuiState) -> Self {
        Self {
            session,
            shared,
            companion_requests: Arc::new(CompanionRequests {
                state: Mutex::new(CompanionRequestState {
                    accepting: true,
                    active: 0,
                }),
                drained: Condvar::new(),
            }),
        }
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

    pub(crate) fn begin_companion(
        &self,
        action: impl Into<String>,
    ) -> Result<CompanionPageTransaction, CoordinationError> {
        let action = action.into();
        let ticket = self.shared.begin_companion_page_action(&action)?;
        Ok(CompanionPageTransaction {
            shared: self.shared.clone(),
            ticket,
            action,
        })
    }

    /// Count a companion tool request before it can be queued on the blocking pool.
    pub(crate) fn begin_companion_request(&self) -> Option<CompanionRequestGuard> {
        let mut state = self.companion_requests.state.lock().unwrap();
        if !state.accepting {
            return None;
        }
        state.active += 1;
        Some(CompanionRequestGuard {
            requests: self.companion_requests.clone(),
        })
    }

    /// Reject new requests and wait until every previously accepted worker exits.
    pub(crate) fn drain_companion_requests(&self) {
        let mut state = self.companion_requests.state.lock().unwrap();
        state.accepting = false;
        while state.active != 0 {
            state = self.companion_requests.drained.wait(state).unwrap();
        }
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
    ) -> Result<FinalizeOutcome, CoordinationError> {
        let tabs = match self.session.list_tabs() {
            Ok(tabs) => tabs,
            Err(_) => {
                let error = CoordinationError::RefreshFailed;
                let _ = self
                    .shared
                    .fail_page_action(ticket, action, error.to_string());
                return Err(error);
            }
        };
        if tabs.is_empty() {
            self.shared.clear_session(ticket)?;
            return Ok(FinalizeOutcome::Cleared);
        }
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
                Ok(FinalizeOutcome::Published(page))
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
        let transaction = self.begin_companion("refresh").map_err(|e| {
            if e == CoordinationError::ActionInProgress {
                CoordinationError::RefreshInProgress
            } else {
                e
            }
        })?;
        let ticket = transaction.ticket();
        if self.session.evaluate("location.reload()", false).is_err() {
            let _ = self.shared.fail_page_action(
                ticket,
                "refresh",
                CoordinationError::RefreshFailed.to_string(),
            );
            return Err(CoordinationError::RefreshFailed);
        }
        match self.finalize_browser_mutation(ticket, "refresh")? {
            FinalizeOutcome::Published(page) => Ok(page),
            FinalizeOutcome::Cleared => Err(CoordinationError::NoDocument),
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::backend::FakeSessionBackend;
    use std::sync::mpsc;
    use std::thread;

    #[test]
    fn companion_request_drain_waits_for_accepted_workers_and_closes_gate() {
        let coordinator = Arc::new(PageCoordinator::new(
            Arc::new(BrowserSession::with_test_backend(FakeSessionBackend::new())),
            SharedTuiState::new(),
        ));
        let request = coordinator
            .begin_companion_request()
            .expect("request accepted before shutdown");
        let (started_tx, started_rx) = mpsc::channel();
        let (drained_tx, drained_rx) = mpsc::channel();
        let draining = coordinator.clone();
        let join = thread::spawn(move || {
            started_tx.send(()).unwrap();
            draining.drain_companion_requests();
            drained_tx.send(()).unwrap();
        });

        started_rx.recv().expect("drain started");
        while coordinator.begin_companion_request().is_some() {
            thread::yield_now();
        }
        assert!(drained_rx.try_recv().is_err(), "drain must wait for worker");
        drop(request);
        drained_rx.recv().expect("drain completed");
        join.join().expect("drain thread");
        assert!(
            coordinator.begin_companion_request().is_none(),
            "shutdown gate must remain closed"
        );
    }

    #[test]
    fn companion_transaction_guard_fails_ticket_during_unwind() {
        let shared = SharedTuiState::new();
        shared.activate_runtime();
        let coordinator = PageCoordinator::new(
            Arc::new(BrowserSession::with_test_backend(FakeSessionBackend::new())),
            shared.clone(),
        );

        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _transaction = coordinator.begin_companion("panic-test").unwrap();
            panic!("simulated companion tool panic");
        }));

        assert!(unwind.is_err());
        assert!(matches!(
            shared.lifecycle(),
            crate::tui::state::Lifecycle::Error { action, message }
                if action == "panic-test" && message == "companion page action aborted"
        ));
    }
}

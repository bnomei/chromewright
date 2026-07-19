//! In-process coordination between the terminal UI and the co-hosted MCP companion.
//!
//! [`SharedTuiState`] is the single source of truth for published
//! [`SemanticDocument`]s, revision retention/eviction, human selection, agent
//! [`Attention`], and the shared Loading → Ready | Error lifecycle. Companion
//! tools and the terminal controller both claim page actions through this type
//! so they cannot race the one live [`crate::browser::BrowserSession`].

use crate::semantic::{SemanticDocument, SemanticRef, render_outline, render_semantic_markdown};
use crate::tui::state::Lifecycle;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// Opaque ownership token for one Loading transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageActionTicket(u64);

/// How many historical captures (excluding the active document) to retain for
/// revisioned MCP resources and fail-closed ref lookups.
pub const DEFAULT_REVISION_RETENTION: usize = 8;

/// Maximum characters allowed in an agent attention message (tool + resource).
pub const MAX_ATTENTION_MESSAGE_CHARS: usize = 512;

/// Fail-closed coordination failures for companion tools and terminal publish paths.
///
/// Distinguishes missing vs. evicted vs. wrong-document revisions so callers can
/// report stale `semantic_ref` targets without falling through to the active page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinationError {
    /// No SemanticDocument has been published yet.
    NoDocument,
    /// Document id empty or contains path-breaking characters.
    MalformedDocumentId,
    /// Revision token empty or contains path-breaking characters.
    MalformedRevision,
    /// Requested revision is not active and not in retention.
    RevisionUnavailable,
    /// `semantic_ref` or lookup targets a different document_id than expected.
    WrongDocument,
    /// Revision left the retention window and must not fall through to active.
    EvictedRevision,
    /// Reference revision no longer matches the retained/active capture.
    StaleReference,
    /// Opaque `semantic_ref` failed structural decode.
    MalformedReference,
    /// Reference is well-formed but unknown in the addressed document.
    UnknownReference,
    /// Attention message exceeds [`MAX_ATTENTION_MESSAGE_CHARS`].
    MessageTooLong,
    /// Companion mutation requires an active TUI runtime / bound session.
    RuntimeRequired,
    /// Another page action already holds the Loading lifecycle lock.
    ActionInProgress,
    /// A companion refresh is already in flight.
    RefreshInProgress,
    /// Browser refresh or semantic recapture failed after Loading was claimed.
    RefreshFailed,
    /// Completion did not own the currently active page transaction.
    StaleTransaction,
}

impl std::fmt::Display for CoordinationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::NoDocument => "no semantic document is published",
            Self::MalformedDocumentId => "malformed document id",
            Self::MalformedRevision => "malformed revision",
            Self::RevisionUnavailable => "revision is unavailable",
            Self::WrongDocument => "reference targets a different document",
            Self::EvictedRevision => "revision was evicted",
            Self::StaleReference => "semantic reference is stale",
            Self::MalformedReference => "semantic reference is malformed",
            Self::UnknownReference => "semantic reference is unknown in the active document",
            Self::MessageTooLong => "attention message exceeds the bound",
            Self::RuntimeRequired => "active TUI runtime is required",
            Self::ActionInProgress => "a TUI page action is already in progress",
            Self::RefreshInProgress => "a TUI refresh is already in progress",
            Self::RefreshFailed => "browser refresh or semantic recapture failed",
            Self::StaleTransaction => "page action transaction is stale",
        };
        f.write_str(message)
    }
}

impl std::error::Error for CoordinationError {}

/// Validated revision token used in revisioned resource URIs and lookups.
///
/// Rejects empty values and characters that would break path segments (`/` or
/// whitespace). Does not interpret browser revision format beyond that.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionId(String);

impl RevisionId {
    /// Parse a revision string for coordination lookups.
    ///
    /// # Errors
    /// Returns [`CoordinationError::MalformedRevision`] when the token is empty
    /// or contains `/` or whitespace.
    pub fn parse(value: &str) -> Result<Self, CoordinationError> {
        if value.is_empty() || value.chars().any(|c| c.is_whitespace() || c == '/') {
            return Err(CoordinationError::MalformedRevision);
        }
        Ok(Self(value.to_owned()))
    }

    /// Borrow the validated revision token.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Validated document identity for revisioned resource lookups.
///
/// Same path-safety rules as [`RevisionId`]; pairs with a revision to address a
/// retained or active capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentId(String);

impl DocumentId {
    /// Parse a document id for coordination lookups.
    ///
    /// # Errors
    /// Returns [`CoordinationError::MalformedDocumentId`] when the token is empty
    /// or contains `/` or whitespace.
    pub fn parse(value: &str) -> Result<Self, CoordinationError> {
        if value.is_empty() || value.chars().any(|c| c.is_whitespace() || c == '/') {
            return Err(CoordinationError::MalformedDocumentId);
        }
        Ok(Self(value.to_owned()))
    }

    /// Borrow the validated document id token.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Record that a historical capture left retention and must fail closed on lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Eviction {
    /// Document id of the dropped capture.
    pub document_id: String,
    /// Revision of the dropped capture.
    pub revision: String,
}

/// Agent-owned attention highlight, independent of human selection.
///
/// Always bound to an exact `semantic_ref` plus the document/revision that
/// validated it. Cleared when the reference no longer resolves after a publish.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Attention {
    /// Exact component ref under attention, when set.
    pub semantic_ref: Option<SemanticRef>,
    /// Document that validated the ref (must match active/retained capture).
    pub document_id: Option<String>,
    /// Revision that validated the ref.
    pub revision: Option<String>,
    /// Optional agent message shown with the highlight.
    pub message: Option<String>,
}

impl Attention {
    /// True when a `semantic_ref` is currently highlighted.
    pub fn is_set(&self) -> bool {
        self.semantic_ref.is_some()
    }
}

fn map_ref_error(error: crate::semantic::SemanticRefError) -> CoordinationError {
    use crate::semantic::SemanticRefError;
    match error {
        SemanticRefError::Malformed => CoordinationError::MalformedReference,
        SemanticRefError::WrongDocument { .. } => CoordinationError::WrongDocument,
        SemanticRefError::Stale { .. } => CoordinationError::StaleReference,
        SemanticRefError::Unknown | SemanticRefError::Ambiguous => {
            CoordinationError::UnknownReference
        }
    }
}

fn bound_attention_message(message: Option<String>) -> Result<Option<String>, CoordinationError> {
    let Some(message) = message else {
        return Ok(None);
    };
    if message.chars().count() > MAX_ATTENTION_MESSAGE_CHARS {
        return Err(CoordinationError::MessageTooLong);
    }
    if message.is_empty() {
        Ok(None)
    } else {
        Ok(Some(message))
    }
}

/// Atomically published metadata for a successful companion-triggered refresh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefreshPage {
    pub document_id: String,
    pub revision: String,
    pub url: String,
    pub title: String,
}

#[derive(Debug)]
struct Inner {
    runtime_active: bool,
    lifecycle: Lifecycle,
    active: Option<SemanticDocument>,
    retained: VecDeque<SemanticDocument>,
    selection: Option<SemanticRef>,
    attention: Attention,
    limit: usize,
    evictions: VecDeque<Eviction>,
    next_ticket: u64,
    active_ticket: Option<PageActionTicket>,
}

/// Cloneable handle to the one in-process TUI/companion coordination object.
///
/// Holds published SemanticDocument history,
/// selection/attention, and the runtime-active gate that companion mutations
/// require. It deliberately has no browser-session access; browser work belongs
/// to [`crate::tui::PageCoordinator`].
#[derive(Clone)]
pub struct SharedTuiState {
    inner: Arc<Mutex<Inner>>,
}

impl SharedTuiState {
    /// Empty coordination storage with default revision retention.
    pub fn new() -> Self {
        Self::with_retention(DEFAULT_REVISION_RETENTION)
    }

    /// Bound coordination with an explicit historical-capture budget.
    ///
    /// `limit` is clamped to at least 1 and counts retained pages only (not the
    /// active document).
    pub fn with_retention(limit: usize) -> Self {
        Self::with_limit(limit)
    }

    /// Alias for constructing empty coordination before the tool registry is sealed.
    pub fn unbound() -> Self {
        Self::new()
    }
    fn with_limit(limit: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                runtime_active: false,
                lifecycle: Lifecycle::Ready,
                active: None,
                retained: VecDeque::new(),
                selection: None,
                attention: Attention::default(),
                limit: limit.max(1),
                evictions: VecDeque::new(),
                next_ticket: 0,
                active_ticket: None,
            })),
        }
    }

    /// Mark the interactive runtime as ready to accept companion operations.
    /// Standard stdio sessions never call this, so their registry stays separate.
    pub fn activate_runtime(&self) {
        self.inner.lock().unwrap().runtime_active = true;
    }

    /// Stop accepting companion state changes before the terminal or HTTP host
    /// is torn down.
    pub fn deactivate_runtime(&self) {
        self.inner.lock().unwrap().runtime_active = false;
    }

    /// Whether companion tools may mutate coordination state (set by the interactive TUI host).
    pub fn runtime_active(&self) -> bool {
        self.inner.lock().unwrap().runtime_active
    }

    /// Atomically check the companion runtime gate and claim a page action.
    pub fn begin_companion_page_action(
        &self,
        action: impl Into<String>,
    ) -> Result<PageActionTicket, CoordinationError> {
        let mut state = self.inner.lock().unwrap();
        if !state.runtime_active {
            return Err(CoordinationError::RuntimeRequired);
        }
        Self::begin_page_action_locked(&mut state, action.into())
    }

    /// Shared Loading → Ready | Error lifecycle visible to both terminal and companion.
    pub fn lifecycle(&self) -> Lifecycle {
        self.inner.lock().unwrap().lifecycle.clone()
    }

    /// Claim the one browser/semantic lifecycle before starting any page
    /// action. Both the terminal controller and companion tools use this
    /// transition, so a second actor cannot touch the shared BrowserSession
    /// while a Loading page is awaiting capture.
    pub fn begin_page_action(
        &self,
        action: impl Into<String>,
    ) -> Result<PageActionTicket, CoordinationError> {
        let mut state = self.inner.lock().unwrap();
        Self::begin_page_action_locked(&mut state, action.into())
    }

    fn begin_page_action_locked(
        state: &mut Inner,
        action: String,
    ) -> Result<PageActionTicket, CoordinationError> {
        if state.lifecycle.is_loading() {
            return Err(CoordinationError::ActionInProgress);
        }
        state.next_ticket = state.next_ticket.wrapping_add(1).max(1);
        let ticket = PageActionTicket(state.next_ticket);
        state.active_ticket = Some(ticket);
        state.lifecycle = Lifecycle::Loading { action };
        Ok(ticket)
    }

    /// Atomically expose a terminal/companion action failure while retaining
    /// the last complete semantic document.
    pub fn fail_page_action(
        &self,
        ticket: PageActionTicket,
        action: impl Into<String>,
        message: impl Into<String>,
    ) -> Result<(), CoordinationError> {
        let mut state = self.inner.lock().unwrap();
        Self::validate_ticket(&state, ticket)?;
        state.active_ticket = None;
        state.lifecycle = Lifecycle::Error {
            action: action.into(),
            message: message.into(),
        };
        Ok(())
    }

    /// End a Loading page action without publishing a new document.
    ///
    /// Used when an in-page activation (clipboard copy, toggle that does not
    /// navigate) leaves document identity and URL unchanged so a full
    /// settle+recapture would only flash Loading and reset the view.
    pub fn finish_page_action_retained(
        &self,
        ticket: PageActionTicket,
    ) -> Result<(), CoordinationError> {
        let mut state = self.inner.lock().unwrap();
        Self::validate_ticket(&state, ticket)?;
        state.active_ticket = None;
        state.lifecycle = Lifecycle::Ready;
        Ok(())
    }

    /// Dismiss a shared Error lifecycle back to Ready without changing the
    /// retained document. No-op while Loading or already Ready so a dismiss
    /// cannot interrupt an in-flight page action.
    pub fn clear_error(&self) -> bool {
        let mut state = self.inner.lock().unwrap();
        if matches!(state.lifecycle, Lifecycle::Error { .. }) {
            state.lifecycle = Lifecycle::Ready;
            true
        } else {
            false
        }
    }

    /// Clear the published document and view-facing selection/attention after
    /// the browser has no remaining tabs. Leaves lifecycle Ready so recovery
    /// actions (new tab) can run.
    pub fn clear_session(&self, ticket: PageActionTicket) -> Result<(), CoordinationError> {
        let mut state = self.inner.lock().unwrap();
        Self::validate_ticket(&state, ticket)?;
        state.active = None;
        state.selection = None;
        state.attention = Attention::default();
        state.lifecycle = Lifecycle::Ready;
        state.active_ticket = None;
        Ok(())
    }

    fn validate_ticket(state: &Inner, ticket: PageActionTicket) -> Result<(), CoordinationError> {
        if state.active_ticket == Some(ticket) && state.lifecycle.is_loading() {
            Ok(())
        } else {
            Err(CoordinationError::StaleTransaction)
        }
    }

    pub fn commit_page_action(
        &self,
        ticket: PageActionTicket,
        document: SemanticDocument,
        selection: Option<SemanticRef>,
    ) -> Result<Option<Eviction>, CoordinationError> {
        let mut state = self.inner.lock().unwrap();
        Self::validate_ticket(&state, ticket)?;
        state.active_ticket = None;
        Ok(Self::publish_locked(&mut state, document, Some(selection)))
    }

    /// Atomically publish a complete SemanticDocument as active and transition
    /// lifecycle to Ready.
    ///
    /// Previous active captures move into retention (oldest first); when the
    /// retention budget is exceeded the oldest is recorded as an [`Eviction`].
    /// Selection is re-validated against the new document (dropped if unresolved);
    /// attention survives only when its exact `semantic_ref` still resolves.
    ///
    /// Returns the oldest eviction produced by this publish, if any.
    pub fn publish(&self, document: SemanticDocument) -> Option<Eviction> {
        let mut state = self.inner.lock().unwrap();
        Self::publish_locked(&mut state, document, None)
    }

    /// Publish the new complete document, lifecycle, and human selection under
    /// one lock. This prevents companion reads from observing a new revision
    /// paired with selection from the previous page.
    pub fn publish_with_selection(
        &self,
        document: SemanticDocument,
        selection: Option<SemanticRef>,
    ) -> Option<Eviction> {
        let mut state = self.inner.lock().unwrap();
        Self::publish_locked(&mut state, document, Some(selection))
    }

    fn publish_locked(
        state: &mut Inner,
        document: SemanticDocument,
        selection: Option<Option<SemanticRef>>,
    ) -> Option<Eviction> {
        if let Some(old) = state.active.replace(document.clone()) {
            state.retained.push_back(old);
        }
        let mut evicted = None;
        // Retain at most `limit` historical captures. Evictions are explicit and
        // never fall through to the active document for revision lookups.
        while state.retained.len() > state.limit {
            let old = state.retained.pop_front().unwrap();
            let eviction = Eviction {
                document_id: old.document.document_id,
                revision: old.document.revision,
            };
            state.evictions.push_back(eviction.clone());
            if state.evictions.len() > state.limit.saturating_mul(4).max(8) {
                state.evictions.pop_front();
            }
            evicted = Some(eviction);
        }
        state.lifecycle = Lifecycle::Ready;
        let selection = selection.unwrap_or_else(|| state.selection.clone());
        state.selection = selection.filter(|reference| document.resolve(reference).is_ok());
        // Attention is exact-ref only: drop it when the spotlight does not survive.
        if let Some(reference) = state.attention.semantic_ref.clone() {
            if document.resolve(&reference).is_ok() {
                state.attention.document_id = Some(document.document.document_id.clone());
                state.attention.revision = Some(document.document.revision.clone());
            } else {
                state.attention = Attention::default();
            }
        }
        evicted
    }
    /// Last complete published SemanticDocument, retained through Loading and Error.
    ///
    /// Does not encode lifecycle: callers that need Loading vs Ready must consult
    /// [`Self::lifecycle`] or [`Self::read_snapshot`]. Fails with [`CoordinationError::NoDocument`]
    /// when nothing has been published yet.
    pub fn active(&self) -> Result<SemanticDocument, CoordinationError> {
        // Last complete capture is retained through Loading/Error so readers
        // never observe a mixed partial revision. Callers that need the
        // lifecycle must consult `lifecycle()` separately (or `read_snapshot`).
        self.inner
            .lock()
            .unwrap()
            .active
            .clone()
            .ok_or(CoordinationError::NoDocument)
    }

    /// Atomic coordination snapshot for resource/tool reads.
    ///
    /// Document content is the last complete capture even while Loading so MCP
    /// cannot claim Ready or a mixed revision from this snapshot alone.
    pub fn read_snapshot(&self) -> CoordinationSnapshot {
        let state = self.inner.lock().unwrap();
        CoordinationSnapshot {
            lifecycle: state.lifecycle.clone(),
            active: state.active.clone(),
            selection: state.selection.clone(),
            attention: state.attention.clone(),
            retained: state.retained.iter().cloned().collect(),
        }
    }

    /// True while terminal or companion owns a Loading page action.
    pub fn page_action_in_progress(&self) -> bool {
        self.inner.lock().unwrap().lifecycle.is_loading()
    }

    /// Reject concurrent page-mutating work against the shared BrowserSession.
    pub fn ensure_page_action_available(&self) -> Result<(), CoordinationError> {
        if self.page_action_in_progress() {
            Err(CoordinationError::ActionInProgress)
        } else {
            Ok(())
        }
    }

    /// Retained historical captures in publication order (oldest first).
    pub fn retained(&self) -> Vec<SemanticDocument> {
        self.inner
            .lock()
            .unwrap()
            .retained
            .iter()
            .cloned()
            .collect()
    }

    /// Retention capacity for historical captures (not including the active page).
    pub fn retention_limit(&self) -> usize {
        self.inner.lock().unwrap().limit
    }

    /// Look up an active or retained capture by document id + revision.
    ///
    /// # Errors
    /// - [`CoordinationError::EvictedRevision`] when the pair was previously dropped
    /// - [`CoordinationError::WrongDocument`] when another document is active/retained
    /// - [`CoordinationError::RevisionUnavailable`] when the pair was never published
    /// - Malformed id/revision parse errors from [`DocumentId`] / [`RevisionId`]
    pub fn revision(
        &self,
        document_id: &str,
        revision: &str,
    ) -> Result<SemanticDocument, CoordinationError> {
        let document_id = DocumentId::parse(document_id)?;
        let revision = RevisionId::parse(revision)?;
        let state = self.inner.lock().unwrap();
        let found = state
            .active
            .iter()
            .chain(state.retained.iter())
            .find(|d| {
                d.document.document_id == document_id.as_str()
                    && d.document.revision == revision.as_str()
            })
            .cloned();
        found.ok_or_else(|| {
            if state
                .active
                .iter()
                .any(|d| d.document.document_id != document_id.as_str())
            {
                CoordinationError::WrongDocument
            } else if state
                .evictions
                .iter()
                .any(|e| e.document_id == document_id.as_str() && e.revision == revision.as_str())
            {
                CoordinationError::EvictedRevision
            } else {
                CoordinationError::RevisionUnavailable
            }
        })
    }

    /// Recorded historical evictions (oldest first), bounded separately from retention.
    pub fn evictions(&self) -> Vec<Eviction> {
        self.inner
            .lock()
            .unwrap()
            .evictions
            .iter()
            .cloned()
            .collect()
    }

    /// Bounded markdown projection of the active SemanticDocument for companion tools.
    pub fn render(&self, limit: usize) -> Result<String, CoordinationError> {
        let output =
            render_semantic_markdown(&self.active()?).map_err(|_| CoordinationError::NoDocument)?;
        Ok(output.content.chars().take(limit).collect())
    }

    /// Bounded outline projection of the active SemanticDocument for companion tools.
    pub fn outline(&self, limit: usize) -> Result<String, CoordinationError> {
        let output = render_outline(&self.active()?).map_err(|_| CoordinationError::NoDocument)?;
        Ok(output.content.chars().take(limit).collect())
    }

    /// Human selection as an exact `semantic_ref` against the active document.
    pub fn selection(&self) -> Option<SemanticRef> {
        self.inner.lock().unwrap().selection.clone()
    }

    /// Set human selection after resolving the ref against the active document.
    ///
    /// Does not mutate agent attention. Fails closed on missing/stale/unknown refs.
    pub fn set_selection(&self, reference: SemanticRef) -> Result<(), CoordinationError> {
        let mut s = self.inner.lock().unwrap();
        let d = s.active.as_ref().ok_or(CoordinationError::NoDocument)?;
        d.resolve(&reference).map_err(map_ref_error)?;
        s.selection = Some(reference);
        Ok(())
    }

    /// Current agent attention spotlight (independent of human selection).
    pub fn attention(&self) -> Attention {
        self.inner.lock().unwrap().attention.clone()
    }

    /// Set agent attention to an exact, currently resolvable `semantic_ref`.
    /// Does not mutate human selection or Chrome.
    pub fn set_attention(
        &self,
        reference: SemanticRef,
        message: Option<String>,
    ) -> Result<(), CoordinationError> {
        let message = bound_attention_message(message)?;
        let mut s = self.inner.lock().unwrap();
        let d = s.active.as_ref().ok_or(CoordinationError::NoDocument)?;
        d.resolve(&reference).map_err(map_ref_error)?;
        s.attention = Attention {
            document_id: Some(d.document.document_id.clone()),
            revision: Some(d.document.revision.clone()),
            semantic_ref: Some(reference),
            message,
        };
        Ok(())
    }

    /// Clear agent attention without touching human selection or the browser.
    pub fn clear_attention(&self) {
        self.inner.lock().unwrap().attention = Attention::default();
    }
}

/// Consistent read of lifecycle, document, selection, and attention.
#[derive(Debug, Clone)]
pub struct CoordinationSnapshot {
    pub lifecycle: Lifecycle,
    pub active: Option<SemanticDocument>,
    pub selection: Option<SemanticRef>,
    pub attention: Attention,
    pub retained: Vec<SemanticDocument>,
}

impl CoordinationSnapshot {
    /// Document content is available through Loading/Error, but never claims Ready
    /// when the coordination lifecycle is still in flight.
    pub fn claims_ready(&self) -> bool {
        matches!(self.lifecycle, Lifecycle::Ready)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::backend::FakeSessionBackend;
    use crate::dom::DocumentMetadata;

    fn doc(document_id: &str, revision: &str) -> SemanticDocument {
        SemanticDocument::empty(DocumentMetadata {
            document_id: document_id.into(),
            revision: revision.into(),
            url: format!("https://example.test/{revision}"),
            title: revision.into(),
            ready_state: "complete".into(),
            frames: vec![],
        })
        .expect("semantic document")
    }

    fn shared_with_limit(limit: usize) -> SharedTuiState {
        SharedTuiState::with_retention(limit)
    }

    #[test]
    fn retention_evicts_oldest_and_never_falls_through_to_active() {
        let shared = shared_with_limit(2);
        shared.publish(doc("tab", "r1"));
        shared.publish(doc("tab", "r2"));
        shared.publish(doc("tab", "r3"));
        let eviction = shared.publish(doc("tab", "r4"));
        assert_eq!(
            eviction,
            Some(Eviction {
                document_id: "tab".into(),
                revision: "r1".into(),
            })
        );
        assert_eq!(shared.active().unwrap().document.revision, "r4");
        assert_eq!(
            shared
                .retained()
                .iter()
                .map(|d| d.document.revision.as_str())
                .collect::<Vec<_>>(),
            vec!["r2", "r3"]
        );
        assert_eq!(
            shared.revision("tab", "r1"),
            Err(CoordinationError::EvictedRevision)
        );
        assert_eq!(
            shared.revision("tab", "r2").unwrap().document.revision,
            "r2"
        );
        assert_eq!(
            shared.revision("other", "r4"),
            Err(CoordinationError::WrongDocument)
        );
        assert_eq!(
            shared.revision("tab", "missing"),
            Err(CoordinationError::RevisionUnavailable)
        );
        assert_eq!(
            RevisionId::parse("bad/rev"),
            Err(CoordinationError::MalformedRevision)
        );
    }

    #[test]
    fn loading_reads_keep_last_document_and_do_not_claim_ready() {
        let shared = shared_with_limit(4);
        shared.publish(doc("tab", "ready-1"));
        shared.begin_page_action("navigate").expect("claim loading");

        let snapshot = shared.read_snapshot();
        assert!(matches!(snapshot.lifecycle, Lifecycle::Loading { .. }));
        assert!(!snapshot.claims_ready());
        assert_eq!(
            snapshot
                .active
                .as_ref()
                .map(|d| d.document.revision.as_str()),
            Some("ready-1")
        );
        assert_eq!(
            shared.active().unwrap().document.revision,
            "ready-1",
            "last complete capture remains visible without mixing revisions"
        );
        assert_eq!(
            shared.ensure_page_action_available(),
            Err(CoordinationError::ActionInProgress)
        );
        shared.activate_runtime();
        assert_eq!(
            crate::tui::PageCoordinator::new(
                Arc::new(crate::browser::BrowserSession::with_test_backend(
                    FakeSessionBackend::new()
                )),
                shared.clone()
            )
            .refresh(),
            Err(CoordinationError::RefreshInProgress)
        );
    }

    #[test]
    fn companion_refresh_publishes_the_post_capture_document_atomically() {
        let shared = shared_with_limit(4);
        shared.activate_runtime();
        shared.publish(doc("fake-tab-1", "fake:1"));

        let coordinator = crate::tui::PageCoordinator::new(
            Arc::new(crate::browser::BrowserSession::with_test_backend(
                FakeSessionBackend::with_semantic_capture_revision_bump(),
            )),
            shared.clone(),
        );
        let refreshed = coordinator.refresh().expect("companion refresh");

        assert!(shared.lifecycle().is_ready());
        let active = shared.active().expect("published capture");
        assert_eq!(active.document.document_id, refreshed.document_id);
        assert_eq!(active.document.revision, refreshed.revision);
        assert_eq!(active.document.url, refreshed.url);
        assert_eq!(active.document.title, refreshed.title);
        assert_eq!(active.document.revision, "fake:3");
    }

    #[test]
    fn clear_error_restores_ready_without_dropping_document() {
        let shared = shared_with_limit(4);
        shared.publish(doc("tab", "ready-1"));
        let ticket = shared.begin_page_action("history_back").unwrap();
        shared
            .fail_page_action(ticket, "history_back", "settle timeout")
            .unwrap();
        assert!(matches!(shared.lifecycle(), Lifecycle::Error { .. }));
        assert!(shared.clear_error());
        assert!(shared.lifecycle().is_ready());
        assert_eq!(shared.active().unwrap().document.revision, "ready-1");
        assert!(!shared.clear_error());
        shared.begin_page_action("navigate").expect("loading");
        assert!(!shared.clear_error());
        assert!(shared.lifecycle().is_loading());
    }

    #[test]
    fn transaction_tickets_reject_concurrency_and_stale_completion() {
        let shared = shared_with_limit(4);
        let first = shared.begin_page_action("first").unwrap();
        assert_eq!(
            shared.begin_page_action("concurrent"),
            Err(CoordinationError::ActionInProgress)
        );
        shared.finish_page_action_retained(first).unwrap();
        let second = shared.begin_page_action("second").unwrap();
        assert_eq!(
            shared.commit_page_action(first, doc("tab", "stale"), None),
            Err(CoordinationError::StaleTransaction)
        );
        assert_eq!(
            shared.fail_page_action(first, "first", "late"),
            Err(CoordinationError::StaleTransaction)
        );
        assert_eq!(
            shared.finish_page_action_retained(first),
            Err(CoordinationError::StaleTransaction)
        );
        assert_eq!(
            shared.clear_session(first),
            Err(CoordinationError::StaleTransaction)
        );
        assert!(matches!(shared.lifecycle(), Lifecycle::Loading { action } if action == "second"));
        shared
            .commit_page_action(second, doc("tab", "current"), None)
            .unwrap();
        assert_eq!(shared.active().unwrap().document.revision, "current");
    }

    #[test]
    fn deactivation_is_atomic_with_companion_claims() {
        let shared = SharedTuiState::new();
        shared.activate_runtime();
        let ticket = shared.begin_companion_page_action("first").unwrap();
        shared.deactivate_runtime();
        // Work claimed before shutdown may finish; no later companion can claim.
        shared.finish_page_action_retained(ticket).unwrap();
        assert_eq!(
            shared.begin_companion_page_action("second"),
            Err(CoordinationError::RuntimeRequired)
        );
        assert!(shared.lifecycle().is_ready());
    }

    #[test]
    fn selection_and_attention_are_independently_owned() {
        use crate::semantic::normalize::{RawSemanticNode, normalize_fixture};

        let shared = shared_with_limit(2);
        assert_eq!(
            shared.set_selection(SemanticRef::from_opaque("sref1.nope")),
            Err(CoordinationError::NoDocument)
        );

        let document = normalize_fixture(
            DocumentMetadata {
                document_id: "tab".into(),
                revision: "r1".into(),
                url: "https://example.test/r1".into(),
                title: "r1".into(),
                ready_state: "complete".into(),
                frames: vec![],
            },
            vec![RawSemanticNode {
                kind: "text".into(),
                tag: Some("p".into()),
                id: Some("spotlight".into()),
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
        )
        .expect("doc");
        let reference = document.semantic_refs().into_iter().next().unwrap();
        shared.publish(document);
        shared
            .set_attention(reference.clone(), Some("agent focus".into()))
            .expect("set attention");
        assert_eq!(shared.attention().message.as_deref(), Some("agent focus"));
        assert_eq!(shared.attention().semantic_ref.as_ref(), Some(&reference));
        assert!(shared.selection().is_none());

        // Stale/unknown refs fail closed without clearing an existing spotlight
        // unless clear is explicit.
        assert_eq!(
            shared.set_attention(SemanticRef::from_opaque("not-a-ref"), None),
            Err(CoordinationError::MalformedReference)
        );
        assert!(shared.attention().is_set());

        shared.clear_attention();
        assert!(!shared.attention().is_set());
    }

    #[test]
    fn attention_message_is_bounded() {
        use crate::semantic::normalize::{RawSemanticNode, normalize_fixture};

        let shared = shared_with_limit(2);
        let document = normalize_fixture(
            DocumentMetadata {
                document_id: "tab".into(),
                revision: "r1".into(),
                url: "https://example.test/r1".into(),
                title: "r1".into(),
                ready_state: "complete".into(),
                frames: vec![],
            },
            vec![RawSemanticNode {
                kind: "text".into(),
                tag: Some("p".into()),
                id: Some("spotlight".into()),
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
        )
        .expect("doc");
        let reference = document.semantic_refs().into_iter().next().unwrap();
        shared.publish(document);
        let too_long = "x".repeat(MAX_ATTENTION_MESSAGE_CHARS + 1);
        assert_eq!(
            shared.set_attention(reference, Some(too_long)),
            Err(CoordinationError::MessageTooLong)
        );
    }
}

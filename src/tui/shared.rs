//! Shared in-process coordination for the terminal and the future co-hosted MCP server.

use crate::browser::BrowserSession;
use crate::semantic::{SemanticDocument, SemanticRef, render_outline, render_semantic_markdown};
use crate::tui::state::Lifecycle;
use std::collections::VecDeque;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

pub const DEFAULT_REVISION_RETENTION: usize = 8;

/// Maximum characters allowed in an agent attention message (tool + resource).
pub const MAX_ATTENTION_MESSAGE_CHARS: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinationError {
    NoDocument,
    MalformedDocumentId,
    MalformedRevision,
    RevisionUnavailable,
    WrongDocument,
    EvictedRevision,
    StaleReference,
    MalformedReference,
    UnknownReference,
    MessageTooLong,
    RuntimeRequired,
    ActionInProgress,
    RefreshInProgress,
    RefreshFailed,
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
        };
        f.write_str(message)
    }
}

impl std::error::Error for CoordinationError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionId(String);

impl RevisionId {
    pub fn parse(value: &str) -> Result<Self, CoordinationError> {
        if value.is_empty() || value.chars().any(|c| c.is_whitespace() || c == '/') {
            return Err(CoordinationError::MalformedRevision);
        }
        Ok(Self(value.to_owned()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentId(String);

impl DocumentId {
    pub fn parse(value: &str) -> Result<Self, CoordinationError> {
        if value.is_empty() || value.chars().any(|c| c.is_whitespace() || c == '/') {
            return Err(CoordinationError::MalformedDocumentId);
        }
        Ok(Self(value.to_owned()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Eviction {
    pub document_id: String,
    pub revision: String,
}

/// Agent-owned attention highlight, independent of human selection.
///
/// Always bound to an exact `semantic_ref` plus the document/revision that
/// validated it. Cleared when the reference no longer resolves after a publish.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Attention {
    pub semantic_ref: Option<SemanticRef>,
    pub document_id: Option<String>,
    pub revision: Option<String>,
    pub message: Option<String>,
}

impl Attention {
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
    lifecycle: Lifecycle,
    active: Option<SemanticDocument>,
    retained: VecDeque<SemanticDocument>,
    selection: Option<SemanticRef>,
    attention: Attention,
    limit: usize,
    evictions: VecDeque<Eviction>,
}

#[derive(Clone)]
pub struct SharedTuiState {
    session: Arc<Mutex<Option<Arc<BrowserSession>>>>,
    inner: Arc<Mutex<Inner>>,
    runtime_active: Arc<AtomicBool>,
}

impl SharedTuiState {
    pub fn new(session: Arc<BrowserSession>) -> Self {
        Self::with_retention(session, DEFAULT_REVISION_RETENTION)
    }
    pub fn with_retention(session: Arc<BrowserSession>, limit: usize) -> Self {
        Self::with_optional_session(Some(session), limit)
    }
    /// Construct the coordination object before the BrowserSession registry is
    /// sealed, then bind the one live session exactly once.
    pub fn unbound() -> Self {
        Self::with_optional_session(None, DEFAULT_REVISION_RETENTION)
    }
    fn with_optional_session(session: Option<Arc<BrowserSession>>, limit: usize) -> Self {
        Self {
            session: Arc::new(Mutex::new(session)),
            inner: Arc::new(Mutex::new(Inner {
                lifecycle: Lifecycle::Ready,
                active: None,
                retained: VecDeque::new(),
                selection: None,
                attention: Attention::default(),
                limit: limit.max(1),
                evictions: VecDeque::new(),
            })),
            runtime_active: Arc::new(AtomicBool::new(false)),
        }
    }
    pub fn bind_session(&self, session: Arc<BrowserSession>) -> Result<(), CoordinationError> {
        let mut current = self.session.lock().unwrap();
        if current.is_some() {
            return Err(CoordinationError::RuntimeRequired);
        }
        *current = Some(session);
        Ok(())
    }
    pub fn session(&self) -> Result<Arc<BrowserSession>, CoordinationError> {
        self.session
            .lock()
            .unwrap()
            .clone()
            .ok_or(CoordinationError::RuntimeRequired)
    }
    /// Mark the interactive runtime as ready to accept companion operations.
    /// Standard stdio sessions never call this, so their registry stays separate.
    pub fn activate_runtime(&self) {
        self.runtime_active.store(true, Ordering::Release);
    }

    /// Stop accepting companion state changes before the terminal or HTTP host
    /// is torn down.
    pub fn deactivate_runtime(&self) {
        self.runtime_active.store(false, Ordering::Release);
    }

    pub fn runtime_active(&self) -> bool {
        self.runtime_active.load(Ordering::Acquire)
    }
    pub fn lifecycle(&self) -> Lifecycle {
        self.inner.lock().unwrap().lifecycle.clone()
    }

    /// Claim the one browser/semantic lifecycle before starting any page
    /// action. Both the terminal controller and companion tools use this
    /// transition, so a second actor cannot touch the shared BrowserSession
    /// while a Loading page is awaiting capture.
    pub fn begin_page_action(&self, action: impl Into<String>) -> Result<(), CoordinationError> {
        let mut state = self.inner.lock().unwrap();
        if state.lifecycle.is_loading() {
            return Err(CoordinationError::ActionInProgress);
        }
        state.lifecycle = Lifecycle::Loading {
            action: action.into(),
        };
        Ok(())
    }

    /// Atomically expose a terminal/companion action failure while retaining
    /// the last complete semantic document.
    pub fn fail_page_action(&self, action: impl Into<String>, message: impl Into<String>) {
        self.inner.lock().unwrap().lifecycle = Lifecycle::Error {
            action: action.into(),
            message: message.into(),
        };
    }

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

    pub fn evictions(&self) -> Vec<Eviction> {
        self.inner
            .lock()
            .unwrap()
            .evictions
            .iter()
            .cloned()
            .collect()
    }
    pub fn render(&self, limit: usize) -> Result<String, CoordinationError> {
        let output =
            render_semantic_markdown(&self.active()?).map_err(|_| CoordinationError::NoDocument)?;
        Ok(output.content.chars().take(limit).collect())
    }
    pub fn outline(&self, limit: usize) -> Result<String, CoordinationError> {
        let output = render_outline(&self.active()?).map_err(|_| CoordinationError::NoDocument)?;
        Ok(output.content.chars().take(limit).collect())
    }

    /// Reload through the one live browser session, settle it, capture a fresh
    /// semantic document, and publish that complete capture atomically. This
    /// intentionally does not reuse rendering: a companion without an active
    /// terminal runtime fails explicitly instead of mutating detached state.
    pub fn refresh(&self) -> Result<RefreshPage, CoordinationError> {
        if !self.runtime_active() {
            return Err(CoordinationError::RuntimeRequired);
        }

        self.begin_page_action("refresh")
            .map_err(|error| match error {
                CoordinationError::ActionInProgress => CoordinationError::RefreshInProgress,
                other => other,
            })?;

        let session = match self.session() {
            Ok(session) => session,
            Err(error) => {
                self.fail_page_action("refresh", error.to_string());
                return Err(error);
            }
        };
        if let Err(error) = session
            .evaluate("location.reload()", false)
            .map_err(|_| CoordinationError::RefreshFailed)
        {
            self.fail_page_action("refresh", error.to_string());
            return Err(error);
        }
        self.complete_page_action_after_browser_work("refresh")
    }

    /// After a companion (or terminal-owned) browser mutation has finished, wait
    /// for settle, capture a fresh semantic document, and publish Ready atomically.
    /// On failure, publishes Error while retaining the last valid document.
    pub fn complete_page_action_after_browser_work(
        &self,
        action: impl Into<String>,
    ) -> Result<RefreshPage, CoordinationError> {
        let action = action.into();
        if !self.lifecycle().is_loading() {
            return Err(CoordinationError::RuntimeRequired);
        }

        let result = (|| {
            let session = self.session()?;
            session
                .wait_for_document_ready_with_timeout(Duration::from_secs(15))
                .map_err(|_| CoordinationError::RefreshFailed)?;
            // Capture before sampling the freshness barrier. Hydration may
            // legitimately change the document after settling but before the
            // semantic snapshot begins; only a change *after* that capture
            // makes the snapshot unsafe to publish.
            let document = session
                .extract_semantic_document()
                .map_err(|_| CoordinationError::RefreshFailed)?;
            let metadata = session
                .document_metadata()
                .map_err(|_| CoordinationError::RefreshFailed)?;
            if metadata.document_id.is_empty()
                || metadata.revision.is_empty()
                || metadata.ready_state != "complete"
            {
                return Err(CoordinationError::RefreshFailed);
            }
            if !crate::semantic::capture_matches_document_metadata(&document.document, &metadata) {
                return Err(CoordinationError::RefreshFailed);
            }
            Ok(document)
        })();

        match result {
            Ok(document) => {
                let page = RefreshPage {
                    document_id: document.document.document_id.clone(),
                    revision: document.document.revision.clone(),
                    url: document.document.url.clone(),
                    title: document.document.title.clone(),
                };
                self.publish(document);
                Ok(page)
            }
            Err(error) => {
                self.fail_page_action(action, error.to_string());
                Err(error)
            }
        }
    }

    pub fn selection(&self) -> Option<SemanticRef> {
        self.inner.lock().unwrap().selection.clone()
    }
    pub fn set_selection(&self, reference: SemanticRef) -> Result<(), CoordinationError> {
        let mut s = self.inner.lock().unwrap();
        let d = s.active.as_ref().ok_or(CoordinationError::NoDocument)?;
        d.resolve(&reference).map_err(map_ref_error)?;
        s.selection = Some(reference);
        Ok(())
    }
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
        shared_with_backend(FakeSessionBackend::new(), limit)
    }

    fn shared_with_backend(backend: FakeSessionBackend, limit: usize) -> SharedTuiState {
        SharedTuiState::with_retention(Arc::new(BrowserSession::with_test_backend(backend)), limit)
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
        assert_eq!(shared.refresh(), Err(CoordinationError::RefreshInProgress));
    }

    #[test]
    fn companion_refresh_publishes_the_post_capture_document_atomically() {
        let shared =
            shared_with_backend(FakeSessionBackend::with_semantic_capture_revision_bump(), 4);
        shared.activate_runtime();
        shared.publish(doc("fake-tab-1", "fake:1"));

        let refreshed = shared.refresh().expect("companion refresh");

        assert!(shared.lifecycle().is_ready());
        let active = shared.active().expect("published capture");
        assert_eq!(active.document.document_id, refreshed.document_id);
        assert_eq!(active.document.revision, refreshed.revision);
        assert_eq!(active.document.url, refreshed.url);
        assert_eq!(active.document.title, refreshed.title);
        assert_eq!(active.document.revision, "fake:3");
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

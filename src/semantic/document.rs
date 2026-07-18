//! Bounded semantic document with revision-scoped reference index.

use crate::dom::DocumentMetadata;
use crate::error::{BrowserError, Result};
use crate::semantic::component::SemanticComponent;
use crate::semantic::identity::{
    SemanticIdentity, SemanticRef, SemanticRefError, SemanticRefPayload,
};
use crate::semantic::limits::{
    validate_component_count, validate_depth, validate_semantic_string, validate_total_text_chars,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// One bounded, revision-identified semantic capture of the hydrated page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticDocument {
    /// Document metadata and revision shared with the browser session.
    pub document: DocumentMetadata,
    /// Top-level semantic components in document order.
    pub roots: Vec<SemanticComponent>,
    /// Precomputed index from opaque refs to depth-first component positions.
    #[serde(skip)]
    ref_index: HashMap<SemanticRef, Vec<usize>>,
    /// Identity-key index for detecting capture-time collisions.
    #[serde(skip)]
    identity_index: HashMap<SemanticIdentity, SemanticRef>,
}

impl SemanticDocument {
    /// Build a document from metadata and already-normalized roots, assigning refs and indexes.
    pub fn from_components(
        document: DocumentMetadata,
        mut roots: Vec<SemanticComponent>,
    ) -> Result<Self> {
        validate_document_metadata_strings(&document)?;

        let mut identity_index = HashMap::new();
        let mut ref_index = HashMap::new();
        let mut component_count = 0usize;
        let mut total_text = 0usize;
        let mut state = IndexingState {
            identity_index: &mut identity_index,
            ref_index: &mut ref_index,
            component_count: &mut component_count,
            total_text: &mut total_text,
        };

        for (root_index, root) in roots.iter_mut().enumerate() {
            index_component(root, &document, &mut state, 1, vec![root_index])?;
        }

        validate_component_count(component_count)?;
        validate_total_text_chars(total_text)?;

        Ok(Self {
            document,
            roots,
            ref_index,
            identity_index,
        })
    }

    /// Empty document for the provided metadata.
    pub fn empty(document: DocumentMetadata) -> Result<Self> {
        Self::from_components(document, Vec::new())
    }

    /// Resolve an opaque `semantic_ref` fail-closed against this document revision.
    ///
    /// Never retargets by text similarity. Wrong document, stale revision, unknown
    /// identity, ambiguity, or a malformed token each yield a distinct
    /// [`SemanticRefError`] without guessing a substitute component.
    pub fn resolve(
        &self,
        semantic_ref: &SemanticRef,
    ) -> std::result::Result<&SemanticComponent, SemanticRefError> {
        let path = self.resolve_path(semantic_ref)?;
        self.component_at_path(&path)
            .ok_or(SemanticRefError::Unknown)
    }

    /// Resolve a raw opaque token fail-closed (same contract as [`Self::resolve`]).
    pub fn resolve_str(
        &self,
        token: &str,
    ) -> std::result::Result<&SemanticComponent, SemanticRefError> {
        self.resolve(&SemanticRef::from_opaque(token))
    }

    /// All opaque references present in document order (depth-first).
    pub fn semantic_refs(&self) -> Vec<SemanticRef> {
        let mut refs = Vec::with_capacity(self.ref_index.len());
        for root in &self.roots {
            root.walk(&mut |component| {
                refs.push(component.semantic_ref.clone());
            });
        }
        refs
    }

    /// Total number of components in the tree.
    pub fn component_count(&self) -> usize {
        self.ref_index.len()
    }

    /// Depth-first iterator over all components.
    pub fn components(&self) -> SemanticComponentIter<'_> {
        SemanticComponentIter {
            stack: self.roots.iter().rev().collect(),
        }
    }

    /// Resolve a reference from a prior capture by durable identity only.
    ///
    /// Used for viewport anchor restoration and selection rebinding after a
    /// successful recapture. The prior token's revision is ignored; identity
    /// must match exactly. Missing identities fail closed as [`SemanticRefError::Unknown`].
    pub fn resolve_surviving(
        &self,
        previous: &SemanticRef,
    ) -> std::result::Result<&SemanticComponent, SemanticRefError> {
        let payload = previous.decode()?;
        if payload.document_id != self.document.document_id {
            return Err(SemanticRefError::WrongDocument {
                expected: self.document.document_id.clone(),
                actual: payload.document_id,
            });
        }
        let current_ref = self
            .identity_index
            .get(&payload.identity)
            .ok_or(SemanticRefError::Unknown)?;
        // `current_ref` is minted for this document revision, so resolve is exact.
        self.resolve(current_ref)
    }

    /// Like [`Self::resolve_surviving`], returning the current opaque ref on success.
    pub fn rebind_surviving(
        &self,
        previous: &SemanticRef,
    ) -> std::result::Result<SemanticRef, SemanticRefError> {
        let component = self.resolve_surviving(previous)?;
        Ok(component.semantic_ref.clone())
    }

    fn resolve_path(
        &self,
        semantic_ref: &SemanticRef,
    ) -> std::result::Result<Vec<usize>, SemanticRefError> {
        let payload = semantic_ref.decode()?;

        if payload.document_id != self.document.document_id {
            return Err(SemanticRefError::WrongDocument {
                expected: self.document.document_id.clone(),
                actual: payload.document_id,
            });
        }

        if payload.revision != self.document.revision {
            return Err(SemanticRefError::Stale {
                expected_revision: self.document.revision.clone(),
                actual_revision: payload.revision,
            });
        }

        match self.ref_index.get(semantic_ref) {
            Some(path) => Ok(path.clone()),
            None => {
                // Identity may have been re-encoded with matching document/revision but
                // the exact opaque token is not in this capture.
                if self.identity_index.contains_key(&payload.identity) {
                    // Same identity minted with a different opaque encoding for this doc/rev
                    // is still unknown for the supplied token.
                    Err(SemanticRefError::Unknown)
                } else {
                    Err(SemanticRefError::Unknown)
                }
            }
        }
    }

    fn component_at_path(&self, path: &[usize]) -> Option<&SemanticComponent> {
        let mut iter = path.iter();
        let first = *iter.next()?;
        let mut current = self.roots.get(first)?;
        for index in iter {
            current = current.children.get(*index)?;
        }
        Some(current)
    }
}

/// Depth-first iterator over components in a semantic document.
pub struct SemanticComponentIter<'a> {
    stack: Vec<&'a SemanticComponent>,
}

impl<'a> Iterator for SemanticComponentIter<'a> {
    type Item = &'a SemanticComponent;

    fn next(&mut self) -> Option<Self::Item> {
        let component = self.stack.pop()?;
        for child in component.children.iter().rev() {
            self.stack.push(child);
        }
        Some(component)
    }
}

/// Mutable indexes and budgets shared while walking a semantic component tree.
struct IndexingState<'a> {
    identity_index: &'a mut HashMap<SemanticIdentity, SemanticRef>,
    ref_index: &'a mut HashMap<SemanticRef, Vec<usize>>,
    component_count: &'a mut usize,
    total_text: &'a mut usize,
}

fn index_component(
    component: &mut SemanticComponent,
    document: &DocumentMetadata,
    state: &mut IndexingState<'_>,
    depth: usize,
    path: Vec<usize>,
) -> Result<()> {
    validate_depth(depth)?;
    *state.component_count += 1;
    validate_component_count(*state.component_count)?;

    if let Some(label) = &component.label {
        validate_semantic_string("label", label)?;
        *state.total_text += label.chars().count();
    }
    if let Some(text) = &component.text {
        validate_semantic_string("text", text)?;
        *state.total_text += text.chars().count();
    }
    accumulate_attr_text(&component.attrs, state.total_text)?;
    validate_total_text_chars(*state.total_text)?;

    // Components arrive with a provisional identity encoded in semantic_ref by the normalizer.
    let payload = component.semantic_ref.decode().map_err(|err| {
        BrowserError::DomParseFailed(format!("invalid provisional semantic_ref: {err}"))
    })?;

    if payload.document_id != document.document_id || payload.revision != document.revision {
        // Rebind provisional identity into this document revision.
        let rebound = SemanticRef::encode(&SemanticRefPayload {
            document_id: document.document_id.clone(),
            revision: document.revision.clone(),
            identity: payload.identity.clone(),
        });
        component.semantic_ref = rebound;
    }

    let final_payload = component.semantic_ref.decode().map_err(|err| {
        BrowserError::DomParseFailed(format!("invalid semantic_ref after rebind: {err}"))
    })?;

    if let Some(existing) = state.identity_index.get(&final_payload.identity) {
        if existing != &component.semantic_ref {
            return Err(BrowserError::DomParseFailed(
                "ambiguous semantic identity during capture".to_string(),
            ));
        }
    } else {
        state.identity_index.insert(
            final_payload.identity.clone(),
            component.semantic_ref.clone(),
        );
    }

    if let Some(existing_path) = state.ref_index.get(&component.semantic_ref) {
        if existing_path != &path {
            return Err(BrowserError::DomParseFailed(
                "duplicate semantic_ref during capture".to_string(),
            ));
        }
    } else {
        state
            .ref_index
            .insert(component.semantic_ref.clone(), path.clone());
    }

    for (child_index, child) in component.children.iter_mut().enumerate() {
        let mut child_path = path.clone();
        child_path.push(child_index);
        index_component(child, document, state, depth + 1, child_path)?;
    }

    Ok(())
}

fn accumulate_attr_text(
    attrs: &crate::semantic::component::SemanticAttrs,
    total_text: &mut usize,
) -> Result<()> {
    for (field, value) in [
        ("href", attrs.href.as_deref()),
        ("src", attrs.src.as_deref()),
        ("alt", attrs.alt.as_deref()),
        ("name", attrs.name.as_deref()),
        ("value", attrs.value.as_deref()),
        ("input_type", attrs.input_type.as_deref()),
        ("placeholder", attrs.placeholder.as_deref()),
        ("button_type", attrs.button_type.as_deref()),
        ("tag", attrs.tag.as_deref()),
    ] {
        if let Some(value) = value {
            validate_semantic_string(field, value)?;
            *total_text += value.chars().count();
        }
    }

    for option in &attrs.options {
        validate_semantic_string("option.value", &option.value)?;
        *total_text += option.value.chars().count();
        if let Some(label) = &option.label {
            validate_semantic_string("option.label", label)?;
            *total_text += label.chars().count();
        }
    }

    Ok(())
}

fn validate_document_metadata_strings(document: &DocumentMetadata) -> Result<()> {
    validate_semantic_string("document_id", &document.document_id)?;
    validate_semantic_string("revision", &document.revision)?;
    validate_semantic_string("url", &document.url)?;
    validate_semantic_string("title", &document.title)?;
    validate_semantic_string("ready_state", &document.ready_state)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic::component::{SemanticAttrs, SemanticKind};
    use crate::semantic::identity::{SemanticIdentity, SemanticRef, SemanticRefPayload};

    fn meta(doc: &str, rev: &str) -> DocumentMetadata {
        DocumentMetadata {
            document_id: doc.to_string(),
            revision: rev.to_string(),
            url: "https://example.com/page".to_string(),
            title: "Example".to_string(),
            ready_state: "complete".to_string(),
            frames: Vec::new(),
        }
    }

    fn text_component(
        doc: &str,
        rev: &str,
        identity: SemanticIdentity,
        text: &str,
    ) -> SemanticComponent {
        SemanticComponent {
            semantic_ref: SemanticRef::encode(&SemanticRefPayload {
                document_id: doc.to_string(),
                revision: rev.to_string(),
                identity,
            }),
            kind: SemanticKind::Text,
            label: None,
            text: Some(text.to_string()),
            attrs: SemanticAttrs::default(),
            interaction_selector: None,
            children: Vec::new(),
        }
    }

    #[test]
    fn resolve_rejects_wrong_document_and_stale_revision() {
        let document = SemanticDocument::from_components(
            meta("doc-a", "rev-1"),
            vec![text_component(
                "doc-a",
                "rev-1",
                SemanticIdentity::author_id("t1"),
                "hello",
            )],
        )
        .expect("document");

        let ok_ref = document.semantic_refs().into_iter().next().expect("ref");
        assert!(document.resolve(&ok_ref).is_ok());

        let wrong_doc = SemanticRef::encode(&SemanticRefPayload {
            document_id: "doc-b".to_string(),
            revision: "rev-1".to_string(),
            identity: SemanticIdentity::author_id("t1"),
        });
        assert!(matches!(
            document.resolve(&wrong_doc),
            Err(SemanticRefError::WrongDocument { .. })
        ));

        let stale = SemanticRef::encode(&SemanticRefPayload {
            document_id: "doc-a".to_string(),
            revision: "rev-2".to_string(),
            identity: SemanticIdentity::author_id("t1"),
        });
        assert!(matches!(
            document.resolve(&stale),
            Err(SemanticRefError::Stale { .. })
        ));
    }

    #[test]
    fn resolve_rejects_unknown_and_malformed() {
        let document = SemanticDocument::from_components(
            meta("doc-a", "rev-1"),
            vec![text_component(
                "doc-a",
                "rev-1",
                SemanticIdentity::author_id("t1"),
                "hello",
            )],
        )
        .expect("document");

        let unknown = SemanticRef::encode(&SemanticRefPayload {
            document_id: "doc-a".to_string(),
            revision: "rev-1".to_string(),
            identity: SemanticIdentity::author_id("missing"),
        });
        assert_eq!(document.resolve(&unknown), Err(SemanticRefError::Unknown));
        assert_eq!(
            document.resolve_str("garbage"),
            Err(SemanticRefError::Malformed)
        );
    }

    #[test]
    fn resolve_surviving_matches_identity_across_revisions() {
        let first = SemanticDocument::from_components(
            meta("doc-a", "rev-1"),
            vec![text_component(
                "doc-a",
                "rev-1",
                SemanticIdentity::author_id("anchor"),
                "hello",
            )],
        )
        .expect("first");
        let old_ref = first.semantic_refs().into_iter().next().expect("ref");

        let second = SemanticDocument::from_components(
            meta("doc-a", "rev-2"),
            vec![text_component(
                "doc-a",
                "rev-2",
                SemanticIdentity::author_id("anchor"),
                "hello again",
            )],
        )
        .expect("second");

        let surviving = second.resolve_surviving(&old_ref).expect("survives");
        assert_eq!(surviving.text.as_deref(), Some("hello again"));
        assert_ne!(surviving.semantic_ref, old_ref);

        let rebound = second.rebind_surviving(&old_ref).expect("rebind");
        assert_eq!(rebound, surviving.semantic_ref);
    }

    #[test]
    fn resolve_surviving_fails_closed_when_identity_absent() {
        let first = SemanticDocument::from_components(
            meta("doc-a", "rev-1"),
            vec![text_component(
                "doc-a",
                "rev-1",
                SemanticIdentity::author_id("gone"),
                "hello",
            )],
        )
        .expect("first");
        let old_ref = first.semantic_refs().into_iter().next().expect("ref");

        let second = SemanticDocument::from_components(
            meta("doc-a", "rev-2"),
            vec![text_component(
                "doc-a",
                "rev-2",
                SemanticIdentity::author_id("other"),
                "different",
            )],
        )
        .expect("second");

        assert_eq!(
            second.resolve_surviving(&old_ref),
            Err(SemanticRefError::Unknown)
        );
    }

    #[test]
    fn resolve_surviving_rejects_same_identity_from_another_document() {
        let first = SemanticDocument::from_components(
            meta("doc-a", "rev-1"),
            vec![text_component(
                "doc-a",
                "rev-1",
                SemanticIdentity::author_id("shared"),
                "one",
            )],
        )
        .expect("first");
        let second = SemanticDocument::from_components(
            meta("doc-b", "rev-2"),
            vec![text_component(
                "doc-b",
                "rev-2",
                SemanticIdentity::author_id("shared"),
                "two",
            )],
        )
        .expect("second");
        assert!(matches!(
            second.resolve_surviving(&first.semantic_refs()[0]),
            Err(SemanticRefError::WrongDocument { .. })
        ));
    }
}

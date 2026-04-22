use crate::dom::element::{AriaChild, AriaNode};
use crate::error::{BrowserError, Result};
use headless_chrome::Tab;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Revision-scoped reference to an actionable node in a snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct NodeRef {
    pub document_id: String,
    pub revision: String,
    pub index: usize,
}

/// Reusable handoff payload for a resolved actionable node.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct Cursor {
    pub node_ref: NodeRef,
    pub selector: String,
    pub index: usize,
    pub role: String,
    pub name: String,
}

/// Metadata about an iframe encountered during extraction.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, Default)]
pub struct FrameMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
}

/// Metadata for the extracted document.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, Default)]
pub struct DocumentMetadata {
    pub document_id: String,
    pub revision: String,
    pub url: String,
    pub title: String,
    pub ready_state: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frames: Vec<FrameMetadata>,
}

/// Agent-facing summary of one actionable node in the current snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct SnapshotNode {
    pub cursor: Cursor,
    pub node_ref: NodeRef,
    pub index: usize,
    pub role: String,
    pub name: String,
}

/// Represents the ARIA snapshot of a web page
/// Based on Playwright's AriaSnapshot structure
#[derive(Debug, Clone)]
pub struct DomTree {
    /// Metadata for the extracted document.
    pub document: DocumentMetadata,

    /// Root AriaNode (usually a fragment)
    pub root: AriaNode,

    indexed: IndexedSnapshot,
}

#[derive(Debug, Clone, Default)]
struct IndexedSnapshot {
    records: BTreeMap<usize, IndexedNodeRecord>,
    frame_boundaries: Vec<FrameBoundaryRecord>,
}

#[derive(Debug, Clone)]
struct IndexedNodeRecord {
    selector: Option<String>,
    role: String,
    name: String,
    path: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FrameBoundaryRecord {
    index: usize,
}

/// Snapshot extraction response from JavaScript
#[derive(Debug, serde::Deserialize)]
struct SnapshotResponse {
    document: DocumentMetadata,
    root: AriaNode,
    selectors: Vec<String>,
    #[serde(rename = "iframe_indices", default)]
    _iframe_indices: Vec<usize>,
}

impl Default for SnapshotResponse {
    fn default() -> Self {
        Self {
            document: DocumentMetadata::default(),
            root: AriaNode::fragment(),
            selectors: Vec::new(),
            _iframe_indices: Vec::new(),
        }
    }
}

impl Default for DomTree {
    fn default() -> Self {
        Self {
            document: DocumentMetadata::default(),
            root: AriaNode::fragment(),
            indexed: IndexedSnapshot::default(),
        }
    }
}

impl IndexedSnapshot {
    fn from_root(root: &AriaNode, selector_overrides: &BTreeMap<usize, String>) -> Self {
        let mut snapshot = Self::default();
        let mut path = Vec::new();
        snapshot.collect(root, &mut path, selector_overrides);
        snapshot
    }

    fn collect(
        &mut self,
        node: &AriaNode,
        path: &mut Vec<usize>,
        selector_overrides: &BTreeMap<usize, String>,
    ) {
        if let Some(index) = node.index {
            let selector = selector_overrides
                .get(&index)
                .filter(|value| !value.is_empty())
                .cloned();

            self.records.insert(
                index,
                IndexedNodeRecord {
                    selector,
                    role: node.role.clone(),
                    name: node.name.clone(),
                    path: path.clone(),
                },
            );

            if node.role == "iframe" {
                self.frame_boundaries.push(FrameBoundaryRecord { index });
            }
        }

        for (child_position, child) in node.children.iter().enumerate() {
            if let AriaChild::Node(child_node) = child {
                path.push(child_position);
                self.collect(child_node, path, selector_overrides);
                path.pop();
            }
        }
    }

    fn selector_map(&self) -> BTreeMap<usize, String> {
        self.records
            .iter()
            .filter_map(|(index, record)| {
                record.selector.clone().map(|selector| (*index, selector))
            })
            .collect()
    }

    fn record(&self, index: usize) -> Option<&IndexedNodeRecord> {
        self.records.get(&index)
    }

    fn interactive_indices(&self) -> Vec<usize> {
        self.records.keys().copied().collect()
    }

    fn iframe_indices(&self) -> Vec<usize> {
        self.frame_boundaries
            .iter()
            .map(|record| record.index)
            .collect()
    }

    fn next_available_index(&self) -> usize {
        self.records
            .keys()
            .next_back()
            .map(|index| index + 1)
            .unwrap_or(0)
    }
}

impl DocumentMetadata {
    /// Read lightweight document metadata from the current tab without rebuilding the full DOM tree.
    pub fn from_tab(tab: &Arc<Tab>) -> Result<Self> {
        let result = tab
            .evaluate(include_str!("document_metadata.js"), false)
            .map_err(|e| {
                BrowserError::DomParseFailed(format!(
                    "Failed to execute document metadata script: {}",
                    e
                ))
            })?;

        let json_value = result.value.ok_or_else(|| {
            BrowserError::DomParseFailed(
                "No value returned from document metadata extraction".to_string(),
            )
        })?;

        let json_str: String = serde_json::from_value(json_value).map_err(|e| {
            BrowserError::DomParseFailed(format!("Failed to get metadata JSON string: {}", e))
        })?;

        serde_json::from_str(&json_str).map_err(|e| {
            BrowserError::DomParseFailed(format!("Failed to parse document metadata JSON: {}", e))
        })
    }
}

/// Snapshot extraction response from JavaScript
#[derive(Debug, serde::Deserialize)]
struct LegacySnapshotResponse {
    root: AriaNode,
    selectors: Vec<String>,
    #[serde(rename = "iframeIndices")]
    iframe_indices: Vec<usize>,
}

impl DomTree {
    fn selector_map_from_slots(selectors: Vec<String>) -> BTreeMap<usize, String> {
        selectors
            .into_iter()
            .enumerate()
            .filter_map(|(index, selector)| (!selector.is_empty()).then_some((index, selector)))
            .collect()
    }

    fn rebuild_indexed_from_selector_map(&mut self, selector_overrides: BTreeMap<usize, String>) {
        self.indexed = IndexedSnapshot::from_root(&self.root, &selector_overrides);
    }

    fn selector_map(&self) -> BTreeMap<usize, String> {
        self.indexed.selector_map()
    }

    fn node_at_path<'a>(node: &'a AriaNode, path: &[usize]) -> Option<&'a AriaNode> {
        let mut current = node;
        for &child_position in path {
            current = match current.children.get(child_position) {
                Some(AriaChild::Node(child_node)) => child_node,
                _ => return None,
            };
        }
        Some(current)
    }

    fn node_at_path_mut<'a>(node: &'a mut AriaNode, path: &[usize]) -> Option<&'a mut AriaNode> {
        if let Some((&child_position, rest)) = path.split_first() {
            match node.children.get_mut(child_position) {
                Some(AriaChild::Node(child_node)) => Self::node_at_path_mut(child_node, rest),
                _ => None,
            }
        } else {
            Some(node)
        }
    }

    fn rebase_children_indices(
        children: &mut [AriaChild],
        next_index: &mut usize,
        remapped_indices: &mut BTreeMap<usize, usize>,
    ) {
        for child in children {
            if let AriaChild::Node(child_node) = child {
                Self::rebase_node_indices(child_node, next_index, remapped_indices);
            }
        }
    }

    fn rebase_node_indices(
        node: &mut AriaNode,
        next_index: &mut usize,
        remapped_indices: &mut BTreeMap<usize, usize>,
    ) {
        if let Some(previous_index) = node.index {
            let rebased_index = *next_index;
            *next_index += 1;
            node.index = Some(rebased_index);
            remapped_indices.insert(previous_index, rebased_index);
        }

        Self::rebase_children_indices(&mut node.children, next_index, remapped_indices);
    }

    /// Create a new DomTree from an AriaNode
    pub fn new(root: AriaNode) -> Self {
        let indexed = IndexedSnapshot::from_root(&root, &BTreeMap::new());
        Self {
            document: DocumentMetadata::default(),
            root,
            indexed,
        }
    }

    /// Build DOM tree from a browser tab
    pub fn from_tab(tab: &Arc<Tab>) -> Result<Self> {
        Self::from_tab_with_prefix(tab, "")
    }

    /// Build DOM tree from a browser tab with a ref prefix (for iframe handling)
    pub fn from_tab_with_prefix(tab: &Arc<Tab>, _ref_prefix: &str) -> Result<Self> {
        // Note: ref_prefix is deprecated but kept for API compatibility
        // JavaScript code to extract ARIA snapshot
        let js_code = include_str!("extract_dom.js");

        // Execute JavaScript to extract DOM
        let result = tab.evaluate(js_code, false).map_err(|e| {
            BrowserError::DomParseFailed(format!("Failed to execute DOM extraction script: {}", e))
        })?;

        // Get the JSON string value
        let json_value = result.value.ok_or_else(|| {
            BrowserError::DomParseFailed("No value returned from DOM extraction".to_string())
        })?;

        // The JavaScript returns a JSON string, so we need to parse it as a string first
        let json_str: String = serde_json::from_value(json_value).map_err(|e| {
            BrowserError::DomParseFailed(format!("Failed to get JSON string: {}", e))
        })?;

        // Then parse the JSON string into SnapshotResponse
        let response: SnapshotResponse = match serde_json::from_str(&json_str) {
            Ok(response) => response,
            Err(_) => {
                let legacy: LegacySnapshotResponse =
                    serde_json::from_str(&json_str).map_err(|e| {
                        BrowserError::DomParseFailed(format!(
                            "Failed to parse snapshot JSON: {}",
                            e
                        ))
                    })?;
                SnapshotResponse {
                    document: DocumentMetadata::default(),
                    root: legacy.root,
                    selectors: legacy.selectors,
                    _iframe_indices: legacy.iframe_indices,
                }
            }
        };

        let SnapshotResponse {
            document,
            root,
            selectors,
            _iframe_indices: _,
        } = response;
        let indexed = IndexedSnapshot::from_root(&root, &Self::selector_map_from_slots(selectors));

        Ok(Self {
            document,
            root,
            indexed,
        })
    }

    /// Get CSS selector for a given index
    pub fn get_selector(&self, index: usize) -> Option<&String> {
        self.indexed.record(index)?.selector.as_ref()
    }

    /// Replace selector slots using legacy index-addressed input.
    pub fn replace_selectors(&mut self, selectors: Vec<String>) {
        self.rebuild_indexed_from_selector_map(Self::selector_map_from_slots(selectors));
    }

    /// Set or clear the selector for a specific actionable index.
    pub fn set_selector(&mut self, index: usize, selector: impl Into<String>) {
        let selector = selector.into();
        let mut selectors = self.selector_map();
        if selector.is_empty() {
            selectors.remove(&index);
        } else {
            selectors.insert(index, selector);
        }
        self.rebuild_indexed_from_selector_map(selectors);
    }

    /// Build a revision-scoped node reference for an actionable index.
    pub fn node_ref_for_index(&self, index: usize) -> Option<NodeRef> {
        self.indexed.record(index).map(|_| NodeRef {
            document_id: self.document.document_id.clone(),
            revision: self.document.revision.clone(),
            index,
        })
    }

    /// Build a reusable cursor for an actionable index.
    pub fn cursor_for_index(&self, index: usize) -> Option<Cursor> {
        let record = self.indexed.record(index)?;
        let selector = record.selector.clone()?;

        Some(Cursor {
            node_ref: self.node_ref_for_index(index)?,
            selector,
            index,
            role: record.role.clone(),
            name: record.name.clone(),
        })
    }

    /// Return the actionable cursors whose selector matches the provided selector exactly.
    pub fn cursors_for_selector(&self, selector: &str) -> Vec<Cursor> {
        self.indexed
            .records
            .iter()
            .filter_map(|(index, record)| {
                (record.selector.as_deref() == Some(selector))
                    .then(|| self.cursor_for_index(*index))
            })
            .flatten()
            .collect()
    }

    /// Return the first actionable cursor whose selector matches the provided selector.
    pub fn cursor_for_selector(&self, selector: &str) -> Option<Cursor> {
        self.cursors_for_selector(selector).into_iter().next()
    }

    /// Collect the actionable nodes currently exposed to agents.
    pub fn snapshot_nodes(&self) -> Vec<SnapshotNode> {
        self.interactive_indices()
            .into_iter()
            .filter_map(|index| {
                let cursor = self.cursor_for_index(index)?;
                Some(SnapshotNode {
                    node_ref: cursor.node_ref.clone(),
                    index: cursor.index,
                    role: cursor.role.clone(),
                    name: cursor.name.clone(),
                    cursor,
                })
            })
            .collect()
    }

    /// Get all interactive element indices
    pub fn interactive_indices(&self) -> Vec<usize> {
        self.indexed.interactive_indices()
    }

    /// Count total nodes in the tree
    pub fn count_nodes(&self) -> usize {
        self.root.count_nodes()
    }

    /// Count interactive elements (elements with indices)
    pub fn count_interactive(&self) -> usize {
        self.indexed.records.len()
    }

    /// Find node by index
    pub fn find_node_by_index(&self, index: usize) -> Option<&AriaNode> {
        let record = self.indexed.record(index)?;
        Self::node_at_path(&self.root, &record.path)
    }

    /// Find node by index (mutable)
    pub fn find_node_by_index_mut(&mut self, index: usize) -> Option<&mut AriaNode> {
        let path = self.indexed.record(index)?.path.clone();
        Self::node_at_path_mut(&mut self.root, &path)
    }

    /// Get all iframe indices for multi-frame snapshot handling
    pub fn get_iframe_indices(&self) -> Vec<usize> {
        self.indexed.iframe_indices()
    }

    /// Convert the DOM tree to JSON
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(&self.root).map_err(|e| {
            BrowserError::DomParseFailed(format!("Failed to serialize DOM to JSON: {}", e))
        })
    }

    /// Replace an iframe node's children with content from another snapshot
    /// Used for multi-frame snapshot assembly
    pub fn inject_iframe_content(&mut self, iframe_index: usize, mut iframe_snapshot: DomTree) {
        let mut selector_overrides = self.selector_map();
        let mut next_index = self.indexed.next_available_index();
        let mut remapped_indices = BTreeMap::new();
        Self::rebase_children_indices(
            &mut iframe_snapshot.root.children,
            &mut next_index,
            &mut remapped_indices,
        );

        for (previous_index, selector) in iframe_snapshot.selector_map() {
            if let Some(rebased_index) = remapped_indices.get(&previous_index) {
                selector_overrides.insert(*rebased_index, selector);
            }
        }

        if let Some(iframe_node) = self.find_node_by_index_mut(iframe_index) {
            // Replace iframe's children with the snapshot's root children
            iframe_node.children = iframe_snapshot.root.children;
        }

        self.rebuild_indexed_from_selector_map(selector_overrides);
    }

    /// Create a snapshot with multiple frames assembled
    /// Takes a function that can retrieve snapshots for iframe elements
    pub fn assemble_with_iframes<F>(mut self, mut get_iframe_snapshot: F) -> Self
    where
        F: FnMut(usize) -> Option<DomTree>,
    {
        let iframe_indices = self.get_iframe_indices();

        for iframe_index in iframe_indices {
            if let Some(iframe_snapshot) = get_iframe_snapshot(iframe_index) {
                self.inject_iframe_content(iframe_index, iframe_snapshot);
            }
        }

        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_tree() -> AriaNode {
        let mut root = AriaNode::fragment();

        root.children.push(AriaChild::Node(Box::new(
            AriaNode::new("button", "Click me")
                .with_index(0)
                .with_box(true, Some("pointer".to_string())),
        )));

        root.children.push(AriaChild::Node(Box::new(
            AriaNode::new("link", "Go to page")
                .with_index(1)
                .with_box(true, None),
        )));

        root.children.push(AriaChild::Node(Box::new(
            AriaNode::new("paragraph", "").with_child(AriaChild::Text("Some text".to_string())),
        )));

        root
    }

    #[test]
    fn test_find_node_by_index() {
        let root = create_test_tree();
        let tree = DomTree::new(root);

        let button = tree.find_node_by_index(0);
        assert!(button.is_some());
        assert_eq!(button.unwrap().role, "button");
        assert_eq!(button.unwrap().name, "Click me");

        let not_found = tree.find_node_by_index(999);
        assert!(not_found.is_none());
    }

    #[test]
    fn test_node_ref_for_index_uses_document_metadata() {
        let root = create_test_tree();
        let mut tree = DomTree::new(root);
        tree.document.document_id = "doc-1".to_string();
        tree.document.revision = "rev-7".to_string();

        let node_ref = tree.node_ref_for_index(1).expect("node ref should exist");
        assert_eq!(node_ref.document_id, "doc-1");
        assert_eq!(node_ref.revision, "rev-7");
        assert_eq!(node_ref.index, 1);
    }

    #[test]
    fn test_cursor_for_index_uses_document_metadata_and_selector() {
        let root = create_test_tree();
        let mut tree = DomTree::new(root);
        tree.document.document_id = "doc-1".to_string();
        tree.document.revision = "rev-7".to_string();
        tree.replace_selectors(vec![
            "button.primary".to_string(),
            "a[href='/next']".to_string(),
        ]);

        let cursor = tree.cursor_for_index(1).expect("cursor should exist");
        assert_eq!(cursor.node_ref.document_id, "doc-1");
        assert_eq!(cursor.node_ref.revision, "rev-7");
        assert_eq!(cursor.node_ref.index, 1);
        assert_eq!(cursor.selector, "a[href='/next']");
        assert_eq!(cursor.index, 1);
        assert_eq!(cursor.role, "link");
        assert_eq!(cursor.name, "Go to page");
    }

    #[test]
    fn test_count_nodes() {
        let root = create_test_tree();
        let tree = DomTree::new(root);

        // fragment + button + link + paragraph = 4
        assert_eq!(tree.count_nodes(), 4);
    }

    #[test]
    fn test_interactive_indices() {
        let root = create_test_tree();
        let tree = DomTree::new(root);

        let indices = tree.interactive_indices();
        assert_eq!(indices.len(), 2);
        assert!(indices.contains(&0));
        assert!(indices.contains(&1));
    }

    #[test]
    fn test_inject_iframe_content() {
        let mut main_tree = AriaNode::fragment();
        main_tree.children.push(AriaChild::Node(Box::new(
            AriaNode::new("iframe", "").with_index(0),
        )));

        let mut iframe_tree = AriaNode::fragment();
        iframe_tree.children.push(AriaChild::Node(Box::new(
            AriaNode::new("button", "Inside iframe").with_index(0),
        )));

        let mut main = DomTree::new(main_tree);
        let iframe = DomTree::new(iframe_tree);

        main.inject_iframe_content(0, iframe);

        // Check that iframe now has the button as a child
        let iframe_node = main.find_node_by_index(0).unwrap();
        assert_eq!(iframe_node.children.len(), 1);

        match &iframe_node.children[0] {
            AriaChild::Node(n) => {
                assert_eq!(n.role, "button");
                assert_eq!(n.name, "Inside iframe");
            }
            _ => panic!("Expected node child"),
        }
    }

    #[test]
    fn test_get_selector_and_snapshot_nodes() {
        let root = create_test_tree();
        let mut tree = DomTree::new(root);
        tree.document.document_id = "doc-1".to_string();
        tree.document.revision = "rev-2".to_string();
        tree.replace_selectors(vec![
            "button.primary".to_string(),
            "a[href='/next']".to_string(),
        ]);

        assert_eq!(
            tree.get_selector(0).map(String::as_str),
            Some("button.primary")
        );
        assert_eq!(
            tree.get_selector(1).map(String::as_str),
            Some("a[href='/next']")
        );
        assert_eq!(tree.get_selector(99), None);
        assert_eq!(tree.count_interactive(), 2);

        let snapshot_nodes = tree.snapshot_nodes();
        assert_eq!(snapshot_nodes.len(), 2);
        assert_eq!(snapshot_nodes[0].index, 0);
        assert_eq!(snapshot_nodes[0].role, "button");
        assert_eq!(snapshot_nodes[0].name, "Click me");
        assert_eq!(snapshot_nodes[0].node_ref.document_id, "doc-1");
        assert_eq!(snapshot_nodes[0].node_ref.revision, "rev-2");
        assert_eq!(snapshot_nodes[0].cursor.selector, "button.primary");
        assert_eq!(snapshot_nodes[0].cursor.index, 0);
        assert_eq!(snapshot_nodes[0].cursor.role, "button");
        assert_eq!(snapshot_nodes[0].cursor.name, "Click me");
        assert_eq!(
            snapshot_nodes[0].cursor.node_ref,
            snapshot_nodes[0].node_ref
        );
        assert_eq!(snapshot_nodes[1].index, 1);
        assert_eq!(snapshot_nodes[1].role, "link");
        assert_eq!(snapshot_nodes[1].cursor.selector, "a[href='/next']");
    }

    #[test]
    fn test_indexed_snapshot_tracks_iframe_boundaries() {
        let mut root = AriaNode::fragment();
        root.children.push(AriaChild::Node(Box::new(
            AriaNode::new("iframe", "Embedded").with_index(3),
        )));

        let tree = DomTree::new(root);

        assert_eq!(tree.get_iframe_indices(), vec![3]);
        assert!(tree.find_node_by_index(3).is_some());
    }

    #[test]
    fn test_to_json_serializes_tree() {
        let tree = DomTree::new(create_test_tree());
        let json = tree.to_json().expect("tree should serialize");

        assert!(json.contains("\"button\""));
        assert!(json.contains("\"Click me\""));
    }

    #[test]
    fn test_assemble_with_iframes_merges_snapshot_and_offsets_nested_iframes() {
        let mut main_root = AriaNode::fragment();
        main_root.children.push(AriaChild::Node(Box::new(
            AriaNode::new("iframe", "Outer Frame").with_index(0),
        )));

        let mut nested_root = AriaNode::fragment();
        nested_root.children.push(AriaChild::Node(Box::new(
            AriaNode::new("button", "Inside iframe").with_index(0),
        )));
        nested_root.children.push(AriaChild::Node(Box::new(
            AriaNode::new("iframe", "Nested Frame").with_index(1),
        )));

        let mut main = DomTree::new(main_root);
        main.replace_selectors(vec!["#outer-frame".to_string()]);

        let mut nested = DomTree::new(nested_root);
        nested.replace_selectors(vec![
            "#inside-button".to_string(),
            "#nested-frame".to_string(),
        ]);

        let assembled = main.assemble_with_iframes(|index| {
            if index == 0 {
                Some(nested.clone())
            } else {
                None
            }
        });

        let iframe_node = assembled
            .find_node_by_index(0)
            .expect("iframe should exist");
        assert_eq!(iframe_node.children.len(), 2);
        assert_eq!(
            assembled.get_selector(2).map(String::as_str),
            Some("#nested-frame")
        );
        assert!(assembled.get_iframe_indices().contains(&0));
        assert!(assembled.get_iframe_indices().contains(&2));
    }
}

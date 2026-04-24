use super::duration_micros;
use crate::browser::{SnapshotCacheEntry, SnapshotCacheScope};
use crate::contract::{SnapshotMode, SnapshotScope};
use crate::dom::{
    AriaChild, AriaNode, DocumentMetadata, DomTree, SnapshotNode, yaml_escape_key_if_needed,
    yaml_escape_value_if_needed,
};
use crate::tools::snapshot::{RenderMode, render_aria_tree};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

pub(crate) struct SnapshotProjectionInput<'a> {
    pub dom: &'a DomTree,
    pub mode: SnapshotMode,
    pub global_interactive_count: Option<usize>,
    pub base: Option<&'a SnapshotCacheEntry>,
}

#[derive(Debug)]
pub(crate) struct SnapshotProjectionOutput {
    pub current: SnapshotProjection,
    pub cache_projection: Option<SnapshotProjection>,
}

#[derive(Debug, Clone)]
pub(crate) struct SnapshotProjection {
    pub snapshot: Arc<str>,
    pub nodes: Arc<[SnapshotNode]>,
    pub scope: SnapshotScope,
    pub render_micros: u64,
}

pub(crate) fn project_snapshot(input: SnapshotProjectionInput<'_>) -> SnapshotProjectionOutput {
    let current_projection = match input.mode {
        SnapshotMode::Full => snapshot_projection(
            input.dom,
            SnapshotMode::Full,
            input.global_interactive_count,
        ),
        SnapshotMode::Viewport | SnapshotMode::Delta => snapshot_projection(
            input.dom,
            SnapshotMode::Viewport,
            input.global_interactive_count,
        ),
    };

    match input.mode {
        SnapshotMode::Delta => delta_snapshot_projection(input.base, current_projection),
        SnapshotMode::Viewport => SnapshotProjectionOutput {
            current: current_projection.clone(),
            cache_projection: Some(current_projection),
        },
        SnapshotMode::Full => SnapshotProjectionOutput {
            current: current_projection,
            cache_projection: None,
        },
    }
}

fn snapshot_projection(
    dom: &DomTree,
    mode: SnapshotMode,
    global_interactive_count: Option<usize>,
) -> SnapshotProjection {
    let (snapshot, nodes, locality_fallback_reason, render_micros) = match mode {
        SnapshotMode::Full => {
            let started = Instant::now();
            (
                Arc::<str>::from(render_aria_tree(&dom.root, RenderMode::Ai, None)),
                Arc::<[SnapshotNode]>::from(snapshot_nodes_from_root(dom, &dom.root)),
                None,
                duration_micros(started.elapsed()),
            )
        }
        SnapshotMode::Viewport | SnapshotMode::Delta => {
            let viewport_local = build_scoped_snapshot_root(
                &dom.root,
                SnapshotAnchorPolicy {
                    strict_viewport: true,
                    allow_persistent_chrome: false,
                },
            );
            let (snapshot_root, locality_fallback_reason) = if !viewport_local.children.is_empty() {
                (viewport_local, None)
            } else {
                let viewport_with_persistent = build_scoped_snapshot_root(
                    &dom.root,
                    SnapshotAnchorPolicy {
                        strict_viewport: true,
                        allow_persistent_chrome: true,
                    },
                );
                if !viewport_with_persistent.children.is_empty() {
                    (
                        viewport_with_persistent,
                        Some("persistent_chrome_only".to_string()),
                    )
                } else {
                    let visible_document = build_scoped_snapshot_root(
                        &dom.root,
                        SnapshotAnchorPolicy {
                            strict_viewport: false,
                            allow_persistent_chrome: false,
                        },
                    );
                    if !visible_document.children.is_empty() {
                        (
                            visible_document,
                            Some("no_viewport_local_anchors".to_string()),
                        )
                    } else {
                        (
                            build_scoped_snapshot_root(
                                &dom.root,
                                SnapshotAnchorPolicy {
                                    strict_viewport: false,
                                    allow_persistent_chrome: true,
                                },
                            ),
                            Some("document_visible_fallback".to_string()),
                        )
                    }
                }
            };

            let started = Instant::now();
            (
                Arc::<str>::from(render_scoped_snapshot_root(&snapshot_root, RenderMode::Ai)),
                Arc::<[SnapshotNode]>::from(snapshot_nodes_from_scoped_root(dom, &snapshot_root)),
                locality_fallback_reason,
                duration_micros(started.elapsed()),
            )
        }
    };

    let returned_node_count = nodes.len();

    SnapshotProjection {
        snapshot,
        nodes,
        scope: SnapshotScope {
            mode,
            fallback_mode: None,
            viewport_biased: mode != SnapshotMode::Full,
            locality_fallback_reason,
            unavailable_frame_count: dom
                .document
                .frames
                .iter()
                .filter(|frame| frame.status != "expanded")
                .count(),
            returned_node_count,
            global_interactive_count,
            viewport: None,
        },
        render_micros,
    }
}

fn delta_snapshot_projection(
    base: Option<&SnapshotCacheEntry>,
    current_projection: SnapshotProjection,
) -> SnapshotProjectionOutput {
    let Some(base) = base else {
        let mut fallback = current_projection.clone();
        fallback.scope.mode = SnapshotMode::Delta;
        fallback.scope.fallback_mode = Some(SnapshotMode::Viewport);
        return SnapshotProjectionOutput {
            current: fallback,
            cache_projection: Some(current_projection),
        };
    };

    if base.scope.mode == "full" {
        let mut fallback = current_projection.clone();
        fallback.scope.mode = SnapshotMode::Delta;
        fallback.scope.fallback_mode = Some(SnapshotMode::Viewport);
        return SnapshotProjectionOutput {
            current: fallback,
            cache_projection: Some(current_projection),
        };
    }

    let mut projection = current_projection.clone();
    projection.scope.mode = SnapshotMode::Delta;
    projection.scope.fallback_mode = None;
    projection.snapshot = Arc::<str>::from(delta_snapshot_text(
        base.snapshot.as_ref(),
        current_projection.snapshot.as_ref(),
    ));

    let mut nodes = delta_snapshot_nodes(&base.nodes, &current_projection.nodes);
    if nodes.is_empty() && current_projection.snapshot.as_ref() != base.snapshot.as_ref() {
        projection.nodes = Arc::clone(&current_projection.nodes);
    } else {
        projection.nodes = Arc::<[SnapshotNode]>::from(std::mem::take(&mut nodes));
    }
    projection.scope.returned_node_count = projection.nodes.len();

    if projection.snapshot.is_empty() {
        projection.snapshot = Arc::clone(&current_projection.snapshot);
    }

    SnapshotProjectionOutput {
        current: projection,
        cache_projection: Some(current_projection),
    }
}

pub(crate) fn snapshot_cache_entry_from_projection(
    document: &DocumentMetadata,
    projection: &SnapshotProjection,
) -> SnapshotCacheEntry {
    SnapshotCacheEntry {
        document: document.clone(),
        snapshot: Arc::clone(&projection.snapshot),
        nodes: Arc::clone(&projection.nodes),
        scope: SnapshotCacheScope {
            mode: snapshot_mode_label(projection.scope.mode).to_string(),
            fallback_mode: projection
                .scope
                .fallback_mode
                .map(|mode| snapshot_mode_label(mode).to_string()),
            viewport_biased: projection.scope.viewport_biased,
            returned_node_count: projection.scope.returned_node_count,
            unavailable_frame_count: projection.scope.unavailable_frame_count,
            global_interactive_count: projection.scope.global_interactive_count,
        },
    }
}

fn snapshot_mode_label(mode: SnapshotMode) -> &'static str {
    match mode {
        SnapshotMode::Viewport => "viewport",
        SnapshotMode::Delta => "delta",
        SnapshotMode::Full => "full",
    }
}

fn delta_snapshot_text(previous: &str, current: &str) -> String {
    let mut previous_counts = HashMap::new();
    for line in previous.lines() {
        *previous_counts.entry(line).or_insert(0usize) += 1;
    }

    let mut changed_lines = Vec::new();
    for line in current.lines() {
        match previous_counts.get_mut(line) {
            Some(count) if *count > 0 => {
                *count -= 1;
            }
            _ => changed_lines.push(line.to_string()),
        }
    }

    changed_lines.join("\n")
}

fn delta_snapshot_nodes(previous: &[SnapshotNode], current: &[SnapshotNode]) -> Vec<SnapshotNode> {
    let previous_by_selector = previous
        .iter()
        .map(|node| (node.cursor.selector.as_str(), node))
        .collect::<HashMap<_, _>>();

    current
        .iter()
        .filter(
            |node| match previous_by_selector.get(node.cursor.selector.as_str()) {
                Some(previous_node) => **previous_node != **node,
                None => true,
            },
        )
        .cloned()
        .collect()
}

#[derive(Debug, Clone, Copy)]
struct SnapshotAnchorPolicy {
    strict_viewport: bool,
    allow_persistent_chrome: bool,
}

#[derive(Debug)]
enum ScopedSnapshotChild<'a> {
    Text(&'a str),
    Node(Box<ScopedSnapshotNode<'a>>),
}

#[derive(Debug)]
struct ScopedSnapshotNode<'a> {
    node: &'a AriaNode,
    public_handle: bool,
    children: Vec<ScopedSnapshotChild<'a>>,
}

fn build_scoped_snapshot_root<'a>(
    root: &'a AriaNode,
    policy: SnapshotAnchorPolicy,
) -> ScopedSnapshotNode<'a> {
    build_scoped_snapshot_node(root, policy, true)
        .expect("snapshot root should always remain renderable")
}

fn build_scoped_snapshot_node<'a>(
    node: &'a AriaNode,
    policy: SnapshotAnchorPolicy,
    is_root: bool,
) -> Option<ScopedSnapshotNode<'a>> {
    let is_anchor = node_is_snapshot_anchor(node, policy);
    let mut scoped_children = Vec::new();
    let mut child_anchor_count = 0usize;

    for child in &node.children {
        match child {
            AriaChild::Text(text) => {
                if is_anchor || is_root {
                    scoped_children.push(ScopedSnapshotChild::Text(text));
                }
            }
            AriaChild::Node(child_node) => {
                if let Some(scoped_child) = build_scoped_snapshot_node(child_node, policy, false) {
                    child_anchor_count += 1;
                    scoped_children.push(ScopedSnapshotChild::Node(Box::new(scoped_child)));
                }
            }
        }
    }

    let public_handle = node.has_public_handle()
        && (node.carries_snapshot_state() || node_matches_policy(node, policy));

    let keep = is_root || is_anchor || child_anchor_count > 0;
    if !keep {
        return None;
    }

    let has_child_nodes = scoped_children
        .iter()
        .any(|child| matches!(child, ScopedSnapshotChild::Node(_)));
    let is_noise = node.role == "generic"
        && node.name.is_empty()
        && node.props.is_empty()
        && !public_handle
        && !node.carries_snapshot_state()
        && !has_child_nodes;

    if !is_root && is_noise {
        return None;
    }

    Some(ScopedSnapshotNode {
        node,
        public_handle,
        children: scoped_children,
    })
}

fn node_matches_policy(node: &AriaNode, policy: SnapshotAnchorPolicy) -> bool {
    let visible = if policy.strict_viewport {
        node.box_info.in_viewport
    } else {
        node.box_info.visible
    };

    visible && (policy.allow_persistent_chrome || !node.is_persistent_chrome())
}

fn node_is_snapshot_anchor(node: &AriaNode, policy: SnapshotAnchorPolicy) -> bool {
    node.carries_snapshot_state() || node_matches_policy(node, policy)
}

fn snapshot_nodes_from_root(dom: &DomTree, root: &AriaNode) -> Vec<SnapshotNode> {
    let mut nodes = Vec::new();
    collect_snapshot_nodes(dom, root, &mut nodes);
    nodes
}

fn snapshot_nodes_from_scoped_root(
    dom: &DomTree,
    root: &ScopedSnapshotNode<'_>,
) -> Vec<SnapshotNode> {
    let mut nodes = Vec::new();
    collect_scoped_snapshot_nodes(dom, root, &mut nodes);
    nodes
}

fn collect_snapshot_nodes(dom: &DomTree, node: &AriaNode, nodes: &mut Vec<SnapshotNode>) {
    if node.has_public_handle()
        && let Some(index) = node.index
        && let Some(cursor) = dom.cursor_for_index(index)
    {
        nodes.push(SnapshotNode {
            node_ref: cursor.node_ref.clone(),
            index: cursor.index,
            role: cursor.role.clone(),
            name: cursor.name.clone(),
            cursor,
        });
    }

    for child in &node.children {
        if let AriaChild::Node(child_node) = child {
            collect_snapshot_nodes(dom, child_node, nodes);
        }
    }
}

fn collect_scoped_snapshot_nodes(
    dom: &DomTree,
    node: &ScopedSnapshotNode<'_>,
    nodes: &mut Vec<SnapshotNode>,
) {
    if node.public_handle
        && let Some(index) = node.node.index
        && let Some(cursor) = dom.cursor_for_index(index)
    {
        nodes.push(SnapshotNode {
            node_ref: cursor.node_ref.clone(),
            index: cursor.index,
            role: cursor.role.clone(),
            name: cursor.name.clone(),
            cursor,
        });
    }

    for child in &node.children {
        if let ScopedSnapshotChild::Node(child_node) = child {
            collect_scoped_snapshot_nodes(dom, child_node, nodes);
        }
    }
}

fn render_scoped_snapshot_root(root: &ScopedSnapshotNode<'_>, mode: RenderMode) -> String {
    let mut lines = Vec::new();
    let render_cursor_pointer = matches!(mode, RenderMode::Ai);
    let render_active = matches!(mode, RenderMode::Ai);

    if root.node.role == "fragment" {
        for child in &root.children {
            match child {
                ScopedSnapshotChild::Text(text) => visit_scoped_text(text, "", &mut lines),
                ScopedSnapshotChild::Node(node) => {
                    visit_scoped_node(node, "", render_cursor_pointer, render_active, &mut lines)
                }
            }
        }
    } else {
        visit_scoped_node(root, "", render_cursor_pointer, render_active, &mut lines);
    }

    lines.join("\n")
}

fn visit_scoped_text(text: &str, indent: &str, lines: &mut Vec<String>) {
    let escaped = yaml_escape_value_if_needed(text);
    if !escaped.is_empty() {
        lines.push(format!("{}- text: {}", indent, escaped));
    }
}

fn visit_scoped_node(
    node: &ScopedSnapshotNode<'_>,
    indent: &str,
    render_cursor_pointer: bool,
    render_active: bool,
    lines: &mut Vec<String>,
) {
    let key = create_scoped_snapshot_key(node, render_cursor_pointer, render_active);
    let escaped_key = format!("{}- {}", indent, yaml_escape_key_if_needed(&key));

    if node.children.is_empty() && node.node.props.is_empty() {
        lines.push(escaped_key);
        return;
    }

    if let Some(text) = scoped_single_inlined_text_child(node) {
        lines.push(format!(
            "{}: {}",
            escaped_key,
            yaml_escape_value_if_needed(text)
        ));
        return;
    }

    lines.push(format!("{}:", escaped_key));

    for (name, value) in &node.node.props {
        lines.push(format!(
            "{}  - /{}: {}",
            indent,
            name,
            yaml_escape_value_if_needed(value)
        ));
    }

    let child_indent = format!("{}  ", indent);
    let in_cursor_pointer =
        node.public_handle && render_cursor_pointer && node.node.has_pointer_cursor();

    for child in &node.children {
        match child {
            ScopedSnapshotChild::Text(text) => visit_scoped_text(text, &child_indent, lines),
            ScopedSnapshotChild::Node(child_node) => visit_scoped_node(
                child_node,
                &child_indent,
                render_cursor_pointer && !in_cursor_pointer,
                render_active,
                lines,
            ),
        }
    }
}

fn create_scoped_snapshot_key(
    node: &ScopedSnapshotNode<'_>,
    render_cursor_pointer: bool,
    render_active: bool,
) -> String {
    let aria_node = node.node;
    let mut key = aria_node.role.clone();

    if !aria_node.name.is_empty() && aria_node.name.len() <= 900 {
        key.push(' ');
        key.push_str(&format!("{:?}", aria_node.name));
    }

    if let Some(checked) = &aria_node.checked {
        match checked {
            crate::dom::element::AriaChecked::Bool(true) => key.push_str(" [checked]"),
            crate::dom::element::AriaChecked::Bool(false) => {}
            crate::dom::element::AriaChecked::Mixed(_) => key.push_str(" [checked=mixed]"),
        }
    }

    if aria_node.disabled == Some(true) {
        key.push_str(" [disabled]");
    }

    if aria_node.expanded == Some(true) {
        key.push_str(" [expanded]");
    }

    if render_active && aria_node.active == Some(true) {
        key.push_str(" [active]");
    }

    if let Some(level) = aria_node.level {
        key.push_str(&format!(" [level={}]", level));
    }

    if let Some(pressed) = &aria_node.pressed {
        match pressed {
            crate::dom::element::AriaPressed::Bool(true) => key.push_str(" [pressed]"),
            crate::dom::element::AriaPressed::Bool(false) => {}
            crate::dom::element::AriaPressed::Mixed(_) => key.push_str(" [pressed=mixed]"),
        }
    }

    if aria_node.selected == Some(true) {
        key.push_str(" [selected]");
    }

    if let Some(index) = aria_node.index.filter(|_| node.public_handle) {
        key.push_str(&format!(" [index={}]", index));

        if render_cursor_pointer && aria_node.has_pointer_cursor() {
            key.push_str(" [cursor=pointer]");
        }
    }

    key
}

fn scoped_single_inlined_text_child<'a>(node: &'a ScopedSnapshotNode<'a>) -> Option<&'a str> {
    if node.children.len() == 1
        && node.node.props.is_empty()
        && let ScopedSnapshotChild::Text(text) = &node.children[0]
    {
        return Some(text);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::{AriaChild, AriaNode, DomTree};

    fn viewport_dom() -> DomTree {
        let mut offscreen_button = AriaNode::new("button", "Offscreen save")
            .with_index(0)
            .with_public_handle(true)
            .with_box(true, Some("pointer".to_string()));
        offscreen_button.box_info.in_viewport = false;

        let visible_heading = AriaNode::new("heading", "Visible story").with_box(true, None);

        let visible_tab = AriaNode::new("button", "Visible tab")
            .with_index(1)
            .with_public_handle(true)
            .with_box(true, Some("pointer".to_string()))
            .with_selected(true);

        let root = AriaNode::fragment()
            .with_child(AriaChild::Node(Box::new(offscreen_button)))
            .with_child(AriaChild::Node(Box::new(visible_heading)))
            .with_child(AriaChild::Node(Box::new(visible_tab)));

        let mut dom = DomTree::new(root);
        dom.document.document_id = "doc-viewport".to_string();
        dom.document.revision = "main:4".to_string();
        dom.replace_selectors(vec![
            "#offscreen-save".to_string(),
            "#visible-tab".to_string(),
        ]);
        dom
    }

    fn persistent_chrome_dom() -> DomTree {
        let mut header_button = AriaNode::new("button", "Header action")
            .with_index(0)
            .with_public_handle(true)
            .with_box(true, Some("pointer".to_string()));
        header_button.box_info.persistent_chrome = true;
        header_button.box_info.persistent_position = Some("sticky".to_string());
        header_button.box_info.persistent_edge = Some("top".to_string());

        let local_heading = AriaNode::new("heading", "Local section").with_box(true, None);

        let local_button = AriaNode::new("button", "Local action")
            .with_index(1)
            .with_public_handle(true)
            .with_box(true, Some("pointer".to_string()));

        let root = AriaNode::fragment()
            .with_child(AriaChild::Node(Box::new(header_button)))
            .with_child(AriaChild::Node(Box::new(local_heading)))
            .with_child(AriaChild::Node(Box::new(local_button)));

        let mut dom = DomTree::new(root);
        dom.document.document_id = "doc-persistent".to_string();
        dom.document.revision = "main:9".to_string();
        dom.replace_selectors(vec![
            "#header-action".to_string(),
            "#local-action".to_string(),
        ]);
        dom
    }

    fn persistent_delta_dom(details_visible: bool) -> DomTree {
        let mut header_button = AriaNode::new("button", "Header action")
            .with_index(0)
            .with_public_handle(true)
            .with_box(true, Some("pointer".to_string()));
        header_button.box_info.persistent_chrome = true;
        header_button.box_info.persistent_position = Some("sticky".to_string());
        header_button.box_info.persistent_edge = Some("top".to_string());

        let local_toggle = AriaNode::new("button", "Show details")
            .with_index(1)
            .with_public_handle(true)
            .with_box(true, Some("pointer".to_string()));

        let mut root = AriaNode::fragment()
            .with_child(AriaChild::Node(Box::new(header_button)))
            .with_child(AriaChild::Node(Box::new(
                AriaNode::new("heading", "Local section").with_box(true, None),
            )))
            .with_child(AriaChild::Node(Box::new(local_toggle)));

        if details_visible {
            root = root.with_child(AriaChild::Node(Box::new(
                AriaNode::new("button", "Details")
                    .with_index(2)
                    .with_public_handle(true)
                    .with_box(true, Some("pointer".to_string())),
            )));
        }

        let mut dom = DomTree::new(root);
        dom.document.document_id = "doc-persistent-delta".to_string();
        dom.document.revision = if details_visible {
            "main:7".to_string()
        } else {
            "main:6".to_string()
        };
        dom.replace_selectors(vec![
            "#header-action".to_string(),
            "#toggle".to_string(),
            "#details".to_string(),
        ]);
        dom
    }

    #[test]
    fn projector_viewport_mode_scopes_snapshot_handles() {
        let dom = viewport_dom();
        let output = project_snapshot(SnapshotProjectionInput {
            dom: &dom,
            mode: SnapshotMode::Viewport,
            global_interactive_count: Some(dom.count_interactive()),
            base: None,
        });

        assert_eq!(output.current.scope.mode, SnapshotMode::Viewport);
        assert_eq!(output.current.scope.fallback_mode, None);
        assert_eq!(output.current.scope.global_interactive_count, Some(2));
        assert_eq!(output.current.nodes.len(), 1);
        assert_eq!(output.current.nodes[0].name, "Visible tab");
        assert!(output.current.snapshot.contains("Visible story"));
        assert!(output.current.snapshot.contains("Visible tab"));
        assert!(!output.current.snapshot.contains("Offscreen save"));
        assert!(output.cache_projection.is_some());
    }

    #[test]
    fn projector_full_mode_preserves_exhaustive_snapshot_handles() {
        let dom = viewport_dom();
        let output = project_snapshot(SnapshotProjectionInput {
            dom: &dom,
            mode: SnapshotMode::Full,
            global_interactive_count: Some(dom.count_interactive()),
            base: None,
        });

        assert_eq!(output.current.scope.mode, SnapshotMode::Full);
        assert_eq!(output.current.scope.viewport_biased, false);
        assert_eq!(output.current.nodes.len(), 2);
        assert!(output.current.snapshot.contains("Offscreen save"));
        assert!(output.current.snapshot.contains("Visible tab"));
        assert!(output.cache_projection.is_none());
    }

    #[test]
    fn projector_delta_without_base_falls_back_to_viewport() {
        let dom = viewport_dom();
        let output = project_snapshot(SnapshotProjectionInput {
            dom: &dom,
            mode: SnapshotMode::Delta,
            global_interactive_count: Some(dom.count_interactive()),
            base: None,
        });

        assert_eq!(output.current.scope.mode, SnapshotMode::Delta);
        assert_eq!(
            output.current.scope.fallback_mode,
            Some(SnapshotMode::Viewport)
        );
        assert_eq!(output.current.nodes.len(), 1);
        assert_eq!(output.current.nodes[0].name, "Visible tab");
        assert!(output.cache_projection.is_some());
    }

    #[test]
    fn projector_delta_diff_uses_cache_base() {
        let base_dom = persistent_delta_dom(false);
        let base_output = project_snapshot(SnapshotProjectionInput {
            dom: &base_dom,
            mode: SnapshotMode::Viewport,
            global_interactive_count: Some(base_dom.count_interactive()),
            base: None,
        });
        let base_entry =
            snapshot_cache_entry_from_projection(&base_dom.document, &base_output.current);

        let current_dom = persistent_delta_dom(true);
        let output = project_snapshot(SnapshotProjectionInput {
            dom: &current_dom,
            mode: SnapshotMode::Delta,
            global_interactive_count: Some(current_dom.count_interactive()),
            base: Some(&base_entry),
        });

        assert_eq!(output.current.scope.mode, SnapshotMode::Delta);
        assert_eq!(output.current.scope.fallback_mode, None);
        assert!(output.current.snapshot.contains("Details"));
        assert!(!output.current.snapshot.contains("Header action"));
        let node_names = output
            .current
            .nodes
            .iter()
            .map(|node| node.name.as_str())
            .collect::<Vec<_>>();
        assert!(node_names.contains(&"Details"));
        assert!(node_names.contains(&"Show details"));
        assert!(!node_names.contains(&"Header action"));
        assert!(output.cache_projection.is_some());
    }

    #[test]
    fn projector_cache_entry_conversion_preserves_scope_fields() {
        let dom = viewport_dom();
        let output = project_snapshot(SnapshotProjectionInput {
            dom: &dom,
            mode: SnapshotMode::Viewport,
            global_interactive_count: Some(dom.count_interactive()),
            base: None,
        });
        let entry = snapshot_cache_entry_from_projection(&dom.document, &output.current);

        assert_eq!(entry.document.document_id, "doc-viewport");
        assert_eq!(entry.scope.mode, "viewport");
        assert_eq!(entry.scope.fallback_mode, None);
        assert!(entry.scope.viewport_biased);
        assert_eq!(entry.scope.returned_node_count, output.current.nodes.len());
        assert_eq!(entry.scope.global_interactive_count, Some(2));
    }

    #[test]
    fn scoped_rendering_demotes_persistent_chrome_when_local_anchors_exist() {
        let dom = persistent_chrome_dom();
        let output = project_snapshot(SnapshotProjectionInput {
            dom: &dom,
            mode: SnapshotMode::Viewport,
            global_interactive_count: Some(dom.count_interactive()),
            base: None,
        });

        assert_eq!(output.current.scope.mode, SnapshotMode::Viewport);
        assert_eq!(output.current.scope.locality_fallback_reason, None);
        assert_eq!(output.current.nodes.len(), 1);
        assert_eq!(output.current.nodes[0].name, "Local action");
        assert!(output.current.snapshot.contains("Local section"));
        assert!(output.current.snapshot.contains("Local action"));
        assert!(!output.current.snapshot.contains("Header action"));
    }
}

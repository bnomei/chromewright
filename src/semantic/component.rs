//! Normalized semantic component tree for TUI and shared renderers.

use crate::semantic::identity::SemanticRef;
use serde::{Deserialize, Serialize};

/// Normalized component kind used by renderers, focus traversal, and projections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticKind {
    /// Structural landmark region (main, nav, …).
    Landmark,
    /// Heading with a level in attrs.
    Heading,
    /// Prose or other non-interactive text block.
    Text,
    /// Ordered or unordered list container.
    List,
    /// Single list item.
    ListItem,
    /// Hyperlink (focusable).
    Link,
    /// Image with optional alt/src.
    Image,
    /// Form input control (focusable).
    Input,
    /// Multiline text control (focusable).
    Textarea,
    /// Dropdown control with options (focusable).
    Select,
    /// Button control (focusable).
    Button,
    /// Generic grouping node without a more specific kind.
    Group,
}

/// Landmark roles retained from the HTML semantic subset for outline and Ratatui frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LandmarkRole {
    Main,
    Aside,
    Header,
    Nav,
    Section,
    Footer,
}

/// One normalized semantic component with an opaque identity and typed metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticComponent {
    /// Opaque reference for this component within its document revision.
    pub semantic_ref: SemanticRef,
    /// Component kind used by renderers and focus traversal.
    pub kind: SemanticKind,
    /// Visible label or accessible name when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Primary text content retained for display and copy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Kind-specific interaction and content metadata.
    #[serde(default, skip_serializing_if = "SemanticAttrs::is_empty")]
    pub attrs: SemanticAttrs,
    /// Capture-scoped exact selector used only by the in-process TUI driver.
    ///
    /// This is deliberately absent from semantic JSON/resources: callers act
    /// through opaque `semantic_ref`s, while the driver validates this locator
    /// against the same document revision and requires one exact match.
    #[serde(skip)]
    pub(crate) interaction_selector: Option<String>,
    /// Child components in document order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<SemanticComponent>,
}

impl SemanticComponent {
    /// Whether this component can receive keyboard focus in later TUI phases.
    pub fn is_focusable(&self) -> bool {
        matches!(
            self.kind,
            SemanticKind::Link
                | SemanticKind::Input
                | SemanticKind::Textarea
                | SemanticKind::Select
                | SemanticKind::Button
        )
    }

    /// Depth-first walk of this component and its descendants.
    pub fn walk<'a>(&'a self, visit: &mut dyn FnMut(&'a SemanticComponent)) {
        visit(self);
        for child in &self.children {
            child.walk(visit);
        }
    }
}

/// Typed attributes retained for interaction and presentation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticAttrs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub landmark: Option<LandmarkRole>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading_level: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ordered: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub src: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub readonly: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multiple: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub button_type: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<SelectOption>,
    /// Source HTML tag when useful for debug projections (not required for renderers).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}

impl SemanticAttrs {
    /// True when no kind-specific fields are set (serde skip and renderer elision).
    pub fn is_empty(&self) -> bool {
        self.landmark.is_none()
            && self.heading_level.is_none()
            && self.ordered.is_none()
            && self.href.is_none()
            && self.src.is_none()
            && self.alt.is_none()
            && self.name.is_none()
            && self.value.is_none()
            && self.input_type.is_none()
            && self.placeholder.is_none()
            && self.checked.is_none()
            && self.disabled.is_none()
            && self.required.is_none()
            && self.readonly.is_none()
            && self.multiple.is_none()
            && self.button_type.is_none()
            && self.options.is_empty()
            && self.tag.is_none()
    }
}

/// One option retained from a `<select>` control (clipped by select-option budgets).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectOption {
    /// Option value submitted with the control.
    pub value: String,
    /// Visible option label when distinct from `value`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Whether this option is currently selected.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub selected: bool,
    /// Whether this option is disabled.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub disabled: bool,
}

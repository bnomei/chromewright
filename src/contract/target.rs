use crate::dom::{Cursor, NodeRef};
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::de::Deserializer;
use std::borrow::Cow;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct TargetEnvelope {
    pub method: String,
    #[serde(default = "default_target_resolution_status")]
    pub resolution_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovered_from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_ref: Option<NodeRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
}

fn default_target_resolution_status() -> String {
    "exact".to_string()
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PublicTarget {
    /// Resolve the target from a selector in the current document.
    Selector { selector: String },
    /// Reuse a revision-scoped cursor from `snapshot` or `inspect_node`.
    Cursor { cursor: Cursor },
}

#[derive(Debug, Clone, serde::Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum TaggedPublicTarget {
    Selector { selector: String },
    Cursor { cursor: Cursor },
}

#[derive(Debug, Clone, serde::Deserialize, JsonSchema)]
#[serde(untagged)]
enum PublicTargetCompat {
    SelectorString(String),
    Tagged(TaggedPublicTarget),
}

impl<'de> serde::Deserialize<'de> for PublicTarget {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match PublicTargetCompat::deserialize(deserializer)? {
            PublicTargetCompat::SelectorString(selector) => Ok(Self::Selector { selector }),
            PublicTargetCompat::Tagged(TaggedPublicTarget::Selector { selector }) => {
                Ok(Self::Selector { selector })
            }
            PublicTargetCompat::Tagged(TaggedPublicTarget::Cursor { cursor }) => {
                Ok(Self::Cursor { cursor })
            }
        }
    }
}

impl JsonSchema for PublicTarget {
    fn schema_name() -> Cow<'static, str> {
        "PublicTarget".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        PublicTargetCompat::json_schema(generator)
    }
}

impl PublicTarget {
    pub(crate) fn into_selector_or_cursor(self) -> (Option<String>, Option<Cursor>) {
        match self {
            Self::Selector { selector } => (Some(selector), None),
            Self::Cursor { cursor } => (None, Some(cursor)),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TargetStatus {
    Same,
    Rebound,
    Detached,
    Unknown,
}

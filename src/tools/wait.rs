//! MCP tool that polls document settle, revision change, or node actionability conditions.
//!
//! Node waits take a selector or cursor target; navigation and revision waits are document-scoped.

use crate::dom::{Cursor, NodeRef};
use crate::error::Result;
use crate::tools::{
    TargetEnvelope, Tool, ToolContext, ToolResult, core::DocumentActionResult, core::PublicTarget,
    services::interaction::TargetStatus, services::wait::execute_wait,
};
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::de::Deserializer;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

#[cfg(test)]
pub(crate) use crate::tools::services::wait::{
    condition_name, validate_wait_condition, wait_condition_matches, wait_condition_predicates,
    wait_condition_uses_interaction_scroll,
};

/// Predicate the wait tool polls until it holds or the timeout elapses.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WaitCondition {
    /// Document load/navigation has settled (no target required; default when omitted).
    NavigationSettled,
    /// Target node exists in the live DOM.
    Present,
    /// Target is visible (layout + visibility checks).
    Visible,
    /// Target is enabled (not disabled).
    Enabled,
    /// Target is editable (input/textarea and not readonly/disabled).
    Editable,
    /// Target passes the full actionability set used by interaction tools.
    Actionable,
    /// Target geometry has stopped moving across consecutive probes.
    Stable,
    /// Target receives pointer events at its hit-test point.
    ReceivesEvents,
    /// Target text content contains the required fragment (`text` required).
    TextContains,
    /// Target form value equals the required string (`value` required).
    ValueEquals,
    /// Document revision differs from `since_revision` (or the baseline at wait start).
    RevisionChanged,
}

/// Condition, optional selector/cursor target, and timeout for a wait poll loop.
#[derive(Debug, Clone, Serialize)]
pub struct WaitParams {
    /// CSS selector to wait for
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,

    /// Element index from the current DOM tree
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,

    /// Revision-scoped node reference from the snapshot tool
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_ref: Option<NodeRef>,

    /// Cursor from the snapshot or inspect_node tools
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,

    /// Wait predicate to apply
    pub condition: WaitCondition,

    /// Expected text fragment for `text_contains`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,

    /// Expected value for `value_equals`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,

    /// Baseline revision token for `revision_changed`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since_revision: Option<String>,

    /// Timeout in milliseconds (default: 30000)
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum NavigationSettledRequestCondition {
    NavigationSettled,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum RevisionChangedRequestCondition {
    RevisionChanged,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum NodeStateWaitCondition {
    Present,
    Visible,
    Enabled,
    Editable,
    Actionable,
    Stable,
    ReceivesEvents,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum TextContainsRequestCondition {
    TextContains,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum ValueEqualsRequestCondition {
    ValueEquals,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct StrictNavigationSettledWaitParams {
    /// Omit `condition` or set it to `navigation_settled` to wait for the document to settle.
    #[serde(default)]
    #[serde(rename = "condition")]
    pub _condition: Option<NavigationSettledRequestCondition>,
    /// Timeout in milliseconds (default: 30000)
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct StrictRevisionChangedWaitParams {
    #[serde(rename = "condition")]
    pub _condition: RevisionChangedRequestCondition,
    /// Omit to use the current document revision as the baseline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since_revision: Option<String>,
    /// Timeout in milliseconds (default: 30000)
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct StrictNodeStateWaitParams {
    pub condition: NodeStateWaitCondition,
    /// Node target to wait on.
    pub target: PublicTarget,
    /// Timeout in milliseconds (default: 30000)
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct StrictTextContainsWaitParams {
    #[serde(rename = "condition")]
    pub _condition: TextContainsRequestCondition,
    /// Node target to wait on.
    pub target: PublicTarget,
    /// Required text fragment for `text_contains`.
    pub text: String,
    /// Timeout in milliseconds (default: 30000)
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct StrictValueEqualsWaitParams {
    #[serde(rename = "condition")]
    pub _condition: ValueEqualsRequestCondition,
    /// Node target to wait on.
    pub target: PublicTarget,
    /// Required value for `value_equals`.
    pub value: String,
    /// Timeout in milliseconds (default: 30000)
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum StrictWaitParams {
    NavigationSettled(StrictNavigationSettledWaitParams),
    RevisionChanged(StrictRevisionChangedWaitParams),
    NodeState(StrictNodeStateWaitParams),
    TextContains(StrictTextContainsWaitParams),
    ValueEquals(StrictValueEqualsWaitParams),
}

impl From<StrictWaitParams> for WaitParams {
    fn from(params: StrictWaitParams) -> Self {
        match params {
            StrictWaitParams::NavigationSettled(params) => Self {
                selector: None,
                index: None,
                node_ref: None,
                cursor: None,
                condition: WaitCondition::NavigationSettled,
                text: None,
                value: None,
                since_revision: None,
                timeout_ms: params.timeout_ms,
            },
            StrictWaitParams::RevisionChanged(params) => Self {
                selector: None,
                index: None,
                node_ref: None,
                cursor: None,
                condition: WaitCondition::RevisionChanged,
                text: None,
                value: None,
                since_revision: params.since_revision,
                timeout_ms: params.timeout_ms,
            },
            StrictWaitParams::NodeState(params) => {
                let (selector, cursor) = params.target.into_selector_or_cursor();
                Self {
                    selector,
                    index: None,
                    node_ref: None,
                    cursor,
                    condition: match params.condition {
                        NodeStateWaitCondition::Present => WaitCondition::Present,
                        NodeStateWaitCondition::Visible => WaitCondition::Visible,
                        NodeStateWaitCondition::Enabled => WaitCondition::Enabled,
                        NodeStateWaitCondition::Editable => WaitCondition::Editable,
                        NodeStateWaitCondition::Actionable => WaitCondition::Actionable,
                        NodeStateWaitCondition::Stable => WaitCondition::Stable,
                        NodeStateWaitCondition::ReceivesEvents => WaitCondition::ReceivesEvents,
                    },
                    text: None,
                    value: None,
                    since_revision: None,
                    timeout_ms: params.timeout_ms,
                }
            }
            StrictWaitParams::TextContains(params) => {
                let (selector, cursor) = params.target.into_selector_or_cursor();
                Self {
                    selector,
                    index: None,
                    node_ref: None,
                    cursor,
                    condition: WaitCondition::TextContains,
                    text: Some(params.text),
                    value: None,
                    since_revision: None,
                    timeout_ms: params.timeout_ms,
                }
            }
            StrictWaitParams::ValueEquals(params) => {
                let (selector, cursor) = params.target.into_selector_or_cursor();
                Self {
                    selector,
                    index: None,
                    node_ref: None,
                    cursor,
                    condition: WaitCondition::ValueEquals,
                    text: None,
                    value: Some(params.value),
                    since_revision: None,
                    timeout_ms: params.timeout_ms,
                }
            }
        }
    }
}

impl<'de> Deserialize<'de> for WaitParams {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        StrictWaitParams::deserialize(deserializer).map(Into::into)
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
#[allow(dead_code)]
struct WaitParamsAdvertisedSchema {
    /// Wait predicate. Defaults to `navigation_settled` when omitted.
    #[serde(default)]
    pub condition: Option<WaitCondition>,
    /// Node target for node-state, `text_contains`, and `value_equals` waits.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<PublicTarget>,
    /// Expected text fragment for `text_contains`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Expected value for `value_equals`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Baseline revision token for `revision_changed`. Omit to use the current document revision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since_revision: Option<String>,
    /// Timeout in milliseconds (default: 30000)
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
}

impl JsonSchema for WaitParams {
    fn schema_name() -> Cow<'static, str> {
        "WaitParams".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        WaitParamsAdvertisedSchema::json_schema(generator)
    }
}

fn default_timeout() -> u64 {
    30000
}

/// Polls document or target conditions until they hold or a timeout elapses.
#[derive(Default)]
pub struct WaitTool;

/// Elapsed wait result with optional before/after target envelopes and status.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WaitOutput {
    #[serde(flatten)]
    pub result: DocumentActionResult,
    pub condition: String,
    pub elapsed_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_before: Option<TargetEnvelope>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_after: Option<TargetEnvelope>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_status: Option<TargetStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since_revision: Option<String>,
}

impl Tool for WaitTool {
    type Params = WaitParams;
    type Output = WaitOutput;

    fn name(&self) -> &str {
        "wait"
    }

    fn description(&self) -> &str {
        "Pause for load, revision change, or node state. Use after actions or before rereading."
    }

    fn execute_typed(&self, params: WaitParams, context: &mut ToolContext) -> Result<ToolResult> {
        execute_wait(params, context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::BrowserSession;
    use crate::browser::backend::FakeSessionBackend;
    use crate::error::BrowserError;
    use crate::tools::actionability::ActionabilityPredicate;
    use crate::tools::limits::MAX_WAIT_TIMEOUT_MS;
    use schemars::schema_for;
    use serde_json::json;

    #[test]
    fn test_wait_params_defaults() {
        let params: WaitParams =
            serde_json::from_value(json!({})).expect("params should deserialize");

        assert_eq!(params.condition, WaitCondition::NavigationSettled);
        assert_eq!(params.timeout_ms, 30_000);
        assert!(params.selector.is_none());
        assert!(params.cursor.is_none());
        assert!(params.text.is_none());
        assert!(params.value.is_none());
    }

    #[test]
    fn test_wait_params_require_target_for_node_scoped_conditions() {
        let error = serde_json::from_value::<WaitParams>(json!({
            "condition": "present"
        }))
        .expect_err("node-scoped wait should require a target");
        assert!(error.to_string().contains("did not match any variant"));

        let error = serde_json::from_value::<WaitParams>(json!({
            "condition": "text_contains",
            "target": {
                "kind": "selector",
                "selector": "#status"
            }
        }))
        .expect_err("text_contains should require text");
        assert!(error.to_string().contains("did not match any variant"));

        let error = serde_json::from_value::<WaitParams>(json!({
            "condition": "value_equals",
            "target": {
                "kind": "selector",
                "selector": "#status"
            }
        }))
        .expect_err("value_equals should require value");
        assert!(error.to_string().contains("did not match any variant"));
    }

    #[test]
    fn test_wait_params_reject_document_scoped_targets_and_legacy_fields() {
        let error = serde_json::from_value::<WaitParams>(json!({
            "condition": "revision_changed",
            "target": {
                "kind": "selector",
                "selector": "#status"
            }
        }))
        .expect_err("revision_changed should reject targets");
        assert!(error.to_string().contains("did not match any variant"));

        let error = serde_json::from_value::<WaitParams>(json!({
            "condition": "visible",
            "selector": "#status"
        }))
        .expect_err("legacy selector field should be rejected");
        assert!(error.to_string().contains("did not match any variant"));
    }

    #[test]
    fn test_validate_wait_condition_requires_target_text_and_value() {
        let target_error = validate_wait_condition(&WaitCondition::Present, false, None, None)
            .expect_err("present without target should fail");
        assert!(matches!(
            target_error,
            crate::error::BrowserError::InvalidArgument(_)
        ));
        assert!(target_error.to_string().contains("wait.target"));

        let document_target_error =
            validate_wait_condition(&WaitCondition::NavigationSettled, true, None, None)
                .expect_err("document-scoped waits should reject targets");
        assert!(matches!(
            document_target_error,
            crate::error::BrowserError::InvalidArgument(_)
        ));
        assert!(document_target_error.to_string().contains("wait.target"));

        let text_error = validate_wait_condition(&WaitCondition::TextContains, true, None, None)
            .expect_err("text_contains without text should fail");
        assert!(matches!(
            text_error,
            crate::error::BrowserError::InvalidArgument(_)
        ));
        assert!(text_error.to_string().contains("wait.text"));

        let value_error = validate_wait_condition(&WaitCondition::ValueEquals, true, None, None)
            .expect_err("value_equals without value should fail");
        assert!(matches!(
            value_error,
            crate::error::BrowserError::InvalidArgument(_)
        ));
        assert!(value_error.to_string().contains("wait.value"));

        validate_wait_condition(&WaitCondition::Present, true, None, None)
            .expect("present should not require extra arguments");
        validate_wait_condition(&WaitCondition::TextContains, true, Some("hello"), None)
            .expect("text_contains should accept text");
        validate_wait_condition(&WaitCondition::ValueEquals, true, None, Some("abc"))
            .expect("value_equals should accept value");
    }

    #[test]
    fn test_wait_params_schema_is_flat_object_without_legacy_target_fields() {
        let schema = schema_for!(WaitParams);
        let schema_json = serde_json::to_value(&schema).expect("schema should serialize");
        assert_eq!(
            schema_json.get("type").and_then(|value| value.as_str()),
            Some("object")
        );
        for key in ["oneOf", "anyOf", "allOf", "enum", "not"] {
            assert!(
                schema_json.get(key).is_none(),
                "wait schema should not expose top-level {key}"
            );
        }

        let properties = schema_json
            .get("properties")
            .and_then(|value| value.as_object())
            .expect("wait schema should expose properties");
        assert!(properties.contains_key("condition"));
        assert!(properties.contains_key("target"));
        assert!(properties.contains_key("text"));
        assert!(properties.contains_key("value"));
        assert!(properties.contains_key("since_revision"));
        assert!(properties.contains_key("timeout_ms"));
        assert!(!properties.contains_key("selector"));
        assert!(!properties.contains_key("index"));
        assert!(!properties.contains_key("node_ref"));
        assert!(!properties.contains_key("cursor"));

        let serialized = serde_json::to_string(&schema_json).expect("schema should stringify");
        assert!(serialized.contains("\"target\""));
        assert!(serialized.contains("\"navigation_settled\""));
        assert!(serialized.contains("\"revision_changed\""));
    }

    #[test]
    fn test_condition_name_covers_all_wait_conditions() {
        let cases = [
            (WaitCondition::NavigationSettled, "navigation_settled"),
            (WaitCondition::Present, "present"),
            (WaitCondition::Visible, "visible"),
            (WaitCondition::Enabled, "enabled"),
            (WaitCondition::Editable, "editable"),
            (WaitCondition::Actionable, "actionable"),
            (WaitCondition::Stable, "stable"),
            (WaitCondition::ReceivesEvents, "receives_events"),
            (WaitCondition::TextContains, "text_contains"),
            (WaitCondition::ValueEquals, "value_equals"),
            (WaitCondition::RevisionChanged, "revision_changed"),
        ];

        for (condition, expected) in cases {
            assert_eq!(condition_name(&condition), expected);
        }
    }

    #[test]
    fn test_wait_condition_predicates_reuse_shared_actionability_model() {
        assert_eq!(
            wait_condition_predicates(&WaitCondition::Present),
            [ActionabilityPredicate::Present]
        );
        assert_eq!(
            wait_condition_predicates(&WaitCondition::Visible),
            [ActionabilityPredicate::Visible]
        );
        assert_eq!(
            wait_condition_predicates(&WaitCondition::Enabled),
            [ActionabilityPredicate::Enabled]
        );
        assert_eq!(
            wait_condition_predicates(&WaitCondition::Editable),
            [ActionabilityPredicate::Editable]
        );
        assert_eq!(
            wait_condition_predicates(&WaitCondition::Actionable),
            [
                ActionabilityPredicate::Present,
                ActionabilityPredicate::Visible,
                ActionabilityPredicate::Enabled,
                ActionabilityPredicate::Stable,
                ActionabilityPredicate::ReceivesEvents,
                ActionabilityPredicate::UnobscuredCenter,
            ]
        );
        assert_eq!(
            wait_condition_predicates(&WaitCondition::Stable),
            [ActionabilityPredicate::Stable]
        );
        assert_eq!(
            wait_condition_predicates(&WaitCondition::ReceivesEvents),
            [ActionabilityPredicate::ReceivesEvents]
        );
        assert_eq!(
            wait_condition_predicates(&WaitCondition::TextContains),
            [ActionabilityPredicate::TextContains]
        );
        assert_eq!(
            wait_condition_predicates(&WaitCondition::ValueEquals),
            [ActionabilityPredicate::ValueEquals]
        );
        assert!(wait_condition_predicates(&WaitCondition::RevisionChanged).is_empty());
        assert!(wait_condition_predicates(&WaitCondition::NavigationSettled).is_empty());
    }

    #[test]
    fn test_wait_condition_predicates_cover_every_targeted_wait_condition() {
        let targeted_conditions = [
            WaitCondition::Present,
            WaitCondition::Visible,
            WaitCondition::Enabled,
            WaitCondition::Editable,
            WaitCondition::Actionable,
            WaitCondition::Stable,
            WaitCondition::ReceivesEvents,
            WaitCondition::TextContains,
            WaitCondition::ValueEquals,
        ];

        for condition in targeted_conditions {
            assert!(
                !wait_condition_predicates(&condition).is_empty(),
                "expected shared predicates for '{}'",
                condition_name(&condition),
            );
        }

        assert_eq!(
            wait_condition_predicates(&WaitCondition::Actionable)
                .iter()
                .map(|predicate| predicate.key())
                .collect::<Vec<_>>(),
            vec![
                "present",
                "visible",
                "enabled",
                "stable",
                "receives_events",
                "unobscured_center",
            ]
        );
        assert_eq!(
            wait_condition_predicates(&WaitCondition::Stable)[0].key(),
            "stable"
        );
        assert_eq!(
            wait_condition_predicates(&WaitCondition::ReceivesEvents)[0].key(),
            "receives_events"
        );
        assert_eq!(
            wait_condition_predicates(&WaitCondition::TextContains)[0].key(),
            "text_contains"
        );
        assert_eq!(
            wait_condition_predicates(&WaitCondition::ValueEquals)[0].key(),
            "value_equals"
        );
    }

    #[test]
    fn test_wait_condition_matches_requires_all_actionable_predicates() {
        let probe = crate::tools::actionability::ActionabilityProbeResult {
            present: true,
            visible: Some(true),
            enabled: Some(true),
            editable: None,
            stable: Some(true),
            receives_events: Some(true),
            in_viewport: None,
            unobscured_center: Some(true),
            text_contains: None,
            value_equals: None,
            frame_depth: Some(0),
            diagnostics: None,
        };
        assert!(wait_condition_matches(
            &WaitCondition::Actionable,
            wait_condition_predicates(&WaitCondition::Actionable),
            &probe
        ));

        let obscured = crate::tools::actionability::ActionabilityProbeResult {
            unobscured_center: Some(false),
            ..probe
        };
        assert!(!wait_condition_matches(
            &WaitCondition::Actionable,
            wait_condition_predicates(&WaitCondition::Actionable),
            &obscured
        ));
    }

    #[test]
    fn test_wait_condition_uses_interaction_scroll_for_event_delivery_checks() {
        assert!(wait_condition_uses_interaction_scroll(
            &WaitCondition::Actionable
        ));
        assert!(wait_condition_uses_interaction_scroll(
            &WaitCondition::ReceivesEvents
        ));
        assert!(!wait_condition_uses_interaction_scroll(
            &WaitCondition::Visible
        ));
        assert!(!wait_condition_uses_interaction_scroll(
            &WaitCondition::Stable
        ));
    }

    #[test]
    fn test_wait_tool_navigation_settled_executes_against_fake_backend() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let tool = WaitTool;
        let mut context = ToolContext::new(&session);

        let result = tool
            .execute_typed(
                WaitParams {
                    selector: None,
                    index: None,
                    node_ref: None,
                    cursor: None,
                    condition: WaitCondition::NavigationSettled,
                    text: None,
                    value: None,
                    since_revision: None,
                    timeout_ms: 100,
                },
                &mut context,
            )
            .expect("navigation_settled should succeed");

        assert!(result.success);
        let data = result.data.expect("wait should include data");
        assert_eq!(data["condition"].as_str(), Some("navigation_settled"));
        assert_eq!(data["document"]["ready_state"].as_str(), Some("complete"));
    }

    #[test]
    fn test_wait_tool_rejects_stale_cursor_for_targeted_wait() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let dom = session.extract_dom().expect("fake DOM should extract");
        let mut stale_cursor = dom.cursor_for_index(0).expect("cursor should exist");
        stale_cursor.node_ref.revision = "fake:0".to_string();
        let tool = WaitTool;
        let mut context = ToolContext::with_dom(&session, dom);

        let result = tool
            .execute_typed(
                WaitParams {
                    selector: None,
                    index: None,
                    node_ref: None,
                    cursor: Some(stale_cursor),
                    condition: WaitCondition::Present,
                    text: None,
                    value: None,
                    since_revision: None,
                    timeout_ms: 1,
                },
                &mut context,
            )
            .expect("stale cursor should stay a tool failure");

        assert!(!result.success);
        let data = result
            .data
            .expect("stale cursor failure should include details");
        assert_eq!(data["code"].as_str(), Some("stale_node_ref"));
        assert_eq!(
            data["details"]["resolution"]["recovered_from"].as_str(),
            Some("cursor")
        );
        assert_eq!(
            data["details"]["resolution"]["selector_rebound_attempted"].as_bool(),
            Some(false)
        );
    }

    #[test]
    fn test_wait_tool_rejects_timeout_above_resource_cap() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let tool = WaitTool;
        let mut context = ToolContext::new(&session);

        let err = tool
            .execute_typed(
                WaitParams {
                    selector: None,
                    index: None,
                    node_ref: None,
                    cursor: None,
                    condition: WaitCondition::NavigationSettled,
                    text: None,
                    value: None,
                    since_revision: None,
                    timeout_ms: MAX_WAIT_TIMEOUT_MS + 1,
                },
                &mut context,
            )
            .expect_err("oversized timeout should be rejected");

        assert!(matches!(err, BrowserError::InvalidArgument(_)));
        assert!(err.to_string().contains("wait.timeout_ms"));
        assert!(err.to_string().contains(&MAX_WAIT_TIMEOUT_MS.to_string()));

        let result = session
            .execute_tool(
                "wait",
                json!({
                    "timeout_ms": MAX_WAIT_TIMEOUT_MS + 1
                }),
            )
            .expect("wait should execute through registry");
        assert!(!result.success);
        let data = result.data.expect("invalid argument should be structured");
        assert_eq!(data["code"].as_str(), Some("invalid_argument"));
        assert!(
            data["error"]
                .as_str()
                .unwrap_or_default()
                .contains("wait.timeout_ms")
        );
    }
}

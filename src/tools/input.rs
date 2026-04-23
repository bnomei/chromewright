use crate::dom::{Cursor, NodeRef};
use crate::error::{BrowserError, Result};
use crate::tools::{
    TargetResolution, Tool, ToolContext, ToolResult,
    actionability::ActionabilityPredicate,
    browser_kernel::render_browser_kernel_script,
    core::PublicTarget,
    core::TargetedActionResult,
    services::interaction::{
        ActionabilityWaitState, DEFAULT_ACTIONABILITY_TIMEOUT_MS, build_actionability_failure,
        build_interaction_failure, build_interaction_handoff, decode_action_result,
        resolve_interaction_target, wait_for_actionability,
    },
};
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::de::Deserializer;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

const INPUT_JS: &str = include_str!("input.js");

#[derive(Debug, Clone, Serialize)]
pub struct InputParams {
    /// CSS selector (use either this or index, not both)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,

    /// Element index from DOM tree (use either this or selector, not both)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,

    /// Revision-scoped node reference from the snapshot tool
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_ref: Option<NodeRef>,

    /// Cursor from the snapshot or inspect_node tools
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,

    /// Text to type into the element.
    pub text: String,

    /// Clear existing content first (default: false)
    #[serde(default)]
    pub clear: bool,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct StrictInputParams {
    /// Target to type into.
    pub target: PublicTarget,
    /// Text to type into the element.
    pub text: String,
    /// Clear existing content first (default: false)
    #[serde(default)]
    pub clear: bool,
}

impl From<StrictInputParams> for InputParams {
    fn from(params: StrictInputParams) -> Self {
        let (selector, cursor) = params.target.into_selector_or_cursor();
        Self {
            selector,
            index: None,
            node_ref: None,
            cursor,
            text: params.text,
            clear: params.clear,
        }
    }
}

impl<'de> Deserialize<'de> for InputParams {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        StrictInputParams::deserialize(deserializer).map(Into::into)
    }
}

impl JsonSchema for InputParams {
    fn schema_name() -> Cow<'static, str> {
        "InputParams".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        StrictInputParams::json_schema(generator)
    }
}

#[derive(Default)]
pub struct InputTool;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InputOutput {
    #[serde(flatten)]
    pub result: TargetedActionResult,
    pub text: String,
    pub clear: bool,
}

impl Tool for InputTool {
    type Params = InputParams;
    type Output = InputOutput;

    fn name(&self) -> &str {
        "input"
    }

    fn description(&self) -> &str {
        "Type into an input after snapshot. Then press_key, click, or wait as the page reacts."
    }

    fn execute_typed(&self, params: InputParams, context: &mut ToolContext) -> Result<ToolResult> {
        let InputParams {
            selector,
            index,
            node_ref,
            cursor,
            text,
            clear,
        } = params;
        let target = match resolve_interaction_target(
            "input", selector, index, node_ref, cursor, context,
        )? {
            TargetResolution::Resolved(target) => target,
            TargetResolution::Failure(failure) => return Ok(context.finish(failure)),
        };

        let predicates = input_actionability_predicates();
        match wait_for_actionability(
            context,
            &target,
            predicates,
            DEFAULT_ACTIONABILITY_TIMEOUT_MS,
        )? {
            ActionabilityWaitState::Ready => {}
            ActionabilityWaitState::TimedOut(probe) => {
                return build_actionability_failure(
                    "input",
                    context.session,
                    &target,
                    &probe,
                    predicates,
                    None,
                )
                .map(|result| context.finish(result));
            }
        }

        let input_config = serde_json::json!({
            "selector": target.selector,
            "target_index": target.cursor.as_ref().map(|cursor| cursor.index).or(target.index),
            "text": text,
            "clear": clear,
        });
        let input_js = build_input_js(&input_config);
        context.record_browser_evaluation();
        let result = context
            .session
            .evaluate(&input_js, false)
            .map_err(|e| match e {
                BrowserError::EvaluationFailed(reason) => BrowserError::ToolExecutionFailed {
                    tool: "input".to_string(),
                    reason,
                },
                other => other,
            })?;
        let action_result = decode_action_result(
            result.value,
            serde_json::json!({
                "success": false,
                "code": "target_detached",
                "error": "Element is no longer present"
            }),
        )?;

        if action_result["success"].as_bool() != Some(true) {
            return build_interaction_failure(
                "input",
                context.session,
                &target,
                action_result["code"]
                    .as_str()
                    .unwrap_or("invalid_target")
                    .to_string(),
                action_result["error"]
                    .as_str()
                    .unwrap_or("Input failed")
                    .to_string(),
                Vec::new(),
                None,
            )
            .map(|result| context.finish(result));
        }

        let handoff = build_interaction_handoff(context, &target)?;
        Ok(context.finish(ToolResult::success_with(InputOutput {
            result: TargetedActionResult::new(
                "input",
                handoff.document,
                handoff.target_before,
                handoff.target_after,
                handoff.target_status,
            ),
            text,
            clear,
        })))
    }
}

fn build_input_js(config: &serde_json::Value) -> String {
    render_browser_kernel_script(INPUT_JS, "__INPUT_CONFIG__", config)
}

fn input_actionability_predicates() -> &'static [ActionabilityPredicate] {
    &[
        ActionabilityPredicate::Present,
        ActionabilityPredicate::Visible,
        ActionabilityPredicate::Enabled,
        ActionabilityPredicate::Editable,
        ActionabilityPredicate::Stable,
    ]
}

#[cfg(test)]
mod tests {
    use super::{InputParams, build_input_js};
    use schemars::schema_for;
    use serde_json::json;

    #[test]
    fn test_input_js_prefers_selector_before_target_index() {
        let input_js = build_input_js(&serde_json::json!({
            "selector": "#query",
            "target_index": 1,
            "text": "search",
            "clear": false,
        }));

        assert!(input_js.contains("function resolveTargetMatch(config, options)"));
        assert!(input_js.contains("const element = resolveTargetElement(config);"));
        assert!(input_js.contains("querySelectorAcrossScopes("));
        assert!(input_js.contains("searchActionableIndex(config.target_index)"));
    }

    #[test]
    fn test_input_params_deserializes_canonical_text_field() {
        let params: InputParams = serde_json::from_value(json!({
            "target": {
                "kind": "selector",
                "selector": "#query"
            },
            "text": "search",
            "clear": true,
        }))
        .expect("canonical text field should deserialize");

        assert_eq!(params.selector.as_deref(), Some("#query"));
        assert_eq!(params.text, "search");
        assert!(params.clear);
    }

    #[test]
    fn test_input_params_rejects_value_alias() {
        let error = serde_json::from_value::<InputParams>(json!({
            "target": {
                "kind": "selector",
                "selector": "#query"
            },
            "value": "search",
            "clear": false,
        }))
        .expect_err("legacy value alias should be rejected");

        assert!(error.to_string().contains("unknown field `value`"));
    }

    #[test]
    fn test_input_params_serializes_text_as_canonical_field() {
        let params = InputParams {
            selector: Some("#query".to_string()),
            index: None,
            node_ref: None,
            cursor: None,
            text: "search".to_string(),
            clear: false,
        };

        let serialized = serde_json::to_value(&params).expect("params should serialize");

        assert_eq!(serialized.get("text"), Some(&json!("search")));
        assert_eq!(serialized.get("value"), None);
    }

    #[test]
    fn test_input_params_schema_keeps_text_property_and_mentions_alias() {
        let schema = schema_for!(InputParams);
        let schema_json = serde_json::to_value(&schema).expect("schema should serialize");
        let properties = schema_json
            .get("properties")
            .and_then(|value| value.as_object())
            .expect("input params schema should expose properties");

        assert!(properties.contains_key("target"));
        assert!(properties.contains_key("text"));
        assert!(!properties.contains_key("value"));
        assert!(!properties.contains_key("selector"));
        assert!(!properties.contains_key("index"));
        assert!(!properties.contains_key("node_ref"));
        assert!(!properties.contains_key("cursor"));
    }
}

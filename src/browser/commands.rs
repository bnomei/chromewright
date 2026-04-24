use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::OnceLock;

const BROWSER_KERNEL_JS: &str = include_str!("../tools/browser_kernel.js");
const ACTIONABILITY_PROBE_TEMPLATE_JS: &str = include_str!("../tools/actionability_probe.js");
const SELECTOR_IDENTITY_TEMPLATE_JS: &str = r#"
(() => {
  const config = __SELECTOR_IDENTITY_CONFIG__;

  __BROWSER_KERNEL__

  function countSelectorMatchesAcrossScopes(selector) {
    const visitedDocs = new Set();
    let count = 0;

    function searchRoot(root) {
      if (!root || typeof root.querySelectorAll !== 'function') {
        return;
      }

      let matches = [];
      try {
        matches = root.querySelectorAll(selector);
      } catch (error) {
        const normalized = normalizeSimpleIdSelector(selector);
        if (!normalized) {
          return;
        }

        try {
          matches = root.querySelectorAll(normalized);
        } catch (fallbackError) {
          return;
        }
      }

      count += matches.length;
      if (count > 1) {
        return;
      }

      const elements = root.querySelectorAll ? root.querySelectorAll('*') : [];
      for (const element of elements) {
        if (element.shadowRoot) {
          searchRoot(element.shadowRoot);
          if (count > 1) {
            return;
          }
        }

        if (element.tagName === 'IFRAME') {
          try {
            const frameDoc = element.contentDocument;
            if (!frameDoc || visitedDocs.has(frameDoc)) {
              continue;
            }

            visitedDocs.add(frameDoc);
            searchRoot(frameDoc);
            if (count > 1) {
              return;
            }
          } catch (error) {
            // Cross-origin frame; selector identity stops at the iframe boundary.
          }
        }
      }
    }

    visitedDocs.add(document);
    searchRoot(document);
    return count;
  }

  const matchCount = config.selector ? countSelectorMatchesAcrossScopes(config.selector) : 0;

  return JSON.stringify({
    present: matchCount > 0,
    unique: matchCount === 1
  });
})()
"#;
const CLICK_TEMPLATE_JS: &str = include_str!("../tools/click.js");
const INPUT_TEMPLATE_JS: &str = include_str!("../tools/input.js");
const HOVER_TEMPLATE_JS: &str = include_str!("../tools/hover.js");
const SELECT_TEMPLATE_JS: &str = include_str!("../tools/select.js");

static ACTIONABILITY_PROBE_SHELL: OnceLock<BrowserCommandTemplateShell> = OnceLock::new();
static SELECTOR_IDENTITY_SHELL: OnceLock<BrowserCommandTemplateShell> = OnceLock::new();
static CLICK_SHELL: OnceLock<BrowserCommandTemplateShell> = OnceLock::new();
static INPUT_SHELL: OnceLock<BrowserCommandTemplateShell> = OnceLock::new();
static HOVER_SHELL: OnceLock<BrowserCommandTemplateShell> = OnceLock::new();
static SELECT_SHELL: OnceLock<BrowserCommandTemplateShell> = OnceLock::new();

struct BrowserCommandTemplateShell {
    prefix: String,
    suffix: String,
}

impl BrowserCommandTemplateShell {
    fn compile(template: &'static str, config_placeholder: &'static str) -> Self {
        let expanded = template.replace("__BROWSER_KERNEL__", BROWSER_KERNEL_JS);
        let mut parts = expanded.split(config_placeholder);
        let prefix = parts
            .next()
            .expect("expanded browser-command template should have a prefix");
        let suffix = parts
            .next()
            .expect("browser-command template must contain exactly one config placeholder");
        assert!(
            parts.next().is_none(),
            "browser-command template must contain exactly one config placeholder"
        );

        Self {
            prefix: prefix.to_string(),
            suffix: suffix.to_string(),
        }
    }

    fn render(&self, config: &Value) -> String {
        let config_json = config.to_string();
        let mut rendered =
            String::with_capacity(self.prefix.len() + config_json.len() + self.suffix.len());
        rendered.push_str(&self.prefix);
        rendered.push_str(&config_json);
        rendered.push_str(&self.suffix);
        rendered
    }
}

fn render_command_script(
    shell_cache: &OnceLock<BrowserCommandTemplateShell>,
    template: &'static str,
    config_placeholder: &'static str,
    config: &Value,
) -> String {
    shell_cache
        .get_or_init(|| BrowserCommandTemplateShell::compile(template, config_placeholder))
        .render(config)
}

#[derive(Debug, Clone)]
pub(crate) enum BrowserCommand {
    ActionabilityProbe(ActionabilityProbeRequest),
    SelectorIdentityProbe(SelectorIdentityProbeRequest),
    Interaction(InteractionCommand),
}

impl BrowserCommand {
    pub(crate) fn capability(&self) -> &'static str {
        match self {
            Self::ActionabilityProbe(_) => "actionability_probe",
            Self::SelectorIdentityProbe(_) => "selector_identity_probe",
            Self::Interaction(command) => command.capability(),
        }
    }

    pub(crate) fn operation(&self) -> &'static str {
        match self {
            Self::ActionabilityProbe(_) => "actionability_probe",
            Self::SelectorIdentityProbe(_) => "selector_identity_probe",
            Self::Interaction(command) => command.operation(),
        }
    }

    pub(crate) fn render_script(&self) -> String {
        match self {
            Self::ActionabilityProbe(request) => {
                let config = serde_json::json!({
                    "selector": request.selector,
                    "target_index": request.target_index,
                    "predicates": request
                        .predicates
                        .iter()
                        .map(|predicate| predicate.key())
                        .collect::<Vec<_>>(),
                    "text": request.expected_text,
                    "value": request.expected_value,
                });
                render_command_script(
                    &ACTIONABILITY_PROBE_SHELL,
                    ACTIONABILITY_PROBE_TEMPLATE_JS,
                    "__ACTIONABILITY_CONFIG__",
                    &config,
                )
            }
            Self::SelectorIdentityProbe(request) => {
                let config = serde_json::json!({ "selector": request.selector });
                render_command_script(
                    &SELECTOR_IDENTITY_SHELL,
                    SELECTOR_IDENTITY_TEMPLATE_JS,
                    "__SELECTOR_IDENTITY_CONFIG__",
                    &config,
                )
            }
            Self::Interaction(command) => command.render_script(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum BrowserCommandResult {
    ActionabilityProbe(ActionabilityProbeResult),
    SelectorIdentityProbe(SelectorIdentityProbeResult),
    Interaction(InteractionCommandResult),
}

#[derive(Debug, Clone)]
pub(crate) struct ActionabilityProbeRequest {
    pub selector: String,
    pub target_index: Option<usize>,
    pub predicates: Vec<ActionabilityPredicate>,
    pub expected_text: Option<String>,
    pub expected_value: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ActionabilityPredicate {
    Present,
    Visible,
    Enabled,
    Editable,
    Stable,
    ReceivesEvents,
    InViewport,
    UnobscuredCenter,
    TextContains,
    ValueEquals,
}

impl ActionabilityPredicate {
    pub(crate) const fn key(self) -> &'static str {
        match self {
            ActionabilityPredicate::Present => "present",
            ActionabilityPredicate::Visible => "visible",
            ActionabilityPredicate::Enabled => "enabled",
            ActionabilityPredicate::Editable => "editable",
            ActionabilityPredicate::Stable => "stable",
            ActionabilityPredicate::ReceivesEvents => "receives_events",
            ActionabilityPredicate::InViewport => "in_viewport",
            ActionabilityPredicate::UnobscuredCenter => "unobscured_center",
            ActionabilityPredicate::TextContains => "text_contains",
            ActionabilityPredicate::ValueEquals => "value_equals",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub(crate) struct ActionabilityProbeResult {
    pub present: bool,
    pub visible: Option<bool>,
    pub enabled: Option<bool>,
    pub editable: Option<bool>,
    pub stable: Option<bool>,
    pub receives_events: Option<bool>,
    pub in_viewport: Option<bool>,
    pub unobscured_center: Option<bool>,
    pub text_contains: Option<bool>,
    pub value_equals: Option<bool>,
    pub frame_depth: Option<usize>,
    pub diagnostics: Option<ActionabilityDiagnostics>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub(crate) struct ActionabilityDiagnostics {
    pub pointer_events: Option<String>,
    pub hit_target: Option<ActionabilityElementSummary>,
    pub text_length: Option<usize>,
    pub has_value: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub(crate) struct ActionabilityElementSummary {
    pub tag: String,
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub classes: Vec<String>,
}

impl ActionabilityProbeResult {
    pub(crate) fn predicate(&self, predicate: ActionabilityPredicate) -> Option<bool> {
        match predicate {
            ActionabilityPredicate::Present => Some(self.present),
            ActionabilityPredicate::Visible => self.visible,
            ActionabilityPredicate::Enabled => self.enabled,
            ActionabilityPredicate::Editable => self.editable,
            ActionabilityPredicate::Stable => self.stable,
            ActionabilityPredicate::ReceivesEvents => self.receives_events,
            ActionabilityPredicate::InViewport => self.in_viewport,
            ActionabilityPredicate::UnobscuredCenter => self.unobscured_center,
            ActionabilityPredicate::TextContains => self.text_contains,
            ActionabilityPredicate::ValueEquals => self.value_equals,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SelectorIdentityProbeRequest {
    pub selector: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct SelectorIdentityProbeResult {
    pub present: bool,
    pub unique: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum InteractionCommand {
    Click(TargetedInteractionRequest),
    Input(InputInteractionRequest),
    Hover(TargetedInteractionRequest),
    Select(SelectInteractionRequest),
}

impl InteractionCommand {
    fn capability(&self) -> &'static str {
        match self {
            Self::Click(_) => "click",
            Self::Input(_) => "input",
            Self::Hover(_) => "hover",
            Self::Select(_) => "select",
        }
    }

    fn operation(&self) -> &'static str {
        self.capability()
    }

    fn render_script(&self) -> String {
        match self {
            Self::Click(request) => render_interaction_script(
                &CLICK_SHELL,
                CLICK_TEMPLATE_JS,
                "__CLICK_CONFIG__",
                &request.config(),
            ),
            Self::Input(request) => render_interaction_script(
                &INPUT_SHELL,
                INPUT_TEMPLATE_JS,
                "__INPUT_CONFIG__",
                &serde_json::json!({
                    "selector": request.target.selector,
                    "target_index": request.target.target_index,
                    "text": request.text,
                    "clear": request.clear,
                }),
            ),
            Self::Hover(request) => render_interaction_script(
                &HOVER_SHELL,
                HOVER_TEMPLATE_JS,
                "__HOVER_CONFIG__",
                &request.config(),
            ),
            Self::Select(request) => render_interaction_script(
                &SELECT_SHELL,
                SELECT_TEMPLATE_JS,
                "__SELECT_CONFIG__",
                &serde_json::json!({
                    "selector": request.target.selector,
                    "target_index": request.target.target_index,
                    "value": request.value,
                }),
            ),
        }
    }
}

fn render_interaction_script(
    shell_cache: &OnceLock<BrowserCommandTemplateShell>,
    template: &'static str,
    config_placeholder: &'static str,
    config: &Value,
) -> String {
    render_command_script(shell_cache, template, config_placeholder, config)
}

#[derive(Debug, Clone)]
pub(crate) struct TargetedInteractionRequest {
    pub selector: String,
    pub target_index: Option<usize>,
}

impl TargetedInteractionRequest {
    fn config(&self) -> Value {
        serde_json::json!({
            "selector": self.selector,
            "target_index": self.target_index,
        })
    }
}

#[derive(Debug, Clone)]
pub(crate) struct InputInteractionRequest {
    pub target: TargetedInteractionRequest,
    pub text: String,
    pub clear: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct SelectInteractionRequest {
    pub target: TargetedInteractionRequest,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) enum InteractionCommandResult {
    Click(ActionCommandResult),
    Input(InputCommandResult),
    Hover(HoverCommandResult),
    Select(SelectCommandResult),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ActionCommandResult {
    pub success: bool,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct InputCommandResult {
    pub success: bool,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct HoverCommandResult {
    pub success: bool,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default, rename = "tagName")]
    pub tag_name: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default, rename = "className")]
    pub class_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct SelectCommandResult {
    pub success: bool,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default, rename = "selectedValue")]
    pub selected_value: Option<String>,
    #[serde(default, rename = "selectedText")]
    pub selected_text: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actionability_command_renders_existing_probe_template() {
        let script = BrowserCommand::ActionabilityProbe(ActionabilityProbeRequest {
            selector: "#save".to_string(),
            target_index: Some(1),
            predicates: vec![
                ActionabilityPredicate::Present,
                ActionabilityPredicate::Visible,
            ],
            expected_text: None,
            expected_value: None,
        })
        .render_script();

        assert!(script.contains("function resolveTargetMatch(config, options)"));
        assert!(script.contains("\"selector\":\"#save\""));
        assert!(script.contains("\"target_index\":1"));
        assert!(script.contains("\"predicates\":[\"present\",\"visible\"]"));
    }

    #[test]
    fn selector_identity_command_renders_cross_scope_probe() {
        let script = BrowserCommand::SelectorIdentityProbe(SelectorIdentityProbeRequest {
            selector: "#save".to_string(),
        })
        .render_script();

        assert!(script.contains("function countSelectorMatchesAcrossScopes(selector)"));
        assert!(script.contains("normalizeSimpleIdSelector(selector)"));
        assert!(script.contains("\"selector\":\"#save\""));
    }

    #[test]
    fn interaction_commands_render_existing_templates() {
        let target = TargetedInteractionRequest {
            selector: "#save".to_string(),
            target_index: Some(1),
        };
        let click =
            BrowserCommand::Interaction(InteractionCommand::Click(target.clone())).render_script();
        let input =
            BrowserCommand::Interaction(InteractionCommand::Input(InputInteractionRequest {
                target: target.clone(),
                text: "hello".to_string(),
                clear: true,
            }))
            .render_script();
        let hover =
            BrowserCommand::Interaction(InteractionCommand::Hover(target.clone())).render_script();
        let select =
            BrowserCommand::Interaction(InteractionCommand::Select(SelectInteractionRequest {
                target,
                value: "choice".to_string(),
            }))
            .render_script();

        assert!(click.contains("const element = resolveTargetElement(config);"));
        assert!(click.contains("searchActionableIndex(config.target_index)"));
        assert!(input.contains("\"text\":\"hello\""));
        assert!(input.contains("\"clear\":true"));
        assert!(hover.contains("const element = resolveTargetElement(config);"));
        assert!(select.contains("\"value\":\"choice\""));
    }
}

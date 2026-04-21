use crate::error::{BrowserError, Result};
use crate::tools::{
    DocumentEnvelopeOptions, TargetResolution, Tool, ToolContext, ToolResult,
    build_document_envelope, resolve_target,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parameters for the click tool
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ClickParams {
    /// CSS selector (use either this or index, not both)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,

    /// Element index from DOM tree (use either this or selector, not both)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,

    /// Revision-scoped node reference from the snapshot tool
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_ref: Option<crate::dom::NodeRef>,
}

/// Tool for clicking elements
#[derive(Default)]
pub struct ClickTool;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ClickOutput {
    #[serde(flatten)]
    pub envelope: crate::tools::DocumentEnvelope,
    pub action: String,
}

impl Tool for ClickTool {
    type Params = ClickParams;
    type Output = ClickOutput;

    fn name(&self) -> &str {
        "click"
    }

    fn execute_typed(&self, params: ClickParams, context: &mut ToolContext) -> Result<ToolResult> {
        let ClickParams {
            selector,
            index,
            node_ref,
        } = params;
        let target = {
            let dom = if index.is_some() || node_ref.is_some() {
                Some(context.get_dom()?)
            } else {
                None
            };
            match resolve_target("click", selector, index, node_ref, dom)? {
                TargetResolution::Resolved(target) => target,
                TargetResolution::Failure(failure) => return Ok(failure),
            }
        };

        let tab = context.session.tab()?;
        let element = context.session.find_element(&tab, &target.selector)?;
        element
            .click()
            .map_err(|e| BrowserError::ToolExecutionFailed {
                tool: "click".to_string(),
                reason: e.to_string(),
            })?;

        context.invalidate_dom();
        Ok(ToolResult::success_with(ClickOutput {
            envelope: build_document_envelope(
                context,
                Some(&target),
                DocumentEnvelopeOptions::minimal(),
            )?,
            action: "click".to_string(),
        }))
    }
}

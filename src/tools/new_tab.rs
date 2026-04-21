use crate::error::Result;
use crate::tools::utils::validate_navigation_url;
use crate::tools::{
    DocumentEnvelopeOptions, Tool, ToolContext, ToolResult, build_document_envelope,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parameters for the new_tab tool
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NewTabParams {
    /// URL to open in the new tab
    pub url: String,

    /// Allow non-web/unsafe absolute schemes such as data: or file:
    #[serde(default)]
    pub allow_unsafe: bool,
}

/// Tool for opening a new tab
#[derive(Default)]
pub struct NewTabTool;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NewTabOutput {
    #[serde(flatten)]
    pub envelope: crate::tools::DocumentEnvelope,
    pub action: String,
    pub url: String,
    pub message: String,
}

impl Tool for NewTabTool {
    type Params = NewTabParams;
    type Output = NewTabOutput;

    fn name(&self) -> &str {
        "new_tab"
    }

    fn execute_typed(&self, params: NewTabParams, context: &mut ToolContext) -> Result<ToolResult> {
        let normalized_url = validate_navigation_url(&params.url, params.allow_unsafe)?;
        context.session.open_tab(&normalized_url)?;
        context.invalidate_dom();
        Ok(ToolResult::success_with(NewTabOutput {
            envelope: build_document_envelope(context, None, DocumentEnvelopeOptions::minimal())?,
            action: "new_tab".to_string(),
            message: format!("Opened a new tab for {}", normalized_url),
            url: normalized_url,
        }))
    }
}

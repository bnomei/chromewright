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

impl Tool for NewTabTool {
    type Params = NewTabParams;

    fn name(&self) -> &str {
        "new_tab"
    }

    fn execute_typed(&self, params: NewTabParams, context: &mut ToolContext) -> Result<ToolResult> {
        let normalized_url = validate_navigation_url(&params.url, params.allow_unsafe)?;
        context.session.open_tab(&normalized_url)?;
        context.invalidate_dom();
        let mut payload = serde_json::to_value(build_document_envelope(
            context,
            None,
            DocumentEnvelopeOptions::minimal(),
        )?)?;
        if let serde_json::Value::Object(ref mut map) = payload {
            map.insert("action".to_string(), serde_json::json!("new_tab"));
            map.insert("url".to_string(), serde_json::json!(normalized_url.clone()));
            map.insert(
                "message".to_string(),
                serde_json::json!(format!("Opened a new tab for {}", normalized_url)),
            );
        }

        Ok(ToolResult::success_with(payload))
    }
}

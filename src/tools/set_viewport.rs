use crate::browser::backend::VIEWPORT_LARGE_DIMENSION_MAX;
use crate::browser::{
    ViewportEmulation, ViewportEmulationRequest, ViewportMetrics, ViewportOperationResult,
    ViewportOrientation, ViewportResetRequest,
};
use crate::error::{BrowserError, Result};
use crate::tools::{DocumentActionResult, Tool, ToolContext, ToolResult};
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct SetViewportParams {
    /// Viewport width in CSS pixels. Required unless reset is true.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    /// Viewport height in CSS pixels. Required unless reset is true.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    /// Device scale factor; must be a finite number greater than zero.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_scale_factor: Option<f64>,
    /// Simulate a mobile viewport.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mobile: Option<bool>,
    /// Enable touch emulation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub touch: Option<bool>,
    /// Optional screen orientation for the emulated viewport. Use snake_case values:
    /// portrait_primary, portrait_secondary, landscape_primary, or landscape_secondary.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orientation: Option<ViewportOrientation>,
    /// Optional stable tab identifier. Omit to target the active tab.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<String>,
    /// Explicitly allow viewport dimensions above the normal automation cap, up to the large-canvas cap.
    /// Use only for intentional large-canvas or regression-capture workloads.
    #[serde(default)]
    pub allow_large_viewport: bool,
    /// Reset viewport emulation. When true, only tab_id may also be supplied; omit width, height,
    /// device_scale_factor, mobile, touch, orientation, and allow_large_viewport.
    #[serde(default)]
    pub reset: bool,
}

impl JsonSchema for SetViewportParams {
    fn schema_name() -> Cow<'static, str> {
        "SetViewportParams".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        #[derive(JsonSchema)]
        #[serde(deny_unknown_fields)]
        #[allow(dead_code)]
        struct SetViewportParamsSchema {
            /// Viewport width in CSS pixels. Required unless reset is true.
            #[schemars(range(min = 1, max = VIEWPORT_LARGE_DIMENSION_MAX))]
            #[serde(skip_serializing_if = "Option::is_none")]
            width: Option<u32>,
            /// Viewport height in CSS pixels. Required unless reset is true.
            #[schemars(range(min = 1, max = VIEWPORT_LARGE_DIMENSION_MAX))]
            #[serde(skip_serializing_if = "Option::is_none")]
            height: Option<u32>,
            /// Device scale factor; must be a finite number greater than zero.
            #[schemars(extend("exclusiveMinimum" = 0.0))]
            #[serde(skip_serializing_if = "Option::is_none")]
            device_scale_factor: Option<f64>,
            /// Simulate a mobile viewport.
            #[serde(skip_serializing_if = "Option::is_none")]
            mobile: Option<bool>,
            /// Enable touch emulation.
            #[serde(skip_serializing_if = "Option::is_none")]
            touch: Option<bool>,
            /// Optional screen orientation for the emulated viewport. Use snake_case values:
            /// portrait_primary, portrait_secondary, landscape_primary, or landscape_secondary.
            #[serde(skip_serializing_if = "Option::is_none")]
            orientation: Option<ViewportOrientation>,
            /// Optional stable tab identifier. Omit to target the active tab.
            #[serde(skip_serializing_if = "Option::is_none")]
            tab_id: Option<String>,
            /// Explicitly allow viewport dimensions above the normal automation cap of 10000 CSS pixels, up to 10000000 CSS pixels.
            #[serde(default)]
            allow_large_viewport: bool,
            /// Reset viewport emulation. When true, only tab_id may also be supplied; omit width,
            /// height, device_scale_factor, mobile, touch, orientation, and allow_large_viewport.
            #[serde(default)]
            reset: bool,
        }

        SetViewportParamsSchema::json_schema(generator)
    }
}

#[derive(Default)]
pub struct SetViewportTool;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SetViewportOutput {
    #[serde(flatten)]
    pub result: DocumentActionResult,
    pub tab_id: String,
    pub reset: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emulation: Option<ViewportEmulation>,
    pub viewport_metrics_after: ViewportMetrics,
    /// Compatibility alias for `viewport_metrics_after`.
    pub viewport_after: ViewportMetrics,
    pub message: String,
}

enum SetViewportRequest {
    Apply(ViewportEmulationRequest),
    Reset(ViewportResetRequest),
}

impl Tool for SetViewportTool {
    type Params = SetViewportParams;
    type Output = SetViewportOutput;

    fn name(&self) -> &str {
        "set_viewport"
    }

    fn description(&self) -> &str {
        "Simulate/reset breakpoints; allow_large_viewport lifts CSS caps; returns viewport_metrics_after."
    }

    fn execute_typed(
        &self,
        params: SetViewportParams,
        context: &mut ToolContext,
    ) -> Result<ToolResult> {
        let reset = params.reset;
        let request = normalize_request(params)?;
        let operation = match request {
            SetViewportRequest::Apply(request) => {
                context.session.apply_viewport_emulation(request)?
            }
            SetViewportRequest::Reset(request) => {
                context.session.reset_viewport_emulation(request)?
            }
        };

        context.invalidate_dom();
        // The emulation may have targeted an inactive tab via tab_id. Source the
        // document envelope from that same tab so the response's document
        // identity (document_id/revision/url) describes the emulated tab rather
        // than whichever tab happens to be active.
        context.record_browser_evaluation();
        let document = context
            .session
            .document_metadata_for_tab(&operation.tab_id)?;

        Ok(context.finish(ToolResult::success_with(SetViewportOutput {
            result: DocumentActionResult::new("set_viewport", document),
            tab_id: operation.tab_id.clone(),
            reset,
            emulation: operation.emulation.clone(),
            viewport_metrics_after: operation.viewport_after.clone(),
            viewport_after: operation.viewport_after.clone(),
            message: viewport_message(&operation, reset),
        })))
    }
}

fn normalize_request(params: SetViewportParams) -> Result<SetViewportRequest> {
    validate_tab_id(params.tab_id.as_deref())?;

    if params.reset {
        if params.width.is_some()
            || params.height.is_some()
            || params.device_scale_factor.is_some()
            || params.mobile.is_some()
            || params.touch.is_some()
            || params.orientation.is_some()
            || params.allow_large_viewport
        {
            return Err(BrowserError::InvalidArgument(
                "reset=true only accepts tab_id; omit width, height, allow_large_viewport, and other emulation fields"
                    .to_string(),
            ));
        }

        return Ok(SetViewportRequest::Reset(ViewportResetRequest {
            tab_id: params.tab_id,
        }));
    }

    let width = params.width.ok_or_else(|| {
        BrowserError::InvalidArgument("set_viewport requires width when reset is false".to_string())
    })?;
    let height = params.height.ok_or_else(|| {
        BrowserError::InvalidArgument(
            "set_viewport requires height when reset is false".to_string(),
        )
    })?;

    Ok(SetViewportRequest::Apply(ViewportEmulationRequest {
        width,
        height,
        device_scale_factor: params.device_scale_factor.unwrap_or(1.0),
        mobile: params.mobile.unwrap_or(false),
        touch: params.touch.unwrap_or(false),
        orientation: params.orientation,
        tab_id: params.tab_id,
        allow_large_viewport: params.allow_large_viewport,
    }))
}

fn validate_tab_id(tab_id: Option<&str>) -> Result<()> {
    if let Some(tab_id) = tab_id
        && tab_id.trim().is_empty()
    {
        return Err(BrowserError::InvalidArgument(
            "set_viewport tab_id cannot be empty".to_string(),
        ));
    }

    Ok(())
}

fn viewport_message(result: &ViewportOperationResult, reset: bool) -> String {
    if reset {
        return format!("Reset viewport emulation on tab {}.", result.tab_id);
    }

    let emulation = result
        .emulation
        .as_ref()
        .expect("apply viewport output should include emulation");
    format!(
        "Set viewport on tab {} to {}x{} @{}x.",
        result.tab_id, emulation.width, emulation.height, emulation.device_scale_factor
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::BrowserSession;
    use crate::browser::backend::FakeSessionBackend;

    fn read_viewport_metrics(session: &BrowserSession) -> (f64, f64, f64) {
        let metrics = session
            .viewport_metrics(None)
            .expect("viewport metrics should be readable");

        (metrics.width, metrics.height, metrics.device_pixel_ratio)
    }

    #[test]
    fn test_set_viewport_tool_executes_against_fake_backend() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let tool = SetViewportTool;
        let mut context = ToolContext::new(&session);

        let result = tool
            .execute_typed(
                SetViewportParams {
                    width: Some(390),
                    height: Some(844),
                    device_scale_factor: Some(3.0),
                    mobile: Some(true),
                    touch: Some(true),
                    orientation: Some(ViewportOrientation::PortraitPrimary),
                    tab_id: None,
                    allow_large_viewport: false,
                    reset: false,
                },
                &mut context,
            )
            .expect("set_viewport should succeed");

        assert!(result.success);
        let data = result.data.expect("set_viewport should include data");
        assert_eq!(data["action"].as_str(), Some("set_viewport"));
        assert_eq!(data["tab_id"].as_str(), Some("tab-1"));
        assert_eq!(data["reset"].as_bool(), Some(false));
        assert_eq!(data["emulation"]["width"].as_u64(), Some(390));
        assert_eq!(data["emulation"]["height"].as_u64(), Some(844));
        assert_eq!(data["viewport_metrics_after"], data["viewport_after"]);
        assert_eq!(
            data["viewport_metrics_after"]["width"].as_f64(),
            Some(390.0)
        );
        assert_eq!(
            data["viewport_metrics_after"]["height"].as_f64(),
            Some(844.0)
        );
        assert_eq!(
            data["viewport_metrics_after"]["device_pixel_ratio"].as_f64(),
            Some(3.0)
        );
        assert_eq!(data["viewport_after"]["width"].as_f64(), Some(390.0));
        assert_eq!(data["viewport_after"]["height"].as_f64(), Some(844.0));
        assert_eq!(
            data["viewport_after"]["device_pixel_ratio"].as_f64(),
            Some(3.0)
        );
        assert_eq!(read_viewport_metrics(&session), (390.0, 844.0, 3.0));
    }

    #[test]
    fn test_set_viewport_tool_reset_executes_against_fake_backend() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        session
            .apply_viewport_emulation(ViewportEmulationRequest {
                width: 390,
                height: 844,
                device_scale_factor: 2.0,
                mobile: true,
                touch: true,
                orientation: Some(ViewportOrientation::PortraitPrimary),
                tab_id: None,
                allow_large_viewport: false,
            })
            .expect("viewport emulation should seed");
        let tool = SetViewportTool;
        let mut context = ToolContext::new(&session);

        let result = tool
            .execute_typed(
                SetViewportParams {
                    reset: true,
                    ..SetViewportParams::default()
                },
                &mut context,
            )
            .expect("set_viewport reset should succeed");

        assert!(result.success);
        let data = result.data.expect("reset should include data");
        assert_eq!(data["reset"].as_bool(), Some(true));
        assert!(data["emulation"].is_null());
        assert_eq!(data["viewport_metrics_after"], data["viewport_after"]);
        assert_eq!(data["viewport_after"]["width"].as_f64(), Some(800.0));
        assert_eq!(data["viewport_after"]["height"].as_f64(), Some(600.0));
        assert_eq!(read_viewport_metrics(&session), (800.0, 600.0, 2.0));
    }

    #[test]
    fn test_set_viewport_tool_returns_targeted_tab_document_not_active_tab() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let first_tab_id = session.list_tabs().expect("tabs should list")[0].id.clone();
        let second_tab = session
            .open_tab_entry("https://second.example")
            .expect("second tab should open");
        // Opening the second tab makes it active; the first tab is now inactive.

        let tool = SetViewportTool;
        let mut context = ToolContext::new(&session);
        let result = tool
            .execute_typed(
                SetViewportParams {
                    width: Some(375),
                    height: Some(812),
                    tab_id: Some(first_tab_id.clone()),
                    ..SetViewportParams::default()
                },
                &mut context,
            )
            .expect("set_viewport against inactive tab should succeed");

        assert!(result.success);
        let data = result.data.expect("set_viewport should include data");
        assert_eq!(data["tab_id"].as_str(), Some(first_tab_id.as_str()));
        // The document envelope must describe the emulated (first) tab, not the
        // active (second) tab.
        assert_eq!(
            data["document"]["document_id"].as_str(),
            Some(first_tab_id.as_str())
        );
        assert_eq!(data["document"]["url"].as_str(), Some("about:blank"));
        assert_ne!(data["document"]["document_id"].as_str(), Some(second_tab.id.as_str()));
    }

    #[test]
    fn test_set_viewport_tool_rejects_invalid_reset_combinations_before_mutation() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let tool = SetViewportTool;
        let mut context = ToolContext::new(&session);

        let error = tool
            .execute_typed(
                SetViewportParams {
                    width: Some(320),
                    reset: true,
                    ..SetViewportParams::default()
                },
                &mut context,
            )
            .expect_err("invalid reset combination should fail");

        assert!(
            error.to_string().contains("reset=true only accepts tab_id"),
            "unexpected error: {error}"
        );
        assert_eq!(read_viewport_metrics(&session), (800.0, 600.0, 2.0));
    }
}

use crate::browser::backend::ScreenshotScale as BrowserScreenshotScale;
use crate::browser::{
    ScreenshotClip as BrowserScreenshotClip, ScreenshotMode as BrowserCaptureMode,
    ScreenshotRequest,
};
use crate::error::{BrowserError, Result};
use crate::tools::core::{
    PublicTarget, TargetResolution, resolve_target_with_cursor, structured_tool_failure,
};
use crate::tools::inspect_node::{
    InspectBoundingBox, InspectLayout, InspectNodeProbePayload, build_inspect_node_js,
    decode_probe_payload,
};
use crate::tools::{Tool, ToolContext, ToolResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(
    Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum ScreenshotMode {
    #[default]
    Viewport,
    FullPage,
    Element,
    Region,
}

#[derive(
    Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum ScreenshotScale {
    #[default]
    Device,
    Css,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct ScreenshotRegion {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct ScreenshotClip {
    pub coordinate_space: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScreenshotParams {
    #[serde(default)]
    pub mode: ScreenshotMode,

    #[serde(default)]
    pub scale: ScreenshotScale,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<PublicTarget>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<ScreenshotRegion>,
}

#[derive(Default)]
pub struct ScreenshotTool;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ScreenshotOutput {
    pub mode: ScreenshotMode,
    pub scale: ScreenshotScale,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<String>,
    pub artifact_uri: String,
    pub artifact_path: String,
    pub format: String,
    pub mime_type: String,
    pub byte_count: usize,
    pub width: u32,
    pub height: u32,
    pub css_width: f64,
    pub css_height: f64,
    pub device_pixel_ratio: f64,
    pub pixel_scale: f64,
    pub revealed_from_offscreen: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clip: Option<ScreenshotClip>,
}

#[derive(Debug, Clone)]
struct PreparedCapture {
    output_mode: ScreenshotMode,
    output_scale: ScreenshotScale,
    revealed_from_offscreen: bool,
    request: ScreenshotRequest,
}

enum CapturePreparation {
    Ready(PreparedCapture),
    Failure(ToolResult),
}

#[derive(Debug, Deserialize)]
struct RevealTargetPayload {
    success: bool,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    scroll_y_before: Option<f64>,
    #[serde(default)]
    scroll_y_after: Option<f64>,
    #[serde(default)]
    visible_in_viewport: Option<bool>,
}

impl From<ScreenshotScale> for BrowserScreenshotScale {
    fn from(value: ScreenshotScale) -> Self {
        match value {
            ScreenshotScale::Device => BrowserScreenshotScale::Device,
            ScreenshotScale::Css => BrowserScreenshotScale::Css,
        }
    }
}

impl Tool for ScreenshotTool {
    type Params = ScreenshotParams;
    type Output = ScreenshotOutput;

    fn name(&self) -> &str {
        "screenshot"
    }

    fn description(&self) -> &str {
        "Capture a managed PNG. target uses selector/cursor objects; mode and scale pick scope."
    }

    fn execute_typed(
        &self,
        params: ScreenshotParams,
        context: &mut ToolContext,
    ) -> Result<ToolResult> {
        let prepared = match prepare_capture(params, context)? {
            CapturePreparation::Ready(prepared) => prepared,
            CapturePreparation::Failure(result) => return Ok(context.finish(result)),
        };
        let PreparedCapture {
            output_mode,
            output_scale,
            revealed_from_offscreen,
            request,
        } = prepared;
        let (artifact, capture) = context
            .session
            .capture_screenshot_artifact_with_capture(request)?;

        Ok(context.finish(ToolResult::success_with(ScreenshotOutput {
            mode: output_mode,
            scale: output_scale,
            tab_id: Some(artifact.tab_id.clone()),
            artifact_uri: artifact.uri.clone(),
            artifact_path: artifact.path.display().to_string(),
            format: "png".to_string(),
            mime_type: artifact.mime_type.to_string(),
            byte_count: artifact.byte_count,
            width: artifact.width,
            height: artifact.height,
            css_width: capture.css_width,
            css_height: capture.css_height,
            device_pixel_ratio: capture.device_pixel_ratio,
            pixel_scale: capture.pixel_scale,
            revealed_from_offscreen,
            clip: artifact.clip.as_ref().map(|clip| ScreenshotClip {
                coordinate_space: "viewport_css_pixels".to_string(),
                x: clip.x,
                y: clip.y,
                width: clip.width,
                height: clip.height,
            }),
        })))
    }
}

fn prepare_capture(
    params: ScreenshotParams,
    context: &mut ToolContext<'_>,
) -> Result<CapturePreparation> {
    validate_tab_id(params.tab_id.as_deref())?;
    let output_scale = params.scale;
    let request_scale: BrowserScreenshotScale = params.scale.into();

    match params.mode {
        ScreenshotMode::Viewport => {
            reject_target_and_region(&params, ScreenshotMode::Viewport)?;
            Ok(CapturePreparation::Ready(PreparedCapture {
                output_mode: ScreenshotMode::Viewport,
                output_scale,
                revealed_from_offscreen: false,
                request: ScreenshotRequest {
                    mode: BrowserCaptureMode::Viewport,
                    scale: request_scale,
                    tab_id: params.tab_id,
                    clip: None,
                },
            }))
        }
        ScreenshotMode::FullPage => {
            reject_target_and_region(&params, ScreenshotMode::FullPage)?;
            Ok(CapturePreparation::Ready(PreparedCapture {
                output_mode: ScreenshotMode::FullPage,
                output_scale,
                revealed_from_offscreen: false,
                request: ScreenshotRequest {
                    mode: BrowserCaptureMode::FullPage,
                    scale: request_scale,
                    tab_id: params.tab_id,
                    clip: None,
                },
            }))
        }
        ScreenshotMode::Element => {
            let Some(target) = params.target else {
                return Err(BrowserError::InvalidArgument(
                    "screenshot mode 'element' requires target".to_string(),
                ));
            };
            if params.region.is_some() {
                return Err(BrowserError::InvalidArgument(
                    "screenshot mode 'element' does not accept region".to_string(),
                ));
            }

            inspect_element_target(target, params.tab_id, output_scale, context)
        }
        ScreenshotMode::Region => {
            let Some(region) = params.region else {
                return Err(BrowserError::InvalidArgument(
                    "screenshot mode 'region' requires region".to_string(),
                ));
            };
            if params.target.is_some() {
                return Err(BrowserError::InvalidArgument(
                    "screenshot mode 'region' does not accept target".to_string(),
                ));
            }
            Ok(CapturePreparation::Ready(PreparedCapture {
                output_mode: ScreenshotMode::Region,
                output_scale,
                revealed_from_offscreen: false,
                request: ScreenshotRequest {
                    mode: BrowserCaptureMode::Viewport,
                    scale: request_scale,
                    tab_id: params.tab_id,
                    clip: Some(region_to_clip(&region)?),
                },
            }))
        }
    }
}

fn reject_target_and_region(params: &ScreenshotParams, mode: ScreenshotMode) -> Result<()> {
    if params.target.is_some() {
        return Err(BrowserError::InvalidArgument(format!(
            "screenshot mode '{}' does not accept target",
            mode_label(mode)
        )));
    }

    if params.region.is_some() {
        return Err(BrowserError::InvalidArgument(format!(
            "screenshot mode '{}' does not accept region",
            mode_label(mode)
        )));
    }

    Ok(())
}

fn validate_tab_id(tab_id: Option<&str>) -> Result<()> {
    if let Some(tab_id) = tab_id
        && tab_id.trim().is_empty()
    {
        return Err(BrowserError::InvalidArgument(
            "screenshot tab_id must not be empty".to_string(),
        ));
    }

    Ok(())
}

fn mode_label(mode: ScreenshotMode) -> &'static str {
    match mode {
        ScreenshotMode::Viewport => "viewport",
        ScreenshotMode::FullPage => "full_page",
        ScreenshotMode::Element => "element",
        ScreenshotMode::Region => "region",
    }
}

fn inspect_element_target(
    target: PublicTarget,
    tab_id: Option<String>,
    scale: ScreenshotScale,
    context: &mut ToolContext<'_>,
) -> Result<CapturePreparation> {
    let (selector, cursor) = target.into_selector_or_cursor();
    let target = match tab_id.as_deref() {
        Some(tab_id) => {
            let dom = context.session.extract_dom_for_tab(tab_id)?;
            match resolve_target_with_cursor(
                "screenshot",
                selector,
                None,
                None,
                cursor,
                Some(&dom),
            )? {
                TargetResolution::Resolved(target) => target,
                TargetResolution::Failure(failure) => {
                    return Ok(CapturePreparation::Failure(failure));
                }
            }
        }
        None => {
            let dom = context.get_dom()?;
            match resolve_target_with_cursor("screenshot", selector, None, None, cursor, Some(dom))?
            {
                TargetResolution::Resolved(target) => target,
                TargetResolution::Failure(failure) => {
                    return Ok(CapturePreparation::Failure(failure));
                }
            }
        }
    };

    let target_index = target
        .cursor
        .as_ref()
        .map(|cursor| cursor.index)
        .or(target.index);
    let mut payload =
        inspect_target_payload(&target.selector, target_index, tab_id.as_deref(), context)?;
    let mut layout = match extract_target_layout_or_failure(&target, &payload)? {
        Ok(layout) => layout,
        Err(failure) => return Ok(CapturePreparation::Failure(failure)),
    };
    let mut revealed_from_offscreen = false;

    if !layout.visible_in_viewport {
        if !layout.visible || layout.bounding_box.width <= 0.0 || layout.bounding_box.height <= 0.0
        {
            return Ok(CapturePreparation::Failure(screenshot_viewport_failure(
                "target_not_visible",
                "Element could not be captured because it is not visible",
                &target,
                &layout,
                false,
                None,
            )));
        }

        let reveal = reveal_target_in_viewport(&target.selector, tab_id.as_deref(), context)?;
        if !reveal.success {
            let error = reveal
                .error
                .clone()
                .unwrap_or_else(|| "Element could not be scrolled into the viewport".to_string());
            let code = reveal
                .code
                .clone()
                .unwrap_or_else(|| "target_not_in_viewport".to_string());
            return Ok(CapturePreparation::Failure(screenshot_viewport_failure(
                code.as_str(),
                error,
                &target,
                &layout,
                true,
                Some(&reveal),
            )));
        }

        payload =
            inspect_target_payload(&target.selector, target_index, tab_id.as_deref(), context)?;
        layout = match extract_target_layout_or_failure(&target, &payload)? {
            Ok(layout) => layout,
            Err(failure) => return Ok(CapturePreparation::Failure(failure)),
        };
        if !layout.visible_in_viewport {
            return Ok(CapturePreparation::Failure(screenshot_viewport_failure(
                "target_not_in_viewport",
                "Element remained outside the viewport after a reveal attempt",
                &target,
                &layout,
                true,
                Some(&reveal),
            )));
        }

        revealed_from_offscreen = true;
    }

    Ok(CapturePreparation::Ready(PreparedCapture {
        output_mode: ScreenshotMode::Element,
        output_scale: scale,
        revealed_from_offscreen,
        request: ScreenshotRequest {
            mode: BrowserCaptureMode::Viewport,
            scale: scale.into(),
            tab_id,
            clip: Some(bounding_box_to_clip(&layout.bounding_box)?),
        },
    }))
}

fn inspect_target_payload(
    selector: &str,
    target_index: Option<usize>,
    tab_id: Option<&str>,
    context: &mut ToolContext<'_>,
) -> Result<InspectNodeProbePayload> {
    let inspect_js = build_inspect_node_js(&json!({
        "selector": selector,
        "target_index": target_index,
        "detail": "compact",
        "style_names": [],
    }));
    context.record_browser_evaluation();
    let evaluation = match tab_id {
        Some(tab_id) => context.session.evaluate_on_tab(tab_id, &inspect_js, false),
        None => context.session.evaluate(&inspect_js, false),
    }
    .map_err(screenshot_evaluation_error)?;
    decode_probe_payload(evaluation.value)
}

fn extract_target_layout_or_failure(
    target: &crate::tools::ResolvedTarget,
    payload: &InspectNodeProbePayload,
) -> Result<std::result::Result<InspectLayout, ToolResult>> {
    if !payload.success {
        let error = payload
            .error
            .clone()
            .unwrap_or_else(|| "Node inspection failed".to_string());
        return Ok(Err(structured_tool_failure(
            payload
                .code
                .clone()
                .unwrap_or_else(|| "inspect_failed".to_string()),
            error,
            None,
            Some(target.to_target_envelope()),
            None,
            Some(serde_json::json!({
                "boundaries": payload.boundaries.clone().unwrap_or_default(),
            })),
        )));
    }

    let layout = payload
        .layout
        .clone()
        .ok_or_else(|| BrowserError::ToolExecutionFailed {
            tool: "screenshot".to_string(),
            reason: "inspect_node probe did not return layout details".to_string(),
        })?;
    Ok(Ok(layout))
}

fn reveal_target_in_viewport(
    selector: &str,
    tab_id: Option<&str>,
    context: &mut ToolContext<'_>,
) -> Result<RevealTargetPayload> {
    let reveal_js = build_reveal_target_js(selector);
    context.record_browser_evaluation();
    let evaluation = match tab_id {
        Some(tab_id) => context.session.evaluate_on_tab(tab_id, &reveal_js, false),
        None => context.session.evaluate(&reveal_js, false),
    }
    .map_err(screenshot_evaluation_error)?;
    decode_reveal_payload(evaluation.value)
}

fn build_reveal_target_js(selector: &str) -> String {
    let selector_json = serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        r#"(() => {{
            const selector = {selector_json};
            let element = null;
            try {{
                element = document.querySelector(selector);
            }} catch (_error) {{
                if (selector.startsWith('#')) {{
                    element = document.getElementById(selector.slice(1));
                }}
            }}

            if (!element) {{
                return JSON.stringify({{
                    success: false,
                    code: 'target_not_found',
                    error: 'Element not found for screenshot reveal'
                }});
            }}

            const scrollYBefore = window.scrollY || 0;
            if (typeof element.scrollIntoView === 'function') {{
                element.scrollIntoView({{
                    block: 'center',
                    inline: 'center',
                    behavior: 'auto'
                }});
            }}

            const rect = element.getBoundingClientRect();
            return JSON.stringify({{
                success: true,
                scroll_y_before: scrollYBefore,
                scroll_y_after: window.scrollY || 0,
                visible_in_viewport:
                    rect.bottom > 0 &&
                    rect.right > 0 &&
                    rect.top < window.innerHeight &&
                    rect.left < window.innerWidth
            }});
        }})()"#
    )
}

fn decode_reveal_payload(value: Option<serde_json::Value>) -> Result<RevealTargetPayload> {
    let value = value.ok_or_else(|| BrowserError::ToolExecutionFailed {
        tool: "screenshot".to_string(),
        reason: "screenshot reveal did not return a payload".to_string(),
    })?;

    match value {
        serde_json::Value::String(json) => {
            serde_json::from_str(&json).map_err(|e| BrowserError::ToolExecutionFailed {
                tool: "screenshot".to_string(),
                reason: format!("screenshot reveal returned invalid JSON: {}", e),
            })
        }
        other => serde_json::from_value(other).map_err(|e| BrowserError::ToolExecutionFailed {
            tool: "screenshot".to_string(),
            reason: format!("screenshot reveal returned an invalid payload: {}", e),
        }),
    }
}

fn screenshot_evaluation_error(error: BrowserError) -> BrowserError {
    match error {
        BrowserError::EvaluationFailed(reason) => BrowserError::ToolExecutionFailed {
            tool: "screenshot".to_string(),
            reason,
        },
        other => other,
    }
}

fn screenshot_viewport_failure(
    code: &str,
    error: impl Into<String>,
    target: &crate::tools::ResolvedTarget,
    layout: &InspectLayout,
    reveal_attempted: bool,
    reveal: Option<&RevealTargetPayload>,
) -> ToolResult {
    structured_tool_failure(
        code,
        error.into(),
        None,
        Some(target.to_target_envelope()),
        Some(serde_json::json!({
            "suggested_tool": "inspect_node",
            "hint": "Scroll or expand the target into view before capturing an element screenshot."
        })),
        Some(serde_json::json!({
            "viewport_state": {
                "visible": layout.visible,
                "visible_in_viewport": layout.visible_in_viewport,
                "bounding_box": layout.bounding_box,
                "reveal_attempted": reveal_attempted,
                "scroll_y_before": reveal.and_then(|payload| payload.scroll_y_before),
                "scroll_y_after": reveal.and_then(|payload| payload.scroll_y_after),
                "visible_in_viewport_after_reveal": reveal.and_then(|payload| payload.visible_in_viewport),
            }
        })),
    )
}

fn region_to_clip(region: &ScreenshotRegion) -> Result<BrowserScreenshotClip> {
    clip_from_values(region.x, region.y, region.width, region.height, "region")
}

fn bounding_box_to_clip(bounding_box: &InspectBoundingBox) -> Result<BrowserScreenshotClip> {
    clip_from_values(
        bounding_box.x,
        bounding_box.y,
        bounding_box.width,
        bounding_box.height,
        "element bounding box",
    )
}

fn clip_from_values(
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    source: &str,
) -> Result<BrowserScreenshotClip> {
    for (label, value) in [("x", x), ("y", y), ("width", width), ("height", height)] {
        if !value.is_finite() {
            return Err(BrowserError::InvalidArgument(format!(
                "screenshot {} has a non-finite {} value",
                source, label
            )));
        }
    }

    if x < 0.0 || y < 0.0 {
        return Err(BrowserError::InvalidArgument(format!(
            "screenshot {} must start within viewport CSS pixels",
            source
        )));
    }

    if width <= 0.0 || height <= 0.0 {
        return Err(BrowserError::InvalidArgument(format!(
            "screenshot {} must have positive width and height",
            source
        )));
    }

    Ok(BrowserScreenshotClip {
        x,
        y,
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::{ScreenshotMode, ScreenshotParams, ScreenshotScale, ScreenshotTool};
    use crate::browser::BrowserSession;
    use crate::browser::backend::FakeSessionBackend;
    use crate::error::{BrowserError, Result};
    use crate::tools::core::PublicTarget;
    use crate::tools::{Tool, ToolContext};
    use serde_json::json;
    use std::path::PathBuf;

    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

    fn png_dimensions(bytes: &[u8]) -> Result<(u32, u32)> {
        if bytes.len() < 24 || &bytes[..PNG_SIGNATURE.len()] != PNG_SIGNATURE {
            return Err(BrowserError::ScreenshotFailed(
                "Browser returned a non-PNG screenshot payload".to_string(),
            ));
        }

        let width = u32::from_be_bytes(bytes[16..20].try_into().expect("width bytes should exist"));
        let height =
            u32::from_be_bytes(bytes[20..24].try_into().expect("height bytes should exist"));
        Ok((width, height))
    }

    #[test]
    fn test_screenshot_params_default_to_viewport_mode() {
        let params: ScreenshotParams =
            serde_json::from_value(json!({})).expect("params should deserialize");
        assert_eq!(params.mode, ScreenshotMode::Viewport);
        assert_eq!(params.scale, ScreenshotScale::Device);
        assert!(params.tab_id.is_none());
        assert!(params.target.is_none());
        assert!(params.region.is_none());
    }

    #[test]
    fn test_png_dimensions_read_fake_backend_png_header() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let bytes = session
            .capture_screenshot(false)
            .expect("fake backend screenshot should succeed");

        assert_eq!(
            png_dimensions(&bytes).expect("png dimensions should parse"),
            (1600, 1200)
        );
    }

    #[test]
    fn test_screenshot_tool_uses_managed_artifact_on_fake_backend() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let tool = ScreenshotTool;
        let mut context = ToolContext::new(&session);

        let result = tool
            .execute_typed(
                ScreenshotParams {
                    mode: ScreenshotMode::FullPage,
                    scale: ScreenshotScale::Device,
                    tab_id: None,
                    target: None,
                    region: None,
                },
                &mut context,
            )
            .expect("screenshot should succeed on fake backend");

        assert!(result.success);
        let data = result.data.expect("screenshot should include data");
        assert_eq!(data["mode"].as_str(), Some("full_page"));
        assert_eq!(data["scale"].as_str(), Some("device"));
        assert_eq!(data["format"].as_str(), Some("png"));
        assert_eq!(data["mime_type"].as_str(), Some("image/png"));
        assert_eq!(data["byte_count"].as_u64(), Some(70));
        assert_eq!(data["width"].as_u64(), Some(1600));
        assert_eq!(data["height"].as_u64(), Some(3600));
        assert_eq!(data["css_width"].as_f64(), Some(800.0));
        assert_eq!(data["css_height"].as_f64(), Some(1800.0));
        assert_eq!(data["device_pixel_ratio"].as_f64(), Some(2.0));
        assert_eq!(data["pixel_scale"].as_f64(), Some(2.0));
        assert_eq!(data["revealed_from_offscreen"].as_bool(), Some(false));
        assert_eq!(data["tab_id"].as_str(), Some("tab-1"));
        assert!(
            data["artifact_uri"]
                .as_str()
                .unwrap_or_default()
                .starts_with("file://")
        );

        let artifact_path = PathBuf::from(
            data["artifact_path"]
                .as_str()
                .expect("artifact path should be returned"),
        );
        let bytes = std::fs::read(&artifact_path).expect("screenshot file should exist");
        assert!(
            bytes.starts_with(&[137, 80, 78, 71]),
            "screenshot should be a PNG"
        );
        std::fs::remove_file(&artifact_path).expect("test screenshot should be removable");
    }

    #[test]
    fn test_screenshot_tool_rejects_element_mode_without_target() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let tool = ScreenshotTool;
        let mut context = ToolContext::new(&session);

        let error = tool
            .execute_typed(
                ScreenshotParams {
                    mode: ScreenshotMode::Element,
                    scale: ScreenshotScale::Device,
                    tab_id: None,
                    target: None,
                    region: None,
                },
                &mut context,
            )
            .expect_err("element mode should require target");

        assert!(matches!(error, BrowserError::InvalidArgument(_)));
        assert!(error.to_string().contains("requires target"));
    }

    #[test]
    fn test_screenshot_tool_rejects_region_mode_without_region() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let tool = ScreenshotTool;
        let mut context = ToolContext::new(&session);

        let error = tool
            .execute_typed(
                ScreenshotParams {
                    mode: ScreenshotMode::Region,
                    scale: ScreenshotScale::Device,
                    tab_id: None,
                    target: None,
                    region: None,
                },
                &mut context,
            )
            .expect_err("region mode should require region");

        assert!(matches!(error, BrowserError::InvalidArgument(_)));
        assert!(error.to_string().contains("requires region"));
    }

    #[test]
    fn test_screenshot_tool_element_mode_uses_inspection_probe_bounding_box() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let tool = ScreenshotTool;
        let mut context = ToolContext::new(&session);

        let result = tool
            .execute_typed(
                ScreenshotParams {
                    mode: ScreenshotMode::Element,
                    scale: ScreenshotScale::Device,
                    tab_id: None,
                    target: Some(PublicTarget::Selector {
                        selector: "#fake-target".to_string(),
                    }),
                    region: None,
                },
                &mut context,
            )
            .expect("element mode should resolve against the fake inspection probe");

        assert!(result.success);
        let data = result.data.expect("element screenshot should include data");
        assert_eq!(data["mode"].as_str(), Some("element"));
        assert_eq!(data["scale"].as_str(), Some("device"));
        assert_eq!(
            data["clip"]["coordinate_space"].as_str(),
            Some("viewport_css_pixels")
        );
        assert_eq!(data["clip"]["width"].as_f64(), Some(100.0));
        assert_eq!(data["clip"]["height"].as_f64(), Some(32.0));
        assert_eq!(data["width"].as_u64(), Some(200));
        assert_eq!(data["height"].as_u64(), Some(64));
        assert_eq!(data["css_width"].as_f64(), Some(100.0));
        assert_eq!(data["css_height"].as_f64(), Some(32.0));
        assert_eq!(data["device_pixel_ratio"].as_f64(), Some(2.0));
        assert_eq!(data["pixel_scale"].as_f64(), Some(2.0));
        assert_eq!(data["revealed_from_offscreen"].as_bool(), Some(false));
    }

    #[test]
    fn test_screenshot_tool_element_mode_supports_tab_id_without_activation() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let captured_tab = session
            .open_tab("https://example.com/captured")
            .expect("captured tab should open");
        let active_tab = session
            .open_tab("https://example.com/active")
            .expect("active tab should open");
        let tool = ScreenshotTool;
        let mut context = ToolContext::new(&session);

        let result = tool
            .execute_typed(
                ScreenshotParams {
                    mode: ScreenshotMode::Element,
                    scale: ScreenshotScale::Device,
                    tab_id: Some(captured_tab.id.clone()),
                    target: Some(PublicTarget::Selector {
                        selector: "#fake-target".to_string(),
                    }),
                    region: None,
                },
                &mut context,
            )
            .expect("element mode should capture a non-active tab by id");

        assert!(result.success);
        let data = result.data.expect("element screenshot should include data");
        assert_eq!(data["tab_id"].as_str(), Some(captured_tab.id.as_str()));
        assert_eq!(
            session.list_tabs().expect("tabs should remain available")[2].id,
            active_tab.id
        );
        assert!(
            session
                .list_tabs()
                .expect("tabs should remain available")
                .into_iter()
                .find(|tab| tab.id == active_tab.id)
                .is_some_and(|tab| tab.active),
            "capturing another tab should not activate it"
        );
    }

    #[test]
    fn test_screenshot_tool_rejects_negative_region_origin() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let tool = ScreenshotTool;
        let mut context = ToolContext::new(&session);

        let error = tool
            .execute_typed(
                ScreenshotParams {
                    mode: ScreenshotMode::Region,
                    scale: ScreenshotScale::Device,
                    tab_id: None,
                    target: None,
                    region: Some(super::ScreenshotRegion {
                        x: -1.0,
                        y: 8.0,
                        width: 16.0,
                        height: 12.0,
                    }),
                },
                &mut context,
            )
            .expect_err("negative region origin should be rejected");

        assert!(matches!(error, BrowserError::InvalidArgument(_)));
        assert!(
            error
                .to_string()
                .contains("must start within viewport CSS pixels")
        );
    }

    #[test]
    fn test_screenshot_tool_css_scale_normalizes_fake_backend_output() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let tool = ScreenshotTool;
        let mut context = ToolContext::new(&session);

        let result = tool
            .execute_typed(
                ScreenshotParams {
                    mode: ScreenshotMode::Region,
                    scale: ScreenshotScale::Css,
                    tab_id: None,
                    target: None,
                    region: Some(super::ScreenshotRegion {
                        x: 4.0,
                        y: 6.0,
                        width: 120.0,
                        height: 90.0,
                    }),
                },
                &mut context,
            )
            .expect("css-normalized region capture should succeed");

        assert!(result.success);
        let data = result
            .data
            .expect("css-normalized screenshot should include data");
        assert_eq!(data["scale"].as_str(), Some("css"));
        assert_eq!(data["width"].as_u64(), Some(120));
        assert_eq!(data["height"].as_u64(), Some(90));
        assert_eq!(data["css_width"].as_f64(), Some(120.0));
        assert_eq!(data["css_height"].as_f64(), Some(90.0));
        assert_eq!(data["device_pixel_ratio"].as_f64(), Some(2.0));
        assert_eq!(data["pixel_scale"].as_f64(), Some(1.0));
    }
}

use crate::browser::commands::{
    BrowserCommand, BrowserCommandResult, InteractionCommand, InteractionCommandResult,
};
use crate::browser::config::CHROME_BROWSER_IDLE_TIMEOUT;
use crate::browser::{ConnectionOptions, LaunchOptions};
use crate::contract::{
    ViewportEmulation, ViewportEmulationRequest, ViewportMetrics, ViewportOperationResult,
    ViewportOrientation, ViewportResetRequest,
};
use crate::dom::{DocumentMetadata, DomTree};
use crate::error::{BackendUnsupportedDetails, BrowserError, PageTargetLostDetails, Result};
use headless_chrome::protocol::cdp::{Emulation, Page};
use headless_chrome::{Browser, Tab};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::ffi::OsStr;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

pub(crate) const DEBUG_PORT_START: u16 = 40_000;
pub(crate) const DEBUG_PORT_END: u16 = 59_999;
pub(crate) const ATTACH_PAGE_TARGET_LOST_CODE: &str = "attach_page_target_lost";
pub(crate) const ATTACH_SESSION_PAGE_TARGET_LOSS_KIND: &str = "page_target_lost";
const ATTACH_SESSION_RECOVERY_HINT: &str = "Run tab_list, then switch_tab to reacquire an active page target. If page actions still fail, reconnect the attach session and rerun snapshot.";
pub(crate) const VIEWPORT_DIMENSION_MAX: u32 = 10_000_000;
static DEBUG_PORT_COUNTER: AtomicU16 = AtomicU16::new(DEBUG_PORT_START);

fn session_close_result(total_tabs: usize, failures: Vec<String>) -> Result<()> {
    if failures.is_empty() {
        return Ok(());
    }

    Err(BrowserError::TabOperationFailed(format!(
        "Session close encountered {} error(s) after attempting {} tab(s): {}",
        failures.len(),
        total_tabs,
        failures.join("; ")
    )))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TabDescriptor {
    pub id: String,
    pub title: String,
    pub url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ScreenshotMode {
    #[default]
    Viewport,
    FullPage,
}

impl ScreenshotMode {
    pub(crate) fn from_legacy_full_page(full_page: bool) -> Self {
        if full_page {
            Self::FullPage
        } else {
            Self::Viewport
        }
    }

    fn capture_beyond_viewport(self) -> bool {
        matches!(self, Self::FullPage)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ScreenshotScale {
    #[default]
    Device,
    Css,
}

impl ScreenshotScale {
    fn viewport_scale(self, device_pixel_ratio: f64) -> f64 {
        match self {
            Self::Device => 1.0,
            Self::Css => 1.0 / sanitize_device_pixel_ratio(device_pixel_ratio),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenshotFormat {
    Png,
}

impl ScreenshotFormat {
    pub(crate) fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
        }
    }

    pub(crate) fn mime_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ScreenshotClip {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl ScreenshotClip {
    fn validate(&self) -> Result<()> {
        for (label, value) in [
            ("x", self.x),
            ("y", self.y),
            ("width", self.width),
            ("height", self.height),
        ] {
            if !value.is_finite() {
                return Err(BrowserError::InvalidArgument(format!(
                    "screenshot clip {label} must be finite"
                )));
            }
        }

        if self.x < 0.0 || self.y < 0.0 {
            return Err(BrowserError::InvalidArgument(
                "screenshot clip origin must be non-negative".to_string(),
            ));
        }

        if self.width <= 0.0 || self.height <= 0.0 {
            return Err(BrowserError::InvalidArgument(
                "screenshot clip width and height must be greater than zero".to_string(),
            ));
        }

        Ok(())
    }

    fn to_viewport(&self, scale: f64) -> Page::Viewport {
        Page::Viewport {
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
            scale,
        }
    }
}

impl ViewportOrientation {
    fn to_screen_orientation(self) -> Emulation::ScreenOrientation {
        let (orientation, angle) = match self {
            Self::PortraitPrimary => (Emulation::ScreenOrientationType::PortraitPrimary, 0),
            Self::PortraitSecondary => (Emulation::ScreenOrientationType::PortraitSecondary, 180),
            Self::LandscapePrimary => (Emulation::ScreenOrientationType::LandscapePrimary, 90),
            Self::LandscapeSecondary => (Emulation::ScreenOrientationType::LandscapeSecondary, 270),
        };

        Emulation::ScreenOrientation {
            Type: orientation,
            angle,
        }
    }
}

impl ViewportEmulationRequest {
    pub(crate) fn validate(&self) -> Result<()> {
        validate_optional_tab_id(self.tab_id.as_deref(), "viewport tab_id")?;

        if self.width == 0 {
            return Err(BrowserError::InvalidArgument(
                "viewport width must be greater than zero".to_string(),
            ));
        }
        if self.width > VIEWPORT_DIMENSION_MAX {
            return Err(BrowserError::InvalidArgument(format!(
                "viewport width must be less than or equal to {VIEWPORT_DIMENSION_MAX}"
            )));
        }

        if self.height == 0 {
            return Err(BrowserError::InvalidArgument(
                "viewport height must be greater than zero".to_string(),
            ));
        }
        if self.height > VIEWPORT_DIMENSION_MAX {
            return Err(BrowserError::InvalidArgument(format!(
                "viewport height must be less than or equal to {VIEWPORT_DIMENSION_MAX}"
            )));
        }

        if !self.device_scale_factor.is_finite() || self.device_scale_factor <= 0.0 {
            return Err(BrowserError::InvalidArgument(
                "viewport device_scale_factor must be a finite number greater than zero"
                    .to_string(),
            ));
        }

        Ok(())
    }

    pub(crate) fn normalized_emulation(&self) -> ViewportEmulation {
        ViewportEmulation {
            width: self.width,
            height: self.height,
            device_scale_factor: self.device_scale_factor,
            mobile: self.mobile,
            touch: self.touch,
            orientation: self.orientation,
        }
    }
}

impl ViewportResetRequest {
    pub(crate) fn validate(&self) -> Result<()> {
        validate_optional_tab_id(self.tab_id.as_deref(), "viewport tab_id")
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, Default)]
pub struct ScreenshotRequest {
    #[serde(default)]
    pub mode: ScreenshotMode,
    #[serde(default)]
    pub scale: ScreenshotScale,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clip: Option<ScreenshotClip>,
}

impl ScreenshotRequest {
    pub(crate) fn from_legacy_full_page(full_page: bool) -> Self {
        Self {
            mode: ScreenshotMode::from_legacy_full_page(full_page),
            scale: ScreenshotScale::Device,
            tab_id: None,
            clip: None,
        }
    }

    pub(crate) fn validate(&self) -> Result<()> {
        validate_optional_tab_id(self.tab_id.as_deref(), "screenshot tab_id")?;

        if let Some(clip) = self.clip.as_ref() {
            clip.validate()?;
        }

        if self.clip.is_some() && self.mode.capture_beyond_viewport() {
            return Err(BrowserError::InvalidArgument(
                "full-page screenshots cannot be combined with a clip region".to_string(),
            ));
        }

        Ok(())
    }
}

fn capture_screenshot_method(
    request: &ScreenshotRequest,
    clip: Option<Page::Viewport>,
) -> Page::CaptureScreenshot {
    Page::CaptureScreenshot {
        format: Some(Page::CaptureScreenshotFormatOption::Png),
        quality: None,
        clip,
        from_surface: Some(true),
        capture_beyond_viewport: Some(request.mode.capture_beyond_viewport()),
        optimize_for_speed: None,
    }
}

fn set_device_metrics_override(
    emulation: &ViewportEmulation,
) -> Emulation::SetDeviceMetricsOverride {
    Emulation::SetDeviceMetricsOverride {
        width: emulation.width,
        height: emulation.height,
        device_scale_factor: emulation.device_scale_factor,
        mobile: emulation.mobile,
        scale: None,
        screen_width: None,
        screen_height: None,
        position_x: None,
        position_y: None,
        dont_set_visible_size: None,
        screen_orientation: emulation
            .orientation
            .map(ViewportOrientation::to_screen_orientation),
        viewport: None,
        display_feature: None,
        device_posture: None,
    }
}

fn clear_device_metrics_override() -> Emulation::ClearDeviceMetricsOverride {
    Emulation::ClearDeviceMetricsOverride(None)
}

fn set_touch_emulation(
    enabled: bool,
    max_touch_points: Option<u32>,
) -> Emulation::SetTouchEmulationEnabled {
    Emulation::SetTouchEmulationEnabled {
        enabled,
        max_touch_points,
    }
}

fn validate_optional_tab_id(tab_id: Option<&str>, label: &str) -> Result<()> {
    if let Some(tab_id) = tab_id
        && tab_id.trim().is_empty()
    {
        return Err(BrowserError::InvalidArgument(format!(
            "{label} cannot be empty"
        )));
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ScreenshotCapture {
    pub mode: ScreenshotMode,
    pub scale: ScreenshotScale,
    pub tab: TabDescriptor,
    pub format: ScreenshotFormat,
    pub mime_type: &'static str,
    pub byte_count: usize,
    pub width: u32,
    pub height: u32,
    pub css_width: f64,
    pub css_height: f64,
    pub device_pixel_ratio: f64,
    pub pixel_scale: f64,
    pub clip: Option<ScreenshotClip>,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ScreenshotImageMetrics {
    css_width: f64,
    css_height: f64,
    device_pixel_ratio: f64,
}

impl ScreenshotCapture {
    fn from_png_bytes(
        mode: ScreenshotMode,
        scale: ScreenshotScale,
        tab: TabDescriptor,
        clip: Option<ScreenshotClip>,
        metrics: ScreenshotImageMetrics,
        bytes: Vec<u8>,
    ) -> Result<Self> {
        let (width, height) = png_dimensions(&bytes)?;
        let css_width = sanitize_css_dimension(metrics.css_width, width);
        let css_height = sanitize_css_dimension(metrics.css_height, height);
        let pixel_scale = infer_pixel_scale(width, height, css_width, css_height);
        Ok(Self {
            mode,
            scale,
            tab,
            format: ScreenshotFormat::Png,
            mime_type: ScreenshotFormat::Png.mime_type(),
            byte_count: bytes.len(),
            width,
            height,
            css_width,
            css_height,
            device_pixel_ratio: sanitize_device_pixel_ratio(metrics.device_pixel_ratio),
            pixel_scale,
            clip,
            bytes,
        })
    }
}

#[derive(Debug, Clone, Copy, serde::Deserialize)]
struct ScreenshotPageMetrics {
    inner_width: f64,
    inner_height: f64,
    scroll_width: f64,
    scroll_height: f64,
    device_pixel_ratio: f64,
}

fn screenshot_page_metrics_script() -> &'static str {
    r#"JSON.stringify((() => {
        const root = document.documentElement;
        const body = document.body;
        return {
            inner_width: Number(window.innerWidth || (root ? root.clientWidth : 0) || 0),
            inner_height: Number(window.innerHeight || (root ? root.clientHeight : 0) || 0),
            scroll_width: Number(Math.max(
                window.innerWidth || 0,
                root ? root.scrollWidth : 0,
                body ? body.scrollWidth : 0
            )),
            scroll_height: Number(Math.max(
                window.innerHeight || 0,
                root ? root.scrollHeight : 0,
                body ? body.scrollHeight : 0
            )),
            device_pixel_ratio: Number(window.devicePixelRatio || 1)
        };
    })())"#
}

fn decode_browser_json_value<T>(value: Value, context: &str) -> Result<T>
where
    T: DeserializeOwned,
{
    match value {
        Value::String(json) => serde_json::from_str(&json)
            .map_err(|e| BrowserError::ScreenshotFailed(format!("{context}: {e}"))),
        other => serde_json::from_value(other)
            .map_err(|e| BrowserError::ScreenshotFailed(format!("{context}: {e}"))),
    }
}

fn decode_browser_command_value<T>(value: Option<Value>, context: &str) -> Result<T>
where
    T: DeserializeOwned,
{
    match value.unwrap_or(Value::Null) {
        Value::String(json) => serde_json::from_str(&json)
            .map_err(|e| BrowserError::EvaluationFailed(format!("{context}: {e}"))),
        other => serde_json::from_value(other)
            .map_err(|e| BrowserError::EvaluationFailed(format!("{context}: {e}"))),
    }
}

impl ScreenshotPageMetrics {
    fn evaluate(tab: &Arc<Tab>) -> Result<Self> {
        let evaluation = tab
            .evaluate(screenshot_page_metrics_script(), false)
            .map_err(|e| BrowserError::ScreenshotFailed(e.to_string()))?;
        let value = evaluation.value.ok_or_else(|| {
            BrowserError::ScreenshotFailed(
                "Screenshot capture could not read page metrics".to_string(),
            )
        })?;
        let metrics: Self =
            decode_browser_json_value(value, "Screenshot capture could not decode page metrics")?;

        Ok(Self {
            inner_width: sanitize_length(metrics.inner_width, 1.0),
            inner_height: sanitize_length(metrics.inner_height, 1.0),
            scroll_width: sanitize_length(metrics.scroll_width, metrics.inner_width),
            scroll_height: sanitize_length(metrics.scroll_height, metrics.inner_height),
            device_pixel_ratio: sanitize_device_pixel_ratio(metrics.device_pixel_ratio),
        })
    }

    fn css_size_for(&self, request: &ScreenshotRequest) -> (f64, f64) {
        if let Some(clip) = request.clip.as_ref() {
            return (clip.width, clip.height);
        }

        match request.mode {
            ScreenshotMode::Viewport => (self.inner_width, self.inner_height),
            ScreenshotMode::FullPage => (self.scroll_width, self.scroll_height),
        }
    }

    fn capture_clip_for(&self, request: &ScreenshotRequest) -> Option<ScreenshotClip> {
        if let Some(clip) = request.clip.as_ref() {
            return Some(clip.clone());
        }

        match (request.mode, request.scale) {
            (ScreenshotMode::Viewport, ScreenshotScale::Css) => Some(ScreenshotClip {
                x: 0.0,
                y: 0.0,
                width: self.inner_width,
                height: self.inner_height,
            }),
            (ScreenshotMode::FullPage, ScreenshotScale::Css) => Some(ScreenshotClip {
                x: 0.0,
                y: 0.0,
                width: self.scroll_width,
                height: self.scroll_height,
            }),
            _ => None,
        }
    }

    fn viewport_metrics(&self) -> ViewportMetrics {
        ViewportMetrics {
            width: self.inner_width,
            height: self.inner_height,
            device_pixel_ratio: self.device_pixel_ratio,
        }
    }
}

fn sanitize_device_pixel_ratio(device_pixel_ratio: f64) -> f64 {
    if device_pixel_ratio.is_finite() && device_pixel_ratio > 0.0 {
        device_pixel_ratio
    } else {
        1.0
    }
}

fn sanitize_length(value: f64, fallback: f64) -> f64 {
    if value.is_finite() && value > 0.0 {
        value
    } else if fallback.is_finite() && fallback > 0.0 {
        fallback
    } else {
        1.0
    }
}

fn sanitize_css_dimension(value: f64, image_dimension: u32) -> f64 {
    sanitize_length(value, image_dimension as f64)
}

fn infer_pixel_scale(width: u32, height: u32, css_width: f64, css_height: f64) -> f64 {
    let width_scale = if css_width > 0.0 {
        width as f64 / css_width
    } else {
        0.0
    };
    let height_scale = if css_height > 0.0 {
        height as f64 / css_height
    } else {
        0.0
    };

    if width_scale.is_finite() && width_scale > 0.0 {
        width_scale
    } else if height_scale.is_finite() && height_scale > 0.0 {
        height_scale
    } else {
        1.0
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ScriptEvaluation {
    pub value: Option<Value>,
    pub description: Option<String>,
    pub type_name: Option<String>,
}

pub(crate) trait SessionBackend: Send + Sync {
    fn navigate(&self, url: &str) -> Result<()>;
    fn wait_for_navigation(&self) -> Result<()>;
    fn wait_for_document_ready_with_timeout(&self, timeout: Duration) -> Result<()>;
    fn document_metadata(&self) -> Result<DocumentMetadata>;
    fn extract_dom(&self) -> Result<DomTree>;
    fn extract_dom_for_tab(&self, tab_id: &str) -> Result<DomTree> {
        if self.active_tab()?.id == tab_id {
            return self.extract_dom();
        }

        Err(BrowserError::BackendUnsupported(
            BackendUnsupportedDetails::new("extract_dom_for_tab", "extract_dom_for_tab"),
        ))
    }
    fn extract_dom_with_prefix(&self, prefix: &str) -> Result<DomTree>;
    fn evaluate(&self, script: &str, await_promise: bool) -> Result<ScriptEvaluation>;
    fn evaluate_on_tab(
        &self,
        tab_id: &str,
        script: &str,
        await_promise: bool,
    ) -> Result<ScriptEvaluation> {
        if self.active_tab()?.id == tab_id {
            return self.evaluate(script, await_promise);
        }

        Err(BrowserError::BackendUnsupported(
            BackendUnsupportedDetails::new("evaluate_on_tab", "evaluate_on_tab"),
        ))
    }
    fn execute_command(&self, command: BrowserCommand) -> Result<BrowserCommandResult> {
        Err(BrowserError::BackendUnsupported(
            BackendUnsupportedDetails::new(command.capability(), command.operation()),
        ))
    }
    fn capture_screenshot(&self, full_page: bool) -> Result<Vec<u8>>;
    fn capture_screenshot_with_request(
        &self,
        request: &ScreenshotRequest,
    ) -> Result<ScreenshotCapture> {
        request.validate()?;
        if request.tab_id.is_some() {
            return Err(BrowserError::BackendUnsupported(
                BackendUnsupportedDetails::new("screenshot_tab_targeting", "capture_screenshot"),
            ));
        }
        if request.clip.is_some() {
            return Err(BrowserError::BackendUnsupported(
                BackendUnsupportedDetails::new("screenshot_clip", "capture_screenshot"),
            ));
        }

        let bytes = self.capture_screenshot(request.mode.capture_beyond_viewport())?;
        let tab = self.active_tab()?;
        ScreenshotCapture::from_png_bytes(
            request.mode,
            request.scale,
            tab,
            None,
            ScreenshotImageMetrics {
                css_width: 1.0,
                css_height: 1.0,
                device_pixel_ratio: 1.0,
            },
            bytes,
        )
    }
    fn viewport_metrics(&self, _tab_id: Option<&str>) -> Result<ViewportMetrics> {
        Err(BrowserError::BackendUnsupported(
            BackendUnsupportedDetails::new("viewport_metrics", "viewport_metrics"),
        ))
    }
    fn apply_viewport_emulation(
        &self,
        _request: &ViewportEmulationRequest,
    ) -> Result<ViewportOperationResult> {
        Err(BrowserError::BackendUnsupported(
            BackendUnsupportedDetails::new("viewport_emulation", "apply_viewport_emulation"),
        ))
    }
    fn reset_viewport_emulation(
        &self,
        _request: &ViewportResetRequest,
    ) -> Result<ViewportOperationResult> {
        Err(BrowserError::BackendUnsupported(
            BackendUnsupportedDetails::new("viewport_emulation", "reset_viewport_emulation"),
        ))
    }
    fn press_key(&self, key: &str) -> Result<()>;
    fn list_tabs(&self) -> Result<Vec<TabDescriptor>>;
    fn active_tab(&self) -> Result<TabDescriptor>;
    fn open_tab(&self, url: &str) -> Result<TabDescriptor>;
    fn activate_tab(&self, tab_id: &str) -> Result<()>;
    fn close_tab(&self, tab_id: &str, with_unload: bool) -> Result<()>;
    fn close(&self) -> Result<()>;
}

pub(crate) fn choose_debug_port() -> u16 {
    let span = DEBUG_PORT_END - DEBUG_PORT_START + 1;
    let offset = DEBUG_PORT_COUNTER.fetch_add(1, Ordering::Relaxed) % span;
    DEBUG_PORT_START + offset
}

pub(crate) fn build_launch_options(
    options: LaunchOptions,
) -> headless_chrome::LaunchOptions<'static> {
    let mut launch_opts = headless_chrome::LaunchOptions::default();

    launch_opts
        .ignore_default_args
        .push(OsStr::new("--enable-automation"));
    launch_opts
        .args
        .push(OsStr::new("--disable-blink-features=AutomationControlled"));

    launch_opts.idle_browser_timeout = CHROME_BROWSER_IDLE_TIMEOUT;
    launch_opts.headless = options.headless;
    launch_opts.window_size = Some((options.window_width, options.window_height));
    launch_opts.port = Some(options.debug_port.unwrap_or_else(choose_debug_port));
    launch_opts.sandbox = options.sandbox;

    if let Some(path) = options.chrome_path {
        launch_opts.path = Some(path);
    }

    if let Some(dir) = options.user_data_dir {
        launch_opts.user_data_dir = Some(dir);
    }

    launch_opts
}

pub(crate) struct ChromeSessionBackend {
    browser: Browser,
    attach_mode: bool,
    active_tab_hint: RwLock<Option<String>>,
    page_target_degraded: AtomicBool,
}

impl ChromeSessionBackend {
    pub(crate) fn launch(options: LaunchOptions) -> Result<Self> {
        let launch_opts = build_launch_options(options);
        let browser =
            Browser::new(launch_opts).map_err(|e| BrowserError::LaunchFailed(e.to_string()))?;
        let initial_tab = browser
            .new_tab()
            .map_err(|e| BrowserError::LaunchFailed(format!("Failed to create tab: {}", e)))?;

        Ok(Self {
            browser,
            attach_mode: false,
            active_tab_hint: RwLock::new(Some(tab_id(&initial_tab))),
            page_target_degraded: AtomicBool::new(false),
        })
    }

    pub(crate) fn connect(options: ConnectionOptions) -> Result<Self> {
        let ws_url = options.resolved_ws_url()?;
        let browser = Browser::connect_with_timeout(ws_url, CHROME_BROWSER_IDLE_TIMEOUT)
            .map_err(|e| BrowserError::ConnectionFailed(e.to_string()))?;

        Ok(Self {
            browser,
            attach_mode: true,
            active_tab_hint: RwLock::new(None),
            page_target_degraded: AtomicBool::new(false),
        })
    }

    pub(crate) fn active_tab_handle(&self) -> Result<Arc<Tab>> {
        if let Some(tab) = self.cached_active_tab()? {
            return Ok(tab);
        }

        let tabs = self.tabs()?;
        self.detect_active_tab_from_tabs(&tabs)
    }

    fn detect_active_tab_from_tabs(&self, tabs: &[Arc<Tab>]) -> Result<Arc<Tab>> {
        for tab in tabs {
            let result = tab.evaluate(
                "document.visibilityState === 'visible' && document.hasFocus()",
                false,
            );
            match result {
                Ok(remote_object) => {
                    if remote_object
                        .value
                        .as_ref()
                        .and_then(|value| value.as_bool())
                        .unwrap_or(false)
                    {
                        self.set_active_tab_hint(Some(tab_id(tab)))?;
                        return Ok(tab.clone());
                    }
                }
                Err(e) => {
                    log::debug!("Failed to check tab status: {}", e);
                }
            }
        }

        for tab in tabs {
            let result = tab.evaluate("document.visibilityState === 'visible'", false);
            match result {
                Ok(remote_object) => {
                    if remote_object
                        .value
                        .as_ref()
                        .and_then(|value| value.as_bool())
                        .unwrap_or(false)
                    {
                        self.set_active_tab_hint(Some(tab_id(tab)))?;
                        return Ok(tab.clone());
                    }
                }
                Err(_) => continue,
            }
        }

        Err(BrowserError::TabOperationFailed(
            "No active tab found".to_string(),
        ))
    }

    pub(crate) fn tabs(&self) -> Result<Vec<Arc<Tab>>> {
        let tabs = self
            .browser
            .get_tabs()
            .lock()
            .map_err(|e| BrowserError::TabOperationFailed(format!("Failed to get tabs: {}", e)))?
            .clone();
        Ok(tabs)
    }

    pub(crate) fn activate_real_tab(&self, tab: &Arc<Tab>) -> Result<()> {
        tab.activate().map_err(|e| {
            BrowserError::TabOperationFailed(format!("Failed to activate tab: {}", e))
        })?;
        self.set_active_tab_hint(Some(tab_id(tab)))?;
        self.mark_page_target_healthy();
        Ok(())
    }

    pub(crate) fn open_real_tab(&self, url: &str) -> Result<Arc<Tab>> {
        let tab = self.browser.new_tab().map_err(|e| {
            BrowserError::TabOperationFailed(format!("Failed to create tab: {}", e))
        })?;

        tab.navigate_to(url).map_err(|e| {
            BrowserError::NavigationFailed(format!("Failed to navigate to {}: {}", url, e))
        })?;

        tab.wait_until_navigated().map_err(|e| {
            BrowserError::NavigationFailed(format!("Navigation to {} did not complete: {}", url, e))
        })?;

        self.activate_real_tab(&tab)?;
        Ok(tab)
    }

    fn cached_active_tab(&self) -> Result<Option<Arc<Tab>>> {
        let Some(tab_id_hint) = self.active_tab_hint()? else {
            return Ok(None);
        };

        Ok(self
            .tabs()?
            .into_iter()
            .find(|tab| tab_id(tab) == tab_id_hint))
    }

    fn active_tab_hint(&self) -> Result<Option<String>> {
        Ok(self
            .active_tab_hint
            .read()
            .map_err(|e| {
                BrowserError::TabOperationFailed(format!("Failed to read active tab hint: {}", e))
            })?
            .clone())
    }

    fn set_active_tab_hint(&self, tab_id: Option<String>) -> Result<()> {
        *self.active_tab_hint.write().map_err(|e| {
            BrowserError::TabOperationFailed(format!("Failed to write active tab hint: {}", e))
        })? = tab_id;
        Ok(())
    }

    fn mark_page_target_degraded(&self) {
        self.page_target_degraded.store(true, Ordering::Relaxed);
    }

    fn mark_page_target_healthy(&self) {
        self.page_target_degraded.store(false, Ordering::Relaxed);
    }

    fn page_target_is_degraded(&self) -> bool {
        self.page_target_degraded.load(Ordering::Relaxed)
    }

    fn browser_inventory_available(&self) -> bool {
        self.tabs().map(|tabs| !tabs.is_empty()).unwrap_or(false)
    }

    fn recover_active_tab_handle(&self) -> Result<Arc<Tab>> {
        let previous_hint = self.active_tab_hint()?;
        self.set_active_tab_hint(None)?;

        let tabs = self.tabs()?;
        if tabs.is_empty() {
            return Err(BrowserError::TabOperationFailed(
                "No surviving tabs available for attach-session recovery".to_string(),
            ));
        }

        if let Some(previous_hint) = previous_hint.as_deref()
            && let Some(tab) = tabs.iter().find(|tab| tab_id(tab) == previous_hint)
        {
            self.set_active_tab_hint(Some(previous_hint.to_string()))?;
            return Ok(tab.clone());
        }

        if let Ok(tab) = self.detect_active_tab_from_tabs(&tabs) {
            return Ok(tab);
        }

        if tabs.len() == 1 {
            let tab = tabs[0].clone();
            self.set_active_tab_hint(Some(tab_id(&tab)))?;
            return Ok(tab);
        }

        Err(BrowserError::TabOperationFailed(
            "Unable to reacquire an active page target from surviving tab inventory".to_string(),
        ))
    }

    fn with_active_tab_operation<T, F>(
        &self,
        operation_name: &'static str,
        mut operation: F,
    ) -> Result<T>
    where
        F: FnMut(&Arc<Tab>) -> Result<T>,
    {
        let tab = self.active_tab_handle()?;
        let result = match operation(&tab) {
            Ok(value) => Ok(value),
            Err(error) => {
                if !self.attach_mode
                    || recoverable_page_target_loss_details(operation_name, &error).is_none()
                    || !self.browser_inventory_available()
                {
                    Err(error)
                } else {
                    match self.recover_active_tab_handle() {
                        Ok(recovered_tab) => match operation(&recovered_tab) {
                            Ok(value) => Ok(value),
                            Err(retry_error)
                                if recoverable_page_target_loss_details(
                                    operation_name,
                                    &retry_error,
                                )
                                .is_some() =>
                            {
                                Err(attach_session_page_target_loss(
                                    operation_name,
                                    format!(
                                        "Attached browser session lost its active page target during {operation_name}. One recovery attempt ran, but the page target stayed unavailable: {}",
                                        browser_error_detail(&retry_error)
                                    ),
                                ))
                            }
                            Err(retry_error) => Err(retry_error),
                        },
                        Err(recovery_error) => Err(attach_session_page_target_loss(
                            operation_name,
                            format!(
                                "Attached browser session lost its active page target during {operation_name}. Reacquiring the active page target failed: {}. Original error: {}",
                                browser_error_detail(&recovery_error),
                                browser_error_detail(&error)
                            ),
                        )),
                    }
                }
            }
        };

        match &result {
            Ok(_) => self.mark_page_target_healthy(),
            Err(BrowserError::PageTargetLost(details)) if details.is_attach_session_degraded() => {
                self.mark_page_target_degraded();
            }
            Err(_) => {}
        }

        result
    }

    fn tab_handle_by_id(&self, target_tab_id: &str) -> Result<Arc<Tab>> {
        self.tabs()?
            .into_iter()
            .find(|tab| tab_id(tab) == target_tab_id)
            .ok_or_else(|| {
                BrowserError::TabOperationFailed(format!("No tab found for id {}", target_tab_id))
            })
    }

    fn with_specific_tab_operation<T, F>(
        &self,
        operation_name: &'static str,
        target_tab_id: &str,
        mut operation: F,
    ) -> Result<T>
    where
        F: FnMut(&Arc<Tab>) -> Result<T>,
    {
        let tab = self.tab_handle_by_id(target_tab_id)?;
        match operation(&tab) {
            Ok(value) => Ok(value),
            Err(error)
                if self.attach_mode
                    && recoverable_page_target_loss_details(operation_name, &error).is_some()
                    && self.browser_inventory_available() =>
            {
                let recovered_tab = self.tab_handle_by_id(target_tab_id).map_err(|recovery_error| {
                    attach_session_page_target_loss(
                        operation_name,
                        format!(
                            "Attached browser session lost tab {target_tab_id} during {operation_name}. Reacquiring the same tab target failed: {}. Original error: {}",
                            browser_error_detail(&recovery_error),
                            browser_error_detail(&error)
                        ),
                    )
                })?;

                match operation(&recovered_tab) {
                    Ok(value) => Ok(value),
                    Err(retry_error)
                        if recoverable_page_target_loss_details(operation_name, &retry_error)
                            .is_some() =>
                    {
                        Err(attach_session_page_target_loss(
                            operation_name,
                            format!(
                                "Attached browser session lost tab {target_tab_id} during {operation_name}. One recovery attempt ran, but the tab target stayed unavailable: {}",
                                browser_error_detail(&retry_error)
                            ),
                        ))
                    }
                    Err(retry_error) => Err(retry_error),
                }
            }
            Err(error) => Err(error),
        }
    }

    fn document_ready_state_for_tab(&self, tab: &Arc<Tab>) -> Result<String> {
        let result = tab.evaluate("document.readyState", false).map_err(|e| {
            BrowserError::NavigationFailed(format!("Failed to read readyState: {}", e))
        })?;

        result
            .value
            .and_then(|value| value.as_str().map(str::to_string))
            .ok_or_else(|| {
                BrowserError::NavigationFailed(
                    "Browser did not return a document.readyState value".to_string(),
                )
            })
    }

    fn wait_for_document_ready_with_tab(&self, tab: &Arc<Tab>, timeout: Duration) -> Result<()> {
        let start = Instant::now();
        loop {
            let ready_state = self.document_ready_state_for_tab(tab)?;
            if ready_state == "complete" {
                return Ok(());
            }

            if start.elapsed() >= timeout {
                return Err(BrowserError::Timeout(format!(
                    "Document did not reach readyState=complete within {} ms",
                    timeout.as_millis()
                )));
            }

            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn measure_viewport_metrics(&self, tab: &Arc<Tab>) -> Result<ViewportMetrics> {
        ScreenshotPageMetrics::evaluate(tab)
            .map(|metrics| metrics.viewport_metrics())
            .map_err(|err| {
                BrowserError::EvaluationFailed(format!("Failed to read viewport metrics: {err}"))
            })
    }
}

fn attach_session_page_target_loss(operation_name: &str, detail: String) -> BrowserError {
    BrowserError::PageTargetLost(PageTargetLostDetails::attach_degraded(
        operation_name,
        detail,
        ATTACH_SESSION_RECOVERY_HINT,
    ))
}

fn browser_error_detail(error: &BrowserError) -> String {
    match error {
        BrowserError::LaunchFailed(reason)
        | BrowserError::ConnectionFailed(reason)
        | BrowserError::Timeout(reason)
        | BrowserError::SelectorInvalid(reason)
        | BrowserError::ElementNotFound(reason)
        | BrowserError::DomParseFailed(reason)
        | BrowserError::InvalidArgument(reason)
        | BrowserError::NavigationFailed(reason)
        | BrowserError::EvaluationFailed(reason)
        | BrowserError::ScreenshotFailed(reason)
        | BrowserError::DownloadFailed(reason)
        | BrowserError::TabOperationFailed(reason)
        | BrowserError::ChromeError(reason) => reason.clone(),
        BrowserError::PageTargetLost(details) => details.detail.clone(),
        BrowserError::BackendUnsupported(details) => details.to_string(),
        BrowserError::ToolExecutionFailed { reason, .. } => reason.clone(),
        BrowserError::JsonError(error) => error.to_string(),
        BrowserError::IoError(error) => error.to_string(),
    }
}

fn recoverable_page_target_loss_details(
    operation_name: &str,
    error: &BrowserError,
) -> Option<PageTargetLostDetails> {
    if let BrowserError::PageTargetLost(details) = error {
        return details.recoverable.then(|| details.clone());
    }

    let reason = browser_error_detail(error);
    let normalized = reason.to_ascii_lowercase();

    [
        "underlying connection is closed",
        "connection is closed",
        "connection closed",
        "session closed. most likely the page has been closed",
        "target closed",
    ]
    .iter()
    .any(|fragment| normalized.contains(fragment))
    .then(|| PageTargetLostDetails::recoverable(operation_name, reason))
}

#[cfg(test)]
fn is_recoverable_page_target_loss(error: &BrowserError) -> bool {
    recoverable_page_target_loss_details("unknown", error).is_some()
}

impl SessionBackend for ChromeSessionBackend {
    fn navigate(&self, url: &str) -> Result<()> {
        self.with_active_tab_operation("navigate", |tab| {
            tab.navigate_to(url).map_err(|e| {
                BrowserError::NavigationFailed(format!("Failed to navigate to {}: {}", url, e))
            })?;
            Ok(())
        })
    }

    fn wait_for_navigation(&self) -> Result<()> {
        self.with_active_tab_operation("wait_for_navigation", |tab| {
            tab.wait_until_navigated().map_err(|e| {
                BrowserError::NavigationFailed(format!("Navigation timeout: {}", e))
            })?;
            self.wait_for_document_ready_with_tab(tab, Duration::from_secs(30))
        })
    }

    fn wait_for_document_ready_with_timeout(&self, timeout: Duration) -> Result<()> {
        self.with_active_tab_operation("wait_for_document_ready", |tab| {
            self.wait_for_document_ready_with_tab(tab, timeout)
        })
    }

    fn document_metadata(&self) -> Result<DocumentMetadata> {
        self.with_active_tab_operation("document_metadata", DocumentMetadata::from_tab)
    }

    fn extract_dom(&self) -> Result<DomTree> {
        self.with_active_tab_operation("extract_dom", DomTree::from_tab)
    }

    fn extract_dom_for_tab(&self, tab_id: &str) -> Result<DomTree> {
        self.with_specific_tab_operation("extract_dom", tab_id, DomTree::from_tab)
    }

    fn extract_dom_with_prefix(&self, prefix: &str) -> Result<DomTree> {
        self.with_active_tab_operation("extract_dom", |tab| {
            DomTree::from_tab_with_prefix(tab, prefix)
        })
    }

    fn evaluate(&self, script: &str, await_promise: bool) -> Result<ScriptEvaluation> {
        let result = self.with_active_tab_operation("evaluate", |tab| {
            tab.evaluate(script, await_promise)
                .map_err(|e| BrowserError::EvaluationFailed(e.to_string()))
        })?;

        Ok(ScriptEvaluation {
            value: result.value,
            description: result.description,
            type_name: Some(format!("{:?}", result.Type)),
        })
    }

    fn evaluate_on_tab(
        &self,
        tab_id: &str,
        script: &str,
        await_promise: bool,
    ) -> Result<ScriptEvaluation> {
        let result = self.with_specific_tab_operation("evaluate", tab_id, |tab| {
            tab.evaluate(script, await_promise)
                .map_err(|e| BrowserError::EvaluationFailed(e.to_string()))
        })?;

        Ok(ScriptEvaluation {
            value: result.value,
            description: result.description,
            type_name: Some(format!("{:?}", result.Type)),
        })
    }

    fn execute_command(&self, command: BrowserCommand) -> Result<BrowserCommandResult> {
        let operation = command.operation();
        self.with_active_tab_operation(operation, |tab| {
            let script = command.render_script();
            let result = tab
                .evaluate(&script, false)
                .map_err(|e| BrowserError::EvaluationFailed(e.to_string()))?;

            match &command {
                BrowserCommand::ActionabilityProbe(_) => Ok(
                    BrowserCommandResult::ActionabilityProbe(decode_browser_command_value(
                        result.value,
                        "Failed to decode actionability probe result",
                    )?),
                ),
                BrowserCommand::SelectorIdentityProbe(_) => Ok(
                    BrowserCommandResult::SelectorIdentityProbe(decode_browser_command_value(
                        result.value,
                        "Failed to decode selector identity probe result",
                    )?),
                ),
                BrowserCommand::Interaction(interaction) => {
                    let result = match interaction {
                        InteractionCommand::Click(_) => {
                            InteractionCommandResult::Click(decode_browser_command_value(
                                result.value,
                                "Failed to decode click result",
                            )?)
                        }
                        InteractionCommand::Input(_) => {
                            InteractionCommandResult::Input(decode_browser_command_value(
                                result.value,
                                "Failed to decode input result",
                            )?)
                        }
                        InteractionCommand::Hover(_) => {
                            InteractionCommandResult::Hover(decode_browser_command_value(
                                result.value,
                                "Failed to decode hover result",
                            )?)
                        }
                        InteractionCommand::Select(_) => {
                            InteractionCommandResult::Select(decode_browser_command_value(
                                result.value,
                                "Failed to decode select result",
                            )?)
                        }
                    };
                    Ok(BrowserCommandResult::Interaction(result))
                }
            }
        })
    }

    fn capture_screenshot(&self, full_page: bool) -> Result<Vec<u8>> {
        let request = ScreenshotRequest::from_legacy_full_page(full_page);
        Ok(self.capture_screenshot_with_request(&request)?.bytes)
    }

    fn capture_screenshot_with_request(
        &self,
        request: &ScreenshotRequest,
    ) -> Result<ScreenshotCapture> {
        request.validate()?;

        let capture_from_tab = |tab: &Arc<Tab>| -> Result<ScreenshotCapture> {
            let metrics = ScreenshotPageMetrics::evaluate(tab)?;
            let resolved_clip = metrics.capture_clip_for(request);
            let (css_width, css_height) = metrics.css_size_for(request);
            let clip = resolved_clip.as_ref().map(|clip| {
                clip.to_viewport(request.scale.viewport_scale(metrics.device_pixel_ratio))
            });
            let data = tab
                .call_method(capture_screenshot_method(request, clip))
                .map_err(|e| BrowserError::ScreenshotFailed(e.to_string()))?
                .data;

            let bytes = decode_base64_standard(&data).map_err(BrowserError::ScreenshotFailed)?;
            ScreenshotCapture::from_png_bytes(
                request.mode,
                request.scale,
                descriptor_for_tab(tab),
                resolved_clip,
                ScreenshotImageMetrics {
                    css_width,
                    css_height,
                    device_pixel_ratio: metrics.device_pixel_ratio,
                },
                bytes,
            )
        };

        match request.tab_id.as_deref() {
            Some(tab_id) => {
                self.with_specific_tab_operation("capture_screenshot", tab_id, capture_from_tab)
            }
            None => self.with_active_tab_operation("capture_screenshot", capture_from_tab),
        }
    }

    fn apply_viewport_emulation(
        &self,
        request: &ViewportEmulationRequest,
    ) -> Result<ViewportOperationResult> {
        request.validate()?;
        let emulation = request.normalized_emulation();

        let apply_to_tab = |tab: &Arc<Tab>| -> Result<ViewportOperationResult> {
            tab.call_method(set_device_metrics_override(&emulation))
                .map_err(|e| {
                    BrowserError::TabOperationFailed(format!(
                        "Failed to apply viewport emulation: {e}"
                    ))
                })?;
            tab.call_method(set_touch_emulation(
                emulation.touch,
                emulation.touch.then_some(1),
            ))
            .map_err(|e| {
                BrowserError::TabOperationFailed(format!(
                    "Failed to configure touch emulation: {e}"
                ))
            })?;

            Ok(ViewportOperationResult {
                tab_id: tab_id(tab),
                emulation: Some(emulation.clone()),
                viewport_after: self.measure_viewport_metrics(tab)?,
            })
        };

        match request.tab_id.as_deref() {
            Some(tab_id) => {
                self.with_specific_tab_operation("apply_viewport_emulation", tab_id, apply_to_tab)
            }
            None => self.with_active_tab_operation("apply_viewport_emulation", apply_to_tab),
        }
    }

    fn viewport_metrics(&self, tab_id: Option<&str>) -> Result<ViewportMetrics> {
        match tab_id {
            Some(tab_id) => self.with_specific_tab_operation("viewport_metrics", tab_id, |tab| {
                self.measure_viewport_metrics(tab)
            }),
            None => self.with_active_tab_operation("viewport_metrics", |tab| {
                self.measure_viewport_metrics(tab)
            }),
        }
    }

    fn reset_viewport_emulation(
        &self,
        request: &ViewportResetRequest,
    ) -> Result<ViewportOperationResult> {
        request.validate()?;

        let reset_tab = |tab: &Arc<Tab>| -> Result<ViewportOperationResult> {
            tab.call_method(clear_device_metrics_override())
                .map_err(|e| {
                    BrowserError::TabOperationFailed(format!(
                        "Failed to clear viewport emulation: {e}"
                    ))
                })?;
            tab.call_method(set_touch_emulation(false, None))
                .map_err(|e| {
                    BrowserError::TabOperationFailed(format!(
                        "Failed to disable touch emulation: {e}"
                    ))
                })?;

            Ok(ViewportOperationResult {
                tab_id: tab_id(tab),
                emulation: None,
                viewport_after: self.measure_viewport_metrics(tab)?,
            })
        };

        match request.tab_id.as_deref() {
            Some(tab_id) => {
                self.with_specific_tab_operation("reset_viewport_emulation", tab_id, reset_tab)
            }
            None => self.with_active_tab_operation("reset_viewport_emulation", reset_tab),
        }
    }

    fn press_key(&self, key: &str) -> Result<()> {
        self.with_active_tab_operation("press_key", |tab| {
            tab.press_key(key)
                .map_err(|e| BrowserError::ToolExecutionFailed {
                    tool: "press_key".to_string(),
                    reason: e.to_string(),
                })?;
            Ok(())
        })
    }

    fn list_tabs(&self) -> Result<Vec<TabDescriptor>> {
        Ok(self.tabs()?.iter().map(descriptor_for_tab).collect())
    }

    fn active_tab(&self) -> Result<TabDescriptor> {
        if self.attach_mode && self.page_target_is_degraded() {
            return Err(attach_session_page_target_loss(
                "active_tab",
                "Active tab metadata is available, but DOM-backed page access is degraded until the attached session is recovered".to_string(),
            ));
        }

        Ok(descriptor_for_tab(&self.active_tab_handle()?))
    }

    fn open_tab(&self, url: &str) -> Result<TabDescriptor> {
        Ok(descriptor_for_tab(&self.open_real_tab(url)?))
    }

    fn activate_tab(&self, target_tab_id: &str) -> Result<()> {
        let tab = self
            .tabs()?
            .into_iter()
            .find(|tab| tab_id(tab) == target_tab_id)
            .ok_or_else(|| {
                BrowserError::TabOperationFailed(format!("No tab found for id {}", target_tab_id))
            })?;
        self.activate_real_tab(&tab)
    }

    fn close_tab(&self, target_tab_id: &str, with_unload: bool) -> Result<()> {
        let tab = self
            .tabs()?
            .into_iter()
            .find(|tab| tab_id(tab) == target_tab_id)
            .ok_or_else(|| {
                BrowserError::TabOperationFailed(format!("No tab found for id {}", target_tab_id))
            })?;

        if self.active_tab_hint()?.as_deref() == Some(target_tab_id) {
            self.set_active_tab_hint(None)?;
        }

        tab.close(with_unload)
            .map_err(|e| BrowserError::TabOperationFailed(format!("Failed to close tab: {}", e)))
            .map(|_| ())
    }

    fn close(&self) -> Result<()> {
        let tabs = self.tabs()?;
        let total_tabs = tabs.len();
        let mut failures = Vec::new();

        for tab in tabs {
            let descriptor = descriptor_for_tab(&tab);
            if let Err(err) = tab.close(false) {
                failures.push(format!(
                    "failed to close '{}' ({}) [id={}]: {}",
                    descriptor.title, descriptor.url, descriptor.id, err
                ));
            }
        }

        if let Err(err) = self.set_active_tab_hint(None) {
            failures.push(format!("failed to clear active tab hint: {}", err));
        }
        self.mark_page_target_healthy();

        session_close_result(total_tabs, failures)
    }
}

fn png_dimensions(bytes: &[u8]) -> Result<(u32, u32)> {
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if bytes.len() < 24 || &bytes[..8] != PNG_SIGNATURE {
        return Err(BrowserError::ScreenshotFailed(
            "Browser returned invalid PNG data".to_string(),
        ));
    }

    let width = u32::from_be_bytes(bytes[16..20].try_into().map_err(|_| {
        BrowserError::ScreenshotFailed("PNG width header was truncated".to_string())
    })?);
    let height = u32::from_be_bytes(bytes[20..24].try_into().map_err(|_| {
        BrowserError::ScreenshotFailed("PNG height header was truncated".to_string())
    })?);
    Ok((width, height))
}

fn decode_base64_standard(data: &str) -> std::result::Result<Vec<u8>, String> {
    fn value(byte: u8) -> Option<u8> {
        match byte {
            b'A'..=b'Z' => Some(byte - b'A'),
            b'a'..=b'z' => Some(byte - b'a' + 26),
            b'0'..=b'9' => Some(byte - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }

    let filtered = data
        .bytes()
        .filter(|byte| !matches!(byte, b' ' | b'\n' | b'\r' | b'\t'))
        .collect::<Vec<_>>();

    if filtered.len() % 4 != 0 {
        return Err("Screenshot response contained invalid base64 length".to_string());
    }

    let mut decoded = Vec::with_capacity(filtered.len() / 4 * 3);
    for chunk in filtered.chunks_exact(4) {
        let mut values = [0u8; 4];
        let mut padding = 0usize;
        for (index, byte) in chunk.iter().copied().enumerate() {
            match byte {
                b'=' => {
                    values[index] = 0;
                    padding += 1;
                }
                _ => {
                    values[index] = value(byte).ok_or_else(|| {
                        format!("Screenshot response contained invalid base64 byte 0x{byte:02x}")
                    })?;
                }
            }
        }

        decoded.push((values[0] << 2) | (values[1] >> 4));
        if padding < 2 {
            decoded.push((values[1] << 4) | (values[2] >> 2));
        }
        if padding == 0 {
            decoded.push((values[2] << 6) | values[3]);
        }
    }

    Ok(decoded)
}

fn tab_id(tab: &Arc<Tab>) -> String {
    tab.get_target_id().to_string()
}

fn descriptor_for_tab(tab: &Arc<Tab>) -> TabDescriptor {
    TabDescriptor {
        id: tab_id(tab),
        title: tab.get_title().unwrap_or_default(),
        url: tab.get_url(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn simulate_attach_recovery<T, Op, Recover>(
        attach_mode: bool,
        operation_name: &'static str,
        mut operation: Op,
        mut recover_and_retry: Recover,
        inventory_available: bool,
    ) -> Result<T>
    where
        Op: FnMut() -> Result<T>,
        Recover: FnMut() -> Result<T>,
    {
        match operation() {
            Ok(value) => Ok(value),
            Err(error) => {
                if !attach_mode
                    || recoverable_page_target_loss_details(operation_name, &error).is_none()
                    || !inventory_available
                {
                    return Err(error);
                }

                match recover_and_retry() {
                    Ok(value) => Ok(value),
                    Err(retry_error)
                        if recoverable_page_target_loss_details(operation_name, &retry_error)
                            .is_some() =>
                    {
                        Err(attach_session_page_target_loss(
                            operation_name,
                            format!(
                                "Attached browser session lost its active page target during {operation_name}. One recovery attempt ran, but the page target stayed unavailable: {}",
                                browser_error_detail(&retry_error)
                            ),
                        ))
                    }
                    Err(retry_error) => Err(retry_error),
                }
            }
        }
    }

    fn closed_connection_error() -> BrowserError {
        BrowserError::EvaluationFailed(
            "Unable to make method calls because underlying connection is closed".to_string(),
        )
    }

    #[test]
    fn screenshot_page_metrics_script_returns_a_json_string_expression() {
        let script = screenshot_page_metrics_script().trim_start();
        assert!(script.starts_with("JSON.stringify(("));
        assert!(script.contains("device_pixel_ratio"));
    }

    #[test]
    fn screenshot_page_metrics_decode_accepts_json_string_payloads() {
        let metrics: ScreenshotPageMetrics = decode_browser_json_value(
            serde_json::json!(
                "{\"inner_width\":1016,\"inner_height\":568,\"scroll_width\":1016,\"scroll_height\":11915,\"device_pixel_ratio\":2}"
            ),
            "Screenshot capture could not decode page metrics",
        )
        .expect("json string payload should decode");

        assert_eq!(metrics.inner_width, 1016.0);
        assert_eq!(metrics.inner_height, 568.0);
        assert_eq!(metrics.scroll_width, 1016.0);
        assert_eq!(metrics.scroll_height, 11915.0);
        assert_eq!(metrics.device_pixel_ratio, 2.0);
    }

    #[test]
    fn screenshot_page_metrics_decode_preserves_object_payload_support() {
        let metrics: ScreenshotPageMetrics = decode_browser_json_value(
            serde_json::json!({
                "inner_width": 800,
                "inner_height": 600,
                "scroll_width": 1200,
                "scroll_height": 2400,
                "device_pixel_ratio": 1.5
            }),
            "Screenshot capture could not decode page metrics",
        )
        .expect("object payload should decode");

        assert_eq!(metrics.inner_width, 800.0);
        assert_eq!(metrics.inner_height, 600.0);
        assert_eq!(metrics.scroll_width, 1200.0);
        assert_eq!(metrics.scroll_height, 2400.0);
        assert_eq!(metrics.device_pixel_ratio, 1.5);
    }

    #[test]
    fn disabled_touch_emulation_omits_max_touch_points() {
        let method = set_touch_emulation(false, None);

        assert!(!method.enabled);
        assert_eq!(method.max_touch_points, None);
    }

    #[test]
    fn attach_recovery_retries_once_for_recoverable_page_target_loss() {
        let attempts = Cell::new(0usize);
        let recoveries = Cell::new(0usize);

        let result = simulate_attach_recovery(
            true,
            "snapshot",
            || {
                let attempt = attempts.get() + 1;
                attempts.set(attempt);
                if attempt == 1 {
                    Err(closed_connection_error())
                } else {
                    Ok("ok")
                }
            },
            || {
                recoveries.set(recoveries.get() + 1);
                attempts.set(attempts.get() + 1);
                Ok("ok")
            },
            true,
        );

        assert_eq!(result.expect("retry should recover"), "ok");
        assert_eq!(attempts.get(), 2);
        assert_eq!(recoveries.get(), 1);
    }

    #[test]
    fn attach_recovery_surfaces_structured_degraded_error_after_single_retry() {
        let attempts = Cell::new(0usize);
        let recoveries = Cell::new(0usize);

        let result = simulate_attach_recovery(
            true,
            "snapshot",
            || {
                attempts.set(attempts.get() + 1);
                Err::<(), _>(closed_connection_error())
            },
            || {
                recoveries.set(recoveries.get() + 1);
                attempts.set(attempts.get() + 1);
                Err::<(), _>(closed_connection_error())
            },
            true,
        )
        .expect_err("persistent page-target loss should surface a degraded attach-session error");

        assert_eq!(attempts.get(), 2);
        assert_eq!(recoveries.get(), 1);
        let BrowserError::PageTargetLost(details) = result else {
            panic!("expected degraded page-target-loss error, got {result:?}");
        };
        assert!(details.is_attach_session_degraded());
        assert_eq!(details.operation, "snapshot");
        assert!(!details.recoverable);
        assert!(details.detail.contains("One recovery attempt ran"));
        assert!(
            details
                .recovery_hint
                .as_deref()
                .unwrap_or_default()
                .contains("tab_list")
        );
    }

    #[test]
    fn attach_recovery_does_not_retry_when_tab_inventory_is_gone() {
        let attempts = Cell::new(0usize);
        let recoveries = Cell::new(0usize);

        let result = simulate_attach_recovery(
            true,
            "snapshot",
            || {
                attempts.set(attempts.get() + 1);
                Err::<(), _>(closed_connection_error())
            },
            || {
                recoveries.set(recoveries.get() + 1);
                attempts.set(attempts.get() + 1);
                Ok(())
            },
            false,
        )
        .expect_err("without surviving inventory the original error should bubble");

        assert_eq!(attempts.get(), 1);
        assert_eq!(recoveries.get(), 0);
        assert!(matches!(result, BrowserError::EvaluationFailed(_)));
    }

    #[test]
    fn page_target_loss_classifier_builds_typed_recoverable_details() {
        let details = recoverable_page_target_loss_details("evaluate", &closed_connection_error())
            .expect("closed connection should classify as page-target loss");

        assert_eq!(details.operation, "evaluate");
        assert!(details.recoverable);
        assert!(details.recovery_hint.is_none());
        assert!(details.detail.contains("underlying connection is closed"));
        assert!(is_recoverable_page_target_loss(
            &BrowserError::PageTargetLost(details)
        ));
    }

    #[test]
    fn screenshot_request_rejects_empty_tab_id() {
        let request = ScreenshotRequest {
            mode: ScreenshotMode::Viewport,
            scale: ScreenshotScale::Device,
            tab_id: Some("  ".to_string()),
            clip: None,
        };

        let err = request
            .validate()
            .expect_err("empty tab ids should be rejected");

        assert!(matches!(err, BrowserError::InvalidArgument(_)));
    }

    #[test]
    fn screenshot_request_rejects_full_page_clip_combination() {
        let request = ScreenshotRequest {
            mode: ScreenshotMode::FullPage,
            scale: ScreenshotScale::Device,
            tab_id: None,
            clip: Some(ScreenshotClip {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 80.0,
            }),
        };

        let err = request
            .validate()
            .expect_err("full-page clipped screenshots should be rejected");

        assert!(matches!(err, BrowserError::InvalidArgument(_)));
    }

    #[test]
    fn png_dimensions_reads_fake_backend_png_header() {
        let bytes = vec![
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1,
            8, 6, 0, 0, 0, 31, 21, 196, 137,
        ];

        let (width, height) = png_dimensions(&bytes).expect("header should parse");
        assert_eq!((width, height), (1, 1));
    }

    #[test]
    fn decode_base64_standard_decodes_png_signature() {
        let decoded = decode_base64_standard("iVBORw0KGgo=").expect("base64 should decode");
        assert_eq!(decoded, b"\x89PNG\r\n\x1a\n");
    }
}

#[cfg(test)]
mod fake;

#[cfg(test)]
pub(crate) use fake::FakeSessionBackend;

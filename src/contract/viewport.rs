//! Viewport emulation requests and post-operation metrics for breakpoint testing.
//!
//! [`ViewportEmulationRequest`] applies CDP device metrics; large canvases need
//! `allow_large_viewport`. Successful set/reset invalidates session snapshot
//! caches so locality-biased reads do not reuse stale viewport bases.

/// Measured viewport size and device pixel ratio after emulation or reset.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ViewportMetrics {
    /// Layout viewport width in CSS pixels.
    pub width: f64,
    /// Layout viewport height in CSS pixels.
    pub height: f64,
    /// Page `devicePixelRatio` after the operation.
    pub device_pixel_ratio: f64,
}

/// Active CDP viewport emulation settings after `set_viewport` applies them.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ViewportEmulation {
    /// Emulated viewport width in CSS pixels.
    pub width: u32,
    /// Emulated viewport height in CSS pixels.
    pub height: u32,
    /// Device scale factor passed to CDP `Emulation.setDeviceMetricsOverride`.
    pub device_scale_factor: f64,
    /// When true, emulate a mobile user agent metrics profile.
    pub mobile: bool,
    /// When true, enable touch emulation for pointer events.
    pub touch: bool,
    /// Optional screen orientation override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orientation: Option<ViewportOrientation>,
}

#[derive(
    Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
/// Screen orientation values accepted by viewport emulation. Serialized values are
/// `portrait_primary`, `portrait_secondary`, `landscape_primary`, and `landscape_secondary`.
pub enum ViewportOrientation {
    /// Upright portrait (primary, 0°).
    PortraitPrimary,
    /// Upside-down portrait (secondary, 180°).
    PortraitSecondary,
    /// Landscape with top of device on the left (primary, 90°).
    LandscapePrimary,
    /// Landscape with top of device on the right (secondary, 270°).
    LandscapeSecondary,
}

/// Input for applying viewport emulation on a tab, including large-canvas opt-in.
///
/// Dimensions are validated against default or large-canvas caps before CDP is called.
/// Session snapshot caches are invalidated after a successful apply.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ViewportEmulationRequest {
    /// Emulated layout width in CSS pixels (validated against size caps).
    pub width: u32,
    /// Emulated layout height in CSS pixels (validated against size caps).
    pub height: u32,
    /// Device scale factor for CDP `Emulation.setDeviceMetricsOverride` (default 1.0).
    #[serde(default = "ViewportEmulationRequest::default_device_scale_factor")]
    pub device_scale_factor: f64,
    /// When true, request a mobile metrics profile from CDP.
    #[serde(default)]
    pub mobile: bool,
    /// When true, enable touch emulation for pointer events.
    #[serde(default)]
    pub touch: bool,
    /// Optional screen orientation override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orientation: Option<ViewportOrientation>,
    /// When set, emulate on this tab; otherwise use the active page target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<String>,
    /// Opt into oversized dimensions for intentional large-canvas or regression captures.
    #[serde(default)]
    pub allow_large_viewport: bool,
}

impl ViewportEmulationRequest {
    const fn default_device_scale_factor() -> f64 {
        1.0
    }
}

/// Clear CDP device metrics and touch emulation on a tab, restoring native viewport behavior.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub struct ViewportResetRequest {
    /// When set, reset this tab; otherwise reset the active page target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<String>,
}

/// Post-operation viewport state returned by set/reset viewport tools.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ViewportOperationResult {
    /// Tab that was emulated or reset.
    pub tab_id: String,
    /// Active emulation after set; `None` after a successful reset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emulation: Option<ViewportEmulation>,
    /// Measured layout metrics after the CDP operation.
    pub viewport_after: ViewportMetrics,
}

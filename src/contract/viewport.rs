//! Viewport emulation requests and post-operation metrics for breakpoint testing.

/// Measured viewport size and device pixel ratio after emulation or reset.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ViewportMetrics {
    pub width: f64,
    pub height: f64,
    pub device_pixel_ratio: f64,
}

/// Active CDP viewport emulation settings after `set_viewport` applies them.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ViewportEmulation {
    pub width: u32,
    pub height: u32,
    pub device_scale_factor: f64,
    pub mobile: bool,
    pub touch: bool,
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
    PortraitPrimary,
    PortraitSecondary,
    LandscapePrimary,
    LandscapeSecondary,
}

/// Input for applying viewport emulation on a tab, including large-canvas opt-in.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ViewportEmulationRequest {
    pub width: u32,
    pub height: u32,
    #[serde(default = "ViewportEmulationRequest::default_device_scale_factor")]
    pub device_scale_factor: f64,
    #[serde(default)]
    pub mobile: bool,
    #[serde(default)]
    pub touch: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orientation: Option<ViewportOrientation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<String>,
    #[serde(default)]
    pub allow_large_viewport: bool,
}

impl ViewportEmulationRequest {
    const fn default_device_scale_factor() -> f64 {
        1.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub struct ViewportResetRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<String>,
}

/// Post-operation viewport state returned by `set_viewport`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ViewportOperationResult {
    pub tab_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emulation: Option<ViewportEmulation>,
    pub viewport_after: ViewportMetrics,
}

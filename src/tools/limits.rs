use crate::error::{BrowserError, Result};

pub(crate) const MAX_WAIT_TIMEOUT_MS: u64 = 120_000;

pub(crate) const SCREENSHOT_MAX_CSS_DIMENSION: f64 = 32_768.0;
pub(crate) const SCREENSHOT_MAX_CSS_AREA: f64 = 50_000_000.0;
pub(crate) const SCREENSHOT_MAX_PNG_BYTES: usize = 64 * 1024 * 1024;

pub(crate) fn validate_wait_timeout(timeout_ms: u64) -> Result<()> {
    if timeout_ms > MAX_WAIT_TIMEOUT_MS {
        return Err(BrowserError::InvalidArgument(format!(
            "wait.timeout_ms must be less than or equal to {MAX_WAIT_TIMEOUT_MS}"
        )));
    }

    Ok(())
}

pub(crate) fn validate_screenshot_css_size(source: &str, width: f64, height: f64) -> Result<()> {
    if width > SCREENSHOT_MAX_CSS_DIMENSION {
        return Err(BrowserError::resource_limit_exceeded(
            "screenshot_css_width",
            format!(
                "screenshot {source} width exceeds the {SCREENSHOT_MAX_CSS_DIMENSION:.0} CSS pixel limit"
            ),
            format!("{SCREENSHOT_MAX_CSS_DIMENSION:.0} CSS pixels"),
            format!("{width:.0} CSS pixels"),
        ));
    }

    if height > SCREENSHOT_MAX_CSS_DIMENSION {
        return Err(BrowserError::resource_limit_exceeded(
            "screenshot_css_height",
            format!(
                "screenshot {source} height exceeds the {SCREENSHOT_MAX_CSS_DIMENSION:.0} CSS pixel limit"
            ),
            format!("{SCREENSHOT_MAX_CSS_DIMENSION:.0} CSS pixels"),
            format!("{height:.0} CSS pixels"),
        ));
    }

    let area = width * height;
    if area > SCREENSHOT_MAX_CSS_AREA {
        return Err(BrowserError::resource_limit_exceeded(
            "screenshot_css_area",
            format!(
                "screenshot {source} area exceeds the {SCREENSHOT_MAX_CSS_AREA:.0} CSS pixel limit"
            ),
            format!("{SCREENSHOT_MAX_CSS_AREA:.0} CSS pixels"),
            format!("{area:.0} CSS pixels"),
        ));
    }

    Ok(())
}

pub(crate) fn validate_screenshot_png_bytes(byte_count: usize) -> Result<()> {
    if byte_count > SCREENSHOT_MAX_PNG_BYTES {
        return Err(BrowserError::resource_limit_exceeded(
            "screenshot_png_bytes",
            format!(
                "screenshot PNG is {byte_count} bytes, exceeding the {SCREENSHOT_MAX_PNG_BYTES} byte limit"
            ),
            format!("{SCREENSHOT_MAX_PNG_BYTES} bytes"),
            format!("{byte_count} bytes"),
        ));
    }

    Ok(())
}

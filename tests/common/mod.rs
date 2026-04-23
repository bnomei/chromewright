#![allow(dead_code)]

use chromewright::{BrowserError, BrowserSession, LaunchOptions, Result};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(50);
const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

fn launch_error_is_environmental(message: &str) -> bool {
    [
        "didn't give us a WebSocket URL before we timed out",
        "Could not auto detect a chrome executable",
        "Running as root without --no-sandbox is not supported",
    ]
    .iter()
    .any(|fragment| message.contains(fragment))
}

pub fn browser_test_guard() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    match LOCK.get_or_init(|| Mutex::new(())).lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            eprintln!("Recovering browser test lock after a prior panic");
            poisoned.into_inner()
        }
    }
}

pub fn launch_or_skip() -> Option<BrowserSession> {
    for attempt in 1..=3 {
        match BrowserSession::launch(LaunchOptions::new().headless(true)) {
            Ok(mut session) => {
                session.tool_registry_mut().register_operator_tools();
                return Some(session);
            }
            Err(err) if launch_error_is_environmental(&err.to_string()) => {
                if attempt == 3 {
                    eprintln!(
                        "Skipping browser integration test due to environment after {} attempt(s): {}",
                        attempt, err
                    );
                    return None;
                }

                std::thread::sleep(Duration::from_millis(250));
            }
            Err(err) => panic!("Unexpected launch failure: {}", err),
        }
    }

    None
}

pub struct BrowserTestContext {
    _guard: MutexGuard<'static, ()>,
    session: BrowserSession,
}

impl BrowserTestContext {
    pub fn session(&self) -> &BrowserSession {
        &self.session
    }
}

pub fn browser_or_skip() -> Option<BrowserTestContext> {
    let guard = browser_test_guard();
    let session = launch_or_skip()?;

    Some(BrowserTestContext {
        _guard: guard,
        session,
    })
}

pub fn navigate_and_wait(session: &BrowserSession, url: &str) -> Result<()> {
    session.navigate(url)?;
    wait_for_document_ready(session)
}

pub fn navigate_html(session: &BrowserSession, html: impl AsRef<str>) -> Result<()> {
    let data_url = format!("data:text/html,{}", html.as_ref());
    navigate_and_wait(session, &data_url)
}

pub fn navigate_encoded_html(session: &BrowserSession, html: impl AsRef<str>) -> Result<()> {
    let data_url = encoded_html_url(html);
    navigate_and_wait(session, &data_url)
}

pub fn encoded_html_url(html: impl AsRef<str>) -> String {
    format!("data:text/html,{}", urlencoding::encode(html.as_ref()))
}

pub fn wait_for_document_ready(session: &BrowserSession) -> Result<()> {
    session.wait_for_document_ready_with_timeout(DEFAULT_TIMEOUT)
}

pub fn wait_for_url_contains(session: &BrowserSession, needle: &str) -> Result<()> {
    wait_until(
        &format!("active tab URL to contain {needle:?}"),
        DEFAULT_TIMEOUT,
        || Ok(session.document_metadata()?.url.contains(needle)),
    )
}

pub fn wait_for_tab_count(session: &BrowserSession, expected: usize) -> Result<()> {
    wait_until(
        &format!("tab count to equal {}", expected),
        DEFAULT_TIMEOUT,
        || Ok(session.list_tabs()?.len() == expected),
    )
}

pub fn wait_for_tab_count_at_least(session: &BrowserSession, minimum: usize) -> Result<()> {
    wait_until(
        &format!("tab count to reach at least {}", minimum),
        DEFAULT_TIMEOUT,
        || Ok(session.list_tabs()?.len() >= minimum),
    )
}

pub fn evaluate(session: &BrowserSession, js: &str) -> Result<Value> {
    let result = session.execute_tool(
        "evaluate",
        serde_json::json!({
            "code": js,
            "await_promise": false,
            "confirm_unsafe": true,
        }),
    )?;

    if !result.success {
        return Err(BrowserError::ToolExecutionFailed {
            tool: "evaluate".to_string(),
            reason: result
                .error
                .unwrap_or_else(|| "evaluate failed".to_string()),
        });
    }

    Ok(result
        .data
        .and_then(|data| data.get("result").cloned())
        .unwrap_or(Value::Null))
}

pub fn wait_for_eval_truthy(
    session: &BrowserSession,
    description: &str,
    js: &str,
    timeout: Duration,
) -> Result<()> {
    wait_until(description, timeout, || {
        let value = evaluate(session, js).map_err(|err| {
            BrowserError::EvaluationFailed(format!(
                "Failed to evaluate wait probe for {}: {}",
                description, err
            ))
        })?;

        Ok(json_value_is_truthy(Some(&value)))
    })
}

pub fn wait_until<F>(description: &str, timeout: Duration, mut check: F) -> Result<()>
where
    F: FnMut() -> Result<bool>,
{
    let start = Instant::now();

    loop {
        if check()? {
            return Ok(());
        }

        if start.elapsed() >= timeout {
            return Err(BrowserError::Timeout(format!(
                "Timed out waiting for {} within {} ms",
                description,
                timeout.as_millis()
            )));
        }

        std::thread::sleep(POLL_INTERVAL);
    }
}

fn json_value_is_truthy(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(value)) => *value,
        Some(Value::Null) | None => false,
        Some(Value::Number(value)) => value
            .as_i64()
            .map(|number| number != 0)
            .or_else(|| value.as_u64().map(|number| number != 0))
            .or_else(|| value.as_f64().map(|number| number != 0.0))
            .unwrap_or(false),
        Some(Value::String(value)) => !value.is_empty(),
        Some(Value::Array(value)) => !value.is_empty(),
        Some(Value::Object(value)) => !value.is_empty(),
    }
}

pub fn assert_png_screenshot_artifact(data: &Value) -> PathBuf {
    assert_eq!(data["format"].as_str(), Some("png"));
    assert_eq!(data["mime_type"].as_str(), Some("image/png"));
    assert!(
        data["artifact_uri"]
            .as_str()
            .unwrap_or_default()
            .starts_with("file://"),
        "artifact_uri should point at a managed file"
    );

    let artifact_path = PathBuf::from(
        data["artifact_path"]
            .as_str()
            .expect("artifact_path should be returned"),
    );
    assert!(
        artifact_path.is_file(),
        "artifact_path should exist on disk: {}",
        artifact_path.display()
    );

    let bytes = std::fs::read(&artifact_path).expect("screenshot artifact should be readable");
    assert!(
        bytes.starts_with(PNG_SIGNATURE),
        "screenshot artifact should be a PNG"
    );
    assert_eq!(data["byte_count"].as_u64(), Some(bytes.len() as u64));

    let (width, height) = png_dimensions(&bytes);
    assert_eq!(data["width"].as_u64(), Some(width as u64));
    assert_eq!(data["height"].as_u64(), Some(height as u64));

    artifact_path
}

fn png_dimensions(bytes: &[u8]) -> (u32, u32) {
    assert!(
        bytes.len() >= 24 && &bytes[..PNG_SIGNATURE.len()] == PNG_SIGNATURE,
        "PNG header should be present"
    );

    let width = u32::from_be_bytes(bytes[16..20].try_into().expect("width bytes should exist"));
    let height = u32::from_be_bytes(bytes[20..24].try_into().expect("height bytes should exist"));
    (width, height)
}

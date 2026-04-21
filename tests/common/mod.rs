use chromewright::{BrowserSession, LaunchOptions};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;

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
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .expect("browser test lock should not be poisoned")
}

pub fn launch_or_skip() -> Option<BrowserSession> {
    for attempt in 1..=3 {
        match BrowserSession::launch(LaunchOptions::new().headless(true)) {
            Ok(session) => return Some(session),
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

//! History navigation with document-settle polling for back and forward tools.

use super::BrowserSession;
use crate::error::{BrowserError, Result};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct HistoryNavigationMetrics {
    pub browser_evaluations: u64,
    pub poll_iterations: u64,
}

impl BrowserSession {
    fn wait_for_history_settle(
        &self,
        previous_url: &str,
        timeout: Duration,
    ) -> Result<HistoryNavigationMetrics> {
        let start = Instant::now();
        let mut observed_navigation = false;
        let mut metrics = HistoryNavigationMetrics::default();

        loop {
            metrics.poll_iterations += 1;
            let document = self.document_metadata()?;
            let current_url = document.url;
            if current_url != previous_url {
                observed_navigation = true;
            }

            metrics.browser_evaluations += 1;
            let elapsed = start.elapsed();
            let grace_period = Duration::from_millis(500);

            if document.ready_state == "complete"
                && (observed_navigation || elapsed >= grace_period)
            {
                return Ok(metrics);
            }

            if elapsed >= timeout {
                return Err(BrowserError::Timeout(format!(
                    "History navigation did not settle within {} ms",
                    timeout.as_millis()
                )));
            }

            std::thread::sleep(Duration::from_millis(50));
        }
    }

    pub(crate) fn go_back_with_metrics(
        &self,
        allow_unsafe: bool,
    ) -> Result<HistoryNavigationMetrics> {
        self.history_navigate(HistoryDirection::Back, allow_unsafe)
    }

    pub(crate) fn go_forward_with_metrics(
        &self,
        allow_unsafe: bool,
    ) -> Result<HistoryNavigationMetrics> {
        self.history_navigate(HistoryDirection::Forward, allow_unsafe)
    }

    fn history_navigate(
        &self,
        direction: HistoryDirection,
        allow_unsafe: bool,
    ) -> Result<HistoryNavigationMetrics> {
        let previous_url = self.document_metadata()?.url;

        self.evaluate(direction.move_js(), false).map_err(|e| {
            BrowserError::NavigationFailed(format!("Failed to {}: {}", direction.verb(), e))
        })?;
        self.invalidate_snapshot_cache()?;
        let settle_metrics = self.wait_for_history_settle(&previous_url, Duration::from_secs(5))?;

        let landed_url = self.document_metadata()?.url;
        if !allow_unsafe && is_unsafe_history_scheme(&landed_url) {
            let reverse = direction.reverse();
            if self.evaluate(reverse.move_js(), false).is_ok() {
                let _ = self.wait_for_history_settle(&landed_url, Duration::from_secs(5));
            }
            self.invalidate_snapshot_cache()?;

            return Err(BrowserError::InvalidArgument(format!(
                "History {} landed on unsafe destination '{}'; pass allow_unsafe=true to opt in.",
                direction.verb(),
                landed_url
            )));
        }

        Ok(HistoryNavigationMetrics {
            browser_evaluations: settle_metrics.browser_evaluations + 1,
            poll_iterations: settle_metrics.poll_iterations,
        })
    }
}

#[derive(Debug, Clone, Copy)]
enum HistoryDirection {
    Back,
    Forward,
}

impl HistoryDirection {
    fn verb(self) -> &'static str {
        match self {
            HistoryDirection::Back => "go back",
            HistoryDirection::Forward => "go forward",
        }
    }

    fn reverse(self) -> Self {
        match self {
            HistoryDirection::Back => HistoryDirection::Forward,
            HistoryDirection::Forward => HistoryDirection::Back,
        }
    }

    fn move_js(self) -> &'static str {
        match self {
            HistoryDirection::Back => {
                r#"
            (function() {
                window.history.back();
                return true;
            })()
        "#
            }
            HistoryDirection::Forward => {
                r#"
            (function() {
                window.history.forward();
                return true;
            })()
        "#
            }
        }
    }
}

fn is_unsafe_history_scheme(url: &str) -> bool {
    let Some((scheme, _rest)) = url.split_once(':') else {
        return false;
    };

    let is_absolute_scheme = !scheme.is_empty()
        && scheme
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'));
    if !is_absolute_scheme {
        return false;
    }

    !matches!(
        scheme.to_ascii_lowercase().as_str(),
        "http" | "https" | "about"
    )
}

#[cfg(test)]
mod tests {
    use crate::browser::backend::FakeSessionBackend;
    use crate::browser::session::{BrowserSession, SnapshotCacheEntry, SnapshotCacheScope};
    use std::sync::Arc;

    fn seed_snapshot_cache(session: &BrowserSession) {
        let document = session
            .document_metadata()
            .expect("document metadata should be available");

        session
            .store_snapshot_cache(Arc::new(SnapshotCacheEntry {
                document,
                snapshot: Arc::<str>::from("button \"Fake target\""),
                nodes: Arc::<[crate::dom::SnapshotNode]>::from(Vec::new()),
                scope: SnapshotCacheScope {
                    mode: "viewport".to_string(),
                    fallback_mode: None,
                    viewport_biased: true,
                    returned_node_count: 0,
                    unavailable_frame_count: 0,
                    global_interactive_count: Some(1),
                },
            }))
            .expect("snapshot cache should store");
    }

    #[test]
    fn history_navigation_invalidates_snapshot_cache() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        seed_snapshot_cache(&session);
        assert!(
            session
                .snapshot_cache_for_test()
                .expect("snapshot cache should be readable")
                .is_some()
        );

        session
            .go_back_with_metrics(false)
            .expect("history navigation should succeed");
        assert!(
            session
                .snapshot_cache_for_test()
                .expect("snapshot cache should be readable")
                .is_none()
        );

        let mut current = session
            .document_metadata()
            .expect("document metadata should still be readable");
        current.revision.push_str("-forward");

        session
            .store_snapshot_cache(Arc::new(SnapshotCacheEntry {
                document: current,
                snapshot: Arc::<str>::from("button \"Fake target\""),
                nodes: Arc::<[crate::dom::SnapshotNode]>::from(Vec::new()),
                scope: SnapshotCacheScope {
                    mode: "viewport".to_string(),
                    fallback_mode: None,
                    viewport_biased: true,
                    returned_node_count: 0,
                    unavailable_frame_count: 0,
                    global_interactive_count: Some(1),
                },
            }))
            .expect("snapshot cache should store");
        session
            .go_forward_with_metrics(false)
            .expect("history forward should succeed");
        assert!(
            session
                .snapshot_cache_for_test()
                .expect("snapshot cache should be readable")
                .is_none()
        );
    }

    #[test]
    fn go_back_blocks_unsafe_scheme_without_opt_in() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        session
            .navigate("file:///Users/agent/secrets.txt")
            .expect("fake navigate should set the active tab url");

        let err = session
            .go_back_with_metrics(false)
            .expect_err("history navigation onto file: should be blocked by default");
        let message = err.to_string();
        assert!(
            message.contains("allow_unsafe=true"),
            "unexpected error message: {message}"
        );
        assert!(message.contains("file:///Users/agent/secrets.txt"));
    }

    #[test]
    fn public_go_back_blocks_unsafe_scheme_by_default() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        session
            .navigate("file:///Users/agent/secrets.txt")
            .expect("fake navigate should set the active tab url");

        let err = session
            .go_back()
            .expect_err("public go_back should be safe by default");
        let message = err.to_string();
        assert!(message.contains("allow_unsafe=true"));
        assert!(message.contains("file:///Users/agent/secrets.txt"));
    }

    #[test]
    fn public_go_forward_blocks_unsafe_scheme_by_default() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        session
            .navigate("file:///Users/agent/secrets.txt")
            .expect("fake navigate should set the active tab url");

        let err = session
            .go_forward()
            .expect_err("public go_forward should be safe by default");
        let message = err.to_string();
        assert!(message.contains("allow_unsafe=true"));
        assert!(message.contains("file:///Users/agent/secrets.txt"));
    }

    #[test]
    fn go_back_allows_unsafe_scheme_with_opt_in() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        session
            .navigate("file:///Users/agent/secrets.txt")
            .expect("fake navigate should set the active tab url");

        session
            .go_back_with_metrics(true)
            .expect("explicit opt-in should permit history navigation onto file:");
    }

    #[test]
    fn is_unsafe_history_scheme_matches_navigate_gate() {
        assert!(!super::is_unsafe_history_scheme("https://example.com/page"));
        assert!(!super::is_unsafe_history_scheme("http://localhost:3000"));
        assert!(!super::is_unsafe_history_scheme("about:blank"));
        assert!(!super::is_unsafe_history_scheme("/relative/path"));
        assert!(super::is_unsafe_history_scheme(
            "file:///Users/agent/secrets.txt"
        ));
        assert!(super::is_unsafe_history_scheme("data:text/html,<h1>x</h1>"));
        assert!(super::is_unsafe_history_scheme("chrome://settings"));
    }
}

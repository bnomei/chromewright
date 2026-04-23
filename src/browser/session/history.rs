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

    pub(crate) fn go_back_with_metrics(&self) -> Result<HistoryNavigationMetrics> {
        let previous_url = self.document_metadata()?.url;
        let go_back_js = r#"
            (function() {
                window.history.back();
                return true;
            })()
        "#;

        self.evaluate(go_back_js, false)
            .map_err(|e| BrowserError::NavigationFailed(format!("Failed to go back: {}", e)))?;
        self.invalidate_snapshot_cache()?;
        let settle_metrics = self.wait_for_history_settle(&previous_url, Duration::from_secs(5))?;

        Ok(HistoryNavigationMetrics {
            browser_evaluations: settle_metrics.browser_evaluations + 1,
            poll_iterations: settle_metrics.poll_iterations,
        })
    }

    pub(crate) fn go_forward_with_metrics(&self) -> Result<HistoryNavigationMetrics> {
        let previous_url = self.document_metadata()?.url;
        let go_forward_js = r#"
            (function() {
                window.history.forward();
                return true;
            })()
        "#;

        self.evaluate(go_forward_js, false)
            .map_err(|e| BrowserError::NavigationFailed(format!("Failed to go forward: {}", e)))?;
        self.invalidate_snapshot_cache()?;
        let settle_metrics = self.wait_for_history_settle(&previous_url, Duration::from_secs(5))?;

        Ok(HistoryNavigationMetrics {
            browser_evaluations: settle_metrics.browser_evaluations + 1,
            poll_iterations: settle_metrics.poll_iterations,
        })
    }
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
                nodes: Vec::new(),
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
            .go_back_with_metrics()
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
                nodes: Vec::new(),
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
            .go_forward_with_metrics()
            .expect("history forward should succeed");
        assert!(
            session
                .snapshot_cache_for_test()
                .expect("snapshot cache should be readable")
                .is_none()
        );
    }
}

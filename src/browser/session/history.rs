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
        let settle_metrics = self.wait_for_history_settle(&previous_url, Duration::from_secs(5))?;

        Ok(HistoryNavigationMetrics {
            browser_evaluations: settle_metrics.browser_evaluations + 1,
            poll_iterations: settle_metrics.poll_iterations,
        })
    }
}

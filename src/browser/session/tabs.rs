use super::{BrowserSession, ClosedTabSummary, ManagedTabsCloseSummary, TabInfo};
use crate::browser::backend::{AttachSessionDegradedDetails, TabDescriptor};
use crate::error::{BrowserError, Result};

impl BrowserSession {
    pub(crate) fn tab_overview(&self) -> Result<Vec<TabInfo>> {
        let tabs = self.backend.list_tabs()?;
        let active_id = match self.backend.active_tab() {
            Ok(tab) => Some(tab.id),
            Err(BrowserError::TabOperationFailed(reason))
                if reason.contains("No active tab found")
                    || AttachSessionDegradedDetails::decode(&reason).is_some() =>
            {
                None
            }
            Err(err) => return Err(err),
        };

        Ok(tabs
            .into_iter()
            .map(|tab| TabInfo {
                active: active_id.as_deref() == Some(tab.id.as_str()),
                id: tab.id,
                title: tab.title,
                url: tab.url,
            })
            .collect())
    }

    pub(crate) fn activate_tab_by_id(&self, tab_id: &str) -> Result<()> {
        self.backend.activate_tab(tab_id)?;
        self.invalidate_snapshot_cache()
    }

    pub(crate) fn open_tab_entry(&self, url: &str) -> Result<TabDescriptor> {
        let tab = self.backend.open_tab(url)?;
        self.remember_managed_tab(tab.id.clone())?;
        self.invalidate_snapshot_cache()?;
        Ok(tab)
    }

    pub(crate) fn close_active_tab_summary(&self) -> Result<ClosedTabSummary> {
        let tabs = self.backend.list_tabs()?;
        let active = self.backend.active_tab()?;
        let index = tabs.iter().position(|tab| tab.id == active.id).unwrap_or(0);

        self.backend.close_tab(&active.id, true)?;
        self.forget_managed_tab(&active.id)?;
        self.invalidate_snapshot_cache()?;
        let active_tab = self.tab_overview()?.into_iter().find(|tab| tab.active);

        Ok(ClosedTabSummary {
            index,
            id: active.id,
            title: active.title,
            url: active.url,
            active_tab,
        })
    }

    pub(crate) fn close_managed_tabs(&self) -> Result<ManagedTabsCloseSummary> {
        let tabs = self.tab_overview()?;
        let mut managed_tabs = Vec::new();

        for tab in &tabs {
            if self.is_tab_managed(&tab.id)? {
                managed_tabs.push(tab.clone());
            }
        }

        let skipped_tabs = tabs.len().saturating_sub(managed_tabs.len());
        let attempted = managed_tabs.len();
        let mut closed_tabs = 0usize;
        let mut failures = Vec::new();

        for tab in managed_tabs {
            match self.backend.close_tab(&tab.id, false) {
                Ok(()) => {
                    self.forget_managed_tab(&tab.id)?;
                    closed_tabs += 1;
                }
                Err(err) => failures.push(format!(
                    "failed to close '{}' ({}) [id={}]: {}",
                    tab.title, tab.url, tab.id, err
                )),
            }
        }

        if failures.is_empty() {
            if closed_tabs > 0 {
                self.invalidate_snapshot_cache()?;
            }
            Ok(ManagedTabsCloseSummary {
                closed_tabs,
                skipped_tabs,
            })
        } else {
            Err(BrowserError::TabOperationFailed(format!(
                "Managed session close encountered {} error(s) after attempting {} managed tab(s): {}",
                failures.len(),
                attempted,
                failures.join("; ")
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::backend::{FakeSessionBackend, ScriptEvaluation, SessionBackend};
    use crate::browser::session::{
        BrowserSession, SessionOrigin, SnapshotCacheEntry, SnapshotCacheScope,
    };
    use crate::dom::{DocumentMetadata, DomTree, SnapshotNode};
    use std::sync::Arc;
    use std::time::Duration;

    struct DegradedActiveTabBackend;

    fn seed_snapshot_cache(session: &BrowserSession) {
        let document = session
            .document_metadata()
            .expect("document metadata should be available");

        session
            .store_snapshot_cache(Arc::new(SnapshotCacheEntry {
                document,
                snapshot: Arc::<str>::from("button \"Fake target\""),
                nodes: Vec::<SnapshotNode>::new(),
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

    impl SessionBackend for DegradedActiveTabBackend {
        fn navigate(&self, _url: &str) -> Result<()> {
            unreachable!("navigate is not used in this test")
        }

        fn wait_for_navigation(&self) -> Result<()> {
            unreachable!("wait_for_navigation is not used in this test")
        }

        fn wait_for_document_ready_with_timeout(&self, _timeout: Duration) -> Result<()> {
            unreachable!("wait_for_document_ready_with_timeout is not used in this test")
        }

        fn document_metadata(&self) -> Result<DocumentMetadata> {
            unreachable!("document_metadata is not used in this test")
        }

        fn extract_dom(&self) -> Result<DomTree> {
            unreachable!("extract_dom is not used in this test")
        }

        fn extract_dom_with_prefix(&self, _prefix: &str) -> Result<DomTree> {
            unreachable!("extract_dom_with_prefix is not used in this test")
        }

        fn evaluate(&self, _script: &str, _await_promise: bool) -> Result<ScriptEvaluation> {
            unreachable!("evaluate is not used in this test")
        }

        fn capture_screenshot(&self, _full_page: bool) -> Result<Vec<u8>> {
            unreachable!("capture_screenshot is not used in this test")
        }

        fn press_key(&self, _key: &str) -> Result<()> {
            unreachable!("press_key is not used in this test")
        }

        fn list_tabs(&self) -> Result<Vec<TabDescriptor>> {
            Ok(vec![TabDescriptor {
                id: "tab-1".to_string(),
                title: "Existing".to_string(),
                url: "https://example.com".to_string(),
            }])
        }

        fn active_tab(&self) -> Result<TabDescriptor> {
            Err(AttachSessionDegradedDetails::page_target_lost(
                "active_tab",
                "DOM-backed page access is degraded".to_string(),
            )
            .into_browser_error())
        }

        fn open_tab(&self, _url: &str) -> Result<TabDescriptor> {
            unreachable!("open_tab is not used in this test")
        }

        fn activate_tab(&self, _tab_id: &str) -> Result<()> {
            unreachable!("activate_tab is not used in this test")
        }

        fn close_tab(&self, _tab_id: &str, _with_unload: bool) -> Result<()> {
            unreachable!("close_tab is not used in this test")
        }

        fn close(&self) -> Result<()> {
            unreachable!("close is not used in this test")
        }
    }

    #[test]
    fn tab_overview_does_not_claim_an_active_tab_when_attach_session_is_degraded() {
        let session = BrowserSession::with_test_backend_origin(
            DegradedActiveTabBackend,
            SessionOrigin::Connected,
        );

        let tabs = session
            .tab_overview()
            .expect("tab inventory should still be available");

        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[0].id, "tab-1");
        assert!(!tabs[0].active);
    }

    #[test]
    fn activate_tab_invalidates_snapshot_cache() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let opened = session
            .open_tab_entry("https://second.example")
            .expect("second tab should open");

        seed_snapshot_cache(&session);
        assert!(
            session
                .snapshot_cache_for_test()
                .expect("snapshot cache should be readable")
                .is_some()
        );

        session
            .activate_tab_by_id("tab-1")
            .expect("tab activation should succeed");
        assert!(
            session
                .snapshot_cache_for_test()
                .expect("snapshot cache should be readable")
                .is_none()
        );
        assert_eq!(opened.id, "tab-2");
    }

    #[test]
    fn open_and_close_tab_seams_invalidate_snapshot_cache() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());

        seed_snapshot_cache(&session);
        session
            .open_tab_entry("https://second.example")
            .expect("tab open should succeed");
        assert!(
            session
                .snapshot_cache_for_test()
                .expect("snapshot cache should be readable")
                .is_none()
        );

        seed_snapshot_cache(&session);
        session
            .close_active_tab_summary()
            .expect("active tab close should succeed");
        assert!(
            session
                .snapshot_cache_for_test()
                .expect("snapshot cache should be readable")
                .is_none()
        );
    }

    #[test]
    fn close_managed_tabs_invalidates_snapshot_cache_when_tabs_are_closed() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        session
            .open_tab_entry("https://managed.example")
            .expect("managed tab should open");

        seed_snapshot_cache(&session);
        let summary = session
            .close_managed_tabs()
            .expect("managed tab close should succeed");

        assert_eq!(summary.closed_tabs, 2);
        assert!(
            session
                .snapshot_cache_for_test()
                .expect("snapshot cache should be readable")
                .is_none()
        );
    }
}

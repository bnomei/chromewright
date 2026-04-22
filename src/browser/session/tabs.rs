use super::{BrowserSession, ClosedTabSummary, TabInfo};
use crate::browser::backend::TabDescriptor;
use crate::error::{BrowserError, Result};

impl BrowserSession {
    pub(crate) fn tab_overview(&self) -> Result<Vec<TabInfo>> {
        let tabs = self.backend.list_tabs()?;
        let active_id = match self.backend.active_tab() {
            Ok(tab) => Some(tab.id),
            Err(BrowserError::TabOperationFailed(reason))
                if reason.contains("No active tab found") =>
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
        self.backend.activate_tab(tab_id)
    }

    pub(crate) fn open_tab_entry(&self, url: &str) -> Result<TabDescriptor> {
        let tab = self.backend.open_tab(url)?;
        self.remember_managed_tab(tab.id.clone())?;
        Ok(tab)
    }

    pub(crate) fn close_active_tab_summary(&self) -> Result<ClosedTabSummary> {
        let tabs = self.backend.list_tabs()?;
        let active = self.backend.active_tab()?;
        let index = tabs.iter().position(|tab| tab.id == active.id).unwrap_or(0);

        self.backend.close_tab(&active.id, true)?;
        self.forget_managed_tab(&active.id)?;
        let active_tab = self.tab_overview()?.into_iter().find(|tab| tab.active);

        Ok(ClosedTabSummary {
            index,
            id: active.id,
            title: active.title,
            url: active.url,
            active_tab,
        })
    }
}

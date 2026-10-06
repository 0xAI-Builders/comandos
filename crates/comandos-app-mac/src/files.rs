//! Native file adapter for Store's single document authority. No SQL or mode transitions.
#![forbid(unsafe_code)]
use comandos_desktop::mac_tabs::{
    SavedTabs, TabMeta, archive_into, load_saved_tabs, load_tab_metadata,
};
use comandos_store::{
    Error, Result,
    domains::{DocHandle, DomainStore},
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Component, PathBuf},
    sync::Mutex,
    time::Duration,
};
const LIMIT: usize = comandos_desktop::state_files::MAX_BYTES;
#[derive(Clone, Copy)]
enum Document {
    Tabs,
    Meta,
    History,
}
impl Document {
    fn names(self) -> (&'static str, &'static str) {
        match self {
            Self::Tabs => ("app-tabs.json", "H/app-tabs.json"),
            Self::Meta => ("app-tabs-meta.json", "H/app-tabs-meta.json"),
            Self::History => ("app-tabs-history.json", "H/app-tabs-history.json"),
        }
    }
}
pub struct MacFiles {
    home: PathBuf,
    hooks: PathBuf,
    expected_tabs: Mutex<Option<Option<Vec<u8>>>>,
}
impl MacFiles {
    pub fn new(home: PathBuf, hooks: PathBuf) -> Result<Self> {
        if [&home, &hooks]
            .iter()
            .any(|p| !p.is_absolute() || p.components().any(|c| matches!(c, Component::ParentDir)))
        {
            return Err(Error::Validation("invalid Mac file scope".into()));
        }
        Ok(Self {
            home,
            hooks,
            expected_tabs: Mutex::new(None),
        })
    }
    fn document(&self, document: Document) -> DocHandle<'_> {
        let (name, key) = document.names();
        DomainStore { home: &self.home }.document(key, "tabs", self.hooks.join(name))
    }
    fn read(&self, name: Document) -> Result<Option<Vec<u8>>> {
        let bytes = self.document(name).read_readonly()?;
        if bytes.as_ref().is_some_and(|b| b.len() > LIMIT) {
            return Err(Error::Validation("Mac tab document exceeds 8 MiB".into()));
        }
        Ok(bytes)
    }
    pub fn load_saved(&self) -> Result<SavedTabs> {
        let raw = self.read(Document::Tabs)?;
        let result = raw.as_deref().map(load_saved_tabs).unwrap_or_default();
        *self
            .expected_tabs
            .lock()
            .map_err(|_| Error::Validation("Mac file cache unavailable".into()))? = Some(raw);
        Ok(result)
    }
    pub fn saved_label(&self, session: &str) -> Result<Option<String>> {
        Ok(self
            .read(Document::Tabs)?
            .as_deref()
            .map(load_saved_tabs)
            .unwrap_or_default()
            .into_iter()
            .find(|(s, _)| s == session)
            .and_then(|(_, v)| v.as_str().filter(|s| !s.is_empty()).map(str::to_string)))
    }
    pub fn metadata(&self) -> Result<BTreeMap<String, TabMeta>> {
        load_tab_metadata(&self.read(Document::Meta)?.unwrap_or_default())
            .map_err(|e| Error::Validation(format!("invalid Mac metadata: {e:?}")))
    }
    pub fn save_tabs(&self, next: &Value, allowed: &dyn Fn() -> bool, now_ms: i64) -> Result<bool> {
        if !allowed() {
            return Ok(false);
        }
        if !next.is_object() {
            return Err(Error::Validation("Mac tab labels require object".into()));
        }
        let mut expected = self
            .expected_tabs
            .lock()
            .map_err(|_| Error::Validation("Mac file cache unavailable".into()))?;
        if expected.is_none() {
            *expected = Some(self.read(Document::Tabs)?);
        }
        let bytes = comandos_core::json::response_dumps(next)
            .map_err(Error::Validation)?
            .into_bytes();
        if bytes.len() > LIMIT {
            return Err(Error::Validation("Mac tab labels exceed 8 MiB".into()));
        }
        let wrote = self.document(Document::Tabs).update_owned_when(
            now_ms,
            Duration::from_secs(3),
            allowed,
            |old| {
                if expected.as_ref().and_then(Option::as_deref) != old {
                    return Err(Error::Validation(
                        "Mac tab labels changed; stale save refused".into(),
                    ));
                }
                if !allowed() {
                    return Ok(None);
                }
                Ok(Some(bytes.clone()))
            },
        )?;
        if wrote {
            *expected = Some(Some(bytes));
        }
        Ok(wrote)
    }
    pub fn archive(&self, item: &Value, allowed: &dyn Fn() -> bool, now_ms: i64) -> Result<bool> {
        self.document(Document::History).update_owned_when(
            now_ms,
            Duration::from_secs(3),
            allowed,
            |old| {
                if old.is_some_and(|b| b.len() > LIMIT) {
                    return Err(Error::Validation("Mac history exceeds 8 MiB".into()));
                }
                let history = old
                    .and_then(|b| std::str::from_utf8(b).ok())
                    .and_then(|s| comandos_core::json::workspace_loads(s).ok());
                let next = archive_into(history, item.clone());
                let bytes = comandos_core::json::response_dumps(&next)
                    .map_err(Error::Validation)?
                    .into_bytes();
                if bytes.len() > LIMIT {
                    return Err(Error::Validation("Mac history exceeds 8 MiB".into()));
                }
                if !allowed() {
                    return Ok(None);
                }
                Ok(Some(bytes))
            },
        )
    }
}

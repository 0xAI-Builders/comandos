//! Scoped, explicit before-mutation adapter for installer-owned transactions.
//! Standalone commands retain their original IO when no observer is installed.
use crate::Result;
use std::{cell::RefCell, path::Path, rc::Rc};
pub enum Mutation<'a> {
    /// Coordination files are validated and retained, never unlinked on rollback.
    Control {
        path: &'a Path,
    },
    File {
        path: &'a Path,
        bytes: &'a [u8],
        mode: u32,
    },
    Directory {
        path: &'a Path,
        mode: u32,
    },
    Link {
        path: &'a Path,
        target: &'a Path,
    },
    Move {
        source: &'a Path,
        target: &'a Path,
    },
    Remove {
        path: &'a Path,
    },
}
pub trait Observer {
    fn before(&mut self, mutation: &Mutation<'_>) -> Result<()>;
    fn after(&mut self, path: &Path) -> Result<()>;
    fn document_committed(&mut self, name: &str) -> Result<()>;
    #[allow(clippy::too_many_arguments)]
    fn document(
        &mut self,
        home: &Path,
        db: &rusqlite::Connection,
        name: &str,
        bytes: &[u8],
        origin: comandos_store::unified::Origin,
        now: i64,
    ) -> Result<()>;
}
type Shared = Rc<RefCell<dyn Observer>>;
thread_local! { static ACTIVE:RefCell<Option<Shared>>=const {RefCell::new(None)}; }
struct Reset;
impl Drop for Reset {
    fn drop(&mut self) {
        ACTIVE.with(|active| *active.borrow_mut() = None);
    }
}
pub fn with<T>(observer: Shared, body: impl FnOnce() -> Result<T>) -> Result<T> {
    ACTIVE.with(|active| -> Result<()> {
        let mut active = active.borrow_mut();
        if active.is_some() {
            return Err("nested extension mutation adapter".into());
        }
        *active = Some(observer);
        Ok(())
    })?;
    let _reset = Reset;
    body()
}
pub(crate) fn before(mutation: Mutation<'_>) -> Result<()> {
    ACTIVE.with(|active| match active.borrow().as_ref() {
        Some(observer) => observer.borrow_mut().before(&mutation),
        None => Ok(()),
    })
}
pub(crate) fn after(path: &Path) -> Result<()> {
    ACTIVE.with(|active| match active.borrow().as_ref() {
        Some(observer) => observer.borrow_mut().after(path),
        None => Ok(()),
    })
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn document(
    home: &Path,
    db: &rusqlite::Connection,
    name: &str,
    bytes: &[u8],
    origin: comandos_store::unified::Origin,
    now: i64,
) -> Result<()> {
    ACTIVE.with(|active| match active.borrow().as_ref() {
        Some(observer) => observer
            .borrow_mut()
            .document(home, db, name, bytes, origin, now),
        None => Ok(()),
    })
}

pub(crate) fn document_committed(name: &str) -> Result<()> {
    ACTIVE.with(|active| match active.borrow().as_ref() {
        Some(observer) => observer.borrow_mut().document_committed(name),
        None => Ok(()),
    })
}

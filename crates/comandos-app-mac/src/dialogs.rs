//! Exact original dialog copy and captured tab identity, without AppKit.
#![forbid(unsafe_code)]
use comandos_desktop::Lang;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabScope {
    pub key: String,
    pub instance: u64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogOutcome {
    Close {
        scope: TabScope,
        confirmed: bool,
    },
    Rename {
        scope: TabScope,
        label: Option<String>,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloseText {
    pub message: String,
    pub informative: &'static str,
    pub accept: &'static str,
    pub cancel: &'static str,
}
pub fn close_text(lang: Lang, label: &str) -> CloseText {
    if lang == Lang::Es {
        CloseText {
            message: format!("¿Cerrar la pestana «{label}»?"),
            informative: "La sesion tmux sigue viva; puedes recuperarla en Recientes del tablero.",
            accept: "Cerrar",
            cancel: "Cancelar",
        }
    } else {
        CloseText {
            message: format!("Close tab “{label}”?"),
            informative: "The tmux session stays alive; recover it from the dashboard's Recents.",
            accept: "Close",
            cancel: "Cancel",
        }
    }
}
pub fn rename_text(lang: Lang) -> (&'static str, &'static str, &'static str) {
    if lang == Lang::Es {
        ("Renombrar pestana", "OK", "Cancelar")
    } else {
        ("Rename tab", "OK", "Cancel")
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DialogId(u64);
#[derive(Default)]
pub struct DialogOwner {
    next: u64,
    active: Option<(DialogId, TabScope)>,
}
impl DialogOwner {
    pub fn begin(&mut self, scope: TabScope) -> Option<DialogId> {
        if self.active.is_some() {
            return None;
        }
        self.next = self.next.checked_add(1)?;
        let id = DialogId(self.next);
        self.active = Some((id, scope));
        Some(id)
    }
    pub fn finish(&mut self, id: DialogId) -> bool {
        if self
            .active
            .as_ref()
            .is_some_and(|(current, _)| *current == id)
        {
            self.active.take();
            true
        } else {
            false
        }
    }
    pub fn cancel(&mut self, scope: &TabScope) -> bool {
        if self
            .active
            .as_ref()
            .is_some_and(|(_, captured)| captured == scope)
        {
            self.active.take();
            true
        } else {
            false
        }
    }
    pub fn shutdown(&mut self) {
        self.active.take();
    }
}

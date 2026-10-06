//! Acciones de pestaña ejecutadas fuera del hilo de GTK.
use crate::{config::RunMode, restore::RestoreTmux};
use serde_json::{Value, json};
use std::sync::Mutex;
pub fn idle_scratch_commands(session: &str, success: bool, commands: &str) -> bool {
    let commands: Vec<_> = commands.split_whitespace().collect();
    session.starts_with("term-")
        && success
        && !commands.is_empty()
        && commands
            .iter()
            .all(|c| matches!(*c, "zsh" | "bash" | "sh" | "fish"))
}
pub use comandos_desktop::validation::{valid_session, valid_window};
pub fn select_window<T: RestoreTmux>(tmux: &T, session: &str, window: &str) -> Result<(), String> {
    if tmux.mode() == RunMode::Shadow {
        return Err("Selección de tmux rechazada en sombra".into());
    }
    if !valid_session(session) {
        return Err("Sesión inválida".into());
    }
    let window = if valid_window(window) {
        window
    } else {
        "claude"
    };
    if session.starts_with("ssh-")
        || session.strip_prefix("sshtab-").is_some_and(|s| {
            s.rsplit_once('-').is_some_and(|(host, n)| {
                !host.is_empty() && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())
            })
        })
    {
        tmux.mutate(&["set-option", "-t", session, "mouse", "off"], None)?;
    }
    if tmux
        .mutate(
            &["select-window", "-t", &format!("={session}:{window}")],
            None,
        )
        .is_ok()
        || window != "claude"
    {
        return Ok(());
    }
    let listing = tmux
        .read(&[
            "list-panes",
            "-s",
            "-t",
            &format!("={session}"),
            "-F",
            "#{window_index}|#{pane_current_command}",
        ])
        .unwrap_or_default();
    if let Some(index) = listing.lines().find_map(|l| {
        l.split_once('|')
            .filter(|(_, command)| *command == "claude")
            .map(|(index, _)| index)
    }) {
        tmux.mutate(
            &["select-window", "-t", &format!("={session}:{index}")],
            None,
        )?;
    } else {
        tmux.mutate(&["select-window", "-t", &format!("={session}:^")], None)?;
    }
    Ok(())
}
#[derive(Default)]
pub struct QuickTerminal {
    pending: Mutex<Option<String>>,
}
impl QuickTerminal {
    pub fn request(
        &self,
        make_id: impl FnOnce() -> Result<String, String>,
        post: impl FnOnce(&Value) -> Result<Value, String>,
    ) -> Result<Value, String> {
        let id = {
            let mut pending = self.pending.lock().map_err(|_| "Solicitud envenenada")?;
            if pending.is_none() {
                *pending = Some(make_id()?);
            }
            pending.as_ref().cloned().ok_or("Falta identidad")?
        };
        let response = post(&json!({"requestId":id}))?;
        if response
            .get("tabId")
            .and_then(Value::as_str)
            .is_some_and(valid_session)
        {
            if let Ok(mut pending) = self.pending.lock()
                && pending.as_ref() == Some(&id)
            {
                pending.take();
            }
            Ok(response)
        } else {
            Err("sin respuesta de cc-dash".into())
        }
    }
}
#[derive(Debug, Clone)]
pub struct PaneClose {
    session: String,
    pane: String,
    identity: Value,
    pub title: String,
}
impl PaneClose {
    pub fn prepare(
        session: &str,
        pane: &str,
        mut post: impl FnMut(&Value) -> Result<Value, String>,
    ) -> Result<Self, String> {
        if !valid_session(session)
            || !pane
                .strip_prefix('%')
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        {
            return Err("Panel inválido".into());
        }
        let listing = post(&json!({"session":session,"action":"list"}))?;
        let chosen = listing
            .get("panes")
            .and_then(Value::as_array)
            .and_then(|panes| {
                panes
                    .iter()
                    .find(|p| p.get("id").and_then(Value::as_str) == Some(pane))
            })
            .ok_or("El panel ya no existe")?;
        let identity = chosen
            .get("identity")
            .filter(|v| v.as_str().is_some_and(|s| !s.is_empty()))
            .cloned()
            .ok_or("Falta identidad del panel")?;
        Ok(Self {
            session: session.into(),
            pane: pane.into(),
            identity,
            title: chosen
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or(pane)
                .into(),
        })
    }
    pub fn finish(
        &self,
        confirmed: bool,
        cancelled: bool,
        post: impl FnOnce(&Value) -> Result<Value, String>,
    ) -> Result<Option<Value>, String> {
        if !confirmed || cancelled {
            return Ok(None);
        }
        post(&json!({"session":self.session,"action":"close","pane":self.pane,"identity":self.identity})).map(Some)
    }
}

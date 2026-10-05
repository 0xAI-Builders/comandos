//! Native effects: paths, clock, random reception ids and process identity.
use comandos_store::{Error, Result};
use rusqlite::Connection;
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
pub mod accounts;
pub mod acp_client;
pub mod agent_procs;
pub mod claude_trust;
pub mod cli_catalog;
pub mod cli_help;
pub mod closed_panes;
pub mod command_chains;
pub mod dialogs;
pub mod events_cli;
pub mod extension_launch;
pub mod hooks;
pub mod launch_command;
pub mod legacy;
pub mod limits;
pub mod model_catalog;
pub mod news_agents;
pub mod pane_exit;
pub mod pane_extensions;
pub mod pane_observe;
pub mod pane_snapshot;
pub mod pane_typing;
pub mod providers;
pub mod quick_terminal;
pub mod session_configuration;
pub mod session_operations;
pub mod ssh_config;
pub mod terminal_history;
pub mod terminal_panes;
pub mod tmux_snapshot;
pub mod tui_state;

/// El Python leería algo que este port no reproduce con certeza: quien lo
/// recibe declina (el frente reenvía al heredado).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unsure;

pub fn state_path(
    explicit: Option<&Path>,
    override_path: Option<&Path>,
    xdg: Option<&Path>,
    home: Option<&Path>,
) -> Result<PathBuf> {
    let nonempty = |p: &&Path| !p.as_os_str().is_empty();
    if let Some(path) = explicit.filter(nonempty).or(override_path.filter(nonempty)) {
        return Ok(path.into());
    }
    let base = if let Some(xdg) = xdg.filter(nonempty) {
        xdg.into()
    } else {
        home.filter(nonempty)
            .ok_or_else(|| {
                Error::Validation("directorio personal no disponible; especifica --state".into())
            })?
            .join(".local/state")
    };
    Ok(base.join("comandos/app-state.sqlite3"))
}

pub fn process_start_time(pid: &str) -> Option<u64> {
    if pid.is_empty() || pid.len() > 10 || !pid.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let number = pid.parse::<u64>().ok()?;
    parse_process_stat(&std::fs::read_to_string(format!("/proc/{number}/stat")).ok()?)
}

pub fn parse_process_stat(stat: &str) -> Option<u64> {
    stat.rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(19)?
        .parse()
        .ok()
}

pub fn now_ms() -> Result<u64> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| Error::Validation(e.to_string()))?;
    u64::try_from(duration.as_millis()).map_err(|e| Error::Validation(e.to_string()))
}

pub fn fresh_id(prefix: &str) -> Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| Error::Io(std::io::Error::other(e.to_string())))?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let mut id = String::with_capacity(prefix.len() + 33);
    id.push_str(prefix);
    id.push('-');
    for byte in bytes {
        write!(&mut id, "{byte:02x}").expect("String formatting");
    }
    Ok(id)
}

pub fn open_state(path: &Path, busy_ms: u64) -> Result<Connection> {
    for attempt in 0..3 {
        let conn = comandos_store::state::connect(path)?;
        conn.busy_timeout(Duration::from_millis(busy_ms))?;
        match comandos_store::state::migrate(
            &conn,
            comandos_store::state::MIGRATIONS,
            now_ms()? as f64 / 1000.0,
        ) {
            Ok(_) => return Ok(conn),
            Err(Error::Sql(_)) if attempt < 2 => {
                drop(conn);
                std::thread::sleep(Duration::from_millis(50 * (attempt + 1)));
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!("last migration attempt returns its result")
}

//! Lista cerrada: el puente puede consultar y engancharse, nunca crear o matar sesiones.
use crate::dash::native::tmux::Tmux;
use std::path::PathBuf;
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TmuxTarget {
    User,
    Private(PathBuf),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachCommand {
    Version,
    HasSession(String),
    ListSessionNames,
    Attach { session: String, active_pane: bool },
}
impl AttachCommand {
    pub fn args(&self) -> Vec<String> {
        match self {
            Self::Version => vec!["-V".into()],
            Self::HasSession(s) => vec!["has-session".into(), "-t".into(), format!("={s}")],
            Self::ListSessionNames => vec![
                "list-sessions".into(),
                "-F".into(),
                "#{session_name}".into(),
            ],
            Self::Attach {
                session,
                active_pane,
            } => {
                let mut args = vec!["attach".into()];
                if *active_pane {
                    args.extend(["-f".into(), "active-pane".into()]);
                }
                args.extend(["-t".into(), format!("={session}")]);
                args
            }
        }
    }
}
impl TmuxTarget {
    pub fn prefix(&self) -> Vec<std::ffi::OsString> {
        match self {
            Self::User => Vec::new(),
            Self::Private(dir) => Tmux::private(dir).program.prefix,
        }
    }
    pub(crate) fn probe(&self, command: &AttachCommand) -> tokio::process::Command {
        let mut cmd = tokio::process::Command::new("tmux");
        cmd.args(self.prefix())
            .args(command.args())
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .kill_on_drop(true);
        if let Self::Private(dir) = self {
            cmd.env_clear()
                .env("HOME", dir.join("home"))
                .env("PATH", "/usr/bin:/bin")
                .env("SHELL", "/bin/sh");
        }
        cmd
    }
    pub(crate) fn pty_command(&self, command: Option<&AttachCommand>) -> pty_process::Command {
        let mut cmd = if let Some(command) = command {
            pty_process::Command::new("tmux")
                .args(self.prefix())
                .args(command.args())
        } else {
            let shell = if matches!(self, Self::Private(_)) {
                "/bin/sh".into()
            } else {
                std::env::var_os("SHELL")
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "/bin/sh".into())
            };
            pty_process::Command::new(shell)
        };
        if let Self::Private(dir) = self {
            cmd = cmd
                .env_clear()
                .env("HOME", dir.join("home"))
                .env("PATH", "/usr/bin:/bin")
                .env("SHELL", "/bin/sh");
        }
        cmd.env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .env("TERM", "xterm-256color")
            .env("COLORTERM", "truecolor")
            .kill_on_drop(true)
    }
}
pub fn valid_session(name: &str) -> bool {
    (1..=80).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}
pub fn supports_active_pane(version: &str) -> bool {
    let parts: Vec<u32> = version
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .take(2)
        .filter_map(|s| s.parse().ok())
        .collect();
    matches!(parts.as_slice(), [major,minor] if *major>3 || (*major==3 && *minor>=2))
}

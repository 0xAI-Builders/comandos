//! Mode selects the socket before any process can be constructed.
#![forbid(unsafe_code)]
use crate::app::RunMode;
pub fn argv_for(mode: &RunMode, args: &[&str]) -> Vec<String> {
    let mut out = vec!["tmux".into()];
    if let RunMode::Sandbox { tmux_socket, .. } = mode {
        out.extend(["-S".into(), tmux_socket.to_string_lossy().into_owned()]);
    }
    out.extend(args.iter().map(|s| s.to_string()));
    out
}
use comandos_desktop::proc::{ProcOutput, ProcSpec, run_when};
use std::{path::PathBuf, time::Duration};
pub type TmuxOut = Result<ProcOutput, String>;
pub trait TmuxRunner: Send + Sync {
    fn run_when(&self, args: &[&str], allowed: &dyn Fn() -> bool) -> TmuxOut;
    fn run(&self, args: &[&str]) -> TmuxOut {
        self.run_when(args, &|| true)
    }
}
pub struct UserTmux {
    _private: (),
}
pub struct PrivateTmux {
    socket: PathBuf,
}
pub fn runner(mode: &RunMode) -> Box<dyn TmuxRunner> {
    match mode {
        RunMode::Live => Box::new(UserTmux { _private: () }),
        RunMode::Sandbox { tmux_socket, .. } => Box::new(PrivateTmux {
            socket: tmux_socket.clone(),
        }),
    }
}
fn execute(mode: &RunMode, args: &[&str], allowed: &dyn Fn() -> bool) -> TmuxOut {
    let argv = argv_for(mode, args);
    let mut env = vec![];
    if matches!(mode, RunMode::Sandbox { .. }) {
        env.push(("LC_ALL".into(), "en_US.UTF-8".into()));
    }
    run_when(
        &ProcSpec {
            program: "tmux".into(),
            args: argv.into_iter().skip(1).map(Into::into).collect(),
            stdin: None,
            env,
            clear_env: false,
            env_remove: if matches!(mode, RunMode::Sandbox { .. }) {
                vec!["TMUX".into()]
            } else {
                vec![]
            },
            cwd: None,
            timeout: Duration::from_secs(10),
        },
        allowed,
    )
    .map_err(|e| format!("tmux: {e:?}"))
}
impl TmuxRunner for UserTmux {
    fn run_when(&self, args: &[&str], allowed: &dyn Fn() -> bool) -> TmuxOut {
        execute(&RunMode::Live, args, allowed)
    }
}
impl TmuxRunner for PrivateTmux {
    fn run_when(&self, args: &[&str], allowed: &dyn Fn() -> bool) -> TmuxOut {
        execute(
            &RunMode::Sandbox {
                tmux_socket: self.socket.clone(),
                hooks: PathBuf::new(),
            },
            args,
            allowed,
        )
    }
}

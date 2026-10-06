//! Procesos externos (`tmux`, `fc-list`) como `subprocess.run(..., text=True,
//! capture_output=True, timeout=…)` del Python: stdout/stderr decodificados
//! como UTF-8 estricto con saltos universales; al vencer el plazo se mata.
use super::py::repr_ascii;
use crate::HandlerError;
use std::{
    ffi::OsString,
    io,
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

#[derive(Debug, Clone)]
pub struct Program {
    pub path: PathBuf,
    /// Argumentos que van antes de los del llamador (dobles de prueba).
    pub prefix: Vec<OsString>,
    pub env: Vec<(OsString, OsString)>,
    pub env_remove: Vec<OsString>,
    /// `env_clear` antes de `env`: el hijo solo ve `env` (pruebas confinadas;
    /// en producción siempre `false`, el entorno del proceso como el Python).
    pub env_clear: bool,
}

/// Variables del frente que ningún hijo hereda: el ajuste de malloc del
/// drop-in de `cc-dash` es solo del frente. Un `tmux` que arranca el servidor
/// (o `systemd-run --scope … tmux new-session`) se lo pasaría a cada pane,
/// agente y terminal.
pub const CHILD_ENV_REMOVE: &[&str] = &[comandos_core::malloc_tuning::GLIBC_TUNABLES_ENV];

impl Program {
    /// Hereda el entorno del proceso, como `subprocess.run`.
    pub fn named(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            prefix: Vec::new(),
            env: Vec::new(),
            env_remove: Vec::new(),
            env_clear: false,
        }
    }

    /// La orden con el prefijo y el entorno del programa, sin las variables de
    /// `CHILD_ENV_REMOVE`. Todo hijo del frente sale de aquí.
    pub fn command(&self) -> tokio::process::Command {
        let mut cmd = tokio::process::Command::new(&self.path);
        cmd.args(&self.prefix);
        if self.env_clear {
            cmd.env_clear();
        }
        for name in CHILD_ENV_REMOVE {
            cmd.env_remove(name);
        }
        for name in &self.env_remove {
            cmd.env_remove(name);
        }
        for (name, value) in &self.env {
            cmd.env(name, value);
        }
        cmd
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    /// `returncode == 0`.
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug)]
pub enum RunError {
    Timeout,
    Spawn(io::Error),
    Decode,
}

/// `text=True`: UTF-8 estricto con saltos universales.
pub(crate) fn universal(bytes: Vec<u8>) -> Result<String, RunError> {
    let text = String::from_utf8(bytes).map_err(|_| RunError::Decode)?;
    Ok(text.replace("\r\n", "\n").replace('\r', "\n"))
}

pub async fn run_program(
    program: &Program,
    args: &[&str],
    timeout: Duration,
) -> Result<Output, RunError> {
    run(program, args, None, timeout).await
}

/// `run_program` con `cwd=…`: un directorio que no existe (o no se puede
/// abrir) da `Spawn`, como el `FileNotFoundError` de `subprocess.run`.
pub async fn run_program_in(
    program: &Program,
    args: &[&str],
    cwd: &Path,
    timeout: Duration,
) -> Result<Output, RunError> {
    run(program, args, Some(cwd), timeout).await
}

async fn run(
    program: &Program,
    args: &[&str],
    cwd: Option<&Path>,
    timeout: Duration,
) -> Result<Output, RunError> {
    let mut cmd = program.command();
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let child = cmd.spawn().map_err(RunError::Spawn)?;
    // Al vencer, el futuro se suelta con el hijo dentro y kill_on_drop lo mata.
    let out = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| RunError::Timeout)?
        .map_err(RunError::Spawn)?;
    Ok(Output {
        ok: out.status.success(),
        stdout: universal(out.stdout)?,
        stderr: universal(out.stderr)?,
    })
}

/// Ruta del socket que tmux usaría con `TMUX_TMPDIR=socket_dir`
/// (`<socket_dir>/tmux-<uid>/default`), para pasarla explícita con `-S`.
pub fn private_socket(socket_dir: &Path) -> PathBuf {
    socket_dir
        .join(format!("tmux-{}", nix::unistd::getuid().as_raw()))
        .join("default")
}

#[derive(Debug, Clone)]
pub struct Tmux {
    pub program: Program,
    pub timeout: Duration,
}

#[derive(Debug)]
pub enum TmuxError {
    Timeout {
        args: Vec<String>,
        after: Duration,
    },
    /// No arrancó (o falló su E/S): la clase y el `errno`, si lo hay.
    Spawn(io::ErrorKind, Option<i32>),
    Decode,
}

impl Tmux {
    /// Producción: `tmux` del PATH con el entorno del proceso (`TMUX_TMPDIR`,
    /// `TMUX`), igual que el Python. Plazo de `tmux()` (5715): 5 s.
    pub fn system() -> Self {
        Self {
            program: Program::named("tmux"),
            timeout: Duration::from_secs(5),
        }
    }

    /// Servidor privado de pruebas: nunca el del usuario. `-f /dev/null`: si esta
    /// llamada arranca el servidor, nace sin `~/.tmux.conf` (que lee
    /// `~/.claude/hooks` y corre `cc-status.sh` en la barra de estado).
    ///
    /// El socket va explícito con `-S`, no solo por `TMUX_TMPDIR`: tmux 3.2a
    /// ignora en silencio un `TMUX_TMPDIR` cuyo directorio no existe y cae en
    /// `/tmp/tmux-<uid>/default`, el servidor real del usuario. Con `-S` un
    /// directorio borrado da «no server running», nunca un `kill-server` ajeno.
    pub fn private(socket_dir: &Path) -> Self {
        let socket = private_socket(socket_dir);
        if let Some(parent) = socket.parent() {
            // 0700 como lo crea tmux: con bits de «otros» rechaza el directorio.
            let _ = std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(parent);
        }
        let mut program = Program::named("tmux");
        program.prefix = vec![
            "-f".into(),
            "/dev/null".into(),
            "-S".into(),
            socket.into_os_string(),
        ];
        program.env.push(("TMUX_TMPDIR".into(), socket_dir.into()));
        program.env_remove.push("TMUX".into());
        Self {
            program,
            timeout: Duration::from_secs(5),
        }
    }

    pub async fn run(&self, args: &[&str]) -> Result<Output, TmuxError> {
        run_program(&self.program, args, self.timeout)
            .await
            .map_err(|error| match error {
                RunError::Timeout => TmuxError::Timeout {
                    args: args.iter().map(|a| (*a).to_owned()).collect(),
                    after: self.timeout,
                },
                RunError::Spawn(e) => TmuxError::Spawn(e.kind(), e.raw_os_error()),
                RunError::Decode => TmuxError::Decode,
            })
    }

    /// `run` desde un hilo de bloqueo (librerías síncronas de `comandos-runtime`).
    /// El reactor, los timers y el reaper los mueve el hilo del runtime, que
    /// está en `Runtime::block_on`; aquí solo se espera el resultado.
    pub fn run_blocking(
        &self,
        handle: &tokio::runtime::Handle,
        args: &[&str],
    ) -> Result<Output, TmuxError> {
        handle.block_on(self.run(args))
    }
}

fn seconds(after: Duration) -> String {
    if after.subsec_nanos() == 0 {
        after.as_secs().to_string()
    } else {
        format!("{}", after.as_secs_f64())
    }
}

impl TmuxError {
    /// `str(exc)` del Python cuando la ruta captura la excepción
    /// (`get_tmux_mouse`/`set_tmux_mouse`). `None`: no hay texto seguro → declinar.
    pub fn python_message(&self) -> Option<String> {
        match self {
            TmuxError::Timeout { args, after } => {
                let mut parts = vec![repr_ascii("tmux")?];
                for arg in args {
                    parts.push(repr_ascii(arg)?);
                }
                Some(format!(
                    "Command '[{}]' timed out after {} seconds",
                    parts.join(", "),
                    seconds(*after)
                ))
            }
            TmuxError::Spawn(io::ErrorKind::NotFound, _) => {
                Some("[Errno 2] No such file or directory: 'tmux'".into())
            }
            TmuxError::Spawn(..) | TmuxError::Decode => None,
        }
    }

    /// Ramas donde el Python NO captura: `TimeoutExpired` → 504, el resto → 500.
    pub fn uncaught(&self) -> HandlerError {
        match self {
            TmuxError::Timeout { .. } => HandlerError::Timeout,
            _ => HandlerError::Failure,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_child_drops_the_front_malloc_tuning() {
        let mut program = Program::named("tmux");
        program.env.push(("TMUX_TMPDIR".into(), "/x".into()));
        let cmd = program.command();
        let envs: Vec<_> = cmd.as_std().get_envs().collect();
        assert!(
            envs.contains(&(std::ffi::OsStr::new("GLIBC_TUNABLES"), None)),
            "{envs:?}"
        );
        assert!(envs.contains(&(
            std::ffi::OsStr::new("TMUX_TMPDIR"),
            Some(std::ffi::OsStr::new("/x"))
        )));
    }
}

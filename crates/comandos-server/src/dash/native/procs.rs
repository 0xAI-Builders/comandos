//! Procesos del frente que no son tmux: entrada por stdin y lanzamientos
//! sueltos (`Popen(..., start_new_session=True)`), la búsqueda de ejecutables
//! en el `PATH` del frente y el registro de tareas largas (D12 del plan 2f).
//!
//! Ligereza: nada de esto crea hilos. Los hijos sueltos los recoge una tarea
//! del runtime (no un hilo por hijo) y las tareas largas son tareas `tokio`.
use super::tmux::{Output, Program, RunError, universal};
use std::{
    ffi::{OsStr, OsString},
    future::Future,
    io,
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::io::AsyncWriteExt;

fn command(program: &Program, args: &[OsString]) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(&program.path);
    cmd.args(&program.prefix).args(args);
    for key in &program.env_remove {
        cmd.env_remove(key);
    }
    for (key, value) in &program.env {
        cmd.env(key, value);
    }
    cmd
}

/// `subprocess.run(args, input=text, capture_output=True, text=True, timeout=…)`.
///
/// Como `communicate()`, la entrada se escribe mientras se lee la salida: un
/// hijo que escribe mucho antes de leer toda su entrada no se queda trabado
/// con la tubería llena. Al vencer el plazo se mata al hijo (`kill_on_drop`) y
/// se devuelve `RunError::Timeout` (el `TimeoutExpired` del Python).
///
/// Solo muere el hijo directo, como `process.kill()` en `subprocess.run`: sus
/// nietos (si el programa lanzó otros) siguen vivos y huérfanos, igual que con
/// el Python. No se crea un grupo de procesos propio para no cambiar a quién
/// llegan las señales del frente; la salida no espera a los nietos porque el
/// futuro que leía las tuberías se suelta con el plazo.
pub async fn run_program_input(
    program: &Program,
    args: &[&str],
    input: &[u8],
    timeout: Duration,
) -> Result<Output, RunError> {
    let args: Vec<OsString> = args.iter().map(OsString::from).collect();
    let mut cmd = command(program, &args);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = cmd.spawn().map_err(RunError::Spawn)?;
    let stdin = child.stdin.take();
    let feed = async move {
        if let Some(mut pipe) = stdin {
            // Un hijo que cierra stdin antes de leerlo todo no es un error del
            // Python (`communicate` ignora `BrokenPipeError`).
            let _ = pipe.write_all(input).await;
            // Al soltar `pipe` se cierra stdin: el hijo ve fin de entrada.
        }
    };
    let run = async move {
        let ((), output) = tokio::join!(feed, child.wait_with_output());
        output
    };
    let output = tokio::time::timeout(timeout, run)
        .await
        .map_err(|_| RunError::Timeout)?
        .map_err(RunError::Spawn)?;
    Ok(Output {
        ok: output.status.success(),
        stdout: universal(output.stdout)?,
        stderr: universal(output.stderr)?,
    })
}

/// `subprocess.Popen(args, env=…, start_new_session=True, stdout=DEVNULL,
/// stderr=DEVNULL)`: no se espera; una tarea del runtime recoge al hijo cuando
/// termine para no dejar zombis (un hijo que no termina nunca no retiene nada:
/// la tarea queda dormida y se abandona al apagar, como un hilo `daemon`).
///
/// `env` se añade al entorno del proceso (lo que `gui_env()` cambia). Grupo de
/// procesos propio (`process_group(0)`): el `setsid` de `start_new_session` a
/// efectos de señales de terminal (el frente no tiene terminal).
///
/// Solo en contexto de runtime (tarea o `spawn_blocking`); fuera de él
/// devuelve un error en vez de entrar en pánico. Desde un hilo de sistema,
/// `spawn_detached_blocking` con el `Handle` del frente.
pub fn spawn_detached(
    program: &Program,
    args: &[OsString],
    env: &[(OsString, OsString)],
) -> io::Result<()> {
    let handle = tokio::runtime::Handle::try_current()
        .map_err(|_| io::Error::other("spawn_detached fuera del runtime"))?;
    let mut cmd = command(program, args);
    for (key, value) in env {
        cmd.env(key, value);
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    let mut child = cmd.spawn()?;
    handle.spawn(async move {
        let _ = child.wait().await;
    });
    Ok(())
}

/// `spawn_detached` desde un hilo sin contexto de runtime (hilos de sistema de
/// las operaciones largas): entra en el runtime del frente con su `Handle`, así
/// el hijo lo recoge la misma tarea de siempre y no hace falta un hilo que
/// espere por cada hijo.
pub fn spawn_detached_blocking(
    handle: &tokio::runtime::Handle,
    program: &Program,
    args: &[OsString],
    env: &[(OsString, OsString)],
) -> io::Result<()> {
    let _entered = handle.enter();
    spawn_detached(program, args, env)
}

/// `gui_env()` del Python (`env.setdefault("DISPLAY", ":1")`) como lo que hay
/// que añadir al entorno heredado del proceso.
pub fn gui_env() -> Vec<(OsString, OsString)> {
    gui_env_with(std::env::var_os("DISPLAY"))
}

/// `gui_env()` con el `DISPLAY` actual dado (las pruebas no tocan el entorno
/// del proceso). Un `DISPLAY` presente, aunque vacío, no se cambia.
pub fn gui_env_with(display: Option<OsString>) -> Vec<(OsString, OsString)> {
    if display.is_some() {
        Vec::new()
    } else {
        vec![(OsString::from("DISPLAY"), OsString::from(":1"))]
    }
}

/// `shutil.which(name)` sobre el `PATH` que el frente tomó al arrancar
/// (`opts.search_path`), no el del proceso. Todo programa externo del frente
/// se lanza por la ruta absoluta que devuelve esto, nunca por nombre suelto:
/// un nombre suelto lo resolvería el `PATH` real del proceso (en pruebas, el
/// del desarrollador). No mira los bins de usuario de `providers::which`.
pub fn which_in(search_path: Option<&OsStr>, name: &str) -> Option<PathBuf> {
    comandos_runtime::providers::which_path(name, search_path)
}

/// Tareas largas del frente (operaciones de sesión, `send-keys` diferidos,
/// agentes de noticias). Se abandonan al apagar, como los hilos `daemon` del
/// Python. `len()` cuenta las que siguen vivas (también si una revienta o el
/// runtime la suelta: la cuenta se baja al soltar la tarea).
#[derive(Default)]
pub struct TaskTracker {
    running: Arc<AtomicUsize>,
}

/// Baja la cuenta al soltarse: al terminar, al entrar en pánico o al
/// abandonarse la tarea con el runtime.
struct Running(Arc<AtomicUsize>);

impl Drop for Running {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

impl TaskTracker {
    /// `tokio::spawn` registrado. Fuera de un runtime no lanza nada y
    /// devuelve el error (nunca un pánico).
    pub fn spawn<F>(&self, fut: F) -> std::io::Result<()>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let handle = tokio::runtime::Handle::try_current().map_err(std::io::Error::other)?;
        self.running.fetch_add(1, Ordering::AcqRel);
        let running = Running(Arc::clone(&self.running));
        handle.spawn(async move {
            let _running = running;
            fut.await;
        });
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.running.load(Ordering::Acquire)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

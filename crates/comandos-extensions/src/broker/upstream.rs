//! Proceso upstream stdio compartido: lanzado con el mismo `command(spec, true)` que usa
//! `serve`, con stdin/stdout por tuberías y stderr heredado (va al journal del daemon).
use super::{blank, read_line};
use crate::Result;
use nix::{
    sys::signal::{Signal, killpg},
    unistd::Pid,
};
use serde_json::Value;
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncWriteExt, BufReader},
    process::Child,
    sync::mpsc,
};

/// Líneas encoladas hacia el stdin del upstream; al llenarse, el actor deja de leer clientes.
const STDIN_QUEUE: usize = 256;
/// Líneas del stdout del upstream pendientes de que el actor las procese.
const LINE_QUEUE: usize = 256;
/// Espera entre SIGTERM y SIGKILL al cerrar un upstream.
const GRACE: Duration = Duration::from_secs(5);

pub(super) struct Upstream {
    /// Escritor del stdin (una tarea); cada elemento es una línea sin `\n`.
    pub stdin: mpsc::Sender<Vec<u8>>,
    /// Líneas del stdout; `None` cuando el upstream cierra su salida.
    pub lines: mpsc::Receiver<Vec<u8>>,
    pub child: Child,
    pub pid: u32,
}

/// Lanza `spec` como lo habría lanzado el proxy directo de la sesión: en `cwd` y con su
/// `env` expandido superpuesto al entorno del daemon.
pub(super) fn spawn(spec: &Value, cwd: &Path, env: &[(String, String)]) -> Result<Upstream> {
    let mut spec = spec.clone();
    if let Some(o) = spec.as_object_mut() {
        o.remove("cwd");
    }
    let mut std_command = crate::command_env(&spec, true, Some(env))?;
    std_command
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    let mut command = tokio::process::Command::from(std_command);
    // Grupo propio para que el cierre alcance a los nietos (npx → node). `kill_on_drop`
    // solo cubre al líder: es la red si el actor desaparece sin pasar por `terminate`.
    command.kill_on_drop(true).process_group(0);
    let mut child = command
        .spawn()
        .map_err(|_| "No se pudo lanzar el servidor")?;
    let pid = child.id().ok_or("El servidor terminó al arrancar")?;
    let mut stdin = child.stdin.take().ok_or("Servidor sin stdin")?;
    let stdout = child.stdout.take().ok_or("Servidor sin stdout")?;
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(STDIN_QUEUE);
    tokio::spawn(async move {
        while let Some(mut line) = rx.recv().await {
            line.push(b'\n');
            if stdin.write_all(&line).await.is_err() || stdin.flush().await.is_err() {
                break;
            }
        }
    });
    let (line_tx, lines) = mpsc::channel(LINE_QUEUE);
    tokio::spawn(async move {
        let mut reader = BufReader::new(stdout);
        let mut buf = Vec::new();
        loop {
            match read_line(&mut reader, &mut buf).await {
                Ok(true) if blank(&buf) => {}
                Ok(true) => {
                    if line_tx.send(buf.clone()).await.is_err() {
                        break;
                    }
                }
                Ok(false) => break,
                Err(e) => {
                    eprintln!("broker: upstream {pid}: salida ilegible ({e})");
                    break;
                }
            }
        }
    });
    Ok(Upstream {
        stdin: tx,
        lines,
        child,
        pid,
    })
}

/// SIGTERM al grupo, 5 s de gracia para el líder, SIGKILL; recoge al líder y al final manda
/// SIGKILL otra vez al grupo para los nietos que ignoren SIGTERM (ESRCH si ya no queda
/// nadie). El líder sin recoger reserva su pid como id de grupo, así que la primera señal
/// no puede alcanzar a un proceso ajeno.
pub(super) async fn terminate(up: &mut Upstream) -> String {
    let group = Pid::from_raw(up.pid as i32);
    let _ = killpg(group, Signal::SIGTERM);
    let status = match tokio::time::timeout(GRACE, up.child.wait()).await {
        Ok(status) => status,
        Err(_) => {
            let _ = killpg(group, Signal::SIGKILL);
            up.child.wait().await
        }
    };
    let _ = killpg(group, Signal::SIGKILL);
    status.map_or_else(|_| "desconocido".into(), |s| s.to_string())
}

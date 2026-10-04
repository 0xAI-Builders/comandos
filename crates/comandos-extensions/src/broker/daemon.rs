//! `comandos ext broker`: escucha en el socket Unix, atiende cada `{"attach":"<nombre>"}`
//! y conecta al cliente con el actor de ese nombre (lanzándolo si no existe).
use super::{
    actor::{self, Handle, Joined, Msg},
    blank, brokerable, check_private_dir, read_line, socket_path, upstream,
};
use crate::Result;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions, TryLockError},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};
use tokio::{
    io::{AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    signal::unix::{SignalKind, signal},
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
};

/// Plazo para que un cliente recién conectado envíe su línea `attach`.
const ATTACH_TIMEOUT: Duration = Duration::from_secs(5);
/// Margen para que los actores cierren sus upstreams (5 s de gracia + recogida).
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Default)]
struct Registry {
    actors: HashMap<String, Handle>,
    /// Actores reemplazados mientras se cerraban; se esperan al parar el daemon.
    retired: Vec<JoinHandle<()>>,
}

struct Ctx {
    catalog: PathBuf,
    idle: Duration,
    stop: watch::Receiver<bool>,
    /// Solo se bloquea sin `.await` dentro (lanzar un proceso es síncrono).
    registry: Mutex<Registry>,
}

/// Corre el daemon hasta SIGTERM/SIGINT. Error si ya hay otro vivo o el socket no se crea.
pub async fn run(catalog: &Path) -> Result<()> {
    let mut term = signal(SignalKind::terminate()).map_err(|_| "Señales no disponibles")?;
    let mut int = signal(SignalKind::interrupt()).map_err(|_| "Señales no disponibles")?;
    let path = socket_path();
    let (_lock, listener) = bind(&path)?;
    let idle = std::env::var("COMANDOS_BROKER_IDLE_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(600);
    let (stop_tx, stop) = watch::channel(false);
    let ctx = Arc::new(Ctx {
        catalog: catalog.into(),
        idle: Duration::from_secs(idle),
        stop,
        registry: Mutex::default(),
    });
    eprintln!(
        "broker: escuchando en {} (inactividad {idle} s)",
        path.display()
    );
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => {
                    tokio::spawn(connection(stream, ctx.clone()));
                }
                Err(e) => {
                    eprintln!("broker: accept falló ({e})");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            },
            _ = term.recv() => break,
            _ = int.recv() => break,
        }
    }
    drop(listener);
    let _ = fs::remove_file(&path);
    let _ = stop_tx.send(true);
    let tasks: Vec<JoinHandle<()>> = {
        let mut reg = ctx.registry.lock().unwrap_or_else(PoisonError::into_inner);
        let mut tasks: Vec<_> = reg.actors.drain().map(|(_, h)| h.task).collect();
        tasks.append(&mut reg.retired);
        tasks
    };
    let all = futures_util::future::join_all(tasks);
    if tokio::time::timeout(SHUTDOWN_TIMEOUT, all).await.is_err() {
        eprintln!("broker: algunos upstreams no cerraron a tiempo");
    }
    eprintln!("broker: parado");
    Ok(())
}

/// Candado de instancia única + socket. Con el candado tomado, un socket existente es de
/// un daemon muerto: se borra y se vuelve a crear.
fn bind(path: &Path) -> Result<(File, UnixListener)> {
    let dir = path.parent().ok_or("Ruta de socket inválida")?;
    crate::config::private_dir(dir)?;
    check_private_dir(dir).map_err(|_| format!("Directorio inseguro: {}", dir.display()))?;
    let lock_path = dir.join("broker.lock");
    let lock = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(&lock_path)
        .map_err(|_| format!("Candado inaccesible: {}", lock_path.display()))?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => return Err("Ya hay un broker en ejecución".into()),
        Err(_) => return Err("No se pudo tomar el candado del broker".into()),
    }
    if std::os::unix::net::UnixStream::connect(path).is_ok() {
        return Err("Ya hay un broker en ejecución".into());
    }
    match fs::remove_file(path) {
        Ok(()) => eprintln!("broker: socket huérfano reemplazado"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(format!("No se pudo borrar {}", path.display())),
    }
    let listener =
        UnixListener::bind(path).map_err(|_| format!("No se pudo crear {}", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|_| "Permisos del socket")?;
    Ok((lock, listener))
}

async fn connection(stream: UnixStream, ctx: Arc<Ctx>) {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut buf = Vec::new();
    let first = tokio::time::timeout(ATTACH_TIMEOUT, read_line(&mut reader, &mut buf)).await;
    if !matches!(first, Ok(Ok(true))) {
        return;
    }
    let name = serde_json::from_slice::<Value>(&buf)
        .ok()
        .and_then(|v| v["attach"].as_str().map(str::to_owned));
    let joined = match name.as_deref() {
        Some(name) => attach(&ctx, name).await,
        None => Err("Se esperaba {\"attach\":\"<nombre>\"}".into()),
    };
    let (id, mut out, actor) = match joined {
        Ok(j) => j,
        Err(e) => {
            let _ = writer
                .write_all(format!("{}\n", json!({"error": e})).as_bytes())
                .await;
            return;
        }
    };
    if writer.write_all(b"{\"ok\":true}\n").await.is_err() {
        let _ = actor.send(Msg::Leave(id)).await;
        return;
    }
    // Termina cuando el actor suelta la cola (desconexión, upstream muerto, cierre).
    let mut write_task = tokio::spawn(async move {
        while let Some(mut line) = out.recv().await {
            line.push(b'\n');
            if writer.write_all(&line).await.is_err() {
                break;
            }
        }
        let _ = writer.shutdown().await;
    });
    let mut writer_done = false;
    loop {
        tokio::select! {
            read = read_line(&mut reader, &mut buf) => match read {
                Ok(true) if blank(&buf) => {}
                Ok(true) => {
                    if actor.send(Msg::Line(id, buf.clone())).await.is_err() {
                        break;
                    }
                }
                _ => break,
            },
            _ = &mut write_task => {
                writer_done = true;
                break;
            }
        }
    }
    let _ = actor.send(Msg::Leave(id)).await;
    if !writer_done
        && tokio::time::timeout(ATTACH_TIMEOUT, &mut write_task)
            .await
            .is_err()
    {
        write_task.abort();
    }
}

/// Registra al cliente en el actor de `name`. Si el actor se está cerrando, reintenta con
/// uno nuevo (como mucho tres veces).
async fn attach(
    ctx: &Ctx,
    name: &str,
) -> Result<(u32, mpsc::Receiver<Vec<u8>>, mpsc::Sender<Msg>)> {
    for _ in 0..3 {
        let spec = crate::cli::server_spec(&ctx.catalog, name)?;
        if !brokerable(&spec, name) {
            return Err(format!("Servidor no compartible: {name}"));
        }
        let tx = actor_for(ctx, name, &spec)?;
        let (reply, joined) = oneshot::channel::<Joined>();
        if tx.send(Msg::Join(reply)).await.is_err() {
            continue;
        }
        if let Ok(Some((id, out))) = joined.await {
            return Ok((id, out, tx));
        }
    }
    Err(format!("Servidor no disponible: {name}"))
}

/// Actor vivo de `name`, o uno nuevo con su upstream recién lanzado.
fn actor_for(ctx: &Ctx, name: &str, spec: &Value) -> Result<mpsc::Sender<Msg>> {
    let mut reg = ctx.registry.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(h) = reg.actors.get(name).filter(|h| !h.tx.is_closed()) {
        return Ok(h.tx.clone());
    }
    let up =
        upstream::spawn(spec).inspect_err(|e| eprintln!("broker: spawn {name} falló ({e})"))?;
    eprintln!("broker: spawn {name} pid {}", up.pid);
    let handle = actor::start(name, up, ctx.idle, ctx.stop.clone());
    let tx = handle.tx.clone();
    reg.retired.retain(|t| !t.is_finished());
    if let Some(old) = reg.actors.insert(name.into(), handle) {
        reg.retired.push(old.task);
    }
    Ok(tx)
}

//! `comandos ext broker`: escucha en el socket Unix, atiende cada línea `attach` y conecta
//! al cliente con el actor de su clave de compartición (lanzándolo si no existe).
use super::{
    actor::{Ctl, Shared},
    blank, check_private_dir, read_line,
    registry::{self, Attached, Ctx},
    socket_path,
};
use crate::Result;
use serde_json::json;
use std::{
    fs::{self, File, OpenOptions, TryLockError},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    signal::unix::{SignalKind, signal},
    sync::watch,
};

/// Plazo para que un cliente recién conectado envíe su línea `attach`.
const ATTACH_TIMEOUT: Duration = Duration::from_secs(5);
/// Margen para que los actores cierren sus upstreams (5 s de gracia + recogida).
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(8);

/// Corre el daemon hasta SIGTERM/SIGINT. Error si ya hay otro vivo o el socket no se crea.
pub async fn run(home: &Path, catalog: &Path) -> Result<()> {
    let mut term = signal(SignalKind::terminate()).map_err(|_| "Señales no disponibles")?;
    let mut int = signal(SignalKind::interrupt()).map_err(|_| "Señales no disponibles")?;
    raise_fd_limit();
    let path = socket_path();
    let (_lock, listener) = bind(&path)?;
    let idle = std::env::var("COMANDOS_BROKER_IDLE_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(600);
    let (stop_tx, stop) = watch::channel(false);
    let ctx = Arc::new(Ctx {
        catalog: catalog.into(),
        home: home.into(),
        shared: Shared {
            idle: Duration::from_secs(idle),
            stop,
            failures: Arc::default(),
        },
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
    let all = futures_util::future::join_all(registry::drain(&ctx));
    if tokio::time::timeout(SHUTDOWN_TIMEOUT, all).await.is_err() {
        eprintln!("broker: algunos upstreams no cerraron a tiempo");
    }
    eprintln!("broker: parado");
    Ok(())
}

/// Por debajo de esto el daemon avisa: cada sesión ocupa un socket y cada upstream tres
/// tuberías, así que 1024 (el blando por omisión) se agota con unas 300 sesiones.
const FD_WARN: u64 = 4096;

/// Sube el límite blando de descriptores al duro (la unidad pone `LimitNOFILE=65536`). Si
/// no se puede y queda bajo [`FD_WARN`], lo deja escrito en el registro.
fn raise_fd_limit() {
    use nix::sys::resource::{Resource, getrlimit, setrlimit};
    let Ok((soft, hard)) = getrlimit(Resource::RLIMIT_NOFILE) else {
        return;
    };
    let soft = if soft < hard && setrlimit(Resource::RLIMIT_NOFILE, hard, hard).is_ok() {
        hard
    } else {
        soft
    };
    if soft < FD_WARN {
        eprintln!("broker: aviso: solo {soft} descriptores abiertos permitidos (duro {hard})");
    }
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
    let Attached {
        id,
        mut out,
        ctl,
        lines,
    } = match registry::attach(&ctx, &buf).await {
        Ok(attached) => attached,
        Err(e) => {
            let reply = format!("{}\n", json!({"error": e}));
            let _ = writer.write_all(reply.as_bytes()).await;
            return;
        }
    };
    if writer.write_all(b"{\"ok\":true}\n").await.is_err() {
        let _ = ctl.send(Ctl::Leave(id)).await;
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
                    if lines.send((id, buf.clone())).await.is_err() {
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
    let _ = ctl.send(Ctl::Leave(id)).await;
    if !writer_done
        && tokio::time::timeout(ATTACH_TIMEOUT, &mut write_task)
            .await
            .is_err()
    {
        write_task.abort();
    }
}

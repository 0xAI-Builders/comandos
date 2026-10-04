//! Registro de actores por clave de compartición y validación de la línea `attach`.
use super::{
    Key,
    actor::{self, Ctl, Handle, JoinError, Joined, Shared},
    brokerable,
    mux::ClientId,
    upstream,
};
use crate::Result;
use serde_json::Value;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Mutex, PoisonError},
    time::Duration,
};
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
    time::Instant,
};

/// Espera máxima a que el actor dé el alta (incluye el `initialize` de un upstream nuevo).
pub(super) const JOIN_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Default)]
pub(super) struct Registry {
    actors: HashMap<Key, Handle>,
    /// Actores reemplazados mientras se cerraban; se esperan al parar el daemon.
    retired: Vec<JoinHandle<()>>,
}

pub(super) struct Ctx {
    /// Catálogo y HOME del daemon: un cliente con otros no se comparte.
    pub catalog: PathBuf,
    pub home: PathBuf,
    pub shared: Shared,
    /// Solo se bloquea sin `.await` dentro (lanzar un proceso es síncrono).
    pub registry: Mutex<Registry>,
}

pub(super) struct Attached {
    pub id: ClientId,
    pub out: mpsc::Receiver<Vec<u8>>,
    pub ctl: mpsc::Sender<Ctl>,
    pub lines: mpsc::Sender<(ClientId, Vec<u8>)>,
}

struct Request {
    name: String,
    cwd: PathBuf,
    env: Vec<(String, String)>,
    /// `PATH` del proceso `serve` que adjunta (opcional); no entra en la clave.
    path: Option<String>,
}

/// Valida `{"attach","cwd","env","catalog","home"[,"path"]}` y da de alta al cliente en el actor de
/// su clave. Si el actor se está cerrando, reintenta con uno nuevo (hasta tres veces).
pub(super) async fn attach(ctx: &Ctx, line: &[u8]) -> Result<Attached> {
    let req = parse(ctx, line)?;
    let spec = crate::cli::server_spec(&ctx.catalog, &req.name)?;
    if !brokerable(&spec, &req.name) {
        return Err(format!("Servidor no compartible: {}", req.name));
    }
    let key = Key::new(&req.name, &req.cwd, &spec, &req.env);
    for _ in 0..3 {
        let (ctl, lines) = actor_for(ctx, &key, &spec, &req)?;
        let (reply, joined) = oneshot::channel::<Joined>();
        if ctl.send(Ctl::Join(reply)).await.is_err() {
            continue;
        }
        // Al vencer, el receptor se suelta: el actor ve el alta huérfana y no la registra.
        match tokio::time::timeout(JOIN_TIMEOUT, joined).await {
            Err(_) => return Err("upstream sin respuesta a initialize en 5 s".into()),
            Ok(Ok(Ok((id, out)))) => {
                return Ok(Attached {
                    id,
                    out,
                    ctl,
                    lines,
                });
            }
            Ok(Ok(Err(JoinError::Failed(msg)))) => return Err(msg),
            Ok(Ok(Err(JoinError::Retry)) | Err(_)) => continue,
        }
    }
    Err(format!("Servidor no disponible: {}", req.name))
}

fn parse(ctx: &Ctx, line: &[u8]) -> Result<Request> {
    let v: Value = serde_json::from_slice(line).map_err(|_| "attach ilegible")?;
    let field = |k: &str| v.get(k).and_then(Value::as_str).ok_or("attach incompleto");
    let name = field("attach")?.to_owned();
    let cwd = PathBuf::from(field("cwd")?);
    if !cwd.is_absolute() || !cwd.is_dir() {
        return Err("cwd inválido".into());
    }
    if !same_path(Path::new(field("catalog")?), &ctx.catalog) {
        return Err("catálogo distinto".into());
    }
    if !same_path(Path::new(field("home")?), &ctx.home) {
        return Err("home distinto".into());
    }
    let env = (v
        .get("env")
        .and_then(Value::as_object)
        .ok_or("attach incompleto")?
        .iter())
    .map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_owned())))
    .collect::<Option<Vec<_>>>()
    .ok_or("env inválido")?;
    let path = match v.get("path") {
        None | Some(Value::Null) => None,
        Some(Value::String(p)) => Some(p.clone()),
        Some(_) => return Err("path inválido".into()),
    };
    Ok(Request {
        name,
        cwd,
        env,
        path,
    })
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

type Inbox = (mpsc::Sender<Ctl>, mpsc::Sender<(ClientId, Vec<u8>)>);

/// Actor vivo de la clave, o uno nuevo con su upstream recién lanzado. Durante el
/// enfriamiento tras un fallo de arranque devuelve el mismo error sin relanzar.
fn actor_for(ctx: &Ctx, key: &Key, spec: &Value, req: &Request) -> Result<Inbox> {
    let mut reg = ctx.registry.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(h) = reg.actors.get(key).filter(|h| !h.ctl.is_closed()) {
        return Ok((h.ctl.clone(), h.lines.clone()));
    }
    {
        let mut failures = (ctx.shared.failures.lock()).unwrap_or_else(PoisonError::into_inner);
        match failures.get(key) {
            Some((until, msg)) if *until > Instant::now() => return Err(msg.clone()),
            Some(_) => {
                failures.remove(key);
            }
            None => {}
        }
    }
    let up = upstream::spawn(spec, &req.cwd, &req.env, req.path.as_deref())
        .inspect_err(|e| eprintln!("broker: spawn {key} falló ({e})"))?;
    eprintln!("broker: spawn {key} pid {}", up.pid);
    let handle = actor::start(key.clone(), up, ctx.shared.clone());
    let inbox = (handle.ctl.clone(), handle.lines.clone());
    reg.retired.retain(|t| !t.is_finished());
    if let Some(old) = reg.actors.insert(key.clone(), handle) {
        reg.retired.push(old.task);
    }
    Ok(inbox)
}

/// Todas las tareas de actor, para esperarlas al parar el daemon.
pub(super) fn drain(ctx: &Ctx) -> Vec<JoinHandle<()>> {
    let mut reg = ctx.registry.lock().unwrap_or_else(PoisonError::into_inner);
    let mut tasks: Vec<_> = reg.actors.drain().map(|(_, h)| h.task).collect();
    tasks.append(&mut reg.retired);
    tasks
}

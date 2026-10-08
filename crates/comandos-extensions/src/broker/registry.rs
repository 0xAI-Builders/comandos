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
};
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
    time::Instant,
};

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

/// El fallo de un arranque compartido sigue bajo la autoridad del registro: todos sus
/// clientes deben esperar el mismo siguiente intento, incluso después de recoger el grupo.
pub(super) enum AttachError {
    /// Esta sesión no pertenece a este broker (identidad del catálogo o modo dedicado).
    Direct(String),
    /// Arranque fallido, cooldown o actor retirándose: reintentar attach al mismo registro.
    Retry(String),
}

struct Request {
    name: String,
    cwd: PathBuf,
    env: Vec<(String, String)>,
    /// v2 global: el catálogo/daemon es la autoridad de cuenta, entorno y directorio.
    global: bool,
    /// Entorno completo de clientes antiguos; sí forma parte de su identidad efectiva.
    environ: Option<Vec<(String, String)>>,
    /// `PATH` del protocolo anterior sin `environ`.
    path: Option<String>,
}

struct Execution {
    cwd: PathBuf,
    env: Vec<(String, String)>,
}

fn execution(ctx: &Ctx, req: &Request, spec: &Value) -> Result<Execution> {
    let (cwd, env) = if req.global {
        // La cuenta global no depende de qué cliente llegue primero, ni de su PATH/cwd.
        let cwd = match spec
            .get("cwd")
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
        {
            Some("~") => ctx.home.clone(),
            Some(value) if value.starts_with("~/") => ctx.home.join(&value[2..]),
            Some(value) => ctx.home.join(crate::expand_user(value)),
            None => ctx.home.clone(),
        };
        let env =
            upstream::effective_environment(&crate::spec_env(spec)?, upstream::Base::Path(None));
        (cwd, env)
    } else {
        let base = match &req.environ {
            Some(environ) => upstream::Base::Environ(environ),
            None => upstream::Base::Path(req.path.as_deref()),
        };
        (
            req.cwd.clone(),
            upstream::effective_environment(&req.env, base),
        )
    };
    if !cwd.is_absolute() || !cwd.is_dir() {
        return Err("cwd del servidor inválido".into());
    }
    Ok(Execution { cwd, env })
}

/// Valida `{"attach","cwd","env","catalog","home"[,"environ"][,"path"]}` y da de alta al cliente en el actor de
/// su clave. Si el actor se está cerrando, reintenta con uno nuevo (hasta tres veces).
pub(super) async fn attach(ctx: &Ctx, line: &[u8]) -> std::result::Result<Attached, AttachError> {
    let req = parse(ctx, line).map_err(AttachError::Direct)?;
    // Una lectura fallida no demuestra que la sesión sea ajena: el catálogo puede estar
    // actualizándose mientras el actor de su configuración anterior sigue sirviendo.
    let spec = crate::cli::server_spec(&ctx.catalog, &req.name).map_err(AttachError::Retry)?;
    if !brokerable(&spec, &req.name) {
        return Err(AttachError::Direct(format!(
            "Servidor no compartible: {}",
            req.name
        )));
    }
    let execution = execution(ctx, &req, &spec).map_err(AttachError::Retry)?;
    let key = Key::new(&req.name, &execution.cwd, &spec, &execution.env);
    for _ in 0..3 {
        let (ctl, lines) = actor_for(ctx, &key, &spec, &execution).map_err(AttachError::Retry)?;
        let (reply, joined) = oneshot::channel::<Joined>();
        if ctl.send(Ctl::Join(reply)).await.is_err() {
            continue;
        }
        // El actor posee el plazo de initialize y recoge su grupo antes de rechazar.
        // Abandonar aquí antes crearía un proxy directo junto al upstream aún arrancando.
        match joined.await {
            Ok(Ok((id, out))) => {
                return Ok(Attached {
                    id,
                    out,
                    ctl,
                    lines,
                });
            }
            Ok(Err(JoinError::Failed(msg))) => return Err(AttachError::Retry(msg)),
            Ok(Err(JoinError::Retry)) | Err(_) => continue,
        }
    }
    Err(AttachError::Retry(format!(
        "Servidor no disponible: {}",
        req.name
    )))
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
    let pairs = |o: &serde_json::Map<String, Value>| {
        (o.iter())
            .map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_owned())))
            .collect::<Option<Vec<_>>>()
    };
    let env = v
        .get("env")
        .and_then(Value::as_object)
        .ok_or("attach incompleto")?;
    let env = pairs(env).ok_or("env inválido")?;
    let environ = match v.get("environ") {
        None | Some(Value::Null) => None,
        Some(Value::Object(o)) => Some(pairs(o).ok_or("environ inválido")?),
        Some(_) => return Err("environ inválido".into()),
    };
    let path = match v.get("path") {
        None | Some(Value::Null) => None,
        Some(Value::String(p)) => Some(p.clone()),
        Some(_) => return Err("path inválido".into()),
    };
    let global = match v.get("global") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(global)) => *global,
        Some(_) => return Err("modo global inválido".into()),
    };
    Ok(Request {
        name,
        cwd,
        env,
        global,
        environ,
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
fn actor_for(ctx: &Ctx, key: &Key, spec: &Value, execution: &Execution) -> Result<Inbox> {
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
    let (upstream_spec, private_env) =
        crate::cli::broker_upstream_spec(&ctx.home, &ctx.catalog, &key.name, spec)?;
    let mut env = execution.env.clone();
    env.extend(private_env);
    let up = upstream::spawn(&upstream_spec, &execution.cwd, &env)
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

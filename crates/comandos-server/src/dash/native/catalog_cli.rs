//! Corte `services` (plan 2f-3, Tarea 4): catálogo de CLIs, cadenas de
//! comandos y modelos de OpenCode de `bin/cc-dash` (D8, `3f0a01a`).
//!
//! - GET `/commands/catalog` (prefijo): `cli_catalog_payload` (1533) con su
//!   caché `_CLI_CATALOG` (catálogo, derivado por `(checkedAt, mtime_ns de
//!   providers.json, ventana de 600 s sin snapshot)` y el sondeo de versiones
//!   de 600 s) más `_DETECT_CACHE` y `_HELP_CACHE`, todo bajo un candado
//!   asíncrono como el `_CLI_CATALOG_LOCK` del Python. `?refresh=1` fuerza un
//!   ciclo del vigilante de modelos: hasta la Tarea 6 declina antes de leer
//!   nada. `--version` (15 s) y `--help` (20 s, `NO_COLOR`, `TERM=dumb`,
//!   `COLUMNS=100`) se ejecutan como en el Python, uno tras otro.
//! - GET y POST `/chains` (igualdad cruda): `command_chains` en
//!   `$XDG_CONFIG_HOME/comandos/cadenas`. El POST corre en su propia tarea
//!   (`Native::tasks`): si el cliente se va, la escritura termina igual.
//! - GET `/opencode/models` (prefijo): stale-while-revalidate de 900 s; el
//!   refresco (`opencode models --verbose --refresh` por `sh -c`, 60 s) es una
//!   tarea registrada; en frío se espera como mucho 8 s.
//!
//! Ligereza: todo lo de disco (catálogo, snapshot, registro, cuentas, la
//! búsqueda de comandos en los binarios por trozos de 1 MiB) va al pool de
//! bloqueo en dos saltos por petición; en frío, uno más para resolver los
//! binarios del sondeo de versiones y otro por cada ayuda que interpretar.
//! Nada bloquea el hilo del runtime.
use super::{
    Answer, Entry, Fault, Key, Native, NativeRoute, Verb,
    files::{Strict, read_json_strict},
    light::{self, data, read_reply},
    procs::which_in,
    py::{is_pane, take_chars},
    query::Query,
    reply,
    states::{PyFloat, py_float},
    target,
    tmux::{RunError, run_program},
};
use crate::{HandlerError, Request};
use comandos_runtime::{
    Unsure, accounts,
    cli_catalog::{
        self, ViewError, ViewInputs, catalog_view, latest_models, names_in_file, native_binary,
        slash_names, validate_catalog, version_of,
    },
    cli_help::{FileKey, file_key, help_text, parse_help},
    command_chains::{self, SaveError},
    hooks::py as pyv,
    model_catalog::catalog_paths,
    providers::{self, RegistryCache},
};
use http::StatusCode;
use serde_json::{Map, Value, json};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ffi::OsString,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::watch;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliRoute {
    Catalog,
    ChainsGet,
    ChainsPost,
    OpencodeModels,
}

pub const ROUTES: &[Entry] = &[
    Entry {
        verb: Verb::Get,
        key: Key::Prefix("/opencode/models"),
        route: NativeRoute::Cli(CliRoute::OpencodeModels),
    },
    Entry {
        verb: Verb::Get,
        key: Key::Prefix("/commands/catalog"),
        route: NativeRoute::Cli(CliRoute::Catalog),
    },
    Entry {
        verb: Verb::Get,
        key: Key::Raw("/chains"),
        route: NativeRoute::Cli(CliRoute::ChainsGet),
    },
    Entry {
        verb: Verb::Post,
        key: Key::Raw("/chains"),
        route: NativeRoute::Cli(CliRoute::ChainsPost),
    },
];

/// `installed_versions`: `subprocess.run([exe, "--version"], timeout=15)`.
const VERSION_SECONDS: u64 = 15;
/// `help_for`: `subprocess.run([exe, "--help"], timeout=20)`.
const HELP_SECONDS: u64 = 20;
/// `_CLI_FALLBACK_TTL`.
const FALLBACK_TTL: f64 = 600.0;
/// `opencode_models`: caché de 900 s, espera en frío de 8 s, plazo de 60 s.
const OPENCODE_FRESH: f64 = 900.0;
const OPENCODE_WAIT: Duration = Duration::from_secs(8);
const OPENCODE_SECONDS: u64 = 60;

/// Estado de proceso de estas rutas (`_CLI_CATALOG`, `_DETECT_CACHE`,
/// `_HELP_CACHE`, `_oc_models_cache`): vive en `Native`, no en estáticos, así
/// cada frente (y cada prueba) tiene el suyo.
#[derive(Default)]
pub struct CliState {
    catalog: tokio::sync::Mutex<CatalogCache>,
    opencode: Mutex<OpencodeCache>,
}

/// La llave de `derived`: `(checkedAt, mtime_ns, ventana)`.
#[derive(Clone)]
struct DerivedKey {
    checked: Option<Value>,
    stamp: Option<i128>,
    window: Option<i64>,
}

impl DerivedKey {
    /// Igualdad de tuplas de Python (`checkedAt` con `==`: `1 == 1.0`).
    fn same(&self, other: &DerivedKey) -> bool {
        let checked = match (&self.checked, &other.checked) {
            (None, None) => true,
            (Some(a), Some(b)) => pyv::eq(a, b),
            (Some(v), None) | (None, Some(v)) => v.is_null(),
        };
        checked && self.stamp == other.stamp && self.window == other.window
    }
}

#[derive(Clone)]
struct Derived {
    key: DerivedKey,
    versions: Map<String, Value>,
    at: f64,
    new_models: Map<String, Value>,
    models: Map<String, Value>,
}

#[derive(Default)]
struct CatalogCache {
    catalog: Option<Arc<Value>>,
    derived: Option<Derived>,
    /// El sondeo de versiones y su hora; `None` si leyó algo que el port no
    /// reproduce: la ruta declina sin sondear hasta que venza (`FALLBACK_TTL`).
    fallback: Option<(Option<Map<String, Value>>, f64)>,
    detect: HashMap<String, (FileKey, HashSet<String>)>,
    /// `_HELP_CACHE`; `None` es una ayuda que el port no interpreta con
    /// certeza: con la misma llave se declina sin volver a correr `--help`.
    help: HashMap<PathBuf, (FileKey, Option<Map<String, Value>>)>,
}

#[derive(Default)]
struct OpencodeCache {
    at: f64,
    data: Vec<Value>,
    refresh: Option<watch::Receiver<bool>>,
    /// Cuándo un refresco leyó por última vez algo que el port no reproduce
    /// con certeza (lo que el Python tiene en su caché es desconocido). Hasta
    /// que pase `OPENCODE_FRESH` la ruta declina sin lanzar nada; después se
    /// sondea una vez, como en frío.
    unsure_at: Option<f64>,
}

pub async fn answer(native: &Arc<Native>, route: CliRoute, request: &Request) -> Answer {
    match route {
        CliRoute::Catalog => catalog(native, request).await,
        CliRoute::ChainsGet => chains_get(native).await,
        CliRoute::ChainsPost => {
            let data = data(request)?.clone();
            let job = native
                .tasks()
                .spawn_handle({
                    let native = Arc::clone(native);
                    async move { chains_post(&native, &data).await }
                })
                .map_err(|_| failure())?;
            job.await.map_err(|_| failure())?
        }
        CliRoute::OpencodeModels => opencode_models(native).await,
    }
}

fn failure() -> Fault {
    Fault::Error(HandlerError::Failure)
}

fn view_fault(error: ViewError) -> Fault {
    match error {
        ViewError::Raises => failure(),
        ViewError::Unsure => Fault::Decline,
    }
}

fn unsure(_: Unsure) -> Fault {
    Fault::Decline
}

/// Un trabajo de disco en el pool de bloqueo; su pánico es una excepción.
async fn blocking<T: Send + 'static>(
    job: impl FnOnce() -> Result<T, Fault> + Send + 'static,
) -> Result<T, Fault> {
    tokio::task::spawn_blocking(job)
        .await
        .map_err(|_| failure())?
}

// ---------------------------------------------------------------------------
// GET /commands/catalog
// ---------------------------------------------------------------------------

async fn catalog(native: &Arc<Native>, request: &Request) -> Answer {
    let query = Query::parse(&request.target)?;
    // `refresh=1` fuerza `_force_model_watch_cycle` (Tarea 6).
    if query.first("refresh") == Some("1") {
        return Err(Fault::Decline);
    }
    let session = take_chars(query.first("session").unwrap_or(""), 80);
    let pane = query.first("pane").unwrap_or("").to_owned();
    let info = match is_pane(&pane) {
        None => return Err(Fault::Decline),
        Some(true) => target::agent_info_for_pane(native, &pane).await?,
        Some(false) => None,
    };
    let (view, at) = payload(native).await?;
    let at = serde_json::Number::from_f64(at).ok_or(Fault::Decline)?;
    read_reply(&json!({
        "cliInPane": info.map(|i| i.agent).unwrap_or_default(),
        "target": {"session": session, "pane": pane},
        "catalog": view,
        "versionsAt": Value::Number(at),
    }))
}

/// `load_catalog()`: `json.load` de `config/cli-commands.json` y su validación.
fn load_catalog(repo: &Path) -> Result<Value, Fault> {
    match read_json_strict(&repo.join("config/cli-commands.json")) {
        Strict::Value(value) => {
            validate_catalog(&value).map_err(view_fault)?;
            Ok(value)
        }
        Strict::Missing | Strict::Unreadable => Err(failure()),
        Strict::Unsure => Err(Fault::Decline),
    }
}

/// `os.stat(path).st_mtime_ns`.
fn mtime_ns(path: &Path) -> Option<i128> {
    let meta = std::fs::metadata(path).ok()?;
    Some(i128::from(meta.mtime()) * 1_000_000_000 + i128::from(meta.mtime_nsec()))
}

/// Lo que el pool de bloqueo lee al empezar.
struct Disk {
    catalog: Arc<Value>,
    snap: Map<String, Value>,
    stamp: Option<i128>,
    registry: Value,
}

fn read_disk(
    native: &Native,
    catalog: Option<Arc<Value>>,
) -> impl FnOnce() -> Result<Disk, Fault> + use<> {
    let opts = native.options();
    let hooks = opts.hooks.clone();
    let repo = opts.repo_root.clone();
    let paths = catalog_paths(
        &opts.home,
        &opts.cwd,
        opts.codex_home.as_deref(),
        opts.grok_home.as_deref(),
    );
    move || {
        let repo = repo.ok_or(Fault::Decline)?;
        // `_read_json_quiet(MODEL_WATCH_FILE) or {}`: algo que no es objeto y
        // es verdad revienta en `snap.get` (500).
        let snap = match light::load(&hooks.join("model-watch.json"))? {
            Some(v) if pyv::truthy(&v) => match v {
                Value::Object(map) => map,
                _ => return Err(failure()),
            },
            _ => Map::new(),
        };
        let providers_file = repo.join("config/providers.json");
        let stamp = mtime_ns(&providers_file);
        let catalog = match catalog {
            Some(catalog) => catalog,
            None => Arc::new(load_catalog(&repo)?),
        };
        // `load_provider_registry()`: un registro que el port no reproduce
        // (o que el Python rechaza: 500) declina.
        let registry = RegistryCache::default()
            .load(&providers_file, &paths)
            .map_err(unsure)?;
        Ok(Disk {
            catalog,
            snap,
            stamp,
            registry,
        })
    }
}

/// `_registry_model_ids()`: ids de cada motor sin los `soon`. Un id lista u
/// objeto revienta el `set` del Python (`TypeError: unhashable`, 500 después
/// del sondeo de versiones): se declina, y quien llama lo hace antes de
/// sondear. Un escalar que no es texto entra en el `set` y `latest_models` lo
/// salta: aquí ni entra.
fn registry_model_ids(registry: &Value) -> Result<BTreeMap<String, Vec<String>>, Fault> {
    let mut out = BTreeMap::new();
    let motors = match registry.get("motors") {
        Some(v) if pyv::truthy(v) => v.as_object().ok_or(Fault::Decline)?,
        _ => return Ok(out),
    };
    for (motor, spec) in motors {
        let mut ids = Vec::new();
        let models = match spec.as_object().ok_or(Fault::Decline)?.get("models") {
            Some(v) if pyv::truthy(v) => v.as_array().ok_or(Fault::Decline)?.clone(),
            _ => Vec::new(),
        };
        for model in models {
            let model = model.as_object().ok_or(Fault::Decline)?;
            if model.get("soon").is_some_and(pyv::truthy) {
                continue;
            }
            match model.get("id") {
                None => ids.push(String::new()),
                Some(Value::String(id)) => ids.push(id.clone()),
                Some(Value::Array(_) | Value::Object(_)) => return Err(Fault::Decline),
                Some(_) => {}
            }
        }
        out.insert(motor.clone(), ids);
    }
    Ok(out)
}

/// `providers.which(binary)` del frente para cada CLI del catálogo.
fn which_all(
    native: &Native,
    binaries: Vec<String>,
) -> impl FnOnce() -> Result<Vec<Option<PathBuf>>, Fault> + use<> {
    let opts = native.options();
    let (search, home, dirs) = (
        opts.search_path.clone(),
        opts.home.clone(),
        opts.user_bin_dirs.clone(),
    );
    move || {
        Ok(binaries
            .iter()
            .map(|b| providers::which_in_dirs(b, search.as_deref(), &home, &dirs))
            .collect())
    }
}

/// `(id, binary)` de cada CLI de un catálogo validado.
fn cli_ids(catalog: &Value) -> Result<Vec<(String, String)>, Fault> {
    cli_catalog::clis(catalog)
        .map_err(view_fault)?
        .iter()
        .map(|cli| {
            Ok((
                cli_catalog::cli_field(cli, "id")
                    .map_err(view_fault)?
                    .to_owned(),
                cli_catalog::cli_field(cli, "binary")
                    .map_err(view_fault)?
                    .to_owned(),
            ))
        })
        .collect()
}

/// `installed_versions(catalog)`: `--version` de cada CLI encontrado;
/// `Ok(None)` en cuanto una salida no se reproduce con certeza.
async fn installed_versions(
    native: &Native,
    clis: &[(String, String)],
    exes: &[Option<PathBuf>],
) -> Result<Option<Map<String, Value>>, Fault> {
    let mut out = Map::new();
    for ((id, _), exe) in clis.iter().zip(exes) {
        let Some(exe) = exe else {
            out.insert(id.clone(), Value::Null);
            continue;
        };
        let program = native.options().program(exe);
        let version = match run_program(
            &program,
            &["--version"],
            Duration::from_secs(VERSION_SECONDS),
        )
        .await
        {
            Ok(output) => match version_of(&format!("{}{}", output.stdout, output.stderr)) {
                Ok(version) => version,
                Err(Unsure) => return Ok(None),
            },
            Err(RunError::Timeout | RunError::Spawn(_)) => "?".to_owned(),
            // `UnicodeDecodeError` no lo captura `installed_versions`.
            Err(RunError::Decode) => return Err(failure()),
        };
        out.insert(id.clone(), Value::from(version));
    }
    Ok(Some(out))
}

/// `float(snap.get("checkedAt") or 0.0)`.
fn checked_at(snap: &Map<String, Value>) -> Result<f64, Fault> {
    match snap.get("checkedAt").filter(|v| pyv::truthy(v)) {
        None => Ok(0.0),
        Some(v) => match py_float(v) {
            PyFloat::Value(x) => Ok(x),
            PyFloat::Raises => Err(failure()),
            PyFloat::Unsure => Err(Fault::Decline),
        },
    }
}

/// `new_models` del snapshot (`newSince`).
fn new_models(snap: &Map<String, Value>) -> Result<Map<String, Value>, Fault> {
    let mut out = Map::new();
    match snap.get("newSince") {
        Some(Value::Object(map)) => {
            for (cli, v) in map {
                if let Value::Object(v) = v {
                    let models = cli_catalog::py_list(v.get("models")).map_err(view_fault)?;
                    out.insert(cli.clone(), Value::Array(models));
                }
            }
        }
        Some(v) if pyv::truthy(v) => return Err(failure()),
        _ => {}
    }
    Ok(out)
}

/// `cli_catalog_payload(refresh=False)`: la vista y `versionsAt`.
async fn payload(native: &Arc<Native>) -> Result<(Value, f64), Fault> {
    let opts = native.options();
    let mut cache = native.cli.catalog.lock().await;
    let disk = blocking(read_disk(native, cache.catalog.clone())).await?;
    cache.catalog = Some(Arc::clone(&disk.catalog));
    let catalog = Arc::clone(&disk.catalog);
    let clis = cli_ids(&catalog)?;
    let snap = &disk.snap;
    let now = (opts.clock_seconds)();
    let no_snap = !snap.get("versions").is_some_and(Value::is_object);
    let window = if no_snap {
        let w = (now / FALLBACK_TTL).floor();
        Some(if w.is_finite() {
            w as i64
        } else {
            return Err(Fault::Decline);
        })
    } else {
        None
    };
    let key = DerivedKey {
        checked: snap.get("checkedAt").cloned(),
        stamp: disk.stamp,
        window,
    };
    let fresh = cache.derived.as_ref().is_some_and(|d| d.key.same(&key));
    if !fresh {
        // Lo que el Python haría reventar después del sondeo, antes de él.
        let ids = registry_model_ids(&disk.registry)?;
        let mut at = checked_at(snap)?;
        let versions = match snap.get("versions") {
            Some(Value::Object(v)) => v.clone(),
            _ => {
                let reuse = cache
                    .fallback
                    .as_ref()
                    .filter(|(_, t)| (opts.clock_seconds)() - t <= FALLBACK_TTL)
                    .cloned();
                let (versions, t) = match reuse {
                    Some(fb) => fb,
                    None => {
                        let binaries = clis.iter().map(|(_, b)| b.clone()).collect();
                        let exes = blocking(which_all(native, binaries)).await?;
                        let versions = installed_versions(native, &clis, &exes).await?;
                        let fb = (versions, (opts.clock_seconds)());
                        cache.fallback = Some(fb.clone());
                        fb
                    }
                };
                // Un sondeo incierto (de ahora o recordado): sin efectos.
                let versions = versions.ok_or(Fault::Decline)?;
                at = t;
                versions
            }
        };
        let new_models = new_models(snap)?;
        let models = latest_models(snap.get("discovered"), &ids).map_err(view_fault)?;
        cache.derived = Some(Derived {
            key,
            versions,
            at,
            new_models,
            models,
        });
    }
    let derived = cache.derived.clone().ok_or_else(failure)?;
    // Cuentas, binarios nativos y comandos presentes: un salto al pool.
    let registry = disk.registry.clone();
    let (home, cwd) = (opts.home.clone(), opts.cwd.clone());
    let binaries: Vec<String> = clis.iter().map(|(_, b)| b.clone()).collect();
    let resolve = which_all(native, binaries);
    let mut detect = std::mem::take(&mut cache.detect);
    let catalog_for_job = Arc::clone(&catalog);
    let clis_for_job = clis.clone();
    type Scan = (
        HashMap<String, Vec<Value>>,
        Vec<(Option<PathBuf>, Option<FileKey>)>,
        Result<HashMap<String, Option<HashSet<String>>>, Fault>,
        HashMap<String, (FileKey, HashSet<String>)>,
    );
    let (accounts, exes, detected, detect) = blocking(move || -> Result<Scan, Fault> {
        let exes = resolve()?;
        let accounts = account_envs(&registry, &clis_for_job, &home, &cwd)?;
        let detected = detected_commands(&catalog_for_job, &clis_for_job, &exes, &mut detect);
        // La llave de `_HELP_CACHE` de cada ejecutable (`OSError` → `None`).
        let keyed = exes
            .into_iter()
            .map(|exe| {
                let key = exe.as_deref().and_then(file_key);
                (exe, key)
            })
            .collect();
        Ok((accounts, keyed, detected, detect))
    })
    .await?;
    cache.detect = detect;
    // `except Exception: detected = None`; lo incierto declina.
    let detected = match detected {
        Ok(found) => Some(found),
        Err(Fault::Decline) => return Err(Fault::Decline),
        Err(_) => None,
    };
    let mut helps: HashMap<String, Map<String, Value>> = HashMap::new();
    for ((id, binary), (exe, key)) in clis.iter().zip(exes) {
        let (Some(exe), Some(key)) = (exe, key) else {
            continue;
        };
        if let Some(help) = help_for(native, &mut cache, binary, &exe, key).await? {
            helps.insert(id.clone(), help);
        }
    }
    let view = catalog_view(
        &catalog,
        &ViewInputs {
            versions: &derived.versions,
            accounts: &accounts,
            models: &derived.models,
            new_models: &derived.new_models,
            detected: detected.as_ref(),
            helps: &helps,
        },
    )
    .map_err(view_fault)?;
    Ok((view, derived.at))
}

/// Las cuentas que no son `main` de cada CLI con su entorno; un error en
/// cualquiera deja la lista de ese CLI vacía (`except Exception: []`).
fn account_envs(
    registry: &Value,
    clis: &[(String, String)],
    home: &Path,
    cwd: &Path,
) -> Result<HashMap<String, Vec<Value>>, Fault> {
    let paths = accounts::Paths::new(home, cwd);
    let mut out = HashMap::new();
    for (id, _) in clis {
        let mut envs = Vec::new();
        let listed = match accounts::list_accounts(registry, id, &paths) {
            Ok(listed) => listed,
            Err(_) => {
                out.insert(id.clone(), envs);
                continue;
            }
        };
        let mut failed = false;
        for account in listed {
            let alias = account.get("alias").cloned().unwrap_or(Value::Null);
            if alias.as_str() == Some("main") {
                continue;
            }
            match accounts::account_environment(registry, id, &alias, &paths) {
                Ok(env) => envs.push(env),
                // El Python la escribiría con sustitutos: no se reproduce.
                Err(e) if e.kind() == accounts::ErrorKind::NonUtf8Home => {
                    return Err(Fault::Decline);
                }
                Err(_) => {
                    failed = true;
                    break;
                }
            }
        }
        out.insert(id.clone(), if failed { Vec::new() } else { envs });
    }
    Ok(out)
}

/// `detected_commands(catalog)` con `_DETECT_CACHE`: `Err(Decline)` si no se
/// sabe; otro `Err` es la excepción que el Python captura (`None`).
fn detected_commands(
    catalog: &Value,
    clis: &[(String, String)],
    exes: &[Option<PathBuf>],
    cache: &mut HashMap<String, (FileKey, HashSet<String>)>,
) -> Result<HashMap<String, Option<HashSet<String>>>, Fault> {
    let entries = cli_catalog::clis(catalog).map_err(view_fault)?;
    let mut out = HashMap::new();
    for (((id, _), exe), cli) in clis.iter().zip(exes).zip(entries) {
        let path = match exe {
            Some(exe) => native_binary(exe).map_err(unsure)?,
            None => None,
        };
        let Some(path) = path else {
            out.insert(id.clone(), None);
            continue;
        };
        let Ok(meta) = std::fs::metadata(&path) else {
            out.insert(id.clone(), None);
            continue;
        };
        let key: FileKey = (
            path.clone(),
            i128::from(meta.mtime()) * 1_000_000_000 + i128::from(meta.mtime_nsec()),
            meta.size(),
        );
        if let Some((cached, found)) = cache.get(id)
            && *cached == key
        {
            out.insert(id.clone(), Some(found.clone()));
            continue;
        }
        let names = slash_names(cli).map_err(view_fault)?;
        match names_in_file(&path, &names) {
            Ok(found) => {
                cache.insert(id.clone(), (key, found.clone()));
                out.insert(id.clone(), Some(found));
            }
            Err(_) => {
                out.insert(id.clone(), None);
            }
        }
    }
    Ok(out)
}

/// `help_for(binary)` con `_HELP_CACHE` (por la ruta de `which`, validada por
/// `(realpath, mtime_ns, tamaño)`, ya leída); las excepciones son `None`.
async fn help_for(
    native: &Native,
    cache: &mut CatalogCache,
    binary: &str,
    exe: &Path,
    key: FileKey,
) -> Result<Option<Map<String, Value>>, Fault> {
    if let Some((cached, parsed)) = cache.help.get(exe)
        && *cached == key
    {
        // Una ayuda incierta recordada declina sin volver a pedirla.
        return parsed.clone().map(Some).ok_or(Fault::Decline);
    }
    let mut program = native.options().program(exe);
    for (name, value) in [("NO_COLOR", "1"), ("TERM", "dumb"), ("COLUMNS", "100")] {
        program
            .env
            .push((OsString::from(name), OsString::from(value)));
    }
    let output = match run_program(&program, &["--help"], Duration::from_secs(HELP_SECONDS)).await {
        Ok(output) => output,
        Err(_) => return Ok(None),
    };
    let Some(text) = help_text(&output.stdout, &output.stderr) else {
        return Ok(None);
    };
    let name = binary.to_owned();
    let parsed = blocking(move || Ok(parse_help(&text, &name).ok())).await?;
    let parsed = parsed.map(|mut parsed| {
        parsed.insert("command".into(), Value::from(format!("{binary} --help")));
        parsed
    });
    cache.help.insert(exe.to_path_buf(), (key, parsed.clone()));
    parsed.map(Some).ok_or(Fault::Decline)
}

// ---------------------------------------------------------------------------
// Cadenas
// ---------------------------------------------------------------------------

/// `command_chains.default_dir()`: `XDG_CONFIG_HOME` del entorno de los hijos
/// del frente (el confinado en las pruebas) o el del proceso.
fn chains_dir(native: &Native) -> PathBuf {
    let opts = native.options();
    let xdg = match &opts.child_env {
        Some(env) => env
            .iter()
            .rev()
            .find(|(k, _)| k == "XDG_CONFIG_HOME")
            .map(|(_, v)| v.clone()),
        None => std::env::var_os("XDG_CONFIG_HOME"),
    };
    command_chains::default_dir(xdg.as_deref(), &opts.home)
}

async fn chains_get(native: &Native) -> Answer {
    let dir = chains_dir(native);
    let chains = blocking(move || command_chains::list_chains(&dir).map_err(unsure)).await?;
    read_reply(&json!({"chains": chains}))
}

async fn chains_post(native: &Native, data: &Map<String, Value>) -> Answer {
    let dir = chains_dir(native);
    let (name, steps, slug) = (
        data.get("name").cloned(),
        data.get("steps").cloned(),
        data.get("slug").cloned(),
    );
    let outcome = blocking(move || {
        command_chains::save_chain(&dir, name.as_ref(), steps.as_ref(), slug.as_ref())
            .map_err(unsure)
    })
    .await?;
    match outcome {
        Ok(chain) => reply(StatusCode::OK, &json!({"ok": true, "chain": chain})),
        Err(SaveError::Chain(message)) => {
            reply(StatusCode::BAD_REQUEST, &json!({"error": message}))
        }
        Err(SaveError::Os(class)) => reply(
            StatusCode::INTERNAL_SERVER_ERROR,
            &json!({"error": format!("No se pudo guardar la cadena: {class}")}),
        ),
    }
}

// ---------------------------------------------------------------------------
// GET /opencode/models
// ---------------------------------------------------------------------------

fn models_reply(data: Vec<Value>) -> Answer {
    read_reply(&json!({"providers": data}))
}

async fn opencode_models(native: &Arc<Native>) -> Answer {
    let now = (native.options().clock_seconds)();
    let (data, mut done) = {
        let mut cache = native.cli.opencode.lock().map_err(|_| failure())?;
        // Lo último leído no se reproduce: se declina antes de cualquier
        // efecto (el refresco reescribe la caché de proveedores de opencode).
        if cache.unsure_at.is_some_and(|t| now - t < OPENCODE_FRESH) {
            return Err(Fault::Decline);
        }
        // Vencida la incertidumbre, los datos viejos no son los del Python:
        // se espera el sondeo como en frío.
        let data = if cache.unsure_at.is_some() {
            Vec::new()
        } else {
            cache.data.clone()
        };
        if !data.is_empty() && now - cache.at < OPENCODE_FRESH {
            return models_reply(data);
        }
        let done = match &cache.refresh {
            Some(done) => done.clone(),
            None => {
                let (tx, rx) = watch::channel(false);
                native
                    .tasks()
                    .spawn(refresh_opencode(Arc::clone(native), tx))
                    .map_err(|_| failure())?;
                cache.refresh = Some(rx.clone());
                rx
            }
        };
        (data, done)
    };
    if !data.is_empty() {
        return models_reply(data);
    }
    let _ = tokio::time::timeout(OPENCODE_WAIT, done.wait_for(|finished| *finished)).await;
    let cache = native.cli.opencode.lock().map_err(|_| failure())?;
    if cache.unsure_at.is_some() {
        return Err(Fault::Decline);
    }
    models_reply(cache.data.clone())
}

/// `_opencode_models_refresh`: solo una lista no vacía se guarda. Lo incierto
/// queda fechado; una lista vacía tras lo incierto no lo aclara (el Python
/// conserva lo que tenía) y vuelve a fechar la incertidumbre.
async fn refresh_opencode(native: Arc<Native>, done: watch::Sender<bool>) {
    let result = fetch_opencode(&native).await;
    if let Ok(mut cache) = native.cli.opencode.lock() {
        let now = (native.options().clock_seconds)();
        match result {
            Ok(data) if !data.is_empty() => {
                cache.at = now;
                cache.data = data;
                cache.unsure_at = None;
            }
            Ok(_) => {
                if cache.unsure_at.is_some() {
                    cache.unsure_at = Some(now);
                }
            }
            Err(Unsure) => cache.unsure_at = Some(now),
        }
        cache.refresh = None;
    }
    let _ = done.send(true);
}

/// `_opencode_models_fetch()`: cualquier excepción es `[]`.
async fn fetch_opencode(native: &Native) -> Result<Vec<Value>, Unsure> {
    let opts = native.options();
    let binary = which_in(opts.search_path.as_deref(), "opencode")
        .unwrap_or_else(|| opts.home.join(".opencode/bin/opencode"));
    let binary = binary.to_str().ok_or(Unsure)?.to_owned();
    let Some(sh) = which_in(opts.search_path.as_deref(), "sh") else {
        return Ok(Vec::new());
    };
    let script = format!(
        "set -a; . \"$HOME/.claude/hooks/providers.env\" 2>/dev/null; set +a; \"{binary}\" models --verbose --refresh"
    );
    let output = match run_program(
        &opts.program(sh),
        &["-c", &script],
        Duration::from_secs(OPENCODE_SECONDS),
    )
    .await
    {
        Ok(output) => output,
        Err(_) => return Ok(Vec::new()),
    };
    tokio::task::spawn_blocking(move || cli_catalog::parse_opencode_models(&output.stdout))
        .await
        .unwrap_or(Ok(Vec::new()))
}

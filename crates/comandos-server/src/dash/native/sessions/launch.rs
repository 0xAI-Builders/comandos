//! POST `/session-new` (9333) y POST `/account/add` (`account_add_request`,
//! 2749, árbol vivo D8) de `bin/cc-dash`: una sesión `term-r<n>` nueva con su
//! pestaña y el comando del agente (o del login de la cuenta) tecleado 1,5 s
//! después, como el hilo del Python.
//!
//! Las dos van antes de `resolve_project_session` en el `do_POST` y no usan
//! scope (`tmux new-session` directo, como el Python): no dependen de
//! `systemd-run`. Para no arrancar un servidor tmux en el cgroup del frente
//! (moriría con él, R2 de la 2d), declinan antes de nada si no hay servidor.
//!
//! Todo lo incierto se decide antes del primer efecto (ruling 2): la ruta y
//! las cuentas, el entorno de la cuenta, el comando entero (también los 400 de
//! Grok y el 409 del entorno, que el Python da tras crear la sesión: aquí se
//! reproducen igual, matando solo la sesión recién creada), los archivos del
//! registro de pestañas, los `.claude.json` de la herencia de confianza y los
//! `settings.json` de la siembra. Lo que este port no reproduce declina antes:
//! un `profileId` (los perfiles de lanzamiento siguen en el heredado), los
//! harnesses `acp`, `opencode` y `agy` (su comando lo porta 2f-2/T1), un
//! carril de uso apagado cuando hay que registrar la configuración y un
//! `.worktreeinclude` con patrones que no son `*`/`?` de un solo nivel.
//!
//! Efectos en vivo (los del Python, en su orden): `/session-new` — worktree
//! (`git worktree add` en `<cwd>/.claude/worktrees/wt-<n>` y copia de los
//! archivos de `.worktreeinclude`) si la carpeta ya tiene un agente vivo y
//! `AUTO_WORKTREE` no es `0`; `tmux new-session -d -s term-r<n> -c <cwd>`; la
//! pestaña (`register_app_tab`); `kill-session -t =term-r<n>` solo de esa
//! sesión si el entorno de la cuenta o el modelo de Grok fallan; la fila de
//! `usage_session_configs`; la herencia de confianza de Claude (`.claude.json`
//! del HOME y de la cuenta, y su fila en `usage_changes`); el `send-keys`
//! diferido. `/account/add` — `os.makedirs(<cuenta>, 0o700)`, el
//! `config.toml` de Codex (creación exclusiva, 0600), la siembra del
//! `settings.json` de Claude bajo su candado, la sesión, la pestaña y el
//! `send-keys` del login. Todo bajo `ACCOUNT_ADD_LOCK` (`_ACCOUNT_ADD_LOCK`).
//!
//! Pendiente para Jesús (no se corrige aquí, se porta tal cual): el alias de
//! una cuenta Claude se valida pero `claude-as` crea carpetas de cuenta falsas
//! (p. ej. `--dangerously-skip-permissions`) que luego aparecen en el menú.
use super::{
    super::{
        files::{self, FileLock, LOCK_WAIT, Strict},
        procs, py, target, usage,
    },
    Fault, Native, NativeOptions, StatusCode, blocking, conf_get, failure, home_text, is_dir,
    mark_effect, mutate, read_conf, registry_preflight, reply, tab_registry, tmux,
};
use crate::dash::native::{Answer, states::gather, tmux::run_program};
use comandos_core::json::{indent_dumps, python_eq, truthy};
use comandos_core::text::shlex_quote;
use comandos_runtime::{
    accounts, claude_trust, launch_command,
    providers::{grok_models, harness_has_accounts, proxy_port, validate_selection},
};
use serde_json::{Map, Value, json};
use std::{
    fs, io,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::Duration,
};

/// `time.sleep(1.5)` del hilo que teclea.
const TYPE_DELAY: Duration = Duration::from_millis(1500);

/// `timeout=8` de `git rev-parse` y `timeout=30` de `git worktree add`.
const GIT_PROBE: Duration = Duration::from_secs(8);
const GIT_WORKTREE: Duration = Duration::from_secs(30);

/// `_ACCOUNT_ADD_LOCK`: toda la petición de `/account/add`, hasta lanzar el
/// tecleo. (El heredado tiene el suyo: un `Decline` que reenvía no lo comparte.)
static ACCOUNT_ADD_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// El prefijo de `env -u …` del login de una cuenta (árbol vivo D8).
const LOGIN_ENV: &str = "env -u CLAUDECODE -u CLAUDE_CONFIG_DIR -u CODEX_HOME -u GROK_HOME -u ANTHROPIC_API_KEY -u ANTHROPIC_AUTH_TOKEN -u ANTHROPIC_BASE_URL -u OPENAI_API_KEY -u CODEX_API_KEY ";

fn decline<T>(_: T) -> Fault {
    Fault::Decline
}

/// `str(value)` de un escalar que el port reproduce; lo demás declina.
fn str_of(value: &Value) -> Result<String, Fault> {
    py::str_scalar(value).ok_or(Fault::Decline)
}

/// `str(a or b or … or fallback)`.
fn str_or(values: &[Option<&Value>], fallback: &str) -> Result<String, Fault> {
    match values.iter().flatten().find(|v| truthy(v)) {
        Some(value) => str_of(value),
        None => Ok(fallback.to_owned()),
    }
}

/// `time.time()`.
fn now(opts: &NativeOptions) -> f64 {
    (opts.clock_seconds)()
}

/// `os.path.basename(path)`.
fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// `" ".join(f"{k}={shlex.quote(v)}" for k, v in env.items())`.
fn env_words(env: &Value) -> Result<String, Fault> {
    let Value::Object(env) = env else {
        return Err(Fault::Decline);
    };
    let mut words = Vec::with_capacity(env.len());
    for (key, value) in env {
        let value = value.as_str().ok_or(Fault::Decline)?;
        words.push(format!("{key}={}", shlex_quote(value)));
    }
    Ok(words.join(" "))
}

/// `accounts::Paths` del frente.
fn paths(opts: &NativeOptions) -> accounts::Paths {
    accounts::Paths::new(&opts.home, &opts.cwd)
}

/// `load_provider_registry()` en un hilo de bloqueo; cualquier fallo declina.
async fn registry(native: &Native) -> Result<Value, Fault> {
    let (opts, cache) = (native.options().clone(), native.registry.clone());
    blocking(move || gather::load_registry(&opts, &cache).map_err(decline)).await
}

/// ¿Hay servidor tmux? Sin él, `new-session` lo arrancaría en el cgroup del
/// frente: se declina antes de nada (el heredado hace lo de siempre).
async fn server_running(opts: &NativeOptions) -> Result<(), Fault> {
    let out = tmux(opts, &["list-sessions", "-F", "#{session_name}"]).await?;
    if out.ok { Ok(()) } else { Err(Fault::Decline) }
}

/// `term-r<n>` libre empezando en `n` (`has-session` hasta que falle).
async fn free_session(opts: &NativeOptions, mut n: i64) -> Result<String, Fault> {
    loop {
        let name = format!("term-r{n}");
        if !tmux(opts, &["has-session", "-t", &format!("={name}")])
            .await?
            .ok
        {
            return Ok(name);
        }
        n += 1;
    }
}

/// `500 {"error": (r.stderr or "tmux fallo").strip()}`.
fn tmux_failed(stderr: &str) -> Answer {
    let text = if stderr.is_empty() {
        "tmux fallo"
    } else {
        stderr
    };
    reply(
        StatusCode::INTERNAL_SERVER_ERROR,
        &json!({"error": py::strip(text)}),
    )
}

/// El hilo `_send` del Python: 1,5 s después, `send-keys -t =<s>: <c> Enter`;
/// su resultado no se mira. Contado en `Native::tasks` (se abandona al apagar,
/// como el hilo `daemon`).
fn type_later(native: &Native, sess: &str, command: String) {
    let tmux = native.options().tmux.clone();
    let target = format!("={sess}:");
    let spawned = native.tasks().spawn(async move {
        tokio::time::sleep(TYPE_DELAY).await;
        let _ = tmux
            .run(&["send-keys", "-t", &target, &command, "Enter"])
            .await;
    });
    if let Err(error) = spawned {
        eprintln!("comandos dash: no se pudo programar el tecleo en {sess}: {error}");
    }
}

/// `register_app_tab(sess, label, kind="project", cwd=cwd)` como efecto.
async fn register_tab(
    native: &Native,
    route: &str,
    sess: &str,
    label: &str,
    cwd: &str,
) -> Result<(), Fault> {
    mark_effect();
    tab_registry::register_app_tab(native, sess, Some(label), "project", "", cwd)
        .await
        .map_err(|e| e.into_fault(route))
}

/// `tmux("kill-session", "-t", "=" + new_sess)`: solo la sesión recién creada.
async fn kill_new(opts: &NativeOptions, sess: &str) -> Result<(), Fault> {
    mutate(opts, &["kill-session", "-t", &format!("={sess}")]).await?;
    Ok(())
}

/// Un `AccountError` con su texto, u otra cosa (que el port no reproduce).
fn account_message(e: &accounts::AccountError) -> Result<String, Fault> {
    if accounts::is_account_error(e) {
        Ok(e.0.clone())
    } else {
        Err(Fault::Decline)
    }
}

// --------------------------------------------------------- la ruta elegida

/// Lo que `resolve_route_selection` devuelve en `selection`.
struct Selection {
    model: String,
    effort: String,
    harness_account: String,
    motor_account: String,
}

/// `(route, selection)` o el código de `ProviderRegistryError`.
type Resolved = Result<(Map<String, Value>, Selection), String>;

/// `(registry.get("harnesses") or {}).get(key) or {}` → `.get("capabilities")
/// or {}`; lo que lanzaría declina.
fn capabilities(registry: &Value, key: &Value) -> Result<Map<String, Value>, Fault> {
    let harnesses = &registry["harnesses"];
    let item = if !truthy(harnesses) {
        Value::Null
    } else {
        let harnesses = harnesses.as_object().ok_or(Fault::Decline)?;
        match key {
            Value::String(k) => harnesses.get(k).cloned().unwrap_or(Value::Null),
            Value::Array(_) | Value::Object(_) => return Err(Fault::Decline),
            _ => Value::Null,
        }
    };
    if !truthy(&item) {
        return Ok(Map::new());
    }
    let caps = &item.as_object().ok_or(Fault::Decline)?.get("capabilities");
    match caps {
        Some(v) if truthy(v) => v.as_object().cloned().ok_or(Fault::Decline),
        _ => Ok(Map::new()),
    }
}

/// `next((a for a in list_accounts(registry, provider) if a["alias"] ==
/// alias), None)` y si es `selectable`. `Err(Ok(()))` = `AccountError`.
fn selectable(
    registry: &Value,
    provider: &str,
    alias: &str,
    paths: &accounts::Paths,
) -> Result<Result<bool, ()>, Fault> {
    let list = match accounts::list_accounts(registry, provider, paths) {
        Ok(list) => list,
        Err(e) if accounts::is_account_error(&e) => return Ok(Err(())),
        Err(_) => return Err(Fault::Decline),
    };
    let wanted = json!(alias);
    Ok(Ok(list
        .iter()
        .find(|a| python_eq(&a["alias"], &wanted))
        .is_some_and(|a| truthy(&a["selectable"]))))
}

/// `resolve_route_selection(data, scope)` (1661) sin `current_harness`.
async fn resolve_route_selection(
    native: &Native,
    data: &Map<String, Value>,
    scope: &str,
) -> Result<(Value, Resolved), Fault> {
    let (registry, matrix) = usage::providers::registry_and_matrix(native).await?;
    let mut route_id = str_or(&[data.get("routeId")], "")?;
    if route_id.is_empty() {
        let agent = str_or(&[data.get("agent")], "")?;
        route_id = match agent.as_str() {
            "claude" => "claude:claude",
            "claude-codex" => "claude:codex",
            "claude-grok" => "claude:grok",
            "codex" => "codex:codex",
            "grok" => "grok:grok",
            _ => "",
        }
        .to_owned();
    }
    let mut model = str_or(&[data.get("model")], "")?;
    let mut effort = str_or(&[data.get("effort")], "")?;
    let wanted = json!(route_id);
    let route = match matrix.iter().find(|r| python_eq(&r["id"], &wanted)) {
        Some(Value::Object(route)) => route.clone(),
        Some(_) => return Err(Fault::Decline),
        None => return Ok((registry, Err("route_unknown".into()))),
    };
    let motor = route.get("motor").cloned().unwrap_or(Value::Null);
    // `(registry.get("motors") or {}).get(motor) or {}` y sus modelos.
    let motors = &registry["motors"];
    let spec = if !truthy(motors) {
        Value::Null
    } else {
        let motors = motors.as_object().ok_or(Fault::Decline)?;
        match &motor {
            Value::String(m) => motors.get(m).cloned().unwrap_or(Value::Null),
            Value::Array(_) | Value::Object(_) => return Err(Fault::Decline),
            _ => Value::Null,
        }
    };
    let models = if truthy(&spec) {
        let models = &spec.as_object().ok_or(Fault::Decline)?.get("models");
        match models {
            Some(v) if truthy(v) => v.as_array().cloned().ok_or(Fault::Decline)?,
            _ => Vec::new(),
        }
    } else {
        Vec::new()
    };
    if model.is_empty()
        && let Some(first) = models.first()
    {
        let id = &first.as_object().ok_or(Fault::Decline)?.get("id");
        model = match id {
            Some(v) if truthy(v) => v.as_str().ok_or(Fault::Decline)?.to_owned(),
            _ => String::new(),
        };
    }
    let wanted_model = json!(model);
    let mut chosen = None;
    for m in &models {
        let m = m.as_object().ok_or(Fault::Decline)?;
        if python_eq(m.get("id").unwrap_or(&Value::Null), &wanted_model) {
            chosen = Some(m);
            break;
        }
    }
    if effort.is_empty()
        && let Some(chosen) = chosen.filter(|c| !c.is_empty())
    {
        let default = chosen.get("defaultEffort").unwrap_or(&Value::Null);
        effort = if truthy(default) {
            default.as_str().ok_or(Fault::Decline)?.to_owned()
        } else {
            match chosen.get("efforts") {
                Some(v) if truthy(v) => {
                    let first = v.as_array().ok_or(Fault::Decline)?.first();
                    first
                        .and_then(Value::as_str)
                        .ok_or(Fault::Decline)?
                        .to_owned()
                }
                _ => String::new(),
            }
        };
    }
    let selection = json!({"routeId": route_id, "model": model, "effort": effort});
    match validate_selection(&registry, &matrix, &selection, scope).map_err(decline)? {
        Ok(_) => {}
        // El `AttributeError` de una celda rara no es un `ProviderRegistryError`.
        Err(code) if code.contains(" object has no attribute ") => return Err(Fault::Decline),
        Err(code) => return Ok((registry, Err(code))),
    }
    let harness = route.get("harness").cloned().unwrap_or(Value::Null);
    let mut harness_alias = str_or(&[data.get("harnessAccount"), data.get("account")], "main")?;
    let same = python_eq(&motor, &harness);
    let mut motor_alias = if same {
        str_or(&[data.get("motorAccount")], &harness_alias)?
    } else {
        str_or(&[data.get("motorAccount")], "main")?
    };
    let harness_caps = capabilities(&registry, &harness)?;
    let motor_caps = capabilities(&registry, &motor)?;
    let scopes = match route.get("accountScopes") {
        Some(v) if truthy(v) => v.as_array().cloned().ok_or(Fault::Decline)?,
        _ => Vec::new(),
    };
    let (reg, paths) = (registry.clone(), paths(native.options()));
    let driver_acp = route.get("driver") == Some(&json!("acp"));
    let checked: Result<(String, String), String> = blocking(move || {
        let text = |v: &Value| v.as_str().map(str::to_owned).ok_or(Fault::Decline);
        if harness_caps.get("accounts").is_some_and(truthy) {
            match selectable(&reg, &text(&harness)?, &harness_alias, &paths)? {
                Err(()) => return Ok(Err("account_invalid".to_owned())),
                Ok(false) => return Ok(Err("harness_account_login_required".to_owned())),
                Ok(true) => {}
            }
        } else {
            harness_alias = "main".into();
        }
        if same {
            motor_alias = harness_alias.clone();
        } else if scopes.iter().any(|s| s.as_str() == Some("motor"))
            && motor_caps.get("accounts").is_some_and(truthy)
        {
            match selectable(&reg, &text(&motor)?, &motor_alias, &paths)? {
                Err(()) => return Ok(Err("account_invalid".to_owned())),
                Ok(false) => return Ok(Err("motor_account_login_required".to_owned())),
                Ok(true) => {}
            }
            if motor_alias != "main" && !driver_acp {
                return Ok(Err("motor_account_gateway_not_ready".to_owned()));
            }
        } else {
            motor_alias = "main".into();
        }
        Ok(Ok((harness_alias, motor_alias)))
    })
    .await?;
    let (harness_account, motor_account) = match checked {
        Ok(pair) => pair,
        Err(code) => return Ok((registry, Err(code))),
    };
    Ok((
        registry,
        Ok((
            route,
            Selection {
                model,
                effort,
                harness_account,
                motor_account,
            },
        )),
    ))
}

// ------------------------------------------------------------- worktree

/// Lo que `make_worktree(cwd)` hará, decidido antes de cualquier efecto.
struct WorktreePlan {
    git: PathBuf,
    /// `_worktree_include_files(cwd)`: archivos a copiar.
    include: Vec<PathBuf>,
}

/// `_worktree_include_files(cwd)` (4136): patrones de `.worktreeinclude` (o
/// `.env`, `.env.*`, `.envrc`) con `glob.glob(os.path.join(cwd, pat))` y solo
/// archivos. Se reproducen patrones de un nivel con `*` y `?`; lo demás (o un
/// `cwd` con caracteres de patrón) declina.
fn include_files(cwd: &str) -> Result<Vec<PathBuf>, Fault> {
    if cwd.contains(['*', '?', '[']) {
        return Err(Fault::Decline);
    }
    let patterns: Vec<String> = match fs::read(Path::new(cwd).join(".worktreeinclude")) {
        Ok(bytes) => {
            let text = String::from_utf8(bytes).map_err(decline)?;
            // `open()` en modo texto: saltos universales.
            let text = text.replace("\r\n", "\n").replace('\r', "\n");
            text.split_inclusive('\n')
                .filter(|l| !py::strip(l).is_empty() && !l.starts_with('#'))
                .map(|l| py::strip(l).to_owned())
                .collect()
        }
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::NotFound
                    | io::ErrorKind::IsADirectory
                    | io::ErrorKind::PermissionDenied
                    | io::ErrorKind::NotADirectory
            ) =>
        {
            vec![".env".into(), ".env.*".into(), ".envrc".into()]
        }
        Err(_) => return Err(Fault::Decline),
    };
    let mut names: Option<Vec<String>> = None;
    let mut out = Vec::new();
    for pattern in patterns {
        if pattern.contains(['/', '[']) {
            return Err(Fault::Decline);
        }
        let found: Vec<PathBuf> = if !pattern.contains(['*', '?']) {
            let path = Path::new(cwd).join(&pattern);
            // `os.path.lexists`.
            if fs::symlink_metadata(&path).is_ok() {
                vec![path]
            } else {
                Vec::new()
            }
        } else {
            if names.is_none() {
                let mut listed = Vec::new();
                if let Ok(entries) = fs::read_dir(cwd) {
                    for entry in entries {
                        let entry = entry.map_err(decline)?;
                        let name = entry.file_name().into_string().map_err(decline)?;
                        listed.push(name);
                    }
                }
                names = Some(listed);
            }
            let hidden = pattern.starts_with('.');
            names
                .iter()
                .flatten()
                .filter(|n| hidden || !n.starts_with('.'))
                .filter(|n| fnmatch(n, &pattern))
                .map(|n| Path::new(cwd).join(n))
                .collect()
        };
        out.extend(found);
    }
    Ok(out.into_iter().filter(|p| p.is_file()).collect())
}

/// `fnmatch` con solo `*` y `?` (sin clases), distinguiendo mayúsculas.
fn fnmatch(name: &str, pattern: &str) -> bool {
    let (n, p): (Vec<char>, Vec<char>) = (name.chars().collect(), pattern.chars().collect());
    let (mut i, mut j) = (0usize, 0usize);
    let (mut star, mut mark) = (None, 0usize);
    while i < n.len() {
        match p.get(j) {
            Some('*') => {
                star = Some(j);
                mark = i;
                j += 1;
            }
            Some(c) if *c == '?' || Some(c) == n.get(i) => {
                i += 1;
                j += 1;
            }
            _ => match star {
                Some(s) => {
                    j = s + 1;
                    mark += 1;
                    i = mark;
                }
                None => return false,
            },
        }
    }
    p.get(j..)
        .is_some_and(|rest| rest.iter().all(|c| *c == '*'))
}

/// La parte de `make_worktree` que solo lee: `git -C <cwd> rev-parse
/// --git-dir` (`None` si no es repositorio) y los archivos a copiar.
async fn worktree_plan(opts: &NativeOptions, cwd: &str) -> Result<Option<WorktreePlan>, Fault> {
    let git = procs::which_in(opts.search_path.as_deref(), "git").ok_or(Fault::Decline)?;
    let program = opts.program(&git);
    let probe = run_program(&program, &["-C", cwd, "rev-parse", "--git-dir"], GIT_PROBE)
        .await
        .map_err(decline)?;
    if !probe.ok {
        return Ok(None);
    }
    let owned = cwd.to_owned();
    let include = blocking(move || include_files(&owned)).await?;
    Ok(Some(WorktreePlan { git, include }))
}

/// `make_worktree(cwd)` (4149) desde su primer efecto: la ruta del worktree o
/// `None` (el Python sigue en `cwd` si `git worktree add` falla).
async fn make_worktree(
    opts: &NativeOptions,
    cwd: &str,
    plan: WorktreePlan,
) -> Result<Option<String>, Fault> {
    let base = Path::new(cwd).join(".claude/worktrees");
    let mut n = (now(opts).floor() as i64).rem_euclid(100_000);
    let (path, name) = loop {
        let name = format!("wt-{n}");
        let path = base.join(&name);
        // `os.path.exists`.
        if !path.exists() {
            break (path, name);
        }
        n += 1;
    };
    mark_effect();
    let dir = base.clone();
    blocking(move || fs::create_dir_all(&dir).map_err(|_| failure())).await?;
    let text = path.to_str().ok_or_else(failure)?.to_owned();
    let branch = format!("worktree-{name}");
    let added = run_program(
        &opts.program(&plan.git),
        &["-C", cwd, "worktree", "add", &text, "-b", &branch],
        GIT_WORKTREE,
    )
    .await
    .map_err(|_| failure())?;
    if !added.ok {
        return Ok(None);
    }
    let target = path.clone();
    blocking(move || {
        for file in plan.include {
            if let Some(name) = file.file_name() {
                let _ = copy2(&file, &target.join(name));
            }
        }
        Ok(())
    })
    .await?;
    Ok(Some(text))
}

/// `shutil.copy2`: contenido, permisos y tiempos.
fn copy2(from: &Path, to: &Path) -> io::Result<()> {
    fs::copy(from, to)?;
    let meta = fs::metadata(from)?;
    let times = fs::FileTimes::new()
        .set_accessed(meta.accessed()?)
        .set_modified(meta.modified()?);
    fs::File::options().write(true).open(to)?.set_times(times)
}

// -------------------------------------------------------------- comando

/// El comando del agente, o la respuesta que el Python da tras crear la
/// sesión (y matarla).
enum Launch {
    Command(String),
    Kill(StatusCode, Value),
}

/// `load_agent_roles().get("dangerFlags") or {}` y su bandera para `agent`
/// si `danger`. Lo que lanzaría tras los efectos declina antes.
fn danger_flag(opts: &NativeOptions, agent: &str, danger: bool) -> Result<String, Fault> {
    if !danger {
        return Ok(String::new());
    }
    let repo = opts.repo_root.as_ref().ok_or(Fault::Decline)?;
    let roles = match files::read_json_strict(&repo.join("config/agent-roles.json")) {
        Strict::Value(Value::Object(map)) => map,
        Strict::Value(_) | Strict::Missing | Strict::Unreadable => Map::new(),
        Strict::Unsure => return Err(Fault::Decline),
    };
    let flags = match roles.get("dangerFlags") {
        Some(v) if truthy(v) => v.as_object().cloned().ok_or(Fault::Decline)?,
        _ => Map::new(),
    };
    match flags.get(agent) {
        Some(v) if truthy(v) => Ok(format!(" {}", v.as_str().ok_or(Fault::Decline)?)),
        _ => Ok(String::new()),
    }
}

/// `motor_lock_env(motor, model)` (1834) como palabras.
fn motor_lock(motor: &str, model: &str) -> String {
    if !matches!(motor, "codex" | "grok") || model.is_empty() {
        return String::new();
    }
    [
        "ANTHROPIC_DEFAULT_OPUS_MODEL",
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        "CLAUDE_CODE_SUBAGENT_MODEL",
    ]
    .iter()
    .map(|k| format!("{k}={}", shlex_quote(model)))
    .collect::<Vec<_>>()
    .join(" ")
}

/// Todo lo que el comando necesita del agente elegido.
struct AgentSpec<'a> {
    agent: &'a str,
    route: Option<&'a Map<String, Value>>,
    model: &'a str,
    effort: &'a str,
    alias: &'a str,
    prefix: &'a str,
    danger: bool,
}

/// El comando de `/session-new` por agente (la tabla del Python). Bloquea.
fn agent_command(
    opts: &NativeOptions,
    registry: &Value,
    spec: &AgentSpec<'_>,
) -> Result<Launch, Fault> {
    let flag = danger_flag(opts, spec.agent, spec.danger)?;
    let (model, effort, prefix) = (spec.model, spec.effort, spec.prefix);
    let command = match spec.agent {
        "claude" => {
            let motor = match spec.route {
                Some(route) => route
                    .get("motor")
                    .and_then(Value::as_str)
                    .ok_or(Fault::Decline)?,
                None => "claude",
            };
            let mut cmd = format!("{prefix}claude");
            if motor != "claude" {
                let repo = opts.repo_root.as_ref().ok_or(Fault::Decline)?;
                let port = proxy_port(repo).map_err(decline)?;
                let lock = motor_lock(motor, model);
                let lock = if lock.is_empty() {
                    lock
                } else {
                    format!("{lock} ")
                };
                cmd = format!("{prefix}ANTHROPIC_BASE_URL=http://127.0.0.1:{port} {lock}claude");
            }
            if !model.is_empty() {
                cmd += &format!(" --model {}", shlex_quote(model));
            }
            if !effort.is_empty() {
                cmd += &format!(" --effort {}", shlex_quote(effort));
            }
            cmd + &flag
        }
        "codex" => {
            let mut cmd = format!("{prefix}codex");
            if !model.is_empty() {
                cmd += &format!(" -m {}", shlex_quote(model));
            }
            if !effort.is_empty() {
                cmd += &format!(
                    " -c {}",
                    shlex_quote(&format!("model_reasoning_effort=\"{effort}\""))
                );
            }
            cmd + &flag
        }
        "grok" => {
            let home = accounts::account_home(registry, "grok", &json!(spec.alias), &paths(opts))
                .map_err(decline)?;
            let mut cmd = format!("{prefix}grok");
            if !model.is_empty() {
                let catalog = grok_models(&home).map_err(decline)?;
                let Some(found) = catalog.iter().find(|m| m["id"] == json!(model)) else {
                    return Ok(Launch::Kill(
                        StatusCode::BAD_REQUEST,
                        json!({"error": format!("modelo Grok no disponible: {model}")}),
                    ));
                };
                if !effort.is_empty() {
                    let known = match &found["efforts"] {
                        Value::Array(list) => list.iter().any(|e| e.as_str() == Some(effort)),
                        v if !truthy(v) => false,
                        _ => return Err(Fault::Decline),
                    };
                    if !known {
                        return Ok(Launch::Kill(
                            StatusCode::BAD_REQUEST,
                            json!({"error": format!("esfuerzo no válido para {model}")}),
                        ));
                    }
                }
                cmd += &format!(" --model {}", shlex_quote(model));
                if !effort.is_empty() {
                    cmd += &format!(" --effort {}", shlex_quote(effort));
                }
            }
            cmd + &flag
        }
        // `_acp_launch_cmd` y `_harness_launch_cmd`: 2f-2/T1.
        "acp" | "opencode" | "agy" => return Err(Fault::Decline),
        _ => String::new(),
    };
    Ok(Launch::Command(command))
}

// ---------------------------------------------------------- /session-new

/// POST `/session-new` (9333).
pub(super) async fn session_new(native: &Native, data: &Map<String, Value>) -> Answer {
    const PATH: &str = "/session-new";
    let opts = native.options();
    // Los perfiles de lanzamiento (`session_profile_store`) siguen en el heredado.
    if data.get("profileId").is_some_and(truthy) {
        return Err(Fault::Decline);
    }
    let requested_agent = match data.get("agent") {
        None => "shell".to_owned(),
        Some(v) => py::take_chars(&str_of(v)?, 24),
    };
    let alias = py::take_chars(
        &str_or(&[data.get("harnessAccount"), data.get("account")], "main")?,
        64,
    );
    let motor_alias = py::take_chars(&str_or(&[data.get("motorAccount")], "main")?, 64);
    // `os.path.expanduser(str(...))`: lo que no es texto nunca empieza por `/`.
    let cwd = match data.get("cwd") {
        None => String::new(),
        Some(Value::String(c)) => {
            claude_trust::expanduser(c, &home_text(opts)?).map_err(decline)?
        }
        Some(_) => String::new(),
    };
    if !cwd.starts_with('/') {
        return reply(
            StatusCode::BAD_REQUEST,
            &json!({"error": "carpeta invalida"}),
        );
    }
    if !is_dir(opts, &cwd).await? {
        return reply(
            StatusCode::CONFLICT,
            &json!({"error": "Esa carpeta no existe todavía.", "code": "cwd_missing", "cwd": cwd}),
        );
    }
    if data.contains_key("routeId") && !data.get("routeId").is_some_and(truthy) {
        return reply(
            StatusCode::CONFLICT,
            &json!({"error": "Esa combinación no está disponible; elige un setup habilitado.",
                    "code": "route_unavailable"}),
        );
    }
    let (registry, route, selected) =
        if requested_agent == "shell" && !data.get("routeId").is_some_and(truthy) {
            let selected = Selection {
                model: String::new(),
                effort: String::new(),
                harness_account: alias,
                motor_account: motor_alias,
            };
            (registry(native).await?, None, selected)
        } else {
            match resolve_route_selection(native, data, "new_session").await? {
                (registry, Ok((route, selected))) => (registry, Some(route), selected),
                (_, Err(code)) => {
                    return reply(StatusCode::CONFLICT, &json!({"error": code, "code": code}));
                }
            }
        };
    let agent = match &route {
        Some(route) => route
            .get("harness")
            .and_then(Value::as_str)
            .ok_or(Fault::Decline)?
            .to_owned(),
        None => "shell".to_owned(),
    };
    let (mut alias, mut motor_alias) = (selected.harness_account, selected.motor_account);
    if agent != "shell" {
        for value in [&mut alias, &mut motor_alias] {
            match accounts::validate_alias(&json!(value)) {
                Ok(valid) => *value = valid,
                Err(e) => {
                    return reply(
                        StatusCode::BAD_REQUEST,
                        &json!({"error": account_message(&e)?, "code": "account_invalid"}),
                    );
                }
            }
        }
    }
    let (model, effort) = (selected.model, selected.effort);

    // Todo lo que queda por decidir, antes del primer efecto.
    let worktree = {
        let reads = opts.clone();
        let conf = blocking(move || read_conf(&reads)).await?;
        if conf_get(&conf, "AUTO_WORKTREE").unwrap_or("1") != "0"
            && agent != "shell"
            && target::cwd_has_live_agent(native, &cwd).await?
        {
            worktree_plan(opts, &cwd).await?
        } else {
            None
        }
    };
    let account = if agent != "shell" && harness_has_accounts(&registry, &agent) {
        let (reg, reads, agent, alias) =
            (registry.clone(), opts.clone(), agent.clone(), alias.clone());
        Some(
            blocking(move || {
                match accounts::account_environment(&reg, &agent, &json!(alias), &paths(&reads)) {
                    Ok(env) => Ok(Ok(format!("{} ", env_words(&env)?))),
                    Err(e) => Ok(Err(account_message(&e)?)),
                }
            })
            .await?,
        )
    } else {
        None
    };
    let launch = {
        let (reads, reg, route, agent) =
            (opts.clone(), registry.clone(), route.clone(), agent.clone());
        let (model, effort, alias) = (model.clone(), effort.clone(), alias.clone());
        let prefix = match &account {
            Some(Ok(prefix)) => prefix.clone(),
            _ => String::new(),
        };
        let danger = data.get("danger").is_some_and(truthy);
        blocking(move || {
            let spec = AgentSpec {
                agent: &agent,
                route: route.as_ref(),
                model: &model,
                effort: &effort,
                alias: &alias,
                prefix: &prefix,
                danger,
            };
            agent_command(&reads, &reg, &spec)
        })
        .await?
    };
    let typed = matches!(&launch, Launch::Command(c) if !c.is_empty());
    let claude_route = route
        .as_ref()
        .is_some_and(|r| r.get("harness") == Some(&json!("claude")));
    if typed && route.is_some() && !native.usage.enabled() {
        // `record_runtime_config` no tendría base: el heredado sí la tiene.
        return Err(Fault::Decline);
    }
    let home = home_text(opts)?;
    if typed && claude_route {
        let (reg, home, alias) = (registry.clone(), home.clone(), alias.clone());
        blocking(move || launch_command::trust_probe(&reg, &home, "main", &alias).map_err(decline))
            .await?;
    }
    registry_preflight(native, PATH, false).await?;
    server_running(opts).await?;

    // (5) worktree: el primer efecto posible.
    let (final_cwd, wt) = match worktree {
        Some(plan) => match make_worktree(opts, &cwd, plan).await? {
            Some(path) => (path, true),
            None => (cwd.clone(), false),
        },
        None => (cwd.clone(), false),
    };
    let n = (now(opts).floor() as i64).rem_euclid(100_000);
    let new_sess = free_session(opts, n).await?;
    let out = mutate(
        opts,
        &["new-session", "-d", "-s", &new_sess, "-c", &final_cwd],
    )
    .await?;
    if !out.ok {
        return tmux_failed(&out.stderr);
    }
    let label = format!(
        "{}{}",
        basename(cwd.trim_end_matches('/')),
        if wt { " ⎇" } else { "" }
    );
    register_tab(native, PATH, &new_sess, &label, &final_cwd).await?;
    if let Some(Err(message)) = account {
        kill_new(opts, &new_sess).await?;
        return reply(
            StatusCode::CONFLICT,
            &json!({"error": message, "code": "account_invalid"}),
        );
    }
    let command = match launch {
        Launch::Kill(status, body) => {
            kill_new(opts, &new_sess).await?;
            return reply(status, &body);
        }
        Launch::Command(command) => command,
    };
    if !command.is_empty() {
        let pane_target = format!("={new_sess}:");
        let shown = tmux(
            opts,
            &["display-message", "-p", "-t", &pane_target, "#{pane_id}"],
        )
        .await?;
        let pane = py::strip(&shown.stdout).to_owned();
        if let Some(route) = &route {
            record_runtime_config(
                native,
                route,
                &new_sess,
                &pane,
                [&model, &effort, &alias, &motor_alias],
            )
            .await;
        }
        if claude_route {
            inherit_trust(native, &registry, &home, &final_cwd, &alias).await;
        }
        type_later(native, &new_sess, command);
    }
    let route_id = match &route {
        Some(route) => route.get("id").cloned().unwrap_or(Value::Null),
        None => json!("shell"),
    };
    reply(
        StatusCode::OK,
        &json!({"ok": true, "session": new_sess,
                "cwd": final_cwd, "worktree": wt, "label": label,
                "routeId": route_id,
                "model": model, "effort": effort,
                "harnessAccount": alias, "motorAccount": motor_alias,
                "profileId": null,
                "profileStatus": null,
                "profileEffectiveNow": null}),
    )
}

/// `record_runtime_config(new_sess, pane, harness, motor, model, effort,
/// alias, motor_alias, route_id, "session-new")` (185): su `except` traga todo
/// error, así que un fallo solo se anota en stderr.
async fn record_runtime_config(
    native: &Native,
    route: &Map<String, Value>,
    sess: &str,
    pane: &str,
    [model, effort, alias, motor_alias]: [&str; 4],
) {
    let field = |key: &str| route.get(key).cloned().unwrap_or(Value::Null);
    let at = now(native.options()).floor() as i64;
    let or_unknown = |s: &str| {
        if s.is_empty() {
            "unknown".to_owned()
        } else {
            s.to_owned()
        }
    };
    let mut data = Map::new();
    data.insert("tmux_session".into(), json!(sess));
    data.insert("tmux_pane".into(), json!(pane));
    data.insert("harness".into(), field("harness"));
    data.insert("motor".into(), field("motor"));
    data.insert("model".into(), json!(model));
    data.insert("effort".into(), json!(effort));
    data.insert("harness_account".into(), json!(or_unknown(alias)));
    data.insert("motor_account".into(), json!(or_unknown(motor_alias)));
    let route_id = field("id");
    let route_id = if truthy(&route_id) {
        route_id
    } else {
        match (field("harness").as_str(), field("motor").as_str()) {
            (Some(h), Some(m)) => json!(format!("{h}:{m}")),
            _ => {
                eprintln!("comandos dash: /session-new sin ruta registrable");
                return;
            }
        }
    };
    data.insert("route_id".into(), route_id);
    data.insert("effective_at".into(), json!(at));
    data.insert("source".into(), json!("session-new"));
    data.insert("confidence".into(), json!("exact"));
    let saved = native
        .usage
        .with(move |b| {
            comandos_store::usage_import::record_session_config(&b.conn, &data, at)
                .map_err(|e| e.to_string())
        })
        .await;
    if !matches!(saved, Ok(Ok(()))) {
        eprintln!("comandos dash: /session-new no registró la configuración del pane");
    }
}

/// `inherit_trust_for_switch(final_cwd, "main", alias, harness="claude")` y,
/// si heredó, su `record_change`. Nunca rompe la petición (como el Python).
async fn inherit_trust(native: &Native, registry: &Value, home: &str, cwd: &str, alias: &str) {
    let (reg, home_owned, cwd_owned, alias_owned) = (
        registry.clone(),
        home.to_owned(),
        cwd.to_owned(),
        alias.to_owned(),
    );
    let inherited = blocking(move || {
        Ok(launch_command::inherit_trust_for_switch(
            &reg,
            &home_owned,
            &cwd_owned,
            "main",
            &alias_owned,
            "claude",
        ))
    })
    .await;
    match inherited {
        Ok(Ok(true)) => {}
        Ok(Ok(false)) => return,
        _ => {
            eprintln!("comandos dash: /session-new no pudo decidir la confianza de la carpeta");
            return;
        }
    }
    let mut event = Map::new();
    event.insert("origin".into(), json!("reparto"));
    event.insert("kind".into(), json!("trust_inherited"));
    event.insert(
        "note".into(),
        json!(launch_command::trust_note(cwd, "main", alias)),
    );
    let at = now(native.options()).floor() as i64;
    let saved = native
        .usage
        .with(move |b| comandos_store::usage::record_change(&b.conn, &event, at).map(|_| ()))
        .await;
    if !matches!(saved, Ok(Ok(()))) {
        eprintln!("comandos dash: /session-new no registró la confianza heredada");
    }
}

// ----------------------------------------------------------- /account/add

/// Lo que la siembra de `settings.json` (Claude) hará, decidido antes.
struct Seed {
    /// `hooks` del `~/.claude/settings.json` del usuario (`or {}`).
    hooks: Value,
    path: PathBuf,
}

/// La parte que solo lee de la siembra: `None` si el Python no siembra nada
/// (no es Claude, o el `settings.json` del usuario no se lee como objeto).
fn seed_plan(opts: &NativeOptions, provider: &str, dir: &Path) -> Result<Option<Seed>, Fault> {
    if provider != "claude" {
        return Ok(None);
    }
    let user = match files::read_json_strict(&opts.home.join(".claude/settings.json")) {
        Strict::Value(Value::Object(map)) => map,
        Strict::Value(_) | Strict::Missing | Strict::Unreadable => return Ok(None),
        Strict::Unsure => return Err(Fault::Decline),
    };
    let hooks = match user.get("hooks") {
        Some(v) if truthy(v) => v.clone(),
        _ => json!({}),
    };
    let path = dir.join("settings.json");
    if matches!(files::read_json_strict(&path), Strict::Unsure) {
        return Err(Fault::Decline);
    }
    Ok(Some(Seed { hooks, path }))
}

/// `update_json_object(<cuenta>/settings.json, seed)` (5212) dentro del
/// `try/except Exception: pass`: bajo el candado del archivo, un archivo que
/// no parsea o no es objeto se deja intacto. Bloquea.
fn apply_seed(lock: FileLock, seed: Seed) {
    let _lock = lock;
    let mut data = match files::read_json_strict(&seed.path) {
        Strict::Missing => Map::new(),
        Strict::Value(Value::Object(map)) => map,
        Strict::Value(_) | Strict::Unreadable => return,
        Strict::Unsure => {
            eprintln!(
                "comandos dash: /account/add no reproduce {}",
                seed.path.display()
            );
            return;
        }
    };
    if truthy(&seed.hooks) && !data.get("hooks").is_some_and(truthy) {
        data.insert("hooks".into(), seed.hooks);
    }
    data.insert(
        "preferredNotifChannel".into(),
        json!("notifications_disabled"),
    );
    let written = indent_dumps(&Value::Object(data), 2, false)
        .map_err(io::Error::other)
        .and_then(|text| files::write_text_atomic(&seed.path, &text));
    if let Err(error) = written {
        eprintln!("comandos dash: /account/add no sembró settings.json: {error}");
    }
}

/// `os.makedirs(d, mode=0o700, exist_ok=True)`: los intermedios con el modo
/// por omisión, la carpeta final con 0700 (menos la umask).
fn makedirs_700(dir: &Path) -> io::Result<()> {
    if let Some(parent) = dir.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    match fs::DirBuilder::new().mode(0o700).create(dir) {
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists && dir.is_dir() => Ok(()),
        other => other,
    }
}

/// El `config.toml` de una cuenta Codex nueva: `open(…, "x")` y `chmod 0600`.
fn codex_config(dir: &Path) -> io::Result<()> {
    let path = dir.join("config.toml");
    if path.exists() {
        return Ok(());
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o666)
        .open(&path)?;
    io::Write::write_all(&mut file, b"cli_auth_credentials_store = \"file\"\n")?;
    drop(file);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
}

/// El alias pedido o el primer `cuenta-<n>` (n ≥ 2) cuya carpeta no existe.
fn pick_alias(
    registry: &Value,
    provider: &str,
    raw: &str,
    paths: &accounts::Paths,
) -> Result<String, Fault> {
    if !raw.is_empty() {
        return Ok(raw.to_owned());
    }
    let mut n = 2u64;
    loop {
        let alias = format!("cuenta-{n}");
        // `account_home(...)` fuera de un `try`: su excepción sería un 500.
        let home =
            accounts::account_home(registry, provider, &json!(alias), paths).map_err(decline)?;
        if !home.exists() {
            return Ok(alias);
        }
        n += 1;
    }
}

/// Lo que `/account/add` decide antes de cualquier efecto.
struct AccountPlan {
    alias: String,
    dir: PathBuf,
    env: String,
    seed: Option<Seed>,
}

/// POST `/account/add` (`account_add_request`, 2749).
pub(super) async fn account_add(native: &Native, data: &Map<String, Value>) -> Answer {
    const PATH: &str = "/account/add";
    let _serial = ACCOUNT_ADD_LOCK.lock().await;
    let opts = native.options();
    let provider = match data.get("provider") {
        Some(v) if truthy(v) => v.as_str().unwrap_or("").to_owned(),
        _ => "claude".to_owned(),
    };
    if !matches!(provider.as_str(), "claude" | "codex" | "grok") {
        return reply(
            StatusCode::BAD_REQUEST,
            &json!({"error": "provider de cuenta no soportado"}),
        );
    }
    let registry = registry(native).await?;
    let raw = match data.get("alias") {
        Some(v) if truthy(v) => py::strip(&str_of(v)?).to_owned(),
        _ => String::new(),
    };
    let (reads, prov) = (opts.clone(), provider.clone());
    let plan = blocking(move || -> Result<Result<AccountPlan, Answer>, Fault> {
        let paths = paths(&reads);
        let alias = pick_alias(&registry, &prov, &raw, &paths)?;
        let checked = accounts::validate_alias(&json!(alias)).and_then(|alias| {
            accounts::account_home(&registry, &prov, &json!(alias), &paths).map(|d| (alias, d))
        });
        let (alias, dir) = match checked {
            Ok(pair) => pair,
            Err(e) => {
                let message = account_message(&e)?;
                return Ok(Err(reply(
                    StatusCode::BAD_REQUEST,
                    &json!({"error": message}),
                )));
            }
        };
        dir.to_str().ok_or(Fault::Decline)?;
        let list = accounts::list_accounts(&registry, &prov, &paths).map_err(decline)?;
        let wanted = json!(alias);
        if list
            .iter()
            .find(|a| python_eq(&a["alias"], &wanted))
            .is_some_and(|a| truthy(&a["selectable"]))
        {
            let error =
                format!("La cuenta {alias} ya tiene sesión iniciada. Elígela para cambiar.");
            return Ok(Err(reply(StatusCode::CONFLICT, &json!({"error": error}))));
        }
        let env =
            accounts::account_environment(&registry, &prov, &wanted, &paths).map_err(decline)?;
        let env = env_words(&env)?;
        let seed = seed_plan(&reads, &prov, &dir)?;
        Ok(Ok(AccountPlan {
            alias,
            dir,
            env,
            seed,
        }))
    })
    .await?;
    let plan = match plan {
        Ok(plan) => plan,
        Err(answer) => return answer,
    };
    let home = home_text(opts)?;
    let cwd = match data.get("cwd") {
        Some(v) if truthy(v) => str_of(v)?,
        _ => home.clone(),
    };
    let cwd = if is_dir(opts, &cwd).await? { cwd } else { home };
    registry_preflight(native, PATH, false).await?;
    server_running(opts).await?;

    // Efectos.
    mark_effect();
    let (dir, prov) = (plan.dir.clone(), provider.clone());
    blocking(move || {
        makedirs_700(&dir).map_err(|_| failure())?;
        if prov == "codex" {
            codex_config(&dir).map_err(|_| failure())?;
        }
        Ok(())
    })
    .await?;
    if let Some(seed) = plan.seed {
        match FileLock::acquire_timeout(&seed.path, LOCK_WAIT).await {
            Ok(lock) => {
                let _ = blocking(move || {
                    apply_seed(lock, seed);
                    Ok(())
                })
                .await;
            }
            Err(error) => {
                eprintln!("comandos dash: /account/add sin candado de settings.json: {error}");
            }
        }
    }
    let n = ((now(opts) * 10.0).floor() as i64).rem_euclid(1_000_000);
    let new_sess = free_session(opts, n).await?;
    let out = mutate(opts, &["new-session", "-d", "-s", &new_sess, "-c", &cwd]).await?;
    if !out.ok {
        return tmux_failed(&out.stderr);
    }
    let label = format!("login {provider} · {}", plan.alias);
    register_tab(native, PATH, &new_sess, &label, &cwd).await?;
    let login = match provider.as_str() {
        "claude" => "claude auth login --claudeai".to_owned(),
        "codex" => format!(
            "codex -c 'cli_auth_credentials_store=\"file\"' login{}",
            if data.get("deviceAuth").is_some_and(truthy) {
                " --device-auth"
            } else {
                ""
            }
        ),
        _ => "grok login".to_owned(),
    };
    type_later(
        native,
        &new_sess,
        format!("{LOGIN_ENV}{} {login}", plan.env),
    );
    reply(
        StatusCode::OK,
        &json!({"ok": true, "session": new_sess, "alias": plan.alias, "provider": provider}),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        Fault, NativeOptions, fnmatch, include_files, make_worktree, motor_lock, worktree_plan,
    };
    use std::{fs, path::PathBuf, process::Command};

    /// Un directorio temporal propio (se borra al soltarlo).
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("cmd-launch-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn git(dir: &std::path::Path, args: &[&str]) -> bool {
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env_remove("GLIBC_TUNABLES")
            .output()
            .is_ok_and(|o| o.status.success())
    }

    fn options(home: &std::path::Path) -> NativeOptions {
        let mut opts = NativeOptions::for_home(home, home.join("state.sqlite3"));
        opts.search_path = std::env::var_os("PATH");
        opts
    }

    #[tokio::test]
    async fn worktree_copies_include_files_like_make_worktree() {
        let scratch = Scratch::new("wt");
        let repo = scratch.0.join("repo");
        fs::create_dir_all(&repo).unwrap();
        if !git(&repo, &["init", "-q"])
            || !git(
                &repo,
                &[
                    "-c",
                    "user.name=prueba",
                    "-c",
                    "user.email=prueba@example.invalid",
                    "commit",
                    "-q",
                    "--allow-empty",
                    "-m",
                    "inicio",
                ],
            )
        {
            eprintln!("git no está disponible: se salta");
            return;
        }
        for name in [".env", ".env.local", ".envrc", "otro.txt"] {
            fs::write(repo.join(name), name).unwrap();
        }
        fs::create_dir_all(repo.join(".env.d")).unwrap();
        let opts = options(&scratch.0);
        let cwd = repo.to_str().unwrap();
        let plan = worktree_plan(&opts, cwd).await.ok().flatten().unwrap();
        let mut names: Vec<String> = plan
            .include
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, [".env", ".env.local", ".envrc"]);
        let path = make_worktree(&opts, cwd, plan)
            .await
            .ok()
            .flatten()
            .unwrap();
        let made = PathBuf::from(&path);
        assert!(made.starts_with(repo.join(".claude/worktrees")), "{path}");
        let name = made.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("wt-"), "{name}");
        assert_eq!(
            fs::read_to_string(made.join(".env.local")).unwrap(),
            ".env.local"
        );
        assert!(!made.join("otro.txt").exists());
        assert!(git(
            &repo,
            &["rev-parse", "--verify", &format!("worktree-{name}")]
        ));
        // `.worktreeinclude` propio; un patrón con clase no se reproduce.
        fs::write(
            repo.join(".worktreeinclude"),
            "# nota\r\n otro.txt \n\n?envrc\n",
        )
        .unwrap();
        let mut found = include_files(cwd).ok().unwrap();
        found.sort();
        // `?envrc` no ve los ocultos (`glob` solo los casa con un `.` literal).
        assert_eq!(found, [repo.join("otro.txt")]);
        fs::write(repo.join(".worktreeinclude"), "[ab].txt\n").unwrap();
        assert!(matches!(include_files(cwd), Err(Fault::Decline)));
        // Fuera de un repositorio: sin worktree.
        let plain = scratch.0.join("suelta");
        fs::create_dir_all(&plain).unwrap();
        assert!(matches!(
            worktree_plan(&opts, plain.to_str().unwrap()).await,
            Ok(None)
        ));
    }

    #[test]
    fn fnmatch_star_and_question() {
        assert!(fnmatch(".env.local", ".env.*"));
        assert!(fnmatch(".env.", ".env.*"));
        assert!(!fnmatch(".env", ".env.*"));
        assert!(fnmatch("a.txt", "?.txt"));
        assert!(!fnmatch("ab.txt", "?.txt"));
        assert!(fnmatch("x", "*"));
        assert!(fnmatch("", "*"));
        assert!(!fnmatch("A", "a"));
        assert!(fnmatch("abcabd", "*abd"));
    }

    #[test]
    fn motor_lock_only_for_foreign_motors_with_model() {
        assert_eq!(motor_lock("claude", "m"), "");
        assert_eq!(motor_lock("codex", ""), "");
        assert_eq!(
            motor_lock("codex", "gpt 5"),
            "ANTHROPIC_DEFAULT_OPUS_MODEL='gpt 5' ANTHROPIC_DEFAULT_SONNET_MODEL='gpt 5' \
             ANTHROPIC_DEFAULT_HAIKU_MODEL='gpt 5' CLAUDE_CODE_SUBAGENT_MODEL='gpt 5'"
        );
    }
}

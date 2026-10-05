//! Modo de ejecución y rutas. El modo decide qué puede escribir la app: solo
//! `Live` toca el estado real; `Shadow` mira el estado real sin escribir nada;
//! `Sandbox` vive en su propio servidor tmux y en directorios temporales.
//!
//! `AppConfig` solo se construye con [`parse_args`] y sus campos son privados: un
//! sandbox sin servidor tmux propio no se puede representar (`Mode::Sandbox` lleva
//! una [`SocketLabel`], que nunca es `default`), así que quien abra tmux a partir de
//! la configuración no puede caer por descuido en el servidor del usuario.
//!
//! En sandbox, además, toda ruta en la que la app escribe se resuelve (enlaces
//! incluidos) y tiene que quedar dentro de `sandbox_root()` o del directorio temporal
//! del proceso: es una lista de permitidos, no de prohibidos.
use std::ffi::OsString;
use std::ops::RangeInclusive;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    /// tmux propio (`-L`), hooks temporales: desarrollo y pruebas.
    Sandbox,
    /// tmux del usuario con clientes de solo lectura; ninguna escritura.
    Shadow,
    /// Sustituto real de `cc-app`.
    Live,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    App { default_live: bool },
    Notifyd,
}

/// `cc-app` (el symlink del cutover) arranca en live; `comandos-app` a secas, en sandbox.
pub fn resolve_entry(argv0: &str) -> Entry {
    let name = argv0.rsplit('/').next().unwrap_or(argv0);
    match name {
        "cc-notifyd" | "comandos-notifyd" => Entry::Notifyd,
        "cc-app" => Entry::App { default_live: true },
        _ => Entry::App {
            default_live: false,
        },
    }
}

/// Etiqueta de un servidor tmux propio (`tmux -L <etiqueta>`). Nunca vacía, nunca
/// `default` (el servidor del usuario) y sin separadores de ruta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SocketLabel(String);

impl SocketLabel {
    pub fn new(raw: &str) -> Result<Self, String> {
        let valid = !raw.is_empty()
            && raw.len() <= 64
            && raw != "default"
            && !raw.starts_with('.')
            && !raw.starts_with('-')
            && raw
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
        if valid {
            Ok(Self(raw.to_string()))
        } else {
            Err(format!(
                "--tmux-socket {raw:?} no vale: hace falta una etiqueta propia \
                 ([A-Za-z0-9._-], sin '.' ni '-' al principio, distinta de \"default\"); \
                 para el servidor del usuario se omite la opción"
            ))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Servidor tmux con el que habla la app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TmuxServer<'a> {
    /// El servidor por omisión del usuario (`/tmp/tmux-<uid>/default`).
    User,
    /// Un servidor propio, `tmux -L <etiqueta>`.
    Private(&'a SocketLabel),
}

/// El modo con lo que cada uno exige: el sandbox siempre trae socket y raíz propios.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    Sandbox { socket: SocketLabel, root: PathBuf },
    Shadow { socket: Option<SocketLabel> },
    Live { socket: Option<SocketLabel> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppConfig {
    mode: Mode,
    home: PathBuf,
    hooks_dir: PathBuf,
    dash_url: Option<String>,
    web_data_dir: PathBuf,
    web_cache_dir: PathBuf,
    runtime_dir: PathBuf,
    repo_root: Option<PathBuf>,
}

const DEFAULT_DASH: &str = "http://127.0.0.1:4777";
/// Directorio (bajo `$XDG_RUNTIME_DIR`) y etiqueta tmux por omisión del sandbox.
const SANDBOX_NAME: &str = "comandos-app-sbx";
const SHADOW_NAME: &str = "comandos-app-shadow";
/// Banda de `devhost`: el sandbox nunca habla con el tablero real (4777–4782).
const SANDBOX_DASH_PORTS: RangeInclusive<u16> = 7200..=7399;
const USAGE: &str = "uso: comandos-app [--mode sandbox|shadow|live] [--tmux-socket NOMBRE] \
                     [--hooks-dir DIR] [--dash-url http://127.0.0.1:PUERTO] [--repo DIR]";

impl AppConfig {
    pub fn mode(&self) -> RunMode {
        match self.mode {
            Mode::Sandbox { .. } => RunMode::Sandbox,
            Mode::Shadow { .. } => RunMode::Shadow,
            Mode::Live { .. } => RunMode::Live,
        }
    }

    pub fn tmux_server(&self) -> TmuxServer<'_> {
        match &self.mode {
            Mode::Sandbox { socket, .. } => TmuxServer::Private(socket),
            Mode::Shadow { socket } | Mode::Live { socket } => socket
                .as_ref()
                .map_or(TmuxServer::User, TmuxServer::Private),
        }
    }

    /// Etiqueta `-L`, o `None` para el servidor del usuario (nunca en sandbox).
    pub fn tmux_socket(&self) -> Option<&str> {
        match self.tmux_server() {
            TmuxServer::User => None,
            TmuxServer::Private(label) => Some(label.as_str()),
        }
    }

    /// Raíz de los directorios temporales del sandbox (`$XDG_RUNTIME_DIR/comandos-app-sbx`,
    /// ya resuelta). Toda escritura del sandbox queda aquí o bajo el temporal del proceso.
    pub fn sandbox_root(&self) -> Option<&Path> {
        match &self.mode {
            Mode::Sandbox { root, .. } => Some(root),
            Mode::Shadow { .. } | Mode::Live { .. } => None,
        }
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn hooks_dir(&self) -> &Path {
        &self.hooks_dir
    }

    /// `None` en sandbox sin `--dash-url`: la app arranca sin tablero.
    pub fn dash_url(&self) -> Option<&str> {
        self.dash_url.as_deref()
    }

    pub fn web_data_dir(&self) -> &Path {
        &self.web_data_dir
    }

    pub fn web_cache_dir(&self) -> &Path {
        &self.web_cache_dir
    }

    pub fn runtime_dir(&self) -> &Path {
        &self.runtime_dir
    }

    pub fn repo_root(&self) -> Option<&Path> {
        self.repo_root.as_deref()
    }

    /// La sombra nunca escribe estado (archivos de hooks, tmux, tablero).
    pub fn writes_allowed(&self) -> bool {
        self.mode() != RunMode::Shadow
    }

    /// Mismo nombre que el Python en live (`bin/cc-app:43`): `DISPLAY` vacío pasa a
    /// `x`, `/` pasa a `_` y se quitan los `:`. Así los dos se excluyen.
    pub fn lock_file_name(&self, display: &str) -> String {
        let display = if display.is_empty() { "x" } else { display };
        let d = display.replace('/', "_").replace(':', "");
        match self.mode() {
            RunMode::Live => format!("cc-app-{d}.lock"),
            RunMode::Shadow => format!("sombra-app-rs-{d}.lock"),
            RunMode::Sandbox => format!("{SANDBOX_NAME}-{d}.lock"),
        }
    }

    pub fn wm_class(&self) -> &'static str {
        match self.mode() {
            RunMode::Live => "comandos",
            RunMode::Shadow => "sombra-app-rs",
            RunMode::Sandbox => "sandbox-app-rs",
        }
    }

    pub fn title(&self) -> &'static str {
        match self.mode() {
            RunMode::Live => "ComandOS",
            RunMode::Shadow => "Sombra de la app (Rust)",
            RunMode::Sandbox => "Sandbox de la app (Rust)",
        }
    }

    /// En sandbox, dentro de `sandbox_root()` (la lista de permitidos de escritura).
    pub fn layout_dump_path(&self) -> PathBuf {
        match &self.mode {
            Mode::Live { .. } => self.runtime_dir.join("comandos-app-layout.json"),
            Mode::Shadow { .. } => self.runtime_dir.join(format!("{SHADOW_NAME}-layout.json")),
            Mode::Sandbox { root, .. } => root.join("layout.json"),
        }
    }
}

/// `http://127.0.0.1:PUERTO[/]` → (URL normalizada, puerto).
fn loopback_only(url: &str) -> Result<(String, u16), String> {
    loopback_from(url, "--dash-url")
}

/// Igual que [`loopback_only`]; `origin` nombra de dónde vino el valor en el error.
fn loopback_from(url: &str, origin: &str) -> Result<(String, u16), String> {
    let rest = url
        .strip_prefix("http://127.0.0.1:")
        .ok_or_else(|| format!("{origin} solo acepta http://127.0.0.1:PUERTO, no {url}"))?;
    let digits = rest.strip_suffix('/').unwrap_or(rest);
    let port = digits
        .parse::<u16>()
        .ok()
        .filter(|p| *p != 0 && digits.chars().all(|c| c.is_ascii_digit()))
        .ok_or_else(|| format!("{origin} con puerto inválido: {url}"))?;
    Ok((format!("http://127.0.0.1:{port}"), port))
}

fn sandbox_dash(url: &str) -> Result<String, String> {
    let (url, port) = loopback_only(url)?;
    if SANDBOX_DASH_PORTS.contains(&port) {
        Ok(url)
    } else {
        Err(format!(
            "en sandbox --dash-url usa la banda {}–{} de devhost (4777–4782 son del \
             tablero real), no {port}",
            SANDBOX_DASH_PORTS.start(),
            SANDBOX_DASH_PORTS.end()
        ))
    }
}

/// Absoluta respecto al directorio actual; error si no se puede saber cuál es.
fn absolute(path: &Path) -> Result<PathBuf, String> {
    std::path::absolute(path)
        .map_err(|e| format!("no se puede hacer absoluta {}: {e}", path.display()))
}

/// Ruta real: el ancestro existente más profundo resuelto con `canonicalize` (enlaces
/// incluidos) y el resto añadido componente a componente. Un `..` en la parte que no
/// existe, o un componente de esa parte que ya existe (un enlace colgante, o algo
/// creado entre medias), es error: no se puede saber adónde llevaría.
fn resolve(path: &Path) -> Result<PathBuf, String> {
    let abs = absolute(path)?;
    let comps: Vec<Component<'_>> = abs.components().collect();
    for split in (0..=comps.len()).rev() {
        let head: PathBuf = comps.iter().take(split).collect();
        if head.as_os_str().is_empty() {
            continue;
        }
        let Ok(mut out) = std::fs::canonicalize(&head) else {
            continue;
        };
        for comp in comps.iter().skip(split) {
            let name: OsString = match comp {
                Component::Normal(name) => name.to_os_string(),
                Component::CurDir => continue,
                Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                    return Err(format!(
                        "{}: `..` fuera de la parte que existe",
                        path.display()
                    ));
                }
            };
            out.push(name);
            if std::fs::symlink_metadata(&out).is_ok() {
                return Err(format!(
                    "{}: {} existe pero no se puede resolver (¿enlace colgante?)",
                    path.display(),
                    out.display()
                ));
            }
        }
        return Ok(out);
    }
    Err(format!("{}: ningún ancestro existe", path.display()))
}

/// Raíces donde el sandbox puede escribir, ya resueltas. Se descarta la raíz que sea
/// `/` o que contenga el `HOME` (un `TMPDIR=/home` no abre la puerta a `~`).
fn sandbox_write_roots(root: &Path, temp: &Path, home: &Path) -> Result<Vec<PathBuf>, String> {
    let home = resolve(home).unwrap_or_else(|_| home.to_path_buf());
    let mut roots = Vec::new();
    for candidate in [root, temp] {
        let resolved = resolve(candidate)?;
        if resolved.parent().is_some() && !home.starts_with(&resolved) {
            roots.push(resolved);
        }
    }
    Ok(roots)
}

/// Resuelve `path` y exige que quede dentro de alguna de `roots`.
fn confine(what: &str, path: &Path, roots: &[PathBuf]) -> Result<PathBuf, String> {
    let resolved = resolve(path).map_err(|e| format!("en sandbox {what}: {e}"))?;
    if roots.iter().any(|r| resolved.starts_with(r)) {
        Ok(resolved)
    } else {
        let allowed: Vec<String> = roots.iter().map(|r| r.display().to_string()).collect();
        Err(format!(
            "en sandbox {what} tiene que quedar dentro de {} (resuelto: {})",
            if allowed.is_empty() {
                "una raíz propia (ninguna es válida)".to_string()
            } else {
                allowed.join(" o ")
            },
            resolved.display()
        ))
    }
}

/// `COMANDOS_APP_REPO`, o el destino de `<hooks>/dash/index.html` dos niveles arriba
/// (mismo criterio que `comandos dash`).
fn repo_root(hooks: &Path, env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    if let Some(raw) = env("COMANDOS_APP_REPO").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(raw));
    }
    let index = std::fs::canonicalize(hooks.join("dash/index.html")).ok()?;
    index.parent()?.parent().map(Path::to_path_buf)
}

/// Lee la configuración de los argumentos y del entorno `env` (en el binario, el del
/// proceso). El temporal permitido al sandbox es `TMPDIR` o `/tmp`, la misma regla que
/// `std::env::temp_dir()`, leída de `env` para que las pruebas no muten el entorno.
pub fn parse_args(
    args: &[String],
    default_live: bool,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<AppConfig, String> {
    let home = env("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .ok_or("HOME no está definido")?;
    let uid = nix::unistd::getuid();
    let runtime_dir = env("XDG_RUNTIME_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/tmp/comandos-{uid}")));
    let mut mode = if default_live {
        RunMode::Live
    } else {
        RunMode::Sandbox
    };
    let (mut socket, mut hooks, mut dash, mut repo) = (None, None, None, None);
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = || {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{arg} necesita valor\n{USAGE}"))
        };
        match arg.as_str() {
            "--mode" => {
                mode = match value()?.as_str() {
                    "sandbox" => RunMode::Sandbox,
                    "shadow" => RunMode::Shadow,
                    "live" => RunMode::Live,
                    other => return Err(format!("modo desconocido: {other}\n{USAGE}")),
                }
            }
            "--tmux-socket" => socket = Some(SocketLabel::new(&value()?)?),
            "--hooks-dir" => hooks = Some(PathBuf::from(value()?)),
            "--dash-url" => dash = Some(value()?),
            "--repo" => repo = Some(PathBuf::from(value()?)),
            other => return Err(format!("opción desconocida: {other}\n{USAGE}")),
        }
    }
    // Vacío equivale a no definido, como `os.environ.get(...) or DEFAULT` (bin/cc-app:86).
    let env_dash = || {
        env("COMANDOS_DASH_URL")
            .filter(|v| !v.is_empty())
            .map(|u| loopback_from(&u, "COMANDOS_DASH_URL").map(|(url, _)| url))
            .transpose()
    };
    let real_dash = |dash: Option<String>| -> Result<String, String> {
        match dash {
            Some(u) => Ok(loopback_only(&u)?.0),
            None => Ok(env_dash()?.unwrap_or_else(|| DEFAULT_DASH.into())),
        }
    };
    let (mode, hooks_dir, dash_url, web_data_dir, web_cache_dir) = match mode {
        RunMode::Sandbox => {
            let temp = env("TMPDIR")
                .filter(|v| !v.is_empty())
                .map_or_else(|| PathBuf::from("/tmp"), PathBuf::from);
            let wanted_root = runtime_dir.join(SANDBOX_NAME);
            let roots = sandbox_write_roots(&wanted_root, &temp, &home)?;
            let root = confine("la raíz del sandbox", &wanted_root, &roots)?;
            let hooks_dir = confine(
                "--hooks-dir",
                &hooks.unwrap_or_else(|| root.join("hooks")),
                &roots,
            )?;
            let socket = match socket {
                Some(label) => label,
                None => SocketLabel::new(SANDBOX_NAME)?,
            };
            let dash_url = dash.map(|u| sandbox_dash(&u)).transpose()?;
            let data = confine("los datos web", &root.join("data"), &roots)?;
            let cache = confine("la caché web", &root.join("cache"), &roots)?;
            (
                Mode::Sandbox { socket, root },
                hooks_dir,
                dash_url,
                data,
                cache,
            )
        }
        RunMode::Shadow => {
            let base = runtime_dir.join(SHADOW_NAME);
            (
                Mode::Shadow { socket },
                absolute(&hooks.unwrap_or_else(|| home.join(".claude/hooks")))?,
                Some(real_dash(dash)?),
                base.join("data"),
                base.join("cache"),
            )
        }
        RunMode::Live => (
            Mode::Live { socket },
            absolute(&hooks.unwrap_or_else(|| home.join(".claude/hooks")))?,
            Some(real_dash(dash)?),
            home.join(".local/share/comandos"),
            home.join(".cache/comandos"),
        ),
    };
    let repo_root = repo.or_else(|| repo_root(&hooks_dir, env));
    Ok(AppConfig {
        mode,
        home,
        hooks_dir,
        dash_url,
        web_data_dir,
        web_cache_dir,
        runtime_dir,
        repo_root,
    })
}

/// Idioma de la interfaz: `CC_LANG=es|en` de `<hooks>/cc-notify.conf`; si no (falta,
/// `auto`, otro valor o archivo ilegible), `es` cuando `$LANG` empieza por `es`.
///
/// Diferencia con el Python: `_ui_lang` (`bin/cc-app:177`) pregunta a `cc-dash`
/// (`GET /conf` → `_lang`) y, si el tablero no responde, cae directamente en `$LANG`.
/// Aquí se lee el archivo con `comandos_runtime::providers::read_conf`, el port de
/// `read_conf` de `cc-dash` (la última asignación gana, comillas emparejadas fuera,
/// `export` no cuenta). Coincide con lo que `/conf` respondería cuando la app y el
/// tablero tienen el mismo `LANG`, y funciona sin tablero (sandbox).
pub fn ui_lang(hooks_dir: &Path, lang_env: Option<&str>) -> &'static str {
    let conf = comandos_runtime::providers::read_conf(&hooks_dir.join("cc-notify.conf"));
    let cc_lang = conf
        .ok()
        .and_then(|pairs| pairs.into_iter().find(|(k, _)| k == "CC_LANG"))
        .map(|(_, v)| v);
    match cc_lang.as_deref() {
        Some("es") => "es",
        Some("en") => "en",
        _ if lang_env.is_some_and(|l| l.to_lowercase().starts_with("es")) => "es",
        _ => "en",
    }
}

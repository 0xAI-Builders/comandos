//! F. Catálogos de solo lectura: GET /model-tiers (8286, `load_model_tiers`
//! 3714) y GET /sovereignty (8323, `sovereignty_report` 4275). Ninguna escribe.
use super::{
    Answer, Entry, Fault, Key, Native, NativeOptions, NativeRoute, Verb,
    files::{self, Strict},
    light::read_reply,
    py,
};
use crate::HandlerError;
use serde_json::{Map, Value, json};
use std::{
    fs,
    io::{self, Read},
    os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, PermissionsExt},
    },
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogRoute {
    ModelTiers,
    Sovereignty,
}

pub const ROUTES: &[Entry] = &[
    Entry {
        verb: Verb::Get,
        key: Key::Path("/model-tiers"),
        route: NativeRoute::Catalog(CatalogRoute::ModelTiers),
    },
    Entry {
        verb: Verb::Get,
        key: Key::Raw("/sovereignty"),
        route: NativeRoute::Catalog(CatalogRoute::Sovereignty),
    },
];

pub async fn answer(native: &Native, route: CatalogRoute) -> Answer {
    match route {
        CatalogRoute::ModelTiers => model_tiers(native),
        CatalogRoute::Sovereignty => sovereignty(native).await,
    }
}

/// `load_model_tiers`: el Python cachea por `mtime` y, si el archivo falta o no
/// parsea, devuelve lo último que cargó. Solo se responde lo que el archivo
/// dice con certeza; lo demás lo sabe solo la caché del Python.
fn model_tiers(native: &Native) -> Answer {
    read_reply(&read_model_tiers(native.options())?)
}

/// `load_model_tiers()` cuando su resultado es seguro: el objeto del archivo
/// (o `{}` si no es objeto). Ausente, ilegible o incierto: el Python devuelve
/// su copia en caché, que solo él conoce → `Decline`. Lo comparten
/// GET /model-tiers y GET /state (`write_app_tab_models`).
pub fn read_model_tiers(opts: &NativeOptions) -> Result<Value, Fault> {
    let Some(root) = opts.repo_root.as_ref() else {
        return Err(Fault::Decline);
    };
    match files::read_json_strict(&root.join("config/model-tiers.json")) {
        Strict::Value(value @ Value::Object(_)) => Ok(value),
        // `data if isinstance(data, dict) else {}`.
        Strict::Value(_) => Ok(json!({})),
        Strict::Missing | Strict::Unreadable | Strict::Unsure => Err(Fault::Decline),
    }
}

fn failure() -> Fault {
    Fault::Error(HandlerError::Failure)
}

/// `glob.has_magic`: un HOME con comodines haría que el `glob` del Python
/// interpretara la ruta misma como patrón.
fn has_magic(text: &str) -> bool {
    text.contains(['*', '?', '['])
}

async fn sovereignty(native: &Native) -> Answer {
    let hooks = native.options().hooks.clone();
    let db = hooks.join("comandos-usage.sqlite");
    // Todo lo que puede declinar se lee antes de abrir la base: el carril
    // migraría una base vieja y luego ya no se podría reenviar sin efectos.
    let read = hooks.clone();
    let inputs = tokio::task::spawn_blocking(move || Inputs::read(&read))
        .await
        .map_err(|_| failure())??;
    // `finfo(db)` va antes que las tablas: si la base no existe no se abre (el
    // carril la crearía). Un archivo vacío lo leería el Python sin tablas y el
    // carril lo migraría: se reenvía para no escribir desde un GET de lectura.
    let tables = match fs::metadata(&db) {
        Ok(meta) if meta.len() == 0 => return Err(Fault::Decline),
        Ok(_) => Some(usage_tables(native).await?),
        Err(_) => None,
    };
    let now = (native.options().clock)().div_euclid(1000);
    let home = native.options().home.clone();
    let report = tokio::task::spawn_blocking(move || report(&home, &hooks, inputs, tables, now))
        .await
        .map_err(|_| failure())?;
    read_reply(&report)
}

/// `[{"name": t, "rows": count(*)} for t in sqlite_master]` por el carril de
/// uso; cualquier error de SQLite → `[]` (`except Exception`). Un nombre con
/// comillas rompería la consulta del Python de forma no reproducible: declina.
async fn usage_tables(native: &Native) -> Result<Vec<(String, i64)>, Fault> {
    let listed = native
        .usage
        .with(|u| -> rusqlite::Result<Option<Vec<(String, i64)>>> {
            let names = u
                .conn
                .prepare(
                    "select name from sqlite_master where type='table' and name not like 'sqlite_%'",
                )?
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            if names.iter().any(|t| t.contains('"')) {
                return Ok(None);
            }
            names
                .into_iter()
                .map(|t| {
                    let sql = format!("select count(*) from \"{t}\"");
                    u.conn.query_row(&sql, [], |r| r.get(0)).map(|n| (t, n))
                })
                .collect::<rusqlite::Result<Vec<_>>>()
                .map(Some)
        })
        .await?;
    match listed {
        Ok(Some(tables)) => Ok(tables),
        Ok(None) => Err(Fault::Decline),
        Err(_) => Ok(Vec::new()),
    }
}

/// `int(st.st_mtime)`: CPython forma `sec + nsec * 1e-9` en doble precisión y
/// trunca hacia cero (un `nsec` cercano a 1e9 redondea al segundo siguiente).
fn py_mtime(meta: &fs::Metadata) -> i64 {
    let seconds = meta.mtime() as f64 + meta.mtime_nsec() as f64 * 1e-9;
    seconds.trunc() as i64
}

/// `finfo` del Python: `os.stat` (sigue enlaces) y `$HOME` → `~` en la ruta.
fn finfo(path: &Path, label: &str, kind: &str, home: &str) -> Option<Map<String, Value>> {
    let meta = fs::metadata(path).ok()?;
    let text = path.to_str()?;
    let mode = meta.permissions().mode() & 0o7777;
    let mut m = Map::new();
    m.insert("label".into(), json!(label));
    m.insert("path".into(), json!(text.replace(home, "~")));
    m.insert("kind".into(), json!(kind));
    m.insert("bytes".into(), json!(meta.len()));
    m.insert("mtime".into(), json!(py_mtime(&meta)));
    m.insert("mode".into(), json!(format!("0o{mode:o}")));
    m.insert("private".into(), json!(mode & 0o077 == 0));
    Some(m)
}

/// `len(glob.glob(base/p1/p2/…))`; cada componente es `(prefijo, sufijo)` de
/// un patrón con un solo `*`. `*` no casa nombres que empiezan por `.`; los
/// componentes intermedios han de ser directorios (siguiendo enlaces) y el
/// último casa cualquier entrada, también directorios y enlaces rotos.
fn glob_count(base: &Path, parts: &[(&str, &str)]) -> usize {
    let Some(((prefix, suffix), rest)) = parts.split_first() else {
        return 1;
    };
    let Ok(entries) = fs::read_dir(base) else {
        return 0;
    };
    let mut n = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let bytes = name.as_bytes();
        let fits = bytes.first() != Some(&b'.')
            && bytes.len() >= prefix.len() + suffix.len()
            && bytes.starts_with(prefix.as_bytes())
            && bytes.ends_with(suffix.as_bytes());
        if !fits {
            continue;
        }
        if rest.is_empty() {
            n += 1;
        } else if entry.path().is_dir() {
            n += glob_count(&entry.path(), rest);
        }
    }
    n
}

/// `sum(1 for _ in open(path, "rb"))`, en trozos; `OSError` → `None`.
fn line_count(path: &Path) -> Option<usize> {
    let mut file = fs::File::open(path).ok()?;
    let mut buf = vec![0u8; 64 * 1024];
    let mut lines = 0usize;
    let mut last = None;
    loop {
        let read = match file.read(&mut buf) {
            Ok(0) => break,
            Ok(read) => read,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return None,
        };
        let chunk = buf.get(..read)?;
        lines += chunk.iter().filter(|b| **b == b'\n').count();
        last = chunk.last().copied();
    }
    Some(lines + usize::from(last.is_some_and(|b| b != b'\n')))
}

/// Líneas de un archivo que el Python lee con `open()` en modo texto:
/// `OSError` → `None`; bytes no UTF-8 → `UnicodeDecodeError`, fuera de su
/// `except OSError` (500 en el Python) → se declina. Saltos universales:
/// `\r\n` y `\r` terminan línea y llegan como `\n`.
fn text_lines(path: &Path) -> Result<Option<Vec<String>>, Fault> {
    let Ok(bytes) = fs::read(path) else {
        return Ok(None);
    };
    let text = String::from_utf8(bytes).map_err(|_| Fault::Decline)?;
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    Ok(Some(
        text.split_inclusive('\n').map(str::to_owned).collect(),
    ))
}

const FILES: [(&str, &str, &str); 10] = [
    ("prefs.json", "Preferencias del tablero", "json"),
    ("snippets.json", "Snippets", "json"),
    ("app-tabs.json", "Pestañas de la app", "json"),
    ("focus.json", "Pomodoro en curso", "json"),
    ("focus-queue.jsonl", "Avisos encolados en foco", "jsonl"),
    ("pane-models.txt", "Modelo por pane (tmux)", "txt"),
    ("dash-token", "Token de acceso remoto", "secret"),
    (
        "telegram.env",
        "Credenciales de Telegram retiradas (sin uso)",
        "secret",
    ),
    ("providers.env", "Credenciales de proveedores", "secret"),
    ("cc-notify.conf", "Configuración de avisos", "conf"),
];

/// Las entradas de `sovereignty_report` que pueden declinar: el HOME como
/// texto sin comodines y los dos archivos de texto que el Python decodifica.
struct Inputs {
    home: PathBuf,
    home_text: String,
    ssh: Option<Vec<String>>,
    conf: Option<Vec<String>>,
}

impl Inputs {
    fn read(hooks: &Path) -> Result<Self, Fault> {
        // `os.path.expanduser("~")`: el HOME del que cuelga `hooks`.
        let home = hooks
            .parent()
            .and_then(Path::parent)
            .ok_or(Fault::Decline)?;
        let home_text = home.to_str().ok_or(Fault::Decline)?;
        if home_text.is_empty() || has_magic(home_text) || has_magic(hooks.to_str().unwrap_or("*"))
        {
            return Err(Fault::Decline);
        }
        Ok(Self {
            home: home.to_path_buf(),
            home_text: home_text.to_owned(),
            ssh: text_lines(&home.join(".ssh/config"))?,
            conf: text_lines(&hooks.join("cc-notify.conf"))?,
        })
    }
}

/// `sovereignty_report`: el inventario, en el orden del Python. Bloquea
/// (recorre directorios y cuenta líneas): corre en `spawn_blocking`.
fn report(
    context_home: &Path,
    hooks: &Path,
    inputs: Inputs,
    tables: Option<Vec<(String, i64)>>,
    now: i64,
) -> Value {
    let Inputs {
        home,
        home_text,
        ssh,
        conf,
    } = inputs;
    let home_text = home_text.as_str();
    let mut stores = Vec::new();
    if let Some(mut fi) = finfo(
        &hooks.join("comandos-usage.sqlite"),
        "Uso y costos (turnos medidos)",
        "sqlite",
        home_text,
    ) {
        let rows: Vec<Value> = tables
            .unwrap_or_default()
            .into_iter()
            .map(|(name, rows)| json!({"name": name, "rows": rows}))
            .collect();
        fi.insert("tables".into(), Value::Array(rows));
        stores.push(Value::Object(fi));
    }
    stores.push(json!({
        "label": "Estado por sesión (hooks)",
        "path": "~/.claude/hooks/state/",
        "kind": "json",
        "count": glob_count(&hooks.join("state"), &[("", ".json")]),
    }));
    let events = hooks.join("events.jsonl");
    if let Some(mut fi) = finfo(&events, "Historial de eventos", "jsonl", home_text) {
        if let Some(n) = line_count(&events) {
            fi.insert("count".into(), json!(n));
        }
        stores.push(Value::Object(fi));
    }
    for (name, label, kind) in FILES {
        if let Some(fi) = finfo(&hooks.join(name), label, kind, home_text) {
            stores.push(Value::Object(fi));
        }
    }
    stores.push(json!({
        "label": "Transcripts de Claude Code",
        "path": "~/.claude/projects/*/",
        "kind": "jsonl",
        "count": glob_count(&home.join(".claude/projects"), &[("", ""), ("", ".jsonl")]),
    }));
    let codex = glob_count(
        &home.join(".codex/sessions"),
        &[("", ""), ("", ""), ("", ""), ("rollout-", ".jsonl")],
    );
    if codex > 0 {
        stores.push(json!({
            "label": "Rollouts de Codex",
            "path": "~/.codex/sessions/",
            "kind": "jsonl",
            "count": codex,
        }));
    }
    // `ln.strip().lower().startswith("host ") and "*" not in ln`.
    let ssh = ssh.map_or(0, |lines| {
        lines
            .iter()
            .filter(|l| py::strip(l).to_lowercase().starts_with("host ") && !l.contains('*'))
            .count()
    });
    stores.push(json!({
        "label": "Servidores SSH",
        "path": "~/.ssh/config",
        "kind": "conf",
        "count": ssh,
    }));
    // `conf[k] = v.strip().strip('"\'')`: gana el último `NATIVE_NOTIFY`.
    let mut native_notify = String::from("0");
    for line in conf.unwrap_or_default() {
        let stripped = py::strip(&line);
        if stripped.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = stripped.split_once('=')
            && key == "NATIVE_NOTIFY"
        {
            native_notify = py::strip(value)
                .trim_matches(|c| c == '"' || c == '\'')
                .to_owned();
        }
    }
    json!({
        "stores": stores,
        "browser": [
            {"key": "cc-pomos", "label": "Registro de pomodoros"},
            {"key": "cc-dash-labels", "label": "Nombres de pestañas"},
            {"key": "cc-ssh-open", "label": "Servidores desplegados"},
        ],
        "outbound": [
            {
                "label": "Acceso remoto (tailnet)",
                "on": super::files::DomainDocument::new(context_home, hooks, "webterm-enabled").ok().and_then(|d| d.read_bytes().ok().flatten()).is_some(),
                "how": "tailscale serve — solo tu tailnet, nunca Funnel",
            },
            {
                "label": "Proveedores de IA (los agentes)",
                "on": true,
                "how": "Claude/Codex/etc. hablan con su API: eso es el agente, no ComandOS",
            },
            {
                "label": "Notificaciones nativas del escritorio",
                "on": native_notify == "1",
                "how": "GNOME, en esta máquina",
            },
        ],
        "generatedAt": now,
    })
}

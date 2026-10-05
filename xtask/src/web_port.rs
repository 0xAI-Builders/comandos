//! Vigilancia del objetivo en movimiento (Fase 3b, B3): `web-port check`
//! compara el hash de cada componente portado con su origen actual en el
//! checkout principal; `web-port pin <id>` fija el texto y el hash actuales.
//!
//! Registro: `crates/comandos-web/components.json` (lista) y, según el
//! preflight R11, `crates/comandos-web/components/<id>.json` (una entrada por
//! archivo). El texto portado de cada componente se guarda en
//! `xtask/web/ported/<id>.txt` para mostrar la deriva como diff.
//!
//! Códigos de salida: 0 todo al día; 1 deriva, origen ausente o error de
//! E/S; 2 uso incorrecto o id desconocido; 3 no hay registro de componentes
//! (todavía no existe `components.json`: lo crea B1).
use crate::web_inventory::{self, Unit, region_text, sha256_hex};
use serde_json::{Map, Value, json};
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct PortOptions {
    /// Repositorio con los orígenes (`dash/…`): el checkout principal.
    pub repo: PathBuf,
    /// `components.json`; su directorio hermano `components/` también cuenta.
    pub registry: PathBuf,
    /// Directorio de los textos portados (`<id>.txt`).
    pub ported: PathBuf,
    /// Inventario del que crear entradas nuevas en `pin`; `None` escanea `repo`.
    pub inventory: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckState {
    Ok,
    Drift { expected: String, found: String },
    MissingSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckEntry {
    pub id: String,
    pub state: CheckState,
    /// Diff del texto portado contra el actual (solo con deriva y texto portado).
    pub diff: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pinned {
    pub id: String,
    pub kind: String,
    pub sha256: String,
    pub ported: PathBuf,
}

#[derive(Debug)]
pub enum PortError {
    NoRegistry(PathBuf),
    UnknownId(String),
    Invalid(String),
    Io(String),
}

impl fmt::Display for PortError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PortError::NoRegistry(p) => write!(
                f,
                "no hay registro de componentes: falta {} (ni su directorio components/); lo crea B1",
                p.display()
            ),
            PortError::UnknownId(id) => write!(
                f,
                "componente desconocido: {id} (ni en el registro ni en el inventario)"
            ),
            PortError::Invalid(m) => write!(f, "registro inválido: {m}"),
            PortError::Io(m) => write!(f, "{m}"),
        }
    }
}

impl PortError {
    pub fn exit_code(&self) -> i32 {
        match self {
            PortError::NoRegistry(_) => 3,
            PortError::UnknownId(_) => 2,
            PortError::Invalid(_) | PortError::Io(_) => 1,
        }
    }
}

/// Dónde vive una entrada del registro.
#[derive(Debug, Clone)]
enum Slot {
    /// Posición en `components.json`.
    List(usize),
    /// `components/<archivo>.json`.
    File(PathBuf),
}

struct Registry {
    entries: Vec<(Slot, Map<String, Value>)>,
}

fn components_dir(registry: &Path) -> PathBuf {
    registry.with_file_name("components")
}

fn load(registry: &Path) -> Result<Registry, PortError> {
    let dir = components_dir(registry);
    if !registry.is_file() && !dir.is_dir() {
        return Err(PortError::NoRegistry(registry.to_path_buf()));
    }
    let mut entries = Vec::new();
    if registry.is_file() {
        let text = std::fs::read_to_string(registry)
            .map_err(|e| PortError::Io(format!("{}: {e}", registry.display())))?;
        let v: Value = serde_json::from_str(&text)
            .map_err(|e| PortError::Invalid(format!("{}: {e}", registry.display())))?;
        let list = v.as_array().ok_or_else(|| {
            PortError::Invalid(format!("{}: se esperaba una lista", registry.display()))
        })?;
        for (i, e) in list.iter().enumerate() {
            let m = e.as_object().ok_or_else(|| {
                PortError::Invalid(format!(
                    "{}: la entrada {i} no es un objeto",
                    registry.display()
                ))
            })?;
            entries.push((Slot::List(i), m.clone()));
        }
    }
    if dir.is_dir() {
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map_err(|e| PortError::Io(format!("{}: {e}", dir.display())))?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        files.sort();
        for p in files {
            let text = std::fs::read_to_string(&p)
                .map_err(|e| PortError::Io(format!("{}: {e}", p.display())))?;
            let v: Value = serde_json::from_str(&text)
                .map_err(|e| PortError::Invalid(format!("{}: {e}", p.display())))?;
            let m = v.as_object().ok_or_else(|| {
                PortError::Invalid(format!("{}: se esperaba un objeto", p.display()))
            })?;
            entries.push((Slot::File(p), m.clone()));
        }
    }
    Ok(Registry { entries })
}

fn field<'a>(m: &'a Map<String, Value>, k: &str) -> Option<&'a str> {
    m.get(k).and_then(Value::as_str)
}

/// Texto actual del origen de una entrada (archivo entero, o su región).
pub fn source_text(repo: &Path, entry: &Map<String, Value>) -> Option<String> {
    let source = field(entry, "source")?;
    let text = std::fs::read(repo.join(source)).ok()?;
    match field(entry, "kind") {
        Some("region") => {
            let html = String::from_utf8(text).ok()?;
            let script = entry
                .get("script_index")
                .and_then(Value::as_u64)
                .and_then(|n| usize::try_from(n).ok());
            region_text(
                &html,
                field(entry, "marker_start")?,
                field(entry, "marker_end")?,
                script,
            )
            .map(str::to_string)
        }
        _ => String::from_utf8(text).ok(),
    }
}

/// Nombre de archivo seguro para un id.
fn file_id(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Directorio temporal propio: `create_dir` falla si ya existe (no sigue
/// enlaces plantados por otro en el `temp_dir` compartido).
fn private_tmp_dir() -> Option<PathBuf> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    (0..16u32).find_map(|n| {
        let d = std::env::temp_dir().join(format!("web-port-{}-{nanos}-{n}", std::process::id()));
        std::fs::create_dir(&d).ok().map(|_| d)
    })
}

/// `git diff --no-index` con cabeceras `portado/<id>` y `actual/<id>`.
fn diff(ported: &Path, current: &str, id: &str) -> Option<String> {
    let old = std::fs::read(ported).ok()?;
    let dir = private_tmp_dir()?;
    let name = file_id(id);
    let run = || -> Option<String> {
        for (sub, bytes) in [("portado", old.as_slice()), ("actual", current.as_bytes())] {
            std::fs::create_dir(dir.join(sub)).ok()?;
            std::fs::write(dir.join(sub).join(&name), bytes).ok()?;
        }
        let out = Command::new("git")
            .current_dir(&dir)
            .args(["diff", "--no-index", "--no-color", "--no-prefix", "--"])
            .arg(format!("portado/{name}"))
            .arg(format!("actual/{name}"))
            .output()
            .ok()?;
        Some(String::from_utf8_lossy(&out.stdout).into_owned())
    };
    let text = run();
    let _ = std::fs::remove_dir_all(&dir);
    text
}

pub fn check(o: &PortOptions) -> Result<Vec<CheckEntry>, PortError> {
    let reg = load(&o.registry)?;
    let mut out = Vec::new();
    for (_, e) in &reg.entries {
        let id = field(e, "id").unwrap_or("?").to_string();
        let expected = field(e, "sha256").unwrap_or("").to_string();
        let Some(text) = source_text(&o.repo, e) else {
            out.push(CheckEntry {
                id,
                state: CheckState::MissingSource,
                diff: None,
            });
            continue;
        };
        let found = sha256_hex(text.as_bytes());
        if found == expected {
            out.push(CheckEntry {
                id,
                state: CheckState::Ok,
                diff: None,
            });
        } else {
            let d = diff(&o.ported.join(format!("{}.txt", file_id(&id))), &text, &id);
            out.push(CheckEntry {
                id,
                state: CheckState::Drift { expected, found },
                diff: d,
            });
        }
    }
    Ok(out)
}

pub fn exit_code(r: &[CheckEntry]) -> i32 {
    if r.iter().all(|e| e.state == CheckState::Ok) {
        0
    } else {
        1
    }
}

fn write_json(p: &Path, v: &Value) -> Result<(), PortError> {
    let text =
        serde_json::to_string_pretty(v).map_err(|e| PortError::Invalid(e.to_string()))? + "\n";
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| PortError::Io(format!("{}: {e}", d.display())))?;
    }
    std::fs::write(p, text).map_err(|e| PortError::Io(format!("{}: {e}", p.display())))
}

/// Unidades e interoperabilidad de las que crear una entrada nueva.
fn inventory(o: &PortOptions) -> Result<(Vec<Value>, Value), PortError> {
    match &o.inventory {
        Some(p) => {
            let text = std::fs::read_to_string(p)
                .map_err(|e| PortError::Io(format!("{}: {e}", p.display())))?;
            let v: Value = serde_json::from_str(&text)
                .map_err(|e| PortError::Invalid(format!("{}: {e}", p.display())))?;
            let units = v["units"].as_array().cloned().unwrap_or_default();
            let ix = std::fs::read_to_string(p.with_file_name("interop.json"))
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or(Value::Null);
            Ok((units, ix))
        }
        None => {
            let units: Vec<Unit> = web_inventory::scan(&o.repo);
            let ix = web_inventory::interop(&units, &o.repo);
            Ok((units.iter().map(web_inventory::unit_json).collect(), ix))
        }
    }
}

/// Componente de una unidad del inventario (`region:red` → `red`).
fn component_of(o: &PortOptions, id: &str) -> Result<Option<String>, PortError> {
    let (units, _) = inventory(o)?;
    Ok(units
        .iter()
        .find(|u| u["id"] == id || u["component"] == id)
        .and_then(|u| u["component"].as_str())
        .map(str::to_string))
}

/// Entrada nueva desde el inventario: exporta los globales suyos que otra
/// unidad, el host o un iframe consumen.
fn new_entry(o: &PortOptions, id: &str) -> Result<Map<String, Value>, PortError> {
    let (units, ix) = inventory(o)?;
    let u = units
        .iter()
        .find(|u| u["component"] == id || u["id"] == id)
        .ok_or_else(|| PortError::UnknownId(id.to_string()))?;
    let uid = u["id"].as_str().unwrap_or("");
    let consumed = |name: &str| -> bool {
        let g = &ix[name];
        let by_me = g["defined_by"]
            .as_array()
            .is_some_and(|a| a.iter().any(|x| x == uid));
        let external = ["used_by", "mutated_by", "called_by"]
            .iter()
            .any(|k| g[*k].as_array().is_some_and(|a| !a.is_empty()));
        by_me && external
    };
    let exports: Vec<String> = u["defines"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .filter(|n| consumed(n))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let mut m = Map::new();
    m.insert("id".into(), json!(u["component"].as_str().unwrap_or(id)));
    m.insert("kind".into(), u["kind"].clone());
    m.insert("source".into(), u["source"].clone());
    if u["kind"] == "region" {
        m.insert("marker_start".into(), u["marker_start"].clone());
        m.insert("marker_end".into(), u["marker_end"].clone());
        m.insert("script_index".into(), u["script_index"].clone());
    }
    m.insert("sha256".into(), json!(""));
    m.insert("exports".into(), json!(exports));
    m.insert("deps".into(), json!([]));
    Ok(m)
}

pub fn pin(o: &PortOptions, id: &str) -> Result<Pinned, PortError> {
    let reg = load(&o.registry)?;
    let by_id = |want: &str| {
        reg.entries
            .iter()
            .find(|(_, e)| field(e, "id") == Some(want))
            .cloned()
    };
    // Un id de unidad (`region:red`) se normaliza a su componente (`red`)
    // antes de buscar: fijar dos veces actualiza, nunca duplica.
    let found = match by_id(id) {
        Some(x) => Some(x),
        None => component_of(o, id)?.and_then(|c| by_id(&c)),
    };
    let (slot, mut entry) = match found {
        Some(x) => x,
        None => {
            let e = new_entry(o, id)?;
            let eid = field(&e, "id").unwrap_or(id).to_string();
            let slot = if o.registry.is_file() {
                Slot::List(usize::MAX)
            } else {
                Slot::File(components_dir(&o.registry).join(format!("{}.json", file_id(&eid))))
            };
            (slot, e)
        }
    };
    let eid = field(&entry, "id").unwrap_or(id).to_string();
    let text = source_text(&o.repo, &entry).ok_or_else(|| {
        PortError::Io(format!(
            "{eid}: no se encuentra su origen en {}",
            o.repo.display()
        ))
    })?;
    let sha = sha256_hex(text.as_bytes());
    entry.insert("sha256".into(), json!(sha));
    let ported = o.ported.join(format!("{}.txt", file_id(&eid)));
    std::fs::create_dir_all(&o.ported)
        .map_err(|e| PortError::Io(format!("{}: {e}", o.ported.display())))?;
    std::fs::write(&ported, &text)
        .map_err(|e| PortError::Io(format!("{}: {e}", ported.display())))?;
    match slot {
        Slot::File(p) => write_json(&p, &Value::Object(entry.clone()))?,
        Slot::List(i) => {
            let text = std::fs::read_to_string(&o.registry)
                .map_err(|e| PortError::Io(format!("{}: {e}", o.registry.display())))?;
            let mut v: Value =
                serde_json::from_str(&text).map_err(|e| PortError::Invalid(e.to_string()))?;
            let list = v.as_array_mut().ok_or_else(|| {
                PortError::Invalid(format!("{}: se esperaba una lista", o.registry.display()))
            })?;
            match list.get_mut(i) {
                Some(slot) => *slot = Value::Object(entry.clone()),
                None => list.push(Value::Object(entry.clone())),
            }
            write_json(&o.registry, &v)?;
        }
    }
    Ok(Pinned {
        id: eid,
        kind: field(&entry, "kind").unwrap_or("script").to_string(),
        sha256: sha,
        ported,
    })
}

/// Raíz del workspace (padre de `xtask/`).
pub fn workspace_root() -> PathBuf {
    let xtask = Path::new(env!("CARGO_MANIFEST_DIR"));
    xtask.parent().unwrap_or(xtask).to_path_buf()
}

/// Checkout principal: el directorio del `.git` común (un worktree apunta
/// a él). Si git no responde, la raíz del workspace.
pub fn default_repo() -> PathBuf {
    let ws = workspace_root();
    let common = Command::new("git")
        .arg("-C")
        .arg(&ws)
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()));
    match common {
        Some(p) if p.file_name().is_some_and(|n| n == ".git") => {
            p.parent().map(Path::to_path_buf).unwrap_or(ws)
        }
        _ => ws,
    }
}

fn usage() -> i32 {
    eprintln!(
        "uso: cargo run -p xtask -- web-port check [--repo DIR] [--registry F] [--ported DIR]\n     \
         cargo run -p xtask -- web-port pin <id> [--repo DIR] [--registry F] [--ported DIR] [--inventory F]\n\
         salida: 0 al día, 1 deriva/origen ausente/error, 2 uso o id desconocido, 3 sin registro de componentes"
    );
    2
}

/// `web-port check|pin`.
pub fn main(args: &[String]) -> i32 {
    let ws = workspace_root();
    let abs = |p: &str| {
        let p = PathBuf::from(p);
        if p.is_absolute() { p } else { ws.join(p) }
    };
    let mut o = PortOptions {
        repo: PathBuf::new(),
        registry: ws.join("crates/comandos-web/components.json"),
        ported: ws.join("xtask/web/ported"),
        inventory: None,
    };
    let mut repo = None;
    let mut positional = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--repo" | "--registry" | "--ported" | "--inventory" => {
                let Some(v) = it.next() else { return usage() };
                match a.as_str() {
                    "--repo" => repo = Some(abs(v)),
                    "--registry" => o.registry = abs(v),
                    "--ported" => o.ported = abs(v),
                    _ => o.inventory = Some(abs(v)),
                }
            }
            s if s.starts_with("--") => return usage(),
            s => positional.push(s.to_string()),
        }
    }
    o.repo = repo.unwrap_or_else(default_repo);
    match positional
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["check"] => match check(&o) {
            Ok(r) => {
                for e in &r {
                    match &e.state {
                        CheckState::Ok => println!("ok     {}", e.id),
                        CheckState::MissingSource => {
                            println!("falta  {} (origen o marcadores no encontrados)", e.id)
                        }
                        CheckState::Drift { expected, found } => {
                            println!(
                                "drift  {}  portado {} → actual {}",
                                e.id,
                                expected.get(..12).unwrap_or(expected),
                                found.get(..12).unwrap_or(found)
                            );
                            match &e.diff {
                                Some(d) => print!("{d}"),
                                None => println!(
                                    "       (sin texto portado en {}: no hay diff)",
                                    o.ported.display()
                                ),
                            }
                        }
                    }
                }
                println!("{} componentes, origen {}", r.len(), o.repo.display());
                exit_code(&r)
            }
            Err(e) => {
                eprintln!("error: {e}");
                e.exit_code()
            }
        },
        ["pin", id] => match pin(&o, id) {
            Ok(p) => {
                println!(
                    "fijado {} ({}) sha256 {} → {}",
                    p.id,
                    p.kind,
                    p.sha256,
                    p.ported.display()
                );
                0
            }
            Err(e) => {
                eprintln!("error: {e}");
                e.exit_code()
            }
        },
        _ => usage(),
    }
}

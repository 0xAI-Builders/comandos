//! `lib/claude_trust.py`: la aceptación de carpeta de Claude Code
//! (`projects[<cwd>].hasTrustDialogAccepted` en `~/.claude.json`) y su
//! herencia entre cuentas.
//!
//! Las rutas son texto como en el Python (`os.path` sobre `str`); una ruta
//! que no es UTF-8 o un `~usuario` (base de contraseñas) no se reproducen:
//! `Unsure`. Leer un `.claude.json` que el Python quizá aceptaría y el parser
//! portado no (sustitutos sueltos, anidamiento enorme, bytes que no son
//! UTF-8) también es `Unsure`: quien llama declina antes de cualquier efecto.
//!
//! Efectos (los del Python): `os.makedirs` del directorio del archivo,
//! `<archivo>.lock` abierto en modo `a+` (0666 menos la umask si es nuevo) con
//! `flock` exclusivo, `<archivo>.tmp` escrito con `json.dump(indent=2)` y un
//! salto de línea, y `os.replace` sobre el archivo. Desviación deliberada:
//! el archivo reemplazado conserva su modo original (el Python le deja el del
//! temporal, y un `~/.claude.json` 0600 pasaba a 0644).
use crate::Unsure;
use crate::agent_procs::{dirname, normpath, realpath};
use comandos_core::json::{MAX_WORKSPACE_JSON_DEPTH, indent_dumps, truthy, workspace_loads};
use serde_json::{Map, Value};
use std::{
    fs, io,
    io::Write,
    os::unix::{
        ffi::OsStrExt,
        fs::{OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
};

/// `json.load(open(path))` clasificado como lo ve el Python.
enum Read {
    /// `FileNotFoundError`.
    Missing,
    /// Cualquier otra excepción (JSON roto, BOM, directorio…): el `except`.
    Failed,
    Value(Value),
}

fn read_json(path: &Path) -> Result<Read, Unsure> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Read::Missing),
        // `IsADirectoryError`, `PermissionError`…: el `except Exception`.
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::IsADirectory
                    | io::ErrorKind::PermissionDenied
                    | io::ErrorKind::NotADirectory
            ) =>
        {
            return Ok(Read::Failed);
        }
        Err(_) => return Err(Unsure),
    };
    let text = std::str::from_utf8(&bytes).map_err(|_| Unsure)?;
    if text.starts_with('\u{feff}') {
        return Ok(Read::Failed);
    }
    match workspace_loads(text) {
        Ok(value) => Ok(Read::Value(value)),
        Err(_) if surrogate_escape(text) || deep(text) => Err(Unsure),
        Err(_) => Ok(Read::Failed),
    }
}

/// `\uD800`–`\uDFFF`: el `json` del Python los admite sueltos; el Rust no.
fn surrogate_escape(text: &str) -> bool {
    text.match_indices("\\u").any(|(i, _)| {
        let hex = text.as_bytes().get(i + 2..i + 4).unwrap_or_default();
        matches!(hex, [b'd' | b'D', b'8'..=b'9' | b'a'..=b'f' | b'A'..=b'F'])
    })
}

/// El límite de anidamiento del parser portado no es el de CPython.
fn deep(text: &str) -> bool {
    text.bytes().filter(|b| matches!(b, b'[' | b'{')).count() >= MAX_WORKSPACE_JSON_DEPTH
}

fn text(bytes: Vec<u8>) -> Result<String, Unsure> {
    String::from_utf8(bytes).map_err(|_| Unsure)
}

/// `os.path.realpath(path)` como texto.
fn real(path: &str) -> Result<String, Unsure> {
    text(realpath(path.as_bytes()))
}

/// `os.path.abspath(path)` de una ruta absoluta (las de este módulo lo son:
/// una relativa dependería del directorio de trabajo del proceso).
fn abspath(path: &str) -> Result<String, Unsure> {
    if !path.starts_with('/') {
        return Err(Unsure);
    }
    text(normpath(path.as_bytes()))
}

/// `os.path.join(a, b)` con `b` relativo.
fn join(a: &str, b: &str) -> String {
    if a.is_empty() || a.ends_with('/') {
        format!("{a}{b}")
    } else {
        format!("{a}/{b}")
    }
}

/// `os.path.expanduser(path)` con `home` (el `HOME` del proceso); `~usuario`
/// no se reproduce.
pub fn expanduser(path: &str, home: &str) -> Result<String, Unsure> {
    let Some(rest) = path.strip_prefix('~') else {
        return Ok(path.to_owned());
    };
    if !(rest.is_empty() || rest.starts_with('/')) {
        return Err(Unsure);
    }
    // `posixpath.expanduser`: `userhome.rstrip("/")` y, si queda vacío, "/".
    let trimmed = home.trim_end_matches('/');
    let base = if trimmed.is_empty() { "/" } else { trimmed };
    Ok(if rest.is_empty() {
        base.to_owned()
    } else if base == "/" {
        rest.to_owned()
    } else {
        format!("{base}{rest}")
    })
}

/// `ensure_cwd_trusted(cwd, config_dir=…, home=…)`: marca la carpeta en el
/// `.claude.json` del HOME (y en el del `config_dir` si es otro archivo).
/// Devuelve si el del HOME quedó marcado.
pub fn ensure_cwd_trusted(cwd: &str, config_dir: Option<&str>, home: &str) -> Result<bool, Unsure> {
    let cwd = expanduser(cwd, home)?;
    if !cwd.starts_with('/') || !Path::new(&cwd).is_dir() {
        return Ok(false);
    }
    let cwd = real(&cwd)?;
    let home_root = real(&expanduser(home, home)?)?;
    if cwd == home_root {
        return Ok(false);
    }
    let home_json = join(&home_root, ".claude.json");
    let ok = stamp(&home_json, &cwd)?;
    if let Some(config_dir) = config_dir.filter(|c| !c.is_empty()) {
        let cfg_json = join(&real(&expanduser(config_dir, home)?)?, ".claude.json");
        if real(&cfg_json)? != real(&home_json)? {
            stamp(&cfg_json, &cwd)?;
        }
    }
    Ok(ok)
}

/// `_stamp(path, cwd)`: directorio, candado y `_stamp_locked`. Toda excepción
/// es `False`.
fn stamp(path: &str, cwd: &str) -> Result<bool, Unsure> {
    let directory = dirname(path.as_bytes());
    if !directory.is_empty() {
        let directory = PathBuf::from(std::ffi::OsStr::from_bytes(&directory));
        if fs::create_dir_all(&directory).is_err() {
            return Ok(false);
        }
    }
    // `open(lock_path, "a+")`: 0666 menos la umask si es nuevo.
    let lock = match fs::OpenOptions::new()
        .read(true)
        .append(true)
        .create(true)
        .open(format!("{path}.lock"))
    {
        Ok(file) => file,
        Err(_) => return Ok(false),
    };
    if lock.lock().is_err() {
        return Ok(false);
    }
    let result = stamp_locked(Path::new(path), cwd);
    // `flock(LOCK_UN)` antes de cerrar, como el `finally` del Python.
    let _ = lock.unlock();
    match result {
        Ok(stamped) => Ok(stamped),
        Err(Stamp::Unsure) => Err(Unsure),
        Err(Stamp::Raised) => Ok(false),
    }
}

enum Stamp {
    Unsure,
    /// Una excepción que el `except Exception` de `_stamp` traga.
    Raised,
}

/// `_stamp_locked(path, cwd)`.
fn stamp_locked(path: &Path, cwd: &str) -> Result<bool, Stamp> {
    let mut data = match read_json(path).map_err(|_| Stamp::Unsure)? {
        Read::Missing => Map::new(),
        Read::Failed => return Ok(false),
        Read::Value(Value::Object(map)) => map,
        Read::Value(_) => return Ok(false),
    };
    let mut projects = match data.get("projects") {
        Some(Value::Object(map)) => map.clone(),
        _ => Map::new(),
    };
    let mut entry = match projects.get(cwd) {
        Some(Value::Object(map)) => map.clone(),
        _ => Map::new(),
    };
    if entry.get("hasTrustDialogAccepted") == Some(&Value::Bool(true)) {
        return Ok(true);
    }
    entry.insert("hasTrustDialogAccepted".into(), Value::Bool(true));
    projects.insert(cwd.to_owned(), Value::Object(entry));
    data.insert("projects".into(), Value::Object(projects));
    let mut body = indent_dumps(&Value::Object(data), 2, true).map_err(|_| Stamp::Unsure)?;
    body.push('\n');
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    // `open(tmp, "w")` y `os.replace`. Desviación deliberada (seguridad): el
    // Python deja el archivo con el modo del temporal (0666 menos la umask, o
    // el de un `.tmp` viejo), así un `~/.claude.json` 0600 acababa legible por
    // todos. Aquí el temporal nace de cero (`open_temp`) ya con el modo del
    // archivo original; un archivo nuevo nace como en el Python.
    let original = fs::metadata(path)
        .ok()
        .map(|meta| meta.permissions().mode() & 0o7777);
    let _ = fs::remove_file(&tmp);
    let tmp = PathBuf::from(tmp);
    let written = open_temp(&tmp, original)
        .and_then(|mut file| file.write_all(body.as_bytes()))
        .and_then(|()| fs::rename(&tmp, path));
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written.map_err(|_| Stamp::Raised)?;
    Ok(true)
}

/// El temporal de `_stamp_locked`, creado en exclusiva y YA con su modo: el del
/// archivo original (`original`), sin ventana en la que otro lo lea con el modo
/// por omisión; sin original, el de `open(tmp, "w")` (0666 menos la umask).
/// La umask puede quitar bits al crear: se devuelven sobre el descriptor antes
/// de escribir nada, y nunca queda más abierto que el original.
fn open_temp(tmp: &Path, original: Option<u32>) -> io::Result<fs::File> {
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(original.unwrap_or(0o666))
        .open(tmp)?;
    if let Some(mode) = original {
        file.set_permissions(fs::Permissions::from_mode(mode))?;
    }
    Ok(file)
}

/// `_ancestors_until_git(cwd, home)`.
fn ancestors_until_git(cwd: &str, home: &str) -> Result<Vec<String>, Unsure> {
    let mut out = Vec::new();
    let mut cur = abspath(cwd)?;
    let home = abspath(home)?;
    loop {
        out.push(cur.clone());
        let parent = text(dirname(cur.as_bytes()))?;
        if Path::new(&join(&cur, ".git")).exists() || cur == home || cur == parent {
            return Ok(out);
        }
        cur = parent;
    }
}

/// `_trusted_in_file(path, cwd, home)`: `Ok(None)` es la excepción que sale
/// del `any()` (fuera del `try`) y que quien llama convierte en `False`.
fn trusted_in_file(path: &str, cwd: &str, home: &str) -> Result<Option<bool>, Unsure> {
    let data = match read_json(Path::new(path))? {
        Read::Value(value) => value,
        Read::Missing | Read::Failed => return Ok(Some(false)),
    };
    // `(json.load(f) or {}).get("projects") or {}`: un valor verdadero que no
    // es objeto lanza dentro del `try` (`False`).
    let projects = if !truthy(&data) {
        Value::Null
    } else {
        match &data {
            Value::Object(map) => map.get("projects").cloned().unwrap_or(Value::Null),
            _ => return Ok(Some(false)),
        }
    };
    if !truthy(&projects) {
        return Ok(Some(false));
    }
    let Value::Object(projects) = projects else {
        // `.get` de algo que no es `dict` en el `any()`: excepción.
        return Ok(None);
    };
    for folder in ancestors_until_git(cwd, home)? {
        match projects.get(&folder) {
            None => {}
            Some(entry) if !truthy(entry) => {}
            Some(Value::Object(entry)) => {
                if entry.get("hasTrustDialogAccepted").is_some_and(truthy) {
                    return Ok(Some(true));
                }
            }
            Some(_) => return Ok(None),
        }
    }
    Ok(Some(false))
}

/// `cwd_trusted_in(cwd, config_dir=…, home=…)`; `Ok(None)` = la excepción.
pub fn cwd_trusted_in(
    cwd: &str,
    config_dir: Option<&str>,
    home: &str,
) -> Result<Option<bool>, Unsure> {
    let mut files = vec![join(home, ".claude.json")];
    if let Some(config_dir) = config_dir.filter(|c| !c.is_empty()) {
        files.push(join(config_dir, ".claude.json"));
    }
    for file in files {
        match trusted_in_file(&file, cwd, home)? {
            Some(false) => {}
            other => return Ok(other),
        }
    }
    Ok(Some(false))
}

/// `inherit_cwd_trust(cwd, source_config_dir=…, dest_config_dir=…, home=…)`:
/// solo marca el destino si el origen ya aceptaba la carpeta (o un ancestro
/// hasta la raíz git). `Ok(None)` = una excepción del Python (quien llama la
/// traga como `False`).
pub fn inherit_cwd_trust(
    cwd: &str,
    source_config_dir: Option<&str>,
    dest_config_dir: &str,
    home: &str,
) -> Result<Option<bool>, Unsure> {
    if abspath(cwd)? == abspath(home)? {
        return Ok(Some(false));
    }
    match cwd_trusted_in(cwd, source_config_dir, home)? {
        Some(true) => {}
        other => return Ok(other),
    }
    ensure_cwd_trusted(cwd, Some(dest_config_dir), home)?;
    Ok(Some(true))
}

/// Antes de cualquier efecto: ¿leería `inherit_cwd_trust` algún archivo que
/// el port no reproduce (`Unsure`)? Lee (sin escribir nada) los
/// `.claude.json` del HOME, del origen y del destino.
pub fn probe(
    source_config_dir: Option<&str>,
    dest_config_dir: &str,
    home: &str,
) -> Result<(), Unsure> {
    let mut files = vec![
        join(home, ".claude.json"),
        join(&real(&expanduser(home, home)?)?, ".claude.json"),
        join(&real(&expanduser(dest_config_dir, home)?)?, ".claude.json"),
    ];
    if let Some(source) = source_config_dir.filter(|s| !s.is_empty()) {
        files.push(join(source, ".claude.json"));
    }
    for file in files {
        read_json(Path::new(&file))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::open_temp;
    use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};

    struct Dir(PathBuf);
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn mode(path: &std::path::Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o7777
    }

    /// El temporal nace YA con el modo del original (ni un instante 0644
    /// antes de escribir); sin original, con el de `open(tmp, "w")`.
    #[test]
    fn temp_is_born_with_the_original_mode() {
        let dir = Dir(std::env::temp_dir().join(crate::fresh_id("trust-tmp").unwrap()));
        fs::create_dir_all(&dir.0).unwrap();
        for original in [0o600, 0o640, 0o400] {
            let tmp = dir.0.join(format!("{original:o}.tmp"));
            let file = open_temp(&tmp, Some(original)).unwrap();
            assert_eq!(mode(&tmp), original, "recién creado, antes de escribir");
            drop(file);
        }
        // Sin original: lo que daría `open(tmp, "w")` con la misma umask.
        let reference = dir.0.join("referencia");
        drop(fs::File::create(&reference).unwrap());
        let tmp = dir.0.join("nuevo.tmp");
        drop(open_temp(&tmp, None).unwrap());
        assert_eq!(mode(&tmp), mode(&reference));
        // Un temporal que ya existe no se reutiliza (creación exclusiva).
        assert!(open_temp(&tmp, Some(0o600)).is_err());
    }
}

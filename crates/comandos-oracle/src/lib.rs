//! Dorados de prueba. Reproducir nunca ejecuta el oráculo ni tolera un caso ausente.
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Component, Path, PathBuf},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Record,
    Replay,
    Check,
}
impl Mode {
    fn environment() -> Result<Self, String> {
        match std::env::var("COMANDOS_ORACLE").as_deref() {
            Ok("record") => Ok(Self::Record),
            Ok("check") => Ok(Self::Check),
            Ok("replay") | Err(std::env::VarError::NotPresent) => Ok(Self::Replay),
            _ => Err("COMANDOS_ORACLE debe ser record, replay o check".into()),
        }
    }
}
pub fn oracle(name: &str, input: &Value, run: impl FnOnce() -> Result<Vec<u8>, String>) -> Vec<u8> {
    let root = std::env::current_dir()
        .unwrap_or_else(|e| panic!("oráculo: {e}"))
        .join("tests/golden");
    oracle_at(&root, name, input, run)
}
pub fn oracle_at(
    root: &Path,
    name: &str,
    input: &Value,
    run: impl FnOnce() -> Result<Vec<u8>, String>,
) -> Vec<u8> {
    try_oracle_at(root, name, input, run).unwrap_or_else(|e| panic!("{e}"))
}
pub fn try_oracle_at(
    root: &Path,
    name: &str,
    input: &Value,
    run: impl FnOnce() -> Result<Vec<u8>, String>,
) -> Result<Vec<u8>, String> {
    try_oracle_at_with_mode(root, name, input, Mode::environment()?, run)
}
pub fn try_oracle_at_with_mode(
    root: &Path,
    name: &str,
    input: &Value,
    mode: Mode,
    run: impl FnOnce() -> Result<Vec<u8>, String>,
) -> Result<Vec<u8>, String> {
    if name.is_empty()
        || Path::new(name)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        || name
            .chars()
            .any(|c| !c.is_ascii_alphanumeric() && !matches!(c, '/' | '_' | '-' | '.'))
    {
        return Err("nombre de oráculo inválido".into());
    }
    let key = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(input).map_err(|e| e.to_string())?)
    );
    let path = root.join(name).join(format!(
        "{}.json",
        key.get(..16).ok_or("hash de entrada inválido")?
    ));
    if mode == Mode::Record {
        let output = run()?;
        let encoded = serde_json::to_vec_pretty(
            &json!({"input":input,"output_b64":STANDARD.encode(&output)}),
        )
        .map_err(|e| e.to_string())?;
        write(&path, &encoded)?;
        return Ok(output);
    }
    let raw = fs::read(&path).map_err(|e| {
        format!(
            "falta el dorado de {name}: COMANDOS_ORACLE=record ({}, {e})",
            path.display()
        )
    })?;
    let stored: Value = serde_json::from_slice(&raw)
        .map_err(|e| format!("dorado corrupto {}: {e}", path.display()))?;
    if stored.get("input") != Some(input) {
        return Err(format!(
            "entrada distinta o colisión de dorado: {}",
            path.display()
        ));
    }
    let output = STANDARD
        .decode(
            stored
                .get("output_b64")
                .and_then(Value::as_str)
                .ok_or("dorado sin output_b64")?,
        )
        .map_err(|e| e.to_string())?;
    if mode == Mode::Check && run()? != output {
        return Err(format!("deriva del oráculo {name}: {}", path.display()));
    }
    Ok(output)
}
fn write(path: &Path, body: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("dorado sin padre")?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(parent)
        .map_err(|e| e.to_string())?;
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).map_err(|e| e.to_string())?;
    let tmp = parent.join(format!(
        ".golden-{}.tmp",
        random
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    ));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&tmp)
            .map_err(|e| e.to_string())?;
        file.write_all(body).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        fs::rename(&tmp, path).map_err(|e| e.to_string())?;
        fs::File::open(parent)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(tmp);
    }
    result
}
/// Normaliza solo UTF-8; los binarios permanecen bytes exactos.
pub fn normalize(bytes: &[u8], roots: &[(&str, &Path)]) -> Vec<u8> {
    replace(bytes, roots, false)
}
pub fn restore(bytes: &[u8], roots: &[(&str, &Path)]) -> Vec<u8> {
    replace(bytes, roots, true)
}
fn replace(bytes: &[u8], roots: &[(&str, &Path)], restore: bool) -> Vec<u8> {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return bytes.to_vec();
    };
    let mut text = text.to_owned();
    let mut roots: Vec<_> = roots.iter().collect();
    roots.sort_by_key(|(_, path)| std::cmp::Reverse(path.as_os_str().len()));
    for (token, path) in roots {
        if let Some(path) = path.to_str() {
            if restore {
                text = text.replace(token, path);
            } else {
                text = text.replace(path, token);
            }
        }
    }
    text.into_bytes()
}
/// Identidad determinista de una entrada normalizada, útil para mapas de prueba.
pub fn key(input: &Value) -> Result<String, String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(input).map_err(|e| e.to_string())?)
    ))
}
/// Raíz explícita para evitar depender del cwd del hijo del oráculo.
pub fn golden_root(manifest_dir: &Path) -> PathBuf {
    manifest_dir.join("tests/golden")
}
mod tree;
pub use tree::{restore_tree, snapshot_tree};

/// Graba stdout y efectos del HOME privado; replay conserva las aserciones de archivos.
pub fn text_with_tree_at(
    root: &Path,
    name: &str,
    input: &Value,
    home: &Path,
    roots: &[(&str, &Path)],
    run: impl FnOnce() -> Result<String, String>,
) -> Result<String, String> {
    text_with_tree_at_with_mode(root, name, input, home, roots, Mode::environment()?, run)
}
pub fn text_with_tree_at_with_mode(
    root: &Path,
    name: &str,
    input: &Value,
    home: &Path,
    roots: &[(&str, &Path)],
    mode: Mode,
    run: impl FnOnce() -> Result<String, String>,
) -> Result<String, String> {
    let before = snapshot_tree(home, roots)?;
    let input = json!({"case": input, "before_sha256": key(&before)?});
    let bytes = try_oracle_at_with_mode(root, name, &input, mode, || {
        let stdout = run()?;
        let stdout =
            String::from_utf8(normalize(stdout.as_bytes(), roots)).map_err(|e| e.to_string())?;
        serde_json::to_vec(&json!({"stdout":stdout,"tree":snapshot_tree(home,roots)?}))
            .map_err(|e| e.to_string())
    })?;
    let artifact: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if mode == Mode::Replay {
        tree::apply_delta(
            home,
            &before,
            artifact.get("tree").ok_or("dorado sin árbol")?,
            roots,
        )?;
    }
    let stdout = artifact
        .get("stdout")
        .and_then(Value::as_str)
        .ok_or("dorado sin stdout")?;
    String::from_utf8(restore(stdout.as_bytes(), roots)).map_err(|e| e.to_string())
}

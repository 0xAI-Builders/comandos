use comandos_store::domains::catalog::{
    control_path_identity, file_classification, is_unified_control_file, source,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

pub fn inventory(home: &Path) -> Result<Value, String> {
    let mut files = Vec::new();
    let mut errors = Vec::new();
    let db = control_path_identity(&comandos_store::unified::unified_path(home))
        .map_err(|e| e.to_string())?;
    let mut seen = BTreeSet::new();
    for (prefix, relative) in [
        ("H", ".claude/hooks"),
        ("STATE", ".local/state/comandos"),
        ("SHARE", ".local/share/comandos"),
    ] {
        let dir = home.join(relative);
        if dir.exists() {
            walk(&dir, prefix, &db, &mut files, &mut errors, &mut seen)?;
        }
    }
    // COMANDOS_DB puede estar fuera de las raíces inventariadas. Solo sus
    // archivos de control se inspeccionan, sin recorrer el resto de ese árbol.
    if let Some(parent) = db.parent() {
        match fs::read_dir(parent) {
            Ok(entries) => {
                for entry in entries {
                    let entry = entry.map_err(|e| format!("{}: {e}", parent.display()))?;
                    let path = entry.path();
                    if is_unified_control_file(&path, &db) {
                        let symbolic = format!("CONTROL/{}", entry.file_name().to_string_lossy());
                        record(&path, &symbolic, &db, &mut files, &mut errors, &mut seen)?;
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{}: {e}", parent.display())),
        }
    }
    files.sort_by(|a, b| a["source"].as_str().cmp(&b["source"].as_str()));
    let mut domains: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    let mut unknown = Vec::new();
    for file in &files {
        let group = file["domain"]
            .as_str()
            .unwrap_or_else(|| file["classification"].as_str().unwrap_or("sin-dominio"));
        let counter = domains.entry(group.into()).or_default();
        counter.0 += 1;
        counter.1 += file["size"].as_u64().unwrap_or(0);
        if file["classification"] == "sin-dominio" {
            unknown.push(file["source"].clone());
        }
    }
    let domains: BTreeMap<_, _> = domains
        .into_iter()
        .map(|(name, (count, bytes))| (name, json!({"files":count,"bytes":bytes})))
        .collect();
    Ok(
        json!({"home": home, "files": files, "domains": domains, "sin-dominio":unknown,"errors":errors}),
    )
}

fn walk(
    dir: &Path,
    prefix: &str,
    db: &Path,
    files: &mut Vec<Value>,
    errors: &mut Vec<String>,
    seen: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| format!("nombre no UTF-8 en {}", dir.display()))?;
        let symbolic = format!("{prefix}/{name}");
        let path = entry.path();
        let before = fs::symlink_metadata(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        if before.is_dir() {
            walk(&path, &symbolic, db, files, errors, seen)?;
            continue;
        }
        record(&path, &symbolic, db, files, errors, seen)?;
    }
    Ok(())
}
fn record(
    path: &Path,
    symbolic: &str,
    db: &Path,
    files: &mut Vec<Value>,
    errors: &mut Vec<String>,
    seen: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    let identity = control_path_identity(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !seen.insert(identity) {
        return Ok(());
    }
    let before = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let symlink = before.file_type().is_symlink();
    let control = is_unified_control_file(path, db);
    let spec = if control { None } else { source(symbolic) };
    let class = if control {
        "metadatos-control"
    } else if spec.is_some() {
        "dominio"
    } else {
        file_classification(symbolic)
    };
    let mut value = json!({"path":path,"source":symbolic,"domain":spec.map(|s|s.domain),
            "classification":class,"size":before.len(),"mtime_ms":before.modified().ok().and_then(|t|t.duration_since(UNIX_EPOCH).ok()).map(|d|d.as_millis()),
            "symlink":symlink,"sha256":null,"changed_during_read":false});
    // Los enlaces nunca se siguen: pueden apuntar fuera del HOME o estar rotos.
    if symlink {
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "target".into(),
                json!(fs::read_link(path).map_err(|e| e.to_string())?),
            );
        }
    } else if before.is_file() {
        match hash(path) {
            Ok(digest) => {
                let after = fs::metadata(path);
                let changed = after.as_ref().is_err()
                    || after.as_ref().is_ok_and(|m| {
                        m.len() != before.len() || m.modified().ok() != before.modified().ok()
                    });
                if let Some(object) = value.as_object_mut() {
                    object.insert("changed_during_read".into(), json!(changed));
                    if !changed {
                        object.insert("sha256".into(), json!(digest));
                    }
                }
            }
            Err(error) => {
                errors.push(format!("{}: {error}", path.display()));
            }
        }
    }
    files.push(value);
    Ok(())
}

fn hash(path: &Path) -> Result<String, std::io::Error> {
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        digest.update(&buffer[..n]);
    }
    Ok(format!("{:x}", digest.finalize()))
}
pub fn main(args: &[String]) -> i32 {
    if args.first().map(String::as_str) != Some("inventory") {
        eprintln!("uso: comandos state inventory [--home DIR] [--json]");
        return 2;
    }
    let mut home = std::env::var_os("HOME").map(PathBuf::from);
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--home" if i + 1 < args.len() => {
                i += 1;
                home = Some(args[i].clone().into());
            }
            "--json" => {}
            _ => {
                eprintln!("uso: comandos state inventory [--home DIR] [--json]");
                return 2;
            }
        }
        i += 1;
    }
    let Some(home) = home else {
        eprintln!("HOME no definido");
        return 2;
    };
    match inventory(&home) {
        Ok(value) => {
            println!("{value}");
            i32::from(value["errors"].as_array().is_some_and(|e| !e.is_empty()))
        }
        Err(error) => {
            eprintln!("{error}");
            1
        }
    }
}

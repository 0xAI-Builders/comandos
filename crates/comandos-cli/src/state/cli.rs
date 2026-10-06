use comandos_store::domains::catalog::{
    UnifiedControlFiles, control_path_identity, file_classification, source,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

pub fn inventory(home: &Path) -> Result<Value, String> {
    let mut files = Vec::new();
    let mut errors = Vec::new();
    let controls = UnifiedControlFiles::inspect(&comandos_store::unified::unified_path(home))
        .map_err(|e| e.to_string())?;
    let db = controls.database_path();
    let mut seen = InventorySeen::default();
    for (prefix, relative) in [
        ("H", ".claude/hooks"),
        ("STATE", ".local/state/comandos"),
        ("SHARE", ".local/share/comandos"),
    ] {
        let dir = home.join(relative);
        if dir.exists() {
            walk(&dir, prefix, &controls, &mut files, &mut errors, &mut seen)?;
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
                    if controls
                        .is_control(&path)
                        .map_err(|e| format!("{}: {e}", path.display()))?
                    {
                        let symbolic = format!("CONTROL/{}", entry.file_name().to_string_lossy());
                        record(
                            &path,
                            &symbolic,
                            &controls,
                            &mut files,
                            &mut errors,
                            &mut seen,
                        )?;
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

#[derive(Default)]
struct InventorySeen {
    paths: BTreeSet<PathBuf>,
    control_inodes: BTreeSet<(u64, u64)>,
}

fn walk(
    dir: &Path,
    prefix: &str,
    controls: &UnifiedControlFiles,
    files: &mut Vec<Value>,
    errors: &mut Vec<String>,
    seen: &mut InventorySeen,
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
            walk(&path, &symbolic, controls, files, errors, seen)?;
            continue;
        }
        record(&path, &symbolic, controls, files, errors, seen)?;
    }
    Ok(())
}
fn record(
    path: &Path,
    symbolic: &str,
    controls: &UnifiedControlFiles,
    files: &mut Vec<Value>,
    errors: &mut Vec<String>,
    seen: &mut InventorySeen,
) -> Result<(), String> {
    let identity = control_path_identity(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !seen.paths.insert(identity) {
        return Ok(());
    }
    let before = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let symlink = before.file_type().is_symlink();
    let control = controls
        .is_control(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if control && before.is_file() && !seen.control_inodes.insert((before.dev(), before.ino())) {
        return Ok(());
    }
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
struct Options {
    home: PathBuf,
    dry_run: bool,
    resume: bool,
    domains: Vec<String>,
    proc_root: PathBuf,
    repo: PathBuf,
}
fn parse(args: &[String]) -> Result<(&str, Options), String> {
    let mut args = args.iter();
    let command = args
        .next()
        .map(String::as_str)
        .ok_or("falta operación state")?;
    let mut opts = Options {
        home: std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or("HOME no definido")?,
        dry_run: false,
        resume: false,
        domains: vec![],
        proc_root: PathBuf::from("/proc"),
        repo: std::env::current_dir().map_err(|e| e.to_string())?,
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--home" => opts.home = args.next().ok_or("falta --home")?.into(),
            "--json" => {}
            "--dry-run" if matches!(command, "migrate" | "move") => opts.dry_run = true,
            "--resume" if command == "migrate" => opts.resume = true,
            "--domain" if matches!(command, "migrate" | "verify") => opts
                .domains
                .push(args.next().ok_or("falta --domain")?.clone()),
            "--proc-root" if command == "status" => {
                opts.proc_root = args.next().ok_or("falta --proc-root")?.into()
            }
            "--repo" if command == "status" => {
                opts.repo = args.next().ok_or("falta --repo")?.into()
            }
            _ if matches!(command, "move" | "demote")
                && arg.starts_with("db-")
                && opts.domains.is_empty() =>
            {
                opts.domains.push(arg.clone())
            }
            _ => return Err(format!("opción state inválida: {arg}")),
        }
    }
    Ok((command, opts))
}
fn execute(command: &str, opts: &Options) -> Result<(Value, bool), String> {
    use comandos_store::{migrate, unified};
    let db = unified::unified_path(&opts.home);
    match command {
        "inventory" => {
            let value = inventory(&opts.home)?;
            let failed = value
                .get("errors")
                .and_then(Value::as_array)
                .is_some_and(|e| !e.is_empty());
            Ok((value, failed))
        }
        "migrate" => migrate::migrate(&migrate::MigrateOptions {
            home: opts.home.clone(),
            db,
            dry_run: opts.dry_run,
            resume: opts.resume,
            domains: if opts.domains.is_empty() {
                None
            } else {
                Some(opts.domains.clone())
            },
            now_ms: migrate::journal::now_ms().map_err(|e| e.to_string())?,
        })
        .map(|report| (report.json(), false))
        .map_err(|e| e.to_string()),
        "move" | "demote" => {
            let domain = opts.domains.first().ok_or("falta dominio db-x")?;
            let spec = migrate::spec_for(&opts.home, domain).map_err(|e|e.to_string())?;
            if opts.dry_run {
                let estimate = migrate::move_estimate(&opts.home, &spec, &db).map_err(|e|e.to_string())?;
                return Ok((json!({"domain":domain,"dry_run":true,"bytes":estimate.bytes,"copy_ms":estimate.copy_ms,"budget_ms":4000}),false));
            }
            if command == "demote" {
                migrate::demote_db(&spec, &db).map_err(|e|e.to_string())?;
                return Ok((json!({"domain":domain,"direction":"inverse","source":spec.legacy,"target":db}),false));
            }
            let id=migrate::journal::new_id(migrate::journal::now_ms().map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
            let backup=opts.home.join(".local/share/comandos/backups").join(id);
            migrate::move_db(&spec, &db, &backup,4000).map_err(|e|e.to_string())?;
            Ok((json!({"domain":domain,"source":spec.legacy,"target":db,"backup_dir":backup.exists().then_some(backup)}),false))
        }
        "verify" => {
            if !db.exists() {
                return Err("verify requiere una base existente".into());
            }
            let conn = unified::open_unified(&db).map_err(|e| e.to_string())?;
            let domains = if opts.domains.is_empty() {
                comandos_store::domains::catalog::DOMAINS
                    .iter()
                    .filter(|d| !d.name.starts_with("db-"))
                    .map(|d| d.name.to_owned())
                    .collect()
            } else {
                opts.domains.clone()
            };
            let mut reports = vec![];
            let mut failed = false;
            for name in domains {
                let report =
                    migrate::verify(&opts.home, &conn, &name).map_err(|e| e.to_string())?;
                migrate::journal::record_verify(
                    &conn,
                    &report,
                    migrate::journal::now_ms().map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
                failed |= !report.mismatches.is_empty();
                reports.push(report.json());
            }
            Ok((json!(reports), failed))
        }
        "backups" => {
            let value = migrate::backup::list(&opts.home).map_err(|e| e.to_string())?;
            let failed = value.as_array().is_some_and(|entries| {
                entries
                    .iter()
                    .any(|e| e.get("verified").and_then(Value::as_bool) != Some(true))
            });
            Ok((value, failed))
        }
        "status" => {
            let mut value = migrate::journal::status(&opts.home, &db).map_err(|e| e.to_string())?;
            if let Some(rows) = value.get_mut("domains").and_then(Value::as_array_mut) {
                for row in rows {
                    let domain = row
                        .get("domain")
                        .and_then(Value::as_str)
                        .ok_or("status sin dominio")?;
                    let writers = super::preflight::domain_writers(
                        &opts.proc_root,
                        &opts.home,
                        &opts.repo,
                        domain,
                    );
                    let writers=writers.iter().map(|w|json!({"pid":w.pid,"exe":w.exe,"argv":w.argv,"protocol":w.protocol,"python_repo":w.python_repo,"release":w.exe.parent()})).collect::<Vec<_>>();
                    row.as_object_mut()
                        .ok_or("status inválido")?
                        .insert("writers".into(), json!(writers));
                }
            }
            Ok((value, false))
        }
        _ => Err(
            "uso: comandos state inventory|migrate|move db-x|demote db-x|verify|status|backups [--home DIR] [--json]"
                .into(),
        ),
    }
}
pub fn main(args: &[String]) -> i32 {
    if args.first().is_some_and(|s| s == "drill") {
        return super::drill::main(args.get(1..).unwrap_or_default());
    }
    if super::lifecycle::handles(args) {
        return super::lifecycle::main(args);
    }
    match parse(args).and_then(|(command, opts)| execute(command, &opts)) {
        Ok((value, failed)) => {
            println!("{value}");
            i32::from(failed)
        }
        Err(error) => {
            eprintln!("{error}");
            1
        }
    }
}

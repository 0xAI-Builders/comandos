//! Ensayo completo dentro de un HOME nuevo; el origen solo se lee.
use comandos_store::{migrate, unified};
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, symlink},
    path::{Path, PathBuf},
};
struct Owned {
    root: PathBuf,
    keep: bool,
}
impl Drop for Owned {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}
pub fn run(source: &Path, keep: bool) -> Result<String, String> {
    if std::env::var_os("COMANDOS_DB").is_some_and(|v| !v.is_empty()) {
        return Err(
            "state-drill requiere COMANDOS_DB sin definir; xtask lo aísla en un proceso privado"
                .into(),
        );
    }
    let now = migrate::journal::now_ms().map_err(|e| e.to_string())?;
    let root = std::env::temp_dir().join(format!(
        "comandos-state-drill-{}",
        migrate::journal::new_id(now).map_err(|e| e.to_string())?
    ));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&root)
        .map_err(|e| e.to_string())?;
    let owned = Owned {
        root: root.clone(),
        keep,
    };
    let home = root.join("home");
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&home)
        .map_err(|e| e.to_string())?;
    migrate::drill::copy_home(source, &home).map_err(|e| e.to_string())?;
    let path = unified::unified_path(&home);
    let opts = migrate::MigrateOptions {
        home: home.clone(),
        db: path.clone(),
        dry_run: true,
        resume: false,
        domains: None,
        now_ms: now - 86_400_002,
    };
    let dry = migrate::migrate(&opts).map_err(|e| e.to_string())?;
    if path.exists() {
        return Err("dry-run creó una base en HOME".into());
    }
    let report = migrate::migrate(&migrate::MigrateOptions {
        dry_run: false,
        ..opts
    })
    .map_err(|e| e.to_string())?;
    let c = unified::open_unified(&path).map_err(|e| e.to_string())?;
    let names: Vec<_> = comandos_store::domains::catalog::DOMAINS
        .iter()
        .filter(|d| !d.name.starts_with("db-"))
        .map(|d| d.name)
        .collect();
    let mirror = migrate::drill::synthetic_write(&home, &c, "mirror", now - 86_400_001)
        .map_err(|e| e.to_string())?;
    let proc_root = root.join("proc");
    fs::create_dir(&proc_root).map_err(|e| e.to_string())?;
    for name in &names {
        super::preflight::can_unify(&super::preflight::domain_writers(
            &proc_root, &home, &root, name,
        ))
        .map_err(|e| e.join("; "))?;
        for time in [now - 86_400_001, now - 43_200_001, now - 1] {
            let r = migrate::verify(&home, &c, name).map_err(|e| e.to_string())?;
            if !r.mismatches.is_empty() {
                return Err(format!("verify mirror {name}: {:?}", r.mismatches));
            }
            migrate::journal::record_verify(&c, &r, time).map_err(|e| e.to_string())?;
        }
        migrate::lifecycle::flip(&home, &c, name, now).map_err(|e| e.to_string())?;
    }
    let unified_writes = migrate::drill::synthetic_write(&home, &c, "unified", now + 1)
        .map_err(|e| e.to_string())?;
    let mut moved = vec![];
    for name in [
        "db-operator",
        "db-news",
        "db-operations",
        "db-app-state",
        "db-usage",
    ] {
        let spec = migrate::spec_for(&home, name).map_err(|e| e.to_string())?;
        if spec.legacy.exists() {
            let backup = home.join(".local/share/comandos/backups").join(format!(
                "{}-{name}",
                migrate::journal::new_id(now).map_err(|e| e.to_string())?
            ));
            migrate::move_db(&spec, &path, &backup, 4000).map_err(|e| e.to_string())?;
            moved.push(name);
        }
    }
    if moved.contains(&"db-operator") {
        c.execute(
            "UPDATE operator_actions SET tool='state-drill-unified-write'",
            [],
        )
        .map_err(|e| e.to_string())?;
    }
    let releases = home.join(".local/share/comandos/releases");
    let bin = home.join(".local/share/comandos/bin");
    fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
    for (id, protocol) in [("aaaaaaaaaaaa", 0), ("bbbbbbbbbbbb", 2)] {
        let dir = releases.join(id);
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        fs::write(dir.join("comandos"), b"private fixture release\n").map_err(|e| e.to_string())?;
        fs::write(
            dir.join("manifest.json"),
            format!("{{\"state_protocol\":{protocol}}}"),
        )
        .map_err(|e| e.to_string())?;
    }
    fs::write(releases.join("previous"), b"aaaaaaaaaaaa\n").map_err(|e| e.to_string())?;
    symlink(releases.join("bbbbbbbbbbbb/comandos"), bin.join("comandos"))
        .map_err(|e| e.to_string())?;
    crate::install::release::rollback_release(&home)?;
    for name in &names {
        if unified::mode_of(Some(&c), name).map_err(|e| e.to_string())? != unified::Mode::Mirror {
            return Err(format!("rollback-release no degradó {name}"));
        }
        let r = migrate::verify(&home, &c, name).map_err(|e| e.to_string())?;
        if !r.mismatches.is_empty() {
            return Err(format!("lector legado difiere {name}: {:?}", r.mismatches));
        }
    }
    migrate::lifecycle::demote_all(&home, &c, now + 2).map_err(|e| e.to_string())?;
    drop(c);
    let rollback = migrate::lifecycle::rollback(&home, &path, &report.run_id, now + 3)
        .map_err(|e| e.to_string())?;
    let initial = migrate::backup::load(
        &report
            .backup_dir
            .ok_or("migración sin respaldo")?
            .join("manifest.json"),
    )
    .map_err(|e| e.to_string())?;
    for entry in initial.entries {
        if fs::read(&entry.source).map_err(|e| e.to_string())?
            != fs::read(&entry.copy).map_err(|e| e.to_string())?
        {
            return Err("rollback no reprodujo fuente inicial".into());
        }
    }
    let text = format!(
        "# Ensayo de estado\n\nOrigen leído: {}\n\nHOME privado: {}\n\nDry-run: {} pasos, sin base creada. Migración: {}.\n\nEscrituras mirror: {mirror}; unified: {unified_writes}. {} dominios cambiados con censo privado y tres verificaciones sintéticas separadas por 24 horas.\n\nSQLite trasladados y degradados: {:?}. Rollback de release protocolo 0: verificado. Lectores legado: bytes idénticos. Demote --all: verificado. Rollback al respaldo: bytes originales restaurados; base conservada en {}.\n\nLas fechas sintéticas solo pertenecen a este ensayo privado. No demuestra las puertas temporales ni el cutover en vivo.\n",
        source.display(),
        home.display(),
        dry.steps.len(),
        report.run_id,
        names.len(),
        moved,
        rollback.archived_db.display()
    );
    drop(owned);
    Ok(text)
}
pub fn main(args: &[String]) -> i32 {
    let mut args = args.iter();
    let mut source = None;
    let mut keep = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--source-home" => source = args.next().map(PathBuf::from),
            "--keep" => keep = true,
            _ => {
                eprintln!("uso: state drill --source-home DIR [--keep]");
                return 1;
            }
        }
    }
    match source
        .ok_or("falta --source-home".into())
        .and_then(|s| run(&s, keep))
    {
        Ok(report) => {
            print!("{report}");
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

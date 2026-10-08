//! User-operated maintenance. Default and --dry-run inspect; mutation is explicit.
use super::{Result, batch, install, protocol::Control, release, reports::Reports, runtime::Local};
use comandos_store::unified::Mode;
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    ffi::OsStr,
    fs,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
pub(super) struct Cancellation {
    pub(super) flag: Arc<AtomicBool>,
    signals: Vec<signal_hook::SigId>,
}
impl Cancellation {
    pub(super) fn new() -> Result<Self> {
        let flag = Arc::new(AtomicBool::new(false));
        let mut own = Self {
            flag,
            signals: vec![],
        };
        for signal in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
            own.signals.push(
                signal_hook::flag::register(signal, own.flag.clone()).map_err(|e| e.to_string())?,
            );
        }
        Ok(own)
    }
}
impl Drop for Cancellation {
    fn drop(&mut self) {
        for id in &self.signals {
            signal_hook::low_level::unregister(*id);
        }
    }
}
struct Lease(fs::File);
impl Drop for Lease {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}
fn lease() -> Result<Lease> {
    let root = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .ok_or("--apply requiere XDG_RUNTIME_DIR privado")?;
    let m = root.symlink_metadata().map_err(|e| e.to_string())?;
    if !root.is_absolute()
        || !m.is_dir()
        || m.mode() & 0o777 != 0o700
        || m.uid() != nix::unistd::geteuid().as_raw()
    {
        return Err("XDG_RUNTIME_DIR debe ser directorio propio 0700 sin symlink".into());
    }
    let dir = root.join("comandos");
    match dir.symlink_metadata() {
        Ok(m)
            if m.is_dir()
                && m.mode() & 0o777 == 0o700
                && m.uid() == nix::unistd::geteuid().as_raw() => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            fs::DirBuilder::new()
                .mode(0o700)
                .create(&dir)
                .map_err(|e| e.to_string())?;
        }
        _ => return Err("runtime ComandOS no privado".into()),
    };
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(
            (nix::fcntl::OFlag::O_NOFOLLOW
                | nix::fcntl::OFlag::O_NONBLOCK
                | nix::fcntl::OFlag::O_NOCTTY)
                .bits(),
        )
        .open(dir.join("codex-full-access.lock"))
        .map_err(|e| e.to_string())?;
    let m = file.metadata().map_err(|e| e.to_string())?;
    if !m.is_file()
        || m.nlink() != 1
        || m.mode() & 0o777 != 0o600
        || m.uid() != nix::unistd::geteuid().as_raw()
    {
        return Err("lease de mantenimiento no privada".into());
    }
    file.try_lock()
        .map_err(|_| "Ya hay un lote Codex en curso".to_string())?;
    Ok(Lease(file))
}
fn stamp() -> Result<(String, i64)> {
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?;
    Ok((
        format!("{}.json", d.as_nanos()),
        i64::try_from(d.as_millis()).map_err(|e| e.to_string())?,
    ))
}
fn reports(home: &Path) -> Result<Reports<'_>> {
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/state"));
    Reports::new(home, state.join("comandos/codex-full-access"))
}
fn perform_release(
    plan: &mut Value,
    cancel: Arc<AtomicBool>,
    checkpoint: &mut dyn FnMut(&Value) -> Result<()>,
) -> Result<()> {
    let snapshot = plan.clone();
    let account = PathBuf::from(release::string(plan, "home")?);
    let sid = release::string(plan, "sid")?.to_owned();
    release::release_with(
        plan,
        || release::writer_locked(&account, &sid),
        || Control::open(&snapshot, Duration::from_secs(30), cancel),
        |p| checkpoint(p),
    )?;
    Ok(())
}
pub(super) fn thread(args: &[String]) -> Result<i32> {
    let mut path = None;
    let mut dry = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--plan" => {
                i += 1;
                path = Some(PathBuf::from(args.get(i).ok_or("--plan sin ruta")?));
            }
            "--dry-run" => dry = true,
            o => return Err(format!("opción desconocida: {o}")),
        }
        i += 1;
    }
    let mut plan =
        super::protocol::read_plan(&path.ok_or("thread-release requiere --plan RUTA_ABSOLUTA")?)?;
    release::validate(&plan)?;
    // The supplied plan is input only. Dedicated recovery records are excluded
    // from full-access retries because releasing a writer is not a resumed turn.
    let home = install::home()?;
    let reports = reports(&home)?;
    reports.mode()?;
    if dry {
        println!("{}",serde_json::to_string(&json!({"dryRun":true,"plan":plan,"writerLocked":release::writer_locked(Path::new(release::string(&plan,"home")?),release::string(&plan,"sid")?)?})).map_err(|e|e.to_string())?);
        return Ok(0);
    }
    if plan["releaseRecovery"]["pending"] != true
        && !release::writer_locked(
            Path::new(release::string(&plan, "home")?),
            release::string(&plan, "sid")?,
        )?
    {
        println!("{}", release::string(&plan, "transcript")?);
        return Ok(0);
    }
    let cancellation = Cancellation::new()?;
    let _lease = lease()?;
    let (key, ms) = stamp()?;
    reports.save(
        &key,
        &json!({"operation":"thread-release","plans":[plan.clone()],"results":[]}),
        ms,
    )?;
    let outcome = perform_release(&mut plan, cancellation.flag.clone(), &mut |p| {
        reports.save(
            &key,
            &json!({"operation":"thread-release","plans":[p],"results":[]}),
            stamp()?.1,
        )
    });
    let result = match &outcome {
        Ok(()) => json!({"sid":plan["sid"],"status":"released"}),
        Err(e) => json!({"sid":plan["sid"],"status":"failed","error":e}),
    };
    reports.save(
        &key,
        &json!({"operation":"thread-release","plans":[plan.clone()],"results":[result]}),
        stamp()?.1,
    )?;
    println!("Recuperación: {}", reports.directory().join(key).display());
    outcome?;
    println!("{}", release::string(&plan, "transcript")?);
    Ok(0)
}
pub(super) fn full(args: &[String]) -> Result<i32> {
    let mut apply = false;
    let mut only = false;
    let mut retry = false;
    let mut retry_path = None;
    let mut proc = PathBuf::from("/proc");
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--apply" => apply = true,
            "--install-only" => only = true,
            "--dry-run" => {}
            "--retry-failed" => retry = true,
            "--retry-report" | "--proc-root" => {
                let k = &args[i];
                i += 1;
                let value = args
                    .get(i)
                    .ok_or("uso: comandos codex full-access: opción sin ruta")?;
                if k == "--proc-root" {
                    proc = PathBuf::from(value);
                } else {
                    retry_path = Some(PathBuf::from(value));
                }
            }
            o => {
                return Err(format!(
                    "uso: comandos codex full-access: opción desconocida: {o}"
                ));
            }
        }
        i += 1;
    }
    if retry && retry_path.is_some() {
        return Err("uso: comandos codex full-access: --retry-failed y --retry-report son mutuamente excluyentes".into());
    }
    if args.iter().any(|s| s == "--dry-run") && (apply || only) {
        return Err(
            "uso: comandos codex full-access: --dry-run no se combina con opciones de aplicación"
                .into(),
        );
    }
    let home = install::home()?;
    let reports = reports(&home)?;
    reports.mode()?;
    if only {
        let install = install::install(&home, None, false)?;
        println!(
            "Regla YOLO permanente instalada: {}",
            release::string(&install, "wrapper")?
        );
        println!(
            "Lanzador original conservado: {}",
            release::string(&install, "original")?
        );
        return Ok(0);
    }
    let previous = if let Some(path) = retry_path {
        Some((path.clone(), reports.read_path(&path)?))
    } else if retry {
        Some(reports.latest()?)
    } else {
        None
    };
    if previous
        .as_ref()
        .is_some_and(|(_, v)| v["operation"] == "thread-release")
    {
        return Err("el informe es de liberación, no de reanudación full-access".into());
    }
    let baseline = previous
        .as_ref()
        .map(|(_, v)| {
            v["results"]
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .filter(|r| r["status"] == "confirmed")
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let pending = previous.as_ref().map(|(_, v)| {
        v["plans"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter(|p| !baseline.iter().any(|r| r["pane"] == p["pane"]))
            .cloned()
            .collect::<Vec<_>>()
    });
    if let Some(rows) = &pending {
        println!(
            "Informe anterior: {}",
            previous.as_ref().ok_or("informe ausente")?.0.display()
        );
        println!(
            "{} sesiones ya confirmadas; {} pendientes.",
            baseline.len(),
            rows.len()
        );
        if rows.is_empty() {
            return Ok(0);
        }
    }
    let cancellation = Cancellation::new()?;
    let mut rt = Local::new(home.clone(), proc, cancellation.flag.clone())?;
    let (mut plans, failures) = rt.inventory(pending.as_deref())?;
    if let Some(rows) = pending
        && failures.is_empty()
    {
        plans = batch::select_retry(&rows, &plans, &mut rt)?;
    }
    for p in &plans {
        println!(
            "{} {} · {} · {}",
            release::string(p, "session")?,
            release::string(p, "pane")?,
            release::string(p, "sid")?,
            release::string(p, "home")?
        );
    }
    if !failures.is_empty() {
        for error in failures {
            println!("SIN CAMBIOS: {error}");
        }
        println!("El lote no empezó: falta identificar todas las sesiones YOLO.");
        return Ok(1);
    }
    println!("{} panes Codex YOLO identificados.", plans.len());
    if !apply {
        return Ok(0);
    }
    if let Some(plan) = plans.first() {
        let help = super::process::run_when(
            Path::new(release::string(plan, "binary")?),
            &[OsStr::new("resume"), OsStr::new("--help")],
            None,
            Duration::from_secs(15),
            &cancellation.flag,
        )?;
        if !help.status.success() || !String::from_utf8_lossy(&help.stdout).contains("--no-daemon")
        {
            return Err("Esta versión de Codex no admite --no-daemon; el lote no empezó".into());
        }
    }
    let _lease = lease()?;
    if plans.is_empty() {
        let install = install::install(&home, None, false)?;
        println!(
            "Regla YOLO permanente instalada: {}",
            release::string(&install, "wrapper")?
        );
        return Ok(0);
    }
    let cohort = RefCell::new(
        previous
            .as_ref()
            .map(|(_, v)| v["plans"].as_array().cloned().unwrap_or_default())
            .unwrap_or_else(|| plans.clone()),
    );
    for p in &plans {
        if let Some(old) = cohort
            .borrow_mut()
            .iter_mut()
            .find(|old| old["pane"] == p["pane"])
        {
            *old = p.clone();
        }
    }
    let completed = RefCell::new(vec![]);
    let (key, ms) = stamp()?;
    let save = || -> Result<()> {
        let mut combined = vec![];
        for p in cohort.borrow().iter() {
            if let Some(result) = completed
                .borrow()
                .iter()
                .chain(baseline.iter())
                .find(|r: &&Value| r["pane"] == p["pane"])
            {
                combined.push(result.clone());
            }
        }
        let mut payload = json!({"plans":*cohort.borrow(),"results":combined});
        if let Some((path, _)) = &previous {
            payload["previousReport"] = json!(path);
        }
        reports.save(&key, &payload, stamp()?.1)
    };
    let mut first = json!({"plans":*cohort.borrow(),"results":baseline});
    if let Some((path, _)) = &previous {
        first["previousReport"] = json!(path);
    }
    reports.save(&key, &first, ms)?;
    let _legacy_lock = if reports.mode()? != Mode::Sealed {
        Some(
            comandos_store::files::FileLock::try_exclusive(&reports.directory().join("batch.lock"))
                .map_err(|e| e.to_string())?
                .ok_or("Ya hay un lote Codex legacy en curso")?,
        )
    } else {
        None
    };
    let install = install::install(&home, None, false)?;
    println!(
        "Regla YOLO permanente instalada: {}",
        release::string(&install, "wrapper")?
    );
    println!(
        "Plan y recuperación: {}",
        reports.directory().join(&key).display()
    );
    for mut p in plans {
        let pane = release::string(&p, "pane")?.to_owned();
        let result = batch::restart(
            &mut rt,
            &mut p,
            |p, checkpoint| perform_release(p, cancellation.flag.clone(), checkpoint),
            &mut |p| {
                if let Some(old) = cohort
                    .borrow_mut()
                    .iter_mut()
                    .find(|old| old["pane"] == p["pane"])
                {
                    *old = p.clone();
                }
                save()
            },
        );
        if let Some(old) = cohort
            .borrow_mut()
            .iter_mut()
            .find(|old| old["pane"] == pane)
        {
            *old = p.clone();
        }
        let mut result = match result {
            Ok(r) => r,
            Err(e) => json!({"status":"failed","error":e}),
        };
        result["pane"] = json!(pane);
        println!(
            "{pane}: {}{}",
            release::string(&result, "status")?,
            result["error"]
                .as_str()
                .map(|e| format!(" · {e}"))
                .unwrap_or_default()
        );
        completed.borrow_mut().push(result);
        save()?;
        use batch::Runtime;
        if rt.cancelled() {
            break;
        }
    }
    let confirmed = baseline.len()
        + completed
            .borrow()
            .iter()
            .filter(|r| r["status"] == "confirmed")
            .count();
    let count = cohort.borrow().len();
    println!(
        "Acceso total confirmado y continua enviado: {confirmed}/{count}. Informe: {}",
        reports.directory().join(&key).display()
    );
    Ok(if confirmed == count { 0 } else { 1 })
}

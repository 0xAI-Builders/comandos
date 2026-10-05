//! Sonda de la importación en frío sobre los datos reales (I2 de la revisión
//! final de la 2e). No es una prueba de la suite: va con `#[ignore]` y además
//! pide `COMANDOS_COLD_IMPORT_PROBE=1`. Corre los importadores `record_local_*`
//! en el orden de la vuelta del frente (Codex → Grok → Claude por cuenta →
//! OpenCode) con el `seen` vacío, como la primera importación tras el cutover,
//! contra los transcripts reales del `HOME`, y escribe en una base de uso
//! nueva dentro de `COMANDOS_PROBE_DIR` (o el directorio temporal).
//!
//! Solo lectura sobre el `HOME`: los importadores solo leen archivos; la base
//! de OpenCode se copia a la carpeta de la sonda y se lee la copia, así que
//! ni siquiera su `-shm` se toca. No abre credenciales ni hace red.
//!
//! Uso (el pico es del proceso entero; `GLIBC_TUNABLES` se lee al arrancar):
//! `COMANDOS_COLD_IMPORT_PROBE=1 [GLIBC_TUNABLES=…] <binario> --ignored --nocapture`.
#![cfg(target_os = "linux")]
use comandos_store::{usage, usage_import};
use rusqlite::Connection;
use std::{
    cell::RefCell,
    collections::HashMap,
    path::{Path, PathBuf},
    process::Command,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

fn status_kib(field: &str) -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let line = status.lines().find(|l| l.starts_with(field)).unwrap();
    line.split_whitespace().nth(1).unwrap().parse().unwrap()
}

/// `git_root_for_path` del frente: `git rev-parse --show-toplevel` con caché
/// por vuelta y la ruta como respaldo.
#[derive(Default)]
struct GitCli(RefCell<HashMap<String, String>>);

impl usage_import::GitRoots for GitCli {
    fn root(&self, path: &str) -> String {
        if let Some(root) = self.0.borrow().get(path) {
            return root.clone();
        }
        let root = if path.is_empty() || !Path::new(path).is_dir() {
            path.to_owned()
        } else {
            Command::new("git")
                .args(["rev-parse", "--show-toplevel"])
                .current_dir(path)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
                .filter(|r| !r.is_empty())
                .unwrap_or_else(|| path.to_owned())
        };
        self.0.borrow_mut().insert(path.to_owned(), root.clone());
        root
    }
}

/// Copia de la base de OpenCode (con su `-wal`) en la carpeta de la sonda,
/// consolidada en modo `DELETE` para que la lectura `mode=ro` no necesite `-shm`.
fn opencode_copy(home: &Path, dir: &Path) -> PathBuf {
    let src = home.join(".local/share/opencode/opencode.db");
    let dest = dir.join("opencode.db");
    if !src.exists() {
        return dest;
    }
    std::fs::copy(&src, &dest).unwrap();
    let wal = PathBuf::from(format!("{}-wal", src.display()));
    if wal.exists() {
        std::fs::copy(&wal, format!("{}-wal", dest.display())).unwrap();
    }
    let conn = Connection::open(&dest).unwrap();
    conn.execute_batch("pragma wal_checkpoint(truncate); pragma journal_mode = delete;")
        .unwrap();
    dest
}

#[test]
#[ignore = "sonda manual sobre los datos reales; pide COMANDOS_COLD_IMPORT_PROBE=1"]
fn cold_import_on_real_data() {
    if std::env::var("COMANDOS_COLD_IMPORT_PROBE").as_deref() != Ok("1") {
        eprintln!("sonda: falta COMANDOS_COLD_IMPORT_PROBE=1; no se hace nada");
        return;
    }
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let base = std::env::var_os("COMANDOS_PROBE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join(format!("cold-import-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let opencode_db = opencode_copy(&home, &dir);
    let db = dir.join("comandos-usage.sqlite");
    let conn = usage::open_usage_db_at(&db).unwrap();
    usage::ensure_schema(&conn).unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let zone = chrono::Local;
    let admit = |_: &Connection| true;
    let plan = usage_import::ImportPlan {
        now,
        max_age_days: 21,
        claude_max_files: None,
        codex_max_files: None,
        home: &home,
        claude_projects_main: None,
        opencode_db,
        zone: &zone,
        admit: &admit,
        cancelled: &|| false,
    };
    let roots = GitCli::default();
    let mut seen = usage_import::ImportSeen::default();
    let tunables = std::env::var("GLIBC_TUNABLES").unwrap_or_default();

    std::fs::write("/proc/self/clear_refs", "5").unwrap();
    let rss_before = status_kib("VmRSS:");
    let started = Instant::now();
    // Cada fase reinicia el pico al Rss de su comienzo: su subida es suya; el
    // pico total es el mayor `VmHWM` visto.
    let mut steps = Vec::new();
    let mut top = rss_before;
    let mut phase: Option<(Instant, u64)> = None;
    let mut mark = |name: Option<&str>, n: usize| {
        let hwm = status_kib("VmHWM:");
        top = top.max(hwm);
        if let (Some(name), Some((at, base))) = (name, phase) {
            steps.push(format!(
                "{name}: {n} turnos, {:.1} s, sube {} KiB sobre {base} KiB",
                at.elapsed().as_secs_f64(),
                hwm.saturating_sub(base)
            ));
        }
        std::fs::write("/proc/self/clear_refs", "5").unwrap();
        phase = Some((Instant::now(), status_kib("VmRSS:")));
        top
    };

    let _ = usage_import::reconcile_orphan_interactions(&conn, now, 14);
    let _ = usage_import::prune_old_turns(&conn, now, 21);
    mark(None, 0);
    let n = usage_import::record_local_codex_rollouts(&conn, &plan, &mut seen, &roots).unwrap();
    mark(Some("codex"), n);
    let grok: Vec<PathBuf> = comandos_runtime::limits::grok_account_homes(&home)
        .into_iter()
        .map(|(_, h)| h)
        .collect();
    let n = usage_import::record_local_grok_updates(&conn, &plan, &grok, &roots).unwrap();
    mark(Some("grok"), n);
    for (alias, account) in usage_import::account_homes(&home, ".claude", ".claude-accounts") {
        let n = usage_import::record_local_claude_jsonl(
            &conn,
            &plan,
            &account.join("projects"),
            &alias,
            &mut seen,
            &roots,
        )
        .unwrap();
        mark(Some(&format!("claude:{alias}")), n);
    }
    let n = usage_import::record_local_opencode_db(&conn, &plan, &roots).unwrap();
    let hwm = mark(Some("opencode"), n);
    let wall = started.elapsed();
    let rss_after = status_kib("VmRSS:");

    let mut counts = Vec::new();
    {
        let mut stmt = conn
            .prepare("select source, count(*) from usage_turns group by source order by source")
            .unwrap();
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    r.get::<_, i64>(1)?,
                ))
            })
            .unwrap();
        for row in rows {
            let (source, count) = row.unwrap();
            counts.push(format!("{source}={count}"));
        }
    }
    let total: i64 = conn
        .query_row("select count(*) from usage_turns", [], |r| r.get(0))
        .unwrap();
    drop(conn);
    eprintln!("sonda importación en frío (GLIBC_TUNABLES={tunables:?})");
    for step in &steps {
        eprintln!("  {step}");
    }
    eprintln!(
        "  archivos leídos {}, turnos {total} ({}), {:.1} s",
        seen.files_read(),
        counts.join(", "),
        wall.as_secs_f64()
    );
    eprintln!(
        "  VmRSS antes {rss_before} KiB, VmHWM {hwm} KiB (+{} KiB), VmRSS después {rss_after} KiB",
        hwm.saturating_sub(rss_before)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

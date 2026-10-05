//! Pruebas de pico de memoria: una base de uso del tamaño de la real en un
//! directorio temporal que se borra también si la prueba falla, y el pico del
//! proceso (`VmHWM`, que se reinicia con `/proc/self/clear_refs`).
use comandos_store::usage;
use rusqlite::params;
use std::path::PathBuf;

/// Directorio temporal propio; se borra al salir de la prueba, también con pánico.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("cmd-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn status_kib(field: &str) -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let line = status.lines().find(|l| l.starts_with(field)).unwrap();
    line.split_whitespace().nth(1).unwrap().parse().unwrap()
}

/// Reinicia el pico del proceso al Rss actual y devuelve ese punto de partida.
pub fn reset_peak() -> u64 {
    std::fs::write("/proc/self/clear_refs", "5").unwrap();
    status_kib("VmHWM:")
}

/// Cuánto subió el pico desde `base`, en MiB.
pub fn peak_mib_since(base: u64) -> u64 {
    status_kib("VmHWM:").saturating_sub(base) / 1024
}

/// Siembra `turns` turnos repartidos en `span` segundos hasta `now`, en bloques de
/// 500 seguidos del mismo pane (como en la base real: se funden en pocas sesiones).
pub fn seed_turns(dir: &TempDir, turns: i64, now: f64, span: f64) -> PathBuf {
    let db = dir.0.join("comandos-usage.sqlite");
    let mut conn = usage::open_usage_db_at(&db).unwrap();
    usage::ensure_schema(&conn).unwrap();
    let tx = conn.transaction().unwrap();
    {
        let mut insert = tx
            .prepare(
                "insert into usage_turns(id,provider,agent,tmux_session,tmux_pane,pane_pwd,git_root,model,\
                 turn_started_at,turn_finished_at,total_tokens,cost_usd,source,confidence,harness_account) \
                 values(?,?,?,?,?,?,?,?,?,?,?,?,'test','exact',?)",
            )
            .unwrap();
        for i in 0..turns {
            let b = i / 500;
            let provider = ["claude", "codex", "grok", "agy"][(b % 4) as usize];
            let finished = now - span + span * i as f64 / turns as f64;
            insert
                .execute(params![
                    format!("turn-{i:08}"),
                    provider,
                    if b % 7 == 0 { "opencode" } else { provider },
                    format!("sesion-de-trabajo-{}", b % 40),
                    format!("%{}", b % 90),
                    format!(
                        "/home/usuario/codebase/organizacion/proyecto-{}/sub",
                        b % 25
                    ),
                    format!("/home/usuario/codebase/organizacion/proyecto-{}", b % 25),
                    format!("modelo-de-prueba-{}", b % 6),
                    (finished - 30.0) as i64,
                    finished as i64,
                    1_000 + i,
                    0.001 * (i % 13) as f64,
                    ["main", "work", "relotto"][(b % 3) as usize],
                ])
                .unwrap();
        }
    }
    tx.commit().unwrap();
    db
}

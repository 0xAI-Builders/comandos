//! Pico de memoria de la semana de Analytics con una base del tamaño de la real
//! (≈ 92 000 turnos en 17 días). Guardar cada turno como objeto JSON llevaba el
//! frente a ~270 MiB de Pss, que la arena del hilo del carril ya no devolvía; las
//! filas se reducen al leerlas (`each_week_row` + `WeekRows`). Un solo `#[test]`
//! en el binario: el pico (`VmHWM`) es del proceso entero.
use comandos_core::analytics_week::{self, WeekInput, WeekRows};
use comandos_store::{
    usage,
    usage_read::{self, WeekRow},
};
use rusqlite::params;

const NOW: f64 = 1_791_115_200.5;
const TURNS: i64 = 100_000;
/// Con objetos JSON por turno el pico de esta base es ~238 MiB; reducido, ~5.
const MAX_PEAK_MIB: u64 = 32;

fn status_kib(field: &str) -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let line = status.lines().find(|l| l.starts_with(field)).unwrap();
    line.split_whitespace().nth(1).unwrap().parse().unwrap()
}

#[test]
fn week_rows_peak_stays_small_with_a_real_sized_db() {
    let dir = std::env::temp_dir().join(format!("cmd-week-mem-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("comandos-usage.sqlite");
    {
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
            let span = 16.0 * 86_400.0;
            // Turnos seguidos del mismo pane (bloques de 500, cada 14 s): como en
            // la base real, se funden en pocas sesiones.
            for i in 0..TURNS {
                let b = i / 500;
                let provider = ["claude", "codex", "grok", "agy"][(b % 4) as usize];
                let finished = NOW - span + span * i as f64 / TURNS as f64;
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
                        format!("modelo-de-prueba-{}", i % 6),
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
    }
    let conn = usage::open_usage_db_at(&db).unwrap();
    let since = NOW - 17.0 * 86_400.0;

    // Reinicia el pico del proceso (`VmHWM`) al Rss actual.
    std::fs::write("/proc/self/clear_refs", "5").unwrap();
    let base = status_kib("VmHWM:");
    let mut rows = WeekRows::new(NOW);
    let mut seen = 0;
    usage_read::each_week_row(&conn, since, true, |kind, row| {
        seen += 1;
        match kind {
            WeekRow::Turn => rows.push_turn(row),
            WeekRow::Span => rows.push_span(row),
        }
    })
    .unwrap();
    let input = WeekInput {
        now: NOW,
        offset: 0,
        limits: &[],
        rows: &rows,
        snapshots: &[],
        records: &[],
        tz_name: analytics_week::TZ,
    };
    let mut week = analytics_week::build_week(&input).unwrap();
    let accounts = week["accounts"].as_array().unwrap().clone();
    week["accounts"] = analytics_week::sidebar_accounts_from(&accounts, &[], &rows)
        .unwrap()
        .into();
    let body = serde_json::to_vec(&week).unwrap();
    let peak_mib = status_kib("VmHWM:").saturating_sub(base) / 1024;
    assert_eq!(seen, TURNS);
    assert!(
        peak_mib <= MAX_PEAK_MIB,
        "pico de la semana {peak_mib} MiB > {MAX_PEAK_MIB} MiB"
    );

    // El camino reducido da lo mismo que el de las filas guardadas (después de
    // medir: este sí materializa todos los turnos).
    let (turns, spans) = usage_read::week_rows(&conn, since, true).unwrap();
    let all = WeekRows::from_rows(&turns, &spans, NOW);
    let mut expected = analytics_week::build_week(&WeekInput {
        rows: &all,
        ..input
    })
    .unwrap();
    expected["accounts"] = analytics_week::sidebar_accounts(&accounts, &[], &turns, NOW)
        .unwrap()
        .into();
    assert_eq!(body, serde_json::to_vec(&expected).unwrap());
    drop(conn);
    let _ = std::fs::remove_dir_all(&dir);
}

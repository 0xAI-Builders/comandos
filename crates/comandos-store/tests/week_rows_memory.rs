//! Pico de memoria de la semana de Analytics con una base del tamaño de la real
//! (≈ 92 000 turnos en 17 días). Guardar cada turno como objeto JSON llevaba el
//! frente a ~270 MiB de Pss, que la arena del hilo del carril ya no devolvía; las
//! filas se reducen al leerlas (`each_week_row` + `WeekRows`). Un solo `#[test]`
//! en el binario: el pico (`VmHWM`) es del proceso entero.
#![cfg(target_os = "linux")]
#[path = "support/memory.rs"]
mod memory;

use comandos_core::analytics_week::{self, WeekInput, WeekRows};
use comandos_store::{
    usage,
    usage_read::{self, WeekRow},
};
use memory::{TempDir, peak_mib_since, reset_peak, seed_turns};

const NOW: f64 = 1_791_115_200.5;
const TURNS: i64 = 100_000;
/// Con objetos JSON por turno el pico de esta base es ~238 MiB; reducido, ~5.
const MAX_PEAK_MIB: u64 = 32;

#[test]
fn week_rows_peak_stays_small_with_a_real_sized_db() {
    let dir = TempDir::new("week-mem");
    let db = seed_turns(&dir, TURNS, NOW, 16.0 * 86_400.0);
    let conn = usage::open_usage_db_at(&db).unwrap();
    let since = NOW - 17.0 * 86_400.0;

    let base = reset_peak();
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
    let peak_mib = peak_mib_since(base);
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
}

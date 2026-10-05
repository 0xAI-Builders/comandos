//! Pico de memoria de los turnos de `/usage/state` (14 días) con una base del
//! tamaño de la real (≈ 84 000 turnos). Como objetos JSON ocupaban ~206 MiB;
//! `state_rows` los compacta según los lee (`StateTurns`). Un solo `#[test]` en
//! el binario: el pico (`VmHWM`) es del proceso entero.
#![cfg(target_os = "linux")]
#[path = "support/memory.rs"]
mod memory;

use comandos_core::usage_state::{self, StateTurns};
use comandos_store::{usage, usage_read};
use memory::{TempDir, peak_mib_since, reset_peak, seed_turns};
use rusqlite::params;
use serde_json::Map;

const NOW: f64 = 1_791_115_200.0;
const TURNS: i64 = 90_000;
/// Compactos, los turnos de esta base suben el pico ≈ 8 MiB; como objetos, > 150.
const MAX_PEAK_MIB: u64 = 24;

#[test]
fn state_rows_peak_stays_small_with_a_real_sized_db() {
    let dir = TempDir::new("state-mem");
    let db = seed_turns(&dir, TURNS, NOW, 13.5 * 86_400.0);
    let conn = usage::open_usage_db_at(&db).unwrap();
    let now = NOW as i64;

    let base = reset_peak();
    let rows = usage_read::state_rows(&conn, now - 14 * 86_400).unwrap();
    let peak_mib = peak_mib_since(base);
    assert_eq!(rows.turns.len(), TURNS as usize);
    assert!(
        peak_mib <= MAX_PEAK_MIB,
        "pico de los turnos de /usage/state {peak_mib} MiB > {MAX_PEAK_MIB} MiB"
    );

    // Lo compacto da lo mismo que las filas completas (después de medir).
    let panes = usage_read::list_panes(&conn).unwrap();
    let mut settings = Map::new();
    settings.insert("COMANDOS_CLAUDE_DAILY_TOKEN_LIMIT".into(), "900000".into());
    let build = |turns: &StateTurns| {
        let state = usage_state::build_state(
            now,
            panes.clone(),
            turns,
            &rows.provider_usage,
            &rows.provider_costs,
            &settings,
        )
        .unwrap();
        serde_json::to_vec(&state).unwrap()
    };
    let mut stmt = conn
        .prepare(
            "select tmux_session, tmux_pane, pane_pwd, git_root, agent, provider, model, confidence, \
             cost_usd, total_tokens, turn_finished_at from usage_turns where turn_finished_at >= ? \
             order by turn_finished_at desc",
        )
        .unwrap();
    let names: Vec<String> = stmt
        .column_names()
        .iter()
        .map(|n| (*n).to_owned())
        .collect();
    let full: Vec<Map<String, serde_json::Value>> = stmt
        .query_map(params![now - 14 * 86_400], |row| {
            let mut out = Map::new();
            for (i, name) in names.iter().enumerate() {
                let value = match row.get_ref(i)? {
                    rusqlite::types::ValueRef::Null => serde_json::Value::Null,
                    rusqlite::types::ValueRef::Integer(n) => n.into(),
                    rusqlite::types::ValueRef::Real(x) => x.into(),
                    rusqlite::types::ValueRef::Text(t) => {
                        String::from_utf8(t.to_vec()).unwrap().into()
                    }
                    rusqlite::types::ValueRef::Blob(_) => unreachable!(),
                };
                out.insert(name.clone(), value);
            }
            Ok(out)
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(build(&rows.turns), build(&StateTurns::from_rows(&full)));
}

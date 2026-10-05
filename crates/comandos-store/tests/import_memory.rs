//! Pico de memoria de la importación de transcripts de Claude: un archivo con
//! líneas de varios MiB (resultados de herramientas) y decenas de miles de turnos.
//! Las líneas se validan sin materializarse y los turnos se escriben por lotes:
//! el pico no crece con el tamaño del archivo ni con el número de turnos. Un solo
//! `#[test]` en el binario: el pico (`VmHWM`) es del proceso entero.
#![cfg(target_os = "linux")]
#[path = "support/memory.rs"]
#[allow(dead_code)]
mod memory;

use comandos_store::{usage, usage_import};
use memory::{TempDir, peak_mib_since, reset_peak};
use rusqlite::Connection;
use std::io::Write;

const NOW: i64 = 1_791_115_200;
const TURNS: usize = 40_000;
const BIG_LINES: usize = 40;
/// Medido ≈ 6 MiB; con los turnos acumulados o las líneas como objetos, > 40.
const MAX_PEAK_MIB: u64 = 20;

struct NoRoots;

impl usage_import::GitRoots for NoRoots {
    fn root(&self, path: &str) -> String {
        path.to_owned()
    }
}

#[test]
fn claude_import_peak_stays_small_with_big_lines() {
    let dir = TempDir::new("import-mem");
    let home = dir.0.join("home");
    let project = home.join(".claude/projects/-r-a");
    std::fs::create_dir_all(&project).unwrap();
    {
        let file = std::fs::File::create(project.join("s.jsonl")).unwrap();
        let mut out = std::io::BufWriter::new(file);
        let big = "x".repeat(1 << 20);
        for i in 0..TURNS {
            if i % (TURNS / BIG_LINES) == 0 {
                writeln!(
                    out,
                    r#"{{"type":"user","message":{{"content":[{{"type":"tool_result","content":"{big}"}}]}}}}"#
                )
                .unwrap();
            }
            writeln!(
                out,
                r#"{{"type":"assistant","timestamp":{},"cwd":"/r/a","sessionId":"s","requestId":"q{i}","message":{{"id":"m{i}","model":"claude-fable-5","content":[{{"type":"text","text":"{}"}}],"usage":{{"input_tokens":{},"output_tokens":7}}}}}}"#,
                NOW - 3600 + (i as i64 % 3000),
                "y".repeat(200),
                i + 1
            )
            .unwrap();
        }
    }
    let db = dir.0.join("usage.sqlite");
    let conn = usage::open_usage_db_at(&db).unwrap();
    usage::ensure_schema(&conn).unwrap();
    let zone = chrono_tz::America::Mexico_City;
    let admit = |_: &Connection| true;
    let plan = usage_import::ImportPlan {
        now: NOW,
        max_age_days: 21,
        claude_max_files: None,
        codex_max_files: None,
        home: &home,
        claude_projects_main: None,
        opencode_db: home.join("opencode.db"),
        zone: &zone,
        admit: &admit,
        cancelled: &|| false,
        big_lines: usage_import::BigLines::Skip,
    };
    let mut seen = usage_import::ImportSeen::default();

    let base = reset_peak();
    let n = usage_import::record_local_claude_jsonl(
        &conn,
        &plan,
        &home.join(".claude/projects"),
        "main",
        &mut seen,
        &NoRoots,
    )
    .unwrap();
    let peak_mib = peak_mib_since(base);
    assert_eq!(n, TURNS);
    let stored: i64 = conn
        .query_row("select count(*) from usage_turns", [], |r| r.get(0))
        .unwrap();
    assert_eq!(stored, TURNS as i64);
    assert!(
        peak_mib <= MAX_PEAK_MIB,
        "pico de la importación {peak_mib} MiB > {MAX_PEAK_MIB} MiB"
    );
}

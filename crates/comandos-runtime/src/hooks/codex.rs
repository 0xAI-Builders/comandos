//! `comandos hook codex`: transcripción de `adapters/codex-notify.sh` (el `notify`
//! de `~/.codex/config.toml`, respaldo de los hooks ricos). Recibe el JSON del
//! turno como único argumento y entrega un `done` al pipeline de `hook claude`,
//! salvo que el hook `Stop` de codex ya lo haya avisado hace 15 s o menos.
use super::adapter::{
    arg, codex_state_file, first_of, jq_get, jq_values, notify, pwd, recent_codex_done, strings,
};
use super::input::clock;

pub fn run(args: &[String]) -> i32 {
    let payload = args.first().map(String::as_bytes).unwrap_or_default();
    if payload.is_empty() {
        return 0;
    }
    let values = jq_values(payload);
    if jq_get(&values, |v| first_of(v, &["type"], Some(""))) != b"agent-turn-complete" {
        return 0;
    }
    let mut cwd = jq_get(&values, |v| first_of(v, &["cwd", "workspace-path"], None));
    if cwd.is_empty() {
        cwd = pwd();
    }
    let last = jq_get(&values, |v| {
        first_of(v, &["last-assistant-message"], Some(""))
    });
    let state = codex_state_file(&cwd);
    if state.is_file() && recent_codex_done(&state, clock().0) {
        return 0;
    }
    // thread-id/turn-id: el mismo turno avisado por hook y por notify es UN evento N1.
    let thread = jq_get(&values, |v| strings(first_of(v, &["thread-id"], Some(""))));
    let turn = jq_get(&values, |v| strings(first_of(v, &["turn-id"], Some(""))));
    notify(&[
        arg("--agent"),
        arg("codex"),
        arg("--event"),
        arg("done"),
        arg("--cwd"),
        cwd,
        arg("--full"),
        last,
        arg("--hook-event"),
        arg("Stop"),
        arg("--session-id"),
        thread,
        arg("--turn-id"),
        turn,
    ])
}

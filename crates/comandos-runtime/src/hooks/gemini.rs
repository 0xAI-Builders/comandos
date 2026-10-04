//! `comandos hook gemini`: transcripción de `adapters/gemini-hooks.sh` (hooks
//! `BeforeAgent`/`AfterAgent`/`Notification`/`SessionEnd` de Gemini CLI). Lee el
//! JSON por stdin y entrega el evento al pipeline de `hook claude`. Nunca imprime
//! nada: los hooks de gemini son síncronos y leen su stdout.
use super::adapter::{arg, first_of, jq_values, notify};
use super::input::env_bytes;
use std::io::Read;

pub fn run(_args: &[String]) -> i32 {
    let mut raw = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut raw);
    let mut agent = env_bytes("CC_AGENT");
    if agent.is_empty() {
        agent = arg("gemini");
    }
    let values = jq_values(&raw);
    let get = |key: &str| super::adapter::jq_get(&values, |v| first_of(v, &[key], Some("")));
    let event = get("hook_event_name");
    let cwd = get("cwd");
    let msg = get("message");
    let (event, full) = match event.as_slice() {
        b"BeforeAgent" => ("working", Vec::new()),
        b"AfterAgent" => ("done", get("prompt_response")),
        b"Notification" => ("waiting", msg.clone()),
        b"SessionEnd" => ("end", Vec::new()),
        _ => return 0,
    };
    // El bash lo lanzaba en segundo plano y callado; aquí corre en proceso.
    notify(&[
        arg("--agent"),
        agent,
        arg("--event"),
        arg(event),
        arg("--cwd"),
        cwd,
        arg("--msg"),
        msg,
        arg("--full"),
        full,
    ]);
    0
}

//! Topes de memoria del cliente ACP (ronda 1 de la Tarea 3 de 2f-4): un
//! agente que inunda `stdout`, `stderr` o manda trozos de texto enormes no
//! hace crecer la memoria del proceso sin límite; el agente queda cerrado.
//!
//! Binario propio con UNA sola prueba: `VmHWM` (pico de memoria residente)
//! es del proceso entero y otras pruebas en paralelo lo ensuciarían. Entre
//! tramos se reinicia con `/proc/self/clear_refs` (`5`).
//!
//! Confinamiento: el único agente es un guion `sh` que la prueba genera en
//! su directorio temporal (ruta absoluta, entorno limpio con
//! `PATH=/usr/bin:/bin`); genera los datos con `head`/`tr` desde `/dev/zero`.
//! Sin red, sin Python, sin agentes reales.
use comandos_runtime::acp_client::{AgentSession, Event, MAX_LINE, OpenOptions, Session};
use comandos_runtime::news_agents::{Opener, TOO_LONG, acp_text};
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

/// `FLOOD=stdout`: 64 MiB sin salto de línea por `stdout`.
/// `FLOOD=stderr`: 8 líneas de 8 MiB y 8 MiB sin salto por `stderr`, y
/// después responde con normalidad (si nadie vaciara `stderr`, se quedaría
/// bloqueado y no respondería).
/// `FLOOD=chunks`: 128 trozos de texto de 512 KiB (64 MiB) y fin de turno.
/// `FLOOD=numbers`: 4 líneas seguidas de casi 2 MiB con `text` = `[1,1,…]`
/// (la línea que más memoria ocupa ya convertida a `Value`) y fin de turno.
/// `FLOOD=exact`: un trozo de texto de `PAD` letras y fin de turno.
const FLOOD_ACP: &str = r#"#!/bin/sh
PATH=/usr/bin:/bin
printf 'PID %s\n' "$$" >> "$FAKEACP_LOG"
big() { head -c "$1" /dev/zero | tr '\0' a; }
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/^{"jsonrpc": "2.0", "id": \([0-9]*\), "method": .*/\1/p')
  case "$line" in
    *'"method": "session/new"'*)
      printf '{"jsonrpc": "2.0", "id": %s, "result": {"sessionId": "s1"}}\n' "$id" ;;
    *'"method": "session/prompt"'*)
      case "$FLOOD" in
        stdout) big 67108864; sleep 30 ;;
        stderr)
          i=0; while [ $i -lt 8 ]; do big 8388608 >&2; printf '\n' >&2; i=$((i+1)); done
          big 8388608 >&2
          printf '{"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "s1", "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "listo"}}}}\n' ;;
        chunks)
          i=0; while [ $i -lt 128 ]; do
            printf '{"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "s1", "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "'
            big 524288
            printf '"}}}}\n'
            i=$((i+1))
          done ;;
        numbers)
          # La línea se prepara antes y se manda 4 veces seguidas (sin pausas
          # del agente entre una y otra).
          one="$FAKEACP_LOG.numbers"
          { printf '{"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "s1", "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": ['
            head -c 1048000 /dev/zero | tr '\0' 1 | sed 's/1/1,/g'
            printf '1]}}}}\n'; } > "$one"
          cat "$one" "$one" "$one" "$one" ;;
        exact)
          printf '%s' "$PREFIX"; big "$PAD"; printf '%s\n' "$SUFFIX" ;;
      esac
      printf '{"jsonrpc": "2.0", "id": %s, "result": {"stopReason": "end_turn"}}\n' "$id" ;;
  esac
done
"#;

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn vm_hwm_kib() -> u64 {
    fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))
        .and_then(|v| v.split_whitespace().next()?.parse().ok())
        .unwrap()
}

/// Pone el pico (`VmHWM`) en la memoria residente actual.
fn reset_hwm() -> u64 {
    fs::write("/proc/self/clear_refs", "5").unwrap();
    vm_hwm_kib()
}

/// Un trozo de texto con el `text` entre estas dos partes.
const PREFIX: &str = r#"{"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "s1", "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": ""#;
const SUFFIX: &str = r#""}}}}"#;

fn options(dir: &Path) -> OpenOptions {
    OpenOptions {
        model: String::new(),
        extra_env: Vec::new(),
        search_path: Some(OsString::from("/usr/bin:/bin")),
        home: dir.to_path_buf(),
        base_env: Some(vec![
            ("HOME".into(), dir.as_os_str().to_owned()),
            ("PATH".into(), "/usr/bin:/bin".into()),
            ("LANG".into(), "C.UTF-8".into()),
        ]),
    }
}

fn spec(dir: &Path, flood: &str) -> Value {
    spec_with(dir, flood, 0)
}

fn spec_with(dir: &Path, flood: &str, pad: usize) -> Value {
    json!({"command": [dir.join("flood-acp").display().to_string()],
           "env": {"FLOOD": flood, "FAKEACP_LOG": dir.join("fake.log").display().to_string(),
                   "PAD": pad.to_string(), "PREFIX": PREFIX, "SUFFIX": SUFFIX}})
}

/// Un turno con un agente falso: el resultado y los textos recibidos.
fn turn(dir: &Path, spec: &Value) -> (Result<Value, String>, Vec<Value>) {
    let mut session = Session::open(spec, &dir.join("cwd"), &options(dir)).unwrap();
    session.new_session(10.0).unwrap();
    let mut texts = Vec::new();
    let got = session.prompt(
        "hola",
        &mut |e: &Event| {
            if let Event::Text(t) = e {
                texts.push(match t {
                    Value::String(s) => json!(s.len()),
                    Value::Array(items) => {
                        // Quien atiende el evento tarda: la línea sigue en
                        // memoria y la siguiente no debe leerse mientras.
                        std::thread::sleep(Duration::from_millis(400));
                        json!(["lista", items.len()])
                    }
                    _ => json!(null),
                });
            }
        },
        Duration::from_secs(60),
    );
    session.close();
    (got.map_err(|e| e.0), texts)
}

/// Procesos vivos con la ruta del guion en su línea de órdenes.
fn alive(script: &Path) -> usize {
    let needle = script.as_os_str().as_encoded_bytes();
    fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|n| n.parse::<u32>().is_ok())
        })
        .filter_map(|e| fs::read(e.path().join("cmdline")).ok())
        .filter(|cmd| cmd.windows(needle.len()).any(|w| w == needle))
        .count()
}

#[test]
fn floods_keep_memory_bounded_and_close_the_agent() {
    let dir = Dir(std::env::temp_dir().join(format!("laneB-acp-limits-{}", std::process::id())));
    let _ = fs::remove_dir_all(&dir.0);
    fs::create_dir_all(dir.0.join("cwd")).unwrap();
    let script = dir.0.join("flood-acp");
    fs::write(&script, FLOOD_ACP).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    let cwd = dir.0.join("cwd");
    let before = vm_hwm_kib();
    let started = Instant::now();

    // stdout sin fin de línea: fin de flujo, sesión fallida y agente cerrado.
    let mut session = Session::open(&spec(&dir.0, "stdout"), &cwd, &options(&dir.0)).unwrap();
    session.new_session(10.0).unwrap();
    let got = session.prompt("hola", &mut |_: &Event| {}, Duration::from_secs(20));
    assert_eq!(
        got.unwrap_err().0,
        format!(
            "el agente cerró stdout: línea de más de {} MiB en stdout",
            MAX_LINE >> 20
        )
    );
    session.close();

    // stderr enorme: se recorta, se sigue vaciando y el agente responde.
    let mut session = Session::open(&spec(&dir.0, "stderr"), &cwd, &options(&dir.0)).unwrap();
    session.new_session(10.0).unwrap();
    let mut text = String::new();
    let got = session.prompt(
        "hola",
        &mut |e: &Event| {
            if let Event::Text(Value::String(t)) = e {
                text.push_str(t);
            }
        },
        Duration::from_secs(20),
    );
    assert_eq!(got.unwrap(), json!("end_turn"));
    assert_eq!(text, "listo");
    session.close();

    // Trozos de texto por 64 MiB: «respuesta demasiado larga».
    let flood_dir = dir.0.clone();
    let opener: Opener = Arc::new(move |_agent: &str, _model: &str| {
        let mut session = Session::open(
            &spec(&flood_dir, "chunks"),
            &flood_dir.join("cwd"),
            &options(&flood_dir),
        )
        .map_err(|e| e.0)?;
        session.new_session(10.0).map_err(|e| e.0)?;
        Ok(Box::new(session) as Box<dyn AgentSession>)
    });
    let got = acp_text(&opener, "x", "", Duration::from_secs(30), "hola");
    assert_eq!(got.err().as_deref(), Some(TOO_LONG));

    let grown = vm_hwm_kib().saturating_sub(before);
    eprintln!("VmHWM +{grown} KiB en {:?}", started.elapsed());
    // Los agentes escribieron más de 200 MiB; el pico del proceso apenas sube.
    assert!(grown < 24 * 1024, "VmHWM creció {grown} KiB");
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(alive(&script), 0, "quedó un agente vivo");

    // Líneas hostiles de casi 2 MiB con `text` = `[1,1,…]`: cada una ocupa
    // más de 100 MiB ya convertida; en memoria a la vez, una sola (ni copias
    // de sus partes ni otra esperando en la cola).
    let before = reset_hwm();
    let (got, texts) = turn(&dir.0, &spec(&dir.0, "numbers"));
    let grown = vm_hwm_kib().saturating_sub(before);
    eprintln!("[1,1,…]: VmHWM +{grown} KiB");
    assert_eq!(got, Ok(json!("end_turn")));
    assert_eq!(texts, vec![json!(["lista", 1048001]); 4]);
    assert!(grown < 160 * 1024, "VmHWM creció {grown} KiB con [1,1,…]");

    // El tope es exacto: una línea de `MAX_LINE` bytes más el salto pasa;
    // una de un byte más es fin de flujo.
    let pad = MAX_LINE - PREFIX.len() - SUFFIX.len();
    let (got, texts) = turn(&dir.0, &spec_with(&dir.0, "exact", pad));
    assert_eq!(got, Ok(json!("end_turn")));
    assert_eq!(texts, vec![json!(pad)]);
    let (got, texts) = turn(&dir.0, &spec_with(&dir.0, "exact", pad + 1));
    assert_eq!(
        got,
        Err(format!(
            "el agente cerró stdout: línea de más de {} MiB en stdout",
            MAX_LINE >> 20
        ))
    );
    assert!(texts.is_empty());

    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(alive(&script), 0, "quedó un agente vivo");
    let pids = fs::read_to_string(dir.0.join("fake.log")).unwrap_or_default();
    assert_eq!(pids.lines().count(), 6, "{pids}");
}

//! Cliente ACP (`comandos_runtime::acp_client`) contra `lib/acp.py`, la parte
//! cliente que usan los Resúmenes (plan 2f-4, Tarea 3).
//!
//! Confinamiento: el ÚNICO agente es `FakeAcp`, un guion `sh` que esta prueba
//! genera en su HOME temporal (no es un archivo del repositorio) y que habla
//! JSON-RPC por líneas con respuestas fijas. El registro de cada prueba solo
//! tiene ese agente (`registry`), con su ruta absoluta: ninguna búsqueda en el
//! `PATH` puede dar con un agente real. El canario (`canary`) comprueba, ANTES
//! de cualquier `session/prompt`, que el proceso lanzado es el guion falso.
//! El Python corre con entorno limpio (HOME temporal, `PATH=/usr/bin:/bin`),
//! sin red ni credenciales; el lado Rust recibe ese mismo entorno.
use comandos_core::json::response_dumps;
use comandos_runtime::acp_client::{
    AgentSession, Event, OpenOptions, Session, agent_specs, deny_agent_tools,
};
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

/// Agente ACP falso. Lee JSON-RPC por líneas (las anota en `FAKEACP_LOG`) y
/// responde: `session/new` → `{"sessionId": "s1"}`; `session/prompt` → con
/// `FAKEACP_PERMISSION=1` pide permiso y anota la respuesta; con
/// `FAKEACP_EXTRA=1` pide un método no soportado; espera `FAKEACP_SLEEP`
/// segundos, manda `FAKEACP_CHUNK` (un literal JSON) como trozo de texto y
/// cierra el turno.
const FAKE_ACP: &str = r#"#!/bin/sh
PATH=/usr/bin:/bin
log=${FAKEACP_LOG:?}
printf 'PID %s %s\n' "$$" "$0" >> "$log"
printf 'ENV %s\n' "${COMANDOS_SILENT_AGENT:-}" >> "$log"
while IFS= read -r line; do
  printf '%s\n' "$line" >> "$log"
  id=$(printf '%s' "$line" | sed -n 's/^{"jsonrpc": "2.0", "id": \([0-9]*\), "method": .*/\1/p')
  case "$line" in
    *'"method": "session/new"'*)
      printf '{"jsonrpc": "2.0", "id": %s, "result": {"sessionId": "s1"}}\n' "$id" ;;
    *'"method": "session/prompt"'*)
      if [ "${FAKEACP_PERMISSION:-}" = 1 ]; then
        printf '%s\n' '{"jsonrpc": "2.0", "id": 900, "method": "session/request_permission", "params": {"sessionId": "s1", "options": [{"optionId": "si", "kind": "allow_once"}, {"optionId": "no", "kind": "reject_once"}], "toolCall": {"title": "rm -rf /"}}}'
        IFS= read -r answer
        printf '%s\n' "$answer" >> "$log"
      fi
      if [ "${FAKEACP_EXTRA:-}" = 1 ]; then
        printf '%s\n' '{"jsonrpc": "2.0", "id": 901, "method": "terminal/create", "params": {}}'
        IFS= read -r answer
        printf '%s\n' "$answer" >> "$log"
        printf '%s\n' 'esto no es JSON'
        printf '%s\n' '{"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "otra", "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "ajeno"}}}}'
        printf '%s\n' '{"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "s1", "update": {"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": "pienso"}}}}'
      fi
      sleep "${FAKEACP_SLEEP:-0}"
      printf '{"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "s1", "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": %s}}}}\n' "$FAKEACP_CHUNK"
      printf '{"jsonrpc": "2.0", "id": %s, "result": {"stopReason": "end_turn"}}\n' "$id" ;;
  esac
done
printf 'EOF\n' >> "$log"
"#;

struct Home {
    root: PathBuf,
}

impl Home {
    fn new(tag: &str) -> Home {
        let root = std::env::temp_dir().join(format!("laneB-acp-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("cwd")).unwrap();
        let script = root.join("fake-acp");
        fs::write(&script, FAKE_ACP).unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
        Home { root }
    }

    fn script(&self) -> PathBuf {
        self.root.join("fake-acp")
    }

    fn log(&self) -> PathBuf {
        self.root.join("fake.log")
    }

    /// El entorno de los dos lados: sin nada del desarrollador.
    fn env(&self) -> Vec<(String, String)> {
        vec![
            ("HOME".into(), self.root.display().to_string()),
            ("PATH".into(), "/usr/bin:/bin".into()),
            ("LANG".into(), "C.UTF-8".into()),
            ("PYTHONDONTWRITEBYTECODE".into(), "1".into()),
        ]
    }

    /// El agente `falso`: el guion por ruta absoluta y su configuración.
    fn spec(&self, extra: &[(&str, &str)]) -> Value {
        let mut env = serde_json::Map::new();
        env.insert(
            "FAKEACP_LOG".into(),
            json!(self.log().display().to_string()),
        );
        for (key, value) in extra {
            env.insert((*key).into(), json!(value));
        }
        json!({"command": [self.script().display().to_string()], "env": env})
    }

    /// Las líneas que recibió el agente, sin la de su PID (cambia por proceso)
    /// ni su `EOF` final (carrera entre cerrar stdin y el SIGTERM de `close`,
    /// la misma en los dos clientes).
    fn take_log(&self) -> Vec<String> {
        let text = fs::read_to_string(self.log()).unwrap_or_default();
        let _ = fs::remove_file(self.log());
        text.lines()
            .filter(|l| !l.starts_with("PID ") && *l != "EOF")
            .map(str::to_owned)
            .collect()
    }

    fn pids(&self) -> Vec<u32> {
        fs::read_to_string(self.log())
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.strip_prefix("PID "))
            .filter_map(|l| l.split(' ').next()?.parse().ok())
            .collect()
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn options(home: &Home) -> OpenOptions {
    OpenOptions {
        model: String::new(),
        extra_env: vec![("COMANDOS_SILENT_AGENT".into(), "1".into())],
        search_path: Some(OsString::from("/usr/bin:/bin")),
        home: home.root.clone(),
        base_env: Some(
            home.env()
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        ),
    }
}

fn python_available() -> bool {
    Command::new("python3")
        .args(["-c", "import sys"])
        .output()
        .is_ok_and(|o| o.status.success())
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

const ORACLE: &str = r#"
import json, os, sys
repo, spec, cwd, text, timeout = sys.argv[1], json.loads(sys.argv[2]), sys.argv[3], sys.argv[4], float(sys.argv[5])
sys.path.insert(0, os.path.join(repo, "lib"))
import acp, news_editions
events, out = [], {}
s = acp.open_session(spec, cwd, model="", permission_handler=news_editions.deny_agent_tools,
                     extra_env={"COMANDOS_SILENT_AGENT": "1"})
try:
    s.new_session()
    out["stop"] = s.prompt(text, on_event=events.append, timeout=timeout)
except Exception as exc:
    out["error"] = str(exc)
finally:
    s.close()
out["events"] = events
print(json.dumps(out))
"#;

fn run_python(home: &Home, spec: &Value, text: &str, timeout: f64) -> Value {
    let out = Command::new("python3")
        .env_clear()
        .envs(home.env())
        .arg("-c")
        .arg(ORACLE)
        .arg(repo())
        .arg(spec.to_string())
        .arg(home.root.join("cwd"))
        .arg(text)
        .arg(timeout.to_string())
        .current_dir(&home.root)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "oráculo: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

fn event_json(event: &Event) -> Value {
    match event {
        Event::Text(text) => json!({"type": "text", "text": text}),
        Event::Thought(text) => json!({"type": "thought", "text": text}),
        Event::Tool {
            title,
            status,
            kind,
            id,
        } => json!({"type": "tool", "title": title, "status": status, "kind": kind, "id": id}),
        Event::Plan(entries) => json!({"type": "plan", "entries": entries}),
        Event::Permission { title, decision } => {
            json!({"type": "permission", "title": title, "decision": decision})
        }
        Event::End(stop) => json!({"type": "end", "stopReason": stop}),
    }
}

fn run_rust(home: &Home, spec: &Value, text: &str, timeout: f64) -> Value {
    let mut events = Vec::new();
    let mut out = serde_json::Map::new();
    let mut session = Session::open(spec, &home.root.join("cwd"), &options(home)).unwrap();
    let result = session.new_session(90.0).and_then(|_| {
        session.prompt(
            text,
            &mut |e: &Event| events.push(event_json(e)),
            Duration::from_secs_f64(timeout),
        )
    });
    match result {
        Ok(stop) => {
            out.insert("stop".into(), stop);
        }
        Err(error) => {
            out.insert("error".into(), json!(error.to_string()));
        }
    }
    session.close();
    out.insert("events".into(), Value::Array(events));
    Value::Object(out)
}

/// Ningún proceso vivo con la ruta del guion en su línea de órdenes.
fn fake_alive(script: &Path) -> Vec<u32> {
    let needle = script.as_os_str().as_encoded_bytes();
    let mut out = Vec::new();
    for entry in fs::read_dir("/proc").unwrap().flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(cmd) = fs::read(entry.path().join("cmdline")) else {
            continue;
        };
        if cmd.windows(needle.len()).any(|w| w == needle) {
            out.push(pid);
        }
    }
    out
}

/// Canario: el registro solo lanza el guion falso. Abre una sesión (sin
/// `session/prompt`), comprueba por `/proc` que el proceso es `sh <guion>` y
/// que muere al cerrarla.
fn canary(home: &Home, spec: &Value) {
    let command = spec.get("command").cloned().unwrap();
    assert_eq!(
        command,
        json!([home.script().display().to_string()]),
        "el agente de la prueba no es el falso"
    );
    let mut session = Session::open(spec, &home.root.join("cwd"), &options(home)).unwrap();
    session.new_session(10.0).unwrap();
    let pids = home.pids();
    assert_eq!(pids.len(), 1, "{pids:?}");
    let pid = pids.first().copied().unwrap();
    let cmdline = fs::read(format!("/proc/{pid}/cmdline")).unwrap();
    let args: Vec<&[u8]> = cmdline.split(|b| *b == 0).collect();
    assert_eq!(
        args.get(1).copied(),
        Some(home.script().as_os_str().as_encoded_bytes()),
        "el proceso lanzado no es el agente falso"
    );
    session.close();
    assert!(
        !Path::new(&format!("/proc/{pid}")).exists(),
        "el agente sigue vivo"
    );
    home.take_log();
}

#[test]
fn registry_specs_come_from_acp_agents() {
    let registry =
        json!({"acpAgents": {"falso": {"command": ["/x"]}, "otro": {"command": ["/y"]}}});
    let specs = agent_specs(&registry);
    assert_eq!(specs.keys().collect::<Vec<_>>(), ["falso", "otro"]);
    assert!(agent_specs(&json!({})).is_empty());
    assert!(agent_specs(&json!({"acpAgents": null})).is_empty());
}

#[test]
fn deny_agent_tools_never_allows() {
    let options = json!([{"optionId": "si", "kind": "allow_once"}, {"optionId": "no", "kind": "reject_once"}]);
    assert_eq!(deny_agent_tools(&options).unwrap(), Some(json!("no")));
    let only_allow = json!([{"optionId": "si", "kind": "allow_always"}]);
    assert_eq!(deny_agent_tools(&only_allow).unwrap(), None);
    let deny = json!([{"optionId": 7, "kind": "deny"}]);
    assert_eq!(deny_agent_tools(&deny).unwrap(), Some(json!(7)));
}

#[test]
fn session_matches_python_client() {
    if !python_available() {
        eprintln!("python3 no está instalado: se salta");
        return;
    }
    let home = Home::new("same");
    let chunk = serde_json::to_string("Claro: {\"reply\": \"ñandú \\u00e9\"} fin").unwrap();
    let spec = home.spec(&[
        ("FAKEACP_CHUNK", &chunk),
        ("FAKEACP_PERMISSION", "1"),
        ("FAKEACP_EXTRA", "1"),
    ]);
    canary(&home, &spec);
    let text = "Pregunta «ñ» con \"comillas\"\ny saltos\t.";
    let python = run_python(&home, &spec, text, 30.0);
    let python_log = home.take_log();
    let rust = run_rust(&home, &spec, text, 30.0);
    let rust_log = home.take_log();
    assert_eq!(rust, python, "eventos y resultado");
    assert_eq!(rust_log, python_log, "lo que recibió el agente");
    // El permiso se respondió negando, nunca con la opción que permite.
    assert!(
        rust_log.iter().any(|l| l
            == r#"{"jsonrpc": "2.0", "id": 900, "result": {"outcome": {"outcome": "selected", "optionId": "no"}}}"#),
        "{rust_log:?}"
    );
    assert!(rust_log.contains(&"ENV 1".to_owned()), "{rust_log:?}");
    assert!(fake_alive(&home.script()).is_empty());
}

#[test]
fn timeout_matches_python_and_kills_the_agent() {
    if !python_available() {
        eprintln!("python3 no está instalado: se salta");
        return;
    }
    let home = Home::new("timeout");
    let chunk = serde_json::to_string("tarde").unwrap();
    let spec = home.spec(&[("FAKEACP_CHUNK", &chunk), ("FAKEACP_SLEEP", "5")]);
    canary(&home, &spec);
    let python = run_python(&home, &spec, "hola", 1.0);
    home.take_log();
    let started = std::time::Instant::now();
    let rust = run_rust(&home, &spec, "hola", 1.0);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    home.take_log();
    assert_eq!(rust, python);
    assert_eq!(
        rust.get("error"),
        Some(&json!("timeout esperando respuesta (1s)"))
    );
    // Ni el guion ni su `sleep` quedan vivos: se mató el grupo entero.
    std::thread::sleep(Duration::from_millis(200));
    assert!(fake_alive(&home.script()).is_empty());
}

/// Desviación deliberada: `fs/*` del agente se rechaza (el Python leería o
/// escribiría el archivo que pida el agente). Solo el lado Rust.
#[test]
fn file_requests_are_refused() {
    let home = Home::new("fs");
    let target = home.root.join("secreto");
    fs::write(&target, "no se lee").unwrap();
    let script = home.root.join("fs-acp");
    let body = format!(
        "#!/bin/sh\nPATH=/usr/bin:/bin\nwhile IFS= read -r line; do printf '%s\\n' \"$line\" >> {log}\n\
         case \"$line\" in *'\"session/new\"'*) printf '%s\\n' '{{\"jsonrpc\": \"2.0\", \"id\": 1, \"result\": {{\"sessionId\": \"s1\"}}}}';;\n\
         *'\"session/prompt\"'*) printf '%s\\n' '{{\"jsonrpc\": \"2.0\", \"id\": 5, \"method\": \"fs/read_text_file\", \"params\": {{\"path\": \"{target}\"}}}}'; IFS= read -r a; printf '%s\\n' \"$a\" >> {log};\n\
         printf '%s\\n' '{{\"jsonrpc\": \"2.0\", \"id\": 6, \"method\": \"fs/write_text_file\", \"params\": {{\"path\": \"{target}\", \"content\": \"pisado\"}}}}'; IFS= read -r a; printf '%s\\n' \"$a\" >> {log};\n\
         printf '%s\\n' '{{\"jsonrpc\": \"2.0\", \"id\": 2, \"result\": {{}}}}';; esac; done\n",
        log = home.log().display(),
        target = target.display(),
    );
    fs::write(&script, body).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    let spec = json!({"command": [script.display().to_string()]});
    let mut session = Session::open(&spec, &home.root.join("cwd"), &options(&home)).unwrap();
    session.new_session(10.0).unwrap();
    let stop = session
        .prompt("x", &mut |_: &Event| {}, Duration::from_secs(10))
        .unwrap();
    session.close();
    assert_eq!(stop, json!("end_turn"));
    let log = home.take_log();
    assert!(log.iter().any(|l| l == r#"{"jsonrpc": "2.0", "id": 5, "error": {"code": -32601, "message": "fs/read_text_file no soportado por cc-acp"}}"#), "{log:?}");
    assert!(log.iter().any(|l| l == r#"{"jsonrpc": "2.0", "id": 6, "error": {"code": -32601, "message": "fs/write_text_file no soportado por cc-acp"}}"#), "{log:?}");
    assert_eq!(fs::read_to_string(&target).unwrap(), "no se lee");
}

#[test]
fn unknown_binary_is_reported_like_python() {
    let home = Home::new("missing");
    let spec = json!({"command": ["laneB-no-existe-este-agente"]});
    let err = Session::open(&spec, &home.root.join("cwd"), &options(&home))
        .err()
        .map(|e| e.to_string());
    assert_eq!(
        err.as_deref(),
        Some("laneB-no-existe-este-agente no está instalado")
    );
    let err = Session::open(
        &json!({"command": []}),
        &home.root.join("cwd"),
        &options(&home),
    )
    .err()
    .map(|e| e.to_string());
    assert_eq!(err.as_deref(), Some("agente sin comando"));
    // El JSON que el cliente escribe es el `json.dumps` del Python.
    assert_eq!(
        response_dumps(&json!({"a": "é"})).unwrap(),
        r#"{"a": "\u00e9"}"#
    );
}

/// Las instrucciones de los agentes son las del Python, letra por letra.
#[test]
fn instructions_match_python() {
    if !python_available() {
        eprintln!("python3 no está instalado: se salta");
        return;
    }
    use comandos_runtime::news_agents::{
        CHAT_INSTRUCTIONS, LEAD_INSTRUCTIONS, SUMMARY_INSTRUCTIONS, TRANSLATE_INSTRUCTIONS,
    };
    let home = Home::new("consts");
    let out = Command::new("python3")
        .env_clear()
        .envs(home.env())
        .arg("-c")
        .arg(
            "import json, os, sys\nsys.path.insert(0, os.path.join(sys.argv[1], 'lib'))\n\
             import news_editions as ne\nprint(json.dumps([ne.SUMMARY_INSTRUCTIONS, ne.LEAD_INSTRUCTIONS, \
             ne.CHAT_INSTRUCTIONS, ne.TRANSLATE_INSTRUCTIONS]))",
        )
        .arg(repo())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let python: Vec<String> = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        python,
        [
            SUMMARY_INSTRUCTIONS,
            LEAD_INSTRUCTIONS,
            CHAT_INSTRUCTIONS,
            TRANSLATE_INSTRUCTIONS
        ]
    );
}

/// Un agente generado por la prueba: el guion `body` por ruta absoluta.
fn custom_agent(home: &Home, name: &str, body: &str) -> Value {
    let script = home.root.join(name);
    fs::write(&script, body).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    json!({"command": [script.display().to_string()],
           "env": {"LANEB_MARK": home.root.display().to_string()}})
}

/// Mata (por PID exacto) los procesos que llevan la marca de esta prueba en
/// su entorno: solo los que lanzó el agente falso.
fn kill_marked(home: &Home) {
    let mark = format!("LANEB_MARK={}", home.root.display());
    for entry in fs::read_dir("/proc").unwrap().flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<i32>().ok())
        else {
            continue;
        };
        let Ok(environ) = fs::read(entry.path().join("environ")) else {
            continue;
        };
        if environ.split(|b| *b == 0).any(|v| v == mark.as_bytes()) {
            let _ = nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(pid),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
    }
}

const ANSWER_NEW: &str = r#"printf '{"jsonrpc": "2.0", "id": 1, "result": {"sessionId": "s1"}}\n'"#;

/// Ronda 1, M1: bytes que no son UTF-8 en stderr no paran de vaciarlo: el
/// agente escribe 256 KiB más (más que la tubería) y responde.
#[test]
fn stderr_with_bad_utf8_keeps_draining() {
    let home = Home::new("badutf8");
    let body = format!(
        "#!/bin/sh\nPATH=/usr/bin:/bin\nIFS= read -r line\n{ANSWER_NEW}\nIFS= read -r line\n\
         printf '\\377\\376 malo\\n' >&2\nhead -c 262144 /dev/zero | tr '\\0' b >&2\nprintf '\\n' >&2\n\
         printf '%s\\n' '{{\"jsonrpc\": \"2.0\", \"id\": 2, \"result\": {{\"stopReason\": \"end_turn\"}}}}'\nsleep 5\n"
    );
    let spec = custom_agent(&home, "utf8-acp", &body);
    let mut session = Session::open(&spec, &home.root.join("cwd"), &options(&home)).unwrap();
    session.new_session(10.0).unwrap();
    let got = session.prompt("x", &mut |_: &Event| {}, Duration::from_secs(10));
    session.close();
    assert_eq!(got.unwrap(), json!("end_turn"));
}

/// Ronda 1, M2: plazos no finitos o enormes no entran en pánico.
#[test]
fn non_finite_timeouts_do_not_panic() {
    let home = Home::new("nan");
    let body = format!(
        "#!/bin/sh\nPATH=/usr/bin:/bin\nIFS= read -r line\nsleep 0.3\n{ANSWER_NEW}\n\
         IFS= read -r line\nprintf '%s\\n' '{{\"jsonrpc\": \"2.0\", \"id\": 2, \"result\": {{}}}}'\nsleep 5\n"
    );
    let spec = custom_agent(&home, "nan-acp", &body);
    let mut session = Session::open(&spec, &home.root.join("cwd"), &options(&home)).unwrap();
    // `NaN` y negativo: ya vencido (como el Python, que no espera nada).
    assert!(
        session
            .new_session(f64::NAN)
            .unwrap_err()
            .0
            .starts_with("timeout esperando respuesta")
    );
    session.close();
    let mut session = Session::open(&spec, &home.root.join("cwd"), &options(&home)).unwrap();
    assert_eq!(session.new_session(f64::INFINITY).unwrap(), json!("s1"));
    let got = session.prompt("x", &mut |_: &Event| {}, Duration::MAX);
    assert_eq!(got.unwrap(), json!("end_turn"));
    session.close();
    let mut session = Session::open(&spec, &home.root.join("cwd"), &options(&home)).unwrap();
    assert!(session.new_session(1e300).is_ok());
    session.close();
}

/// Ronda 1, M3: un descendiente que se escapó del grupo (`setsid`) con
/// `stdin` abierto sin leerlo no deja colgada la escritura del prompt: vence
/// el plazo de la llamada. (`exec 3<&0`: una lista asíncrona del `sh` sin
/// control de trabajos recibe `/dev/null` como entrada antes de sus
/// redirecciones; así el descendiente hereda de verdad la tubería.)
#[test]
fn stalled_stdin_does_not_hang_the_writer() {
    let home = Home::new("stall");
    let body = format!(
        "#!/bin/sh\nPATH=/usr/bin:/bin\nexec 3<&0\nsetsid sleep 37 0<&3 3<&- >/dev/null 2>&1 &\nexec 3<&-\nIFS= read -r line\n{ANSWER_NEW}\nexec sleep 37\n"
    );
    let spec = custom_agent(&home, "stall-acp", &body);
    let mut session = Session::open(&spec, &home.root.join("cwd"), &options(&home)).unwrap();
    session.new_session(10.0).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        // 1 MiB: más que la tubería, que nadie vacía.
        let text = "a".repeat(1024 * 1024);
        let started = std::time::Instant::now();
        let got = session.prompt(&text, &mut |_: &Event| {}, Duration::from_secs(2));
        session.close();
        let _ = tx.send((got, started.elapsed()));
    });
    let outcome = rx.recv_timeout(Duration::from_secs(20));
    // Pase lo que pase, nada de esta prueba queda vivo.
    kill_marked(&home);
    let _ = worker.join();
    let (got, elapsed) = outcome.expect("la escritura del prompt se quedó colgada");
    assert_eq!(got.unwrap_err().0, "timeout esperando respuesta (2s)");
    assert!(elapsed < Duration::from_secs(6), "{elapsed:?}");
}

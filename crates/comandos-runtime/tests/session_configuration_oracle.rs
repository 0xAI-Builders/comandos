//! El adaptador portado (`session_configuration`, `pane_exit`) contra el
//! escenario real de `tests/test_session_tmux.py`: tmux privado (`-S`), Codex
//! falso compilado, HOME temporal; ningún proveedor real. Los casos
//! `change`, `account` y `launch-failure` se comparan además con el Python
//! (`bin/cc-dash` cargado con `SourceFileLoader`, su `tmux` forzado al `-S`
//! de un segundo laboratorio idéntico): mismo estado final del journal, mismo
//! resultado observable, mismos comandos tecleados y de vuelta.
//!
//! Confinamiento: cada laboratorio es un `PrivateTmux` propio (`-f /dev/null
//! -S <dir>/tmux-<uid>/default`, sin `TMUX`) cuyo `Drop` hace `kill-server`
//! con ese `-S` y después borra el directorio. Las únicas señales salen del
//! adaptador hacia el stub que lanzó la prueba, o de la prueba hacia su
//! propio `sleep 30`.
#[path = "support/ops_lab.rs"]
mod ops_lab;
#[path = "support/private_tmux.rs"]
mod private_tmux;
#[path = "support/python.rs"]
mod python;

use comandos_runtime::{
    agent_procs, pane_exit,
    session_configuration::{self as sc, Kind, SessionConfiguration},
    session_operations::{OperationStore, open_journal, run_operation},
};
use ops_lab::{OpsLab, SID, Stub};
use serde_json::{Value, json};
use std::{
    ffi::OsStr,
    path::Path,
    time::{Duration, Instant},
};

/// `run_operation` del adaptador portado sobre el journal del laboratorio,
/// con la sonda previa al `claim` que hace `session_configure`.
fn run_rust(lab: &OpsLab, data: &Value, attempts: Option<i64>) -> (Value, Value) {
    let conn = open_journal(&lab.journal()).unwrap();
    let owner = || i64::from(std::process::id());
    let clock = || 1_000.0;
    let store = OperationStore::new(&conn, &owner, &clock).unwrap();
    let identity = lab.identity();
    let id = data["requestId"].as_str().unwrap();
    let mut adapter =
        SessionConfiguration::new(Kind::Session, data.clone(), identity.clone(), lab.env())
            .unwrap();
    adapter
        .probe()
        .expect("la sonda previa al claim no declina");
    if let Some(n) = attempts {
        adapter = adapter.with_verify_attempts(n);
    }
    assert!(store.claim(id, &sc::identity_key(&identity), data).unwrap());
    let result = run_operation(&store, id, &mut adapter, |_, _| Ok(()));
    let row = store.get(id).unwrap().unwrap();
    (result, row)
}

/// Lo observable de una operación, con el HOME como `~H` y sin lo que
/// depende del proceso o del reloj (`observedAt`, `pid`, `identity`).
fn summary(lab: &OpsLab, result: &Value, row: &Value) -> String {
    let mut result = result.clone();
    if let Some(observed) = result.get_mut("observed").and_then(Value::as_object_mut) {
        // `retain`: sin mover las demás claves (`remove` intercambia).
        observed
            .retain(|k, _| !["observedAt", "evidenceAt", "pid", "identity"].contains(&k.as_str()));
    }
    let snapshot = row.get("snapshot").cloned().unwrap_or(Value::Null);
    let info = sc::agent_info_for_pane(&lab.env(), &lab.pane)
        .unwrap()
        .unwrap();
    let out = json!({
        "result": result,
        "state": row["state"],
        "command": snapshot.pointer("/destination/command"),
        "resume": snapshot.pointer("/origin/resume_command"),
        "args": agent_procs::proc_cmdline(Path::new("/proc"), info.pid),
    });
    serde_json::to_string(&out)
        .unwrap()
        .replace(lab.home.to_str().unwrap(), "~H")
}

const PYTHON: &str = r#"
import importlib.machinery, importlib.util, json, os, subprocess, sys, time, types
repo = sys.argv[1]
sys.path[:0] = [os.path.join(repo, 'bin'), os.path.join(repo, 'lib')]
loader = importlib.machinery.SourceFileLoader('cc_dash_oracle', os.path.join(repo, 'bin/cc-dash'))
spec = importlib.util.spec_from_loader(loader.name, loader)
dash = importlib.util.module_from_spec(spec)
loader.exec_module(dash)
from session_operations import OperationStore, run_operation
tmux_path, socket, home, pane, raw = sys.argv[2:7]
def tmux(*args, timeout=5):
    assert os.path.isdir(os.path.dirname(socket)), 'sin socket privado'
    return subprocess.run([tmux_path, '-f', '/dev/null', '-S', socket, *map(str, args)],
                          capture_output=True, text=True, timeout=10)
registry = json.load(open(os.path.join(repo, 'config/providers.json')))
for name, spec in registry['harnesses'].items():
    if spec['capabilities'].get('accounts'):
        spec.update(defaultHome=os.path.join(home, '.' + name), accountsRoot=os.path.join(home, '.' + name + '-accounts'))
stub = os.path.join(home, 'bin', 'codex')
dash.tmux = tmux
dash.read_conf = lambda: {}
dash.load_provider_registry = lambda: registry
dash._harness_bin = lambda harness: stub if harness == 'codex' else '/missing/' + harness
facts = {'harnesses': {'codex': {'available': True, 'authenticated': True}}, 'motors': {}, 'gateway': {}}
dash.capability_matrix = lambda: dash.provider_registry.evaluate_capability_matrix(registry, facts)
dash.time = types.SimpleNamespace(time=time.time, sleep=lambda seconds: time.sleep(min(seconds, .04)))
dash.cc_usage.record_change = lambda *a, **k: None
store = OperationStore(os.path.join(home, 'operations.sqlite3'))
dash.session_operation_store = lambda: store
data = json.loads(raw)
identity = dash._pane_identity('audit', pane)
adapter = dash.SessionConfiguration(data, identity)
adapter.store = store
store.claim(data['requestId'], dash._identity_key(identity), data)
result = run_operation(store, data['requestId'], adapter)
row = store.get(data['requestId'])
snapshot = row['snapshot'] or {}
observed = result.get('observed')
if isinstance(observed, dict):
    for volatile in ('observedAt', 'evidenceAt', 'pid', 'identity'):
        observed.pop(volatile, None)
print(json.dumps({'result': result, 'state': row['state'],
                  'command': (snapshot.get('destination') or {}).get('command'),
                  'resume': (snapshot.get('origin') or {}).get('resume_command'),
                  'args': dash._proc_cmdline(dash.agent_info_for_pane(pane)['pid'])},
                 separators=(',', ':'), ensure_ascii=False).replace(home, '~H'))
"#;

/// La misma petición en un segundo laboratorio con el Python; `None` sin python3.
fn run_python(lab: &OpsLab, data: &Value) -> Option<String> {
    let socket = lab.tmux.socket();
    let raw = data.to_string();
    let out = python::run_python(
        PYTHON,
        &[
            lab.tmux.tmux_path().as_os_str(),
            socket.as_os_str(),
            lab.home.as_os_str(),
            OsStr::new(&lab.pane),
            OsStr::new(&raw),
        ],
        &lab.home,
    )?;
    Some(out.lines().last().unwrap_or("").to_owned())
}

fn request(mode: &str) -> Value {
    let work = mode == "account";
    json!({
        "session": "audit", "pane": "", "requestId": "private-tmux-operation",
        "toHarness": "codex", "motor": "codex",
        "model": if mode == "launch-failure" { "gpt-5.6-luna" } else { "gpt-5.5" },
        "effort": "low",
        "harnessAccount": if work { "work" } else { "main" },
        "motorAccount": if work { "work" } else { "main" },
        "interrupt": true,
    })
}

fn switch_and_compare(mode: &str) {
    let Some(lab) = OpsLab::start(&format!("rs{}", &mode[..2])) else {
        return;
    };
    let mut data = request(mode);
    data["pane"] = json!(lab.pane);
    let other_before = lab.show(&lab.other, "#{pane_pid}\t#{pane_current_command}");
    let layout = lab.show(&lab.pane, "#{window_layout}");
    let (result, row) = run_rust(&lab, &data, None);
    assert_eq!(result["ok"], json!(mode != "launch-failure"), "{result}");
    assert_eq!(result["observed"]["conversationId"], json!(SID), "{result}");
    assert_eq!(result["observed"]["confirmed"], json!(true));
    let effort = if mode == "launch-failure" {
        "high"
    } else {
        "low"
    };
    assert_eq!(result["observed"]["effort"], json!(effort));
    if mode == "launch-failure" {
        assert_eq!(result["rolledBack"], json!(true));
        assert_eq!(row["state"], json!("rolled_back"));
    } else {
        assert_eq!(row["state"], json!("confirmed"));
    }
    if mode == "account" {
        assert_eq!(result["observed"]["harnessAccount"], json!("work"));
        assert_eq!(result["observed"]["motorAccount"], json!("work"));
        assert!(
            lab.home
                .join(format!(
                    ".codex-accounts/work/sessions/rollout-test-{SID}.jsonl"
                ))
                .exists()
        );
    }
    let info = sc::agent_info_for_pane(&lab.env(), &lab.pane)
        .unwrap()
        .unwrap();
    let args = agent_procs::proc_cmdline(Path::new("/proc"), info.pid);
    let after = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .map(|i| args[i + 1].clone())
    };
    assert_eq!(after("--sandbox").as_deref(), Some("read-only"));
    assert_eq!(after("--ask-for-approval").as_deref(), Some("untrusted"));
    assert_eq!(
        lab.show(&lab.other, "#{pane_pid}\t#{pane_current_command}"),
        other_before,
        "el otro pane no se toca"
    );
    assert_eq!(lab.show(&lab.pane, "#{window_layout}"), layout);
    let rust = summary(&lab, &result, &row);

    let Some(twin) = OpsLab::start(&format!("py{}", &mode[..2])) else {
        return;
    };
    let mut data = request(mode);
    data["pane"] = json!(twin.pane);
    let Some(expected) = run_python(&twin, &data) else {
        return;
    };
    // Mismo resultado, salvo el id del pane (cada laboratorio tiene el suyo).
    let rust = rust.replace(&lab.pane, "%P");
    let expected = expected.replace(&twin.pane, "%P");
    // Texto exacto: también el orden de las claves.
    assert_eq!(rust, expected);
}

#[test]
fn change_matches_python() {
    switch_and_compare("change");
}

#[test]
fn account_matches_python() {
    switch_and_compare("account");
}

#[test]
fn launch_failure_rolls_back_like_python() {
    switch_and_compare("launch-failure");
}

/// `account_switch_configuration({'alias': alias}, 'audit', pane)`.
fn account_switch(alias: &str, pane: &str, id: &str) -> Value {
    json!({
        "alias": alias, "session": "audit", "pane": pane,
        "harnessAccount": alias, "motorAccount": alias,
        "accountOnly": true, "interrupt": true, "requestId": id,
    })
}

#[test]
fn direct_account_round_trip_keeps_conversation_model_and_other_pane() {
    let Some(lab) = OpsLab::start("round") else {
        return;
    };
    let before = lab.show(&lab.other, "#{pane_pid}\t#{pane_current_command}");
    for alias in ["work", "main"] {
        let data = account_switch(alias, &lab.pane, &format!("direct-account-{alias}"));
        let (result, row) = run_rust(&lab, &data, None);
        assert_eq!(result["ok"], json!(true), "{result}");
        assert_eq!(row["state"], json!("confirmed"));
        assert_eq!(result["observed"]["conversationId"], json!(SID));
        assert_eq!(result["observed"]["harnessAccount"], json!(alias));
        assert_eq!(result["observed"]["model"], json!("gpt-5.5"));
        assert_eq!(result["observed"]["effort"], json!("high"));
        assert_eq!(
            lab.show(&lab.other, "#{pane_pid}\t#{pane_current_command}"),
            before
        );
    }
}

#[test]
fn old_prompt_cannot_confirm_delayed_new_cli() {
    let Some(lab) = OpsLab::start("slow") else {
        return;
    };
    lab.tmux
        .run(&["resize-window", "-t", &lab.pane, "-x", "300", "-y", "50"]);
    let data = json!({
        "session": "audit", "pane": lab.pane, "requestId": "slow-redraw-operation",
        "toHarness": "codex", "motor": "codex", "model": "gpt-5.5", "effort": "high",
        "harnessAccount": "work", "motorAccount": "work", "interrupt": true,
    });
    let (result, row) = run_rust(&lab, &data, Some(2));
    assert_eq!(result["pending"], json!(true), "{result}");
    assert_eq!(result["confirmed"], json!(false), "{result}");
    assert_eq!(row["state"], json!("awaiting_confirmation"));
    assert!(
        row.pointer("/snapshot/destinationProcess/pid").is_some(),
        "el destino quedó fijado en el journal: {row}"
    );
    let env = lab.env();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut row = row;
    while Instant::now() < deadline {
        row = sc::refresh_confirmation(&env, &row).row;
        if row["state"] == json!("confirmed") {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(row["state"], json!("confirmed"), "{}", row["result"]);
    assert_eq!(row["result"]["observed"]["harnessAccount"], json!("work"));
}

#[test]
fn stuck_origin_types_nothing() {
    let Some(lab) = OpsLab::start_with("stuck", Stub::IgnoresExit) else {
        return;
    };
    let original = sc::agent_info_for_pane(&lab.env(), &lab.pane)
        .unwrap()
        .unwrap();
    let start = agent_procs::process_start(Path::new("/proc"), original.pid);
    let data = account_switch("work", &lab.pane, "stuck-origin-0001");
    let (result, row) = run_rust(&lab, &data, None);
    assert_eq!(result["ok"], json!(false), "{result}");
    assert_eq!(result["error"], json!("el agente original no cerró"));
    assert_eq!(row["state"], json!("rolled_back"), "{row}");
    assert!(
        !lab.capture(&lab.pane).contains("COMANDOS_OPERATION_ID"),
        "no se tecleó el destino"
    );
    // El origen sigue siendo el mismo proceso.
    assert_eq!(
        agent_procs::process_start(Path::new("/proc"), original.pid),
        start
    );
}

#[test]
fn exit_never_signals_foreign_pid() {
    let Some(lab) = OpsLab::start("foreign") else {
        return;
    };
    let mut victim = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .unwrap();
    let pid = i64::from(victim.id());
    let started = agent_procs::process_start(Path::new("/proc"), pid);
    let before = lab.capture(&lab.pane);
    let closed =
        pane_exit::exit_current(&lab.env(), &lab.pane, pid, "codex", Some("otro-inicio")).unwrap();
    assert!(!closed);
    assert_eq!(
        agent_procs::process_start(Path::new("/proc"), pid),
        started,
        "sigue vivo"
    );
    assert_eq!(lab.capture(&lab.pane), before, "ninguna tecla");
    // Pids que no son procesos: nunca se señalan.
    assert!(!pane_exit::exit_current(&lab.env(), &lab.pane, 0, "codex", None).unwrap());
    assert!(!pane_exit::exit_current(&lab.env(), &lab.pane, -1, "codex", None).unwrap());
    let _ = victim.kill(); // hijo propio de la prueba
    let _ = victim.wait();
}

#[test]
fn unchanged_account_is_confirmed_without_touching_the_pane() {
    let Some(lab) = OpsLab::start("same") else {
        return;
    };
    let original = sc::agent_info_for_pane(&lab.env(), &lab.pane)
        .unwrap()
        .unwrap();
    let data = account_switch("main", &lab.pane, "same-account-0001");
    let (result, row) = run_rust(&lab, &data, None);
    assert_eq!(result["unchanged"], json!(true), "{result}");
    assert_eq!(row["state"], json!("confirmed"));
    let after = sc::agent_info_for_pane(&lab.env(), &lab.pane)
        .unwrap()
        .unwrap();
    assert_eq!(after.pid, original.pid);
}

const HANDOFF: &str = r#"
import importlib.machinery, importlib.util, json, os, subprocess, sys
repo = sys.argv[1]
sys.path[:0] = [os.path.join(repo, 'bin'), os.path.join(repo, 'lib')]
loader = importlib.machinery.SourceFileLoader('cc_dash_oracle', os.path.join(repo, 'bin/cc-dash'))
spec = importlib.util.spec_from_loader(loader.name, loader)
dash = importlib.util.module_from_spec(spec)
loader.exec_module(dash)
tmux_path, socket, pane, cwd, sid, transcript = sys.argv[2:8]
def tmux(*args, timeout=5):
    assert os.path.isdir(os.path.dirname(socket)), 'sin socket privado'
    return subprocess.run([tmux_path, '-f', '/dev/null', '-S', socket, *map(str, args)],
                          capture_output=True, text=True, timeout=10)
dash.tmux = tmux
print(json.dumps([dash.capture_handoff(pane, 'codex', 'claude', cwd, sid, transcript_path=transcript),
                  dash.capture_handoff(pane, 'codex', 'grok', '/no/existe', '', '')]))
"#;

#[test]
fn capture_handoff_matches_python() {
    let Some(lab) = OpsLab::start("hand") else {
        return;
    };
    let code = lab.home.join("code");
    std::fs::create_dir_all(&code).unwrap();
    let init = std::process::Command::new("git")
        .args(["init", "-q"])
        .arg(&code)
        .env("HOME", &lab.home)
        .status();
    if !init.is_ok_and(|s| s.success()) {
        eprintln!("sin git: se salta la comparación del traspaso");
        return;
    }
    std::fs::write(code.join("nuevo.txt"), "hola\n").unwrap();
    // Líneas visibles que el traspaso filtra y conserva.
    lab.tmux.run(&[
        "send-keys",
        "-t",
        &lab.other,
        "-l",
        "--",
        "echo '──────────' ; echo visible ; echo 'esc to interrupt'",
    ]);
    lab.tmux.run(&["send-keys", "-t", &lab.other, "Enter"]);
    std::thread::sleep(Duration::from_millis(300));
    let transcript = lab.home.join("t.jsonl");
    let (code_s, transcript_s) = (code.to_str().unwrap(), transcript.to_str().unwrap());
    let env = lab.env();
    let rust = json!([
        pane_exit::capture_handoff(
            &env,
            &lab.other,
            "codex",
            "claude",
            code_s,
            SID,
            transcript_s
        )
        .unwrap(),
        pane_exit::capture_handoff(&env, &lab.other, "codex", "grok", "/no/existe", "", "")
            .unwrap(),
    ]);
    let socket = lab.tmux.socket();
    let Some(expected) = python::run_python(
        HANDOFF,
        &[
            lab.tmux.tmux_path().as_os_str(),
            socket.as_os_str(),
            OsStr::new(&lab.other),
            OsStr::new(code_s),
            OsStr::new(SID),
            OsStr::new(transcript_s),
        ],
        &lab.home,
    ) else {
        return;
    };
    let expected = comandos_core::json::workspace_loads(expected.lines().last().unwrap()).unwrap();
    assert_eq!(rust, expected);
    assert!(rust[0].as_str().unwrap().contains("visible"));
    // La línea de la regla se filtra; el eco del comando que la escribió, no.
    assert!(
        !rust[0]
            .as_str()
            .unwrap()
            .lines()
            .any(|l| l.starts_with("──────────"))
    );
}

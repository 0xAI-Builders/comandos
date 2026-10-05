//! Escritor de bordes contra un tmux privado (socket explícito con `-S`, nunca
//! el servidor del usuario); avisos de nivel una vez por pane y hora; evento y
//! popup como `usage_alert_send`. El Python de `bin/cc-dash` es el oráculo:
//! corre sobre su propio HOME y su propio tmux privado, con `urlopen`
//! sustituido (ninguna petición sale hacia el cc-notifyd real).
mod support;
use comandos_server::dash::native::{
    Native,
    tmux::{Program, Tmux, private_socket},
    usage::pane_models::{
        HyperNotify, NotifyPost, PaneModelWriter, PaneOutcome, TIER_ALERT_COOLDOWN_S, TierAlert,
        TierAlerts, live_rows, pane_values, send_alerts, ui_lang_es,
    },
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{Duration, Instant},
};
use support::{FakeNotify, NOW_MS, TestHome, oracle::D7_KEYS, repo, tmux_available};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// `time.time()` de las pruebas de avisos: el `NOW_MS` común, en segundos.
const NOW: f64 = (NOW_MS / 1000) as f64;

/// tmux privado del HOME (`-f /dev/null -S <socket>`): nunca el del usuario.
async fn tmux(home: &TestHome, args: &[&str]) -> String {
    let out = Tmux::private(&home.tmux_dir()).run(args).await.unwrap();
    assert!(out.ok, "tmux {args:?}: {}", out.stderr);
    out.stdout
}

async fn wait_idle(writer: &PaneModelWriter) {
    let start = Instant::now();
    while writer.running() {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "la reconciliación no terminó"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn tmux_binary() -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join("tmux"))
        .find(|p| p.is_file())
}

fn python_available() -> bool {
    let ok = Command::new("python3")
        .args(["-c", "import sys"])
        .output()
        .is_ok_and(|o| o.status.success());
    if !ok {
        eprintln!("python3 no está instalado: se salta la comparación con el oráculo");
    }
    ok
}

/// `python3 -c <guion> <repo> <args…>` con el HOME del `TestHome`: los
/// ejecutables con efectos fuera del HOME (incluido `tmux`) son enlaces a
/// `true`, sin las claves de D7 y con `LANG=C.UTF-8`.
fn run_python(script: &str, args: &[&OsStr], home: &TestHome) -> String {
    let fakebin = home.root.join("fakebin");
    let runtime = home.root.join("xdg-runtime");
    std::fs::create_dir_all(&fakebin).unwrap();
    std::fs::create_dir_all(&runtime).unwrap();
    for name in [
        "systemctl",
        "wmctrl",
        "cc-webterm",
        "cc-webterm-attach",
        "systemd-run",
        "tailscale",
        "notify-send",
        "pw-play",
        "paplay",
        "spd-say",
        "piper",
        "xdg-open",
        "tmux",
        "curl",
    ] {
        let link = fakebin.join(name);
        if !link.exists() {
            std::os::unix::fs::symlink("/bin/true", &link).unwrap();
        }
    }
    let path = format!(
        "{}:{}",
        fakebin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut command = Command::new("python3");
    for key in D7_KEYS {
        command.env_remove(key);
    }
    let out = command
        .arg("-c")
        .arg(script)
        .arg(repo())
        .args(args)
        .current_dir(repo())
        .env("HOME", &home.root)
        .env("PATH", &path)
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("XDG_STATE_HOME", home.root.join(".local/state"))
        .env("TMUX_TMPDIR", home.tmux_dir())
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("LANG", "C.UTF-8")
        .env_remove("LC_ALL")
        .env_remove("LC_CTYPE")
        .env_remove("TMUX")
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CODEX_HOME")
        .env_remove("GROK_HOME")
        .env_remove("COMANDOS_STATE_DB")
        .env_remove("COMANDOS_USAGE_DB")
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("http_proxy")
        .env_remove("https_proxy")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "oráculo: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

/// Carga `bin/cc-dash` como módulo. `time.time` fijo y `urlopen` sustituido
/// ANTES de cualquier llamada: el oráculo nunca habla con el cc-notifyd real.
const PRELUDE: &str = r#"
import importlib.machinery, importlib.util, json, os, subprocess, sys, threading, time
import urllib.request
SENT = []
def _fake_urlopen(req, timeout=None):
    SENT.append({"url": req.full_url, "data": req.data.decode(),
                 "ctype": req.get_header("Content-type"), "timeout": timeout})
    raise OSError("sin red en la prueba")
urllib.request.urlopen = _fake_urlopen
repo = sys.argv[1]
sys.path.insert(0, os.path.join(repo, "bin"))
sys.path.insert(0, os.path.join(repo, "lib"))
loader = importlib.machinery.SourceFileLoader("cc_dash_oracle", os.path.join(repo, "bin/cc-dash"))
spec = importlib.util.spec_from_loader(loader.name, loader)
dash = importlib.util.module_from_spec(spec)
loader.exec_module(dash)
assert dash.urllib.request.urlopen is _fake_urlopen
class SyncThread:
    def __init__(self, target=None, args=(), kwargs=None, daemon=None):
        self.target, self.args, self.kwargs = target, args, kwargs or {}
    def start(self):
        self.target(*self.args, **(self.kwargs))
"#;

/// Lo que se compara después de cada paso del escritor.
fn snapshot(
    applied: &BTreeMap<String, Option<String>>,
    physical: &str,
    hooks: &Path,
    retry: bool,
    discovered: bool,
) -> Value {
    let file = hooks.join("pane-models.txt");
    let (text, mode) = match std::fs::read_to_string(&file) {
        Ok(text) => (
            json!(text),
            json!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o7777),
        ),
        Err(_) => (Value::Null, Value::Null),
    };
    json!({
        "applied": applied,
        "physical": physical,
        "file": text,
        "mode": mode,
        "retry": retry,
        "discovered": discovered,
    })
}

fn sorted(value: &Value) -> String {
    comandos_core::json::dumps(value, true, false).unwrap()
}

/// Dos panes con opciones viejas y un `pane-models.txt` previo en 0644.
async fn stage(home: &TestHome) {
    tmux(
        home,
        &[
            "new-session",
            "-d",
            "-s",
            "p",
            "-x",
            "80",
            "-y",
            "24",
            "sleep 300",
        ],
    )
    .await;
    tmux(home, &["split-window", "-d", "-t", "=p:", "sleep 300"]).await;
    let ids = tmux(home, &["list-panes", "-a", "-F", "#{pane_id}"]).await;
    assert_eq!(ids, "%0\n%1\n", "un servidor privado nuevo numera desde %0");
    tmux(home, &["set-option", "-p", "-t", "%0", "@ccmodel", "rojo"]).await;
    tmux(home, &["set-option", "-p", "-t", "%1", "@ccmodel", "viejo"]).await;
    let file = home.hooks().join("pane-models.txt");
    std::fs::write(&file, "viejo\n").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
}

fn writer_steps() -> Value {
    let codex = "#[fg=colour43,bold]▸ codex · gpt-5.6-sol#[default]";
    json!([
        {"values": {"%0": "A ▸ x", "%9": null}, "text": "%0 a\n"},
        {"values": {"%0": null, "%1": codex}, "text": "%1 codex\n"},
        {"values": {"%0": null, "%1": codex}, "text": "%1 codex\n"},
        {"values": {"%0": null, "%1": codex, "%7": "x"}, "text": "%1 codex\n"},
        {"values": {"%0": null, "%1": codex, "%7": "x"}, "text": "%1 codex\n"},
        {"values": {"%1": codex}, "text": ""},
    ])
}

const WRITER_ORACLE: &str = r##"
tmux_bin, sock, steps_path = sys.argv[2:5]
env = {k: v for k, v in os.environ.items() if k != "TMUX"}
def tmux(*args, timeout=5):
    return subprocess.run([tmux_bin, "-f", "/dev/null", "-S", sock, *args],
                          capture_output=True, text=True, timeout=timeout, env=env)
dash.tmux = tmux
dash._pane_model_values = lambda panes: panes
out = []
for step in json.load(open(steps_path)):
    dash.write_pane_models((step["values"], step["text"]))
    deadline = time.monotonic() + 10
    while dash._pane_model_state["worker_running"] and time.monotonic() < deadline:
        time.sleep(0.01)
    st = dash._pane_model_state
    f = dash.PANE_MODELS_FILE
    exists = os.path.exists(f)
    out.append({"applied": dict(st["applied"]),
                "physical": tmux("list-panes", "-a", "-F", "#{pane_id}\t#{@ccmodel}").stdout,
                "file": open(f).read() if exists else None,
                "mode": (os.stat(f).st_mode & 0o7777) if exists else None,
                "retry": st["retry_after"] > time.monotonic(),
                "discovered": st["discovered"]})
print(json.dumps(out))
"##;

#[tokio::test]
async fn writer_sets_and_clears_pane_options_and_file() {
    if !tmux_available() {
        eprintln!("sin tmux: se salta");
        return;
    }
    let home = TestHome::new("pane-models");
    tmux(&home, &["new-session", "-d", "-s", "p", "sleep 60"]).await;
    let pane = tmux(&home, &["display-message", "-p", "-t", "=p:", "#{pane_id}"])
        .await
        .trim()
        .to_owned();
    // La opción del pane vivo existe antes de arrancar: se descubre.
    tmux(
        &home,
        &["set-option", "-p", "-t", &pane, "@ccmodel", "viejo"],
    )
    .await;
    let writer = Arc::new(PaneModelWriter::default());
    let opts = home.options();
    let value = "#[fg=colour43,bold]▸ codex#[default]";
    let mut values = BTreeMap::new();
    values.insert(pane.clone(), Some(value.to_owned()));
    writer
        .apply(
            values,
            format!("{pane} codex\n"),
            opts.tmux.clone(),
            opts.hooks.clone(),
        )
        .await;
    wait_idle(&writer).await;
    assert!(writer.discovered());
    assert_eq!(
        tmux(
            &home,
            &["show-options", "-p", "-v", "-t", &pane, "@ccmodel"]
        )
        .await
        .trim(),
        value
    );
    assert_eq!(
        std::fs::read_to_string(home.hooks().join("pane-models.txt")).unwrap(),
        format!("{pane} codex\n")
    );
    let mut cleared = BTreeMap::new();
    cleared.insert(pane.clone(), None);
    writer
        .apply(
            cleared,
            String::new(),
            opts.tmux.clone(),
            opts.hooks.clone(),
        )
        .await;
    wait_idle(&writer).await;
    assert_eq!(
        tmux(
            &home,
            &["show-options", "-p", "-v", "-t", &pane, "@ccmodel"]
        )
        .await
        .trim(),
        ""
    );
    assert_eq!(
        std::fs::read_to_string(home.hooks().join("pane-models.txt")).unwrap(),
        ""
    );
    assert_eq!(writer.applied().get(&pane), Some(&None));
}

#[tokio::test]
async fn writer_matches_python_reconciliation() {
    if !tmux_available() || !python_available() {
        eprintln!("sin tmux o python3: se salta");
        return;
    }
    let Some(tmux_bin) = tmux_binary() else {
        eprintln!("sin tmux en el PATH: se salta");
        return;
    };
    let py = TestHome::new("pane-models-py");
    let rs = TestHome::new("pane-models-rs");
    stage(&py).await;
    stage(&rs).await;
    let steps = writer_steps();
    let steps_file = py.root.join("steps.json");
    std::fs::write(&steps_file, steps.to_string()).unwrap();
    let socket = private_socket(&py.tmux_dir());
    let expected: Value = serde_json::from_str(&run_python(
        &format!("{PRELUDE}{WRITER_ORACLE}"),
        &[
            tmux_bin.as_os_str(),
            socket.as_os_str(),
            steps_file.as_os_str(),
        ],
        &py,
    ))
    .unwrap();

    let writer = Arc::new(PaneModelWriter::default());
    let opts = rs.options();
    let mut seen = Vec::new();
    for step in steps.as_array().unwrap() {
        let values: BTreeMap<String, Option<String>> =
            serde_json::from_value(step["values"].clone()).unwrap();
        let text = step["text"].as_str().unwrap().to_owned();
        writer
            .apply(values, text, opts.tmux.clone(), opts.hooks.clone())
            .await;
        wait_idle(&writer).await;
        let physical = tmux(&rs, &["list-panes", "-a", "-F", "#{pane_id}\t#{@ccmodel}"]).await;
        seen.push(snapshot(
            &writer.applied(),
            &physical,
            &rs.hooks(),
            writer.retry_pending(),
            writer.discovered(),
        ));
    }
    let seen = Value::Array(seen);
    assert_eq!(sorted(&seen), sorted(&expected));
    // Contenido concreto, por si los dos lados se equivocaran igual.
    let last = &seen[5];
    assert_eq!(last["file"], "");
    assert_eq!(last["mode"], 0o644);
    assert_eq!(seen[3]["retry"], true, "un pane muerto con valor reintenta");
    assert_eq!(
        seen[4]["retry"], true,
        "los mismos valores esperan el reintento"
    );
    assert_eq!(last["retry"], false, "valores nuevos lanzan la vuelta ya");
}

#[tokio::test]
async fn writer_without_tmux_retries_later() {
    let home = TestHome::new("pane-models-notmux");
    let mut tmux = home.options().tmux;
    tmux.program = Program::named("/no-existe/tmux");
    let writer = Arc::new(PaneModelWriter::default());
    let mut values = BTreeMap::new();
    values.insert("%0".to_owned(), Some("x".to_owned()));
    writer
        .apply(values.clone(), "%0 x\n".into(), tmux.clone(), home.hooks())
        .await;
    wait_idle(&writer).await;
    assert!(!writer.discovered());
    assert!(
        writer.retry_pending(),
        "sin tmux espera 5 s antes de volver"
    );
    // El archivo sí se escribió (no depende de tmux).
    assert_eq!(
        std::fs::read_to_string(home.hooks().join("pane-models.txt")).unwrap(),
        "%0 x\n"
    );
    // Mismos valores dentro de la espera: no se lanza otra vuelta.
    writer
        .apply(values, "%0 x\n".into(), tmux, home.hooks())
        .await;
    assert!(!writer.running());
    assert!(writer.applied().is_empty());
    // Tras un declinar del frente (R1 b) se vuelve a descubrir sin esperar.
    writer.forget_discovery();
    assert!(!writer.retry_pending());
}

#[test]
fn tier_alert_once_per_pane_per_hour() {
    let mut alerts = TierAlerts::default();
    let key = ("s".to_owned(), "%1".to_owned());
    assert!(!alerts.observe(&key, "low", "high", NOW)); // primera vista: nunca avisa
    assert!(alerts.observe(&key, "high", "high", NOW + 10.0)); // cambia a alerta: avisa
    assert!(!alerts.observe(&key, "low", "high", NOW + 20.0));
    assert!(!alerts.observe(&key, "high", "high", NOW + 30.0)); // dentro de la hora: no
    assert!(!alerts.observe(&key, "low", "high", NOW + 3700.0));
    assert!(alerts.observe(&key, "high", "high", NOW + 3711.0)); // pasó la hora: sí
    // Otro pane de la misma sesión lleva su propia cuenta.
    let other = ("s".to_owned(), "%2".to_owned());
    assert!(!alerts.observe(&other, "high", "high", NOW + 3712.0));
    // Sin sesión o sin nivel no se anota nada.
    let empty = (String::new(), "%3".to_owned());
    assert!(!alerts.observe(&empty, "low", "high", NOW));
    assert!(!alerts.observe(&empty, "high", "high", NOW + 1.0));
    assert_eq!(alerts.len(), 2);
}

#[test]
fn tier_alerts_forget_dead_panes_only_after_cooldown() {
    let mut alerts = TierAlerts::default();
    let alive = ("s".to_owned(), "%1".to_owned());
    let dead = ("s".to_owned(), "%2".to_owned());
    let quiet = ("s".to_owned(), "%3".to_owned());
    let paneless = ("t".to_owned(), String::new());
    alerts.observe(&alive, "low", "high", NOW);
    alerts.observe(&dead, "low", "high", NOW);
    assert!(alerts.observe(&dead, "high", "high", NOW + 1.0));
    alerts.observe(&quiet, "mid", "high", NOW);
    alerts.observe(&paneless, "mid", "high", NOW);
    let live: BTreeSet<String> = ["%1".to_owned()].into();
    // %3 murió sin avisar nunca: se olvida ya; %2 avisó hace un segundo: se queda.
    alerts.prune(&live, NOW + 2.0);
    assert_eq!(alerts.len(), 3);
    // Pasada la hora del aviso, el pane muerto se olvida; el vivo y el sin pane, no.
    alerts.prune(&live, NOW + 1.0 + TIER_ALERT_COOLDOWN_S);
    assert_eq!(alerts.len(), 2);
    assert!(!alerts.observe(&alive, "low", "high", NOW + 4000.0));
    assert!(alerts.observe(&alive, "high", "high", NOW + 4001.0));
}

/// `_maybe_tier_alert` con hilos síncronos y `usage_alert_send` anotado.
const TIER_ORACLE: &str = r#"
steps = json.load(open(sys.argv[2]))
calls = []
dash.threading.Thread = SyncThread
dash.usage_alert_send = lambda msg, title=None, kind="info", project=None: calls.append([msg, title, kind, project])
out = []
for now, sess, agent, model, tier, pane in steps:
    dash.time.time = lambda now=now: now
    before = len(calls)
    dash._maybe_tier_alert(sess, agent, model, tier, pane)
    out.append(calls[before:])
print(json.dumps(out))
"#;

fn tier_steps() -> Value {
    json!([
        [NOW, "s", "codex", "gpt-5.5", "mid", "%1"],
        [NOW + 5.0, "s", "codex", "gpt-5.6-sol", "high", "%1"],
        [NOW + 6.0, "s", "", "fable", "high", "%2"],
        [NOW + 7.0, "s", "", "haiku", "low", "%2"],
        [NOW + 8.0, "s", "", "fable", "high", "%2"],
        [NOW + 9.0, "s", "claude", "haiku", "low", "%1"],
        [NOW + 10.0, "s", "claude", "opus", "high", "%1"],
        [NOW + 4000.0, "s", "claude", "haiku", "low", "%1"],
        [NOW + 4001.0, "s", "claude", "opus", "high", "%1"],
        [NOW + 4002.0, "", "claude", "opus", "low", "%5"],
        [NOW + 4003.0, "", "claude", "opus", "high", "%5"],
        [NOW + 4004.0, "u", "grok", "grok-4.5", "", ""],
        [NOW + 4005.0, "u", "grok", "grok-4.6", "mid", ""],
        [NOW + 4006.0, "u", "grok", "grok-4.6", "high", ""],
    ])
}

fn model_tiers() -> Value {
    serde_json::from_str(&std::fs::read_to_string(repo().join("config/model-tiers.json")).unwrap())
        .unwrap()
}

/// El lado Rust de `TIER_ORACLE`: `observe`, estilo y textos.
fn rust_tier_calls(es: bool) -> Value {
    let tiers = model_tiers();
    let alert_tier = tiers["alertTier"].as_str().unwrap_or("high").to_owned();
    let mut alerts = TierAlerts::default();
    let mut out = Vec::new();
    for step in tier_steps().as_array().unwrap() {
        let s: Vec<&Value> = step.as_array().unwrap().iter().collect();
        let (now, sess, agent, model, tier, pane) = (
            s[0].as_f64().unwrap(),
            s[1].as_str().unwrap(),
            s[2].as_str().unwrap(),
            s[3].as_str().unwrap(),
            s[4].as_str().unwrap(),
            s[5].as_str().unwrap(),
        );
        let key = (sess.to_owned(), pane.to_owned());
        let mut calls = Vec::new();
        if alerts.observe(&key, tier, &alert_tier, now) {
            let alert = TierAlert::styled(sess, agent, model, tier, &tiers).unwrap();
            let (title, msg) = alert.texts(es);
            calls.push(json!([msg, title, "done", sess]));
        }
        out.push(Value::Array(calls));
    }
    Value::Array(out)
}

#[test]
fn tier_alerts_match_python_oracle() {
    if !python_available() {
        return;
    }
    for (lang, es) in [(Some("es"), true), (Some("en"), false), (None, false)] {
        let py = TestHome::new("tier-oracle");
        if let Some(lang) = lang {
            py.write("cc-notify.conf", &format!("CC_LANG={lang}\n"));
        }
        let steps = py.root.join("steps.json");
        std::fs::write(&steps, tier_steps().to_string()).unwrap();
        let expected: Value = serde_json::from_str(&run_python(
            &format!("{PRELUDE}{TIER_ORACLE}"),
            &[steps.as_os_str()],
            &py,
        ))
        .unwrap();
        // Sin CC_LANG decide el LANG del proceso (C.UTF-8 en las dos partes).
        assert_eq!(ui_lang_es(lang, Some("C.UTF-8")), es);
        let seen = rust_tier_calls(es);
        assert_eq!(seen, expected, "idioma {lang:?}");
        let count = seen
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| !c.as_array().unwrap().is_empty())
            .count();
        assert_eq!(count, 4, "cuatro avisos en la secuencia");
    }
}

#[test]
fn tier_style_and_lang_edges() {
    assert!(ui_lang_es(None, Some("es_MX.UTF-8")));
    assert!(ui_lang_es(Some("auto"), Some("ES")));
    assert!(!ui_lang_es(Some("en"), Some("es_MX.UTF-8")));
    assert!(ui_lang_es(Some("es"), None));
    // Sin estilo: el símbolo vacío y la etiqueta es el nivel.
    let alert = TierAlert::styled("s", "", "m", "high", &json!({})).unwrap();
    assert_eq!(alert.texts(true).0, "Modelo high en uso");
    assert_eq!(
        alert.texts(false).1,
        "The agent in this session switched to m : it uses quota faster."
    );
    // `tiers` verdadero que no es objeto, o símbolo contenedor (su `repr`):
    // incierto. Un escalar pasa por el `str()` del f-string.
    assert!(TierAlert::styled("s", "a", "m", "high", &json!({"tiers": [1]})).is_err());
    assert!(
        TierAlert::styled(
            "s",
            "a",
            "m",
            "high",
            &json!({"tiers": {"high": {"symbol": [3]}}})
        )
        .is_err()
    );
    let scalar = TierAlert::styled(
        "s",
        "a",
        "m",
        "high",
        &json!({"tiers": {"high": {"symbol": 3, "label": true}}}),
    )
    .unwrap();
    assert_eq!(
        (scalar.symbol.as_str(), scalar.label.as_str()),
        ("3", "True")
    );
    let falsy = TierAlert::styled(
        "s",
        "a",
        "m",
        "high",
        &json!({"tiers": {"high": {"symbol": 0, "label": ""}}}),
    )
    .unwrap();
    assert_eq!((falsy.symbol.as_str(), falsy.label.as_str()), ("", "high"));
}

/// `_maybe_tier_alert` hasta el final: evento en app-state y POST anotado.
const SEND_ORACLE: &str = r#"
steps = json.load(open(sys.argv[2]))
dash.threading.Thread = SyncThread
for now, sess, agent, model, tier, pane in steps:
    dash.time.time = lambda now=now: now
    dash._maybe_tier_alert(sess, agent, model, tier, pane)
conn = dash.app_state.connect()
events = dash.event_store.list_events(conn, 0, 100)
receipts = conn.execute("select count(*) from event_receipts").fetchone()[0]
print(json.dumps({"sent": SENT, "events": events, "receipts": receipts}))
"#;

/// El `eventId` lleva 8 hex aleatorios: se tapan para comparar.
fn masked(events: &[Value]) -> Value {
    Value::Array(
        events
            .iter()
            .map(|e| {
                let mut e = e.clone();
                let id = e["eventId"].as_str().unwrap().to_owned();
                let (head, hex) = id.rsplit_once(':').unwrap();
                assert_eq!(hex.len(), 8);
                assert!(hex.bytes().all(|b| b.is_ascii_hexdigit()));
                e["eventId"] = json!(format!("{head}:XXXXXXXX"));
                e
            })
            .collect(),
    )
}

async fn rust_send(home: &TestHome, steps: &Value) -> (Arc<FakeNotify>, Native) {
    let notify = Arc::new(FakeNotify::default());
    let mut opts = home.options();
    opts.notifyd = notify.clone();
    let native = Native::new(opts);
    let tiers = model_tiers();
    for step in steps.as_array().unwrap() {
        let s = step.as_array().unwrap();
        let (now, sess, agent, model, tier, pane) = (
            s[0].as_f64().unwrap(),
            s[1].as_str().unwrap(),
            s[2].as_str().unwrap(),
            s[3].as_str().unwrap(),
            s[4].as_str().unwrap(),
            s[5].as_str().unwrap(),
        );
        let key = (sess.to_owned(), pane.to_owned());
        let fire = native.tier_alerts().observe(&key, tier, "high", now);
        if fire {
            let alert = TierAlert::styled(sess, agent, model, tier, &tiers).unwrap();
            send_alerts(&native, vec![alert]).await;
        }
    }
    (notify, native)
}

fn rust_events(home: &TestHome) -> (Vec<Value>, i64) {
    let conn = rusqlite::Connection::open(home.state_db()).unwrap();
    let events = comandos_store::list_events(&conn, 0, 100).unwrap();
    let receipts: i64 = conn
        .query_row("select count(*) from event_receipts", [], |r| r.get(0))
        .unwrap();
    (events, receipts)
}

#[tokio::test]
async fn send_alerts_match_python_notice_and_popup() {
    if !python_available() {
        return;
    }
    // Agente largo con acentos: el excerpt se corta a 500 puntos de código y
    // el popup lleva el texto entero (`body` y `full`).
    let long = format!("agente-{}", "é".repeat(600));
    let steps = json!([
        [NOW, "proj-a", "codex", "gpt-5.5", "mid", "%1"],
        [NOW, "proj-a", "codex", "gpt-5.6-sol", "high", "%1"],
        [NOW, "proj-b", long, "fable", "low", "%4"],
        [NOW, "proj-b", long, "fable", "high", "%4"],
    ]);
    for conf in ["CC_LANG=es\n", "", "CC_LANG=es\nDESKTOP_NOTIFY=0\n"] {
        let py = TestHome::new("send-oracle-py");
        let rs = TestHome::new("send-oracle-rs");
        py.write("cc-notify.conf", conf);
        rs.write("cc-notify.conf", conf);
        let file = py.root.join("steps.json");
        std::fs::write(&file, steps.to_string()).unwrap();
        let expected: Value = serde_json::from_str(&run_python(
            &format!("{PRELUDE}{SEND_ORACLE}"),
            &[file.as_os_str()],
            &py,
        ))
        .unwrap();
        let (notify, native) = rust_send(&rs, &steps).await;
        native.shutdown().await;
        let posts = expected["sent"].as_array().unwrap();
        let bodies = notify.bodies();
        assert_eq!(bodies.len(), posts.len(), "conf {conf:?}");
        for (body, post) in bodies.iter().zip(posts) {
            assert_eq!(post["url"], "http://127.0.0.1:4778/notify");
            assert_eq!(post["ctype"], "application/json");
            assert_eq!(post["timeout"], 2);
            assert_eq!(body, post["data"].as_str().unwrap(), "cuerpo del popup");
        }
        let (events, receipts) = rust_events(&rs);
        assert_eq!(events.len(), 2, "conf {conf:?}");
        assert_eq!(json!(receipts), expected["receipts"]);
        assert_eq!(
            masked(&events),
            masked(expected["events"].as_array().unwrap()),
            "conf {conf:?}"
        );
        assert_eq!(
            events[1]["excerpt"].as_str().unwrap().chars().count(),
            500,
            "excerpt recortado como el Python"
        );
        if conf.is_empty() {
            assert!(bodies[0].contains("\"title\": \"caro model in use\""));
            assert!(bodies[1].contains(&long.replace('é', "\\u00e9")));
        }
    }
}

#[tokio::test]
async fn send_alerts_respect_conf_and_shadow() {
    let alert = || TierAlert::styled("s", "codex", "gpt-5.6-sol", "high", &model_tiers()).unwrap();
    // NOTIFY_MODEL_TIER=0: ni evento ni popup.
    let home = TestHome::new("send-quiet");
    home.write("cc-notify.conf", "NOTIFY_MODEL_TIER=0\n");
    let notify = Arc::new(FakeNotify::default());
    let mut opts = home.options();
    opts.notifyd = notify.clone();
    let native = Native::new(opts);
    send_alerts(&native, vec![alert()]).await;
    assert!(notify.bodies().is_empty());
    native.shutdown().await;
    assert!(!home.state_db().exists(), "sin avisos ni se abre app-state");

    // Sombra (`--no-usage-effects`): nada.
    let home = TestHome::new("send-shadow");
    let notify = Arc::new(FakeNotify::default());
    let mut opts = home.options();
    opts.notifyd = notify.clone();
    opts.usage_effects = false;
    let native = Native::new(opts);
    send_alerts(&native, vec![alert()]).await;
    assert!(notify.bodies().is_empty());
    native.shutdown().await;
    assert!(!home.state_db().exists());

    // `cc-notify.conf` ilegible (no UTF-8): ningún aviso.
    let home = TestHome::new("send-badconf");
    std::fs::write(home.hooks().join("cc-notify.conf"), b"CC_LANG=\xff\n").unwrap();
    let notify = Arc::new(FakeNotify::default());
    let mut opts = home.options();
    opts.notifyd = notify.clone();
    let native = Native::new(opts);
    send_alerts(&native, vec![alert()]).await;
    assert!(notify.bodies().is_empty());
    native.shutdown().await;
    assert!(!home.state_db().exists());

    // Configuración por omisión: evento y popup, en inglés por `LANG` vacío.
    let home = TestHome::new("send-default");
    let notify = Arc::new(FakeNotify::default());
    let mut opts = home.options();
    opts.notifyd = notify.clone();
    let native = Native::new(opts);
    send_alerts(&native, vec![alert(), alert()]).await;
    assert_eq!(notify.bodies().len(), 2);
    native.shutdown().await;
    let (events, _) = rust_events(&home);
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["title"], "caro model in use");
    assert_eq!(events[0]["projectKey"], "s");
    assert_eq!(events[0]["occurredAtMs"], NOW_MS);
}

/// Servidor HTTP mínimo en un puerto efímero: guarda la petición en crudo y
/// responde (o no, si `hang`).
async fn fake_notifyd(hang: bool) -> (u16, Arc<std::sync::Mutex<Vec<Vec<u8>>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen: Arc<std::sync::Mutex<Vec<Vec<u8>>>> = Arc::default();
    let log = seen.clone();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let log = log.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    let Ok(n) = stream.read(&mut chunk).await else {
                        return;
                    };
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
                        continue;
                    };
                    let head = String::from_utf8_lossy(&buf[..end]).to_ascii_lowercase();
                    let length = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .map_or(0, |v| v.trim().parse::<usize>().unwrap_or(0));
                    if buf.len() >= end + 4 + length {
                        break;
                    }
                }
                log.lock().unwrap().push(buf);
                if hang {
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    return;
                }
                let _ = stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\n\r\n{\"ok\":true}")
                    .await;
            });
        }
    });
    (port, seen)
}

#[tokio::test]
async fn hyper_notify_posts_like_urllib_and_never_hangs() {
    let body = r#"{"title": "Modelo caro en uso", "body": "é"}"#.to_owned();
    let (port, seen) = fake_notifyd(false).await;
    let notify = HyperNotify {
        addr: ([127, 0, 0, 1], port).into(),
    };
    notify.post(body.clone()).await;
    let raw = seen.lock().unwrap().clone();
    assert_eq!(raw.len(), 1);
    let text = String::from_utf8(raw[0].clone()).unwrap();
    let (head, sent) = text.split_once("\r\n\r\n").unwrap();
    let lower = head.to_ascii_lowercase();
    assert!(head.starts_with("POST /notify HTTP/1.1\r\n"), "{head}");
    assert!(lower.contains(&format!("host: 127.0.0.1:{port}")), "{head}");
    assert!(lower.contains("content-type: application/json"), "{head}");
    assert!(
        lower.contains(&format!("content-length: {}", body.len())),
        "{head}"
    );
    assert!(
        !lower.contains("origin:"),
        "cc-notifyd rechaza peticiones con Origin"
    );
    assert_eq!(sent, body);

    // Un cc-notifyd colgado no retiene más que el plazo de 2 s.
    let (port, seen) = fake_notifyd(true).await;
    let notify = HyperNotify {
        addr: ([127, 0, 0, 1], port).into(),
    };
    let start = Instant::now();
    notify.post(body.clone()).await;
    assert!(start.elapsed() < Duration::from_secs(4));
    assert_eq!(seen.lock().unwrap().len(), 1);

    // Nadie escuchando: vuelve enseguida y sin pánico.
    let notify = HyperNotify {
        addr: ([127, 0, 0, 1], support::dead_port()).into(),
    };
    let start = Instant::now();
    notify.post(body).await;
    assert!(start.elapsed() < Duration::from_secs(3));
}

#[test]
fn production_default_targets_the_session_notifyd() {
    // Solo la dirección: ninguna prueba envía nada a 4778.
    assert_eq!(
        HyperNotify::default().addr,
        std::net::SocketAddr::from(([127, 0, 0, 1], 4778))
    );
}

// ---------------------------------------------------------------------------
// Tarea 7b: filas vivas, valores de borde y el escritor con tmux falso
// ---------------------------------------------------------------------------

/// Proceso `sleep` propio con un entorno de cuenta conocido; se mata por su
/// manejador (nunca por patrón) al soltarlo.
struct Sleeper(std::process::Child);

impl Drop for Sleeper {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Sleeper {
    fn spawn(env: &[(&str, &Path)]) -> Self {
        let mut command = Command::new("sleep");
        command
            .arg("120")
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("CODEX_HOME")
            .env_remove("GROK_HOME")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        for (key, value) in env {
            command.env(key, value);
        }
        let child = command.spawn().unwrap();
        // `/proc/<pid>/environ` solo es el del `sleep` tras el `exec`.
        let comm = format!("/proc/{}/comm", child.id());
        let start = Instant::now();
        while std::fs::read_to_string(&comm).unwrap_or_default() != "sleep\n" {
            assert!(start.elapsed() < Duration::from_secs(5), "sleep no arrancó");
            std::thread::sleep(Duration::from_millis(5));
        }
        Self(child)
    }

    fn pid(&self) -> i64 {
        i64::from(self.0.id())
    }
}

/// `tmux` falso en `<home>/fakebin/tmux`: anota cada argv (un argumento por
/// línea y `--` al final) en `fakebin/argv`; `list-panes` imprime
/// `fakebin/options` (formato con `@ccmodel`) o `fakebin/ids`; `set-option`
/// sobre `%8` o `%9` falla. Nunca habla con ningún servidor tmux.
fn fake_tmux(home: &TestHome) -> (Tmux, PathBuf) {
    let dir = home.root.join("fakebin");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("tmux");
    std::fs::write(
        &path,
        "#!/bin/sh\n\
         d=$(dirname \"$0\")\n\
         for a in \"$@\"; do printf '%s\\n' \"$a\" >> \"$d/argv\"; done\n\
         printf -- '--\\n' >> \"$d/argv\"\n\
         case \"$1\" in\n\
         list-panes) case \"$4\" in *ccmodel*) cat \"$d/options\" ;; *) cat \"$d/ids\" ;; esac ;;\n\
         set-option) for a in \"$@\"; do case \"$a\" in %8|%9) exit 1 ;; esac; done ;;\n\
         esac\n\
         exit 0\n",
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    let tmux = Tmux {
        program: Program::named(&path),
        timeout: Duration::from_secs(5),
    };
    // Canario: el binario es el falso del HOME de la prueba, sin prefijo.
    assert!(tmux.program.path.starts_with(&home.root));
    assert!(tmux.program.prefix.is_empty());
    (tmux, dir)
}

fn read_and_clear(path: &Path) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    std::fs::write(path, "").unwrap();
    text
}

/// `config/model-tiers.json` propio: patrones, un símbolo entero y colores
/// de tmux.
fn custom_tiers() -> Value {
    json!({
        "alertTier": "high",
        "defaultTier": "mid",
        "patterns": [
            {"match": "opus|sol", "tier": "high"},
            {"match": "haiku|mini", "tier": "low"}
        ],
        "tiers": {
            "high": {"symbol": "$$$", "label": "Alto", "tmux": "196"},
            "mid": {"symbol": "$$", "label": "Medio"},
            "low": {"symbol": 1, "label": "Bajo", "tmux": 46}
        }
    })
}

fn write_tiers(root: &Path, tiers: &Value) -> PathBuf {
    let file = root.join("repo/config/model-tiers.json");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, tiers.to_string()).unwrap();
    file
}

/// Fila del memo (`build_usage_state`) de un pane.
fn memo_row(sess: &str, pane: &str, agent: &str, model: &str, pid: i64) -> Value {
    json!({
        "tmux_session": sess, "tmux_pane": pane, "session": sess, "agent": agent,
        "provider": agent, "model": model, "pid": pid, "agent_pid": pid,
        "pane_pwd": "/tmp", "reasoning_effort": ""
    })
}

fn live_row(sess: &str, pane: &str, agent: &str) -> Value {
    json!({"tmux_session": sess, "tmux_pane": pane, "agent": agent, "pane_pwd": "/tmp"})
}

fn card(pane: &str, agent: &str, model: &str) -> Value {
    json!({"pane": pane, "agent": agent, "model": model, "effort": "high", "alive": true})
}

/// Seis vueltas de `/usage/state`: cambio de modelo con aviso, cambio en
/// curso, pane muerto, enfriamiento de una hora y reintento.
fn e2e_steps(work: i64, main: i64, gk: i64) -> Value {
    let t = NOW;
    let ids = "%1\n%2\n%3\n%4\n%5\n%6\n";
    let live = json!([
        live_row("alpha", "%1", "claude"),
        live_row("alpha", "%2", "codex"),
        live_row("beta", "%3", "grok"),
        live_row("beta", "%4", "claude"),
        live_row("gamma", "%5", "opencode"),
        live_row("gamma", "%6", ""),
    ]);
    let memo = |m1: &str, m3: &str| {
        json!([
            memo_row("alpha", "%1", "claude", "claude-opus-4-7", work),
            memo_row("alpha", "%2", "codex", "openai/gpt-5.6-sol", main),
            memo_row("beta", "%3", "grok", m3, gk),
            memo_row("beta", "%4", "claude", "", 0),
            memo_row("gamma", "%5", "opencode", "", 0),
            memo_row("gamma", "%6", "", "", 0),
            memo_row("gamma", "%7", "codex", "gpt-5.5", main),
            memo_row("local", "local", "codex", "gpt-5.5", main),
            json!({"tmux_session": "alpha", "tmux_pane": "%1", "agent": "claude",
                   "model": m1, "pid": work}),
        ])
    };
    let cards = |m1: &str| {
        json!([
            card("%1", "claude", m1),
            card("%2", "codex", ""),
            card("", "codex", "x"),
            card("%9", "codex", "gpt-5.5"),
        ])
    };
    let switching = json!({"alpha|%2": {"stage": "cambiando", "ts": t - 10.0,
                                         "model": "anthropic/claude-haiku-4"}});
    let done = json!({"alpha|%2": {"stage": "listo", "ok": true, "ts": t - 10.0}});
    let options = "%1\t\n%2\tviejo\n%7\tresto\n";
    json!([
        {"now": t, "ids": ids, "options": options, "motor": switching,
         "live": live, "memo": memo("x", "grok-4-mini"), "cards": cards("claude-sonnet-4-5")},
        {"now": t + 10.0, "ids": ids, "options": options, "motor": done,
         "live": live, "memo": memo("x", "grok-4-mini"), "cards": cards("claude-opus-4-7")},
        {"now": t + 20.0, "ids": ids, "options": options, "motor": done,
         "live": [live[0], live[1], live[2], live_row("delta", "%9", "")],
         "memo": [memo("x", "grok-4-sol")[0], memo("x", "grok-4-sol")[1],
                  memo("x", "grok-4-sol")[2], memo_row("delta", "%9", "", "", 0)],
         "cards": cards("claude-sonnet-4-5")},
        {"now": t + 30.0, "ids": ids, "options": options, "motor": {},
         "live": live, "memo": memo("x", "grok-4-sol"), "cards": cards("claude-opus-4-7")},
        {"now": t + 3700.0, "ids": ids, "options": options, "motor": {},
         "live": live, "memo": memo("x", "grok-4-sol"), "cards": cards("claude-sonnet-4-5")},
        {"now": t + 3720.0, "ids": ids, "options": options, "motor": {},
         "live": [live[0], live[1], live_row("eps", "%8", "opencode")],
         "memo": [memo("x", "x")[0], memo("x", "x")[1], memo_row("eps", "%8", "opencode", "", 0)],
         "cards": cards("claude-opus-4-7")},
    ])
}

/// `write_pane_models(_pane_models_for_live_state(panes, state))` del
/// `/usage/state` del Python, con hilos síncronos (la reconciliación corre
/// dentro de la llamada), `usage_alert_send` anotado, `read_states_cached`
/// sustituido y el `tmux` falso del PATH.
const E2E_ORACLE: &str = r#"
steps = json.load(open(sys.argv[2]))
fake = sys.argv[3]
dash.MODEL_TIERS_FILE = sys.argv[4]
dash._model_tiers_cache.update(mtime=None, data={})
dash.threading.Thread = SyncThread
ALERTS = []
def _alert(message, title=None, kind="info", project=None):
    ALERTS.append([message, title, kind, project])
dash.usage_alert_send = _alert
open(os.path.join(fake, "argv"), "w").close()
out = []
for step in steps:
    dash.time.time = lambda now=step["now"]: now
    open(os.path.join(fake, "ids"), "w").write(step["ids"])
    open(os.path.join(fake, "options"), "w").write(step["options"])
    dash.MOTOR_RESULT.clear()
    dash.MOTOR_RESULT.update(step["motor"])
    dash.read_states_cached = lambda cards=step["cards"]: cards
    rows = dash._pane_models_for_live_state(step["live"], {"panes": step["memo"]})
    del ALERTS[:]
    dash.write_pane_models(rows)
    st = dash._pane_model_state
    argv_file = os.path.join(fake, "argv")
    argv = open(argv_file).read()
    open(argv_file, "w").close()
    f = dash.PANE_MODELS_FILE
    out.append({"rows": rows, "applied": dict(st["applied"]), "argv": argv,
                "file": open(f).read() if os.path.exists(f) else None,
                "alerts": list(ALERTS), "retry": st["retry_after"] > time.monotonic(),
                "known": len(set(dash._TIER_LAST) | set(dash._TIER_ALERTED))})
print(json.dumps(out))
"#;

#[tokio::test]
async fn usage_state_borders_match_python_twin() {
    if !python_available() {
        return;
    }
    let py = TestHome::new("borders-py");
    let rs = TestHome::new("borders-rs");
    // Cuentas: una de Claude con su carpeta, Codex en la principal (sin
    // `CODEX_HOME`) y Grok con barra final. Las carpetas las leen los dos.
    let shared = py.root.join("accounts");
    std::fs::create_dir_all(shared.join("work")).unwrap();
    std::fs::create_dir_all(shared.join("gk")).unwrap();
    std::fs::write(
        shared.join("work/.claude.json"),
        r#"{"oauthAccount": {"emailAddress": "w@example.com"}}"#,
    )
    .unwrap();
    let work = Sleeper::spawn(&[("CLAUDE_CONFIG_DIR", &shared.join("work"))]);
    let main = Sleeper::spawn(&[]);
    let gk_dir = PathBuf::from(format!("{}/", shared.join("gk").display()));
    let gk = Sleeper::spawn(&[("GROK_HOME", &gk_dir)]);
    let steps = e2e_steps(work.pid(), main.pid(), gk.pid());
    for home in [&py, &rs] {
        home.write("cc-notify.conf", "CC_LANG=es\n");
    }

    let (_, py_fake) = fake_tmux(&py);
    let py_tiers = write_tiers(&py.root, &custom_tiers());
    let steps_file = py.root.join("steps.json");
    std::fs::write(&steps_file, steps.to_string()).unwrap();
    let expected: Value = serde_json::from_str(&run_python(
        &format!("{PRELUDE}{E2E_ORACLE}"),
        &[
            steps_file.as_os_str(),
            py_fake.as_os_str(),
            py_tiers.as_os_str(),
        ],
        &py,
    ))
    .unwrap();

    let (tmux, rs_fake) = fake_tmux(&rs);
    write_tiers(&rs.root, &custom_tiers());
    let clock = Arc::new(std::sync::atomic::AtomicI64::new(NOW_MS));
    let mut opts = rs.options();
    opts.repo_root = Some(rs.root.join("repo"));
    opts.tmux = tmux.clone();
    let shared_clock = clock.clone();
    opts.clock = Arc::new(move || shared_clock.load(std::sync::atomic::Ordering::SeqCst));
    let native = Native::new(opts);
    let writer = native.pane_models().clone();
    let mut seen = Vec::new();
    for step in steps.as_array().unwrap() {
        let now = step["now"].as_f64().unwrap();
        clock.store((now * 1000.0) as i64, std::sync::atomic::Ordering::SeqCst);
        std::fs::write(rs_fake.join("ids"), step["ids"].as_str().unwrap()).unwrap();
        std::fs::write(rs_fake.join("options"), step["options"].as_str().unwrap()).unwrap();
        std::fs::write(
            rs.hooks().join("motor-results.json"),
            step["motor"].to_string(),
        )
        .unwrap();
        let live: Vec<serde_json::Map<String, Value>> =
            serde_json::from_value(step["live"].clone()).unwrap();
        let cards = step["cards"].as_array().unwrap().clone();
        let state = json!({"panes": step["memo"]});
        let rows = live_rows(&live, &state, Some(&cards)).unwrap();
        // El Python nunca olvida claves: para comparar `known` esta prueba no
        // poda (sin lista de tmux). La poda tiene su propia prueba.
        let PaneOutcome::Ready(values) = pane_values(&native, &rows, None).await else {
            panic!("la vuelta debía escribir");
        };
        let alerts: Vec<Value> = values
            .alerts
            .iter()
            .map(|a| {
                let (title, msg) = a.texts(true);
                json!([msg, title, "done", a.session])
            })
            .collect();
        writer
            .apply(values.values, values.file_text, tmux.clone(), rs.hooks())
            .await;
        wait_idle(&writer).await;
        let file = std::fs::read_to_string(rs.hooks().join("pane-models.txt")).ok();
        seen.push(json!({
            "rows": rows, "applied": writer.applied(),
            "argv": read_and_clear(&rs_fake.join("argv")),
            "file": file, "alerts": alerts, "retry": writer.retry_pending(),
            "known": native.tier_alerts().len(),
        }));
    }
    native.shutdown().await;
    let seen = Value::Array(seen);
    for (i, (ours, theirs)) in seen
        .as_array()
        .unwrap()
        .iter()
        .zip(expected.as_array().unwrap())
        .enumerate()
    {
        assert_eq!(sorted(ours), sorted(theirs), "vuelta {i}");
    }
    assert_eq!(seen.as_array().unwrap().len(), 6);
    // Contenido concreto, por si los dos lados se equivocaran igual.
    let first_file = seen[0]["file"].as_str().unwrap();
    assert!(
        first_file.contains("%1 work · claude · sonnet-4-5 $$\n"),
        "{first_file}"
    );
    assert!(first_file.contains("%2 codex · cambiando → haiku-4\n"));
    assert!(first_file.contains("%3 gk · grok · grok-4-mini 1\n"));
    assert!(first_file.contains("%4 claude · detectando…\n"));
    assert!(first_file.contains("%5 opencode\n"));
    assert!(!first_file.contains("%6"));
    assert!(seen[0]["argv"].as_str().unwrap().starts_with(
        "list-panes\n-a\n-F\n#{pane_id}\t#{@ccmodel}\n--\nset-option\n-p\n-t\n%1\n@ccmodel\n"
    ));
    assert_eq!(seen[1]["alerts"][0][1], "Modelo Alto en uso");
    assert_eq!(seen[2]["alerts"][0][3], "beta");
    assert_eq!(seen[3]["alerts"], json!([]), "dentro de la hora no avisa");
    assert_eq!(
        seen[5]["alerts"].as_array().unwrap().len(),
        1,
        "pasada la hora sí"
    );
    assert_eq!(
        seen[2]["applied"]["%9"],
        Value::Null,
        "pane muerto: quitado"
    );
    assert_eq!(seen[5]["retry"], true, "fallo con valor: reintento");
    drop((work, main, gk));
}

/// `_pane_models_for_live_state` con tarjetas raras (excepción atrapada →
/// sin reconciliar) y con el agente ausente en tarjeta y fila.
const LIVE_ORACLE: &str = r#"
cases = json.load(open(sys.argv[2]))
out = []
for case in cases:
    dash.read_states_cached = lambda cards=case["cards"]: cards
    out.append(dash._pane_models_for_live_state(case["live"], case["state"]))
print(json.dumps(out))
"#;

#[test]
fn live_rows_match_python() {
    if !python_available() {
        return;
    }
    let live = json!([
        live_row("s", "%1", "codex"),
        live_row("s", "%2", "claude"),
        json!({"tmux_pane": "local"}),
        json!({"tmux_pane": ""})
    ]);
    let state = json!({"panes": [
        {"tmux_pane": "%1", "agent": "codex", "model": "a", "z": 1},
        {"tmux_pane": "%2", "model": "b"},
        {"tmux_pane": "%3", "agent": "codex", "model": "c"},
        {"tmux_pane": "local", "agent": "codex", "model": "d"},
    ]});
    let cases = json!([
        {"live": live, "state": state, "cards": [
            {"pane": "%1", "model": "m1", "agent": "", "effort": "xhigh"},
            {"pane": "%2", "model": "m2", "effort": ""},
            {"pane": "%2", "model": "m2b"},
            {"pane": 7, "model": "num"},
            {"pane": "", "model": "nada"},
        ]},
        {"live": live, "state": state, "cards": [{"pane": "%1", "model": "m1"}, 5]},
        {"live": live, "state": state, "cards": [{"pane": ["%1"], "model": "m1"}]},
        {"live": live, "state": state, "cards": [{"pane": "%1", "model": ""}]},
        {"live": live, "state": {"panes": null}, "cards": []},
        {"live": [], "state": state, "cards": []},
    ]);
    let home = TestHome::new("live-rows");
    let file = home.root.join("cases.json");
    std::fs::write(&file, cases.to_string()).unwrap();
    let expected: Value = serde_json::from_str(&run_python(
        &format!("{PRELUDE}{LIVE_ORACLE}"),
        &[file.as_os_str()],
        &home,
    ))
    .unwrap();
    let mut seen = Vec::new();
    for case in cases.as_array().unwrap() {
        let live: Vec<serde_json::Map<String, Value>> =
            serde_json::from_value(case["live"].clone()).unwrap();
        let cards = case["cards"].as_array().unwrap().clone();
        seen.push(json!(
            live_rows(&live, &case["state"], Some(&cards)).unwrap()
        ));
    }
    assert_eq!(Value::Array(seen), expected);
    assert_eq!(
        expected[0][1]["agent"],
        Value::Null,
        "agente ausente → None"
    );
    assert!(
        live_rows(&[], &state, None).is_none(),
        "sin tarjetas no hay vuelta"
    );
}

/// `_pane_model_values` por rondas: `ok` con valores y texto, o `raise`; y los
/// avisos de cada ronda (también los de antes de una excepción).
const VALUES_ORACLE: &str = r#"
cases = json.load(open(sys.argv[2]))
tiers_file = sys.argv[3]
dash.threading.Thread = SyncThread
ALERTS = []
dash.usage_alert_send = lambda m, title=None, kind="info", project=None: ALERTS.append([m, title, project])
out = []
for case in cases:
    open(tiers_file, "w").write(json.dumps(case["tiers"]))
    dash.MODEL_TIERS_FILE = tiers_file
    dash._model_tiers_cache.update(mtime=None, data={})
    dash._TIER_LAST.clear(); dash._TIER_ALERTED.clear()
    dash.MOTOR_RESULT.clear(); dash.MOTOR_RESULT.update(case["motor"])
    dash.time.time = lambda now=case["now"]: now
    rounds = []
    for rows in case["rounds"]:
        del ALERTS[:]
        try:
            values, text = dash._pane_model_values(rows)
            rounds.append({"ok": [values, text], "alerts": list(ALERTS)})
        except Exception:
            rounds.append({"raise": True, "alerts": list(ALERTS)})
    out.append(rounds)
print(json.dumps(out))
"#;

#[tokio::test]
async fn pane_values_raise_like_python() {
    if !python_available() {
        return;
    }
    let p = |extra: Value| {
        let mut row = json!({"tmux_pane": "%1", "tmux_session": "s", "agent": "codex",
                             "model": "gpt-5.5"});
        for (k, v) in extra.as_object().unwrap() {
            row[k] = v.clone();
        }
        row
    };
    let tiers = custom_tiers();
    let cases = json!([
        // Modelo que no es texto: `.replace` lanza.
        {"now": NOW, "tiers": tiers, "motor": {}, "rounds": [[p(json!({"model": 5}))]]},
        // `tmux_pane` que no es texto: `.startswith` lanza.
        {"now": NOW, "tiers": tiers, "motor": {}, "rounds": [[p(json!({"tmux_pane": 7}))]]},
        // Agente lista: `AGENT_ACCOUNT_ENV.get` lanza `TypeError`.
        {"now": NOW, "tiers": tiers, "motor": {}, "rounds": [[p(json!({"agent": ["x"]}))]]},
        // `float("abc")` de un cambio en curso.
        {"now": NOW, "tiers": tiers, "motor": {"s|%1": {"stage": "x", "ts": "abc"}},
         "rounds": [[p(json!({}))]]},
        // `tiers` que no es objeto: `tier_style` lanza.
        {"now": NOW, "tiers": {"tiers": [1]}, "motor": {}, "rounds": [[p(json!({}))]]},
        // Agente numérico (sin cuenta, color 244) y sin `tmux_pane`/con otro.
        {"now": NOW, "tiers": tiers, "motor": {}, "rounds": [[
            p(json!({"agent": 3, "model": ""})),
            p(json!({"tmux_pane": "", "model": 5})),
            p(json!({"tmux_pane": "local", "model": 5})),
            p(json!({"tmux_pane": "%2", "agent": "", "provider": "grok", "model": ""})),
            p(json!({"tmux_pane": "%2", "agent": "", "model": ""})),
        ]]},
        // Aviso decidido antes de una excepción: sale igual.
        {"now": NOW, "tiers": tiers, "motor": {}, "rounds": [
            [p(json!({"model": "gpt-5.5"}))],
            [p(json!({"model": "gpt-5.6-sol"})), p(json!({"tmux_pane": "%2", "model": 5}))],
        ]},
        // `alertTier` que no es texto: nunca avisa.
        {"now": NOW, "tiers": {"alertTier": 1, "defaultTier": "mid",
                   "patterns": [{"match": "sol", "tier": "high"}]},
         "motor": {}, "rounds": [
            [p(json!({"model": "gpt-5.5"}))],
            [p(json!({"model": "gpt-5.6-sol"}))],
        ]},
    ]);
    let py = TestHome::new("values-raise-py");
    let file = py.root.join("cases.json");
    std::fs::write(&file, cases.to_string()).unwrap();
    let tiers_file = py.root.join("model-tiers.json");
    let expected: Value = serde_json::from_str(&run_python(
        &format!("{PRELUDE}{VALUES_ORACLE}"),
        &[file.as_os_str(), tiers_file.as_os_str()],
        &py,
    ))
    .unwrap();

    let rs = TestHome::new("values-raise-rs");
    rs.write("cc-notify.conf", "CC_LANG=es\n");
    let mut seen = Vec::new();
    for case in cases.as_array().unwrap() {
        write_tiers(&rs.root, &case["tiers"]);
        std::fs::write(
            rs.hooks().join("motor-results.json"),
            case["motor"].to_string(),
        )
        .unwrap();
        let mut opts = rs.options();
        opts.repo_root = Some(rs.root.join("repo"));
        let native = Native::new(opts);
        let mut rounds = Vec::new();
        for rows in case["rounds"].as_array().unwrap() {
            let rows: Vec<serde_json::Map<String, Value>> =
                serde_json::from_value(rows.clone()).unwrap();
            let texts = |alerts: &[TierAlert]| -> Value {
                alerts
                    .iter()
                    .map(|a| {
                        let (title, msg) = a.texts(false);
                        json!([msg, title, a.session])
                    })
                    .collect()
            };
            rounds.push(match pane_values(&native, &rows, None).await {
                PaneOutcome::Ready(v) => {
                    json!({"ok": [v.values, v.file_text], "alerts": texts(&v.alerts)})
                }
                PaneOutcome::Raises(alerts) => json!({"raise": true, "alerts": texts(&alerts)}),
                PaneOutcome::Skip => panic!("nada incierto en estos casos"),
            });
        }
        native.shutdown().await;
        seen.push(Value::Array(rounds));
    }
    assert_eq!(Value::Array(seen.clone()), expected);
    assert_eq!(seen[6][1]["raise"], true);
    assert_eq!(seen[6][1]["alerts"].as_array().unwrap().len(), 1);
    assert_eq!(
        seen[5][0]["ok"][1], "%1 3\n%2 grok · detectando…\n",
        "la línea anterior del pane se queda aunque el valor sea None"
    );
}

#[tokio::test]
async fn pane_values_skip_when_inputs_are_uncertain() {
    let home = TestHome::new("values-skip");
    let row = json!({"tmux_pane": "%1", "tmux_session": "s", "agent": "codex",
                     "model": "gpt-5.6-sol"});
    let rows: Vec<serde_json::Map<String, Value>> = vec![row.as_object().unwrap().clone()];
    write_tiers(&home.root, &custom_tiers());
    let native_with = |repo: Option<PathBuf>| {
        let mut opts = home.options();
        opts.repo_root = repo;
        Native::new(opts)
    };
    let repo = Some(home.root.join("repo"));
    // `motor-results.json` roto o con valores que no son objeto: incierto.
    for broken in ["{", r#"{"s|%1": 3}"#] {
        home.write("motor-results.json", broken);
        let native = native_with(repo.clone());
        assert_eq!(pane_values(&native, &rows, None).await, PaneOutcome::Skip);
        assert!(native.tier_alerts().is_empty(), "sin observar nada");
        native.shutdown().await;
    }
    std::fs::remove_file(home.hooks().join("motor-results.json")).unwrap();
    // Sin `model-tiers.json` (el Python usaría su copia en caché): incierto.
    let native = native_with(None);
    assert_eq!(pane_values(&native, &rows, None).await, PaneOutcome::Skip);
    native.shutdown().await;
    // Sesión numérica (clave de `dict` que no se reproduce): incierto.
    let native = native_with(repo.clone());
    let mut odd = rows.clone();
    odd[0].insert("tmux_session".into(), json!(4));
    assert_eq!(pane_values(&native, &odd, None).await, PaneOutcome::Skip);
    assert!(native.tier_alerts().is_empty());
    // Pid que no es entero con cuenta posible: incierto.
    let mut odd = rows.clone();
    odd[0].insert("pid".into(), json!("12"));
    assert_eq!(pane_values(&native, &odd, None).await, PaneOutcome::Skip);
    // Lo normal sí escribe.
    let PaneOutcome::Ready(values) = pane_values(&native, &rows, None).await else {
        panic!("debía escribir");
    };
    assert_eq!(values.file_text, "%1 codex · gpt-5.6-sol $$$\n");
    assert_eq!(native.tier_alerts().len(), 1);
    native.shutdown().await;
}

#[tokio::test]
async fn pane_values_bound_tier_memory_with_all_tmux_panes() {
    let home = TestHome::new("values-prune");
    write_tiers(&home.root, &custom_tiers());
    let clock = Arc::new(std::sync::atomic::AtomicI64::new(NOW_MS));
    let mut opts = home.options();
    opts.repo_root = Some(home.root.join("repo"));
    let shared = clock.clone();
    opts.clock = Arc::new(move || shared.load(std::sync::atomic::Ordering::SeqCst));
    let native = Native::new(opts);
    let row = |pane: &str| {
        json!({"tmux_pane": pane, "tmux_session": "s", "agent": "codex", "model": "gpt-5.5"})
            .as_object()
            .unwrap()
            .clone()
    };
    let all: BTreeSet<String> = ["%1".to_owned(), "%2".to_owned()].into();
    let _ = pane_values(&native, &[row("%1"), row("%2")], Some(&all)).await;
    assert_eq!(native.tier_alerts().len(), 2);
    // %2 sigue vivo en tmux aunque esta vuelta no tenga fila: no se olvida.
    let _ = pane_values(&native, &[row("%1")], Some(&all)).await;
    assert_eq!(native.tier_alerts().len(), 2);
    // Sin la lista de tmux (falló) no se poda nada.
    let only: BTreeSet<String> = ["%1".to_owned()].into();
    let _ = pane_values(&native, &[row("%1")], None).await;
    assert_eq!(native.tier_alerts().len(), 2);
    // %2 murió en tmux y nunca avisó: se olvida.
    let _ = pane_values(&native, &[row("%1")], Some(&only)).await;
    assert_eq!(native.tier_alerts().len(), 1);
    native.shutdown().await;
}

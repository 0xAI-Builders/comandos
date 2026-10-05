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
        HyperNotify, NotifyPost, PaneModelWriter, TIER_ALERT_COOLDOWN_S, TierAlert, TierAlerts,
        send_alerts, ui_lang_es,
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
    // `tiers` verdadero que no es objeto, o símbolo que no es texto: incierto.
    assert!(TierAlert::styled("s", "a", "m", "high", &json!({"tiers": [1]})).is_err());
    assert!(
        TierAlert::styled(
            "s",
            "a",
            "m",
            "high",
            &json!({"tiers": {"high": {"symbol": 3}}})
        )
        .is_err()
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

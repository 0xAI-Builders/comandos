//! read_states (bin/cc-dash) con sus efectos sustituidos, contra
//! states::cards + suggest + tab_models con efectos falsos idénticos; y
//! observe_pane con su recolección sustituida, contra states::observe.
mod support;

use comandos_core::json::response_dumps;
use comandos_runtime::{
    agent_procs::{
        AgentInfo, AgentMaps, AgentProc, PaneRow, agent_pane_maps, external_agents, process_owners,
    },
    providers::harness_has_accounts,
    tui_state::StateTracker,
};
use comandos_server::dash::native::{
    states::{
        StateFault,
        cards::{self, CardEffects, Inputs},
        observe::{self, ObserveEvidence, Observed},
        records::RecordCache,
        suggest::{self, SuggestContext},
        tab_models::tab_models,
    },
    tmux::{Output, TmuxError},
};
use serde_json::{Map, Value, json};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
    time::Duration,
};

const ORACLE: &str = r#"
import importlib.machinery, importlib.util, json, os, sys, types
repo, case, out_path = sys.argv[1:4]
sys.path.insert(0, os.path.join(repo, "bin"))
loader = importlib.machinery.SourceFileLoader("cc_dash_oracle", os.path.join(repo, "bin/cc-dash"))
spec = importlib.util.spec_from_loader(loader.name, loader)
dash = importlib.util.module_from_spec(spec)
loader.exec_module(dash)
c = json.load(open(case))
dash.STATE = c["stateDir"]
dash.APP_TAB_MODELS_FILE = out_path
dash.time.time = lambda: c["now"]
dash.tmux_pane_inventory = lambda: c["panes"]
dash.agent_procs = lambda: [tuple(p) for p in c["procs"]]
dash.parent_pid = lambda pid: c["parents"].get(str(pid), 0)
dash._proc_cmdline = lambda pid: c["cmdlines"].get(str(pid), [])
dash.pane_snapshot.PaneInspector = lambda *a, **k: None
dash.session_labels = lambda: c["labels"]
def tmux(*args, timeout=5):
    hit = c["tmux"].get("\x1f".join(args))
    if hit is None:
        return types.SimpleNamespace(returncode=1, stdout="", stderr="no")
    return types.SimpleNamespace(returncode=hit[0], stdout=hit[1], stderr="")
dash.tmux = tmux
dash.account_for_pid = lambda pid, agent: dict(c["accounts"].get(str(pid), {}))
dash.grok_metadata_for_pid = lambda pid: dict(c["grok"].get(str(pid), {}))
def observe_pane(sess, pane, info=None, inspector=None):
    seen = c["observed"].get(pane)
    if seen is None:
        raise ValueError("el panel ya no existe")
    return dict(seen)
dash.observe_pane = observe_pane
dash.cc_usage.latest_session_config = lambda db, s, p: dict(c["configs"].get(s + "|" + p, {}))
real_run = dash.subprocess.run
def run(argv, **kw):
    if argv[:3] == ["ssh", "-O", "check"]:
        return types.SimpleNamespace(returncode=0 if c["ssh"].get(argv[3]) else 255)
    return real_run(argv, **kw)
dash.subprocess.run = run
latency = {(l[0], l[1]): (l[2], l[3]) for l in c["latency"]}
dash._suggestion_context = lambda: (c["guard"], set(c["routes"]), latency)
dash.MOTOR_RESULT.clear()
dash.MOTOR_RESULT.update(c["motor"])
dash.load_provider_registry = lambda: c["registry"]
dash.load_model_tiers = lambda: c["tiers"]
print(json.dumps(dash.read_states()))
"#;

/// `observe_pane` real con la recolección (tmux, /proc, transcripts, cuentas)
/// sustituida por lo que dice cada caso; un solo rastreador para todos.
const OBSERVE_ORACLE: &str = r#"
import importlib.machinery, importlib.util, json, os, sys, types
repo, case = sys.argv[1:3]
sys.path.insert(0, os.path.join(repo, "bin"))
loader = importlib.machinery.SourceFileLoader("cc_dash_oracle", os.path.join(repo, "bin/cc-dash"))
spec = importlib.util.spec_from_loader(loader.name, loader)
dash = importlib.util.module_from_spec(spec)
loader.exec_module(dash)
c = json.load(open(case))
dash.load_provider_registry = lambda: c["registry"]
out = []
for k in c["cases"]:
    dash.time.time = lambda k=k: k["now"]
    dash._pane_identity = lambda s, p, k=k: dict(k["identity"])
    dash.account_for_pid = lambda pid, agent, k=k: dict(k["account"])
    dash._proc_cmdline = lambda pid, k=k: list(k["cmdline"])
    dash.glob.glob = lambda pattern, k=k: list(k["glob"])
    dash.os.readlink = lambda fd, k=k: k["link"]
    dash._TUI_TRANSCRIPTS = types.SimpleNamespace(read=lambda h, s, p, k=k: dict(k["transcript"]))
    dash.grok_metadata_for_pid = lambda pid, k=k: dict(k["grok"])
    dash.acp_state_for_pane = lambda pane, k=k: dict(k["acp"])
    dash.pane_visible_config = lambda pane, harness, k=k: dict(k["visible"])
    dash._process_start = lambda pid, k=k: k["start"]
    dash._read_environ = lambda pid, k=k: {kk.encode(): v.encode() for kk, v in k["environ"].items()}
    inspector = lambda row, k=k: dict(k["snap"])
    info = {"session": k["session"], "pane": k["pane"], "cwd": "/w", "agent": k["agent"], "pid": k["pid"]}
    out.append(dash.observe_pane(k["session"], k["pane"], info, inspector))
print(json.dumps(out))
"#;

struct FakeEffects<'a>(&'a Value);

impl CardEffects for FakeEffects<'_> {
    async fn tmux(&self, args: &[&str], _timeout: Duration) -> Result<Output, TmuxError> {
        Ok(match self.0["tmux"].get(args.join("\u{1f}")) {
            None => Output {
                ok: false,
                stdout: String::new(),
                stderr: "no".into(),
            },
            Some(hit) => Output {
                ok: hit[0] == json!(0),
                stdout: hit[1].as_str().unwrap().into(),
                stderr: String::new(),
            },
        })
    }
    async fn account(&self, pid: i64, _agent: &str) -> Result<Map<String, Value>, StateFault> {
        Ok(self.0["accounts"]
            .get(pid.to_string())
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default())
    }
    async fn grok_metadata(&self, pid: i64) -> Result<Map<String, Value>, StateFault> {
        Ok(self.0["grok"]
            .get(pid.to_string())
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default())
    }
    async fn observe(&self, _s: &str, pane: &str, _i: &AgentInfo) -> Result<Observed, StateFault> {
        Ok(
            match self.0["observed"].get(pane).and_then(Value::as_object) {
                Some(seen) => Observed::Seen(seen.clone()),
                None => Observed::Unconfirmed,
            },
        )
    }
    async fn session_config(
        &self,
        s: &str,
        p: &str,
    ) -> Result<Option<Map<String, Value>>, StateFault> {
        Ok(self.0["configs"]
            .get(format!("{s}|{p}"))
            .and_then(Value::as_object)
            .cloned())
    }
    async fn ssh_check(&self, host: &str) -> bool {
        self.0["ssh"].get(host).is_some_and(|v| v == &json!(true))
    }
}

/// Lo que el escaneo real (Tarea 5) produce, construido desde el caso.
fn inputs(c: &Value, records: &mut RecordCache) -> Inputs {
    let panes: Vec<PaneRow> = c["panes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| PaneRow {
            session: p["session"].as_str().unwrap().into(),
            pane: p["pane"].as_str().unwrap().into(),
            pane_pid: p["pane_pid"].as_i64().unwrap(),
            command: p["command"].as_str().unwrap().into(),
            cwd: p["cwd"].as_str().unwrap().into(),
            activity: p["activity"].as_f64().unwrap(),
            pane_active: p["paneActive"].as_bool().unwrap(),
        })
        .collect();
    let procs: Vec<AgentProc> = c["procs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| AgentProc {
            pid: p[0].as_i64().unwrap(),
            cwd: p[1].as_str().unwrap().into(),
            agent: p[2].as_str().unwrap().into(),
        })
        .collect();
    let mut parent = |pid: i64| {
        c["parents"]
            .get(pid.to_string())
            .and_then(Value::as_i64)
            .unwrap_or(0)
    };
    let mut cmdline = |pid: i64| -> Vec<String> {
        serde_json::from_value(c["cmdlines"][pid.to_string()].clone()).unwrap_or_default()
    };
    let owners = process_owners(&procs, &panes, &mut parent);
    let maps: AgentMaps = agent_pane_maps(&procs, &panes, &owners, &mut cmdline, &mut parent);
    let external: HashSet<(String, String)> = external_agents(&procs, &owners, &mut parent);
    let labels: HashMap<String, String> = serde_json::from_value(c["labels"].clone()).unwrap();
    let scanned = records
        .scan(Path::new(c["stateDir"].as_str().unwrap()))
        .unwrap();
    Inputs {
        now: c["now"].as_f64().unwrap(),
        panes,
        maps,
        external,
        labels,
        records: scanned,
    }
}

async fn rust_side(c: &Value) -> (String, String) {
    let mut cache = RecordCache::default();
    let inputs = inputs(c, &mut cache);
    let effects = FakeEffects(c);
    let mut items = cards::build(&inputs, &effects).await.unwrap();
    let motor = c["motor"].as_object().cloned().unwrap();
    let ctx = SuggestContext {
        guard: c["guard"].clone(),
        routes: c["routes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r.as_str().unwrap().to_owned())
            .collect(),
        latency: c["latency"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| ((l[0].clone(), l[1].clone()), (l[2].clone(), l[3].clone())))
            .collect(),
    };
    suggest::annotate_all(&mut items, &ctx, &motor, inputs.now).unwrap();
    cards::sort_items(&mut items).unwrap();
    let models = tab_models(&items, &c["registry"], &motor, &c["tiers"], inputs.now).unwrap();
    (
        response_dumps(&Value::Array(items)).unwrap(),
        response_dumps(&models).unwrap(),
    )
}

fn write_state(dir: &Path, name: &str, record: Value) {
    fs::write(dir.join(name), record.to_string()).unwrap();
}

fn config_json(name: &str) -> Value {
    serde_json::from_str(&fs::read_to_string(support::repo().join("config").join(name)).unwrap())
        .unwrap()
}

fn base_case(dir: &Path) -> Value {
    let state = dir.join("state");
    fs::create_dir_all(&state).unwrap();
    // Con fracción: `int(time.time())` decide los bordes de 600 s y 86400 s.
    let now = 1_791_115_200.75_f64;
    let now_ts = now.trunc();
    write_state(
        &state,
        "exact.json",
        json!({"session":"mixed","pane":"%3","agent":"grok","status":"waiting","detail":"exact question","ts":now - 50.0,"cwd":"/stale"}),
    );
    write_state(
        &state,
        "duplicate.json",
        json!({"session":"mixed","pane":"%3","agent":"grok","status":"working","ts":now - 60.0}),
    );
    write_state(
        &state,
        "legacy.json",
        json!({"session":"mixed","agent":"grok","status":"waiting","ts":now - 10.0}),
    );
    write_state(
        &state,
        "old-done.json",
        json!({"project":"gone.proj","status":"done","ts":now - 300.0,"cwd":"/ext","agent":"codex"}),
    );
    write_state(
        &state,
        "done-border.json",
        json!({"session":"gone2","pane":"%40","status":"done","ts":now_ts - 600.0}),
    );
    write_state(
        &state,
        "done-over.json",
        json!({"session":"gone3","status":"done","ts":now_ts - 601.0}),
    );
    write_state(
        &state,
        "zombie.json",
        json!({"session":"ghost","status":"waiting","ts":now - 200_000.0,"pane":"%77"}),
    );
    write_state(
        &state,
        "zombie-newer.json",
        json!({"session":"ghost","status":"waiting","ts":now - 100.0,"pane":"%77","detail":"más reciente"}),
    );
    write_state(
        &state,
        "str-ts.json",
        json!({"session":"term","pane":"%6","agent":"claude","status":"working","ts":"1791115100"}),
    );
    write_state(&state, "bad-ts.json", json!({"session":"x","ts":"mañana"}));
    write_state(&state, "list-ts.json", json!({"session":"x","ts":[1]}));
    write_state(&state, "array.json", json!([1, 2]));
    fs::write(state.join("roto.json"), "{").unwrap();
    fs::write(state.join(".oculto.json"), "{}").unwrap();
    fs::write(state.join("nota.txt"), "{}").unwrap();
    fs::create_dir_all(state.join("dir.json")).unwrap();
    json!({
        "stateDir": state,
        "now": now,
        "panes": [
            {"session":"mixed","pane":"%1","pane_pid":100,"command":"node","cwd":"/same","activity":now - 5.0,"paneActive":true},
            {"session":"mixed","pane":"%2","pane_pid":200,"command":"claude","cwd":"/same","activity":now - 5.0,"paneActive":false},
            {"session":"mixed","pane":"%3","pane_pid":300,"command":"grok","cwd":"/same","activity":now - 5.0,"paneActive":false},
            {"session":"term","pane":"%6","pane_pid":600,"command":"claude","cwd":"/shell","activity":now - 9.0,"paneActive":true},
            {"session":"hub","pane":"%7","pane_pid":700,"command":"zsh","cwd":"/h","activity":now,"paneActive":false},
            {"session":"local","pane":"%8","pane_pid":800,"command":"zsh","cwd":"/home/u","activity":0.0,"paneActive":false},
            {"session":"ssh-box","pane":"%9","pane_pid":900,"command":"zsh","cwd":"/r","activity":now - 2.0,"paneActive":false},
            {"session":"work","pane":"%10","pane_pid":1000,"command":"claude","cwd":"/proj/app/","activity":now - 3.0,"paneActive":false}
        ],
        "procs": [[101,"/same","codex"],[201,"/same","claude"],[301,"/same","grok"],[601,"/shell","claude"],[555,"/ext","codex"],[1001,"","claude"]],
        "parents": {"101":100,"201":200,"301":300,"601":600,"555":1,"1001":1000},
        "cmdlines": {"101":["codex"],"201":["claude"],"301":["grok"],"601":["claude"],"1001":["claude"]},
        "labels": {"mixed": "Project", "work": ""},
        "tmux": {
            "capture-pane\u{1f}-p\u{1f}-t\u{1f}%1\u{1f}-S\u{1f}-12": [0, "  Working (12s • esc to interrupt)\n"],
            "capture-pane\u{1f}-p\u{1f}-t\u{1f}%2\u{1f}-S\u{1f}-14": [0, "● hola\n esc to interrupt\n"],
            "capture-pane\u{1f}-p\u{1f}-t\u{1f}%10\u{1f}-S\u{1f}-14": [0, "Do you want to proceed?\n❯ 1. Yes\n"],
            "list-panes\u{1f}-s\u{1f}-t\u{1f}=ssh-box\u{1f}-F\u{1f}#{pane_current_command}": [0, "zsh\nsshd\n"]
        },
        "accounts": {"101": {"account":"main","accountEmail":"a@b.c"}, "201": {"account":"work","accountEmail":""}, "301": {"account":"main","accountEmail":""}, "601": {"account":"main","accountEmail":""}, "1001": {"account":"main","accountEmail":""}},
        "grok": {"301": {"title": "Grok T"}},
        "observed": {
            "%1": {"harness":"codex","pid":101,"conversationId":"c1","model":"gpt-5.6-sol","effort":"xhigh","source":"conversation","observedAt":now,"identity":"i1","harnessAccount":"main","evidenceAt":now,"motor":"codex","motorAccount":"main","confirmed":true,"accountSource":"process-environment","limitations":[]},
            "%2": {"harness":"claude","pid":201,"conversationId":"s2","model":"claude-fable-5[1m]","effort":"max","source":"pane","observedAt":now,"identity":"i2","harnessAccount":"work","motor":"claude","motorAccount":"work","confirmed":true,"accountSource":"process-environment","limitations":[]},
            "%10": {"harness":"claude","pid":1001,"conversationId":"s10","model":"claude-opus-5-20251101","effort":"","source":"conversation","observedAt":now,"identity":"i10","harnessAccount":"main","motor":"claude","motorAccount":"main","confirmed":true,"accountSource":"process-environment","limitations":["effort-unobserved"]}
        },
        "configs": {"mixed|%2": {"id": 7, "tmux_session": "mixed", "tmux_pane": "%2", "model": "claude-opus-5", "effective_at": 1791115000.5}},
        "ssh": {"box": true},
        "guard": {"projects": [{"project": "Project", "level": "warning", "calls10m": 61, "tokensHour": 4200000}], "forecasts": [{"scope": "Fable", "level": "critical", "downtimeHours": 30.5}]},
        "routes": ["claude:claude", "codex:codex"],
        "latency": [["claude-sonnet-5", "low", 4500, 12]],
        "motor": {"mixed|%1": {"stage": "cambiando", "ts": now - 10.0, "model": "gpt-5.6-luna"}},
        "registry": config_json("providers.json"),
        "tiers": config_json("model-tiers.json")
    })
}

fn case_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("cmd-cards-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

async fn compare(tag: &str, case: Value, dir: &Path) {
    let file = dir.join("case.json");
    fs::write(&file, case.to_string()).unwrap();
    let models_py = dir.join("models-py.json");
    let Some(expected) =
        support::oracle::run_python(ORACLE, &[file.as_os_str(), models_py.as_os_str()], dir)
    else {
        return;
    };
    let (items, models) = rust_side(&case).await;
    assert_eq!(items, expected.trim_end(), "{tag}: tarjetas");
    assert_eq!(
        models,
        fs::read_to_string(&models_py).unwrap(),
        "{tag}: app-tab-models.json"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn cards_match_python_oracle() {
    let dir = case_dir("base");
    let case = base_case(&dir);
    compare("base", case, &dir).await;
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "current_thread")]
async fn suggestions_and_models_match_python_oracle() {
    let dir = case_dir("suggest");
    let mut case = base_case(&dir);
    // Sin cambio en curso en %1 (el de %2 ya terminó: `ok`); %3 cambia de motor.
    case["motor"] = json!({"mixed|%2": {"stage": "listo", "ok": true, "ts": 1, "model": "x"},
                           "mixed|%3": {"stage": "cambiando", "ts": "1791115100", "model": "claude-grok/xai/grok-4.5"}});
    case["guard"]["forecasts"] = json!([{"scope": "General", "level": "critical", "downtimeHours": 2.5},
                                        {"scope": "Fable", "level": "warning"}]);
    case["latency"] = json!([
        ["gpt-5.6-luna", "low", 2500, 3],
        ["claude-sonnet-5", "low", 1500.0, 9]
    ]);
    case["guard"]["projects"] = json!([{"project": "shell", "level": "ok", "calls10m": 0},
                                       {"project": "Project", "level": "critical", "calls10m": "7", "tokensHour": 12_500_000.9}]);
    case["observed"]["%1"]["model"] = json!("gpt-5.6-luna");
    // Contexto en texto: `float("81.5")`, la sugerencia de compactar.
    case["accounts"]["601"]["contextPct"] = json!("81.5");
    compare("suggest", case, &dir).await;
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "current_thread")]
async fn stuck_turns_loops_and_project_quota_match_python_oracle() {
    let dir = case_dir("stuck");
    let mut case = base_case(&dir);
    let now = case["now"].as_f64().unwrap();
    let state = dir.join("state");
    // term %6: turno de 25 min con cero llamadas (colgado).
    write_state(
        &state,
        "str-ts.json",
        json!({"session":"term","pane":"%6","agent":"claude","status":"working","ts":format!("{}", now.trunc() - 1500.0)}),
    );
    // mixed %1: la pista de Codex lo pone a trabajar; 25 min y 70 llamadas (loop).
    write_state(
        &state,
        "codex-old.json",
        json!({"session":"mixed","pane":"%1","agent":"codex","status":"idle","ts":now - 1500.0}),
    );
    case["motor"] = json!({});
    case["guard"]["forecasts"] = json!([{"scope": "Fable", "level": "warning"}]);
    case["guard"]["projects"] = json!([{"project": "shell", "level": "ok", "calls10m": 0},
                                       {"project": "same", "level": "critical", "calls10m": "70", "tokensHour": 12_500_000.9}]);
    compare("stuck", case, &dir).await;
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "current_thread")]
async fn annotation_error_stops_remaining_like_python() {
    let dir = case_dir("err");
    let mut case = base_case(&dir);
    // %2 (claude, fable, cwd /same) cae en la rama del proyecto y
    // `int(project.get('tokensHour'))` de una lista lanza TypeError dentro del
    // try: %6, que después tendría la sugerencia de compactar, queda sin ella.
    case["motor"] = json!({});
    case["guard"]["forecasts"] = json!([]);
    case["guard"]["projects"] =
        json!([{"project": "same", "level": "warning", "calls10m": 1, "tokensHour": ["x"]}]);
    case["accounts"]["601"]["contextPct"] = json!(90);
    compare("err", case, &dir).await;
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn unsure_record_declines_scan() {
    let dir = case_dir("unsure");
    fs::write(
        dir.join("a.json"),
        r#"{"session":"s","detail":"\ud800","ts":1}"#,
    )
    .unwrap();
    assert_eq!(
        RecordCache::default().scan(&dir).err(),
        Some(StateFault::Decline)
    );
    fs::write(dir.join("a.json"), r#"{"session":"s","ts":1e400}"#).unwrap();
    assert_eq!(
        RecordCache::default().scan(&dir).err(),
        Some(StateFault::Decline)
    );
    // Dígitos Unicode: `float()` los acepta; no se puede saber con certeza.
    fs::write(dir.join("a.json"), r#"{"session":"s","ts":"١٢"}"#).unwrap();
    assert_eq!(
        RecordCache::default().scan(&dir).err(),
        Some(StateFault::Decline)
    );
    // Texto no numérico con no-ASCII: ValueError seguro, se salta.
    fs::write(dir.join("a.json"), r#"{"session":"s","ts":"mañana"}"#).unwrap();
    assert_eq!(RecordCache::default().scan(&dir).unwrap().len(), 0);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn record_cache_reuses_unchanged_and_drops_removed() {
    let dir = case_dir("cache");
    fs::write(dir.join("a.json"), r#"{"session":"s","ts":"5"}"#).unwrap();
    fs::write(dir.join("b.json"), r#"{"session":"t"}"#).unwrap();
    let mut cache = RecordCache::default();
    let first = cache.scan(&dir).unwrap();
    assert_eq!(first.len(), 2);
    assert!(first.iter().any(|r| r.timestamp == 5.0));
    // Sin cambios: mismos registros.
    assert_eq!(cache.scan(&dir).unwrap().len(), 2);
    fs::remove_file(dir.join("b.json")).unwrap();
    fs::write(dir.join("a.json"), r#"{"session":"s","ts":7.5,"x":1}"#).unwrap();
    let again = cache.scan(&dir).unwrap();
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].timestamp, 7.5);
    assert!(
        RecordCache::default()
            .scan(&dir.join("no-existe"))
            .unwrap()
            .is_empty()
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Lo que la Tarea 5 recogería para un caso del oráculo de observación.
fn evidence(registry: &Value, k: &Value) -> ObserveEvidence {
    let agent = k["agent"].as_str().unwrap().to_owned();
    let snap = k["snap"].as_object().cloned().unwrap();
    let conversation_id = observe::conversation_id(&snap).unwrap();
    let transcript = k["transcript"].as_object().cloned().unwrap();
    let conversation = match agent.as_str() {
        "codex" if comandos_core::json::truthy(&conversation_id) => {
            if comandos_core::json::truthy(transcript.get("model").unwrap_or(&Value::Null)) {
                transcript
            } else {
                Map::new()
            }
        }
        "claude" if comandos_core::json::truthy(&conversation_id) => transcript,
        "grok" => observe::project_metadata(
            k["grok"].as_object().unwrap(),
            &conversation_id,
            "lastActiveAt",
        ),
        "opencode" | "agy" => observe::project_metadata(
            snap.get("nativeMetadata")
                .and_then(Value::as_object)
                .unwrap_or(&Map::new()),
            &conversation_id,
            "updatedAt",
        ),
        _ => Map::new(),
    };
    let id = &k["identity"];
    let identity = [
        "socket_path",
        "pid",
        "server_start",
        "session_id",
        "pane_id",
        "pane_pid",
    ]
    .iter()
    .map(|f| id.get(f).and_then(Value::as_str).unwrap_or(""))
    .collect::<Vec<_>>()
    .join("|");
    let account = k["account"]
        .get("account")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("unknown")
        .to_owned();
    ObserveEvidence {
        harness_has_accounts: harness_has_accounts(registry, &agent),
        agent,
        pid: k["pid"].as_i64().unwrap(),
        identity,
        snap,
        account,
        cmdline: serde_json::from_value(k["cmdline"].clone()).unwrap(),
        conversation,
        acp: k["acp"].as_object().cloned().unwrap(),
        visible: k["visible"].as_object().cloned().unwrap(),
        process_start: k["start"].as_str().unwrap().into(),
        base_url: k["environ"]
            .get("ANTHROPIC_BASE_URL")
            .and_then(Value::as_str)
            .unwrap_or("")
            .into(),
    }
}

fn observe_case(agent: &str, pid: i64, now: f64) -> Value {
    json!({
        "agent": agent, "pid": pid, "session": "s", "pane": format!("%{pid}"), "now": now,
        "identity": {"socket_path": "/tmp/tmux-1/default", "pid": "42", "server_start": "9",
                     "session_id": "$1", "pane_id": format!("%{pid}"), "pane_pid": "7",
                     "pane_current_command": agent, "session_name": "s"},
        "account": {"account": "main", "accountEmail": ""},
        "cmdline": [agent], "glob": [], "link": "", "transcript": {}, "grok": {}, "acp": {},
        "visible": {}, "start": "1234", "environ": {}, "snap": {}
    })
}

#[test]
fn observe_matches_python_oracle() {
    let dir = case_dir("observe");
    let registry = config_json("providers.json");
    let now = 1_791_115_200.25_f64;
    let mut codex = observe_case("codex", 11, now);
    codex["cmdline"] = json!([
        "codex",
        "-m",
        "gpt-5.5",
        "-c",
        "model_reasoning_effort=\"high\"",
        "--effort",
        "-x",
        "--model"
    ]);
    codex["snap"] = json!({"resume_id": "c1"});
    codex["glob"] = json!(["/proc/11/fd/3"]);
    codex["link"] = json!("/home/u/.codex/sessions/rollout-c1.jsonl");
    codex["transcript"] = json!({"model": "gpt-5.6-sol", "effort": "xhigh", "revision": "r1"});
    codex["account"] = json!({"account": "work", "accountEmail": "w@x"});
    // Mismo proceso y conversación, nueva evidencia de pantalla: gana la que cambió.
    let mut codex_again = codex.clone();
    codex_again["now"] = json!(now + 3.0);
    codex_again["visible"] = json!({"model": "gpt-5.6-luna", "effort": "low"});
    let mut claude = observe_case("claude", 12, now);
    claude["snap"] = json!({"resume_id": "s2", "claude_config_dir": "/home/u/.claude-work"});
    claude["glob"] = json!(["/home/u/.claude-work/projects/x/s2.jsonl"]);
    claude["transcript"] = json!({"model": "gpt-5.6-luna", "effort": ""});
    claude["environ"] = json!({"ANTHROPIC_BASE_URL": "http://127.0.0.1:18765"});
    claude["account"] = json!({"account": "", "accountEmail": ""});
    let mut grok = observe_case("grok", 13, now);
    grok["snap"] = json!({"resume_id": "g1"});
    grok["grok"] =
        json!({"sessionId": "g1", "model": "grok-4.5", "effort": "", "lastActiveAt": "t"});
    grok["cmdline"] = json!(["grok", "--model", "grok-4.6"]);
    let mut acp = observe_case("acp", 14, now);
    acp["snap"] = json!({"acp": {"sessionId": "a1"}});
    acp["cmdline"] = json!(["cc-acp", "--model", "x"]);
    acp["acp"] = json!({"pid": 14, "sessionId": "a1", "agent": "codex", "account": "work",
                         "observedModel": "gpt-5.6-sol", "observedEffort": "high",
                         "requestedModel": "", "model": "gpt-5.5", "effortSource": "protocol"});
    let mut acp_other = observe_case("acp", 15, now);
    acp_other["snap"] = json!({"acp": {"sessionId": "a2"}});
    acp_other["acp"] = json!({"pid": 99, "sessionId": "a2", "agent": "codex"});
    let mut opencode = observe_case("opencode", 16, now);
    opencode["snap"] = json!({"resume_id": "o1", "nativeMetadata": {"sessionId": "o1", "model": "opencode/big", "effort": "max", "updatedAt": 5}});
    let gemini = observe_case("gemini", 17, now);
    let mut codex_no_conv = observe_case("codex", 18, now);
    codex_no_conv["cmdline"] = json!([
        "node",
        "--model",
        "gpt-5.5",
        "-c",
        "model_reasoning_effort='low'"
    ]);
    let cases = vec![
        codex,
        codex_again,
        claude,
        grok,
        acp,
        acp_other,
        opencode,
        gemini,
        codex_no_conv,
    ];
    let case = json!({"registry": registry, "cases": cases});
    let file = dir.join("case.json");
    fs::write(&file, case.to_string()).unwrap();
    let Some(expected) = support::oracle::run_python(OBSERVE_ORACLE, &[file.as_os_str()], &dir)
    else {
        return;
    };
    let mut tracker = StateTracker::new(64);
    let mut out = Vec::new();
    for k in case["cases"].as_array().unwrap() {
        let ev = evidence(&registry, k);
        let obs =
            observe::observe(&ev, &mut tracker, &registry, k["now"].as_f64().unwrap()).unwrap();
        out.push(Value::Object(obs));
    }
    assert_eq!(
        response_dumps(&Value::Array(out)).unwrap(),
        expected.trim_end()
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn reconcile_unconfirmed_and_config() {
    let mut item = Map::new();
    item.insert("agent".into(), json!("claude"));
    item.insert("account".into(), json!("main"));
    observe::reconcile(&mut item, &Observed::Unconfirmed, Some(Map::new())).unwrap();
    assert_eq!(
        response_dumps(&Value::Object(item)).unwrap(),
        r#"{"agent": "claude", "account": "unknown", "observedConfig": {"confirmed": false, "model": "", "effort": "", "source": "unconfirmed"}, "lastConfirmedConfig": null, "model": "", "effort": "", "modelSource": "unconfirmed", "configConfirmed": false, "agentSessionId": "", "motor": "unknown", "harnessAccount": "unknown", "motorAccount": "unknown", "routeId": "claude:unknown"}"#
    );
}

/// La Tarea 5 lo corre dentro de un manejador (`HandlerFuture: Send`).
#[test]
fn build_future_is_send() {
    fn is_send<T: Send>(_: &T) {}
    let case = json!({});
    let inputs = Inputs {
        now: 0.0,
        panes: Vec::new(),
        maps: AgentMaps::default(),
        external: HashSet::new(),
        labels: HashMap::new(),
        records: Vec::new(),
    };
    let effects = FakeEffects(&case);
    let future = cards::build(&inputs, &effects);
    is_send(&future);
}

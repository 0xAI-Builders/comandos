//! `lib/tui_state.py` contra `comandos_runtime::tui_state` sobre los mismos casos.
#![recursion_limit = "512"]
#[path = "support/python.rs"]
mod python;

use comandos_core::json::response_dumps;
use comandos_runtime::tui_state::{
    GrokMetadataCache, StateTracker, TranscriptCache, model_id, screen_state,
};
use python::run_python;
use serde_json::{Value, json};
use std::{collections::HashMap, fs, path::Path};

const ORACLE: &str = r#"
import json, os, sys
repo, cases, work = sys.argv[1:4]
sys.path.insert(0, os.path.join(repo, "lib"))
import tui_state
caches, trackers, groks, out = {}, {}, {}, []
for c in json.load(open(cases)):
    op = c["op"]
    if op == "write":
        path = os.path.join(work, c["name"])
        os.makedirs(os.path.dirname(path), exist_ok=True)
        data = bytes.fromhex(c["hex"]) if "hex" in c else c["text"].encode()
        with open(path, "ab" if c.get("append") else "wb") as fh:
            fh.write(data)
        os.utime(path, ns=(c["mtime"], c["mtime"]))
        continue
    if op == "symlink":
        path = os.path.join(work, c["name"])
        os.makedirs(os.path.dirname(path), exist_ok=True)
        os.symlink(c["target"], path)
        continue
    if op == "model_id":
        out.append(tui_state.model_id(c["label"]))
    elif op == "screen":
        out.append(tui_state.screen_state(c["harness"], c["text"]))
    elif op == "transcript":
        cache = caches.setdefault(c["cache"], tui_state.TranscriptCache(max_bytes=c.get("maxBytes", 2097152)))
        path = os.path.join(work, c["name"])
        r = cache.read(c["harness"], c["sid"], path)
        try:
            st = os.stat(path)
            sig = str((st.st_dev, st.st_ino, st.st_size, st.st_mtime_ns))
            r = {k: v.replace(sig, "<sig>") if isinstance(v, str) else v for k, v in r.items()}
        except OSError:
            pass
        out.append(r)
    elif op == "observe":
        tracker = trackers.setdefault(c["tracker"], tui_state.StateTracker())
        out.append(tracker.observe(c["identity"], c["launch"], c["conversation"], c["visible"], now=c["now"]))
    elif op == "grok":
        cache = groks.setdefault(c["cache"], tui_state.GrokMetadataCache())
        out.append(cache.read(c["pid"], os.path.join(work, c["home"])))
print(json.dumps(out).replace(work, "<work>"))
"#;

fn obs(v: &Value) -> serde_json::Map<String, Value> {
    v.as_object().cloned().unwrap_or_default()
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn run_rust(cases: &[Value], work: &Path) -> String {
    let mut caches: HashMap<String, TranscriptCache> = HashMap::new();
    let mut trackers: HashMap<String, StateTracker> = HashMap::new();
    let mut groks: HashMap<String, GrokMetadataCache> = HashMap::new();
    let mut out = Vec::new();
    for c in cases {
        let s = |k: &str| c[k].as_str().unwrap().to_owned();
        match c["op"].as_str().unwrap() {
            "write" => {
                let path = work.join(s("name"));
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                let mut bytes = if c["append"].as_bool() == Some(true) {
                    fs::read(&path).unwrap_or_default()
                } else {
                    Vec::new()
                };
                match c["hex"].as_str() {
                    Some(h) => bytes.extend_from_slice(&unhex(h)),
                    None => bytes.extend_from_slice(s("text").as_bytes()),
                }
                fs::write(&path, bytes).unwrap();
                let mtime = std::time::UNIX_EPOCH
                    + std::time::Duration::from_nanos(c["mtime"].as_u64().unwrap());
                fs::File::options()
                    .write(true)
                    .open(&path)
                    .unwrap()
                    .set_modified(mtime)
                    .unwrap();
            }
            "symlink" => {
                let path = work.join(s("name"));
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::os::unix::fs::symlink(s("target"), path).unwrap();
            }
            "model_id" => out.push(json!(model_id(&s("label")).unwrap())),
            "screen" => out.push(Value::Object(
                screen_state(&s("harness"), &s("text")).unwrap(),
            )),
            "transcript" => {
                let cache = caches.entry(s("cache")).or_insert_with(|| {
                    TranscriptCache::new(128, c["maxBytes"].as_u64().unwrap_or(2_097_152))
                });
                let path = work.join(s("name"));
                let mut got = cache.read(&s("harness"), &s("sid"), &path).unwrap();
                if let Ok(m) = fs::metadata(&path) {
                    use std::os::unix::fs::MetadataExt;
                    let ns = i128::from(m.mtime()) * 1_000_000_000 + i128::from(m.mtime_nsec());
                    let sig = format!("({}, {}, {}, {ns})", m.dev(), m.ino(), m.size());
                    for v in got.values_mut() {
                        if let Value::String(t) = v {
                            *t = t.replace(&sig, "<sig>");
                        }
                    }
                }
                out.push(Value::Object(got));
            }
            "observe" => {
                let tracker = trackers
                    .entry(s("tracker"))
                    .or_insert_with(|| StateTracker::new(128));
                out.push(Value::Object(tracker.observe(
                    &s("identity"),
                    &obs(&c["launch"]),
                    &obs(&c["conversation"]),
                    &obs(&c["visible"]),
                    c["now"].as_f64().unwrap(),
                )));
            }
            "grok" => {
                let cache = groks
                    .entry(s("cache"))
                    .or_insert_with(|| GrokMetadataCache::new(128));
                out.push(Value::Object(
                    cache
                        .read(c["pid"].as_i64().unwrap(), &work.join(s("home")))
                        .unwrap(),
                ));
            }
            other => panic!("op {other}"),
        }
    }
    response_dumps(&Value::Array(out))
        .unwrap()
        .replace(&work.display().to_string(), "<work>")
}

fn utf16le_line(text: &str) -> Vec<u8> {
    let mut out = vec![0xFF, 0xFE];
    for unit in text.encode_utf16() {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    out
}

fn cases() -> Vec<Value> {
    let text_rows = [
        r#"{"type":"assistant","uuid":"u1","sessionId":"s1","message":{"model":"claude-opus-5"}}"#,
        r#"{"type":"assistant","uuid":"u2","sessionId":"s1","effort":"high","message":{"model":"claude-opus-5"}}"#,
        r#"{"type":"assistant","uuid":"u3","sessionId":"otra","message":{"model":"gpt-5"}}"#,
        r#"{"type":"assistant","uuid":"u4","isSidechain":true,"message":{"model":"haiku"}}"#,
        r#"{"type":"user","uuid":"u5","sessionId":"s1","message":{"content":"<local-command-stdout>Set model to Fable 5 (claude-fable-5[1m]) with max effort</local-command-stdout>"}}"#,
        r#"{"type":"assistant","uuid":"u6","sessionId":"s1","message":{"model":"<synthetic>"}}"#,
        r#"{"type":"assistant","uuid":"u7","sessionId":"s1","message":{"model":"bad model!"}}"#,
        "no es json",
        "\u{feff}{\"type\":\"assistant\",\"uuid\":\"u8\",\"sessionId\":\"s1\",\"message\":{\"model\":\"claude-sonnet-5\"},\"effort\":\"low\"}",
        // Resultado de herramienta con un sustituto suelto: campo que no se lee.
        r#"{"type":"user","uuid":"u9","sessionId":"s1","message":{"content":[{"type":"tool_result","content":"corte \udc00"}]}}"#,
        r#"{"type":"assistant","uuid":"u10","sessionId":"s1","message":{"model":"claude-opus-5","content":"x \ud800"},"perTurnEffort":"xhigh"}"#,
        r#"{"type":"assistant","uuid":"u11","sessionId":"s1","effort":"HIGH","message":{"model":"claude-haiku-5"}}"#,
        r#"{"type":"assistant","uuid":7,"sessionId":"s1","message":{"model":5}}"#,
        r#"{"type":"user","uuid":"u12","sessionId":"s1","message":{"content":"<local-command-stdout>Set effort level to medium\nmás texto</local-command-stdout>"}}"#,
        r#"{"type":"user","uuid":"u13","session_id":"s1","message":{"content":"<local-command-stdout>Effort set to low but not applied</local-command-stdout>"}}"#,
        // Línea cortada con mucho anidamiento poco profundo: se salta.
        &format!("{{\"type\":\"assistant\",\"x\":{}", "[1,".repeat(300)),
    ];
    let mut rows: Vec<Vec<u8>> = text_rows.iter().map(|r| r.as_bytes().to_vec()).collect();
    // Entero de más de 4300 dígitos: `ValueError` en CPython; el flotante no.
    for number in ["9".repeat(5000), format!("{}.0", "9".repeat(5000))] {
        rows.push(
            format!(
                r#"{{"type":"assistant","uuid":"u18","sessionId":"s1","n":{number},"message":{{"model":"claude-opus-5"}}}}"#
            )
            .into_bytes(),
        );
    }
    rows.push(utf16le_line(
        r#"{"type":"assistant","uuid":"u16","sessionId":"s1","message":{"model":"claude-haiku-5"}}"#,
    ));
    rows.push(b"\xff\xfe\xfd basura".to_vec());
    rows.push(b"\0\0\0\0".to_vec());
    rows.push(b"\0{}".to_vec());
    rows.push(
        b"{\"type\":\"assistant\",\"uuid\":\"u17\",\"sessionId\":\"s1\",\"message\":{\"model\":\"claude-opus-5\",\"content\":\"\xed\xa0\x80\"}}"
            .to_vec(),
    );
    // Sin `uuid` ni `timestamp`: la revisión es la firma y el índice.
    rows.push(
        br#"{"type":"assistant","sessionId":"s1","message":{"model":"claude-opus-5"}}"#.to_vec(),
    );
    let mut raw = rows.join(&b'\n');
    raw.push(b'\n');
    let codex = [
        r#"{"type":"session_meta","payload":{"id":"c1"}}"#,
        r#"{"type":"turn_context","uuid":"t1","payload":{"model":"gpt-5.6-sol","effort":"xhigh"}}"#,
        r#"{"type":"turn_context","timestamp":"2026-10-04T12:00:00Z","payload":{"model":"gpt-5.6-luna","reasoning_effort":"low"}}"#,
    ]
    .join("\r\n");
    let cases = json!([
        {"op":"model_id","label":"Opus 4.6 (1M context)"},
        {"op":"model_id","label":"sonnet (claude-sonnet-5-20260901)"},
        {"op":"model_id","label":"Grok 4.5 Heavy"},
        {"op":"model_id","label":"nada"},
        {"op":"model_id","label":"Claude Opus 4.6 [1M]"},
        {"op":"model_id","label":"CLAUDE-OPUS-4-6[1M] y opus"},
        {"op":"model_id","label":"opus haiku"},
        {"op":"model_id","label":"⚠\u{fe0f} Sonnet 5 · é"},
        {"op":"screen","harness":"codex","text":"x\n  gpt-5.6-sol xhigh · 40% left\n"},
        {"op":"screen","harness":"codex","text":"1. gpt-5 high · opción\n\n\n\n\n\n\n\n\n"},
        {"op":"screen","harness":"codex","text":"gpt-5 high\u{1f}· x"},
        {"op":"screen","harness":"codex","text":"GPT-5 ULTRACODE • y\n"},
        {"op":"screen","harness":"opencode","text":"┃  build · Claude Sonnet 5 · high\n╹▀▀▀\n"},
        {"op":"screen","harness":"opencode","text":"┃  plan · gpt-5 (openai)\n  ╹▀▀\n┃  x · nada\n╹▀\n"},
        {"op":"screen","harness":"claude","text":"❯ /model\n  ⎿  Set model to Opus 5 and high effort\n● hecho\n"},
        {"op":"screen","harness":"claude","text":"  ⎿  Set effort level to max\n\x1b[2m·\x1b[0m claude · claude-fable-5[1m]\n"},
        {"op":"screen","harness":"claude","text":"── · claude · claude-opus-5 · main\n"},
        {"op":"screen","harness":"claude","text":"● respuesta ✅\n❯ /effort\n  ⎿  Effort level set to low\n"},
        {"op":"screen","harness":"claude","text":"  ⎿  Kept model as Sonnet 4.5\n  ⎿  Set effort level to high but only for this turn\n"},
        {"op":"screen","harness":"gemini","text":"lo que sea"},
        {"op":"write","name":"claude/s1.jsonl","hex":hex(&raw),"mtime":1_791_115_200_000_000_000u64},
        {"op":"transcript","cache":"a","harness":"claude","sid":"s1","name":"claude/s1.jsonl"},
        {"op":"transcript","cache":"a","harness":"claude","sid":"s1","name":"claude/s1.jsonl"},
        {"op":"write","name":"codex/c1.jsonl","text":codex,"mtime":1_791_115_201_000_000_000u64},
        {"op":"transcript","cache":"b","harness":"codex","sid":"c1","name":"codex/c1.jsonl"},
        {"op":"transcript","cache":"c","harness":"codex","sid":"c1","name":"codex/c1.jsonl","maxBytes":120},
        {"op":"write","name":"codex/c1.jsonl","text":"\n{\"type\":\"event\"}","append":true,"mtime":1_791_115_202_000_000_000u64},
        {"op":"transcript","cache":"c","harness":"codex","sid":"c1","name":"codex/c1.jsonl","maxBytes":30},
        {"op":"transcript","cache":"d","harness":"claude","sid":"s1","name":"falta.jsonl"},
        {"op":"transcript","cache":"d","harness":"claude","sid":"s1","name":"claude"},
        {"op":"observe","tracker":"t","identity":"p1","launch":{"model":"","effort":""},"conversation":{},"visible":{},"now":1.0},
        {"op":"observe","tracker":"t","identity":"p1","launch":{},"conversation":{"model":"claude-opus-5","revision":"u1"},"visible":{"model":"claude-opus-5","kind":"custom-status"},"now":2.0},
        {"op":"observe","tracker":"t","identity":"p1","launch":{},"conversation":{"model":"claude-opus-5","revision":"u1"},"visible":{"model":"claude-fable-5","effort":"max","kind":"confirmation"},"now":3.0},
        {"op":"observe","tracker":"t","identity":"p1","launch":{},"conversation":{"model":"claude-opus-5","revision":"u1"},"visible":{},"now":4.0},
        {"op":"observe","tracker":"t","identity":"p1","launch":{},"conversation":{"model":"claude-opus-5","revision":"u1"},"visible":{"model":"claude-fable-5","effort":"max","kind":"confirmation"},"now":4.5},
        {"op":"observe","tracker":"t","identity":"p1","launch":{},"conversation":{"model":"claude-sonnet-5","revision":"u9"},"visible":{},"now":5.0},
        {"op":"observe","tracker":"t","identity":"p2","launch":{"model":"gpt-5","effort":"low"},"conversation":{},"visible":{"model":"gpt-5","effort":"high","kind":"status"},"now":6.0},
        {"op":"observe","tracker":"t","identity":"p3","launch":{},"conversation":{},"visible":{"model":"claude-opus-5","kind":"custom-status"},"now":1_791_115_200.75},
        {"op":"observe","tracker":"t","identity":"p3","launch":{},"conversation":{},"visible":{"model":"claude-opus-5","kind":"custom-status","revision":null},"now":1_791_115_201.5},
        {"op":"write","name":"grok/active_sessions.json","text":"[{\"pid\": 77, \"session_id\": \"g1\"}, {\"pid\": 78, \"session_id\": \"../x\"}, {\"pid\": \" 80 \", \"session_id\": \"g2\"}, {\"pid\": 81.9, \"session_id\": \"g3\"}, {\"pid\": 82, \"session_id\": \"g4\"}, {\"pid\": 83, \"session_id\": \"g5\"}, 5, {\"pid\": 77, \"session_id\": \"g1\"}]","mtime":1_791_115_203_000_000_000u64},
        {"op":"write","name":"grok/sessions/a/g1/summary.json","text":"{\"info\":{\"cwd\":\"/w\"},\"current_model_id\":\"grok-4.5\",\"reasoning_effort\":\"low\",\"generated_title\":\"T\",\"last_active_at\":\"2026\"}","mtime":1_791_115_203_000_000_000u64},
        {"op":"write","name":"grok/sessions/g2/summary.json","text":"{\"info\":[1],\"current_model_id\":\"grok-4.5\"}","mtime":1_791_115_203_000_000_000u64},
        {"op":"write","name":"grok/sessions/b/c/g3/summary.json","text":"{\"info\":{\"cwd\":5},\"current_model_id\":1.5,\"reasoning_effort\":[\"a\",1],\"session_summary\":\"S\",\"last_active_at\":null}","mtime":1_791_115_203_000_000_000u64},
        {"op":"write","name":"grok/sessions/x/g4/summary.json","text":"{}","mtime":1_791_115_203_000_000_000u64},
        {"op":"symlink","name":"grok/sessions/y","target":"x"},
        {"op":"write","name":"grok/sessions/.oculto/g5/summary.json","text":"{}","mtime":1_791_115_203_000_000_000u64},
        {"op":"grok","cache":"g","pid":77,"home":"grok"},
        {"op":"grok","cache":"g","pid":77,"home":"grok"},
        {"op":"grok","cache":"g","pid":78,"home":"grok"},
        {"op":"grok","cache":"g","pid":79,"home":"grok"},
        {"op":"grok","cache":"g","pid":80,"home":"grok"},
        {"op":"grok","cache":"g","pid":81,"home":"grok"},
        {"op":"grok","cache":"g","pid":82,"home":"grok"},
        {"op":"grok","cache":"g","pid":83,"home":"grok"},
        {"op":"grok","cache":"g","pid":77,"home":"falta"}
    ])
    .as_array()
    .unwrap()
    .clone();
    // Cada fila sola en su archivo: su efecto no queda tapado por las siguientes.
    let mut cases = cases;
    for (i, row) in rows.iter().enumerate() {
        cases.push(json!({"op":"write","name":format!("lines/{i}.jsonl"),"hex":hex(row),"mtime":1_791_115_210_000_000_000u64}));
        cases.push(json!({"op":"transcript","cache":format!("l{i}"),"harness":"claude","sid":"s1","name":format!("lines/{i}.jsonl")}));
    }
    cases
}

fn scratch(tag: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("cmd-tui-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn tui_state_matches_python_oracle() {
    let root = scratch("oracle");
    let (py_work, rs_work) = (root.join("py"), root.join("rs"));
    fs::create_dir_all(&py_work).unwrap();
    fs::create_dir_all(&rs_work).unwrap();
    let cases = cases();
    let file = root.join("cases.json");
    fs::write(&file, serde_json::to_string(&cases).unwrap()).unwrap();
    let Some(expected) = run_python(ORACLE, &[file.as_os_str(), py_work.as_os_str()], &root) else {
        return;
    };
    let got = run_rust(&cases, &rs_work);
    assert_eq!(got, expected.trim_end());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn transcript_surrogate_in_used_field_is_unsure() {
    let dir = scratch("sur");
    let path = dir.join("t.jsonl");
    fs::write(
        &path,
        "{\"type\":\"assistant\",\"uuid\":\"u\\ud800\",\"message\":{\"model\":\"claude-opus-5\"}}\n",
    )
    .unwrap();
    assert!(
        TranscriptCache::new(128, 2_097_152)
            .read("claude", "s", &path)
            .is_err()
    );
    fs::write(
        &path,
        b"{\"type\":\"assistant\",\"uuid\":\"u\xed\xb0\x80\",\"message\":{\"model\":\"claude-opus-5\"}}\n",
    )
    .unwrap();
    assert!(
        TranscriptCache::new(128, 2_097_152)
            .read("claude", "s", &path)
            .is_err()
    );
    fs::write(&path, "{\"type\":\"assistant\",\"uuid\":\"u1\",\"message\":{\"model\":\"claude-opus-5\",\"content\":\"\\udc00\"}}\n").unwrap();
    let got = TranscriptCache::new(128, 2_097_152)
        .read("claude", "s", &path)
        .unwrap();
    assert_eq!(got["model"], json!("claude-opus-5"));
    // Un resultado de herramienta truncado (lista de contenido) nunca declina.
    fs::write(&path, "{\"type\":\"user\",\"uuid\":\"u2\",\"message\":{\"content\":[{\"type\":\"tool_result\",\"content\":\"\\udc00\"}]}}\n").unwrap();
    assert!(
        TranscriptCache::new(128, 2_097_152)
            .read("claude", "s", &path)
            .unwrap()
            .is_empty()
    );
    // Anidamiento que en el Python puede acabar en `RecursionError`.
    let deep = format!("{}{}\n", "[".repeat(950), "]".repeat(950));
    fs::write(&path, deep).unwrap();
    assert!(
        TranscriptCache::new(128, 2_097_152)
            .read("claude", "s", &path)
            .is_err()
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn screen_with_folding_letter_is_unsure() {
    assert!(screen_state("codex", "gpt-5 \u{212a}igh · x").is_err());
    assert!(screen_state("claude", "  ⎿  Set effort level to h\u{130}gh").is_err());
    // U+001F es `\s` en `re`: se resuelve con certeza (el oráculo lo compara).
    assert!(screen_state("codex", "gpt-5 high\u{1f}· x").is_ok());
}

#[test]
fn grok_unsure_paths() {
    let dir = scratch("grok");
    fs::write(
        dir.join("active_sessions.json"),
        r#"[{"pid": 1, "session_id": 123}, {"pid": 2, "session_id": "g"}, {"pid": 3, "session_id": "h"}]"#,
    )
    .unwrap();
    let mut cache = GrokMetadataCache::new(128);
    // `str(sid)` válido pero no cadena: `os.path.join` daría `TypeError`.
    assert!(cache.read(1, &dir).is_err());
    // Ciclo de enlaces bajo `sessions/`.
    fs::create_dir_all(dir.join("sessions/a")).unwrap();
    std::os::unix::fs::symlink("..", dir.join("sessions/a/up")).unwrap();
    assert!(cache.read(2, &dir).is_err());
    // Resumen con un sustituto suelto que llega a la salida.
    fs::remove_file(dir.join("sessions/a/up")).unwrap();
    fs::create_dir_all(dir.join("sessions/a/h")).unwrap();
    fs::write(
        dir.join("sessions/a/h/summary.json"),
        r#"{"generated_title": "t\ud800"}"#,
    )
    .unwrap();
    assert!(cache.read(3, &dir).is_err());
    let _ = fs::remove_dir_all(&dir);
}

/// Las clases de `re` de CPython (`str.isspace`, `isalnum`, `isdecimal`, y lo
/// que `re.I` pliega a ASCII) contra las de `regex` y las guardas del port.
const CLASSES: &str = r#"
import json, re, sys
fold = re.compile('[a-z]', re.I)
sets = {"space": [], "word": [], "digit": [], "fold": []}
def add(name, cp):
    ranges = sets[name]
    if ranges and ranges[-1][1] == cp - 1:
        ranges[-1][1] = cp
    else:
        ranges.append([cp, cp])
for cp in range(0x110000):
    if 0xD800 <= cp <= 0xDFFF:
        continue
    c = chr(cp)
    if c.isspace(): add("space", cp)
    if c.isalnum() or c == "_": add("word", cp)
    if c.isdecimal(): add("digit", cp)
    if cp > 127 and fold.fullmatch(c): add("fold", cp)
print(json.dumps(sets))
"#;

#[test]
fn python_character_classes_match_rust_guards() {
    let root = scratch("classes");
    let Some(out) = run_python(CLASSES, &[], &root) else {
        return;
    };
    let sets: Value = serde_json::from_str(&out).unwrap();
    let bits: HashMap<&str, Vec<bool>> = ["space", "word", "digit", "fold"]
        .into_iter()
        .map(|name| {
            let mut bits = vec![false; 0x11_0000];
            for r in sets[name].as_array().unwrap() {
                for cp in r[0].as_u64().unwrap()..=r[1].as_u64().unwrap() {
                    bits[cp as usize] = true;
                }
            }
            (name, bits)
        })
        .collect();
    let member = |name: &str, c: char| bits[name][u32::from(c) as usize];
    let re = |p: &str| regex::Regex::new(p).unwrap();
    let (space, word, digit, fold) = (
        re(r"^[\s\x1c-\x1f]$"),
        re(r"^\w$"),
        re(r"^\d$"),
        re(r"(?i)^[a-z]$"),
    );
    let word_div = re(r"^[[\w--[\p{L}\p{N}_]][[\p{L}\p{N}]--\w][[\w\p{L}\p{N}]&&\P{Age=13.0}]]$");
    let digit_div = re(r"^[\d&&\P{Age=13.0}]$");
    let folds = ['\u{130}', '\u{131}', '\u{17f}', '\u{212a}'];
    for c in (0..0x11_0000u32).filter_map(char::from_u32) {
        let mut buf = [0u8; 4];
        let s = c.encode_utf8(&mut buf);
        assert_eq!(space.is_match(s), member("space", c), "\\s en {:?}", c);
        if word.is_match(s) != member("word", c) {
            assert!(word_div.is_match(s), "\\w en {:?} sin guarda", c);
        }
        if digit.is_match(s) != member("digit", c) {
            assert!(digit_div.is_match(s), "\\d en {:?} sin guarda", c);
        }
        if !c.is_ascii() && (fold.is_match(s) || member("fold", c)) {
            assert!(folds.contains(&c), "re.I pliega {:?} a ASCII", c);
        }
    }
    let _ = fs::remove_dir_all(&root);
}

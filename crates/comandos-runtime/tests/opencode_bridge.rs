use comandos_runtime::hooks::opencode::{Bridge, run_bridge_with};
use serde_json::{Value, json};
use std::{
    fs,
    io::Cursor,
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};
static SEQ: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "comandos-opencode-bridge-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        for s in ["home", "data", "config", "cache", "state", "run", "tmp"] {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(p.join(s))
                .unwrap()
        }
        Self(p)
    }
    fn home(&self) -> PathBuf {
        self.0.join("home")
    }
    fn node(&self) -> Command {
        let node = std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|p| p.join("node"))
            .find(|p| p.is_file())
            .expect("Node oracle required");
        let mut c = Command::new(node);
        c.env_clear()
            .current_dir(&self.0)
            .env("HOME", self.home())
            .env("LANG", "C.UTF-8");
        for (k, d) in [
            ("XDG_DATA_HOME", "data"),
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_CACHE_HOME", "cache"),
            ("XDG_STATE_HOME", "state"),
            ("XDG_RUNTIME_DIR", "run"),
            ("TMPDIR", "tmp"),
        ] {
            c.env(k, self.0.join(d));
        }
        c
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap()
    }
}
fn events() -> Vec<Value> {
    vec![
        json!({"type":"message.part.updated","properties":{"sessionID":"root"}}),
        json!({"type":"message.updated","properties":{"info":{"sessionID":"root","role":"user","providerID":"p","modelID":"m"}}}),
        json!({"type":"permission.asked","properties":{"sessionID":"root"}}),
        json!({"type":"message.updated","properties":{"info":{"sessionID":"root","role":"assistant","providerID":"q","modelID":"n"}}}),
        json!({"type":"session.idle","properties":{"sessionID":"child"}}),
        json!({"type":"session.error","properties":{"sessionID":"missing"}}),
        json!({"type":"permission.updated","properties":{"sessionID":"bad/../id"}}),
        json!({"type":"session.idle","properties":{"sessionID":"root"}}),
        json!({"type":"message.updated","properties":{"info":{"sessionID":"other","role":"assistant","providerID":"x","modelID":"y"}}}),
        json!({"type":"session.idle","properties":{"sessionID":"other"}}),
    ]
}
fn response(id: &Value) -> Value {
    match id.as_str() {
        Some("child") => json!({"data":{"id":"child","parentID":"root"}}),
        Some("missing") => Value::Null,
        _ => json!({"data":{"id":id}}),
    }
}
fn original_trace(f: &Fixture, events: &[Value], silent: bool) -> Value {
    let original = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/hooks/oracle/opencode-comandos.js");
    let source = fs::read_to_string(original)
        .unwrap()
        .replace("process.pid", "42")
        .replace("`/proc/${42}/stat`", "new URL('./stat', import.meta.url)")
        .replace("Date.now()", "6000");
    fs::write(f.0.join("original.mjs"), source).unwrap();
    fs::write(
        f.0.join("stat"),
        format!(
            "42 (fake) {}",
            (0..20)
                .map(|i| if i == 19 { "123" } else { "0" })
                .collect::<Vec<_>>()
                .join(" ")
        ),
    )
    .unwrap();
    fs::write(
        f.0.join("events.json"),
        serde_json::to_string(events).unwrap(),
    )
    .unwrap();
    fs::write(f.0.join("run.mjs"),r#"import { Comandos } from './original.mjs';import { readFile,unlink } from 'node:fs/promises';const calls=[],posts=[],states=[];globalThis.fetch=async(_,o)=>{posts.push(JSON.parse(o.body));return {}};const plugin=await Comandos({directory:'/private/work',client:{session:{get:async(r)=>{calls.push(r);const id=r.path.id;return id==='missing'?null:{data:{id,...(id==='child'?{parentID:'root'}:{})}}}}}});for(const event of JSON.parse(await readFile('./events.json'))){if(event.type==='permission.asked')await unlink(process.env.HOME+'/.claude/hooks/native-processes/42.json').catch(()=>{});await plugin.event?.({event});states.push(await readFile(process.env.HOME+'/.claude/hooks/native-processes/42.json','utf8').then(JSON.parse,()=>null));}console.log(JSON.stringify({calls,posts,states}));"#).unwrap();
    let mut c = f.node();
    c.arg(f.0.join("run.mjs"));
    if silent {
        c.env("COMANDOS_SILENT_AGENT", "1");
    }
    let out = c.output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stderr, b"");
    serde_json::from_slice(&out.stdout).unwrap()
}
#[test]
fn native_bridge_matches_actual_sdk_event_trace_and_retains_current_in_memory() {
    let old = Fixture::new();
    let new = Fixture::new();
    let events = events();
    let expected = original_trace(&old, &events, false);
    let mut engine = Bridge::new(new.home(), 42, "123".into());
    let mut calls = Vec::new();
    let mut posts = Vec::new();
    let mut states = Vec::new();
    for event in events {
        if event["type"] == "permission.asked" {
            fs::remove_file(new.home().join(".claude/hooks/native-processes/42.json")).unwrap();
        }
        let request = engine.request(&event);
        let response = if let Some(request) = request {
            calls.push(request.clone());
            response(&request["path"]["id"])
        } else {
            Value::Null
        };
        engine.handle(
            Some(json!("/private/work")),
            event,
            response,
            6000,
            &mut |event, cwd| {
                posts.push(json!({"agent":"opencode","cwd":cwd,"event":event}));
            },
        );
        states.push(
            fs::read(new.home().join(".claude/hooks/native-processes/42.json"))
                .ok()
                .map(|b| serde_json::from_slice::<Value>(&b).unwrap())
                .unwrap_or(Value::Null),
        );
    }
    assert_eq!(
        json!({"calls":calls,"posts":posts,"states":states}),
        expected
    );
}
#[test]
fn jsonl_actor_only_requests_sdk_for_eligible_events_and_silent_actor_has_no_effects() {
    let f = Fixture::new();
    let event = json!({"type":"session.idle","properties":{"sessionID":"root"}});
    let input = format!(
        "{}\n{}\n{}\n",
        json!({"directory":"/private/work","event":{"type":"log"}}),
        json!({"directory":"/private/work","event":event}),
        response(&json!("root"))
    );
    let mut output = Vec::new();
    let mut posts = Vec::new();
    run_bridge_with(
        Cursor::new(input),
        &mut output,
        Bridge::new(f.home(), 42, "123".into()),
        || 6000,
        &mut |event, cwd| posts.push((event.to_string(), cwd.to_string())),
        false,
    )
    .unwrap();
    assert_eq!(
        String::from_utf8(output).unwrap(),
        "null\nnull\n{\"path\":{\"id\":\"root\"}}\nnull\n"
    );
    assert_eq!(posts, vec![("done".into(), "/private/work".into())]);
    let f = Fixture::new();
    let input = format!("{}\n", json!({"directory":"/private/work","event":event}));
    let mut output = Vec::new();
    run_bridge_with(
        Cursor::new(input),
        &mut output,
        Bridge::new(f.home(), 42, "123".into()),
        || 6000,
        &mut |_, _| panic!("silent notification"),
        true,
    )
    .unwrap();
    assert_eq!(output, b"null\nnull\n");
    assert_eq!(fs::read_dir(f.home()).unwrap().count(), 0);
    assert_eq!(
        original_trace(&f, &[event], true),
        json!({"calls":[],"posts":[],"states":[null]})
    );
}
#[test]
fn sdk_requests_match_the_actual_current_adapter_source_without_exec_or_network() {
    let f = Fixture::new();
    let original =
        Path::new("/home/someguy/codebase/0xJesus/ComandOS/adapters/opencode-comandos.js");
    let source = fs::read_to_string(original).unwrap().replace(
        "import { spawn } from 'node:child_process';",
        "import { spawn } from './fake-spawn.mjs';",
    );
    fs::write(f.0.join("current.mjs"), source).unwrap();
    fs::write(f.0.join("fake-spawn.mjs"),r#"import { EventEmitter } from 'node:events';export function spawn(){const child=new EventEmitter();child.stdout=new EventEmitter();child.stdin=new EventEmitter();child.stdin.end=()=>queueMicrotask(()=>{child.stdout.emit('data',Buffer.from('{}'));child.emit('close',0)});return child;}"#).unwrap();
    let events = events();
    fs::write(
        f.0.join("events.json"),
        serde_json::to_string(&events).unwrap(),
    )
    .unwrap();
    fs::write(f.0.join("run.mjs"),r#"import {Comandos} from './current.mjs';import {readFile} from 'node:fs/promises';const calls=[];const plugin=await Comandos({directory:'/private/work',client:{session:{get:async(r)=>{calls.push(r);return {data:{id:r.path.id}}}}}});for(const event of JSON.parse(await readFile('./events.json')))await plugin.event({event});console.log(JSON.stringify(calls));"#).unwrap();
    let out = f.node().arg(f.0.join("run.mjs")).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stderr, b"");
    let expected: Value = serde_json::from_slice(&out.stdout).unwrap();
    let bridge = Bridge::new(f.home(), 42, "123".into());
    let actual: Vec<Value> = events
        .iter()
        .filter_map(|event| bridge.request(event))
        .collect();
    assert_eq!(json!(actual), expected);
}
#[test]
fn actor_rejects_incomplete_invalid_and_oversized_frames_without_persisting() {
    for input in [
        b"{broken}\n".to_vec(),
        b"{}".to_vec(),
        vec![b'x'; 16 * 1024 * 1024 + 1],
        format!(
            "{}\n",
            json!({"event":{"type":"session.idle","properties":{"sessionID":"root"}}})
        )
        .into_bytes(),
    ] {
        let f = Fixture::new();
        let mut out = Vec::new();
        assert!(
            run_bridge_with(
                Cursor::new(input),
                &mut out,
                Bridge::new(f.home(), 42, "123".into()),
                || 6000,
                &mut |_, _| panic!("invalid protocol event"),
                false
            )
            .is_err()
        );
        assert_eq!(fs::read_dir(f.home()).unwrap().count(), 0);
    }
    let f = Fixture::new();
    let mut out = Vec::new();
    run_bridge_with(
        Cursor::new(b""),
        &mut out,
        Bridge::new(f.home(), 42, "123".into()),
        || 6000,
        &mut |_, _| panic!("EOF event"),
        false,
    )
    .unwrap();
    assert!(out.is_empty());
}
#[test]
fn user_message_timestamp_is_read_after_notification_and_sdk_reply() {
    use std::cell::Cell;
    let f = Fixture::new();
    let mut bridge = Bridge::new(f.home(), 42, "123".into());
    let now = Cell::new(6000);
    bridge.handle_with_clock(
        Some(json!("/private/work")),
        json!({"type":"message.updated","properties":{"info":{"sessionID":"root","role":"user"}}}),
        json!({"data":{"id":"root"}}),
        &mut || now.get(),
        &mut |_, _| now.set(6123),
    );
    let record: Value = serde_json::from_slice(
        &fs::read(f.home().join(".claude/hooks/native-processes/42.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(record["updatedAt"], 6123);
}

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::disallowed_methods
)]
use comandos_app::{
    config::RunMode,
    restore::{RestorePlan, RestoreTmux, ScopeLauncher, execute, remap_layout},
};
use comandos_core::workspace::snapshot::layout_checksum;
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

type RecordedCall = (Vec<String>, Option<Vec<u8>>);
#[derive(Default)]
struct Recorder {
    calls: RefCell<Vec<RecordedCall>>,
    exists: Cell<bool>,
    next: Cell<u32>,
    fail: RefCell<Option<String>>,
    cleaned: Cell<bool>,
    shadow: bool,
}
impl Recorder {
    fn record(&self, args: &[&str], stdin: Option<&[u8]>) -> Result<String, String> {
        self.calls.borrow_mut().push((
            args.iter().map(|a| (*a).into()).collect(),
            stdin.map(|b| b.to_vec()),
        ));
        if self.fail.borrow().as_deref() == args.first().copied() {
            return Err("fixture failure".into());
        }
        Ok(match args[0] {
            "has-session" if !self.exists.get() => return Err("absent".into()),
            "new-session" | "new-window" | "split-window" => {
                let next = self.next.get() + 10;
                self.next.set(next);
                format!("%{next}")
            }
            "display-message" if args.last() == Some(&"#{window_id}") => "@7".into(),
            "display-message" if args.last() == Some(&"#{window_index}") => "0".into(),
            _ => String::new(),
        })
    }
}
impl RestoreTmux for Recorder {
    type Ownership = &'static str;
    fn mode(&self) -> RunMode {
        if self.shadow {
            RunMode::Shadow
        } else {
            RunMode::Sandbox
        }
    }
    fn read(&self, a: &[&str]) -> Result<String, String> {
        self.record(a, None)
    }
    fn mutate(&self, a: &[&str], b: Option<&[u8]>) -> Result<String, String> {
        assert!(!self.shadow);
        self.record(a, b)
    }
    fn create(&self, a: &[&str]) -> Result<(String, Self::Ownership), String> {
        assert!(!self.shadow);
        self.record(a, None).map(|s| (s, "owned-identity"))
    }
    fn cleanup(&self, t: Self::Ownership) -> Result<(), String> {
        assert_eq!(t, "owned-identity");
        self.cleaned.set(true);
        Ok(())
    }
}
#[derive(Default)]
struct Scopes(RefCell<Vec<(String, String)>>);
impl ScopeLauncher for Scopes {
    fn argv(&self, p: &str, c: &str) -> Result<Vec<String>, String> {
        self.0.borrow_mut().push((p.into(), c.into()));
        Ok(vec!["fake-scope".into(), p.into(), c.into()])
    }
}
fn snapshot() -> Value {
    let body = "120x32,0,0{59x32,0,0,1,60x32,60,0,2}";
    json!({"windows":[{"index":3,"name":"split ' 日本", "width":120,"height":32,"layout":format!("{:04x},{body}",layout_checksum(body)),"active":true,"zoomed":true,"panes":[
        {"id":"%1","cwd":"/tmp","active":true,"agent":"claude","resume_id":"12345678-1234-1234-1234-123456789abc","key":"pane-one"},
        {"id":"%2","cwd":"/tmp","active":false,"agent":"codex","resume_id":"87654321-4321-4321-4321-cba987654321","flags":["-c","name='日本'"]}
    ]}]})
}
fn plan(s: Value, present: BTreeSet<String>, mode: RunMode) -> RestorePlan {
    RestorePlan::build(&json!({"term-a":"A"}), &json!({"term-a":s}), &present, mode).unwrap()
}
#[test]
fn scope_launch_is_per_pane_and_geometry_precedes_exact_resume() {
    let tmux = Recorder::default();
    let scopes = Scopes::default();
    let out = execute(
        &plan(snapshot(), BTreeSet::new(), RunMode::Sandbox),
        &tmux,
        &scopes,
        Path::new("/tmp"),
    );
    assert!(out.tabs[0].attached, "{:?}", out);
    assert_eq!(
        out.tabs[0].pane_mapping,
        BTreeMap::from([("%1".into(), "%10".into()), ("%2".into(), "%20".into())])
    );
    assert_eq!(scopes.0.borrow().len(), 2);
    assert_eq!(scopes.0.borrow()[0].0, "%10");
    assert_eq!(scopes.0.borrow()[1].0, "%20");
    let calls = tmux.calls.borrow();
    let first_resume = calls
        .iter()
        .position(|(a, _)| a[0] == "load-buffer")
        .unwrap();
    let layout = calls
        .iter()
        .position(|(a, _)| a[0] == "select-layout" && a.last().unwrap().contains("x32"))
        .unwrap();
    assert!(layout < first_resume);
    assert!(
        calls
            .iter()
            .filter(|(a, _)| a[0] == "load-buffer")
            .all(|(_, b)| b.is_some())
    );
    assert_eq!(calls.iter().filter(|(a, _)| a[0] == "send-keys").count(), 2);
    assert!(calls.iter().all(|(a, _)| a.iter().all(|v| v != ";")));
}
#[test]
fn restore_existing_never_sends_resume_even_if_plan_is_stale() {
    let tmux = Recorder::default();
    tmux.exists.set(true);
    let scopes = Scopes::default();
    let out = execute(
        &plan(snapshot(), BTreeSet::new(), RunMode::Sandbox),
        &tmux,
        &scopes,
        Path::new("/tmp"),
    );
    assert!(out.tabs[0].attached);
    assert!(scopes.0.borrow().is_empty());
    assert_eq!(tmux.calls.borrow().len(), 1);
}
#[test]
fn failed_restore_cleans_only_owned_placeholder() {
    let tmux = Recorder::default();
    *tmux.fail.borrow_mut() = Some("split-window".into());
    let out = execute(
        &plan(snapshot(), BTreeSet::new(), RunMode::Sandbox),
        &tmux,
        &Scopes::default(),
        Path::new("/tmp"),
    );
    assert!(out.tabs[0].error.is_some());
    assert!(tmux.cleaned.get());
}
#[test]
fn invalid_checksum_prevents_all_mutation() {
    let tmux = Recorder::default();
    let mut s = snapshot();
    s["windows"][0]["layout"] = "0000,120x32,0,0,1".into();
    let out = execute(
        &plan(s, BTreeSet::new(), RunMode::Sandbox),
        &tmux,
        &Scopes::default(),
        Path::new("/tmp"),
    );
    assert!(out.tabs[0].error.is_some());
    assert_eq!(tmux.calls.borrow().len(), 1);
    assert!(!tmux.cleaned.get());
}
#[test]
fn shadow_never_mutates_or_automatically_attaches() {
    let tmux = Recorder {
        shadow: true,
        ..Recorder::default()
    };
    let scopes = Scopes::default();
    let out = execute(
        &plan(snapshot(), BTreeSet::new(), RunMode::Shadow),
        &tmux,
        &scopes,
        Path::new("/tmp"),
    );
    assert!(!out.tabs[0].attached);
    assert!(!out.tabs[0].ambiguity.is_empty());
    assert_eq!(tmux.calls.borrow().len(), 1);
}
#[test]
fn remap_matches_python_tmux_snapshot_oracle() {
    let s = snapshot();
    let layout = s["windows"][0]["layout"].as_str().unwrap();
    let map = BTreeMap::from([("%1".into(), "%110".into()), ("%2".into(), "%120".into())]);
    let result=std::process::Command::new("python3").args(["-c","import json,sys; sys.path.insert(0, sys.argv[3]); import tmux_snapshot; print(tmux_snapshot.remap_layout(sys.argv[1],json.loads(sys.argv[2])))",layout,&serde_json::to_string(&map).unwrap(),concat!(env!("CARGO_MANIFEST_DIR"),"/../../lib")]).env_clear().env("PATH","/usr/bin:/bin").env("HOME","/tmp/comandos-python-oracle-private-home").output().unwrap();
    assert!(result.status.success());
    assert_eq!(
        remap_layout(layout, &map).unwrap(),
        String::from_utf8(result.stdout).unwrap().trim()
    );
}

#[test]
fn web_tabs_never_become_tmux_placeholders() {
    let tmux = Recorder::default();
    let plan = RestorePlan::build(
        &json!({"xterm-local":"Web"}),
        &json!({}),
        &BTreeSet::new(),
        RunMode::Sandbox,
    )
    .unwrap();
    let result = execute(&plan, &tmux, &Scopes::default(), Path::new("/tmp"));
    assert_eq!(result.tabs[0].key, "xterm-local");
    assert!(!result.tabs[0].attached);
    assert!(tmux.calls.borrow().is_empty());
}

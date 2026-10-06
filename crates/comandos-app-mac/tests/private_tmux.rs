#![allow(clippy::unwrap_used)]
use comandos_app_mac::{
    app::{App, RunMode},
    jobs::{Backend, Jobs, ResultData, Task},
    tmux::{TmuxRunner, runner},
};
use comandos_desktop::{Lang, proc::ProcOutput};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::DirBuilderExt,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
struct Own {
    root: std::path::PathBuf,
    tmux: Box<dyn TmuxRunner>,
    history: Mutex<Vec<Value>>,
}
impl Own {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("m4-tmux-{}", std::process::id()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let mode = RunMode::Sandbox {
            tmux_socket: root.join("s"),
            hooks: root.join("hooks"),
        };
        Self {
            root,
            tmux: runner(&mode),
            history: Mutex::new(vec![]),
        }
    }
}
impl Drop for Own {
    fn drop(&mut self) {
        let _ = self.tmux.run(&["kill-server"]);
        let _ = fs::remove_dir_all(&self.root);
    }
}
impl Backend for Own {
    fn get(&self, _: &str) -> Result<Value, String> {
        Ok(Value::Null)
    }
    fn post(&self, _: &str, _: &Value) -> Result<Value, String> {
        Ok(Value::Null)
    }
    fn tmux(&self, args: &[&str], allowed: &dyn Fn() -> bool) -> Result<ProcOutput, String> {
        self.tmux.run_when(args, allowed)
    }
    fn cancel(&self) {}
    fn archive(&self, item: &Value, allowed: &dyn Fn() -> bool) -> Result<(), String> {
        assert!(allowed());
        self.history.lock().unwrap().push(item.clone());
        Ok(())
    }
}
fn job(jobs: &Jobs, app: &App, task: Task) -> ResultData {
    jobs.submit(app.ticket(), task).unwrap();
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(result) = jobs.drain().into_iter().next() {
            return result.result.unwrap();
        }
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(1));
    }
}
#[test]
fn new_local_and_idle_scratch_close_use_only_owned_private_server() {
    let own = Arc::new(Own::new());
    let jobs = Jobs::new(own.clone(), Arc::new(|| {})).unwrap();
    let mut app = App::new("noche", Lang::En);
    let ticket = app.ticket();
    app.finish_boot(&ticket, "fake");
    let ResultData::Opened(opened) = job(
        &jobs,
        &app,
        Task::NewLocal {
            home: own.root.clone(),
            active: None,
            epoch: 456,
            pid: 123,
        },
    ) else {
        panic!("wrong delivery");
    };
    assert_eq!(opened.session, "term-123-456");
    assert_eq!(
        opened.metadata.as_ref().unwrap().kind,
        comandos_desktop::TabKind::Scratch
    );
    app.add_tab(&opened.session, &opened.label, &opened.session, false)
        .unwrap();
    app.set_metadata(&opened.session, opened.metadata);
    let tab = app.tabs()[0].clone();
    assert!(matches!(
        job(&jobs, &app, Task::ArchiveClose { tab }),
        ResultData::Closed
    ));
    assert_eq!(
        own.history.lock().unwrap()[0]["session"],
        json!("term-123-456")
    );
    assert_ne!(
        own.tmux
            .run(&["has-session", "-t", "=term-123-456"])
            .unwrap()
            .code,
        Some(0)
    );
    drop(jobs);
    let socket = own.root.join("s");
    drop(own);
    assert!(!socket.exists());
}

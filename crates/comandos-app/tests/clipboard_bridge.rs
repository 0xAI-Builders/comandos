// Filesystem writes below create only injected private fixtures under TMPDIR.
#![allow(clippy::disallowed_methods, clippy::indexing_slicing)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use comandos_app::{
    clipboard_bridge::{Bridge, MAX_BYTES, newest_auto},
    config::RunMode,
    ui::clipboard::{Clipboard, ClipboardPort, Target},
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
#[test]
fn auto_buffers_baseline_restart_and_limits() {
    assert_eq!(
        newest_auto("buffer9\t4\nbuffer10\t8\ncomandos-snip-a\t9\nbuffer11\tbad"),
        Some("buffer10".into())
    );
    let mut b = Bridge::default();
    assert_eq!(b.poll("buffer9\t4"), None);
    assert_eq!(b.poll("buffer10\t8"), Some("buffer10".into()));
    assert_eq!(b.poll("buffer10\t8"), None);
    assert_eq!(b.poll("buffer1\t8"), None);
    assert_eq!(b.poll("buffer2\t0"), None);
    assert_eq!(b.poll(&format!("buffer3\t{}", MAX_BYTES + 1)), None);
    assert_eq!(b.poll("buffer4\t2"), Some("buffer4".into()));
}
type Completion = Box<dyn FnOnce(Option<String>)>;
#[path = "support/t16_oracle.rs"]
mod oracle;
#[test]
fn original_ast_bridge_matches_numeric_watermark_without_importing_app() {
    let rows = vec![
        "buffer9\t3",
        "named\t2\nbuffer10\t8",
        "buffer10\t8",
        "buffer2\t1",
        "buffer3\t0",
        "buffer4\t8388609",
        "buffer5\t5",
        "buffer6\tbad",
        "buffer8\t2\nbuffer7\t3",
        " buffer0009\t4",
        "buffer١٠\t1_2",
        "buffer１１\t+2",
    ];
    let expected = oracle::original(serde_json::json!({"op":"auto","listings":rows}));
    let mut b = Bridge::default();
    assert_eq!(
        serde_json::json!(rows.iter().map(|x| b.poll(x)).collect::<Vec<_>>()),
        expected["poll"]
    );
    let names = expected["newest"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| {
            v.as_array()
                .and_then(|a| a.get(1))
                .cloned()
                .unwrap_or(serde_json::Value::Null)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        serde_json::json!(rows.iter().map(|x| newest_auto(x)).collect::<Vec<_>>()),
        serde_json::json!(names)
    );
}
#[derive(Default)]
struct FakeClipboard {
    writes: RefCell<Vec<(Target, String)>>,
    requests: RefCell<Vec<Completion>>,
}
impl ClipboardPort for FakeClipboard {
    fn set_text(&self, target: Target, text: &str) -> bool {
        self.writes.borrow_mut().push((target, text.into()));
        true
    }
    fn request_text(&self, _: Target, done: Completion) {
        self.requests.borrow_mut().push(done);
    }
}
#[test]
fn shadow_never_reads_or_writes_clipboard() {
    let port = Rc::new(FakeClipboard::default());
    let clip = Clipboard::new(
        RunMode::Shadow,
        port.clone(),
        Arc::new(AtomicBool::new(false)),
    );
    assert!(!clip.copy_selection("secret", 1.));
    clip.request(Target::Clipboard, Box::new(|_| panic!("shadow callback")));
    clip.osc52(Target::Primary, "secret");
    assert!(port.writes.borrow().is_empty());
    assert!(port.requests.borrow().is_empty());
}
#[test]
fn late_clipboard_reply_cannot_cross_instance_or_request_generation() {
    let port = Rc::new(FakeClipboard::default());
    let cancelled = Arc::new(AtomicBool::new(false));
    let clip = Clipboard::new(RunMode::Sandbox, port.clone(), cancelled.clone());
    let accepted = Rc::new(Cell::new(0));
    for _ in 0..2 {
        let a = accepted.clone();
        clip.request(Target::Clipboard, Box::new(move |_| a.set(a.get() + 1)));
    }
    let old = port.requests.borrow_mut().remove(0);
    old(Some("old".into()));
    assert_eq!(accepted.get(), 0);
    cancelled.store(true, Ordering::Release);
    let current = port.requests.borrow_mut().remove(0);
    current(Some("closed".into()));
    assert_eq!(accepted.get(), 0);
    let replacement = Clipboard::new(
        RunMode::Sandbox,
        port.clone(),
        Arc::new(AtomicBool::new(false)),
    );
    let a = accepted.clone();
    replacement.request(Target::Clipboard, Box::new(move |_| a.set(a.get() + 1)));
    port.requests.borrow_mut().remove(0)(Some("ñ😀".into()));
    assert_eq!(accepted.get(), 1);
}
#[test]
fn selection_timestamp_and_osc52_limit_are_instance_local() {
    let p = Rc::new(FakeClipboard::default());
    let c = Clipboard::new(
        RunMode::Sandbox,
        p.clone(),
        Arc::new(AtomicBool::new(false)),
    );
    assert!(!c.copy_selection("", 2.));
    assert!(!c.recent_selection(3.));
    assert!(c.copy_selection("ñ😀", 10.));
    assert!(c.recent_selection(24.99));
    assert!(!c.recent_selection(25.));
    c.osc52(
        Target::Primary,
        &"x".repeat(comandos_term::engine::MAX_CLIPBOARD_BYTES + 1),
    );
    assert_eq!(p.writes.borrow().len(), 1);
    c.release_selection();
    assert!(c.take_selection_release());
    assert!(!c.take_selection_release());
}
#[test]
fn replay_queue_is_bounded_orders_keys_and_marks_only_first_copy_context() {
    use comandos_app::ui::clipboard::Replay;
    let mut replay = Replay::default();
    assert!(!replay.pending());
    replay.begin("first").unwrap();
    replay.push("second").unwrap();
    assert_eq!(
        replay.finish(true),
        vec![("first", true), ("second", false)]
    );
    assert!(!replay.pending());
    replay.begin("first").unwrap();
    for _ in 1..64 {
        replay.push("more").unwrap();
    }
    assert!(replay.push("overflow").is_err());
    replay.cancel();
    assert!(replay.finish(true).is_empty());
}
struct ScriptIo {
    mode: RunMode,
    reads: RefCell<std::collections::VecDeque<(String, String)>>,
    mutations: RefCell<Vec<Vec<String>>>,
    fail_first: Cell<bool>,
    cancelled: Arc<AtomicBool>,
}
impl ScriptIo {
    fn new(rows: &[(&str, &str)]) -> Self {
        Self {
            mode: RunMode::Sandbox,
            reads: RefCell::new(
                rows.iter()
                    .map(|(a, b)| (a.to_string(), b.to_string()))
                    .collect(),
            ),
            mutations: RefCell::new(vec![]),
            fail_first: Cell::new(false),
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }
}
impl comandos_app::ui::clipboard::TmuxIo for ScriptIo {
    fn mode(&self) -> RunMode {
        self.mode
    }
    fn socket(&self) -> &std::path::Path {
        std::path::Path::new("/tmp/private-fake-S-t16")
    }
    fn read(
        &self,
        args: &[&str],
    ) -> Result<comandos_app::tmux::TmuxOut, comandos_app::tmux::TmuxError> {
        let (command, text) = self
            .reads
            .borrow_mut()
            .pop_front()
            .expect("unexpected read");
        assert_eq!(args[0], command);
        if text == "cancel" {
            self.cancelled.store(true, Ordering::Release);
        }
        Ok(comandos_app::tmux::TmuxOut {
            code: 0,
            stdout: if text == "cancel" {
                "111|$1|42|%7".into()
            } else {
                text
            },
            stderr: String::new(),
        })
    }
    fn mutate(
        &self,
        args: &[&str],
        _: Option<&[u8]>,
    ) -> Result<comandos_app::tmux::TmuxOut, comandos_app::tmux::TmuxError> {
        self.mutations
            .borrow_mut()
            .push(args.iter().map(|s| s.to_string()).collect());
        Ok(comandos_app::tmux::TmuxOut {
            code: i32::from(self.fail_first.replace(false)),
            stdout: String::new(),
            stderr: String::new(),
        })
    }
}
#[test]
fn copy_mode_cancellation_after_read_never_sends_keys() {
    let io = ScriptIo::new(&[("display-message", "cancel")]);
    let cancel = io.cancelled.clone();
    assert!(
        comandos_app::clipboard_bridge::copy_tmux_selection(&io, "%7", || cancel
            .load(Ordering::Acquire))
        .is_err()
    );
    assert!(
        io.mutations.borrow().is_empty(),
        "closed terminal copied a global tmux buffer after capture"
    );
}
#[test]
fn actual_tty_and_copy_mode_exit_pin_destination() {
    use comandos_app::clipboard_bridge::{exit_copy_mode, term_session};
    let io = ScriptIo::new(&[(
        "list-clients",
        "/tty-other|other\n/private/fake-tty|fixture\n",
    )]);
    assert_eq!(
        term_session(&io, Some("/private/fake-tty")).unwrap(),
        Some("fixture".into())
    );
    let io = ScriptIo::new(&[("list-clients", "/tty-other|other")]);
    assert_eq!(term_session(&io, Some("/private/missing")).unwrap(), None);
    let io = ScriptIo::new(&[
        ("list-clients", "/private/fake-tty|fixture"),
        ("display-message", "1|111|$1|42|%7"),
        ("display-message", "222|$1|42|%7"),
    ]);
    assert!(exit_copy_mode(&io, Some("/private/fake-tty"), || false).is_err());
    assert!(io.mutations.borrow().is_empty());
    let io = ScriptIo::new(&[
        ("list-clients", "/private/fake-tty|fixture"),
        ("display-message", "1|111|$1|42|%7"),
        ("display-message", "111|$1|42|%7"),
    ]);
    assert!(exit_copy_mode(&io, Some("/private/fake-tty"), || false).unwrap());
    assert_eq!(io.mutations.borrow().len(), 1);
}

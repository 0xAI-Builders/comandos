#![allow(clippy::unwrap_used)]
use comandos_app_mac::{
    app::{Action, App},
    jobs::{Backend, execute_open},
};
use comandos_desktop::{Lang, proc::ProcOutput};
use serde_json::{Value, json};
use std::sync::Mutex;
struct Fake {
    app: Mutex<App>,
    calls: Mutex<Vec<String>>,
    cancel_on_post: bool,
}
impl Backend for Fake {
    fn get(&self, _: &str) -> Result<Value, String> {
        Ok(json!({}))
    }
    fn post(&self, path: &str, _: &Value) -> Result<Value, String> {
        self.calls.lock().unwrap().push(path.into());
        if self.cancel_on_post {
            self.app.lock().unwrap().shutdown();
        }
        Ok(json!({}))
    }
    fn tmux(&self, args: &[&str], allowed: &dyn Fn() -> bool) -> Result<ProcOutput, String> {
        assert!(allowed());
        self.calls.lock().unwrap().push(args.join(" "));
        Ok(ProcOutput {
            code: Some(0),
            ..Default::default()
        })
    }
    fn cancel(&self) {}
}
#[test]
fn cancelled_ensure_response_never_selects_or_opens_a_tab() {
    let fake = Fake {
        app: Mutex::new(App::new("noche", Lang::Es)),
        calls: Mutex::new(vec![]),
        cancel_on_post: true,
    };
    let ticket = fake.app.lock().unwrap().ticket();
    assert!(
        execute_open(
            &fake,
            &Action::Open {
                session: "owned".into(),
                win: "claude".into(),
                label: None
            },
            false,
            &ticket
        )
        .is_err()
    );
    assert_eq!(*fake.calls.lock().unwrap(), vec!["/ensure"]);
}
#[test]
fn existing_session_selects_without_ensure() {
    let fake = Fake {
        app: Mutex::new(App::new("noche", Lang::Es)),
        calls: Mutex::new(vec![]),
        cancel_on_post: false,
    };
    let ticket = fake.app.lock().unwrap().ticket();
    let opened = execute_open(
        &fake,
        &Action::Open {
            session: "owned".into(),
            win: "work".into(),
            label: Some("Label".into()),
        },
        true,
        &ticket,
    )
    .unwrap();
    assert_eq!(opened.session, "owned");
    assert_eq!(
        *fake.calls.lock().unwrap(),
        vec!["select-window -t =owned:work"]
    );
}
#[test]
fn dom_dump_refuses_fifo_symlink_and_existing_files_without_blocking() {
    use comandos_app_mac::jobs::write_dom;
    use std::os::unix::fs::{DirBuilderExt, symlink};
    let root = std::env::temp_dir().join(format!("mac-m3-dump-{}", std::process::id()));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&root)
        .unwrap();
    let a = App::new("noche", Lang::Es);
    let ticket = a.ticket();
    let path = root.join("dom.html");
    write_dom(&path, "<html>owned</html>", &ticket).unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "<html>owned</html>"
    );
    assert!(write_dom(&path, "overwrite", &ticket).is_err());
    let link = root.join("link");
    symlink(&path, &link).unwrap();
    assert!(write_dom(&link, "changed", &ticket).is_err());
    let fifo = root.join("fifo");
    nix::unistd::mkfifo(
        &fifo,
        nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR,
    )
    .unwrap();
    assert!(write_dom(&fifo, "changed", &ticket).is_err());
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn dom_dump_accepts_owned_directory_through_a_system_style_parent_alias() {
    use comandos_app_mac::jobs::write_dom;
    use std::os::unix::fs::{DirBuilderExt, symlink};
    let root = std::env::temp_dir().join(format!("mac-m3-alias-{}", std::process::id()));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&root)
        .unwrap();
    let real = root.join("real");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&real)
        .unwrap();
    let child = real.join("child");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&child)
        .unwrap();
    let alias = root.join("alias");
    symlink(&real, &alias).unwrap();
    let app = App::new("noche", Lang::Es);
    let result = write_dom(&alias.join("child/dom.html"), "owned", &app.ticket());
    std::fs::remove_dir_all(root).unwrap();
    assert!(result.is_ok(), "{result:?}");
}

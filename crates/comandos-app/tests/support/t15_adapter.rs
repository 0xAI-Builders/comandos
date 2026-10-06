//! Extrae los cuerpos de producción y sustituye únicamente colaboradores de GUI/HTTP.
#![allow(
    clippy::disallowed_methods,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing
)]
use comandos_app::proc::{ProcSpec, run};
use std::path::{Path, PathBuf};
fn body(source: &str, needle: &str) -> String {
    let start = source.find(needle).expect(needle);
    let brace = start + source[start..].find('{').unwrap();
    let mut level = 0;
    for (i, b) in source.as_bytes().iter().enumerate().skip(brace) {
        if *b == b'{' {
            level += 1;
        } else if *b == b'}' {
            level -= 1;
            if level == 0 {
                return source[start..=i]
                    .replace("pub(super) ", "")
                    .replace("pub(in crate::ui) ", "");
            }
        }
    }
    panic!("unclosed {needle}")
}
fn library(dir: &Path, name: &str) -> PathBuf {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|f| f.path())
        .filter(|p| {
            p.extension().is_some_and(|e| e == "rlib")
                && p.file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .starts_with(&format!("lib{name}-"))
        })
        .max_by_key(|p| p.metadata().unwrap().modified().unwrap())
        .unwrap()
}
fn dependency(deps: &Path, app: &Path, name: &str) -> PathBuf {
    let hash = app
        .file_stem()
        .unwrap()
        .to_str()
        .unwrap()
        .strip_prefix("libcomandos_app-")
        .unwrap();
    let fingerprints = deps.parent().unwrap().join(".fingerprint");
    let meta: serde_json::Value = serde_json::from_slice(
        &std::fs::read(fingerprints.join(format!("comandos-app-{hash}/lib-comandos_app.json")))
            .unwrap(),
    )
    .unwrap();
    let wanted = meta["deps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d[1] == name)
        .unwrap()[3]
        .as_u64()
        .unwrap();
    for dir in std::fs::read_dir(&fingerprints).unwrap().flatten() {
        let encoded = dir.path().join(format!("lib-{name}"));
        if let Ok(text) = std::fs::read_to_string(encoded) {
            let bytes: Vec<_> = (0..8)
                .map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).unwrap())
                .collect();
            if u64::from_le_bytes(bytes.try_into().unwrap()) == wanted {
                let folder = dir.file_name();
                let suffix = folder.to_str().unwrap().rsplit('-').next().unwrap();
                let library = deps.join(format!("lib{name}-{suffix}.rlib"));
                if library.exists() {
                    return library;
                }
            }
        }
    }
    panic!("missing matched dependency {name}")
}
pub fn execute() -> serde_json::Value {
    let dir = std::env::var_os("COMANDOS_T15_PROBE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("comandos-t15-probe-{}", std::process::id()))
        });
    std::fs::create_dir_all(&dir).unwrap();
    let panel = include_str!("../../src/ui/switcher.rs");
    let panel_methods = body(panel, "pub fn key(") + &body(panel, "pub fn query(");
    let term = include_str!("../../src/term/view.rs");
    let term_methods = body(term, "pub fn shutdown(")
        + &body(term, "pub fn on_app_key(")
        + &body(term, "pub fn on_key_observer(")
        + &body(term, "pub fn cleanup_cancellation(")
        + &body(term, "pub fn ctrl_c_action(")
        + &body(term, "pub fn clipboard(")
        + &body(term, "pub fn on_primary_press(")
        + &body(term, "pub fn on_context_menu(")
        + &body(term, "pub fn on_clean_click(")
        + &body(term, "pub fn on_link_event(")
        + &body(term, "pub fn paste_clipboard(")
        + &body(term, "pub fn clear_selection(")
        + &body(term, "pub fn selection_text(")
        + &body(term, "pub fn key_pending(")
        + &body(term, "pub fn queue_key(")
        + &body(term, "pub fn finish_keys(")
        + &body(term, "pub fn forward_key(");
    let dispatch = body(term, "fn dispatch_app_key(")
        + &body(term, "fn forward_key(self:")
        + &body(
            &term[term.find("impl Inner {").unwrap()..],
            "fn selection_text(",
        );
    let teardown = body(term, "impl Drop for Inner {");
    let key_callback = body(term, "inner.area.connect_key_press_event(");
    let term_code = format!(
        "impl TermView{{{term_methods}}}impl Inner{{{dispatch}}}{teardown}fn connect(inner:&Rc<Inner>){{let weak=Rc::downgrade(inner);{key_callback});}}"
    );
    let app = include_str!("../../src/ui/app_t15.rs");
    let mut app_methods = String::new();
    for n in [
        "install_keys",
        "attach_term_keys",
        "handle_key",
        "execute_key",
        "execute_key_context",
        "focus_current_term",
        "arm_agent_cleanup",
        "modal_key",
        "close_modal",
        "mount_modal",
        "restack_modal",
    ] {
        app_methods += &body(app, &format!("fn {n}("));
    }
    let t16 = include_str!("../../src/ui/app_t16.rs");
    app_methods += &body(t16, "fn instance(");
    app_methods += &body(t16, "fn defer_copy_key(");
    for needle in [
        "fn attach_term_t16(",
        "fn select_click(",
        "fn capture_primary_click(",
        "fn install_t16(",
        "fn paste_terminal(",
        "fn copy_terminal(",
        "fn feedback(",
    ] {
        app_methods += &body(t16, needle);
    }
    app_methods = app_methods
        .replace("KeyAction::", "AppKeyAction::")
        .replace("glib::idle_add_local_once", "idle_add_local_once");
    let activate = body(app, "let activate = Rc::new(move |row: &Candidate|");
    let close = body(app, "let close = Rc::new(move |row: &Candidate|");
    let app_code = format!(
        "impl App{{{app_methods}}}fn actions(app:&Rc<App>,generation:u64)->(Rc<dyn Fn(&Candidate)>,Rc<dyn Fn(&Candidate)>){{let weak=Rc::downgrade(app);{activate});let weak=Rc::downgrade(app);{close});(activate,close)}}"
    );
    let stop = include_str!("../../src/agent_stop.rs");
    let pidfd = (body(stop, "pub struct PinnedProcess {")
        + &body(stop, "impl SignalSink for PidfdSignals {"))
        .replace("std::os::fd::OwnedFd", "FakeFd");
    let click = body(panel, "list.connect_row_activated(");
    let click_code = format!(
        "fn click(rows:&Rc<RefCell<Vec<Candidate>>>,go:Rc<dyn Fn(&Candidate)>,index:i32){{let list=Click(index);let weak=Rc::downgrade(rows);{click});}}"
    );
    let pidfd_code = format!(
        "mod pidfd_probe{{{}{pidfd}}}",
        include_str!("t15_pidfd.rs.txt")
    );
    let click_state = "#[derive(Default)]".to_owned() + &body(t16, "struct ClickState {");
    let source = format!(
        "{}\n{click_state}impl Panel{{{panel_methods}}}\n{term_code}\n{app_code}\n{pidfd_code}\n{click_code}\n{}",
        include_str!("t15_probe_preamble.rs.txt"),
        include_str!("t15_probe_main.rs.txt")
    );
    let path = dir.join("main.rs");
    std::fs::write(&path, &source).unwrap();
    let deps = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let app_lib = library(&deps, "comandos_app");
    let exe = dir.join("probe");
    let flags = run(&ProcSpec {
        program: "pkg-config".into(),
        args: vec!["--libs-only-L".into(), "webkit2gtk-4.1".into()],
        stdin: None,
        env: vec![],
        clear_env: false,
        env_remove: vec![],
        cwd: None,
        timeout: std::time::Duration::from_secs(5),
    })
    .unwrap();
    assert_eq!(flags.code, Some(0), "SDK linker paths");
    let mut arguments = vec![
        "--edition=2024".into(),
        path.into_os_string(),
        "-L".into(),
        format!("dependency={}", deps.display()).into(),
        "--extern".into(),
        format!("comandos_app={}", app_lib.display()).into(),
        "--extern".into(),
        format!(
            "serde_json={}",
            dependency(&deps, &app_lib, "serde_json").display()
        )
        .into(),
        "--extern".into(),
        format!(
            "comandos_core={}",
            dependency(&deps, &app_lib, "comandos_core").display()
        )
        .into(),
        "-o".into(),
        exe.clone().into_os_string(),
    ];
    for name in ["glib", "gdk_pixbuf", "gdk"] {
        arguments.push("--extern".into());
        arguments.push(format!("{name}={}", dependency(&deps, &app_lib, name).display()).into());
    }
    for flag in String::from_utf8(flags.stdout).unwrap().split_whitespace() {
        if let Some(path) = flag.strip_prefix("-L") {
            arguments.push("-L".into());
            arguments.push(format!("native={path}").into());
        }
    }
    let output = run(&ProcSpec {
        program: "rustc".into(),
        args: arguments,
        stdin: None,
        env: vec![],
        clear_env: false,
        env_remove: vec![],
        cwd: None,
        timeout: std::time::Duration::from_secs(60),
    })
    .unwrap();
    std::fs::write(dir.join("compile.log"), &output.stderr).unwrap();
    assert_eq!(
        output.code,
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = run(&ProcSpec {
        program: exe.display().to_string(),
        args: vec![],
        stdin: None,
        env: vec![],
        clear_env: false,
        env_remove: vec![],
        cwd: None,
        timeout: std::time::Duration::from_secs(10),
    })
    .unwrap();
    std::fs::write(dir.join("run.log"), &output.stdout).unwrap();
    std::fs::write(dir.join("run.stderr.log"), &output.stderr).unwrap();
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.code,
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_str(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .last()
            .unwrap(),
    )
    .unwrap()
}

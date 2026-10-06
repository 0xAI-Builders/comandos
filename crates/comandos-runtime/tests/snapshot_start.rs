use comandos_runtime::{pane_snapshot::PaneInspector, pane_typing::TmuxResult, tmux_snapshot};
use std::{fs, os::unix::fs::DirBuilderExt};
#[test]
fn capture_uses_only_the_supplied_start_reader_and_private_inventory() {
    let root = std::env::temp_dir().join(format!("comandos-snapshot-start-{}", std::process::id()));
    for sub in ["home", "proc/42"] {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(root.join(sub))
            .unwrap();
    }
    fs::write(
        root.join("proc/42/stat"),
        format!("42 (private shell) S 0 {} 123", vec!["0"; 17].join(" ")),
    )
    .unwrap();
    fs::write(root.join("proc/42/cmdline"), b"/bin/bash\0").unwrap();
    let inspector = PaneInspector::new(&root.join("home"), &root.join("proc")).unwrap();
    let body = "100x30,0,0,1";
    let layout = format!("{:04x},{body}", tmux_snapshot::layout_checksum(body));
    let mut callback = |args: &[&str]| -> tmux_snapshot::Result<TmuxResult> {
        let stdout = match args[0] {
            "list-windows" => format!("@1\t0\tprivate\t{layout}\t100\t30\t1\t0"),
            "show-options" => String::new(),
            "list-panes" => "%1\t0\t/private\t42\tbash\t1\t".into(),
            "display-message" => layout.clone(),
            _ => panic!("unexpected fake tmux operation"),
        };
        Ok(TmuxResult {
            returncode: 0,
            stdout,
            stderr: String::new(),
        })
    };
    let mut reads = vec![];
    let captured = tmux_snapshot::capture_session_with_start_at(
        &mut callback,
        "local",
        &inspector,
        600,
        &mut |pid| {
            reads.push(pid.to_string());
            comandos_runtime::parse_process_stat(
                &fs::read_to_string(root.join("proc").join(pid).join("stat")).unwrap(),
            )
            .map(serde_json::Value::from)
            .unwrap()
        },
    )
    .unwrap();
    assert_eq!(reads, vec!["42"]);
    assert_eq!(captured["windows"][0]["panes"][0]["start"], 123);
    assert_eq!(captured["captured_at"], 600);
    fs::remove_dir_all(root).unwrap();
}

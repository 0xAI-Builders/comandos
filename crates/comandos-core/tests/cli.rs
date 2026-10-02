use serde_json::{Value, json};
use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn malformed_request_does_not_drop_following_events() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_comandos-contract"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"not-json\n{\"op\":\"unknown\"}\n{\"op\":\"destination\",\"event\":{\"paneKey\":\"p\"}}\n").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let values: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert!(values[0]["error"].is_string());
    assert_eq!(values[1], json!({"error":"operación desconocida"}));
    assert_eq!(values[2], "pane");
}

#[test]
fn overlarge_line_is_rejected_before_unbounded_allocation() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_comandos-contract"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&vec![b' '; 1024 * 1024 + 1])
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8(output.stderr).unwrap().contains("1 MiB"));
}

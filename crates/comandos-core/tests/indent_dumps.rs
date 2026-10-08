//! `json.dumps(x, indent=n, ensure_ascii=…)` del Python: orden de inserción,
//! separadores `","`/`": "` con saltos de línea, contenedores vacíos compactos.
use comandos_core::json::{indent_dumps, workspace_loads};
use std::process::Command;

const CASES: [&str; 6] = [
    r#"{"hooks": {"Stop": [{"matcher": "", "hooks": [{"type": "command", "command": "x ñ"}]}]}, "n": 1.0, "e": [], "o": {}, "big": 123456789012345678901234567890}"#,
    r#"[1, 2.5, -0, 1e400, NaN, null, true, "a\u0001\n\"b", [], {}, [[]], {"k": {}}]"#,
    r#"{"projects": {"/home/x/é": {"hasTrustDialogAccepted": true}}, "dup": 1, "dup": 2}"#,
    r#""texto ☃""#,
    r#"{}"#,
    r#"[]"#,
];

/// Default replay uses immutable CPython output and never starts Python.
fn python(raw: &str, indent: usize, ascii: bool) -> String {
    let input = serde_json::json!({"source":"CPython stdlib json", "raw":raw,"indent":indent,"ascii":ascii});
    let bytes = comandos_oracle::oracle_at(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden"),
        "core-json-indent",
        &input,
        || {
            let flag = if ascii { "True" } else { "False" };
            let out = Command::new(std::env::var_os("COMANDOS_CORE_ORACLE_PYTHON").unwrap_or_else(|| "python3".into()))
                .arg("-c")
                .arg(format!("import json,sys; sys.stdout.write(json.dumps(json.loads(sys.argv[1]), indent={indent}, ensure_ascii={flag}))"))
                .arg(raw).env("PYTHONIOENCODING", "utf-8").output().map_err(|e| e.to_string())?;
            if !out.status.success() {
                return Err(String::from_utf8_lossy(&out.stderr).into_owned());
            }
            Ok(out.stdout)
        },
    );
    String::from_utf8(bytes).unwrap()
}

#[test]
fn matches_python_indent_dumps() {
    for raw in CASES {
        let value = workspace_loads(raw).unwrap();
        for indent in [2, 4] {
            for ascii in [false, true] {
                let theirs = python(raw, indent, ascii);
                assert_eq!(
                    indent_dumps(&value, indent, ascii).unwrap(),
                    theirs,
                    "{raw} indent={indent} ascii={ascii}"
                );
            }
        }
    }
}

#[test]
fn literal_shape() {
    let value = workspace_loads(r#"{"a": [1, {}], "b": {"c": []}}"#).unwrap();
    assert_eq!(
        indent_dumps(&value, 2, false).unwrap(),
        "{\n  \"a\": [\n    1,\n    {}\n  ],\n  \"b\": {\n    \"c\": []\n  }\n}"
    );
}

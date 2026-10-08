//! `json::python_loads` contra `json.loads` del `python3` del sistema: el mismo
//! «decodifica / mensaje de `JSONDecodeError`» para una lista escrita a mano y
//! miles de textos casi-JSON generados (semilla fija). Replay no ejecuta Python.
use comandos_core::json::{PythonLoads, python_loads};
use serde_json::Value;
use std::{
    io::Write,
    process::{Command, Stdio},
};

const ORACLE: &str = r#"
import json, sys
out = []
for text in json.load(sys.stdin):
    try:
        json.loads(text)
        out.append(None)
    except json.JSONDecodeError as exc:
        out.append(str(exc))
    except Exception as exc:
        out.append("OTHER " + type(exc).__name__)
print(json.dumps(out))
"#;

fn python(inputs: &[String]) -> Vec<Value> {
    let input = serde_json::json!({"script":ORACLE,"inputs":inputs});
    let bytes = comandos_oracle::oracle_at(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden"),
        "core-json-loads",
        &input,
        || {
            let mut child = Command::new(
                std::env::var_os("COMANDOS_CORE_ORACLE_PYTHON").unwrap_or_else(|| "python3".into()),
            )
            .args(["-c", ORACLE])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?;
            child
                .stdin
                .take()
                .unwrap()
                .write_all(serde_json::to_string(inputs).unwrap().as_bytes())
                .map_err(|e| e.to_string())?;
            let out = child.wait_with_output().map_err(|e| e.to_string())?;
            if !out.status.success() {
                return Err("CPython loads oracle failed".into());
            }
            Ok(out.stdout)
        },
    );
    serde_json::from_slice::<Value>(&bytes)
        .unwrap()
        .as_array()
        .unwrap()
        .clone()
}

/// Generador congruencial: reproducible sin dependencias.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[(self.next() as usize) % items.len()]
    }
}

fn corpus() -> Vec<String> {
    let mut out: Vec<String> = [
        "",
        " ",
        "\n",
        "{broken",
        "false trailing",
        "{\"a\":1,}",
        "[1,]",
        "[,1]",
        "\"abc",
        "\"a\\",
        "\"a\\q\"",
        "\"\\u123",
        "\"\\u12\"",
        "\"\\uZZZZ\"",
        "\"\\ud800\\uZZ\"",
        "\"\\ud800\\udc00\"",
        "\"\\ud800\\u0041\"",
        "\"\\ud800\\uZZZZ\"",
        "1e",
        "1e+",
        "1.",
        "-",
        "-0",
        "01",
        "1.5e-3",
        "\"\u{1}\"",
        "\u{feff}{}",
        "{\"a\" 1}",
        "{\"a\":}",
        "{1:2}",
        "[1 2]",
        "nul",
        "NaN",
        "-Infinity",
        "Infinity x",
        "{\"ok\": true, \"closed\": []}",
        "\n\n  [\n 1,\n x]",
        "\"ñ\" x",
        "[\"ñandú\", ]",
        "{\"a\":\"\\n\"}",
        "[[[[",
        "]",
        "{\"a\":[1,{\"b\":\"c\\\"\"}]}",
        "\"\\/\\b\\f\\n\\r\\t\"",
        "tru",
        "true false",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    let pieces = [
        "{", "}", "[", "]", "\"", ":", ",", " ", "\n", "\t", "\\", "u", "d8", "dc", "00", "1", "0",
        "-", "+", ".", "e", "E", "a", "ñ", "null", "true", "fals", "NaN", "Infin", "\u{1}",
        "\"k\"", "\"v\"", "12", "\\u00", "\\n", "\\x", "x",
    ];
    let mut rng = Lcg(0x5eed_2f1d);
    for _ in 0..20_000 {
        let n = 1 + rng.next() as usize % 9;
        out.push((0..n).map(|_| rng.pick(&pieces)).collect());
    }
    out
}

#[test]
fn messages_match_cpython() {
    let inputs = corpus();
    let expected = python(&inputs);
    assert_eq!(inputs.len(), expected.len());
    let mut checked = 0;
    for (text, want) in inputs.iter().zip(&expected) {
        match python_loads(text) {
            PythonLoads::Unsure => continue,
            PythonLoads::Ok => assert_eq!(want, &Value::Null, "{text:?}"),
            PythonLoads::Error(message) => assert_eq!(want, &Value::String(message), "{text:?}"),
        }
        checked += 1;
    }
    assert!(checked > 19_000, "{checked}");
}

//! `ssh_config::{parse, read, hosts, host_entry}` contra `parse_ssh_config` y
//! `ssh_host_entry` de `bin/cc-dash`. El `~/.ssh/config` es siempre el de un
//! HOME temporal (nunca el real del usuario).
#[path = "support/python.rs"]
mod python;

use comandos_core::json::response_dumps;
use comandos_runtime::ssh_config::{self, host_entry, hosts, is_host, parse};
use python::run_python;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const LOAD_DASH: &str = r#"
import importlib.machinery, importlib.util, json, os, sys
repo = sys.argv[1]
sys.path.insert(0, os.path.join(repo, "bin"))
loader = importlib.machinery.SourceFileLoader("cc_dash_oracle", os.path.join(repo, "bin/cc-dash"))
spec = importlib.util.spec_from_loader(loader.name, loader)
dash = importlib.util.module_from_spec(spec)
loader.exec_module(dash)
"#;

const PARSE: &str = r#"
texts = json.load(open(sys.argv[2]))
print(json.dumps([dash.parse_ssh_config(t) for t in texts]))
"#;

/// Sin texto: el archivo del HOME; el error de lectura como su tipo.
const FROM_HOME: &str = r#"
def run(f):
    try:
        return f()
    except Exception as e:
        return {"error": type(e).__name__}
print(json.dumps([run(dash.parse_ssh_config), run(lambda: dash.ssh_host_entry("b")),
                  run(lambda: dash.ssh_host_entry("nada"))]))
"#;

const TEXTS: [&str; 12] = [
    "",
    "# comentario\n   # otro\n\n",
    "Host a b *.x c?\n  HostName 10.0.0.1\n  User yo\n",
    "HOST Mayus\nHOSTNAME h.example\nPORT 2222\nIdentityFile ~/.ssh/k\n",
    "Host\ttab\n\tUser\tconte  nido \t\n",
    "Host crlf\r\n  Port 22\r\n  User u\r\n",
    "Host sep\u{1f}otro\n  User\u{1f}x\n",
    "Host k\n  IdentityFile ~/.ssh/k\n  identityfile ~/.ssh/k2\n",
    "Host\n  User huérfano\nHost z\n  User zeta\n",
    "Host v\n  User\n  Port   \n  HostName  h  \n",
    "User antes\nHost *\n  User comodín\nHost w\n  ProxyCommand nc %h\n  user abc\n",
    "Host \u{0130}stanbul\n  \u{212a}ey x\n  HOSTNAME ok\n",
];

fn home(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cmd-ssh-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn parse_matches_python() {
    let dir = home("parse");
    let case = dir.join("texts.json");
    std::fs::write(&case, json!(TEXTS).to_string()).unwrap();
    let expected = run_python(&format!("{LOAD_DASH}{PARSE}"), &[case.as_os_str()], &dir);
    let rust: Vec<Value> = TEXTS
        .iter()
        .map(|t| Value::Array(parse(t).into_iter().map(Value::Object).collect()))
        .collect();
    let rust = response_dumps(&Value::Array(rust)).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    let Some(expected) = expected else {
        return;
    };
    assert_eq!(rust, expected.trim_end());
    assert!(
        rust.contains(r#"{"host": "a"}, {"host": "b", "hostname""#),
        "{rust}"
    );
}

fn from_home(dir: &Path) -> String {
    let wrap = |r: std::io::Result<Value>| match r {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
            json!({"error": "UnicodeDecodeError"})
        }
        Err(e) if e.kind() == std::io::ErrorKind::IsADirectory => {
            json!({"error": "IsADirectoryError"})
        }
        Err(e) => panic!("{e}"),
    };
    let all = wrap(hosts(dir).map(|h| Value::Array(h.into_iter().map(Value::Object).collect())));
    let b = wrap(host_entry(dir, "b").map(|e| e.map_or(Value::Null, Value::Object)));
    let none = wrap(host_entry(dir, "nada").map(|e| e.map_or(Value::Null, Value::Object)));
    response_dumps(&json!([all, b, none])).unwrap()
}

#[test]
fn read_and_host_entry_match_python() {
    let dir = home("read");
    let ssh = dir.join(".ssh");
    let mut cases: Vec<(&str, Option<Vec<u8>>)> = vec![
        ("ausente", None),
        (
            "crlf",
            Some(b"Host a\r\n  User u\rHost b c\r\n  Port 2\r\n  HostName b.x\n".to_vec()),
        ),
        ("no-utf8", Some(b"Host b\n  User \xff\n".to_vec())),
    ];
    for (tag, content) in cases.drain(..) {
        let _ = std::fs::remove_dir_all(&ssh);
        if let Some(bytes) = &content {
            std::fs::create_dir_all(&ssh).unwrap();
            std::fs::write(ssh.join("config"), bytes).unwrap();
        }
        let rust = from_home(&dir);
        let Some(expected) = run_python(&format!("{LOAD_DASH}{FROM_HOME}"), &[], &dir) else {
            break;
        };
        assert_eq!(rust, expected.trim_end(), "{tag}");
    }
    // Un directorio en lugar del archivo: el `OSError` sube.
    let _ = std::fs::remove_dir_all(&ssh);
    std::fs::create_dir_all(ssh.join("config")).unwrap();
    assert!(ssh_config::read(&dir).is_err());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn host_re_matches_python() {
    let dir = home("re");
    let samples = [
        "a",
        "srv-1.b_c",
        "",
        "a b",
        "a\n",
        "ñ",
        "x".repeat(60).leak(),
        "y".repeat(61).leak(),
    ];
    let case = dir.join("hosts.json");
    std::fs::write(&case, json!(samples).to_string()).unwrap();
    let expected = run_python(
        &format!(
            "{LOAD_DASH}print(json.dumps([bool(dash.SSH_HOST_RE.match(s)) for s in json.load(open(sys.argv[2]))]))\n"
        ),
        &[case.as_os_str()],
        &dir,
    );
    let rust: Vec<bool> = samples.iter().map(|s| is_host(s)).collect();
    std::fs::remove_dir_all(&dir).unwrap();
    let Some(expected) = expected else {
        return;
    };
    assert_eq!(
        json!(rust).to_string().replace(',', ", "),
        expected.trim_end()
    );
}

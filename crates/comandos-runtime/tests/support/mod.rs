//! Apoyo de `launch_command_oracle.rs` y `claude_trust_oracle.rs`: HOME
//! temporales y el oráculo `bin/cc-dash` (D8) cargado con `SourceFileLoader`.
//!
//! El Python corre con el entorno vacío salvo `HOME`, `PATH` y la
//! codificación: el `PATH` solo tiene `<HOME>/.oracle/bin`, con `/bin/true`
//! enlazado como cada CLI y como `tmux`/`systemctl`, más `python3.11` (el lector
//! TOML que usa el `_parse_toml` del Python 3.10). Nada fuera del HOME.
#![allow(dead_code)]
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};

pub fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// `config/providers.json` tal cual (el registro del oráculo es el mismo).
pub fn registry() -> Value {
    let text = fs::read_to_string(repo().join("config/providers.json")).unwrap();
    comandos_core::json::workspace_loads(&text).unwrap()
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Los CLI falsos del `PATH` del oráculo (y del `Ctx` de las pruebas).
const FAKE: &[&str] = &[
    "claude",
    "codex",
    "grok",
    "cc-acp",
    "opencode",
    "agy",
    "tmux",
    "systemctl",
    "systemd-run",
    "notify-send",
    "xdg-open",
    "wmctrl",
];

pub struct Home(PathBuf);

impl Home {
    pub fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(comandos_runtime::fresh_id(tag).unwrap());
        fs::create_dir(&p).unwrap();
        let p = p.canonicalize().unwrap();
        let bin = p.join(".oracle/bin");
        fs::create_dir_all(&bin).unwrap();
        for name in FAKE {
            std::os::unix::fs::symlink("/bin/true", bin.join(name)).unwrap();
        }
        if Path::new("/usr/bin/python3.11").exists() {
            std::os::unix::fs::symlink("/usr/bin/python3.11", bin.join("python3.11")).unwrap();
        }
        Self(p)
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
    pub fn s(&self) -> String {
        self.0.display().to_string()
    }
    pub fn bin(&self) -> PathBuf {
        self.0.join(".oracle/bin")
    }
    pub fn put(&self, relative: &str, text: &str) {
        let p = self.0.join(relative);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }
    /// (ruta relativa, contenido con el HOME como `~`, modo) bajo `.codex*` y
    /// `code`; los enlaces como `-> destino`.
    pub fn tree(&self) -> Vec<(String, String, u32)> {
        fn walk(dir: &Path, root: &Path, home: &str, out: &mut Vec<(String, String, u32)>) {
            let Ok(entries) = fs::read_dir(dir) else {
                return;
            };
            let mut entries: Vec<_> = entries.map(|e| e.unwrap().path()).collect();
            entries.sort();
            for path in entries {
                let rel = path.strip_prefix(root).unwrap().display().to_string();
                let meta = fs::symlink_metadata(&path).unwrap();
                let mode = meta.permissions().mode() & 0o7777;
                if meta.file_type().is_symlink() {
                    let target = fs::read_link(&path).unwrap().display().to_string();
                    out.push((rel, format!("-> {}", target.replace(home, "~")), 0));
                } else if meta.is_dir() {
                    out.push((rel, String::new(), mode));
                    walk(&path, root, home, out);
                } else {
                    let text = fs::read_to_string(&path).unwrap().replace(home, "~");
                    out.push((rel, text, mode));
                }
            }
        }
        let mut out = Vec::new();
        for top in [".codex", ".codex-accounts", "code"] {
            walk(&self.0.join(top), &self.0, &self.s(), &mut out);
        }
        out
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// El entorno completo del oráculo (su `os.environ`).
pub fn oracle_environ(home: &Home) -> Vec<(String, String)> {
    vec![
        ("HOME".into(), home.s()),
        ("PATH".into(), home.bin().display().to_string()),
        ("LANG".into(), "C.UTF-8".into()),
        ("PYTHONDONTWRITEBYTECODE".into(), "1".into()),
    ]
}

const SCRIPT: &str = r#"
import importlib.machinery, importlib.util, json, os, sys
repo, case_path = sys.argv[1], sys.argv[2]
sys.path[:0] = [os.path.join(repo, 'bin'), os.path.join(repo, 'lib')]
loader = importlib.machinery.SourceFileLoader('cc_dash_oracle', os.path.join(repo, 'bin/cc-dash'))
spec = importlib.util.spec_from_loader(loader.name, loader)
dash = importlib.util.module_from_spec(spec)
loader.exec_module(dash)
import extension_launch as el
registry = json.load(open(os.path.join(repo, 'config/providers.json')))
dash.load_provider_registry = lambda: registry
dash.cc_usage.record_change = lambda *a, **k: None
out = []
for call in json.load(open(case_path)):
    if call.get('port') is not None:
        dash.load_proxy_cfg = lambda port=call['port']: {'port': port}
    if 'repo' in call and dash.REPO_ROOT != call['repo']:
        dash.REPO_ROOT = call['repo']
        dash._DIALOG_CACHE.update(mtime=None, value=None)
    try:
        out.append({'ok': eval(call['expr'], {'dash': dash, 'el': el, 'json': json, 'os': os})})
    except Exception as e:
        out.append({'error': str(e), 'value': True} if isinstance(e, ValueError) else {'other': True})
print(json.dumps(out))
"#;

pub struct Oracle<'a> {
    home: &'a Home,
    python: PathBuf,
}

impl<'a> Oracle<'a> {
    /// `None` (y un aviso) sin `python3` ni `python3.11`.
    pub fn new(home: &'a Home) -> Option<Self> {
        let python = PathBuf::from("/usr/bin/python3");
        if !python.exists() || !home.bin().join("python3.11").exists() {
            eprintln!("python3/python3.11 no están instalados: se salta el oráculo");
            return None;
        }
        Some(Self { home, python })
    }

    /// Evalúa cada `{"expr", "port"?, "repo"?}` en el oráculo.
    pub fn run(&self, calls: &[Value], extra_env: &[(&str, &str)]) -> Vec<Value> {
        let dir = self.home.path().join(".oracle");
        let case = dir.join(format!(
            "case-{}.json",
            comandos_runtime::fresh_id("c").unwrap()
        ));
        fs::write(&case, serde_json::to_string(calls).unwrap()).unwrap();
        let mut command = Command::new(&self.python);
        command
            .env_clear()
            .arg("-c")
            .arg(SCRIPT)
            .arg(repo())
            .arg(&case)
            .current_dir(repo());
        for (k, v) in oracle_environ(self.home) {
            command.env(k, v);
        }
        for (k, v) in extra_env {
            command.env(k, v);
        }
        let out = command.output().unwrap();
        let _ = fs::remove_file(&case);
        assert!(
            out.status.success(),
            "oráculo: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8(out.stdout).unwrap();
        let line = stdout.lines().last().unwrap_or("[]");
        match comandos_core::json::workspace_loads(line).unwrap() {
            Value::Array(items) => items,
            other => panic!("oráculo: {other}"),
        }
    }
}

//! Ayudas de prueba del corte `ops` (2f-2): operaciones de sesión.
//!
//! `FakeCodex` es el Codex falso de `tests/test_session_tmux.py` (el mismo
//! stub en C que `crates/comandos-runtime/tests/support/ops_lab.rs`):
//! compilado con `cc` dentro del HOME temporal, sin red; escribe su rollout y
//! espera. `seed_registry` deja las casas y raíces de cuentas bajo el HOME
//! temporal, como la fixture del Python.
#![allow(dead_code)]
use super::TestHome;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const STUB: &str = r#"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
int main(int argc, char **argv) {
  const char *sid="11111111-1111-1111-1111-111111111111", *model="gpt-5.5";
  char effort[40]="high", path[4096];
  for (int i=1; i+1<argc; i++) {
    if (!strcmp(argv[i],"resume")) sid=argv[++i];
    else if (!strcmp(argv[i],"-m")) model=argv[++i];
    else if (!strcmp(argv[i],"-c")) sscanf(argv[++i],"model_reasoning_effort=\"%39[^\"]",effort);
  }
  if (!strcmp(model,"gpt-5.6-luna")) return 23;
  const char *account_home=getenv("CODEX_HOME");
  if (account_home) snprintf(path,sizeof(path),"%s/sessions/rollout-test-%s.jsonl",account_home,sid);
  else snprintf(path,sizeof(path),"%s/.codex/sessions/rollout-test-%s.jsonl",getenv("HOME"),sid);
  FILE *f=fopen(path,"a+"); if (!f) return 24;
  fseek(f,0,SEEK_END);
  if (!ftell(f)) fprintf(f,"{\"type\":\"session_meta\",\"payload\":{\"id\":\"%s\",\"source\":\"cli\"}}\n",sid);
  fprintf(f,"{\"type\":\"turn_context\",\"payload\":{\"model\":\"%s\",\"effort\":\"%s\"}}\n",model,effort);
  fflush(f);
  const char *operation=getenv("COMANDOS_OPERATION_ID");
  if (operation && !strcmp(operation,"slow-redraw-operation")) sleep(3);
  printf("\033[2J\033[H> %s %s\n",model,effort); fflush(stdout);
  for (;;) pause();
}
"#;

pub struct FakeCodex {
    pub binary: PathBuf,
}

/// Install the real native executor only into this test's owned HOME. The
/// binary selection matches the runtime operation fixtures.
pub fn install_extension_launcher(home: &TestHome) {
    let native = std::env::var_os("COMANDOS_EXTENSION_SESSION_NATIVE_FIXTURE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_exe()
                .unwrap()
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("comandos")
        });
    assert!(
        native.is_file(),
        "build comandos-cli first or provide COMANDOS_EXTENSION_SESSION_NATIVE_FIXTURE for the private extension fixture"
    );
    let installed = home.root.join(".local/share/comandos/bin/comandos");
    let alias = comandos_runtime::extension_launch::helper_for_home(&home.root);
    std::fs::create_dir_all(installed.parent().unwrap()).unwrap();
    std::fs::create_dir_all(alias.parent().unwrap()).unwrap();
    std::fs::copy(native, &installed).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&installed, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::os::unix::fs::symlink(installed, &alias).unwrap();
    comandos_runtime::extension_launch::require_helper(&home.root).unwrap();
}

impl FakeCodex {
    /// Compila el stub como `<dir>/codex`; `None` (con aviso) sin `cc`.
    pub fn build(dir: &Path) -> Option<Self> {
        let ok = Command::new("cc")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if !ok {
            eprintln!("sin compilador de C: se salta la prueba con el Codex falso");
            return None;
        }
        std::fs::create_dir_all(dir).unwrap();
        let source = dir.join("codex-stub.c");
        std::fs::write(&source, STUB).unwrap();
        let binary = dir.join("codex");
        let built = Command::new("cc")
            .arg(&source)
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            built.status.success(),
            "{}",
            String::from_utf8_lossy(&built.stderr)
        );
        Some(Self { binary })
    }
}

/// `config/providers.json` con `defaultHome`/`accountsRoot` de cada harness con
/// cuentas bajo `home`, y las cuentas `main` y `work` de Codex con login falso.
pub fn seed_registry(home: &Path) -> Value {
    let text = std::fs::read_to_string(super::repo().join("config/providers.json")).unwrap();
    let mut registry = comandos_core::json::workspace_loads(&text).unwrap();
    for (name, spec) in registry["harnesses"].as_object_mut().unwrap().iter_mut() {
        if spec["capabilities"]["accounts"].as_bool() == Some(true) {
            let spec = spec.as_object_mut().unwrap();
            spec.insert(
                "defaultHome".into(),
                json!(home.join(format!(".{name}")).display().to_string()),
            );
            spec.insert(
                "accountsRoot".into(),
                json!(home.join(format!(".{name}-accounts")).display().to_string()),
            );
        }
    }
    for dir in [home.join(".codex"), home.join(".codex-accounts/work")] {
        std::fs::create_dir_all(dir.join("sessions")).unwrap();
        std::fs::write(
            dir.join("auth.json"),
            r#"{"tokens": {"access_token": "fixture"}}"#,
        )
        .unwrap();
    }
    registry
}

/// Otro HOME temporal con los mismos archivos de `~/.claude/hooks`.
pub fn clone_home(home: &TestHome, tag: &str) -> TestHome {
    let twin = TestHome::new(tag);
    for entry in std::fs::read_dir(home.hooks()).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_file() {
            std::fs::copy(entry.path(), twin.hooks().join(entry.file_name())).unwrap();
        }
    }
    twin
}

/// Semilla confinada para las rutas: dos cuentas falsas y un pane privado
/// ejecutando el stub; send-keys nunca sale del socket de TestHome.
pub fn seed_fake_codex(home: &TestHome) {
    let _ = seed_registry(&home.root);
    FakeCodex::build(&home.root.join("bin")).expect("compilador C para FakeCodex");
    super::run_tmux(
        home,
        &[
            "new-session",
            "-d",
            "-s",
            "audit",
            "-c",
            home.root.to_str().unwrap(),
            "/bin/sh",
        ],
    );
    let line = format!(
        "{} --sandbox read-only --ask-for-approval untrusted -m gpt-5.5 -c 'model_reasoning_effort=\"high\"'",
        home.root.join("bin/codex").display()
    );
    super::run_tmux(home, &["send-keys", "-t", "%0", "-l", "--", &line]);
    super::run_tmux(home, &["send-keys", "-t", "%0", "Enter"]);
    let rollout = home
        .root
        .join(".codex/sessions/rollout-test-11111111-1111-1111-1111-111111111111.jsonl");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !rollout.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(rollout.exists(), "FakeCodex no arrancó");
}

/// Estado final de cada lado: no compara pids, snapshots ni relojes.
pub fn journal_summary(home: &TestHome) -> Vec<Value> {
    let conn = rusqlite::Connection::open(home.journal_db()).unwrap();
    let mut stmt = conn
        .prepare("SELECT id,state,result FROM session_operations ORDER BY id")
        .unwrap();
    let rows:Vec<Value>=stmt.query_map([], |r| {
        let id:String=r.get(0)?; let state:String=r.get(1)?; let result:Option<String>=r.get(2)?;
        Ok(json!({"id":id,"state":state,"result":result.map(|s|serde_json::from_str::<Value>(&s).unwrap())}))
    }).unwrap().map(Result::unwrap).collect();
    fn clean(v: &mut Value, home: &str) {
        match v {
            Value::Object(map) => {
                map.retain(|k, _| {
                    !["pid", "identity", "observedAt", "evidenceAt", "ts"].contains(&k.as_str())
                });
                for v in map.values_mut() {
                    clean(v, home);
                }
            }
            Value::Array(list) => {
                for v in list {
                    clean(v, home);
                }
            }
            Value::String(s) => *s = s.replace(home, "<HOME>"),
            _ => {}
        }
    }
    let mut rows = rows;
    for row in &mut rows {
        clean(row, home.root.to_str().unwrap());
    }
    rows
}

pub async fn wait_operations_idle(t: &super::twin::Twin) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(45);
    loop {
        let states = [t.get("/model/status?operationKey=audit%7C%250").await];
        let done = states.iter().all(|run| {
            [&run.front, &run.oracle].iter().all(|w| {
                let v: Value = serde_json::from_str(&w.text()).unwrap();
                v.get("state").and_then(Value::as_str).is_some_and(|s| {
                    ["confirmed", "failed", "rolled_back", "recovery_required"].contains(&s)
                })
            })
        });
        if done {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "operación pendiente más de 45 s: front={} oracle={}",
            states[0].front.text(),
            states[0].oracle.text()
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

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

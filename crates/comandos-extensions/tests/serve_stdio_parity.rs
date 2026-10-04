//! `serve` de un servidor stdio debe reemplazar el proceso por el comando del catálogo
//! (exec, sin proceso intermedio), igual que `os.execvpe` del proxy Python.
#[allow(dead_code)] // cada prueba usa solo parte de las utilidades compartidas
mod support;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const CATALOG: &str = r#"{"version":1,"servers":{
 "eco":{"enabled":true,"command":"~/bin/env_dump","args":["a","$HOME","~/x"],
  "env":{"COMANDOS_A":"${HOME}/m","COMANDOS_B":"$HOME/x/$COMANDOS_UNSET_ZZ/${COMANDOS_UNSET_ZZ}","COMANDOS_N":42,"COMANDOS_T":true,"COMANDOS_F":1.5,"COMANDOS_INHERITED":"pisado-por-el-catalogo-no"},
  "cwd":"~/work"},
 "plain":{"enabled":true,"command":"env_dump_in_path"},
 "missing":{"enabled":true,"command":"/nonexistent/xyz","args":["q"]},
 "off":{"enabled":false,"command":"env_dump_in_path"}}}"#;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn prepare_home(tag: &str) -> PathBuf {
    let home = std::env::temp_dir().join(format!("comandos-stdio-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(home.join(".config/comandos/extensions")).unwrap();
    fs::create_dir_all(home.join("bin")).unwrap();
    fs::create_dir_all(home.join("work")).unwrap();
    let dump = PathBuf::from(env!("CARGO_BIN_EXE_env_dump"));
    std::os::unix::fs::symlink(&dump, home.join("bin/env_dump")).unwrap();
    // Copia con otro nombre para probar la búsqueda en PATH y un argv[0] distinto.
    std::os::unix::fs::symlink(&dump, home.join("bin/env_dump_in_path")).unwrap();
    fs::write(
        home.join(".config/comandos/extensions/catalog.json"),
        CATALOG,
    )
    .unwrap();
    support::link_oracle_venv(&home);
    home
}

fn python(home: &Path, args: &[&str]) -> Output {
    Command::new("python3.11")
        .arg(root().join("bin/cc-extensions"))
        .args(args)
        .current_dir("/")
        .env("HOME", home)
        .env("COMANDOS_INHERITED", "padre")
        .env("PATH", format!("{}/bin:/usr/bin:/bin", home.display()))
        .output()
        .unwrap()
}

fn rust(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_comandos-extensions"))
        .args(args)
        .current_dir("/")
        .env("HOME", home)
        .env("COMANDOS_INHERITED", "padre")
        .env("PATH", format!("{}/bin:/usr/bin:/bin", home.display()))
        .output()
        .unwrap()
}

/// Separa el volcado (idéntico entre proxies) de la línea `pid=N`.
fn split_pid(out: &Output) -> (String, u32) {
    let text = String::from_utf8(out.stdout.clone()).unwrap();
    let (dump, pid) = text.rsplit_once("pid=").expect("falta pid");
    (dump.to_owned(), pid.trim().parse().unwrap())
}

#[test]
fn stdio_serve_execs_catalog_command_like_python() {
    let home = prepare_home("exec");
    for name in ["eco", "plain"] {
        let (r, p) = (
            rust(&home, &["serve", name]),
            python(&home, &["serve", name]),
        );
        assert!(
            r.status.success() && p.status.success(),
            "{name}: {r:?} {p:?}"
        );
        let ((rd, _), (pd, _)) = (split_pid(&r), split_pid(&p));
        assert_eq!(rd, pd, "{name}: volcado distinto");
        assert!(r.stderr.is_empty(), "{name} rust stderr: {:?}", r.stderr);
        assert!(p.stderr.is_empty(), "{name} python stderr: {:?}", p.stderr);
        if name == "eco" {
            let v: serde_json::Value = serde_json::from_str(rd.trim()).unwrap();
            assert_eq!(v["cwd"], format!("{}/work", home.display()));
            assert_eq!(v["env"]["COMANDOS_INHERITED"], "pisado-por-el-catalogo-no");
            assert_eq!(v["env"]["COMANDOS_A"], format!("{}/m", home.display()));
            assert_eq!(v["argv"][0], format!("{}/bin/env_dump", home.display()));
        }
    }
    // El entorno del padre se hereda cuando el catálogo no lo pisa.
    let r = rust(&home, &["serve", "plain"]);
    assert!(
        String::from_utf8_lossy(&r.stdout).contains(r#""COMANDOS_INHERITED":"padre""#),
        "{r:?}"
    );
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn stdio_serve_is_exec_not_a_wrapper() {
    let home = prepare_home("pid");
    let child = Command::new(env!("CARGO_BIN_EXE_comandos-extensions"))
        .args(["serve", "plain"])
        .current_dir("/")
        .env("HOME", &home)
        .env("PATH", format!("{}/bin:/usr/bin:/bin", home.display()))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let spawned = child.id();
    let out = child.wait_with_output().unwrap();
    let (_, reported) = split_pid(&out);
    assert_eq!(reported, spawned, "hay un proceso intermedio");
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn stdio_serve_missing_command_fails_like_python() {
    let home = prepare_home("missing");
    let (r, p) = (
        rust(&home, &["serve", "missing"]),
        python(&home, &["serve", "missing"]),
    );
    assert_eq!(r.status.code(), p.status.code(), "{r:?} {p:?}");
    assert_eq!(r.stdout, p.stdout);
    assert!(r.stderr.is_empty() && p.stderr.is_empty(), "{r:?} {p:?}");
    for name in ["off", "nope"] {
        let (r, p) = (
            rust(&home, &["serve", name]),
            python(&home, &["serve", name]),
        );
        assert_eq!(r.status.code(), p.status.code(), "{name}: {r:?} {p:?}");
        assert_eq!(r.stderr, p.stderr, "{name}");
    }
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn status_matches_python() {
    let home = prepare_home("status");
    let (r, p) = (rust(&home, &["status"]), python(&home, &["status"]));
    assert_eq!(
        String::from_utf8_lossy(&r.stdout),
        String::from_utf8_lossy(&p.stdout)
    );
    assert_eq!(r.status.code(), p.status.code());
    let _ = fs::remove_dir_all(&home);
}

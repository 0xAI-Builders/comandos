//! El gemelo (Tarea 2 del maestro 2f, R1 y B3 del pre-flight) compara
//! mutaciones sobre dos HOME sembrados igual. Además de la normalización, aquí
//! viven los «canarios» del confinamiento: si alguno falla, ninguna prueba con
//! gemelo es segura de ejecutar.
//!
//! Confinamiento: cada lado tiene su tmux privado (`-f /dev/null -S <socket>`
//! bajo un HOME temporal corto) al que solo se llega por el guardián del
//! `fakebin`, que limpia el entorno (`env -i`); el `PATH` de los paneles y del
//! oráculo es SOLO el `fakebin` (falsos que anotan + una lista corta de
//! herramientas inocuas); el `systemd-run` falso solo ejecuta colas de tmux; el
//! oráculo arranca sin sus bucles de fondo y con la red cerrada salvo los
//! puertos de la propia prueba. Ninguna prueba ejecuta `kill-*`: los servidores
//! privados mueren en el `Drop` de `TestHome`, siempre con su `-S`.
mod support;

use comandos_runtime::providers;
use comandos_server::dash::native::procs::{gui_env_for, spawn_detached};
use std::{
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use support::{
    TestHome,
    frozen_twin::{Twin, TwinOpts, normalize},
    oracle::{FakeCall, OracleOpts, fake_calls, tmux_guard, write_executable},
};

#[test]
fn twin_normalizes_clock_but_not_content() {
    let a = r#"{"session": "term-r12345", "ts": 1791115200.123, "label": "x"}"#;
    let b = r#"{"session": "term-r12399", "ts": 1791115201.9, "label": "x"}"#;
    assert_eq!(normalize(a), normalize(b));
    let c = r#"{"session": "term-r12399", "ts": 1791115201.9, "label": "y"}"#;
    assert_ne!(normalize(a), normalize(c));
    assert_eq!(
        normalize(r#"{"closedAt": 1.5, "name": "1791115200123456789-0f3a.json"}"#),
        normalize(r#"{"closedAt": 2.5, "name": "1791115299123456789-aaaa.json"}"#)
    );
}

#[test]
fn tmux_guard_refuses_missing_socket_dir() {
    let dir = TempDir::new("cmd-guard");
    let dir = dir.0.as_path();
    // El directorio del socket NO existe: el guardián debe negarse sin lanzar
    // tmux (un tmux real con `-S` daría «no server running», no 97). El
    // «tmux real» del guardián es además un programa que deja una marca: si se
    // ejecutara, la marca aparecería.
    let marker = dir.join("lanzado");
    let real = dir.join("tmux-real");
    write_executable(&real, &format!("#!/bin/sh\ntouch '{}'\n", marker.display()));
    let socket = dir.join("no-existe/tmux-1000/default");
    let guard = dir.join("tmux");
    write_executable(&guard, &tmux_guard(&real, &socket));
    let out = Command::new(&guard)
        .arg("list-sessions")
        .env_remove("TMUX")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(97), "{out:?}");
    assert!(
        !marker.exists(),
        "el guardián lanzó tmux sin socket privado"
    );
}

/// Directorio temporal que se borra aunque la prueba falle (no tiene tmux).
struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Canario del oráculo: la red sale cerrada (4778/4779/4780, DNS, sockets Unix
/// fuera del HOME) salvo los puertos de la prueba, sin `DISPLAY` ni DBus, con
/// HOME temporal y `PATH` = solo el `fakebin`.
#[test]
fn canary_confined_python_cannot_reach_live_services() {
    let home = TestHome::new_short("canary-py");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let allowed = listener.local_addr().unwrap().port();
    let code = r#"
import os, socket, urllib.request, urllib.error, json
out = {}
def attempt(name, fn):
    try:
        fn()
        out[name] = "abierto"
    except (OSError, urllib.error.URLError) as exc:
        out[name] = "cerrado"
for port in (4777, 4778, 4779, 4780, 4781, 4782):
    attempt(f"tcp-{port}", lambda port=port: socket.create_connection(("127.0.0.1", port), timeout=1).close())
# Solo con el puerto ya cerrado a nivel de socket: un canario no debe poder
# mandar un aviso real si el confinamiento estuviera roto.
if out["tcp-4778"] == "cerrado":
    attempt("urlopen-4778", lambda: urllib.request.urlopen("http://127.0.0.1:4778/", timeout=1))
if out["tcp-4780"] == "cerrado":
    attempt("urlopen-4780", lambda: urllib.request.urlopen("http://127.0.0.1:4780/", timeout=1))
attempt("dns", lambda: socket.getaddrinfo("example.com", 443))
attempt("udp", lambda: socket.socket(socket.AF_INET, socket.SOCK_DGRAM).sendto(b"x", ("1.1.1.1", 53)))
attempt("unix-runtime", lambda: socket.socket(socket.AF_UNIX).connect("/run/user/%d/bus" % os.getuid()))
attempt("permitido", lambda: socket.create_connection(("127.0.0.1", int(os.environ["CANARY_PORT"])), timeout=1).close())
out["env"] = {k: os.environ.get(k) for k in ("DISPLAY", "WAYLAND_DISPLAY", "DBUS_SESSION_BUS_ADDRESS", "TMUX", "HOME", "PATH", "SHELL", "CLAUDE_CONFIG_DIR")}
out["loops"] = [getattr(dash, n).__name__ for n in ("_model_watch_loop", "_news_editions_loop", "_notices_push_loop", "_limits_snapshot_loop")]
print(json.dumps(out))
"#;
    let opts = OracleOpts {
        allow_ports: vec![allowed],
        extra_env: vec![("CANARY_PORT".into(), allowed.to_string())],
        ..OracleOpts::default()
    };
    let roots = [("<HOME>", home.root.as_path())];
    let input = serde_json::json!({"source_commit":support::frozen::SOURCE_COMMIT,
        "source_sha256":"4e4e26305485b4926bd2c77618a4a68eb8da9ea425825c57a9a0fea6847a6f24",
        "python":"CPython 3.10.12", "code":code,"allowed_ports":["private fixture listener"],"CANARY_PORT":"private fixture listener"});
    let bytes = comandos_oracle::oracle_at(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden"),
        "server-twin-network-canary",
        &input,
        || {
            Ok(comandos_oracle::normalize(
                support::frozen::run_dash_original(&home, code, &opts)?.as_bytes(),
                &roots,
            ))
        },
    );
    let text = String::from_utf8(comandos_oracle::restore(&bytes, &roots)).unwrap();
    let out: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
    for name in [
        "tcp-4777",
        "tcp-4778",
        "tcp-4779",
        "tcp-4780",
        "tcp-4781",
        "tcp-4782",
        "urlopen-4778",
        "urlopen-4780",
        "dns",
        "udp",
        "unix-runtime",
    ] {
        assert_eq!(
            out[name], "cerrado",
            "{name} salió del confinamiento: {out}"
        );
    }
    assert_eq!(out["permitido"], "abierto", "{out}");
    let env = &out["env"];
    for key in [
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "DBUS_SESSION_BUS_ADDRESS",
        "TMUX",
        "CLAUDE_CONFIG_DIR",
    ] {
        assert!(env[key].is_null(), "{key} llegó al oráculo: {env}");
    }
    assert_eq!(env["HOME"], home.root.display().to_string());
    assert_eq!(env["PATH"], home.root.join("fakebin").display().to_string());
    assert_eq!(env["SHELL"], "/bin/bash");
    for name in out["loops"].as_array().unwrap() {
        assert_eq!(name, "_twin_loop_off", "bucle de fondo encendido: {out}");
    }
    drop(listener);
}

/// Canario de los binarios reales: un panel nacido del guardián (aunque quien
/// lo llame traiga `DISPLAY`, el HOME real y el `PATH` real) solo ve el
/// `fakebin`: agentes, terminales, `ssh-copy-id`, `git` y `pkill` son falsos
/// que anotan, y lo que no está en la lista (`curl`) no existe.
#[test]
fn canary_panes_only_see_fakes() {
    if !support::tmux_available() {
        eprintln!("tmux no está instalado: se salta");
        return;
    }
    let home = TestHome::new_short("canary-pane");
    support::oracle::confined_fakebin(&home, &[]);
    let out = home.root.join("pane.out");
    // Cada programa solo se ejecuta si resuelve dentro del `fakebin`: un
    // canario roto nunca lanza el `claude` o el `ssh-copy-id` reales.
    let fakebin = home.root.join("fakebin");
    let script = format!(
        "{{ env; for p in claude codex grok ssh-copy-id pkill git tilix kitty \
         gnome-terminal google-chrome xdg-open tailscale systemctl curl wget nc; do \
         printf 'which %s=%s\\n' \"$p\" \"$(command -v \"$p\")\"; done; \
         fake() {{ case \"$(command -v \"$1\")\" in '{fb}'/*) \"$@\";; esac; }}; \
         fake claude auth login; fake ssh-copy-id host; fake pkill -f x; fake git status; \
         [ -z \"$(command -v curl)\" ] && {{ curl; echo \"curl=$?\"; }}; echo FIN; }} > '{out}' 2>&1",
        fb = fakebin.display(),
        out = out.display()
    );
    // El guardián llamado con un entorno hostil, como lo heredaría el frente.
    let guard = home.root.join("fakebin/tmux");
    let status = Command::new(&guard)
        .args(["new-session", "-d", "-s", "canario", &script])
        .env("DISPLAY", ":99")
        .env("WAYLAND_DISPLAY", "wayland-99")
        .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/no-existe/bus")
        .env("HOME", "/home/no-es-la-prueba")
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .env("SHELL", "/usr/bin/zsh")
        .env("CLAUDE_CONFIG_DIR", "/no-existe/claude")
        .env_remove("TMUX")
        .stdin(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
    let text = wait_for(&out, "FIN");
    for line in text.lines() {
        assert!(
            !line.starts_with("DISPLAY=")
                && !line.starts_with("WAYLAND_DISPLAY=")
                && !line.starts_with("DBUS_SESSION_BUS_ADDRESS=")
                && !line.starts_with("CLAUDE_CONFIG_DIR="),
            "variable de escritorio en el panel: {line}"
        );
    }
    assert!(
        text.contains(&format!("HOME={}\n", home.root.display())),
        "{text}"
    );
    assert!(text.contains("SHELL=/bin/bash\n"), "{text}");
    for p in [
        "claude",
        "codex",
        "grok",
        "ssh-copy-id",
        "pkill",
        "git",
        "tilix",
        "kitty",
        "gnome-terminal",
        "google-chrome",
        "xdg-open",
        "tailscale",
        "systemctl",
    ] {
        let want = format!("which {p}={}/{p}\n", fakebin.display());
        assert!(text.contains(&want), "{p} no es el falso: {text}");
    }
    for p in ["curl", "wget", "nc"] {
        assert!(
            text.contains(&format!("which {p}=\n")),
            "{p} existe: {text}"
        );
    }
    assert!(text.contains("curl=127"), "{text}");
    let calls = fake_calls(&home.root);
    let names: Vec<&str> = calls.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names,
        ["claude", "ssh-copy-id", "pkill", "git"],
        "{calls:?}"
    );
    assert_eq!(
        calls.first(),
        Some(&FakeCall {
            name: "claude".into(),
            args: vec!["auth".into(), "login".into()]
        })
    );
}

/// Canario del `systemd-run` falso: solo ejecuta colas de tmux (con las
/// banderas de `scope_cmd`); una terminal (`spawn_terminal`) se anota y no corre.
#[test]
fn canary_fake_scope_only_runs_tmux() {
    if !support::tmux_available() {
        eprintln!("tmux no está instalado: se salta");
        return;
    }
    let home = TestHome::new_short("canary-scope");
    support::oracle::confined_fakebin(&home, &[]);
    let scope = home.root.join("fakebin/systemd-run");
    let marker = home.root.join("terminal-lanzada");
    let terminal = Command::new(&scope)
        .args(["--user", "--collect", "--quiet", "sh", "-c"])
        .arg(format!("touch '{}'", marker.display()))
        .status()
        .unwrap();
    assert!(terminal.success());
    let unflagged = Command::new(&scope)
        .args(["--user", "tmux", "new-session", "-d", "-s", "sin-banderas"])
        .status()
        .unwrap();
    assert_eq!(unflagged.code(), Some(97));
    let ok = Command::new(&scope)
        .args([
            "--user",
            "--scope",
            "--collect",
            "--quiet",
            "tmux",
            "new-session",
            "-d",
            "-s",
            "con-scope",
            "sleep 30",
        ])
        .status()
        .unwrap();
    assert!(ok.success());
    std::thread::sleep(Duration::from_millis(200));
    assert!(
        !marker.exists(),
        "el systemd-run falso ejecutó una terminal"
    );
    let sessions = support::run_tmux(&home, &["list-sessions", "-F", "#{session_name}"]);
    assert_eq!(sessions.trim(), "con-scope");
    assert_eq!(support::fake_scope_calls(&home).len(), 3);
}

/// El gemelo entero: oráculo confinado (marca del prólogo, entorno del
/// proceso), frente con el guardián como tmux y archivos comparados tras
/// normalizar.
#[tokio::test]
async fn twin_starts_confined_and_compares_files() {
    let seeded = |home: &TestHome| {
        home.write("app-tabs-meta.json", r#"{"ts": 1791115200.5}"#);
    };
    let Some(twin) = Twin::start_with(
        "base",
        seeded,
        TwinOpts {
            fakebin_extra: vec![("mi-falso".into(), "#!/bin/sh\nexit 3\n".into())],
            front: Some(Box::new(|opts| opts.desktop_device = "gemelo".into())),
            ..TwinOpts::default()
        },
    )
    .await
    else {
        return;
    };
    // Original guard is an immutable expectation. Listener ownership and
    // process environment remain actual queries against our Rust child in replay.
    let marker = String::from_utf8(twin.original_file(".comandos-twin-guard").unwrap()).unwrap();
    assert!(marker.contains("red cerrada"), "{marker}");
    let rust_actor = if twin.source_pid().is_none() {
        Some(support::owned_actor::OwnedActor::start(&twin.b))
    } else {
        None
    };
    let pid = twin
        .source_pid()
        .unwrap_or_else(|| rust_actor.as_ref().unwrap().pid());
    let environ = std::fs::read(format!("/proc/{pid}/environ")).unwrap();
    let vars: Vec<String> = environ
        .split(|b| *b == 0)
        .map(|v| String::from_utf8_lossy(v).into_owned())
        .collect();
    for key in [
        "DISPLAY=",
        "WAYLAND_DISPLAY=",
        "DBUS_SESSION_BUS_ADDRESS=",
        "TMUX=",
    ] {
        assert!(
            !vars.iter().any(|v| v.starts_with(key)),
            "{key} en el oráculo"
        );
    }
    assert!(vars.contains(&format!("HOME={}", twin.b.root.display())));
    // Frente: tmux = guardián del `fakebin` de A, con `-S` en el prefijo.
    assert_eq!(
        twin.front_options.tmux.program.path,
        twin.a.root.join("fakebin/tmux")
    );
    support::assert_private_tmux(&twin.front_options);
    // Respaldos de `providers::which`: solo directorios del HOME de A.
    assert_eq!(
        twin.front_options.user_bin_dirs,
        support::twin::home_bin_dirs()
    );
    assert!(
        twin.front_options
            .user_bin_dirs
            .iter()
            .all(|d| d.starts_with("~/")),
        "{:?}",
        twin.front_options.user_bin_dirs
    );
    // El oráculo importa el `sitecustomize.py` del confinamiento.
    if twin.source_pid().is_some() {
        assert!(vars.iter().any(|v| v.starts_with("PYTHONPATH=")));
    } else {
        assert!(
            !vars.iter().any(|v| v.starts_with("PYTHONPATH=")),
            "Rust replay actor has no Python dependency"
        );
    }
    assert_eq!(twin.front_options.desktop_device, "gemelo");
    assert!(twin.a.root.join("fakebin/mi-falso").exists());
    // Los dos lados responden.
    let run = twin.get("/tabs").await;
    assert_eq!((run.front.status, run.oracle.status), (200, 200));
    // Archivos: iguales tras normalizar el reloj; una diferencia real se ve.
    twin.a
        .write("app-tabs-meta.json", r#"{"ts": 1791115299.75}"#);
    assert_eq!(
        twin.files_equal(&["app-tabs-meta.json", "no-existe.json"]),
        Ok(())
    );
    twin.a.write("app-tabs-meta.json", r#"{"ts": 1, "x": 1}"#);
    assert!(twin.files_equal(&["app-tabs-meta.json"]).is_err());
    // El tmux de cada lado es privado y distinto.
    twin.tmux_a(&["new-session", "-d", "-s", "solo-a", "sleep 30"]);
    assert!(has_session(&twin.a, "solo-a"));
    assert!(!has_session(&twin.b, "solo-a"));
}

fn has_session(home: &TestHome, name: &str) -> bool {
    home.tmux_command()
        .args(["has-session", "-t", &format!("={name}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Espera (5 s) a que `path` contenga `needle` y devuelve su texto.
fn wait_for(path: &Path, needle: &str) -> String {
    let started = Instant::now();
    loop {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        if text.contains(needle) || started.elapsed() > Duration::from_secs(5) {
            return text;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn run_dash_loads_the_module() {
    let home = TestHome::new_short("run-dash");
    let text = support::http_golden::dash_files(
        &home,
        "server-twin-module-load",
        &[],
        "print(callable(dash.scope_cmd))",
        &Default::default(),
    );
    assert_eq!(text.trim(), "True");
}

/// Canario de C1 (revisión de la Tarea 2): lo que el frente lanza fuera de
/// tmux (`NativeOptions::program` + `spawn_detached`) solo ve el entorno
/// confinado de A: HOME temporal, `PATH` = su `fakebin`, sin escritorio, DBus
/// ni `CLAUDE_CONFIG_DIR`; `gui_env_for` trata `DISPLAY` como ausente.
#[tokio::test]
async fn canary_front_children_get_confined_env() {
    let Some(twin) = Twin::start("canary-child", |_| {}).await else {
        return;
    };
    let opts = &twin.front_options;
    let fakebin = twin.a.root.join("fakebin");
    let out = twin.a.root.join("child.env");
    let sh = opts.program(fakebin.join("sh"));
    spawn_detached(
        &sh,
        &[
            "-c".into(),
            format!("env > '{}'; echo FIN >> '{}'", out.display(), out.display()).into(),
        ],
        &[],
    )
    .unwrap();
    let text = wait_for(&out, "FIN");
    let env: Vec<&str> = text.lines().collect();
    assert!(
        env.contains(&format!("HOME={}", twin.a.root.display()).as_str()),
        "{text}"
    );
    assert!(
        env.contains(&format!("PATH={}", fakebin.display()).as_str()),
        "{text}"
    );
    for key in [
        "DISPLAY=",
        "WAYLAND_DISPLAY=",
        "DBUS_SESSION_BUS_ADDRESS=",
        "CLAUDE_CONFIG_DIR=",
        "TMUX=",
    ] {
        assert!(
            !env.iter().any(|l| l.starts_with(key)),
            "{key} llegó al hijo: {text}"
        );
    }
    assert_eq!(
        gui_env_for(opts),
        vec![(
            std::ffi::OsString::from("DISPLAY"),
            std::ffi::OsString::from(":1")
        )]
    );
    assert_eq!(opts.ssh.path, fakebin.join("ssh"));
    assert!(opts.ssh.env_clear);
    // Producción no cambia: sin `child_env` el programa hereda el entorno.
    let mut production = opts.clone();
    production.child_env = None;
    let plain = production.program("/bin/sh");
    assert!(!plain.env_clear && plain.env.is_empty());
}

/// Los binarios que nombra `config/providers.json` (arneses, `command[0]` de
/// ACP y `requires`) más `node`/`npx`/`bun`.
fn registry_binaries() -> Vec<String> {
    let text = std::fs::read_to_string(support::repo().join("config/providers.json")).unwrap();
    let registry: serde_json::Value = serde_json::from_str(&text).unwrap();
    let mut names: Vec<String> = ["node", "npx", "bun"]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    let mut add = |v: &serde_json::Value| {
        if let Some(name) = v.as_str().filter(|n| !n.is_empty())
            && !names.iter().any(|n| n == name)
        {
            names.push(name.to_owned());
        }
    };
    for harness in registry["harnesses"].as_object().unwrap().values() {
        add(&harness["binary"]);
    }
    for agent in registry["acpAgents"].as_object().unwrap().values() {
        add(&agent["command"][0]);
        for need in agent["requires"].as_array().into_iter().flatten() {
            add(need);
        }
    }
    names
}

/// Canario de I1: `lib/providers.py` y `lib/acp.py` caen en `/usr/local/bin`
/// (y en los bins de usuario del HOME, aquí vacíos) si `PATH` no tiene el
/// nombre. Cada binario del registro tiene su falso en el `fakebin`, así que
/// el `PATH` confinado lo encuentra antes en los dos lados.
#[test]
fn canary_registry_binaries_resolve_to_fakes() {
    let home = TestHome::new_short("canary-reg");
    let fakebin = support::oracle::confined_fakebin(&home, &[]);
    let names = registry_binaries();
    for name in &names {
        let hit = providers::which(name, Some(fakebin.as_os_str()), &home.root)
            .unwrap_or_else(|| panic!("{name} no resuelve"));
        assert!(
            hit.starts_with(&fakebin),
            "Rust: {name} → {}",
            hit.display()
        );
    }
    let code = format!(
        "import json, shutil\nimport providers, acp\nnames = json.loads({names:?})\n\
         print(json.dumps([[n, shutil.which(n), providers.which(n), acp.which(n)] for n in names]))",
        names = serde_json::to_string(&names).unwrap()
    );
    let text = support::http_golden::dash_files(
        &home,
        "server-twin-registry-resolution",
        &[],
        &code,
        &Default::default(),
    );
    let rows: Vec<Vec<serde_json::Value>> = serde_json::from_str(text.trim()).unwrap();
    assert_eq!(rows.len(), names.len());
    for row in rows {
        for hit in &row[1..] {
            let hit = hit.as_str().unwrap_or("");
            assert!(
                hit.starts_with(&fakebin.display().to_string()),
                "Python: {row:?}"
            );
        }
    }
}

/// Canario menor de la revisión de la Tarea 2: un `new-session` sin orden abre
/// un shell de login, que añade directorios por `/etc/profile.d` (p. ej.
/// `/snap/bin`) al FINAL del `PATH`. El `fakebin` sigue primero y los agentes y
/// efectos resuelven a sus falsos.
#[test]
fn canary_bare_session_path_keeps_fakebin_first() {
    if !support::tmux_available() {
        eprintln!("tmux no está instalado: se salta");
        return;
    }
    let home = TestHome::new_short("canary-bare");
    let fakebin = support::oracle::confined_fakebin(&home, &[]);
    let out = home.root.join("bare.out");
    let guard = fakebin.join("tmux");
    let status = Command::new(&guard)
        .args(["new-session", "-d", "-s", "bare"])
        .env_remove("TMUX")
        .stdin(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
    let line = format!(
        "{{ printf 'PATH=%s\\n' \"$PATH\"; for n in claude codex node npx bun ssh-copy-id tilix; do \
         printf '%s=%s\\n' \"$n\" \"$(command -v $n)\"; done; echo FIN; }} > '{}'",
        out.display()
    );
    support::run_tmux(&home, &["send-keys", "-t", "=bare:", "-l", &line]);
    support::run_tmux(&home, &["send-keys", "-t", "=bare:", "Enter"]);
    let text = wait_for(&out, "FIN");
    let path = text
        .lines()
        .find_map(|l| l.strip_prefix("PATH="))
        .unwrap_or_else(|| panic!("{text}"));
    assert!(
        path.split(':').next() == Some(fakebin.to_str().unwrap()),
        "el fakebin no va primero: {path}"
    );
    for name in [
        "claude",
        "codex",
        "node",
        "npx",
        "bun",
        "ssh-copy-id",
        "tilix",
    ] {
        assert!(
            text.contains(&format!("{name}={}/{name}\n", fakebin.display())),
            "{name} no es el falso: {text}"
        );
    }
}

/// Canario de la revisión de la Tarea 3: la exclusión de `/usr/local/bin` es
/// absoluta. Un nombre real que NO está en el registro ni en el `fakebin`
/// (`ollama`, `ngrok`… de `/usr/local/bin` en esta máquina) no resuelve en
/// ningún sitio real: ni por el `PATH` confinado ni por los respaldos
/// `_USER_BIN_DIRS` de `lib/providers.py`/`lib/acp.py` (recortados por el
/// `sitecustomize.py` del confinamiento) ni por los del frente
/// (`NativeOptions::user_bin_dirs` del gemelo). Los respaldos del HOME siguen
/// vivos en los dos lados (`~/.local/bin`).
#[test]
fn canary_unlisted_names_resolve_nowhere_real() {
    let home = TestHome::new_short("canary-ulb");
    let fakebin = support::oracle::confined_fakebin(&home, &[]);
    let unlisted = ["ollama", "ngrok", "stripe", "circom", "android-studio"];
    let present: Vec<&str> = unlisted
        .iter()
        .copied()
        .filter(|n| Path::new("/usr/local/bin").join(n).is_file())
        .collect();
    if present.is_empty() {
        eprintln!("ninguno de {unlisted:?} está en /usr/local/bin: el canario solo mira la lista");
    }
    // Sin el recorte, el Rust de producción sí los encontraría (el canario mide algo).
    for name in &present {
        assert!(
            providers::which(name, Some(fakebin.as_os_str()), &home.root)
                .is_some_and(|p| p.starts_with("/usr/local/bin")),
            "{name}"
        );
    }
    let dirs = support::twin::home_bin_dirs();
    assert!(!dirs.iter().any(|d| d.starts_with('/')), "{dirs:?}");
    // Un ejecutable del HOME sigue resolviendo por el respaldo `~/.local/bin`.
    let local = home.root.join(".local/bin");
    std::fs::create_dir_all(&local).unwrap();
    write_executable(&local.join("casa-solo"), "#!/bin/sh\nexit 0\n");
    let mut names: Vec<&str> = unlisted.to_vec();
    names.push("casa-solo");
    let rust: Vec<serde_json::Value> = names
        .iter()
        .map(|n| {
            let hit = providers::which_in_dirs(n, Some(fakebin.as_os_str()), &home.root, &dirs);
            serde_json::json!([n, hit.map(|p| p.display().to_string())])
        })
        .collect();
    let casa = local.join("casa-solo").display().to_string();
    for row in &rust {
        let expected = if row[0] == "casa-solo" {
            serde_json::Value::from(casa.clone())
        } else {
            serde_json::Value::Null
        };
        assert_eq!(row[1], expected, "Rust: {row}");
    }
    let code = format!(
        "import json\nimport providers, acp\nnames = json.loads({names:?})\n\
         assert all(d.startswith('~') for d in providers._USER_BIN_DIRS + acp._USER_BIN_DIRS)\n\
         print(json.dumps([[n, providers.which(n), acp.which(n)] for n in names]))",
        names = serde_json::to_string(&names).unwrap()
    );
    let text = support::http_golden::dash_files(
        &home,
        "server-twin-registry-resolution",
        &[],
        &code,
        &Default::default(),
    );
    let rows: Vec<Vec<serde_json::Value>> = serde_json::from_str(text.trim()).unwrap();
    assert_eq!(rows.len(), names.len());
    for (row, rust) in rows.iter().zip(&rust) {
        assert_eq!(row[1], rust[1], "providers.which: {row:?}");
        assert_eq!(row[2], rust[1], "acp.which: {row:?}");
    }
}

/// Ronda 1 de la 2f-1/T2: un oráculo solo está «listo» cuando su PROPIO
/// proceso escucha en el puerto (`/proc/net/tcp` contra sus descriptores),
/// nunca porque algo acepte conexiones ahí (el oráculo de otra prueba).
#[tokio::test]
async fn oracle_ready_means_its_own_listener() {
    use support::oracle::listens_on;
    let home = TestHome::new_short("oracle-own");
    support::oracle::confined_fakebin(&home, &[]);
    let py = support::owned_actor::OwnedActor::start(&home);
    assert!(listens_on(py.pid(), py.port));
    assert!(!listens_on(std::process::id(), py.port));
    // Puerto de los oráculos: fuera del rango efímero de `dead_port()`.
    assert!((20_000..30_000).contains(&py.port), "{}", py.port);
    let mine = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = mine.local_addr().unwrap().port();
    assert!(listens_on(std::process::id(), port));
    assert!(
        !listens_on(py.pid(), port),
        "otro proceso en el puerto no es el oráculo"
    );
}

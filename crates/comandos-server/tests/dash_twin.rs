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

use std::{
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use support::{
    TestHome,
    oracle::{
        FakeCall, OracleOpts, fake_calls, run_dash, run_dash_with, tmux_guard, write_executable,
    },
    twin::{Twin, TwinOpts, normalize},
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
    let dir = std::env::temp_dir().join(format!("cmd-guard-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
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
    let _ = std::fs::remove_dir_all(&dir);
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
    let Some(text) = run_dash_with(&home, code, &opts) else {
        return;
    };
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
    if !support::tmux_available() || !python_available() {
        eprintln!("tmux o python3 no están instalados: se salta");
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
    // Oráculo: prólogo instalado y proceso sin escritorio, HOME = B.
    let marker = std::fs::read_to_string(twin.b.root.join(".comandos-twin-guard")).unwrap();
    assert!(marker.contains("red cerrada"), "{marker}");
    let environ = std::fs::read(format!("/proc/{}/environ", twin.oracle.pid())).unwrap();
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

fn python_available() -> bool {
    Command::new("python3")
        .args(["-c", "import sys"])
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
    let Some(text) = run_dash(&home, "print(callable(dash.scope_cmd))") else {
        return;
    };
    assert_eq!(text.trim(), "True");
}

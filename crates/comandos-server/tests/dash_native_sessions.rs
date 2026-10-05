//! Corte tabs, T3: crear y revivir sesiones (`/recover-tab`, `/ensure`,
//! `/new`, `/shell`, `/up`) contra el Python (gemelo).
//!
//! Confinamiento: cada lado del gemelo tiene su HOME temporal corto, su tmux
//! privado (`-S`) y su `fakebin` confinado (`PATH` = solo él). Las sesiones
//! que crean las rutas nacen por el `systemd-run` falso del `fakebin` (anota y
//! solo ejecuta colas de tmux, sin unidades) y el guardián de tmux (`-S` al
//! socket privado, `env -i` con el entorno confinado): su panel corre `bash`
//! con el `claude`/`codex`/`ssh` FALSOS del `fakebin`, que solo anotan. La
//! terminal de `spawn_terminal` y `wmctrl` también son falsos que anotan. Cada
//! prueba que crea sesiones empieza por `assert_confined` (canario). Los
//! clientes de control (`tmux -C attach`) los lanza la prueba con
//! `TestHome::tmux_command` (`-S`) y los mata ella misma (su propio proceso
//! cliente; nunca un `kill-server`/`kill-session`). La limpieza es el `Drop`
//! de cada `TestHome` (`kill-server -S` y después borrar).
mod support;

use std::{
    process::{Child, Stdio},
    time::Duration,
};
use support::{
    FakeLegacy, TestHome, fake_scope_calls, front,
    oracle::fake_calls,
    request_body, run_tmux,
    tabs::{normalize_home, options_for, read_normalized, seed_registry},
    twin::{Twin, normalize},
};

/// Los archivos que escriben estas rutas.
const FILES: [&str; 5] = [
    "app-tabs.json",
    "app-tabs-meta.json",
    "app-tabs-history.json",
    "app-tab-open.json",
    "app-focus.json",
];

/// Canario antes de crear nada: todo lo que una sesión nueva puede lanzar en
/// los dos lados es un falso del `fakebin`, el tmux lleva `-S` al socket
/// privado y el `systemd-run` es el falso que solo ejecuta tmux.
fn assert_confined(t: &Twin) {
    for home in [&t.a, &t.b] {
        let fakebin = home.root.join("fakebin");
        for fake in [
            "claude",
            "codex",
            "grok",
            "opencode",
            "gemini",
            "agy",
            "aider",
            "cc-acp",
            "ssh",
            "kitty",
            "tilix",
            "gnome-terminal",
            "wmctrl",
        ] {
            let path = fakebin.join(fake);
            assert!(
                !path.is_symlink(),
                "{fake} es un enlace: {}",
                path.display()
            );
            let text = std::fs::read_to_string(&path).unwrap();
            assert!(
                text.contains("fakebin.log"),
                "{fake} no es el falso: {text}"
            );
        }
        let socket = comandos_server::dash::native::tmux::private_socket(&home.tmux_dir());
        let guard = std::fs::read_to_string(fakebin.join("tmux")).unwrap();
        assert!(
            guard.contains(&format!("-S '{}'", socket.display())),
            "guardián sin -S privado: {guard}"
        );
        assert!(guard.contains(" -i "), "guardián sin env -i: {guard}");
        let scope = std::fs::read_to_string(fakebin.join("systemd-run")).unwrap();
        assert!(
            scope.contains("faltan las banderas de scope_cmd"),
            "systemd-run no es el falso: {scope}"
        );
        let path = home
            .confined_env()
            .into_iter()
            .find(|(k, _)| k == "PATH")
            .map(|(_, v)| v);
        assert_eq!(path, Some(fakebin.display().to_string()));
    }
    let opts = &t.front_options;
    let fakebin = t.a.root.join("fakebin");
    assert_eq!(opts.tmux.program.path, fakebin.join("tmux"));
    support::assert_private_tmux(opts);
    let scope = opts.scope.as_ref().expect("scope del gemelo");
    assert_eq!(scope.path, fakebin.join("systemd-run"));
    assert!(scope.env_clear, "el scope del frente hereda el entorno");
    assert!(
        t.b.root.join(".comandos-twin-guard").exists(),
        "el prólogo confinado del oráculo no corrió"
    );
}

/// Texto comparable entre los dos HOME: reloj normalizado y raíz como `~`.
fn comparable(home: &TestHome, text: &str) -> String {
    normalize_home(home, &normalize(text))
}

/// Un argumento de una llamada a un falso o a tmux, comparable: la ruta de un
/// ejecutable del `fakebin` por su nombre, los clientes de control por `N`.
fn arg_of(home: &TestHome, arg: &str) -> String {
    let fakebin = format!("{}/", home.root.join("fakebin").display());
    let arg = arg.strip_prefix(&fakebin).unwrap_or(arg);
    let arg = regex::Regex::new(r"client-[0-9]+")
        .unwrap()
        .replace_all(arg, "client-N");
    comparable(home, &arg)
}

/// Las órdenes de tmux que mutan, comparables, y vacía el registro.
fn take_mutations(t: &Twin) -> (Vec<Vec<String>>, Vec<Vec<String>>) {
    let side = |home: &TestHome, calls: Vec<Vec<String>>| -> Vec<Vec<String>> {
        let _ = std::fs::remove_file(home.root.join("tmux.log"));
        calls
            .into_iter()
            .map(|c| c.iter().map(|a| arg_of(home, a)).collect())
            .collect()
    };
    let (a, b) = (t.tmux_mutations_a(), t.tmux_mutations_b());
    (side(&t.a, a), side(&t.b, b))
}

/// Llamadas al `systemd-run` falso, comparables: sin el prefijo del tmux del
/// frente (`-f /dev/null -S <socket>`, el Python no lo pasa).
fn scope_calls(home: &TestHome) -> Vec<Vec<String>> {
    fake_scope_calls(home)
        .into_iter()
        .map(|call| {
            let mut args: Vec<String> = call.iter().map(|a| arg_of(home, a)).collect();
            if let Some(at) = args.iter().position(|a| a == "tmux") {
                while args
                    .get(at + 1)
                    .is_some_and(|flag| flag == "-f" || flag == "-S")
                {
                    args.drain(at + 1..(at + 3).min(args.len()));
                }
            }
            args
        })
        .collect()
}

/// Llamadas a los falsos del `fakebin` con nombre en `names`, comparables.
fn fakes(home: &TestHome, names: &[&str]) -> Vec<Vec<String>> {
    fake_calls(&home.root)
        .into_iter()
        .filter(|c| names.contains(&c.name.as_str()))
        .map(|c| {
            let mut row = vec![c.name.clone()];
            row.extend(c.args.iter().map(|a| arg_of(home, a)));
            row
        })
        .collect()
}

/// La misma petición a los dos lados: estado, cuerpo (raíz como `~`),
/// archivos y órdenes de tmux que mutan iguales. Devuelve el estado.
async fn same(t: &Twin, path: &str, body: &str) -> u16 {
    same_logged(t, path, body).await.0
}

/// `same`, devolviendo además las órdenes de tmux que mutan (iguales).
async fn same_logged(t: &Twin, path: &str, body: &str) -> (u16, Vec<Vec<String>>) {
    take_mutations(t);
    let run = t.post(path, body).await;
    assert_eq!(
        run.front.status,
        run.oracle.status,
        "{path} {body}: {} ≠ {}",
        run.front.text(),
        run.oracle.text()
    );
    assert_eq!(
        comparable(&t.a, &run.front.text()),
        comparable(&t.b, &run.oracle.text()),
        "cuerpo de {path} {body}"
    );
    for name in FILES {
        assert_eq!(
            read_normalized(&t.a, name),
            read_normalized(&t.b, name),
            "{name} tras {path} {body}"
        );
    }
    let (a, b) = take_mutations(t);
    assert_eq!(a, b, "órdenes de tmux de {path} {body}");
    (run.front.status, a)
}

fn argv(calls: &[&[&str]]) -> Vec<Vec<String>> {
    calls
        .iter()
        .map(|c| c.iter().map(|a| (*a).to_owned()).collect())
        .collect()
}

/// `#{session_name}:#{window_name}` de todas las ventanas (vacío sin servidor).
/// Los nombres que no fijan las rutas (`claude`, `shell`) los pone el
/// `automatic-rename` de tmux según el proceso del momento: cuentan como `*`.
fn windows(home: &TestHome) -> Vec<String> {
    let out = home
        .tmux_command()
        .args(["list-windows", "-a", "-F", "#{session_name}:#{window_name}"])
        .output()
        .unwrap();
    let mut lines: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|line| match line.rsplit_once(':') {
            Some((sess, name)) if name != "claude" && name != "shell" => format!("{sess}:*"),
            _ => line.to_owned(),
        })
        .collect();
    lines.sort();
    lines
}

/// Espera a que los dos `fakebin.log` tengan la llamada `name` (el panel corrió
/// el falso); devuelve cuántas hay en A.
async fn wait_fake(t: &Twin, name: &str, count: usize) {
    for _ in 0..200 {
        let a = fakes(&t.a, &[name]).len();
        let b = fakes(&t.b, &[name]).len();
        if a >= count && b >= count {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("el falso {name} no corrió {count} veces en los dos lados");
}

#[tokio::test]
async fn create_and_revive_match_python() {
    let Some(t) = Twin::start("sess", |h| {
        seed_registry(h);
        run_tmux(h, &["new-session", "-d", "-s", "ssh-x", "cat"]);
    })
    .await
    else {
        return;
    };
    assert_confined(&t);
    // Proyecto: ventana del agente (el `claude` falso) y `shell`.
    let launch = "set -a; . \"$HOME/.claude/hooks/providers.env\" 2>/dev/null; set +a; \
                  claude --continue 2>/dev/null || claude; exec $SHELL";
    let (status, log) = same_logged(&t, "/new", r#"{"session":"p2f"}"#).await;
    assert_eq!(status, 200);
    assert_eq!(
        log,
        argv(&[
            &[
                "new-session",
                "-d",
                "-s",
                "p2f",
                "-n",
                "claude",
                "-c",
                "~/codebase/p2f",
                launch
            ],
            &[
                "new-window",
                "-d",
                "-t",
                "=p2f",
                "-n",
                "shell",
                "-c",
                "~/codebase/p2f"
            ],
        ])
    );
    let scoped = scope_calls(&t.a);
    assert_eq!(
        scoped
            .first()
            .map(|c| c.iter().take(6).cloned().collect::<Vec<_>>()),
        Some(
            [
                "--user",
                "--scope",
                "--collect",
                "--quiet",
                "tmux",
                "new-session"
            ]
            .map(String::from)
            .to_vec()
        ),
        "{scoped:?}"
    );
    wait_fake(&t, "claude", 1).await;
    // Ya viva: solo el registro.
    assert_eq!(same(&t, "/new", r#"{"session":"p2f"}"#).await, 200);
    // Sin proyecto: shell en el HOME.
    assert_eq!(same(&t, "/new", r#"{"session":"suelta"}"#).await, 200);
    assert_eq!(
        same(&t, "/ensure", r#"{"session":"p2f","win":"shell"}"#).await,
        200
    );
    // ssh: identidad derivada y la ventana `shell` con otra conexión (el
    // `ssh` falso, que falla).
    assert_eq!(
        same(&t, "/ensure", r#"{"session":"ssh-x","win":"shell"}"#).await,
        200
    );
    wait_fake(&t, "ssh", 1).await;
    // Sin clientes: la terminal falsa por el `systemd-run` falso.
    assert_eq!(same(&t, "/shell", r#"{"session":"p2f"}"#).await, 200);
    assert_eq!(same(&t, "/up", r#"{"session":"p2f"}"#).await, 200);
    // Revivir con otro agente (el `codex` falso) y una que no tiene carpeta.
    assert_eq!(
        same(
            &t,
            "/recover-tab",
            r#"{"session":"rec","label":"p2f","agent":"codex"}"#
        )
        .await,
        200
    );
    wait_fake(&t, "codex", 1).await;
    assert_eq!(
        same(&t, "/recover-tab", r#"{"session":"rec2","label":"nada"}"#).await,
        400
    );
    // Agente fuera de `agent_set()`: `claude`.
    assert_eq!(
        same(
            &t,
            "/recover-tab",
            r#"{"session":"rec3","label":"p2f","agent":"nave"}"#
        )
        .await,
        200
    );
    wait_fake(&t, "claude", 2).await;
    // `/ensure` revive con el `cwd` pedido.
    let cwd = |h: &TestHome| h.root.join("codebase/p2f").display().to_string();
    let body_a = format!(r#"{{"session":"otra-e","cwd":"{}"}}"#, cwd(&t.a));
    let body_b = format!(r#"{{"session":"otra-e","cwd":"{}"}}"#, cwd(&t.b));
    let (fa, fb) = (
        t.post_front("/ensure", &body_a).await,
        t.post_oracle("/ensure", &body_b).await,
    );
    assert_eq!((fa.status, fa.text()), (fb.status, fb.text()));
    for name in FILES {
        assert_eq!(read_normalized(&t.a, name), read_normalized(&t.b, name));
    }
    let (ma, mb) = take_mutations(&t);
    assert_eq!(ma, mb, "órdenes de tmux de /ensure con cwd");
    wait_fake(&t, "claude", 3).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(windows(&t.a), windows(&t.b), "ventanas");
    assert_eq!(scope_calls(&t.a), scope_calls(&t.b), "systemd-run");
    let agents = ["claude", "codex", "ssh", "kitty", "wmctrl"];
    let (mut fa, mut fb) = (fakes(&t.a, &agents), fakes(&t.b, &agents));
    fa.sort();
    fb.sort();
    assert_eq!(fa, fb, "falsos lanzados");
    // Las terminales se lanzaron por el `systemd-run` falso con sus banderas.
    assert!(
        scope_calls(&t.a).iter().any(|c| c.starts_with(&[
            "--user".into(),
            "--collect".into(),
            "--quiet".into(),
            "kitty".into()
        ])),
        "{:?}",
        scope_calls(&t.a)
    );
}

#[tokio::test]
async fn up_without_dir_writes_nothing() {
    let Some(t) = Twin::start("up-nodir", seed_registry).await else {
        return;
    };
    assert_confined(&t);
    let before: Vec<Option<String>> = FILES.iter().map(|f| read_normalized(&t.a, f)).collect();
    assert_eq!(same(&t, "/up", r#"{"session":"inexistente"}"#).await, 400);
    for (status, path) in [(400, "/shell"), (400, "/ensure")] {
        assert_eq!(same(&t, path, r#"{"session":"inexistente"}"#).await, status);
    }
    assert!(windows(&t.a).is_empty() && windows(&t.b).is_empty());
    let after: Vec<Option<String>> = FILES.iter().map(|f| read_normalized(&t.a, f)).collect();
    assert_eq!(before, after, "el registro no cambia");
    assert!(fake_scope_calls(&t.a).is_empty() && fake_scope_calls(&t.b).is_empty());
}

#[tokio::test]
async fn no_scope_in_production_declines_before_creating() {
    if !support::tmux_available() {
        return;
    }
    let home = TestHome::new_short("sess-noscope");
    seed_registry(&home);
    let legacy = FakeLegacy::start().await;
    let mut opts = options_for(&home);
    opts.scope = None;
    let fr = front(&home, legacy.port, opts).await;
    for path in ["/new", "/ensure", "/shell", "/up", "/recover-tab"] {
        let wire = request_body(fr.port, "POST", path, "", r#"{"session":"p2f"}"#).await;
        assert_eq!(wire.text(), r#"{"legacy": true}"#, "{path}");
    }
    assert_eq!(legacy.requests().len(), 5);
    // Ni una orden de tmux, ni un servidor, ni el registro tocado.
    assert!(!home.root.join("tmux.log").exists());
    let socket = comandos_server::dash::native::tmux::private_socket(&home.tmux_dir());
    assert!(!socket.exists(), "nació un servidor tmux");
    assert!(!support::tabs::exists(&home, "app-tab-open.json"));
    fr.stop().await;
}

/// Un cliente de control (`tmux -C attach`, sin terminal) en `sess` del
/// servidor privado de `home`. La prueba lo mata con `stop_client`.
fn control_client(home: &TestHome, sess: &str) -> Child {
    let target = format!("={sess}");
    home.tmux_command()
        .args(["-C", "attach", "-t", &target])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

/// Mata el proceso cliente que lanzó la prueba (no el servidor ni sesiones).
fn stop_client(mut child: Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Espera a que `list-clients` del servidor de `home` cumpla `want`.
async fn wait_clients(home: &TestHome, want: impl Fn(&str) -> bool) {
    for _ in 0..200 {
        let out = home
            .tmux_command()
            .args(["list-clients", "-F", "#{session_name}"])
            .output()
            .unwrap();
        if want(&String::from_utf8_lossy(&out.stdout)) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("los clientes de control no llegaron al estado esperado");
}

#[tokio::test]
async fn focus_with_clients_matches_python() {
    let Some(t) = Twin::start("sess-focus", |h| {
        seed_registry(h);
        for sess in ["p2f", "local", "otra"] {
            run_tmux(h, &["new-session", "-d", "-s", sess, "cat"]);
        }
    })
    .await
    else {
        return;
    };
    assert_confined(&t);
    // App abierta (cliente en `local`): `app-focus.json` y la ventana de la app.
    let (ca, cb) = (control_client(&t.a, "local"), control_client(&t.b, "local"));
    for home in [&t.a, &t.b] {
        wait_clients(home, |s| s.contains("local")).await;
    }
    assert_eq!(same(&t, "/up", r#"{"session":"p2f"}"#).await, 200);
    assert!(support::tabs::exists(&t.a, "app-focus.json"));
    stop_client(ca);
    stop_client(cb);
    for home in [&t.a, &t.b] {
        wait_clients(home, |s| s.trim().is_empty()).await;
    }
    // Sin app y sin clientes en `p2f`: el cliente más activo cambia a `p2f`.
    let (ca, cb) = (control_client(&t.a, "otra"), control_client(&t.b, "otra"));
    for home in [&t.a, &t.b] {
        wait_clients(home, |s| s.contains("otra")).await;
    }
    assert_eq!(same(&t, "/shell", r#"{"session":"p2f"}"#).await, 200);
    // El pulso keep-above termina 0,6 s después.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    stop_client(ca);
    stop_client(cb);
    let (wa, wb) = (fakes(&t.a, &["wmctrl"]), fakes(&t.b, &["wmctrl"]));
    assert_eq!(wa, wb, "wmctrl");
    assert!(
        wa.iter().any(|c| c.iter().any(|a| a == "remove,above")),
        "{wa:?}"
    );
    assert_eq!(windows(&t.a), windows(&t.b), "ventanas");
}

#[tokio::test]
async fn bad_inputs_match_python() {
    let Some(t) = Twin::start("sess-bad", seed_registry).await else {
        return;
    };
    assert_confined(&t);
    for (path, body) in [
        ("/new", r#"{"session":"mal nombre"}"#),
        ("/recover-tab", r#"{"session":"a b"}"#),
        ("/recover-tab", r#"{"session":5}"#),
        ("/ensure", r#"{"session":["x"]}"#),
        // Agente que no es texto al crear: el `agent.replace` del Python (500).
        ("/new", r#"{"session":"p2f","agent":5}"#),
    ] {
        same(&t, path, body).await;
    }
    assert!(windows(&t.a).is_empty() && windows(&t.b).is_empty());
}

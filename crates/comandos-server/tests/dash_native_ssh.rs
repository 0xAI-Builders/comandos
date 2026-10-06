//! Configuración SSH nativa (plan 2f-3, Tarea 3) contra el `cc-dash` Python.
//!
//! Confinamiento: `~/.ssh` es el del HOME temporal de cada lado (nunca el
//! real); `/ssh-key-setup` crea su sesión en el tmux privado de cada HOME por el
//! `systemd-run` falso del gemelo (que solo ejecuta colas de tmux) y el
//! guardián (`-S`); `ssh-copy-id` y `ssh` son falsos del `fakebin` que solo
//! anotan (y esperan, para que la sesión siga viva al compararla). Las
//! sesiones `ssh-key-*` mueren con el `Drop` de `TestHome`, por su `-S`. Ningún
//! `kill-*` en la prueba.
mod support;

use comandos_server::dash::native::files::FileLock;
use serde_json::json;
use std::{
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::Path,
    time::{Duration, Instant},
};
use support::{
    FakeLegacy, TestHome, front,
    oracle::{FakeCall, write_executable},
    request_body,
    twin::{Twin, TwinOpts},
};

/// `ssh-copy-id` falso de los dos lados: anota en `$HOME/fakebin.log` (el HOME
/// del pane es el temporal de su lado) y espera, así la sesión sigue viva.
const SSH_COPY_ID: &str = "#!/bin/sh\n\
    printf '%s\\0' ssh-copy-id \"$@\" \"$(printf '\\036')\" >> \"$HOME/fakebin.log\"\n\
    exec sleep 30\n";

/// Lo observable de `~/.ssh` en un HOME: modo del directorio, bytes y modo de
/// `config`, modo de `config.lock`.
fn snapshot(home: &TestHome) -> String {
    let ssh = home.root.join(".ssh");
    let mode = |p: &Path| {
        std::fs::metadata(p)
            .map(|m| format!("{:o}", m.permissions().mode() & 0o7777))
            .unwrap_or_else(|_| "-".into())
    };
    let config = std::fs::read(ssh.join("config"))
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_else(|_| "<sin archivo>".into());
    format!(
        "dir {} | config {} {:?} | lock {}",
        mode(&ssh),
        mode(&ssh.join("config")),
        config,
        mode(&ssh.join("config.lock")),
    )
}

/// Rutas de cada HOME sustituidas por `<HOME>` (la llave va con ruta absoluta).
fn unhome(home: &TestHome, text: &str) -> String {
    text.replace(&home.root.display().to_string(), "<HOME>")
}

fn calls(home: &TestHome, list: Vec<FakeCall>, name: &str) -> Vec<Vec<String>> {
    list.into_iter()
        .filter(|c| c.name == name)
        .map(|c| c.args.iter().map(|a| unhome(home, a)).collect())
        .collect()
}

fn mutations(home: &TestHome, list: Vec<Vec<String>>) -> Vec<Vec<String>> {
    list.into_iter()
        .map(|args| args.iter().map(|a| unhome(home, a)).collect())
        .collect()
}

/// Escribe lo mismo en los dos lados (bytes y modo).
fn both(t: &Twin, rel: &str, bytes: &[u8], mode: u32) {
    for home in [&t.a, &t.b] {
        let path = home.root.join(rel);
        std::fs::write(&path, bytes).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
    }
}

async fn same(t: &Twin, method: &str, path: &str, body: &str) -> String {
    let run = t.request(method, path, body).await;
    run.assert_same();
    assert_eq!(
        snapshot(&t.a),
        snapshot(&t.b),
        "~/.ssh tras {method} {path} {body}"
    );
    run.front.text()
}

#[tokio::test]
async fn ssh_routes_match_python() {
    let opts = TwinOpts {
        fakebin_extra: vec![("ssh-copy-id".into(), SSH_COPY_ID.into())],
        ..TwinOpts::default()
    };
    let Some(t) = Twin::start_with("ssh", |_| {}, opts).await else {
        return;
    };
    // Sin `~/.ssh`: lista vacía y una inyección rechazada sin crear nada.
    assert_eq!(same(&t, "GET", "/ssh", "").await, "[]");
    let inject = json!({"host": "srv", "hostname": "10.0.0.5\nProxyCommand touch /tmp/pwn"});
    let body = same(&t, "POST", "/ssh-add", &inject.to_string()).await;
    assert!(body.contains("Caracteres no permitidos"), "{body}");
    assert!(!t.a.root.join(".ssh").exists());
    // Alta válida: crea `~/.ssh` (0700), `config` (0600) y el candado.
    let add = json!({"host": "srv", "hostname": "10.0.0.5", "user": "root", "port": "2222",
                     "identity": "~/.ssh/id_srv"});
    assert_eq!(
        same(&t, "POST", "/ssh-add", &add.to_string()).await,
        r#"{"ok": true}"#
    );
    assert!(snapshot(&t.a).starts_with("dir 700 | config 600"));
    let body = same(&t, "POST", "/ssh-add", &add.to_string()).await;
    assert!(body.contains("ya existe"), "{body}");
    for bad in [
        json!({"host": "a b", "hostname": "h"}),
        json!({"host": "a", "hostname": ""}),
        json!({"host": "a", "hostname": "h", "user": "a b"}),
        json!({"host": "a", "hostname": "h", "port": "0"}),
        json!({"host": "a", "hostname": "h", "port": 70000}),
        json!({"host": "a", "hostname": "h", "port": 1.5}),
        json!({"host": "a", "hostname": "h", "port": true}),
        json!({"host": "a", "hostname": "h", "port": ["1"]}),
        json!({"host": "a", "hostname": "h", "identity": "a b"}),
        json!({"host": "a", "hostname": "h", "user": "u\u{1f}"}),
        json!({"host": 5, "hostname": "h"}),
        json!({"host": "a", "hostname": 5}),
        json!({"host": "a", "hostname": "h", "identity": null}),
    ] {
        same(&t, "POST", "/ssh-add", &bad.to_string()).await;
    }
    let other = json!({"host": "otro", "hostname": "[::1]", "port": 22, "user": "a@b"});
    same(&t, "POST", "/ssh-add", &other.to_string()).await;
    // Texto a mano: CRLF, una línea de varios hosts, separadores Unicode de
    // `splitlines` y un modo propio que la reescritura debe conservar.
    let mut text = std::fs::read(t.a.root.join(".ssh/config")).unwrap();
    text.extend_from_slice(
        "\r\nHost compartido tercero\r\n  HostName c\r\n# fin\u{2028}Host wild*\n  User w\n"
            .as_bytes(),
    );
    both(&t, ".ssh/config", &text, 0o640);
    same(&t, "GET", "/ssh", "").await;
    same(&t, "GET", "/ssh?x=1", "").await;
    let edit = json!({"orig": "srv", "host": "srv2", "hostname": "h2", "port": 22});
    assert_eq!(
        same(&t, "POST", "/ssh-update", &edit.to_string()).await,
        r#"{"ok": true}"#
    );
    assert!(snapshot(&t.a).contains("config 640"));
    for body in [
        json!({"orig": "srv2", "host": "otro", "hostname": "h"}),
        json!({"orig": "nada", "host": "n", "hostname": "h"}),
        json!({"orig": 5, "host": "n", "hostname": "h"}),
        json!({"orig": null, "host": "n", "hostname": "h"}),
        json!({"orig": "srv2", "host": "n", "hostname": "h", "port": "x"}),
        json!({"orig": "compartido", "host": "n", "hostname": "h"}),
    ] {
        same(&t, "POST", "/ssh-update", &body.to_string()).await;
    }
    for body in [
        json!({"host": "compartido"}),
        json!({"host": 5}),
        json!({}),
        json!({"host": "otro"}),
    ] {
        same(&t, "POST", "/ssh-del", &body.to_string()).await;
    }
    same(&t, "GET", "/ssh", "").await;

    // `/ssh-key-setup`: desconocido, no texto, sin llave, sin `ssh-copy-id`.
    let conllave = json!({"host": "conllave", "hostname": "k", "identity": "~/.ssh/propia"});
    same(&t, "POST", "/ssh-add", &conllave.to_string()).await;
    same(&t, "POST", "/ssh-key-setup", r#"{"host": "desconocido"}"#).await;
    same(&t, "POST", "/ssh-key-setup", r#"{"host": "a b"}"#).await;
    same(&t, "POST", "/ssh-key-setup", r#"{"host": 5}"#).await;
    let body = same(&t, "POST", "/ssh-key-setup", r#"{"host": "srv2"}"#).await;
    assert!(body.contains("No encontre llave publica"), "{body}");
    for home in [&t.a, &t.b] {
        std::fs::remove_file(home.root.join("fakebin/ssh-copy-id")).unwrap();
    }
    let body = same(&t, "POST", "/ssh-key-setup", r#"{"host": "srv2"}"#).await;
    assert!(body.contains("ssh-copy-id no esta instalado"), "{body}");
    for home in [&t.a, &t.b] {
        write_executable(&home.root.join("fakebin/ssh-copy-id"), SSH_COPY_ID);
    }
    assert!(t.tmux_mutations_a().is_empty() && t.tmux_mutations_b().is_empty());

    // Con llave: la sesión nace en los dos lados con la misma orden.
    both(
        &t,
        ".ssh/id_ed25519.pub",
        b"ssh-ed25519 AAAA prueba\n",
        0o644,
    );
    both(&t, ".ssh/propia.pub", b"ssh-ed25519 BBBB propia\n", 0o644);
    let body = same(&t, "POST", "/ssh-key-setup", r#"{"host": "srv2"}"#).await;
    assert_eq!(body, r#"{"ok": true, "session": "ssh-key-srv2"}"#);
    same(&t, "POST", "/ssh-key-setup", r#"{"host": "conllave"}"#).await;
    // Repetir reutiliza la sesión (`has-session`), sin otra `new-session`.
    same(&t, "POST", "/ssh-key-setup", r#"{"host": "srv2"}"#).await;
    let (front, oracle) = (
        mutations(&t.a, t.tmux_mutations_a()),
        mutations(&t.b, t.tmux_mutations_b()),
    );
    assert_eq!(front, oracle, "órdenes de tmux");
    assert_eq!(front.len(), 2, "{front:?}");
    let sessions = |out: String| {
        let mut names: Vec<String> = out.lines().map(str::to_owned).collect();
        names.sort();
        names
    };
    let listed = sessions(t.tmux_a(&["list-sessions", "-F", "#{session_name}"]));
    assert_eq!(listed, ["ssh-key-conllave", "ssh-key-srv2"]);
    assert_eq!(
        listed,
        sessions(t.tmux_b(&["list-sessions", "-F", "#{session_name}"]))
    );
    // `ssh-copy-id` corrió en los dos panes con la misma llave.
    let started = Instant::now();
    let copies = loop {
        let a = calls(&t.a, t.fake_calls_a(), "ssh-copy-id");
        let b = calls(&t.b, t.fake_calls_b(), "ssh-copy-id");
        if (a.len() >= 2 && b.len() >= 2) || started.elapsed() > Duration::from_secs(10) {
            break (a, b);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    let (mut a, mut b) = copies;
    a.sort();
    b.sort();
    assert_eq!(a, b);
    assert!(
        a.contains(&vec![
            "-o".into(),
            "ConnectTimeout=8".into(),
            "-i".into(),
            "<HOME>/.ssh/propia.pub".into(),
            "conllave".into(),
        ]),
        "{a:?}"
    );
}

/// Review Focus 3: una edición cuya alta no valida (o cuyo origen no existe,
/// o que choca con otro host) deja `~/.ssh/config` byte a byte igual, sin
/// reescribirlo (mismo inodo y mismo modo).
#[tokio::test]
async fn ssh_update_invalid_keeps_file() {
    let home = TestHome::new("ssh-keep");
    let ssh = home.root.join(".ssh");
    std::fs::create_dir_all(&ssh).unwrap();
    let config = ssh.join("config");
    let text = b"Host a\r\n  HostName x\r\nHost b c\n  User u\n\n\n";
    std::fs::write(&config, text).unwrap();
    std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o644)).unwrap();
    let before = std::fs::metadata(&config).unwrap();
    let legacy = FakeLegacy::start().await;
    let server = front(&home, legacy.port, home.options()).await;
    for (body, expected) in [
        (
            json!({"orig": "a", "host": "a", "hostname": "h", "port": "99999"}),
            "Puerto invalido",
        ),
        (
            json!({"orig": "a", "host": "a", "hostname": "h\nProxyCommand x"}),
            "Caracteres no permitidos",
        ),
        (
            json!({"orig": "zz", "host": "a", "hostname": "h"}),
            "No existe",
        ),
        (
            json!({"orig": "a", "host": "c", "hostname": "h"}),
            "'c' ya existe en ~/.ssh/config",
        ),
        (
            json!({"orig": "b", "host": "d", "hostname": "h"}),
            "Ese host comparte linea con otros; editalo a mano",
        ),
    ] {
        let wire = request_body(server.port, "POST", "/ssh-update", "", &body.to_string()).await;
        assert_eq!(wire.status, 400, "{}", wire.text());
        assert_eq!(wire.text(), format!(r#"{{"error": "{expected}"}}"#));
        let after = std::fs::metadata(&config).unwrap();
        assert_eq!(std::fs::read(&config).unwrap(), text);
        assert_eq!(after.ino(), before.ino());
        assert_eq!(after.mode(), before.mode());
    }
    assert!(legacy.requests().is_empty(), "{:?}", legacy.requests());
    server.stop().await;
}

/// `file_lock` del Python: con el candado en manos de otro (cc-app, el Python)
/// el alta espera sin bloquear el runtime y escribe al soltarse.
#[tokio::test]
async fn ssh_add_waits_for_lock() {
    let home = TestHome::new("ssh-lock");
    let ssh = home.root.join(".ssh");
    std::fs::create_dir_all(&ssh).unwrap();
    let held = FileLock::acquire(&ssh.join("config")).unwrap();
    let legacy = FakeLegacy::start().await;
    let server = front(&home, legacy.port, home.options()).await;
    let port = server.port;
    let pending = tokio::spawn(async move {
        request_body(
            port,
            "POST",
            "/ssh-add",
            "",
            r#"{"host": "srv", "hostname": "h"}"#,
        )
        .await
    });
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(!pending.is_finished(), "no esperó el candado");
    // El runtime sigue libre: otra ruta responde mientras tanto.
    let list = request_body(port, "GET", "/ssh", "", "").await;
    assert_eq!((list.status, list.text()), (200, "[]".into()));
    drop(held);
    let wire = pending.await.unwrap();
    assert_eq!((wire.status, wire.text()), (200, r#"{"ok": true}"#.into()));
    assert_eq!(
        std::fs::read_to_string(ssh.join("config")).unwrap(),
        "\nHost srv\n    HostName h\n"
    );
    server.stop().await;
}

/// Baja y edición sin `~/.ssh`: los dos lados crean el directorio y el
/// candado (`file_lock` hace `makedirs`) y responden «No hay ~/.ssh/config».
#[tokio::test]
async fn ssh_del_and_update_without_dir_match_python() {
    let Some(t) = Twin::start("ssh-nodir", |_| {}).await else {
        return;
    };
    assert!(!t.a.root.join(".ssh").exists());
    let body = same(&t, "POST", "/ssh-del", r#"{"host": "srv"}"#).await;
    assert_eq!(body, r#"{"error": "No hay ~/.ssh/config"}"#);
    let _ = std::fs::remove_dir_all(t.a.root.join(".ssh"));
    let _ = std::fs::remove_dir_all(t.b.root.join(".ssh"));
    let edit = json!({"orig": "srv", "host": "srv", "hostname": "h"});
    let body = same(&t, "POST", "/ssh-update", &edit.to_string()).await;
    assert_eq!(body, r#"{"error": "No hay ~/.ssh/config"}"#);
    assert!(snapshot(&t.a).contains("config - \"<sin archivo>\""));
}

/// `~/.ssh/config` que no es UTF-8: el Python (locale UTF-8) lanza al leerlo
/// y el frente da el mismo 500, sin tocar el archivo.
#[tokio::test]
async fn ssh_non_utf8_config_fails_and_keeps_file() {
    let Some(t) = Twin::start("ssh-latin", |_| {}).await else {
        return;
    };
    let text = b"Host a\n  HostName caf\xe9\n";
    for home in [&t.a, &t.b] {
        let ssh = home.root.join(".ssh");
        std::fs::create_dir_all(&ssh).unwrap();
        std::fs::set_permissions(&ssh, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    both(&t, ".ssh/config", text, 0o600);
    let before = std::fs::metadata(t.a.root.join(".ssh/config")).unwrap();
    for (method, path, body) in [
        ("GET", "/ssh", String::new()),
        (
            "POST",
            "/ssh-add",
            json!({"host": "b", "hostname": "h"}).to_string(),
        ),
        ("POST", "/ssh-del", json!({"host": "a"}).to_string()),
        (
            "POST",
            "/ssh-update",
            json!({"orig": "a", "host": "a", "hostname": "h"}).to_string(),
        ),
    ] {
        let run = t.request(method, path, &body).await;
        run.assert_same();
        assert_eq!(
            run.front.status,
            500,
            "{method} {path}: {}",
            run.front.text()
        );
        assert_eq!(
            snapshot(&t.a),
            snapshot(&t.b),
            "~/.ssh tras {method} {path}"
        );
        let config = t.a.root.join(".ssh/config");
        assert_eq!(std::fs::read(&config).unwrap(), text);
        assert_eq!(std::fs::metadata(&config).unwrap().ino(), before.ino());
    }
}

/// Sin `systemd-run` el frente declina `/ssh-key-setup` antes de crear la
/// sesión: la petición llega tal cual al heredado y no nace ningún servidor
/// tmux ni corre `ssh-copy-id`.
#[tokio::test]
async fn ssh_key_setup_without_scope_declines() {
    let home = TestHome::new("ssh-noscope");
    let ssh = home.root.join(".ssh");
    std::fs::create_dir_all(&ssh).unwrap();
    std::fs::write(ssh.join("config"), "Host srv\n  HostName h\n").unwrap();
    std::fs::write(ssh.join("id_ed25519.pub"), "ssh-ed25519 AAAA prueba\n").unwrap();
    let log = home.root.join("ssh-copy-id.log");
    write_executable(
        &home.root.join("bin/ssh-copy-id"),
        &format!("#!/bin/sh\necho \"$@\" >> '{}'\n", log.display()),
    );
    let legacy = FakeLegacy::start().await;
    let opts = home.options();
    assert!(opts.scope.is_none());
    let server = front(&home, legacy.port, opts).await;
    let wire = request_body(
        server.port,
        "POST",
        "/ssh-key-setup",
        "",
        r#"{"host": "srv"}"#,
    )
    .await;
    assert_eq!(wire.text(), r#"{"legacy": true}"#);
    assert_eq!(legacy.requests().len(), 1, "{:?}", legacy.requests());
    assert!(!log.exists(), "ssh-copy-id no corrió");
    assert!(
        !support::private_socket_of(&home.tmux_dir()).exists(),
        "ningún servidor tmux"
    );
    server.stop().await;
}

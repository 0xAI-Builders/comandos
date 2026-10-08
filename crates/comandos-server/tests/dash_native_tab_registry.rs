//! Registro de pestañas (2f-1, Tarea 1): mismo resultado en disco, mismas
//! respuestas de las funciones y mismas órdenes de tmux que el Python.
//!
//! Confinamiento: dos HOME temporales cortos, cada uno con su tmux privado
//! (`-S`), su `fakebin` confinado y su `~/.ssh/config` falso. El frente (A)
//! llama a tmux por el guardián de su `fakebin`; el Python (B, `run_dash`)
//! también. Las únicas órdenes de tmux de estas funciones son lecturas
//! (`list-sessions`, `display-message`); las sesiones las siembra la prueba con
//! `run_tmux` y las mata el `Drop` de `TestHome` por su `-S`.
mod support;

use comandos_core::json::response_dumps;
use comandos_server::{
    HandlerError,
    dash::native::{
        Fault,
        files::FileLock,
        tabs::tab_registry::{self as reg, RegistryError},
    },
};
use comandos_store::workspace::WorkspaceStore;
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use support::{
    TestHome, run_tmux,
    tabs::{REGISTRY_FILES, TmuxLog, native_for, normalize_home, read_normalized, seed_registry},
    tmux_available,
};

// Only hook documents are restored on replay. Expected tmux argv is carried
// in stdout and compared against calls made by the real native private actor.
fn frozen_dash(home: &TestHome, code: &str) -> String {
    let code = format!(
        "dash.time.time = lambda: {}\n{code}",
        support::NOW_MS / 1000
    );
    support::http_golden::dash_files(
        home,
        "tab-registry",
        &REGISTRY_FILES,
        &code,
        &Default::default(),
    )
}

/// Dos HOME sembrados igual (A para el frente, B para el Python).
fn pair(tag: &str, sessions: bool) -> Option<(TestHome, TestHome)> {
    if sessions && !tmux_available() {
        eprintln!("tmux no está instalado: se salta");
        return None;
    }
    let a = TestHome::new_short(&format!("{tag}-a"));
    let b = TestHome::new_short(&format!("{tag}-b"));
    for home in [&a, &b] {
        seed_registry(home);
        if sessions {
            let root = home.root.to_str().unwrap();
            run_tmux(home, &["new-session", "-d", "-s", "p2f", "-c", root, "cat"]);
            run_tmux(home, &["new-session", "-d", "-s", "otra", "-c", "/", "cat"]);
        }
    }
    Some((a, b))
}

fn assert_files_equal(a: &TestHome, b: &TestHome) {
    for name in REGISTRY_FILES {
        assert_eq!(read_normalized(a, name), read_normalized(b, name), "{name}");
    }
}

fn value(found: Option<Value>) -> Value {
    found.unwrap_or(Value::Null)
}

/// El workspace guardado: revisión y documento.
async fn workspace_of(native: &comandos_server::dash::native::Native) -> Value {
    native
        .with_state(|b| {
            WorkspaceStore::new(&b.conn)
                .current()
                .ok()
                .flatten()
                .map(|s| json!({"revision": s.revision, "document": s.document}))
        })
        .await
        .ok()
        .flatten()
        .unwrap_or(Value::Null)
}

const PY_WORKSPACE: &str = r#"
_c = dash.workspace_store().current()
_out.append(_c and {"revision": _c["revision"], "document": _c["document"]})
print(json.dumps(_out))
"#;

#[tokio::test]
async fn register_close_and_history_match_python() {
    let Some((a, b)) = pair("reg", true) else {
        return;
    };
    let native = native_for(&a).await;
    let mut rust = Vec::new();
    reg::register_app_tab(&native, "p2f", Some("Proyecto ñ"), "project", "", "/tmp")
        .await
        .unwrap();
    rust.push(value(
        reg::write_tab_metadata(&native, "ssh-x", "ssh", "x", "")
            .await
            .unwrap(),
    ));
    reg::register_app_tab(&native, "term-r1", None, "", "", "")
        .await
        .unwrap();
    reg::register_app_tab(&native, "otra", Some("Nueva"), "nave", "", "")
        .await
        .unwrap();
    // Un workspace previo con las pestañas, para que el cierre lo ajuste.
    let hooks = a.hooks();
    native
        .with_state(move |b| comandos_server::dash::native::workspace::sync(b, &hooks, 1.0))
        .await
        .ok()
        .unwrap()
        .ok()
        .unwrap();
    TmuxLog(&a).clear();
    rust.push(json!(
        reg::close_app_tab(&native, "p2f", false).await.unwrap()
    ));
    let rust_tmux = TmuxLog(&a).take();
    rust.push(workspace_of(&native).await);
    // Lo que la prueba quiere ver también sin oráculo.
    assert_eq!(
        rust_tmux,
        [
            vec!["list-sessions", "-F", "#{session_name}"],
            vec![
                "display-message",
                "-p",
                "-t",
                "=p2f:",
                "#{pane_current_path}"
            ],
        ]
    );
    let history: Value = serde_json::from_str(
        &std::fs::read_to_string(a.hooks().join("app-tabs-history.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(history[0]["session"], "p2f");
    assert_eq!(history[0]["label"], "Proyecto ñ");
    assert_eq!(history[0]["reason"], "closed");
    let code = r#"
import json
_out = []
dash.register_app_tab("p2f", "Proyecto ñ", kind="project", cwd="/tmp")
_out.append(dash.write_tab_metadata("ssh-x", "ssh", host="x"))
dash.register_app_tab("term-r1", None)
dash.register_app_tab("otra", "Nueva", kind="nave")
dash.workspace_sync()
open(dash.os.path.join(dash.os.environ["HOME"], "tmux.log"), "w").close()
_out.append(dash.close_app_tab("p2f"))
"#;
    let workspace_and_calls = PY_WORKSPACE.replace(
        "print(json.dumps(_out))",
        r#"
import os
_calls = []
for _r in open(os.path.join(os.environ['HOME'], 'tmux.log')).read().split('\0\x1e\0'):
    if not _r:
        continue
    _args = _r.split('\0')[1:]
    while len(_args) >= 2 and _args[0] in ('-f', '-S', '-L'):
        _args = _args[2:]
    _calls.append(_args)
print(json.dumps({'result': _out, 'calls': _calls}))
"#,
    );
    let text = frozen_dash(&b, &format!("{code}{workspace_and_calls}"));
    let artifact: Value = serde_json::from_str(text.trim()).unwrap();
    let python = &artifact["result"];
    assert_eq!(
        normalize_home(&a, &response_dumps(&json!(rust)).unwrap()),
        normalize_home(&b, &response_dumps(python).unwrap())
    );
    let expected_calls: Vec<Vec<String>> =
        serde_json::from_value(artifact["calls"].clone()).unwrap();
    assert_eq!(rust_tmux, expected_calls, "órdenes de tmux");
    assert_files_equal(&a, &b);
}

/// `read_tab_metadata` y `tab_metadata_for_session`: solo leen, así que el
/// frente y el Python leen el MISMO HOME.
#[tokio::test]
async fn tab_metadata_rules_match_python() {
    let home = TestHome::new_short("reg-meta");
    seed_registry(&home);
    std::fs::create_dir_all(home.root.join("codebase/grupo/Otro.Proyecto")).unwrap();
    let native = native_for(&home).await;
    let sessions = [
        "otra",
        "rara",
        "p2f",
        "P2F",
        "otro-proyecto",
        "ssh-x",
        "ssh-y",
        "sshtab-x-2",
        "sshtab-x-a",
        "sshtab-x-",
        "sshtab-x",
        "sshtab-y-1",
        "term-9",
        "cosa",
        "local",
    ];
    let before = std::fs::read(home.hooks().join("app-tabs-meta.json")).unwrap();
    let mut rust = vec![Value::Object(
        reg::read_tab_metadata(&home.hooks().join("app-tabs-meta.json")).unwrap(),
    )];
    for sess in sessions {
        rust.push(reg::tab_metadata_for_session(&native, sess).await.unwrap());
    }
    // Nadie escribió nada.
    assert_eq!(
        before,
        std::fs::read(home.hooks().join("app-tabs-meta.json")).unwrap()
    );
    assert_eq!(rust[0], json!({"otra": {"kind": "shell", "cwd": "/srv"}}));
    assert_eq!(rust[8], json!({"kind": "ssh-tab", "host": "x"}));
    assert_eq!(rust[9], json!({"kind": "shell"}));
    let code = format!(
        "import json\nprint(json.dumps([dash.read_tab_metadata()] + \
         [dash.tab_metadata_for_session(s) for s in json.loads({:?})]))",
        serde_json::to_string(&sessions).unwrap()
    );
    let text = frozen_dash(&home, &code);
    let python: Value = serde_json::from_str(text.trim()).unwrap();
    assert_eq!(
        response_dumps(&json!(rust)).unwrap(),
        response_dumps(&python).unwrap()
    );
}

/// `write_tab_metadata` (validación, host, cwd absoluto, orden de claves) y
/// `remove_tab_metadata` (solo escribe si la clave estaba; el resto conserva su
/// orden).
#[tokio::test]
async fn write_and_remove_metadata_match_python() {
    let Some((a, b)) = pair("reg-wr", false) else {
        return;
    };
    let native = native_for(&a).await;
    let mut rust = Vec::new();
    for (sess, kind, host, cwd) in [
        ("a b", "shell", "", ""),
        ("uno", "nave", "", ""),
        ("uno", "ssh", "h", "rel/x"),
        ("dos", "project", "", "/srv/dos"),
        ("tres", "scratch", "", ""),
        ("otra", "project", "", "/srv/otra"),
    ] {
        rust.push(value(
            reg::write_tab_metadata(&native, sess, kind, host, cwd)
                .await
                .unwrap(),
        ));
    }
    reg::remove_tab_metadata(&native, "uno").await.unwrap();
    let meta = a.hooks().join("app-tabs-meta.json");
    let stamp = std::fs::metadata(&meta).unwrap().modified().unwrap();
    std::thread::sleep(Duration::from_millis(20));
    reg::remove_tab_metadata(&native, "nada").await.unwrap();
    assert_eq!(
        stamp,
        std::fs::metadata(&meta).unwrap().modified().unwrap(),
        "quitar una clave ausente no escribe"
    );
    assert_eq!(rust[0], Value::Null);
    assert_eq!(rust[1], Value::Null);
    assert_eq!(rust[2], json!({"kind": "ssh", "host": "h"}));
    let code = r#"
import json
_out = []
for s, k, h, c in [("a b", "shell", "", ""), ("uno", "nave", "", ""), ("uno", "ssh", "h", "rel/x"),
                   ("dos", "project", "", "/srv/dos"), ("tres", "scratch", "", ""),
                   ("otra", "project", "", "/srv/otra")]:
    _out.append(dash.write_tab_metadata(s, k, host=h, cwd=c))
dash.remove_tab_metadata("uno")
dash.remove_tab_metadata("nada")
print(json.dumps(_out))
"#;
    let text = frozen_dash(&b, code);
    let python: Value = serde_json::from_str(text.trim()).unwrap();
    assert_eq!(json!(rust), python);
    assert_files_equal(&a, &b);
}

/// `write_app_tab` (etiqueta vacía → sesión, sin recorte) y `register_app_tab`
/// (`local` no se toca, una pestaña existente conserva su etiqueta, 80
/// caracteres, `meta` derivada y guardada si falta).
#[tokio::test]
async fn app_tab_writers_match_python() {
    let Some((a, b)) = pair("reg-app", false) else {
        return;
    };
    let native = native_for(&a).await;
    let long = "ñ".repeat(100);
    reg::write_app_tab(&native, "n1", "").await.unwrap();
    reg::write_app_tab(&native, "n2", &long).await.unwrap();
    reg::register_app_tab(&native, "local", Some("X"), "shell", "", "")
        .await
        .unwrap();
    reg::register_app_tab(&native, "otra", Some("Cambiada"), "", "", "")
        .await
        .unwrap();
    reg::register_app_tab(&native, "n3", Some(&long), "ssh", "x", "/a")
        .await
        .unwrap();
    reg::register_app_tab(&native, "p2f", Some(""), "", "", "")
        .await
        .unwrap();
    let tabs: Value =
        serde_json::from_str(&std::fs::read_to_string(a.hooks().join("app-tabs.json")).unwrap())
            .unwrap();
    assert_eq!(tabs["n1"], "n1");
    assert_eq!(tabs["n2"].as_str().unwrap().chars().count(), 100);
    assert_eq!(tabs["n3"].as_str().unwrap().chars().count(), 80);
    assert_eq!(tabs["otra"], "Otra");
    assert_eq!(tabs["local"], "local");
    let code = format!(
        "dash.write_app_tab('n1', '')\ndash.write_app_tab('n2', {long:?})\n\
         dash.register_app_tab('local', 'X', kind='shell')\n\
         dash.register_app_tab('otra', 'Cambiada')\n\
         dash.register_app_tab('n3', {long:?}, kind='ssh', host='x', cwd='/a')\n\
         dash.register_app_tab('p2f', '')\n"
    );
    frozen_dash(&b, &code);
    assert_files_equal(&a, &b);
}

/// `remember_tab`: sesión inválida ignorada, recortes (16, 32, 80), `cwd`
/// relativo vacío, sin duplicados y como mucho 80 entradas.
#[tokio::test]
async fn remember_tab_rules_match_python() {
    let Some((a, b)) = pair("reg-hist", false) else {
        return;
    };
    let many: Vec<Value> = (0..85)
        .map(|i| json!({"session": format!("s{i}"), "label": format!("S {i}"), "ts": i}))
        .collect();
    for home in [&a, &b] {
        home.write(
            "app-tabs-history.json",
            &Value::Array(many.clone()).to_string(),
        );
    }
    let native = native_for(&a).await;
    let agent = "agente-con-un-nombre-largo";
    let reason = "r".repeat(40);
    reg::remember_tab(&native, "a b", Some("X"), "/x", "", "closed")
        .await
        .unwrap();
    reg::remember_tab(&native, "s3", None, "rel", agent, &reason)
        .await
        .unwrap();
    reg::remember_tab(&native, "nueva", Some(""), "/n", "", "recovered")
        .await
        .unwrap();
    let history: Value = serde_json::from_str(
        &std::fs::read_to_string(a.hooks().join("app-tabs-history.json")).unwrap(),
    )
    .unwrap();
    let items = history.as_array().unwrap();
    assert_eq!(items.len(), 80);
    assert_eq!(items[0]["session"], "nueva");
    assert_eq!(items[0]["label"], "nueva");
    assert_eq!(items[1]["session"], "s3");
    assert_eq!(items[1]["agent"].as_str().unwrap().len(), 16);
    assert_eq!(items[1]["reason"].as_str().unwrap().len(), 32);
    assert_eq!(items[1]["cwd"], "");
    let code = format!(
        "dash.remember_tab('a b', 'X', '/x', '', reason='closed')\n\
         dash.remember_tab('s3', None, 'rel', {agent:?}, reason={reason:?})\n\
         dash.remember_tab('nueva', '', '/n', '', reason='recovered')\n"
    );
    frozen_dash(&b, &code);
    assert_files_equal(&a, &b);
}

#[tokio::test]
async fn ephemeral_and_local_close_nothing() {
    let Some((a, b)) = pair("reg-eph", true) else {
        return;
    };
    let native = native_for(&a).await;
    assert_eq!(
        reg::close_app_tab(&native, "local", false)
            .await
            .unwrap()
            .as_deref(),
        Some("La pestaña local permanece abierta")
    );
    assert_eq!(
        reg::close_app_tab(&native, "p2f", true)
            .await
            .unwrap()
            .as_deref(),
        Some("ephemeral requiere comandos-e2e-")
    );
    assert!(!a.hooks().join("app-tab-close.json").exists());
    // Una efímera de verdad se quita sin entrar al historial ni tocar tmux.
    reg::write_app_tab(&native, "comandos-e2e-1", "E2E")
        .await
        .unwrap();
    let history = std::fs::read(a.hooks().join("app-tabs-history.json")).unwrap();
    TmuxLog(&a).clear();
    assert_eq!(
        reg::close_app_tab(&native, "comandos-e2e-1", true)
            .await
            .unwrap(),
        None
    );
    assert!(TmuxLog(&a).take().is_empty(), "una efímera no lee tmux");
    assert_eq!(
        history,
        std::fs::read(a.hooks().join("app-tabs-history.json")).unwrap()
    );
    let code = "import json\nprint(json.dumps([dash.close_app_tab('local'), \
                dash.close_app_tab('p2f', ephemeral=True)]))\n\
                dash.write_app_tab('comandos-e2e-1', 'E2E')\n\
                dash.close_app_tab('comandos-e2e-1', ephemeral=True)\n";
    let text = frozen_dash(&b, code);
    let python: Value = serde_json::from_str(text.trim()).unwrap();
    assert_eq!(
        python,
        json!([
            "La pestaña local permanece abierta",
            "ephemeral requiere comandos-e2e-"
        ])
    );
    assert_files_equal(&a, &b);
}

#[tokio::test]
async fn unsure_registry_is_500_not_decline() {
    let home = TestHome::new_short("reg-unsure");
    seed_registry(&home);
    let deep = format!("{}{}", "[".repeat(1100), "]".repeat(1100));
    home.write("app-tabs-meta.json", &deep);
    let meta = home.hooks().join("app-tabs-meta.json");
    assert!(matches!(
        reg::read_tab_metadata(&meta),
        Err(RegistryError::Unsure(_))
    ));
    let native = native_for(&home).await;
    let err = reg::write_tab_metadata(&native, "uno", "shell", "", "")
        .await
        .unwrap_err();
    assert!(matches!(
        err.into_fault("POST /tab-metadata"),
        Fault::Error(HandlerError::Failure)
    ));
    // Nada se escribió.
    assert_eq!(std::fs::read_to_string(&meta).unwrap(), deep);
    // `app-tabs.json` incierto: el registro no lo pisa.
    home.write("app-tabs.json", &deep);
    let err = reg::register_app_tab(&native, "uno", None, "shell", "", "")
        .await
        .unwrap_err();
    assert!(matches!(err, RegistryError::Unsure(_)));
    assert!(matches!(
        Fault::from(err),
        Fault::Error(HandlerError::Failure)
    ));
    assert_eq!(
        std::fs::read_to_string(home.hooks().join("app-tabs.json")).unwrap(),
        deep
    );
}

/// `kind` que no es hashable (lista u objeto): `TypeError` en el Python → 500.
#[tokio::test]
async fn unhashable_kind_is_a_python_error() {
    let home = TestHome::new_short("reg-kind");
    seed_registry(&home);
    home.write("app-tabs-meta.json", r#"{"a": {"kind": ["shell"]}}"#);
    let meta = home.hooks().join("app-tabs-meta.json");
    assert!(matches!(
        reg::read_tab_metadata(&meta),
        Err(RegistryError::Fault(Fault::Error(HandlerError::Failure)))
    ));
    let text = frozen_dash(
        &home,
        "try:\n    dash.read_tab_metadata()\n    print('ok')\nexcept TypeError:\n    print('TypeError')",
    );
    assert_eq!(text.trim(), "TypeError");
}

/// cc-app (u otro proceso) tiene el `flock` de `app-tabs.json`: el frente
/// espera sin bloquear el runtime y no pierde la escritura del otro.
#[tokio::test]
async fn waits_for_a_held_flock_and_keeps_the_other_write() {
    let home = TestHome::new_short("reg-lock");
    seed_registry(&home);
    let native = native_for(&home).await;
    let tabs = home.hooks().join("app-tabs.json");
    let (held, release) = std::sync::mpsc::channel();
    let path = tabs.clone();
    let holder = std::thread::spawn(move || {
        let lock = FileLock::acquire(&path).unwrap();
        held.send(()).unwrap();
        std::thread::sleep(Duration::from_millis(400));
        // Leer-modificar-escribir del «cc-app» bajo su candado.
        let mut map: serde_json::Map<String, Value> =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        map.insert("cc-app".into(), json!("CC"));
        std::fs::write(&path, Value::Object(map).to_string()).unwrap();
        drop(lock);
    });
    release.recv().unwrap();
    let started = Instant::now();
    // El runtime sigue libre mientras se espera.
    let ticker = tokio::spawn(async {
        tokio::time::sleep(Duration::from_millis(50)).await;
        Instant::now()
    });
    reg::write_app_tab(&native, "nueva", "Nueva").await.unwrap();
    let finished = Instant::now();
    let ticked = ticker.await.unwrap();
    // Con el runtime de un hilo de `#[tokio::test]`, el temporizador solo
    // corre antes del final si la espera no bloqueó el hilo.
    assert!(
        ticked < finished,
        "la espera del candado bloqueó el runtime"
    );
    assert!(started.elapsed() >= Duration::from_millis(300));
    holder.join().unwrap();
    let map: Value = serde_json::from_str(&std::fs::read_to_string(&tabs).unwrap()).unwrap();
    assert_eq!(map["cc-app"], "CC");
    assert_eq!(map["nueva"], "Nueva");
    assert_eq!(map["otra"], "Otra");
}

/// Revisión de la T1: un `app-tabs-meta.json` incierto es el 500 ANTES de
/// cualquier escritura (registro, cierre normal y cierre efímero).
#[tokio::test]
async fn unsure_meta_fails_before_any_write() {
    let home = TestHome::new_short("reg-unsure-meta");
    seed_registry(&home);
    home.write(
        "app-tabs.json",
        r#"{"otra": "Otra", "comandos-e2e-x": "E"}"#,
    );
    let deep = format!("{}{}", "[".repeat(1100), "]".repeat(1100));
    home.write("app-tabs-meta.json", &deep);
    let before: Vec<Option<String>> = REGISTRY_FILES
        .iter()
        .map(|name| std::fs::read_to_string(home.hooks().join(name)).ok())
        .collect();
    let native = native_for(&home).await;
    for kind in ["shell", "rara"] {
        let err = reg::register_app_tab(&native, "uno", Some("Uno"), kind, "", "")
            .await
            .unwrap_err();
        assert!(matches!(err, RegistryError::Unsure(_)), "{kind}");
    }
    let err = reg::close_app_tab(&native, "otra", false)
        .await
        .unwrap_err();
    assert!(matches!(err, RegistryError::Unsure(_)));
    let err = reg::close_app_tab(&native, "comandos-e2e-x", true)
        .await
        .unwrap_err();
    assert!(matches!(err, RegistryError::Unsure(_)));
    let after: Vec<Option<String>> = REGISTRY_FILES
        .iter()
        .map(|name| std::fs::read_to_string(home.hooks().join(name)).ok())
        .collect();
    assert_eq!(before, after, "nada se escribió");
    // Ni una orden de tmux: las lecturas fallan antes.
    assert!(TmuxLog(&home).read().is_empty());
}

/// Revisión de la T1: el futuro de `write_tab_metadata` se suelta tras lanzar
/// su trabajo de bloqueo; el candado viaja con el trabajo, así que el segundo
/// escritor espera a que termine y ninguna entrada se pierde.
#[tokio::test]
async fn dropped_metadata_write_keeps_its_lock() {
    let home = TestHome::new_short("reg-meta-drop");
    seed_registry(&home);
    // Un archivo grande: leerlo y reescribirlo lleva su tiempo.
    let mut big = serde_json::Map::new();
    for i in 0..60_000 {
        big.insert(format!("s{i}"), json!({"kind": "shell", "cwd": "/srv/a/b"}));
    }
    home.write("app-tabs-meta.json", &Value::Object(big).to_string());
    let native = native_for(&home).await;
    for round in 0..3 {
        let (first, second) = (format!("uno{round}"), format!("dos{round}"));
        let mut pending = Box::pin(reg::write_tab_metadata(&native, &first, "shell", "", "/u"));
        // Se sondea 20 ms (toma el candado y lanza el trabajo, que con este
        // archivo tarda más) y se suelta a mitad de la escritura.
        tokio::select! {
            biased;
            _ = &mut pending => {}
            () = tokio::time::sleep(Duration::from_millis(20)) => {}
        }
        drop(pending);
        reg::write_tab_metadata(&native, &second, "shell", "", "/d")
            .await
            .unwrap();
        let meta: Value = serde_json::from_str(
            &std::fs::read_to_string(home.hooks().join("app-tabs-meta.json")).unwrap(),
        )
        .unwrap();
        assert!(meta.get(&first).is_some(), "{first} perdida");
        assert!(meta.get(&second).is_some(), "{second} perdida");
    }
}

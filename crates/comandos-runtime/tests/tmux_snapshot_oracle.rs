//! `tmux_snapshot::{capture_session, remap_layout, layout_checksum}` y
//! `closed_panes::save` contra `lib/tmux_snapshot.py` y
//! `save_closed_pane_snapshot` de `bin/cc-dash`, sobre un tmux privado.
//!
//! Confinamiento: todo tmux de esta prueba va con `-f /dev/null -S <dir>`
//! (`PrivateTmux`), también el del Python (callback con el mismo `-S` o el
//! guardián del `fakebin`); el `Drop` mata solo ese servidor y después borra.
#[path = "support/private_tmux.rs"]
mod private_tmux;
#[path = "support/python.rs"]
mod python;

use comandos_core::json::response_dumps;
use comandos_runtime::{
    closed_panes,
    pane_snapshot::PaneInspector,
    pane_typing::TmuxResult,
    tmux_snapshot::{self, SnapshotError, capture_session_at, layout_checksum, remap_layout},
};
use private_tmux::PrivateTmux;
use python::run_python;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    ffi::OsStr,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::Path,
    time::{Duration, Instant},
};

const NOW: f64 = 1_791_115_200.75;

const CAPTURE: &str = r#"
import json, os, subprocess, sys, time
repo, tmux_bin, socket, now = sys.argv[1:5]
sys.path.insert(0, os.path.join(repo, "lib"))
import pane_snapshot, tmux_snapshot
time.time = lambda: float(now)
env = {k: v for k, v in os.environ.items() if k != "TMUX"}
tmux = lambda *a, timeout=5: subprocess.run([tmux_bin, '-f', '/dev/null', '-S', socket, *a],
                                            capture_output=True, text=True, timeout=timeout, env=env)
print(json.dumps(tmux_snapshot.capture_session(tmux, 's1', pane_snapshot.PaneInspector())))
"#;

const REMAP: &str = r#"
import json, os, sys
repo, case = sys.argv[1:3]
sys.path.insert(0, os.path.join(repo, "lib"))
import tmux_snapshot
c = json.load(open(case))
print(json.dumps([tmux_snapshot.remap_layout(l, c["map"]) for l in c["layouts"]]))
"#;

const LOAD_DASH: &str = r#"
import importlib.machinery, importlib.util, json, os, sys, time, types, uuid
repo = sys.argv[1]
sys.path.insert(0, os.path.join(repo, "bin"))
loader = importlib.machinery.SourceFileLoader("cc_dash_oracle", os.path.join(repo, "bin/cc-dash"))
spec = importlib.util.spec_from_loader(loader.name, loader)
dash = importlib.util.module_from_spec(spec)
loader.exec_module(dash)
"#;

/// `save_closed_pane_snapshot` con `time.time_ns`, `uuid.uuid4` y `time.time`
/// fijos. Su `tmux` es el del módulo: `tmux` del `PATH` = el guardián.
const SAVE: &str = r#"
calls = json.loads(sys.argv[2])
names = []
for ns, hexv, now in calls:
    time.time_ns = lambda ns=ns: ns
    time.time = lambda now=now: now
    uuid.uuid4 = lambda hexv=hexv: types.SimpleNamespace(hex=hexv)
    names.append(dash.save_closed_pane_snapshot('s1', '%0'))
print(json.dumps(names))
"#;

fn callback(pt: &PrivateTmux) -> impl FnMut(&[&str]) -> tmux_snapshot::Result<TmuxResult> + '_ {
    move |args| {
        let out = pt
            .command()
            .args(args)
            .output()
            .map_err(SnapshotError::Io)?;
        Ok(TmuxResult {
            returncode: out.status.code().unwrap_or(1),
            stdout: String::from_utf8(out.stdout).map_err(|_| SnapshotError::Unsure)?,
            stderr: String::from_utf8(out.stderr).map_err(|_| SnapshotError::Unsure)?,
        })
    }
}

/// Sesión `s1`: ventana 0 con tres panes `cat` (uno con `@comandos-pane-key`
/// y nombre no ASCII), ventana 1 con uno. Todo en el servidor privado.
fn seed(pt: &PrivateTmux) {
    let work = pt.dir.join("trabajo");
    std::fs::create_dir_all(&work).unwrap();
    let work = work.to_str().unwrap();
    pt.run(&[
        "new-session",
        "-d",
        "-s",
        "s1",
        "-x",
        "120",
        "-y",
        "40",
        "-n",
        "ventaña",
        "-c",
        work,
        "cat",
    ]);
    pt.run(&["split-window", "-h", "-t", "=s1:0", "-c", work, "cat"]);
    pt.run(&["split-window", "-v", "-t", "%1", "-c", work, "cat"]);
    pt.run(&[
        "new-window",
        "-d",
        "-t",
        "=s1",
        "-n",
        "segunda",
        "-c",
        work,
        "cat",
    ]);
    pt.run(&[
        "set-option",
        "-p",
        "-t",
        "%0",
        "@comandos-pane-key",
        "pk-uno",
    ]);
    pt.run(&["send-keys", "-t", "%0", "hola ñ", "Enter"]);
    // Espera al eco de `cat` para que las dos capturas vean lo mismo.
    let started = Instant::now();
    loop {
        let text = pt.run(&["capture-pane", "-p", "-J", "-t", "%0"]);
        if text.matches("hola ñ").count() >= 2 {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "cat no hizo eco"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn checksum_matches_tmux() {
    let Some(pt) = PrivateTmux::new("csum") else {
        return;
    };
    seed(&pt);
    // Layouts reales de tmux: los 4 hex son el checksum del resto.
    for layout in pt
        .run(&["list-windows", "-t", "=s1", "-F", "#{window_layout}"])
        .lines()
    {
        let (sum, body) = layout.split_once(',').unwrap();
        assert_eq!(format!("{:04x}", layout_checksum(body)), sum, "{layout}");
    }
}

#[test]
fn capture_session_matches_python() {
    let Some(pt) = PrivateTmux::new("capture") else {
        return;
    };
    seed(&pt);
    let inspector = PaneInspector::new(&pt.home(), Path::new("/proc")).unwrap();
    let mut tmux = callback(&pt);
    let rust = capture_session_at(&mut tmux, "s1", &inspector, NOW.trunc() as i64).unwrap();
    let socket = pt.socket();
    let Some(expected) = run_python(
        CAPTURE,
        &[
            pt.tmux_path().as_os_str(),
            socket.as_os_str(),
            OsStr::new(&NOW.to_string()),
        ],
        &pt.home(),
    ) else {
        return;
    };
    assert_eq!(response_dumps(&rust).unwrap(), expected.trim_end());
    // La ventana 0 tiene la clave etiquetada y el nombre no ASCII.
    let first = &rust["windows"][0];
    assert_eq!(first["name"], "ventaña");
    assert_eq!(first["panes"][0]["tagged_key"], "pk-uno");
    assert_eq!(rust["windows"].as_array().unwrap().len(), 2);
}

#[test]
fn capture_session_errors_like_python() {
    let Some(pt) = PrivateTmux::new("capture-err") else {
        return;
    };
    seed(&pt);
    let inspector = PaneInspector::new(&pt.home(), Path::new("/proc")).unwrap();
    // Sesión inexistente: `_checked` con el stderr de tmux.
    let mut tmux = callback(&pt);
    match capture_session_at(&mut tmux, "nada", &inspector, 0) {
        Err(SnapshotError::Runtime(text)) => {
            assert!(text.starts_with("tmux list-windows: "), "{text}");
            assert!(text.contains("nada"), "{text}");
        }
        other => panic!("{other:?}"),
    }
    // Un layout que cambia entre la lista y la comprobación final.
    let mut real = callback(&pt);
    let mut resized = |args: &[&str]| {
        let mut out = real(args)?;
        if args.first() == Some(&"display-message") {
            out.stdout = "0000,1x1,0,0,0\n".into();
        }
        Ok(out)
    };
    match capture_session_at(&mut resized, "s1", &inspector, 0) {
        Err(SnapshotError::Runtime(text)) => assert_eq!(text, "Window resized during capture"),
        other => panic!("{other:?}"),
    }
    // Una fila de ventana con un campo de menos: el `ValueError` del Python.
    let mut short = |args: &[&str]| {
        Ok(TmuxResult {
            returncode: 0,
            stdout: if args.first() == Some(&"list-windows") {
                "@0\t0\tx\n".into()
            } else {
                String::new()
            },
            stderr: String::new(),
        })
    };
    match capture_session_at(&mut short, "s1", &inspector, 0) {
        Err(SnapshotError::Value(text)) => {
            assert_eq!(text, "not enough values to unpack (expected 8, got 3)")
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn remap_layout_matches_python() {
    let Some(pt) = PrivateTmux::new("remap") else {
        return;
    };
    seed(&pt);
    pt.run(&["new-window", "-d", "-t", "=s1", "cat"]);
    let layouts: Vec<String> = pt
        .run(&["list-windows", "-t", "=s1", "-F", "#{window_layout}"])
        .lines()
        .map(str::to_owned)
        .collect();
    assert_eq!(layouts.len(), 3);
    let map: BTreeMap<String, String> = (0..8)
        .map(|i| (format!("%{i}"), format!("%{}", i + 100)))
        .collect();
    let case = pt.dir.join("remap.json");
    std::fs::write(
        &case,
        serde_json::json!({"layouts": layouts, "map": map}).to_string(),
    )
    .unwrap();
    let Some(expected) = run_python(REMAP, &[case.as_os_str()], &pt.home()) else {
        return;
    };
    let rust: Vec<Value> = layouts
        .iter()
        .map(|l| Value::from(remap_layout(l, &map).unwrap()))
        .collect();
    assert_eq!(
        response_dumps(&Value::Array(rust)).unwrap(),
        expected.trim_end()
    );
    // Identidad: con los mismos ids el layout no cambia (`valid_snapshot`).
    let same: BTreeMap<String, String> =
        (0..8).map(|i| (format!("%{i}"), format!("%{i}"))).collect();
    for layout in &layouts {
        assert_eq!(
            remap_layout(layout, &same).as_deref(),
            Some(layout.as_str())
        );
    }
    // `KeyError` e `IndexError` del Python.
    assert_eq!(
        remap_layout(layouts.first().unwrap(), &BTreeMap::new()),
        None
    );
    assert_eq!(remap_layout("sin-coma", &map), None);
}

/// Carpeta de copias con 52 registros previos (más dos que no casan el patrón)
/// y modo 0755, idéntica en los dos HOME.
fn seed_closed(home: &Path) {
    let root = home.join(".local/state/comandos/closed-panes");
    std::fs::create_dir_all(root.parent().unwrap()).unwrap();
    std::fs::DirBuilder::new()
        .mode(0o755)
        .create(&root)
        .unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
    for i in 0..50 {
        std::fs::write(
            root.join(format!("17000000000000000{i:02}-{i:032x}.json")),
            "{}",
        )
        .unwrap();
    }
    for name in ["9-z.json", "1-y.json", "notas.txt", "zz-1.json"] {
        std::fs::write(root.join(name), "{}").unwrap();
    }
}

fn listing(home: &Path) -> Vec<(String, u32)> {
    let root = home.join(".local/state/comandos/closed-panes");
    let mut out: Vec<(String, u32)> = std::fs::read_dir(root)
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            let mode = e.metadata().unwrap().permissions().mode() & 0o777;
            (e.file_name().into_string().unwrap(), mode)
        })
        .collect();
    out.sort();
    out
}

#[test]
fn closed_pane_save_matches_python() {
    let Some(pt) = PrivateTmux::new("closed") else {
        return;
    };
    seed(&pt);
    // Lado Rust: su propio HOME; lado Python: el HOME de `run_python`.
    let home_a = pt.dir.join("a");
    let home_b = pt.home();
    seed_closed(&home_a);
    seed_closed(&home_b);
    let calls = [
        (1_791_115_200_000_000_001i128, "a".repeat(32), NOW),
        (1_791_115_200_000_000_002i128, "b".repeat(32), NOW + 0.5),
    ];
    let inspector = PaneInspector::new(&home_a, Path::new("/proc")).unwrap();
    let mut rust_names = Vec::new();
    for (ns, hex, now) in &calls {
        let mut tmux = callback(&pt);
        rust_names.push(
            closed_panes::save(&home_a, &mut tmux, &inspector, "s1", "%0", *ns, hex, *now).unwrap(),
        );
    }
    // El `tmux` del Python es el guardián con el `-S` de esta prueba.
    pt.install_guard(&home_b.join("fakebin"));
    let plan = serde_json::to_string(
        &calls
            .iter()
            .map(|(ns, hex, now)| serde_json::json!([*ns as i64, hex, now]))
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let Some(expected) = run_python(&format!("{LOAD_DASH}{SAVE}"), &[OsStr::new(&plan)], &home_b)
    else {
        return;
    };
    assert_eq!(
        response_dumps(&serde_json::json!(rust_names)).unwrap(),
        expected.trim_end()
    );
    // Quedan 50 que casan el patrón, más los dos que no lo casan.
    let a = listing(&home_a);
    assert_eq!(a, listing(&home_b));
    assert_eq!(a.len(), 52);
    for gone in ["1-y.json", "1700000000000000000-", "1700000000000000002-"] {
        assert!(!a.iter().any(|(n, _)| n.starts_with(gone)), "{gone}");
    }
    for kept in ["9-z.json", "notas.txt", "zz-1.json", "1700000000000000003-"] {
        assert!(a.iter().any(|(n, _)| n.starts_with(kept)), "{kept}");
    }
    let root_mode = |home: &Path| {
        std::fs::metadata(home.join(".local/state/comandos/closed-panes"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777
    };
    assert_eq!(root_mode(&home_a), 0o700);
    assert_eq!(root_mode(&home_b), 0o700);
    for name in &rust_names {
        let path = |home: &Path| home.join(".local/state/comandos/closed-panes").join(name);
        let rust = std::fs::read(path(&home_a)).unwrap();
        assert_eq!(rust, std::fs::read(path(&home_b)).unwrap(), "{name}");
        let mode = std::fs::metadata(path(&home_a))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
        assert!(String::from_utf8(rust).unwrap().contains("hola ñ"));
    }
}

#[test]
fn closed_pane_save_refuses_missing_pane() {
    let Some(pt) = PrivateTmux::new("closed-err") else {
        return;
    };
    seed(&pt);
    let home = pt.dir.join("a");
    let inspector = PaneInspector::new(&home, Path::new("/proc")).unwrap();
    let mut tmux = callback(&pt);
    match closed_panes::save(&home, &mut tmux, &inspector, "s1", "%99", 1, "c", 1.0) {
        Err(SnapshotError::Value(text)) => {
            assert_eq!(text, "El panel cambió antes de guardar la copia")
        }
        other => panic!("{other:?}"),
    }
    // `capture-pane` que falla: nada se escribe.
    let mut real = callback(&pt);
    let mut failing = |args: &[&str]| {
        if args.first() == Some(&"capture-pane") {
            return Ok(TmuxResult {
                returncode: 1,
                ..TmuxResult::default()
            });
        }
        real(args)
    };
    match closed_panes::save(&home, &mut failing, &inspector, "s1", "%0", 1, "c", 1.0) {
        Err(SnapshotError::Value(text)) => {
            assert_eq!(text, "No se pudo guardar el texto. El panel sigue abierto")
        }
        other => panic!("{other:?}"),
    }
    assert!(!home.join(".local/state/comandos/closed-panes").exists());
}

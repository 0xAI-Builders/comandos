//! Acciones de los popups (`send_key`, `choose`, `send_text`, `open_session`)
//! y barrido de «te espera» (`stale_waiting`, `waiting_sweep_loop`) contra
//! `bin/cc-notifyd` con un `gi` falso.
//!
//! Confinamiento: el tablero es siempre un servidor HTTP de esta prueba en un
//! puerto efímero (nunca 4777/4778). El argv de tmux se compara con un `tmux`
//! falso que solo anota (generado aquí, nunca el real); las pruebas con tmux
//! real usan un servidor privado con `-S <tempdir>/tmux-<uid>/default`
//! (`PrivateTmux`), que se mata con ese mismo `-S` antes de borrar el
//! directorio. `wmctrl` es siempre un falso que anota. Sin pantalla ni sonido.
mod support;

use comandos_notifyd::actions::{
    self, ALLOWED_KEYS, Effects, Target, TmuxRunner, choose, open_session, send_key, send_text,
};
use comandos_notifyd::dash::DashClient;
use comandos_notifyd::stack::PopupMeta;
use comandos_notifyd::sweep::{self, STATE_TIMEOUT, SWEEP_EVERY, WAITING_GRACE, stale_waiting};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use support::{TempDir, python_eval, tempdir};

// ---------------------------------------------------------------------------
// Tablero falso

/// Lo que recibió el tablero falso: método, ruta y cuerpo.
#[derive(Clone, Debug, PartialEq)]
struct Seen {
    method: String,
    path: String,
    body: String,
}

/// Servidor HTTP de la prueba: responde siempre lo mismo y anota cada petición.
struct FakeDash {
    port: u16,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl FakeDash {
    fn start(replies: Vec<(u16, String)>) -> FakeDash {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        std::thread::spawn(move || {
            for (n, stream) in listener.incoming().enumerate() {
                let Ok(mut stream) = stream else { break };
                let (status, body) = replies
                    .get(n.min(replies.len().saturating_sub(1)))
                    .cloned()
                    .unwrap_or((200, "{}".into()));
                if let Some(req) = read_request(&mut stream) {
                    sink.lock().unwrap().push(req);
                }
                let reply = format!(
                    "HTTP/1.0 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(reply.as_bytes());
            }
        });
        FakeDash { port, seen }
    }

    fn ok() -> FakeDash {
        FakeDash::start(vec![(200, "{\"ok\": true}".into())])
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

fn read_request(stream: &mut std::net::TcpStream) -> Option<Seen> {
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let mut raw = Vec::new();
    let mut buf = [0u8; 4096];
    let split = loop {
        if let Some(i) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        let n = stream.read(&mut buf).ok()?;
        if n == 0 {
            return None;
        }
        raw.extend_from_slice(&buf[..n]);
    };
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let length = head
        .lines()
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.trim().parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = raw[split + 4..].to_vec();
    while body.len() < length {
        let n = stream.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&buf[..n]);
    }
    let mut first = head.lines().next()?.split(' ');
    Some(Seen {
        method: first.next()?.to_string(),
        path: first.next()?.to_string(),
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

/// Un puerto sin nadie escuchando: el tablero «caído».
fn dead_url() -> String {
    let port = TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    format!("http://127.0.0.1:{port}")
}

// ---------------------------------------------------------------------------
// tmux y wmctrl falsos (solo anotan; nunca el tmux real)

/// `tmux` falso: anota su argv (campos separados por \x1f, órdenes por \x1e) y
/// responde `display-message`/`has-session` según `panes`/`sessions`. Solo
/// órdenes internas de `sh`: con `PATH` = este directorio no hay nada más.
const FAKE_TMUX: &str = r#"#!/bin/sh
D='@DIR@'
for a in "$@"; do printf '%s\037' "$a"; done >> "$D/tmux.log"
printf '\036' >> "$D/tmux.log"
if [ "$1" = "-S" ]; then shift 2; fi
case "$1" in
display-message)
  while IFS= read -r p; do if [ "$4" = "$p" ]; then printf '%s\n' "$p"; fi; done < "$D/panes"
  exit 0;;
has-session)
  while IFS= read -r s; do if [ "$3" = "=$s" ]; then exit 0; fi; done < "$D/sessions"
  exit 1;;
esac
exit 0
"#;

const FAKE_WMCTRL: &str = r#"#!/bin/sh
D='@DIR@'
for a in "$@"; do printf '%s\037' "$a"; done >> "$D/wmctrl.log"
printf '\036' >> "$D/wmctrl.log"
exit 0
"#;

/// Directorio con `tmux` y `wmctrl` falsos; vivos: pane `%1` y sesión `s`.
struct FakeBin {
    dir: TempDir,
}

impl FakeBin {
    fn new() -> FakeBin {
        let dir = tempdir();
        let path = dir.path().to_string_lossy().into_owned();
        assert!(!path.contains('\''));
        for (name, text) in [("tmux", FAKE_TMUX), ("wmctrl", FAKE_WMCTRL)] {
            let file = dir.path().join(name);
            std::fs::write(&file, text.replace("@DIR@", &path)).unwrap();
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        std::fs::write(dir.path().join("panes"), "%1\n").unwrap();
        std::fs::write(dir.path().join("sessions"), "s\n").unwrap();
        FakeBin { dir }
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Lee y vacía los registros: `(tmux, wmctrl)`.
    fn take(&self) -> (String, String) {
        let mut out = Vec::new();
        for name in ["tmux.log", "wmctrl.log"] {
            let file = self.dir.path().join(name);
            out.push(std::fs::read_to_string(&file).unwrap_or_default());
            let _ = std::fs::remove_file(&file);
        }
        (out.remove(0), out.remove(0))
    }

    fn effects(&self, dash_url: &str, socket: Option<PathBuf>) -> Effects {
        Effects {
            dash: DashClient::parse(dash_url).unwrap(),
            tmux: TmuxRunner::with_program(self.path().join("tmux").into_os_string(), socket),
            wmctrl: self.path().join("wmctrl").into_os_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Servidor tmux privado (`-S` propio; nunca el del usuario)

fn tmux_binary() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join("tmux"))
        .find(|p| p.is_file())
}

/// Servidor tmux de la prueba con sesión `s` de dos panes `cat`. Todo con
/// `-S <dir>/tmux-<uid>/default`; `Drop` hace `kill-server` con ese `-S` y
/// solo después se borra el directorio.
struct PrivateTmux {
    socket: PathBuf,
    binary: PathBuf,
    dir: TempDir,
}

impl PrivateTmux {
    fn start() -> Option<PrivateTmux> {
        let Some(binary) = tmux_binary() else {
            eprintln!("aviso: sin tmux; se omite la prueba con servidor privado");
            return None;
        };
        let dir = tempdir();
        let uid = std::fs::metadata(dir.path()).unwrap().uid();
        let parent = dir.path().join(format!("tmux-{uid}"));
        std::fs::create_dir_all(&parent).unwrap();
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o700)).unwrap();
        let tmux = PrivateTmux {
            socket: parent.join("default"),
            binary,
            dir,
        };
        tmux.run(&[
            "new-session",
            "-d",
            "-s",
            "s",
            "-x",
            "80",
            "-y",
            "24",
            "cat",
        ]);
        tmux.run(&["split-window", "-t", "=s:", "cat"]);
        Some(tmux)
    }

    /// `tmux -f /dev/null -S <socket> …` con un entorno mínimo (sin `TMUX`,
    /// HOME temporal y `sh` como shell: nada lee la configuración del usuario).
    fn command(&self) -> Command {
        let mut c = Command::new(&self.binary);
        c.arg("-f")
            .arg("/dev/null")
            .arg("-S")
            .arg(&self.socket)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.dir.path())
            .env("SHELL", "/bin/sh")
            .env("TERM", "screen")
            .env("TMUX_TMPDIR", self.dir.path())
            .stdin(Stdio::null());
        c
    }

    fn run(&self, args: &[&str]) -> String {
        let out = self.command().args(args).output().unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn panes(&self) -> Vec<String> {
        self.run(&["list-panes", "-t", "=s:", "-F", "#{pane_id}"])
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn active(&self) -> String {
        self.run(&["display-message", "-p", "-t", "=s:", "#{pane_id}"])
            .trim()
            .to_string()
    }

    fn capture(&self, pane: &str) -> String {
        self.run(&["capture-pane", "-p", "-t", pane])
            .trim()
            .to_string()
    }

    /// Espera hasta 3 s a que el pane muestre `want`.
    fn wait_for(&self, pane: &str, want: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let seen = self.capture(pane);
            if seen.contains(want) || Instant::now() >= deadline {
                return seen;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Efectos de producción con `--tmux-socket` apuntando a este servidor.
    fn effects(&self, dash_url: &str, fake: &FakeBin) -> Effects {
        let effects = Effects {
            dash: DashClient::parse(dash_url).unwrap(),
            tmux: TmuxRunner::with_socket(self.socket.clone()),
            wmctrl: fake.path().join("wmctrl").into_os_string(),
        };
        // Salvaguarda: toda orden lleva el `-S` de este servidor.
        let argv = effects.tmux.argv(&["has-session"]);
        assert_eq!(argv.first().map(|a| a.as_os_str()), Some("-S".as_ref()));
        assert_eq!(argv.get(1).map(PathBuf::from), Some(self.socket.clone()));
        effects
    }
}

impl Drop for PrivateTmux {
    fn drop(&mut self) {
        // Primero kill-server con nuestro `-S`; el directorio se borra después
        // (al soltarse `dir`).
        let _ = self
            .command()
            .arg("kill-server")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

fn target(session: &str, pane: &str) -> Target {
    Target {
        session: session.into(),
        pane: pane.into(),
    }
}

// ---------------------------------------------------------------------------
// Pruebas

#[test]
fn allowed_keys_are_python_ones() {
    assert_eq!(ALLOWED_KEYS, ["Enter", "Escape", "1", "2", "3"]);
}

#[test]
fn key_goes_through_dash() {
    let dash = FakeDash::ok();
    let fake = FakeBin::new();
    let fx = fake.effects(&dash.url(), None);
    send_key(&fx, &target("s", "%1"), "1");
    let seen = dash.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].method, "POST");
    assert_eq!(seen[0].path, "/key");
    // `json.dumps` del Python, byte a byte.
    assert_eq!(
        seen[0].body,
        r#"{"session": "s", "key": "1", "pane": "%1"}"#
    );
    assert_eq!(
        serde_json::from_str::<Value>(&seen[0].body).unwrap(),
        json!({"session": "s", "key": "1", "pane": "%1"})
    );
    // Con el tablero arriba, tmux ni se toca.
    assert_eq!(fake.take(), (String::new(), String::new()));
}

#[test]
fn key_falls_back_to_exact_pane() {
    let Some(tmux) = PrivateTmux::start() else {
        return;
    };
    let fake = FakeBin::new();
    let panes = tmux.panes();
    assert_eq!(panes.len(), 2, "{panes:?}");
    let active = tmux.active();
    let other = panes.iter().find(|p| **p != active).unwrap().clone();
    let fx = tmux.effects(&dead_url(), &fake);
    choose(&fx, &target("s", &other), "1");
    // `1` y `Enter`: la terminal lo repite y `cat` lo devuelve.
    let seen = tmux.wait_for(&other, "1\n1");
    assert_eq!(seen, "1\n1");
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(tmux.capture(&active), "", "la tecla llegó al pane activo");
}

#[test]
fn text_without_pane_goes_to_active() {
    let Some(tmux) = PrivateTmux::start() else {
        return;
    };
    let fake = FakeBin::new();
    let active = tmux.active();
    let fx = tmux.effects(&dead_url(), &fake);
    send_text(&fx, &target("s", ""), "hola");
    assert_eq!(tmux.wait_for(&active, "hola\nhola"), "hola\nhola");
}

#[test]
fn dead_pane_types_nothing() {
    let Some(tmux) = PrivateTmux::start() else {
        return;
    };
    let fake = FakeBin::new();
    let fx = tmux.effects(&dead_url(), &fake);
    send_key(&fx, &target("s", "%999"), "1");
    send_text(&fx, &target("s", "%999"), "nada");
    // Sesión inexistente sin pane: tampoco.
    send_text(&fx, &target("otra", ""), "nada");
    std::thread::sleep(Duration::from_millis(400));
    for pane in tmux.panes() {
        assert_eq!(tmux.capture(&pane), "", "pane {pane} cambió");
    }
}

#[test]
fn text_fallback_is_literal() {
    let Some(tmux) = PrivateTmux::start() else {
        return;
    };
    let fake = FakeBin::new();
    let pane = tmux.panes().first().unwrap().clone();
    let fx = tmux.effects(&dead_url(), &fake);
    let text = "echo $(touch X); ls ; Enter C-c";
    send_text(&fx, &target("s", &pane), &format!("  {text}\t\n"));
    let want = format!("{text}\n{text}");
    assert_eq!(tmux.wait_for(&pane, &want), want);
}

#[test]
fn invalid_key_does_nothing() {
    let dash = FakeDash::ok();
    let fake = FakeBin::new();
    for url in [dash.url(), dead_url()] {
        let fx = fake.effects(&url, None);
        send_key(&fx, &target("s", "%1"), "x");
        send_key(&fx, &target("s", "%1"), "Tab");
        send_key(&fx, &target("s", "%1"), "enter");
        send_key(&fx, &target("a b", ""), "1");
        send_key(&fx, &target("", ""), "1");
        send_key(&fx, &target("s", "%x"), "1");
        send_key(&fx, &target("s", "1"), "1");
        send_text(&fx, &target("s", "%1"), " \n\t ");
        send_text(&fx, &target("a;b", ""), "hola");
        open_session(&fx, &target("", ""));
        open_session(&fx, &target(&"x".repeat(81), ""));
    }
    assert!(dash.seen().is_empty(), "{:?}", dash.seen());
    assert_eq!(fake.take(), (String::new(), String::new()));
}

#[test]
fn socket_goes_first_in_argv() {
    let fake = FakeBin::new();
    let socket = fake.path().join("tmux-1/default");
    let fx = fake.effects(&dead_url(), Some(socket.clone()));
    send_key(&fx, &target("s", "%1"), "2");
    let s = socket.to_string_lossy();
    let (tmux, _) = fake.take();
    assert_eq!(
        tmux,
        format!(
            "-S\x1f{s}\x1fdisplay-message\x1f-p\x1f-t\x1f%1\x1f#{{pane_id}}\x1f\x1e\
             -S\x1f{s}\x1fsend-keys\x1f-t\x1f%1\x1f2\x1f\x1e"
        )
    );
}

/// Escenarios del diferencial de acciones: función y argumentos.
fn scenarios() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        ("send_key", vec!["s", "1", "%1"]),
        ("send_key", vec!["s", "Enter", "%2"]),
        ("send_key", vec!["s", "Escape", ""]),
        ("send_key", vec!["gone", "2", ""]),
        ("send_key", vec!["s", "x", "%1"]),
        ("send_key", vec!["a b", "1", ""]),
        ("send_key", vec!["s", "1", "%x"]),
        ("send_key", vec!["s\n", "3", ""]),
        ("choose", vec!["s", "2", "%1"]),
        ("choose", vec!["s", "3", ""]),
        ("send_text", vec!["s", "  hola; $(rm -rf x) Enter  ", "%1"]),
        ("send_text", vec!["s", "   ", "%1"]),
        ("send_text", vec!["s", "ñandú «x» \u{2028}", ""]),
        ("send_text", vec!["gone", "hola", ""]),
        ("send_text", vec!["s", "hola", "%9"]),
        ("open_session", vec!["s", "%1"]),
        ("open_session", vec!["s", "%x"]),
        ("open_session", vec!["a b", ""]),
        ("open_session", vec!["gone", ""]),
    ]
}

/// El Python: `urlopen` redirigido del 4777 al tablero de la prueba (cualquier
/// otra URL lanza) y `GLib.timeout_add` que ejecuta al acto y anota el plazo.
const ACTIONS_PY: &str = r#"
import urllib.request, types
BASE = INPUT['base']
_orig = urllib.request.urlopen
def _uo(req, *a, **k):
    if not isinstance(req, urllib.request.Request):
        raise RuntimeError('solo Request')
    url = req.full_url
    if not url.startswith('http://127.0.0.1:4777/'):
        raise RuntimeError('url ' + url)
    req.full_url = BASE + url[len('http://127.0.0.1:4777'):]
    return _orig(req, *a, **k)
urllib.request.urlopen = _uo
TIMERS = []
def _timeout_add(ms, fn, *a):
    TIMERS.append(ms)
    fn(*a)
    return 0
mod.GLib = types.SimpleNamespace(timeout_add=_timeout_add, idle_add=lambda *a: 0)
def _take():
    res = []
    for name in ('tmux.log', 'wmctrl.log'):
        p = INPUT['logs'] + '/' + name
        try:
            with open(p) as f: res.append(f.read())
            os.remove(p)
        except FileNotFoundError:
            res.append('')
    return res
import os
out = []
for fn, args in INPUT['scenarios']:
    del TIMERS[:]
    getattr(mod, fn)(*args)
    out.append([*_take(), list(TIMERS)])
"#;

fn run_rust(fake: &FakeBin, url: &str) -> Vec<Value> {
    let fx = fake.effects(url, None);
    let mut out = Vec::new();
    for (name, args) in scenarios() {
        let a = |i: usize| args.get(i).copied().unwrap_or_default();
        let mut timers = Vec::new();
        match name {
            "send_key" => send_key(&fx, &target(a(0), a(2)), a(1)),
            "choose" => {
                choose(&fx, &target(a(0), a(2)), a(1));
                timers.push(actions::CHOOSE_DELAY.as_millis() as u64);
            }
            "send_text" => send_text(&fx, &target(a(0), a(2)), a(1)),
            "open_session" => open_session(&fx, &target(a(0), a(1))),
            other => panic!("{other}"),
        }
        let (tmux, wmctrl) = fake.take();
        out.push(json!([tmux, wmctrl, timers]));
    }
    out
}

fn run_python(fake: &FakeBin, url: &str) -> Option<Vec<Value>> {
    let input = json!({
        "base": url,
        "logs": fake.path(),
        "scenarios": scenarios(),
    });
    let expr = format!(
        "(lambda g: (exec({}, g), g['out'])[1])({{'mod': mod, 'json': json, 'INPUT': json.loads({})}})",
        serde_json::to_string(ACTIONS_PY).unwrap(),
        serde_json::to_string(&input.to_string()).unwrap()
    );
    let hooks = tempdir();
    let path = fake.path().to_string_lossy().into_owned();
    let out = python_eval(hooks.path(), &[("PATH", &path)], &expr)?;
    Some(out.as_array().unwrap().clone())
}

/// Mismas peticiones al tablero, mismo argv de tmux y `wmctrl` (sin
/// `--tmux-socket`: el de producción) y mismo plazo de `choose`, con el
/// tablero arriba, respondiendo 500 y caído.
#[test]
fn actions_match_python() {
    for mode in ["up", "500", "down"] {
        let (rust_dash, python_dash) = match mode {
            "up" => (Some(FakeDash::ok()), Some(FakeDash::ok())),
            "500" => (
                Some(FakeDash::start(vec![(500, "{}".into())])),
                Some(FakeDash::start(vec![(500, "{}".into())])),
            ),
            _ => (None, None),
        };
        let url = |d: &Option<FakeDash>| d.as_ref().map_or_else(dead_url, FakeDash::url);
        let python_fake = FakeBin::new();
        let Some(expected) = run_python(&python_fake, &url(&python_dash)) else {
            eprintln!("aviso: sin python3; se omite el diferencial de acciones");
            return;
        };
        let rust_fake = FakeBin::new();
        let got = run_rust(&rust_fake, &url(&rust_dash));
        for (i, (g, e)) in got.iter().zip(&expected).enumerate() {
            assert_eq!(g, e, "modo {mode}, escenario {i}: {:?}", scenarios()[i]);
        }
        assert_eq!(got.len(), expected.len());
        if let (Some(r), Some(p)) = (rust_dash, python_dash) {
            let strip = |v: Vec<Seen>| {
                v.into_iter()
                    .map(|s| (s.method, s.path, s.body))
                    .collect::<Vec<_>>()
            };
            let (r, p) = (strip(r.seen()), strip(p.seen()));
            assert!(!p.is_empty());
            assert_eq!(r, p, "peticiones al tablero, modo {mode}");
        }
    }
}

// ---------------------------------------------------------------------------
// Barrido de «te espera»

fn meta(kind: &str, session: &str, pane: &str, born: f64) -> PopupMeta {
    PopupMeta {
        kind: kind.into(),
        session: session.into(),
        pane: pane.into(),
        born,
    }
}

const STALE_PY: &str = r#"
NS = types.SimpleNamespace
import types
out = []
for case in INPUT:
    wins = [NS(_kind=k, _session=s, _pane=p, _born=b) for k, s, p, b in case['wins']]
    try:
        res = mod.stale_waiting(wins, case['state'], case['now'])
        out.append([wins.index(w) for w in res])
    except Exception:
        out.append([])
"#;

#[test]
fn stale_waiting_matches_python() {
    assert_eq!(WAITING_GRACE, 4.0);
    let wins = vec![
        ("waiting", "s", "", 100.0),
        ("waiting", "s", "%1", 100.0),
        ("waiting", "t", "%2", 100.0),
        ("done", "s", "", 100.0),
        ("waiting", "", "", 100.0),
        ("waiting", "u", "", 103.0),
        ("waiting", "v", "%7", 96.0),
    ];
    let item = |s: Value, p: Value, st: &str| json!({"session": s, "pane": p, "status": st});
    let states: Vec<(Value, f64)> = vec![
        (json!([]), 110.0),
        (json!([]), 103.5),
        (json!([]), 104.0),
        (json!([]), 106.999),
        (json!([item(json!("s"), json!(""), "waiting")]), 110.0),
        (json!([item(json!("s"), json!("%1"), "waiting")]), 110.0),
        (json!([item(json!("s"), json!("%2"), "waiting")]), 110.0),
        (
            json!([
                item(json!("t"), json!("%2"), "waiting"),
                item(json!("u"), Value::Null, "waiting")
            ]),
            110.0,
        ),
        (json!([item(json!("s"), json!("%1"), "working")]), 110.0),
        (
            json!([{"session": "s", "pane": "%1", "status": "waiting", "alive": false}]),
            110.0,
        ),
        (
            json!([{"session": "s", "pane": "%1", "status": "waiting", "alive": 0}]),
            110.0,
        ),
        (
            json!([{"session": "s", "pane": "%1", "status": "waiting", "alive": null}]),
            110.0,
        ),
        (
            json!([{"session": "s", "status": "waiting"}, "raro"]),
            110.0,
        ),
        (json!([item(json!(["s"]), json!(""), "waiting")]), 110.0),
        (json!([item(json!("s"), json!(["%1"]), "waiting")]), 110.0),
        (json!([item(json!("s"), json!([]), "waiting")]), 110.0),
        (json!([item(json!("s"), json!({}), "waiting")]), 110.0),
        (json!([item(json!(1), json!(""), "waiting")]), 110.0),
        (json!([item(json!("v"), json!("%7"), "waiting"), {}]), 110.0),
        (json!([item(json!("s"), json!(0), "waiting")]), 110.0),
        (json!([item(json!("s"), json!("%1"), "Waiting")]), 99.0),
        (json!([{"pane": "%1", "status": "waiting"}]), 110.0),
    ];
    assert!(states.len() >= 20);
    let input: Vec<Value> = states
        .iter()
        .map(|(state, now)| json!({"wins": wins, "state": state, "now": now}))
        .collect();
    let expr = format!(
        "(lambda g: (exec({}, g), g['out'])[1])({{'mod': mod, 'json': json, 'types': __import__('types'), 'INPUT': json.loads({})}})",
        serde_json::to_string(STALE_PY).unwrap(),
        serde_json::to_string(&Value::Array(input).to_string()).unwrap()
    );
    let hooks = tempdir();
    let Some(expected) = python_eval(hooks.path(), &[], &expr) else {
        eprintln!("aviso: sin python3; se omite el diferencial de stale_waiting");
        return;
    };
    let metas: Vec<PopupMeta> = wins.iter().map(|(k, s, p, b)| meta(k, s, p, *b)).collect();
    let got: Vec<Vec<usize>> = states
        .iter()
        .map(|(state, now)| stale_waiting(&metas, state, *now))
        .collect();
    assert_eq!(json!(got), expected);
}

/// `waiting_sweep_loop` con un reloj falso: espera 3 s, no consulta `/state`
/// sin popups «te espera», y solo entrega las respuestas que son listas.
#[test]
fn sweep_run_with_fake_clock() {
    assert_eq!(SWEEP_EVERY, Duration::from_secs(3));
    assert_eq!(STATE_TIMEOUT, Duration::from_secs(2));
    let dash = FakeDash::start(vec![
        (200, r#"[{"session": "s", "status": "waiting"}]"#.into()),
        (200, r#"{"no": "lista"}"#.into()),
        (500, "[]".into()),
        (200, "no es json".into()),
        (200, "[]".into()),
    ]);
    let client = DashClient::parse(&dash.url()).unwrap();
    let waiting = [false, true, true, false, true, true, true];
    let mut ticks = 0usize;
    let mut slept = Vec::new();
    let mut asked = 0usize;
    let mut delivered = Vec::new();
    sweep::run(
        &client,
        |d| {
            slept.push(d);
            ticks += 1;
            ticks <= waiting.len()
        },
        || {
            asked += 1;
            waiting.get(asked - 1).copied().unwrap_or(false)
        },
        |state| delivered.push(state),
    );
    assert_eq!(slept, vec![SWEEP_EVERY; waiting.len() + 1]);
    assert_eq!(asked, waiting.len());
    let seen = dash.seen();
    assert_eq!(seen.len(), 5);
    assert!(seen.iter().all(|s| s.method == "GET" && s.path == "/state"));
    assert_eq!(
        delivered,
        vec![json!([{"session": "s", "status": "waiting"}]), json!([])]
    );
}

#[test]
fn tmux_socket_flag_needs_a_path() {
    let out = Command::new(env!("CARGO_BIN_EXE_comandos-notifyd"))
        .args(["--headless", "--tmux-socket"])
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--tmux-socket necesita una ruta"));
}

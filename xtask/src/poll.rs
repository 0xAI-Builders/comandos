//! Carga sintética: reproduce el polling real del tablero y de la app (inventario §1.11) y
//! mide el Pss/RSS del servidor a lo largo del tiempo.
//!
//! Herramienta de desarrollo. Cliente HTTP a mano sobre `TcpStream`.

use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::TcpStream,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

/// Cliente que genera la carga: el tablero (navegador) o la app de escritorio (`cc-app`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Client {
    Dashboard,
    App,
}

/// (periodo en ms, método, ruta) del tablero: `index.html`, `notifications.js`,
/// `work-marks.js`, `pomodoro.js`, `term.html`. El long-poll `/notices/watch` no entra:
/// es continuo y lo lleva un hilo propio por cliente.
const DASHBOARD: [(u64, &str, &str); 11] = [
    (2_000, "GET", "/state"),
    (10_000, "GET", "/usage/state"),
    (5_000, "GET", "/prefs"),
    (1_000, "GET", "/active-tab"),
    (60_000, "GET", "/analytics/week"),
    (3_000, "GET", "/notices"),
    (5_000, "GET", "/work-marks"),
    (15_000, "GET", "/pomodoro"),
    (30_000, "POST", "/presence"),
    (2_000, "POST", "/terminal-panes"),
    (2_000, "GET", "/tab-models?session=poll"),
];

/// Filas de `cc-app` (inventario §1.11).
const APP: [(u64, &str, &str); 5] = [
    (3_000, "GET", "/state"),
    (3_000, "GET", "/prefs"),
    (2_000, "GET", "/workspace"),
    (5_000, "GET", "/work-marks"),
    (30_000, "POST", "/presence"),
];

/// Peticiones periódicas de un cliente durante `minutes` minutos, ordenadas por instante
/// (ms desde el inicio). Cada ruta arranca en 0 y repite cada su periodo.
pub fn schedule(minutes: u64, client: Client) -> Vec<(u64, &'static str, String)> {
    let total = minutes * 60_000;
    let table: &[(u64, &str, &str)] = match client {
        Client::Dashboard => &DASHBOARD,
        Client::App => &APP,
    };
    let mut out = Vec::new();
    for (period, method, path) in table {
        let mut t = 0;
        while t < total {
            out.push((t, *method, path.to_string()));
            t += period;
        }
    }
    out.sort_by_key(|e| e.0); // estable: dentro de un mismo instante, orden de la tabla
    out
}

/// Pendiente (KiB/h) de mínimos cuadrados sobre los puntos `(minuto, kib)` desde el minuto 5.
/// `None` si quedan menos de dos puntos.
pub fn slope_kib_per_hour(points: &[(u64, u64)]) -> Option<f64> {
    let p: Vec<(f64, f64)> = points
        .iter()
        .filter(|(m, _)| *m >= 5)
        .map(|(m, k)| (*m as f64, *k as f64))
        .collect();
    if p.len() < 2 {
        return None;
    }
    let n = p.len() as f64;
    let (sx, sy) = p.iter().fold((0.0, 0.0), |a, (x, y)| (a.0 + x, a.1 + y));
    let (mx, my) = (sx / n, sy / n);
    let num: f64 = p.iter().map(|(x, y)| (x - mx) * (y - my)).sum();
    let den: f64 = p.iter().map(|(x, _)| (x - mx).powi(2)).sum();
    (den > 0.0).then(|| num / den * 60.0)
}

// ------------------------------------------------------------------- cliente

fn http(
    addr: &str,
    method: &str,
    path: &str,
    token: &str,
    body: Option<&str>,
    timeout: Duration,
) -> Result<(u16, Vec<u8>), String> {
    let mut s = TcpStream::connect(addr).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(timeout)).ok();
    s.set_write_timeout(Some(Duration::from_secs(10))).ok();
    let mut req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nX-Comandos-Token: {token}\r\nConnection: close\r\n"
    );
    if let Some(b) = body {
        req += &format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            b.len()
        );
    }
    req += "\r\n";
    s.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    if let Some(b) = body {
        s.write_all(b.as_bytes()).map_err(|e| e.to_string())?;
    }
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).map_err(|e| e.to_string())?;
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("sin cabeceras")?;
    let head = String::from_utf8_lossy(&raw[..split]).to_string();
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or("sin estado")?;
    let rest = raw[split + 4..].to_vec();
    // Solo hace falta el cuerpo para leer `rev` del long-poll; con chunked se deja crudo
    // (contiene el JSON entero igualmente y `rev` se busca por texto).
    Ok((status, rest))
}

/// Cuerpo de las rutas POST del calendario. `/terminal-panes` lista la sesión
/// `panes`: con `--shadow`, una sesión real con varios panes del tmux privado de
/// la pila; sin él, `poll` (inexistente: nunca se toca una sesión del usuario).
fn body_for(path: &str, panes: &str) -> Option<String> {
    match path {
        "/presence" => Some(
            r#"{"deviceId":"xtask-poll","visible":true,"canPlayAudio":false,"interaction":false}"#
                .to_string(),
        ),
        "/terminal-panes" => Some(panes_list(panes)),
        _ => None,
    }
}

/// `{"session":…,"action":"list"}`, como `term.html` (`refresh`).
pub fn panes_list(session: &str) -> String {
    serde_json::json!({"session": session, "action": "list"}).to_string()
}

/// Sesiones del tmux privado de la pila que carga `--shadow`: la del tablero
/// (`local`, la crea la pila) y la de un segundo iframe remoto.
pub const SHADOW_SESSIONS: [&str; 2] = ["local", "poll-b"];

/// Panes por sesión en `--shadow` (el inventario hace un `display-message` y
/// una lectura de `/proc/<pid>/stat` por pane).
pub const SHADOW_PANES: usize = 3;

/// Agentes falsos de `--shadow` (preflight B15): una sesión por agente cuyo
/// proceso es `/bin/sleep` copiado con ese nombre, para que GET `/state`
/// recorra la observación cara (`display-message`, `capture-pane -S -45`, la
/// pista de Codex, cuentas y el transcript de Claude), no solo panes `cat`.
pub const SHADOW_AGENTS: [(&str, &str); 2] = [("poll-claude", "claude"), ("poll-codex", "codex")];

/// Los `count` agentes falsos de `--shadow`: los dos de `SHADOW_AGENTS` y,
/// con `--agents N`, más sesiones que alternan Claude y Codex
/// (`poll-claude-2`, `poll-codex-3`, …) para acercarse a la carga real
/// (decenas de agentes vivos a la vez).
pub fn shadow_agents(count: usize) -> Vec<(String, &'static str)> {
    (0..count)
        .map(|i| match SHADOW_AGENTS.get(i) {
            Some((session, agent)) => ((*session).to_string(), *agent),
            None if i % 2 == 0 => (format!("poll-claude-{i}"), "claude"),
            None => (format!("poll-codex-{i}"), "codex"),
        })
        .collect()
}

/// Crea las sesiones de `agents` en el tmux privado del frente, una sesión de
/// Claude (`~/.claude/sessions`) con su transcript por cada agente Claude y un
/// registro de estado por agente, todo en el HOME temporal del frente y su
/// heredado. `extra_panes` ventanas `cat` se reparten entre esas sesiones.
fn seed_agents(
    stack: &crate::parity::Stack,
    agents: &[(String, &'static str)],
    extra_panes: usize,
) -> Result<(), String> {
    let home = stack.front_home()?;
    let home_text = home.to_str().ok_or("HOME temporal no UTF-8")?;
    let bin = stack.root().join("agents");
    std::fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
    for kind in ["claude", "codex"] {
        let exe = bin.join(kind);
        std::fs::copy("/bin/sleep", &exe).map_err(|e| format!("{}: {e}", exe.display()))?;
    }
    for (session, agent) in agents {
        let cmd = format!(
            "env -i HOME={home_text} PATH=/usr/bin:/bin {} 86400",
            bin.join(agent).display()
        );
        stack.front_tmux(&["new-session", "-d", "-s", session, "-c", home_text, &cmd])?;
    }
    if !agents.is_empty() {
        for i in 0..extra_panes {
            let (session, _) = &agents[i % agents.len()];
            stack.front_tmux(&["new-window", "-d", "-t", &format!("={session}:"), "cat"])?;
        }
    }
    thread::sleep(Duration::from_millis(300));
    let write = |path: PathBuf, text: String| -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let state = home.join(".claude/hooks/state");
    for (session, agent) in agents {
        if *agent == "claude" {
            let pid = stack.front_tmux_output(&[
                "display-message",
                "-p",
                "-t",
                &format!("={session}:0"),
                "#{pane_pid}",
            ])?;
            let pid: i64 = pid
                .trim()
                .parse()
                .map_err(|_| format!("pane_pid ilegible: {pid:?}"))?;
            // El primero conserva los nombres de siempre (`poll.json`, `poll-conv`).
            let (file, conv) = if session == SHADOW_AGENTS[0].0 {
                ("poll".to_string(), "poll-conv".to_string())
            } else {
                (session.clone(), format!("{session}-conv"))
            };
            write(
                home.join(format!(".claude/sessions/{file}.json")),
                serde_json::json!({"pid": pid, "sessionId": conv, "cwd": home_text}).to_string(),
            )?;
            write(
                home.join(format!(".claude/projects/poll/{conv}.jsonl")),
                format!(
                    "{}\n",
                    serde_json::json!({
                        "type": "assistant", "uuid": "u1", "sessionId": conv,
                        "message": {"model": "claude-sonnet-5"},
                    })
                ),
            )?;
        }
        let (status, detail) = if *agent == "claude" {
            ("working", "poll")
        } else {
            ("waiting", "¿sigo?")
        };
        write(
            state.join(format!("{session}.json")),
            serde_json::json!({
                "session": session, "agent": agent, "status": status,
                "detail": detail, "ts": now,
            })
            .to_string(),
        )?;
    }
    println!(
        "poll: {} agentes falsos ({} Claude) y {extra_panes} ventanas `cat` extra en el tmux privado",
        agents.len(),
        agents.iter().filter(|(_, a)| *a == "claude").count(),
    );
    Ok(())
}

/// `(id, identity)` de cada pane de una respuesta de `/terminal-panes`. El cuerpo
/// puede venir en un solo trozo chunked: se toma del primer `{` al último `}`.
pub fn pane_targets(body: &[u8]) -> Vec<(String, serde_json::Value)> {
    let text = String::from_utf8_lossy(body);
    let (Some(start), Some(end)) = (text.find('{'), text.rfind('}')) else {
        return Vec::new();
    };
    let Some(json) = text.get(start..=end) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    value["panes"]
        .as_array()
        .map(|panes| {
            panes
                .iter()
                .filter_map(|p| Some((p["id"].as_str()?.to_string(), p["identity"].clone())))
                .collect()
        })
        .unwrap_or_default()
}

/// Mezcla de un panel de terminal abierto (`term.html`), solo con `--shadow`:
/// un segundo iframe que lista `poll-b` cada 2 s y, sobre `local`, un `resize`
/// cada 10 s (alternando altura, como arrastrar el borde) y un `select` con
/// `scope:"client"` cada 20 s (como tocar un pane desde el móvil).
fn spawn_pane_mix(
    addr: &str,
    token: &str,
    minutes: u64,
    t0: Instant,
    stats: &Arc<Stats>,
) -> thread::JoinHandle<()> {
    let (addr, token, stats) = (addr.to_string(), token.to_string(), stats.clone());
    let [board, second] = SHADOW_SESSIONS;
    thread::spawn(move || {
        let total = minutes * 60_000;
        let send = |body: &str| -> Option<Vec<u8>> {
            stats.sent.fetch_add(1, Ordering::Relaxed);
            match http(
                &addr,
                "POST",
                "/terminal-panes",
                &token,
                Some(body),
                Duration::from_secs(30),
            ) {
                Ok((st, b)) if (200..300).contains(&st) => Some(b),
                Ok(_) => {
                    stats.non_2xx.fetch_add(1, Ordering::Relaxed);
                    None
                }
                Err(_) => {
                    stats.errors.fetch_add(1, Ordering::Relaxed);
                    None
                }
            }
        };
        let mut tick: u64 = 0;
        // Desfase de 1 s respecto al calendario del tablero.
        while 1_000 + tick * 2_000 < total {
            let due = t0 + Duration::from_millis(1_000 + tick * 2_000);
            if let Some(wait) = due.checked_duration_since(Instant::now()) {
                thread::sleep(wait);
            }
            let _ = send(&panes_list(second));
            let (resize, select) = (tick.is_multiple_of(5), tick % 10 == 3);
            if resize || select {
                let targets = send(&panes_list(board))
                    .map(|b| pane_targets(&b))
                    .unwrap_or_default();
                if resize && let Some((id, _)) = targets.first() {
                    let size = if tick.is_multiple_of(10) { 6 } else { 8 };
                    let body = serde_json::json!({
                        "session": board, "action": "resize", "pane": id,
                        "axis": "y", "size": size,
                    });
                    let _ = send(&body.to_string());
                } else if let Some((id, identity)) = targets.get(1) {
                    let body = serde_json::json!({
                        "session": board, "action": "select", "pane": id,
                        "identity": identity, "scope": "client",
                    });
                    let _ = send(&body.to_string());
                }
            }
            tick += 1;
        }
    })
}

/// Una petición medida: cuenta el envío, el error o el no-2xx y guarda la
/// latencia bajo `key`.
fn timed(stats: &Stats, key: &str, addr: &str, method: &str, path: &str, token: &str) {
    timed_with(stats, key, addr, method, path, token, None);
}

fn timed_with(
    stats: &Stats,
    key: &str,
    addr: &str,
    method: &str,
    path: &str,
    token: &str,
    body: Option<&str>,
) {
    stats.sent.fetch_add(1, Ordering::Relaxed);
    let t = Instant::now();
    let result = http(addr, method, path, token, body, Duration::from_secs(30));
    let ms = t.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
    stats
        .latency
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .entry(key.to_string())
        .or_default()
        .push(ms);
    match result {
        Ok((st, _)) if (200..300).contains(&st) => {}
        Ok(_) => {
            stats.non_2xx.fetch_add(1, Ordering::Relaxed);
        }
        Err(_) => {
            stats.errors.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Un sondeador extra de GET `/state` a 1 Hz (como el tablero, el remoto y
/// `cc-app` abiertos a la vez), desfasado `index × 137 ms` para no ir en fase.
fn spawn_poller(
    addr: &str,
    token: &str,
    minutes: u64,
    t0: Instant,
    index: usize,
    stats: &Arc<Stats>,
) -> thread::JoinHandle<()> {
    let (addr, token, stats) = (addr.to_string(), token.to_string(), stats.clone());
    let offset = (index as u64 * 137) % 1_000;
    thread::spawn(move || {
        let total = minutes * 60_000;
        let mut tick = 0;
        while offset + tick * 1_000 < total {
            let due = t0 + Duration::from_millis(offset + tick * 1_000);
            if let Some(wait) = due.checked_duration_since(Instant::now()) {
                thread::sleep(wait);
            }
            timed(&stats, "GET /state@1Hz", &addr, "GET", "/state", &token);
            tick += 1;
        }
    })
}

/// Cada 60 s (desde el segundo 5), una ola de `size` refrescos simultáneos de
/// `term.html`: cada iframe de terminal hace a la vez POST `/terminal-panes`
/// (`list` de su sesión) y GET `/tab-models?session=…`. Es lo que pasa cuando
/// `cc-app` vuelve al frente: `visibilitychange` despierta todas sus pestañas
/// de golpe, y cada `/terminal-panes` ocupa un hilo de bloqueo mientras espera
/// a tmux.
fn spawn_burst(
    addr: &str,
    token: &str,
    minutes: u64,
    t0: Instant,
    size: usize,
    sessions: Vec<String>,
    stats: &Arc<Stats>,
) -> thread::JoinHandle<()> {
    let (addr, token, stats) = (addr.to_string(), token.to_string(), stats.clone());
    thread::spawn(move || {
        let total = minutes * 60_000;
        let mut tick = 0;
        while 5_000 + tick * 60_000 < total {
            let due = t0 + Duration::from_millis(5_000 + tick * 60_000);
            if let Some(wait) = due.checked_duration_since(Instant::now()) {
                thread::sleep(wait);
            }
            let mut wave = Vec::new();
            for i in 0..size {
                let session = sessions
                    .get(i % sessions.len().max(1))
                    .cloned()
                    .unwrap_or_else(|| "poll".to_string());
                let (a, t, s) = (addr.clone(), token.clone(), stats.clone());
                let body = panes_list(&session);
                wave.push(thread::spawn(move || {
                    timed_with(
                        &s,
                        "POST /terminal-panes (ola)",
                        &a,
                        "POST",
                        "/terminal-panes",
                        &t,
                        Some(&body),
                    );
                }));
                let (a, t, s) = (addr.clone(), token.clone(), stats.clone());
                let path = format!("/tab-models?session={session}");
                wave.push(thread::spawn(move || {
                    timed_with(&s, "GET /tab-models (ola)", &a, "GET", &path, &t, None);
                }));
            }
            // Mientras dura la ola, un estático cada 100 ms: si la ola acapara el
            // pool de bloqueo, su lectura espera detrás.
            for _ in 0..10 {
                timed(
                    &stats,
                    STATIC_DURING_WAVE,
                    &addr,
                    "GET",
                    "/term.html",
                    &token,
                );
                thread::sleep(Duration::from_millis(100));
            }
            for t in wave {
                let _ = t.join();
            }
            tick += 1;
        }
    })
}

/// Clave de latencia de los estáticos pedidos durante la ola.
pub const STATIC_DURING_WAVE: &str = "GET /term.html (durante la ola)";

/// Fallos de los criterios `--max-threads` y `--max-pss-mib` sobre las
/// muestras `(minuto, Pss KiB, hilos)`; vacío si se cumplen.
pub fn bound_failures(
    samples: &[(u64, u64, Option<usize>)],
    max_threads: Option<usize>,
    max_pss_mib: Option<u64>,
) -> Vec<String> {
    let mut out = Vec::new();
    for (minute, pss, threads) in samples {
        if let (Some(max), Some(t)) = (max_threads, threads)
            && t > &max
        {
            out.push(format!("minuto {minute}: {t} hilos > {max}"));
        }
        if let Some(max) = max_pss_mib
            && *pss > max * 1024
        {
            out.push(format!("minuto {minute}: Pss {pss} KiB > {max} MiB"));
        }
    }
    out
}

/// Hilos del proceso (`/proc/<pid>/task`); `None` si ya no existe.
fn thread_count(pid: u32) -> Option<usize> {
    std::fs::read_dir(format!("/proc/{pid}/task"))
        .ok()
        .map(|dir| dir.count())
}

#[derive(Default)]
struct Stats {
    sent: AtomicU64,
    errors: AtomicU64,
    non_2xx: AtomicU64,
    /// Milisegundos de cada petición periódica por `"<método> <ruta>"`.
    latency: Mutex<BTreeMap<String, Vec<u32>>>,
}

/// Percentil por rango más cercano (ms); 0 sin muestras.
pub fn percentile(samples: &mut [u32], p: f64) -> u32 {
    if samples.is_empty() {
        return 0;
    }
    samples.sort_unstable();
    let rank = ((p / 100.0) * samples.len() as f64).ceil() as usize;
    samples
        .get(rank.saturating_sub(1).min(samples.len() - 1))
        .copied()
        .unwrap_or(0)
}

fn extract_rev(body: &[u8]) -> String {
    let t = String::from_utf8_lossy(body);
    let Some(i) = t.find("\"rev\"") else {
        return String::new();
    };
    t[i + 5..]
        .trim_start_matches([':', ' ', '"'])
        .chars()
        .take_while(|c| !matches!(c, '"' | ',' | '}'))
        .collect()
}

pub struct Opts {
    base: String,
    token: String,
    minutes: u64,
    pid: u32,
    /// `None` = archivo de trabajo en el directorio temporal; el rastreado exige `--out`.
    out: Option<PathBuf>,
    shadow: Option<(PathBuf, Option<PathBuf>)>,
    /// Con `--shadow`: base de estado copiada (solo lectura) a la pila aislada.
    state_db: Option<PathBuf>,
    /// Con `--shadow`: base de uso copiada (solo lectura) a la pila aislada.
    usage_db: Option<PathBuf>,
    /// Con `--shadow`: el frente arranca con `--no-native` (A/B contra la 2a).
    no_native: bool,
    /// Con `--shadow`: agentes falsos (por defecto los dos de `SHADOW_AGENTS`).
    agents: usize,
    /// Con `--shadow`: ventanas `cat` extra repartidas entre los agentes.
    extra_panes: usize,
    /// Sondeadores extra de GET `/state` a 1 Hz (tablero, remoto y app a la vez).
    pollers: usize,
    /// Cada 60 s, una ola de `burst` iframes de terminal que refrescan a la vez.
    burst: usize,
    /// Criterio: hilos del frente como mucho esto en cada muestra (falla si no).
    max_threads: Option<usize>,
    /// Criterio: Pss del frente como mucho esto (MiB) en cada muestra.
    max_pss_mib: Option<u64>,
    /// Con `--shadow`: cada `tmux list-panes` del frente y del heredado tarda esto.
    slow_tmux_ms: Option<u64>,
    /// Criterio: p95 de los estáticos pedidos durante la ola como mucho esto (ms).
    max_static_p95_ms: Option<u32>,
}

/// Opciones de `poll`; un error aquí es de uso (salida 2).
pub fn parse(args: &[String]) -> Result<Opts, String> {
    let (mut base, mut token, mut minutes, mut pid, mut out) = (None, None, None, None, None);
    let (mut shadow, mut hooks, mut comandos) = (false, None, None);
    let (mut state_db, mut usage_db, mut no_native) = (None, None, false);
    let (mut agents, mut extra_panes, mut pollers, mut burst) = (None, None, 0, 0);
    let (mut max_threads, mut max_pss_mib) = (None, None);
    let (mut slow_tmux_ms, mut max_static_p95_ms) = (None, None);
    let count = |a: &str, v: &str| {
        v.parse::<usize>()
            .map_err(|_| format!("{a}: {v:?} no es un entero"))
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--shadow" {
            shadow = true;
            continue;
        }
        if a == "--no-native" {
            no_native = true;
            continue;
        }
        let v = it.next().ok_or_else(|| format!("{a} sin valor"))?;
        match a.as_str() {
            "--base" => base = Some(v.clone()),
            "--token" => token = Some(v.clone()),
            "--minutes" => minutes = v.parse::<u64>().ok().filter(|m| *m >= 1),
            "--pid" => pid = v.parse::<u32>().ok(),
            "--out" => out = Some(PathBuf::from(v)),
            "--hooks" => hooks = Some(PathBuf::from(v)),
            "--comandos" => comandos = Some(PathBuf::from(v)),
            "--state-db" => state_db = Some(PathBuf::from(v)),
            "--usage-db" => usage_db = Some(PathBuf::from(v)),
            "--agents" => agents = Some(count(a, v)?),
            "--extra-panes" => extra_panes = Some(count(a, v)?),
            "--pollers" => pollers = count(a, v)?,
            "--burst" => burst = count(a, v)?,
            "--max-threads" => max_threads = Some(count(a, v)?),
            "--max-pss-mib" => max_pss_mib = Some(count(a, v)? as u64),
            "--slow-tmux-ms" => slow_tmux_ms = Some(count(a, v)? as u64),
            "--max-static-p95-ms" => {
                max_static_p95_ms = Some(u32::try_from(count(a, v)?).unwrap_or(u32::MAX))
            }
            other => return Err(format!("argumento desconocido: {other}")),
        }
    }
    // Sin pila aislada no hay frente que arrancar: estas opciones no tienen efecto.
    if !shadow
        && (state_db.is_some()
            || usage_db.is_some()
            || no_native
            || agents.is_some()
            || extra_panes.is_some()
            || slow_tmux_ms.is_some())
    {
        return Err(
            "--state-db, --usage-db, --no-native, --agents, --extra-panes y --slow-tmux-ms \
             requieren --shadow"
                .into(),
        );
    }
    let shadow = if shadow {
        Some((hooks.ok_or("--shadow requiere --hooks")?, comandos))
    } else {
        None
    };
    Ok(Opts {
        base: if shadow.is_some() {
            String::new()
        } else {
            base.ok_or("falta --base (o --shadow --hooks)")?
        },
        token: token.unwrap_or_default(),
        minutes: minutes.ok_or("falta --minutes N (entero ≥ 1)")?,
        pid: if shadow.is_some() {
            0
        } else {
            pid.ok_or("falta --pid")?
        },
        out,
        shadow,
        state_db,
        usage_db,
        no_native,
        agents: agents.unwrap_or(SHADOW_AGENTS.len()),
        extra_panes: extra_panes.unwrap_or(0),
        pollers,
        burst,
        max_threads,
        max_pss_mib,
        slow_tmux_ms,
        max_static_p95_ms,
    })
}

/// (mínimo, máximo) de los puntos `(minuto, kib)` desde el minuto `from`.
pub fn min_max_from(points: &[(u64, u64)], from: u64) -> Option<(u64, u64)> {
    let v: Vec<u64> = points
        .iter()
        .filter(|(m, _)| *m >= from)
        .map(|p| p.1)
        .collect();
    Some((*v.iter().min()?, *v.iter().max()?))
}

/// Servidor cargado y sesión que lista `/terminal-panes`.
struct Target<'a> {
    addr: &'a str,
    token: &'a str,
    panes: &'static str,
}

/// Un cliente completo: su calendario periódico en un hilo y su long-poll en otro.
fn spawn_client(
    target: &Target,
    client: Client,
    minutes: u64,
    t0: Instant,
    stats: &Arc<Stats>,
    stop: &Arc<AtomicBool>,
) -> Vec<thread::JoinHandle<()>> {
    let (addr, token, panes) = (target.addr, target.token, target.panes);
    let sched = schedule(minutes, client);
    let (a1, t1, s1) = (addr.to_string(), token.to_string(), stats.clone());
    let periodic = thread::spawn(move || {
        for (ms, method, path) in sched {
            let due = t0 + Duration::from_millis(ms);
            if let Some(wait) = due.checked_duration_since(Instant::now()) {
                thread::sleep(wait);
            }
            s1.sent.fetch_add(1, Ordering::Relaxed);
            let t = Instant::now();
            let result = http(
                &a1,
                method,
                &path,
                &t1,
                body_for(&path_only(&path), panes).as_deref(),
                Duration::from_secs(30),
            );
            let ms = t.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
            s1.latency
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .entry(format!("{method} {}", path_only(&path)))
                .or_default()
                .push(ms);
            match result {
                Ok((st, _)) if (200..300).contains(&st) => {}
                Ok(_) => {
                    s1.non_2xx.fetch_add(1, Ordering::Relaxed);
                }
                Err(_) => {
                    s1.errors.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    });
    let (a2, t2, st2, s2) = (
        addr.to_string(),
        token.to_string(),
        stop.clone(),
        stats.clone(),
    );
    let watcher = thread::spawn(move || {
        let mut rev = String::new();
        while !st2.load(Ordering::Relaxed) {
            let p = format!("/notices/watch?rev={rev}&wait=25");
            match http(&a2, "GET", &p, &t2, None, Duration::from_secs(40)) {
                Ok((200, b)) => rev = extract_rev(&b),
                Ok(_) => {
                    s2.non_2xx.fetch_add(1, Ordering::Relaxed);
                    thread::sleep(Duration::from_secs(2));
                }
                Err(_) => {
                    s2.errors.fetch_add(1, Ordering::Relaxed);
                    thread::sleep(Duration::from_secs(2));
                }
            }
        }
    });
    vec![periodic, watcher]
}

pub fn run(args: &[String]) -> Result<(), String> {
    let o = parse(args)?;
    // Con --shadow se levanta la pila aislada (copia de hooks, Python heredado y frente) y se
    // carga el frente. Sin --shadow solo se alcanzan servidores dentro del mismo namespace.
    let stack = match &o.shadow {
        Some((hooks, comandos)) => {
            let comandos = match comandos {
                Some(c) => c.clone(),
                None => crate::parity::default_comandos()?,
            };
            Some(crate::parity::Stack::start_with(
                crate::parity::StackOptions {
                    hooks,
                    comandos: &comandos,
                    keep: false,
                    state_db: o.state_db.as_deref(),
                    usage_db: o.usage_db.as_deref(),
                    no_native: o.no_native,
                },
            )?)
        }
        None => None,
    };
    let (base, token, pid) = match &stack {
        Some(s) => (
            format!("http://127.0.0.1:{}", s.p_front),
            s.token.clone(),
            s.pid_front,
        ),
        None => (o.base.clone(), o.token.clone(), o.pid),
    };
    let addr = base
        .strip_prefix("http://")
        .ok_or("--base debe ser http://host:puerto")?
        .trim_end_matches('/')
        .to_string();
    if !addr.starts_with("127.0.0.1:") && !addr.starts_with("localhost:") {
        return Err("solo se permite cargar un servidor local (127.0.0.1)".into());
    }
    let out = o.out.clone().unwrap_or_else(|| {
        std::env::temp_dir().join(format!("xtask-poll-{}.jsonl", std::process::id()))
    });
    println!(
        "poll: tablero {} + app {} peticiones periódicas en {} min contra {addr}; salida {}",
        schedule(o.minutes, Client::Dashboard).len(),
        schedule(o.minutes, Client::App).len(),
        o.minutes,
        out.display()
    );
    // Con --shadow, `/terminal-panes` carga sesiones reales del tmux PRIVADO de
    // la pila (`local` la crea ella; `poll-b` se crea aquí), con varios panes.
    let panes = match &stack {
        Some(s) => {
            s.front_tmux(&["new-session", "-d", "-s", SHADOW_SESSIONS[1], "cat"])?;
            for session in SHADOW_SESSIONS {
                for _ in 1..SHADOW_PANES {
                    s.front_tmux(&["split-window", "-d", "-t", session, "cat"])?;
                }
            }
            seed_agents(s, &shadow_agents(o.agents), o.extra_panes)?;
            if let Some(ms) = o.slow_tmux_ms {
                s.slow_tmux(ms)?;
                println!("poll: cada `tmux list-panes` de la pila tarda {ms} ms");
            }
            SHADOW_SESSIONS[0]
        }
        None => "poll",
    };
    let stats = Arc::new(Stats::default());
    let stop = Arc::new(AtomicBool::new(false));
    let t0 = Instant::now();
    let mut handles = Vec::new();
    for client in [Client::Dashboard, Client::App] {
        let target = Target {
            addr: &addr,
            token: &token,
            panes,
        };
        handles.extend(spawn_client(&target, client, o.minutes, t0, &stats, &stop));
    }
    if stack.is_some() {
        handles.push(spawn_pane_mix(&addr, &token, o.minutes, t0, &stats));
    }
    for i in 0..o.pollers {
        handles.push(spawn_poller(&addr, &token, o.minutes, t0, i, &stats));
    }
    if o.burst > 0 {
        let sessions = if stack.is_some() {
            shadow_agents(o.agents)
                .into_iter()
                .map(|(s, _)| s)
                .collect()
        } else {
            vec!["poll".to_string()]
        };
        handles.push(spawn_burst(
            &addr, &token, o.minutes, t0, o.burst, sessions, &stats,
        ));
    }

    // Muestreo de memoria cada 60 s (y uno inicial en el minuto 0).
    let mut points: Vec<(u64, u64)> = Vec::new();
    let mut thread_points: Vec<(u64, Option<usize>)> = Vec::new();
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(&out)
        .map_err(|e| format!("{}: {e}", out.display()))?;
    for minute in 0..=o.minutes {
        let due = t0 + Duration::from_secs(minute * 60);
        if let Some(wait) = due.checked_duration_since(Instant::now()) {
            thread::sleep(wait);
        }
        let m = crate::rss::sample(pid)?;
        points.push((minute, m.pss_kib_total));
        let threads = thread_count(pid);
        thread_points.push((minute, threads));
        let legacy = stack
            .as_ref()
            .and_then(|s| crate::rss::sample(s.pid_legacy).ok())
            .map(|l| l.pss_kib_total);
        let line = serde_json::json!({
            "cmd": ["dash-poll", base],
            "minute": minute,
            "pss_kib": m.pss_kib_total,
            "rss_kib": m.rss_kib_total,
            "legacy_pss_kib": legacy,
            "threads": threads,
            "pss_exact": m.pss_exact,
            "procs": m.procs,
            "sent": stats.sent.load(Ordering::Relaxed),
            "errors": stats.errors.load(Ordering::Relaxed),
            "non_2xx": stats.non_2xx.load(Ordering::Relaxed),
        });
        println!("{line}");
        writeln!(file, "{line}").map_err(|e| e.to_string())?;
    }
    stop.store(true, Ordering::Relaxed);
    // Los hilos periódicos ya terminaron; los long-poll cierran en ≤ 25 s al soltar la pila.
    drop(handles);

    if let Some((lo, hi)) = min_max_from(&points, 0) {
        println!("resumen (total): Pss min {lo} KiB, max {hi} KiB");
    }
    match min_max_from(&points, 5) {
        Some((lo, hi)) => println!("resumen (desde min 5): Pss min {lo} KiB, max {hi} KiB"),
        None => println!("resumen (desde min 5): n/d (la ventana es de menos de 5 min)"),
    }
    let at = |minute: u64| {
        thread_points
            .iter()
            .find(|(m, _)| *m == minute)
            .and_then(|(_, t)| *t)
            .map_or_else(|| "n/d".to_string(), |t| t.to_string())
    };
    println!(
        "hilos del frente: min 1 {}, min {} {}",
        at(1),
        o.minutes,
        at(o.minutes)
    );
    match slope_kib_per_hour(&points) {
        Some(sl) => println!("pendiente desde min 5: {sl:.0} KiB/h"),
        None => println!("pendiente: n/d (menos de 2 puntos desde el min 5)"),
    }
    println!(
        "peticiones: {} enviadas, {} errores de transporte, {} no-2xx",
        stats.sent.load(Ordering::Relaxed),
        stats.errors.load(Ordering::Relaxed),
        stats.non_2xx.load(Ordering::Relaxed)
    );
    println!("\nlatencia por ruta (ms): p50 p95 p99 n");
    let mut table = stats.latency.lock().unwrap_or_else(|p| p.into_inner());
    for (route, samples) in table.iter_mut() {
        let n = samples.len();
        let (p50, p95, p99) = (
            percentile(samples, 50.0),
            percentile(samples, 95.0),
            percentile(samples, 99.0),
        );
        println!("  {route:<28} {p50:>5} {p95:>5} {p99:>5} {n:>5}");
    }
    drop(table);
    let samples: Vec<(u64, u64, Option<usize>)> = points
        .iter()
        .zip(&thread_points)
        .map(|((m, pss), (_, t))| (*m, *pss, *t))
        .collect();
    let mut failures = bound_failures(&samples, o.max_threads, o.max_pss_mib);
    if let Some(max) = o.max_static_p95_ms {
        let mut table = stats.latency.lock().unwrap_or_else(|p| p.into_inner());
        match table.get_mut(STATIC_DURING_WAVE) {
            Some(samples) if !samples.is_empty() => {
                let p95 = percentile(samples, 95.0);
                if p95 > max {
                    failures.push(format!("estáticos durante la ola: p95 {p95} ms > {max} ms"));
                }
            }
            _ => failures.push("estáticos durante la ola: sin muestras (¿--burst 0?)".into()),
        }
    }
    if let Some(stack) = &stack {
        let forwarded = stack.forwarded_summary();
        println!(
            "\nreenviadas al heredado por el frente ({} rutas distintas):",
            forwarded.len()
        );
        for (route, n) in &forwarded {
            println!("  {n:>4}  {route}");
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "criterios de ligereza incumplidos: {}",
            failures.join("; ")
        ))
    }
}

fn path_only(p: &str) -> String {
    p.split('?').next().unwrap_or(p).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_nearest_rank() {
        let mut v = vec![50, 10, 40, 20, 30];
        assert_eq!(percentile(&mut v, 50.0), 30);
        assert_eq!(percentile(&mut v, 95.0), 50);
        assert_eq!(percentile(&mut Vec::new(), 95.0), 0);
    }
}

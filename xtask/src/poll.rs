//! Carga sintética: reproduce el polling real del tablero y de la app (inventario §1.11) y
//! mide el Pss/RSS del servidor a lo largo del tiempo.
//!
//! Herramienta de desarrollo. Cliente HTTP a mano sobre `TcpStream`.

use std::{
    io::{Read, Write},
    net::TcpStream,
    path::PathBuf,
    sync::{
        Arc,
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

fn body_for(path: &str) -> Option<&'static str> {
    match path {
        "/presence" => Some(
            r#"{"deviceId":"xtask-poll","visible":true,"canPlayAudio":false,"interaction":false}"#,
        ),
        "/terminal-panes" => Some(r#"{"session":"poll","action":"list"}"#),
        _ => None,
    }
}

#[derive(Default)]
struct Stats {
    sent: AtomicU64,
    errors: AtomicU64,
    non_2xx: AtomicU64,
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
    /// Con `--shadow`: el frente arranca con `--no-native` (A/B contra la 2a).
    no_native: bool,
}

/// Opciones de `poll`; un error aquí es de uso (salida 2).
pub fn parse(args: &[String]) -> Result<Opts, String> {
    let (mut base, mut token, mut minutes, mut pid, mut out) = (None, None, None, None, None);
    let (mut shadow, mut hooks, mut comandos) = (false, None, None);
    let (mut state_db, mut no_native) = (None, false);
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
            other => return Err(format!("argumento desconocido: {other}")),
        }
    }
    // Sin pila aislada no hay frente que arrancar: estas opciones no tienen efecto.
    if !shadow && (state_db.is_some() || no_native) {
        return Err("--state-db y --no-native requieren --shadow".into());
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
        no_native,
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

/// Un cliente completo: su calendario periódico en un hilo y su long-poll en otro.
fn spawn_client(
    addr: &str,
    token: &str,
    client: Client,
    minutes: u64,
    t0: Instant,
    stats: &Arc<Stats>,
    stop: &Arc<AtomicBool>,
) -> Vec<thread::JoinHandle<()>> {
    let sched = schedule(minutes, client);
    let (a1, t1, s1) = (addr.to_string(), token.to_string(), stats.clone());
    let periodic = thread::spawn(move || {
        for (ms, method, path) in sched {
            let due = t0 + Duration::from_millis(ms);
            if let Some(wait) = due.checked_duration_since(Instant::now()) {
                thread::sleep(wait);
            }
            s1.sent.fetch_add(1, Ordering::Relaxed);
            match http(
                &a1,
                method,
                &path,
                &t1,
                body_for(&path_only(&path)),
                Duration::from_secs(30),
            ) {
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
    let stats = Arc::new(Stats::default());
    let stop = Arc::new(AtomicBool::new(false));
    let t0 = Instant::now();
    let mut handles = Vec::new();
    for client in [Client::Dashboard, Client::App] {
        handles.extend(spawn_client(
            &addr, &token, client, o.minutes, t0, &stats, &stop,
        ));
    }

    // Muestreo de memoria cada 60 s (y uno inicial en el minuto 0).
    let mut points: Vec<(u64, u64)> = Vec::new();
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
    Ok(())
}

fn path_only(p: &str) -> String {
    p.split('?').next().unwrap_or(p).to_string()
}

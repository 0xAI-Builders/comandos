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

/// (periodo en ms, método, ruta). El long-poll `/notices/watch` no entra: es continuo y lo
/// llevan hilos propios.
const TABLE: [(u64, &str, &str); 12] = [
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
    (2_000, "GET", "/workspace"),
];

/// Todas las peticiones periódicas de `minutes` minutos, ordenadas por instante (ms desde el
/// inicio). Cada ruta arranca en 0 y repite cada su periodo.
pub fn schedule(minutes: u64) -> Vec<(u64, &'static str, String)> {
    let total = minutes * 60_000;
    let mut out = Vec::new();
    for (period, method, path) in TABLE {
        let mut t = 0;
        while t < total {
            out.push((t, method, path.to_string()));
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

struct Opts {
    base: String,
    token: String,
    minutes: u64,
    pid: u32,
    out: PathBuf,
}

fn parse(args: &[String]) -> Result<Opts, String> {
    let (mut base, mut token, mut minutes, mut pid, mut out) = (None, None, None, None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let v = it.next().ok_or_else(|| format!("{a} sin valor"))?;
        match a.as_str() {
            "--base" => base = Some(v.clone()),
            "--token" => token = Some(v.clone()),
            "--minutes" => minutes = v.parse::<u64>().ok().filter(|m| *m >= 1),
            "--pid" => pid = v.parse::<u32>().ok(),
            "--out" => out = Some(PathBuf::from(v)),
            other => return Err(format!("argumento desconocido: {other}")),
        }
    }
    Ok(Opts {
        base: base.ok_or("falta --base")?,
        token: token.unwrap_or_default(),
        minutes: minutes.ok_or("falta --minutes N (entero ≥ 1)")?,
        pid: pid.ok_or("falta --pid")?,
        out: out.unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../docs/verification/rss.jsonl")
        }),
    })
}

pub fn run(args: &[String]) -> Result<(), String> {
    let o = parse(args)?;
    let addr = o
        .base
        .strip_prefix("http://")
        .ok_or("--base debe ser http://host:puerto")?
        .trim_end_matches('/')
        .to_string();
    if !addr.starts_with("127.0.0.1:") && !addr.starts_with("localhost:") {
        return Err("solo se permite cargar un servidor local (127.0.0.1)".into());
    }
    let sched = schedule(o.minutes);
    println!(
        "poll: {} peticiones periódicas en {} min contra {addr}",
        sched.len(),
        o.minutes
    );
    let stats = Arc::new(Stats::default());
    let stop = Arc::new(AtomicBool::new(false));
    let t0 = Instant::now();
    let mut handles = Vec::new();

    // Dos clientes (tablero y app) se reparten el calendario por índice.
    for client in 0..2usize {
        let mine: Vec<_> = sched.iter().skip(client).step_by(2).cloned().collect();
        let (addr, token, stats) = (addr.clone(), o.token.clone(), stats.clone());
        handles.push(thread::spawn(move || {
            for (ms, method, path) in mine {
                let due = t0 + Duration::from_millis(ms);
                if let Some(wait) = due.checked_duration_since(Instant::now()) {
                    thread::sleep(wait);
                }
                stats.sent.fetch_add(1, Ordering::Relaxed);
                match http(
                    &addr,
                    method,
                    &path,
                    &token,
                    body_for(&path_only(&path)),
                    Duration::from_secs(30),
                ) {
                    Ok((st, _)) if (200..300).contains(&st) => {}
                    Ok(_) => {
                        stats.non_2xx.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(_) => {
                        stats.errors.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }));
    }
    // Long-poll continuo de 25 s, uno por cliente (tablero y app).
    let watchers: Vec<_> = (0..2)
        .map(|_| {
            let (addr, token, stop, stats) =
                (addr.clone(), o.token.clone(), stop.clone(), stats.clone());
            thread::spawn(move || {
                let mut rev = String::new();
                while !stop.load(Ordering::Relaxed) {
                    let p = format!("/notices/watch?rev={rev}&wait=25");
                    match http(&addr, "GET", &p, &token, None, Duration::from_secs(40)) {
                        Ok((200, b)) => rev = extract_rev(&b),
                        Ok(_) => {
                            stats.non_2xx.fetch_add(1, Ordering::Relaxed);
                            thread::sleep(Duration::from_secs(2));
                        }
                        Err(_) => {
                            stats.errors.fetch_add(1, Ordering::Relaxed);
                            thread::sleep(Duration::from_secs(2));
                        }
                    }
                }
            })
        })
        .collect();

    // Muestreo de memoria cada 60 s (y uno inicial en el minuto 0).
    let mut points: Vec<(u64, u64)> = Vec::new();
    if let Some(parent) = o.out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(&o.out)
        .map_err(|e| format!("{}: {e}", o.out.display()))?;
    for minute in 0..=o.minutes {
        let due = t0 + Duration::from_secs(minute * 60);
        if let Some(wait) = due.checked_duration_since(Instant::now()) {
            thread::sleep(wait);
        }
        let m = crate::rss::sample(o.pid)?;
        points.push((minute, m.pss_kib_total));
        let line = serde_json::json!({
            "cmd": ["dash-poll", o.base],
            "minute": minute,
            "pss_kib": m.pss_kib_total,
            "rss_kib": m.rss_kib_total,
            "pss_exact": m.pss_exact,
            "procs": m.procs,
            "sent": stats.sent.load(Ordering::Relaxed),
            "errors": stats.errors.load(Ordering::Relaxed),
            "non_2xx": stats.non_2xx.load(Ordering::Relaxed),
        });
        println!("{line}");
        writeln!(file, "{line}").map_err(|e| e.to_string())?;
    }
    for h in handles {
        let _ = h.join();
    }
    stop.store(true, Ordering::Relaxed);
    // Los watchers terminan al cerrar su espera (≤ 25 s); no se bloquea el informe en ellos.
    drop(watchers);

    let pss: Vec<u64> = points.iter().map(|p| p.1).collect();
    let (min, max) = (
        pss.iter().min().copied().unwrap_or(0),
        pss.iter().max().copied().unwrap_or(0),
    );
    match slope_kib_per_hour(&points) {
        Some(s) => println!(
            "resumen: Pss min {min} KiB, max {max} KiB, pendiente desde min 5: {s:.0} KiB/h"
        ),
        None => println!(
            "resumen: Pss min {min} KiB, max {max} KiB, pendiente: n/d (menos de 2 puntos desde el min 5)"
        ),
    }
    println!(
        "peticiones: {} enviadas, {} errores de transporte, {} no-2xx",
        stats.sent.load(Ordering::Relaxed),
        stats.errors.load(Ordering::Relaxed),
        stats.non_2xx.load(Ordering::Relaxed)
    );
    Ok(())
}

fn path_only(p: &str) -> String {
    p.split('?').next().unwrap_or(p).to_string()
}

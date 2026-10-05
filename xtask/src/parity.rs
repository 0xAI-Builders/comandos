//! Arnés diferencial: la misma petición contra `bin/cc-dash` (oráculo Python) y contra
//! `comandos dash` (frente Rust), ambos sobre copias privadas de `~/.claude/hooks`.
//!
//! Herramienta de desarrollo, no código de producto. El Python solo corre como oráculo.
//! Reglas de oro: jamás toca el `~/.claude/hooks` real, los puertos 4777/4778/4781 ni el
//! tmux del usuario; todo vive bajo `std::env::temp_dir()` y dentro de un namespace de red
//! propio (`unshare -Urn`) donde el loopback es privado: fallar cerrado si no se puede.

use std::{
    fs,
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    os::unix::{fs::DirBuilderExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde_json::Value;

/// El arnés decide el modo del frente con `--no-native`, nunca el entorno del usuario.
const NATIVE_ENV_OF_FRONT: &str = "COMANDOS_DASH_NATIVE";
const MASK: &str = "<volátil>";
/// Cabeceras que `same` compara; el resto (Date, Server…) es ruido por construcción.
const RELEVANT_HEADERS: [&str; 4] = [
    "content-type",
    "content-length",
    "cache-control",
    "connection",
];
/// Marca de que ya estamos dentro del namespace de red aislado.
const NETNS_ENV: &str = "COMANDOS_XTASK_NETNS";
/// Ejecutables que el oráculo podría lanzar y que aquí no deben hacer nada.
const FAKE_BINS: [&str; 12] = [
    "systemctl",
    "wmctrl",
    "cc-webterm",
    "cc-webterm-attach",
    "systemd-run",
    "tailscale",
    "notify-send",
    "pw-play",
    "paplay",
    "spd-say",
    "piper",
    "xdg-open",
];
/// Archivos de la copia de hooks que activan servicios externos o red.
const STRIP_FILES: [&str; 5] = [
    "webterm-enabled",
    "news-editions.json",
    "news-watch.json",
    "model-watch.json",
    "push-subscriptions.json",
];
/// Tope de un symlink externo que se sustituye por copia.
const MAX_LINK_COPY: u64 = 256 * 1024 * 1024;

// ---------------------------------------------------------------- modelo puro

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expect {
    Same,
    StaticAccepted,
    /// Solo el código de estado (cuerpos de error de la plantilla HTML de Python vs JSON).
    StatusOnly,
    Skip,
}

impl Expect {
    pub fn parse(s: &str) -> Option<Expect> {
        match s {
            "same" => Some(Expect::Same),
            "static-accepted" => Some(Expect::StaticAccepted),
            "status-only" => Some(Expect::StatusOnly),
            "skip" => Some(Expect::Skip),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    Diff(String),
    Skip(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resp {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Resp {
    /// Todos los valores de una cabecera (puede repetirse), en orden.
    fn header_values(&self, name: &str) -> Vec<&str> {
        self.headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
            .collect()
    }
}

/// Sustituye por `"<volátil>"` cada valor señalado por un puntero JSON. `*` recorre todos los
/// índices de un array (o valores de un objeto). Lo que no existe no se crea: la ausencia
/// de una clave también es un dato que comparar.
pub fn normalize(v: &mut Value, pointers: &[String]) {
    for p in pointers {
        let segs: Vec<String> = p
            .split('/')
            .skip(1)
            .map(|s| s.replace("~1", "/").replace("~0", "~"))
            .collect();
        if !segs.is_empty() {
            mask(v, &segs);
        }
    }
}

fn mask(v: &mut Value, segs: &[String]) {
    let (head, rest) = (&segs[0], &segs[1..]);
    let targets: Vec<&mut Value> = match v {
        Value::Array(a) if head == "*" => a.iter_mut().collect(),
        Value::Object(o) if head == "*" => o.values_mut().collect(),
        Value::Array(a) => head
            .parse::<usize>()
            .ok()
            .and_then(|i| a.get_mut(i))
            .into_iter()
            .collect(),
        Value::Object(o) => o.get_mut(head.as_str()).into_iter().collect(),
        _ => Vec::new(),
    };
    for t in targets {
        if rest.is_empty() {
            *t = Value::String(MASK.into());
        } else {
            mask(t, rest);
        }
    }
}

/// Cuerpo normalizado de una respuesta cuando hay `volatile`: JSON parseado, punteros
/// sustituidos y reserializado con el volcado de respuestas de Python (orden de inserción).
fn masked_dump(body: &[u8], volatile: &[String]) -> Option<String> {
    let mut v: Value = serde_json::from_slice(body).ok()?;
    normalize(&mut v, volatile);
    comandos_core::json::response_dumps(&v).ok()
}

/// Sin `volatile`: bytes crudos (el orden de claves y el formato de números cuentan). Con
/// `volatile`: ambos cuerpos parseados, sustituidos y reserializados con `response_dumps`;
/// ahí el formato de floats y escapes ya no se compara (lo canoniza el volcado).
/// Devuelve (diferencia, true si se comparó reserializado).
fn body_diff(py: &Resp, rs: &Resp, volatile: &[String]) -> (Option<String>, bool) {
    if !volatile.is_empty()
        && let (Some(a), Some(b)) = (
            masked_dump(&py.body, volatile),
            masked_dump(&rs.body, volatile),
        )
    {
        let d =
            (a != b).then(|| format!("cuerpo JSON (normalizado) distinto:\n  py: {a}\n  rs: {b}"));
        return (d, true);
    }
    let d = (py.body != rs.body).then(|| {
        format!(
            "cuerpo distinto ({} B py, {} B rs)",
            py.body.len(),
            rs.body.len()
        )
    });
    (d, false)
}

/// Compara la respuesta del oráculo (`py`) con la del frente (`rs`).
/// `same`: estado + cabeceras relevantes + cuerpo. `static-accepted`: estado + cuerpo.
pub fn compare(py: &Resp, rs: &Resp, expect: Expect, volatile: &[String]) -> Outcome {
    if expect == Expect::Skip {
        return Outcome::Skip("fixture".into());
    }
    if py.status != rs.status {
        return Outcome::Diff(format!("status {} (py) vs {} (rs)", py.status, rs.status));
    }
    if expect == Expect::StatusOnly {
        return Outcome::Ok;
    }
    let (body, reserialized) = body_diff(py, rs, volatile);
    if expect == Expect::Same {
        for h in RELEVANT_HEADERS {
            // Con volátiles la longitud cruda cambia con los valores: solo cuenta la del
            // cuerpo reserializado, que ya cubre `body`.
            if h == "content-length" && reserialized {
                continue;
            }
            let (a, b) = (py.header_values(h), rs.header_values(h));
            if a != b {
                return Outcome::Diff(format!("cabecera {h}: {a:?} (py) vs {b:?} (rs)"));
            }
        }
    }
    match body {
        Some(d) => Outcome::Diff(d),
        None => Outcome::Ok,
    }
}

// ------------------------------------------------------------ cliente HTTP/1.1

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// `Ok(None)` si el cuerpo chunked aún no está completo.
fn dechunk(mut raw: &[u8]) -> Result<Option<Vec<u8>>, String> {
    let mut out = Vec::new();
    loop {
        let Some(eol) = find(raw, b"\r\n") else {
            return Ok(None);
        };
        let line = std::str::from_utf8(&raw[..eol]).map_err(|e| e.to_string())?;
        let size = usize::from_str_radix(line.split(';').next().unwrap_or("").trim(), 16)
            .map_err(|e| format!("tamaño de chunk: {e}"))?;
        raw = &raw[eol + 2..];
        if size == 0 {
            return Ok(Some(out));
        }
        if raw.len() < size + 2 {
            return Ok(None);
        }
        out.extend_from_slice(&raw[..size]);
        raw = &raw[size + 2..];
    }
}

/// Intenta cerrar una respuesta con lo leído hasta ahora. `Ok(None)` = falta leer.
/// `eof`: el servidor ya cerró. `bodiless`: HEAD (sin cuerpo aunque haya longitud).
/// Un cuerpo más corto que `Content-Length` al llegar el EOF es un error, no un recorte.
fn frame(raw: &[u8], eof: bool, bodiless: bool) -> Result<Option<Resp>, String> {
    let Some(split) = find(raw, b"\r\n\r\n") else {
        return if eof {
            Err("respuesta sin cabeceras".into())
        } else {
            Ok(None)
        };
    };
    let head = std::str::from_utf8(&raw[..split]).map_err(|e| e.to_string())?;
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or("");
    let mut parts = status_line.split_whitespace();
    if !parts.next().is_some_and(|v| v.starts_with("HTTP/")) {
        return Err(format!("línea de estado inválida: {status_line:?}"));
    }
    let status: u16 = parts
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| format!("estado inválido: {status_line:?}"))?;
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect();
    let rest = &raw[split + 4..];
    let get = |n: &str| {
        headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(n))
            .map(|(_, v)| v.as_str())
    };
    let body = if bodiless || status == 204 || status == 304 || (100..200).contains(&status) {
        Vec::new()
    } else if get("transfer-encoding").is_some_and(|v| v.to_ascii_lowercase().contains("chunked")) {
        match dechunk(rest)? {
            Some(b) => b,
            None if eof => return Err("cuerpo chunked truncado".into()),
            None => return Ok(None),
        }
    } else if let Some(n) = get("content-length").and_then(|v| v.parse::<usize>().ok()) {
        if rest.len() < n {
            return if eof {
                Err(format!("cuerpo truncado: {} B de {n} B", rest.len()))
            } else {
                Ok(None)
            };
        }
        rest[..n].to_vec()
    } else if eof {
        rest.to_vec()
    } else {
        return Ok(None); // cuerpo delimitado por el cierre de la conexión
    };
    Ok(Some(Resp {
        status,
        headers,
        body,
    }))
}

/// Interpreta una respuesta completa (leída hasta EOF).
#[allow(dead_code)] // lo ejercitan los tests de integración
pub fn parse_response(raw: &[u8]) -> Result<Resp, String> {
    frame(raw, true, false)?.ok_or_else(|| "respuesta incompleta".into())
}

/// Petición con keep-alive (sin `Connection: close`) para que el `Connection: close` de las
/// respuestas de rechazo sea observable. Termina en cuanto la respuesta queda enmarcada.
pub fn request(
    port: u16,
    method: &str,
    path: &str,
    headers: &[(String, String)],
    body: Option<&[u8]>,
) -> Result<Resp, String> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).map_err(|e| format!("connect: {e}"))?;
    s.set_read_timeout(Some(Duration::from_secs(10))).ok();
    s.set_write_timeout(Some(Duration::from_secs(10))).ok();
    let has = |n: &str| headers.iter().any(|(k, _)| k.eq_ignore_ascii_case(n));
    let mut head = format!("{method} {path} HTTP/1.1\r\n");
    if !has("host") {
        head += &format!("Host: 127.0.0.1:{port}\r\n");
    }
    for (k, v) in headers {
        head += &format!("{k}: {v}\r\n");
    }
    if !has("content-length") && (body.is_some() || method == "POST") {
        head += &format!("Content-Length: {}\r\n", body.map_or(0, <[u8]>::len));
    }
    head += "\r\n";
    let mut out = head.into_bytes();
    if let Some(b) = body {
        out.extend_from_slice(b);
    }
    // El servidor puede cerrar antes de leer todo el cuerpo (413): un fallo de escritura no
    // invalida la respuesta que ya haya enviado.
    let _ = s.write_all(&out);
    let mut raw = Vec::new();
    let mut buf = [0u8; 16 * 1024];
    loop {
        match s.read(&mut buf) {
            Ok(0) => {
                return frame(&raw, true, method == "HEAD")?.ok_or_else(|| "incompleta".into());
            }
            Ok(n) => {
                raw.extend_from_slice(&buf[..n]);
                if let Some(r) = frame(&raw, false, method == "HEAD")? {
                    return Ok(r);
                }
            }
            Err(e) => return Err(format!("lectura: {e}")),
        }
    }
}

// --------------------------------------------------------- aislamiento de red

/// Reejecuta el proceso dentro de `unshare -Urn` (loopback privado). Si ya estamos dentro
/// (marca en el entorno) comprueba que 4777/4778/4779/4780/4781 no responden. Falla cerrado:
/// sin namespace no se lanza el oráculo y no hay opción para saltárselo.
pub fn ensure_isolated() -> Result<(), String> {
    if std::env::var(NETNS_ENV).as_deref() == Ok("1") {
        for port in [4777u16, 4778, 4779, 4780, 4781] {
            let addr = SocketAddr::from(([127, 0, 0, 1], port));
            if TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_ok() {
                return Err(format!(
                    "el puerto {port} responde dentro del namespace: no está aislado, se aborta"
                ));
            }
        }
        return Ok(());
    }
    let probe = Command::new("unshare")
        .args(["-Urn", "--", "sh", "-c", "ip link set lo up"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if !matches!(probe, Ok(s) if s.success()) {
        return Err(
            "no se pudo crear un namespace de red (`unshare -Urn`): el oráculo Python podría \
             alcanzar servicios reales, así que no se lanza nada"
                .into(),
        );
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let err = Command::new("unshare")
        .args([
            "-Urn",
            "--",
            "sh",
            "-c",
            "ip link set lo up && exec \"$0\" \"$@\"",
        ])
        .arg(exe)
        .args(std::env::args().skip(1))
        .env(NETNS_ENV, "1")
        .exec();
    Err(format!("exec unshare: {err}"))
}

// --------------------------------------------------------------- entorno aislado

struct Server {
    child: Child,
    name: &'static str,
}

impl Drop for Server {
    fn drop(&mut self) {
        let pgid = nix::unistd::Pid::from_raw(self.child.id() as i32);
        let _ = nix::sys::signal::killpg(pgid, nix::sys::signal::Signal::SIGTERM);
        for _ in 0..30 {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            thread::sleep(Duration::from_millis(100));
        }
        let _ = nix::sys::signal::killpg(pgid, nix::sys::signal::Signal::SIGKILL);
        let _ = self.child.wait();
        eprintln!("({} terminado a la fuerza)", self.name);
    }
}

/// Directorio raíz temporal que se borra al soltarlo (también en las rutas de error).
struct TempRoot {
    path: PathBuf,
    keep: bool,
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        if !self.keep && under_tmp(&self.path) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

fn free_port() -> Result<u16, String> {
    let l = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    Ok(l.local_addr().map_err(|e| e.to_string())?.port())
}

fn unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn tmp_canon() -> PathBuf {
    let tmp = std::env::temp_dir();
    tmp.canonicalize().unwrap_or(tmp)
}

fn under_tmp(p: &Path) -> bool {
    let t = tmp_canon();
    p.canonicalize().is_ok_and(|c| c.starts_with(&t) && c != t)
}

/// Guardia previa a lanzar nada: `--hooks` debe ser un directorio con `state/`.
fn guard_hooks(hooks: &Path) -> Result<(), String> {
    if !hooks.is_dir() || !hooks.join("state").is_dir() {
        return Err(format!(
            "--hooks {} no es un directorio con state/ dentro",
            hooks.display()
        ));
    }
    Ok(())
}

/// Todo `HOME`/`TMUX_TMPDIR` de hijo debe colgar de `temp_dir()`.
fn guard_home(home: &Path) -> Result<(), String> {
    if under_tmp(home) {
        Ok(())
    } else {
        Err(format!(
            "HOME {} no está bajo {}: se aborta",
            home.display(),
            tmp_canon().display()
        ))
    }
}

fn target_of(link: &Path) -> Option<PathBuf> {
    let t = fs::read_link(link).ok()?;
    let abs = if t.is_absolute() {
        t
    } else {
        link.parent()?.join(t)
    };
    abs.canonicalize().ok()
}

/// Symlinks de la copia cuyo destino cae fuera de las raíces permitidas (o está roto).
pub fn outside_links(root: &Path, allowed: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for e in fs::read_dir(&dir).into_iter().flatten().flatten() {
            let p = e.path();
            let Ok(md) = fs::symlink_metadata(&p) else {
                continue;
            };
            if md.is_dir() {
                stack.push(p);
            } else if md.file_type().is_symlink() {
                match target_of(&p) {
                    Some(t) if allowed.iter().any(|a| t.starts_with(a)) => {}
                    _ => out.push(p),
                }
            }
        }
    }
    out
}

/// Sustituye cada symlink externo por una copia del archivo (si es regular y no enorme) o lo
/// borra (directorio, roto o demasiado grande). Devuelve cuántos tocó.
pub fn sanitize_links(root: &Path, allowed: &[PathBuf]) -> Result<usize, String> {
    let bad = outside_links(root, allowed);
    for p in &bad {
        let target = target_of(p);
        fs::remove_file(p).map_err(|e| format!("{}: {e}", p.display()))?;
        if let Some(t) = target
            && t.is_file()
            && fs::metadata(&t).is_ok_and(|m| m.len() <= MAX_LINK_COPY)
        {
            fs::copy(&t, p).map_err(|e| format!("{}: {e}", p.display()))?;
        }
    }
    Ok(bad.len())
}

/// Copia `--hooks` (`cp -a`), quita lo que activa servicios externos y neutraliza symlinks.
fn copy_hooks(src: &Path, home: &Path, repo: &Path) -> Result<PathBuf, String> {
    let dest = home.join(".claude").join("hooks");
    fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    guard_home(&dest)?;
    // Los hooks reales están vivos: un archivo (p. ej. un -wal de SQLite) puede desaparecer
    // a mitad de la copia. Eso se tolera; cualquier otro error de `cp` no.
    let out = Command::new("cp")
        .arg("-a")
        .arg(format!("{}/.", src.display()))
        .arg(&dest)
        .output()
        .map_err(|e| format!("cp: {e}"))?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success()
        && stderr
            .lines()
            .any(|l| !l.is_empty() && !l.contains("No such file or directory"))
    {
        return Err(format!("cp -a {} falló: {stderr}", src.display()));
    }
    for e in fs::read_dir(&dest).map_err(|e| e.to_string())?.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if STRIP_FILES
            .iter()
            .any(|f| name == *f || name.starts_with(&format!("{f}.")))
        {
            let _ = fs::remove_file(e.path());
        }
    }
    let allowed = [tmp_canon(), repo.to_path_buf()];
    sanitize_links(&dest, &allowed)?;
    let left = outside_links(&dest, &allowed);
    if !left.is_empty() {
        return Err(format!("quedan symlinks externos en la copia: {left:?}"));
    }
    Ok(dest)
}

/// Directorio estático común: symlinks a `<repo>/dash/*` más `assets -> <repo>/assets`.
fn stage_dash(repo: &Path, dest: &Path) -> Result<(), String> {
    fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    for e in fs::read_dir(repo.join("dash"))
        .map_err(|e| e.to_string())?
        .flatten()
    {
        std::os::unix::fs::symlink(e.path(), dest.join(e.file_name()))
            .map_err(|e| e.to_string())?;
    }
    let assets = repo.join("assets");
    if assets.is_dir() && !dest.join("assets").exists() {
        std::os::unix::fs::symlink(assets, dest.join("assets")).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Ejecutables que no hacen nada, antepuestos al PATH de los hijos.
fn make_fakebin(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    for n in FAKE_BINS {
        let p = dir.join(n);
        fs::write(&p, "#!/bin/sh\nexit 0\n").map_err(|e| e.to_string())?;
        fs::set_permissions(&p, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .map_err(|e| e.to_string())?;
    }
    // Los hijos (oráculo, heredado y el frente, cuyo `Tmux::system()` busca
    // `tmux` en el PATH) llaman a tmux con `-S` al socket de SU `TMUX_TMPDIR`:
    // tmux 3.2a ignora un `TMUX_TMPDIR` que no existe y caería en el servidor
    // real del usuario. Sin `TMUX_TMPDIR` el envoltorio se niega a correr.
    // El envoltorio se escribe siempre: sin tmux real falla (`exit 1`) en vez
    // de dejar que el PATH encuentre otro `tmux` sin `-S` (falla cerrado).
    let script = tmux_wrapper(
        ["/usr/bin/tmux", "/bin/tmux", "/usr/local/bin/tmux"]
            .into_iter()
            .find(|p| Path::new(p).is_file()),
        nix::unistd::getuid().as_raw(),
    );
    let p = dir.join("tmux");
    fs::write(&p, script).map_err(|e| e.to_string())?;
    fs::set_permissions(&p, std::os::unix::fs::PermissionsExt::from_mode(0o755))
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// El guion del `tmux` del fakebin: `-S` al socket de `TMUX_TMPDIR`, o
/// `exit 1` si falta `TMUX_TMPDIR` o no hay tmux real.
fn tmux_wrapper(real: Option<&str>, uid: u32) -> String {
    slow_tmux_wrapper(real, uid, 0)
}

/// `tmux_wrapper` con `list-panes` retrasado `delay_ms` (tmux lento en
/// `poll --shadow --slow-tmux-ms`); 0 es el envoltorio de siempre.
fn slow_tmux_wrapper(real: Option<&str>, uid: u32, delay_ms: u64) -> String {
    let pause = if delay_ms == 0 {
        String::new()
    } else {
        format!(
            "case \"$1\" in list-panes) sleep {}.{:03};; esac\n",
            delay_ms / 1000,
            delay_ms % 1000
        )
    };
    match real {
        Some(real) => format!(
            "#!/bin/sh\n[ -n \"$TMUX_TMPDIR\" ] || exit 1\n{pause}\
             exec {real} -S \"$TMUX_TMPDIR/tmux-{uid}/default\" \"$@\"\n"
        ),
        None => "#!/bin/sh\necho 'xtask: sin tmux real; el envoltorio no corre' >&2\nexit 1\n"
            .to_owned(),
    }
}

/// Opciones de la pila aislada: las de la 2a (hooks, binario, `--keep`) más las de la 2b.
pub struct StackOptions<'a> {
    pub hooks: &'a Path,
    pub comandos: &'a Path,
    pub keep: bool,
    /// Copia de solo lectura (backup de SQLite) en los dos HOME.
    pub state_db: Option<&'a Path>,
    /// Base de uso copiada (backup de SQLite) a `~/.claude/hooks/comandos-usage.sqlite`
    /// de los dos HOME.
    pub usage_db: Option<&'a Path>,
    /// Pasa `--no-native` al frente: A/B contra la 2a.
    pub no_native: bool,
}

/// Copia la base con la API de backup: nunca escribe en `src`.
fn copy_state_db(src: &Path, home: &Path) -> Result<(), String> {
    let dest = home.join(".local/state/comandos/app-state.sqlite3");
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let source = rusqlite::Connection::open_with_flags(
        src,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("--state-db {}: {e}", src.display()))?;
    source
        .backup(rusqlite::MAIN_DB, &dest, None)
        .map_err(|e| format!("backup de {}: {e}", src.display()))
}

/// Copia la base de uso con la API de backup sobre la que trajo `cp -a`
/// (que puede estar a medias si la real tenía WAL): nunca escribe en `src`.
fn copy_usage_db(src: &Path, home: &Path) -> Result<(), String> {
    let dest = home.join(".claude/hooks/comandos-usage.sqlite");
    for suffix in ["", "-wal", "-shm"] {
        let mut name = dest.as_os_str().to_owned();
        name.push(suffix);
        let _ = fs::remove_file(PathBuf::from(name));
    }
    let source = rusqlite::Connection::open_with_flags(
        src,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("--usage-db {}: {e}", src.display()))?;
    source
        .backup(rusqlite::MAIN_DB, &dest, None)
        .map_err(|e| format!("backup de {}: {e}", src.display()))
}

/// Sesiones `local` + claves de `app-tabs.json` (≤20, nombres válidos), creadas
/// en el MISMO orden en los dos servidores privados: mismos `%pane` y `$id`.
fn tmux_sessions_for(hooks: &Path) -> Vec<String> {
    let mut names = vec!["local".to_string()];
    if let Ok(text) = fs::read_to_string(hooks.join("app-tabs.json"))
        && let Ok(Value::Object(tabs)) = serde_json::from_str::<Value>(&text)
    {
        for key in tabs.keys() {
            let valid = (1..=80).contains(&key.len())
                && key
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
            if valid && !names.contains(key) && names.len() < 21 {
                names.push(key.clone());
            }
        }
    }
    names
}

fn tmux_available() -> bool {
    Command::new("tmux")
        .arg("-V")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Servidor tmux privado en `socket_dir`. `-f /dev/null`: el servidor nace sin
/// la configuración del usuario (sus `run-shell` y plugins nunca corren aquí).
fn start_tmux(socket_dir: &Path, sessions: &[String]) -> Result<(), String> {
    for name in sessions {
        let status = private_tmux(socket_dir)
            .args(["-f", "/dev/null", "new-session", "-d", "-s", name, "cat"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| format!("tmux: {e}"))?;
        if !status.success() {
            return Err(format!("tmux new-session {name} falló"));
        }
    }
    Ok(())
}

/// `#{session_activity}` (segundos enteros) de cada sesión del servidor privado.
fn tmux_activity(socket_dir: &Path) -> Result<Vec<String>, String> {
    let out = private_tmux(socket_dir)
        .args(["list-sessions", "-F", "#{session_activity}"])
        .stderr(Stdio::null())
        .output()
        .map_err(|e| format!("tmux: {e}"))?;
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_owned)
        .collect())
}

/// Duerme hasta poco después del próximo cambio de segundo del reloj.
fn wait_next_second() {
    let into = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_millis())
        .unwrap_or(0);
    thread::sleep(Duration::from_millis(u64::from(1000 - into) + 20));
}

/// Los dos servidores privados con las mismas sesiones y la MISMA
/// `session_activity` en todas. `read_states` del Python ordena por ella (en
/// segundos enteros): si la creación cruza un cambio de segundo, los empates
/// se rompen distinto a cada lado y `/state` difiere sin que nada haya cambiado.
fn start_tmux_pair(a: &Path, b: &Path, sessions: &[String]) -> Result<(), String> {
    const TRIES: usize = 5;
    for _ in 0..TRIES {
        // Empezar justo tras un cambio de segundo deja casi un segundo entero.
        wait_next_second();
        start_tmux(a, sessions)?;
        start_tmux(b, sessions)?;
        let mut stamps = tmux_activity(a)?;
        stamps.extend(tmux_activity(b)?);
        stamps.sort();
        stamps.dedup();
        if stamps.len() <= 1 {
            return Ok(());
        }
        kill_tmux(a);
        kill_tmux(b);
    }
    Err(format!(
        "tmux privado: tras {TRIES} intentos las sesiones de los dos servidores \
         no comparten una única session_activity ({} sesiones por lado)",
        sessions.len()
    ))
}

/// `tmux` contra el servidor privado de `socket_dir` y solo contra ese: el
/// socket va explícito con `-S`, no solo por `TMUX_TMPDIR`. tmux 3.2a ignora
/// en silencio un `TMUX_TMPDIR` cuyo directorio no existe y cae en
/// `/tmp/tmux-<uid>/default`, el servidor real del usuario; con `-S` un
/// directorio borrado da «no server running» y nunca un `kill-server` ajeno.
fn private_tmux(socket_dir: &Path) -> Command {
    let socket = socket_dir
        .join(format!("tmux-{}", nix::unistd::getuid().as_raw()))
        .join("default");
    if let Some(parent) = socket.parent() {
        // 0700 como lo crea tmux: con bits de «otros» rechaza el directorio.
        let _ = fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent);
    }
    let mut cmd = Command::new("tmux");
    cmd.arg("-S")
        .arg(socket)
        .env_remove("TMUX")
        .env("TMUX_TMPDIR", socket_dir);
    cmd
}

fn kill_tmux(socket_dir: &Path) {
    let _ = private_tmux(socket_dir)
        .arg("kill-server")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

struct SpawnEnv<'a> {
    home: &'a Path,
    tmux: &'a Path,
    dash: &'a Path,
    fakebin: &'a Path,
}

fn spawn(name: &'static str, mut cmd: Command, e: &SpawnEnv, log: &Path) -> Result<Server, String> {
    guard_home(e.home)?;
    guard_home(e.tmux)?;
    let out = fs::File::create(log).map_err(|e| e.to_string())?;
    let err = out.try_clone().map_err(|e| e.to_string())?;
    let path = format!(
        "{}:{}",
        e.fakebin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    // El namespace de red no aísla los sockets Unix con ruta: sin estas variables el oráculo
    // no encuentra el systemd, el DBus ni la pantalla reales.
    cmd.env_remove("TMUX")
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CODEX_HOME")
        .env_remove("GROK_HOME")
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        // La base de estado nunca es la real: ni override ni XDG del usuario.
        .env_remove("COMANDOS_STATE_DB")
        .env_remove("COMANDOS_USAGE_DB")
        .env_remove(NATIVE_ENV_OF_FRONT)
        // La carpeta de las terminales rápidas tampoco: sin esto,
        // `d-quick-barra` crearía carpetas en la del desarrollador.
        .env_remove("COMANDOS_QUICK_TERMINAL_BASE")
        .env("XDG_STATE_HOME", e.home.join(".local/state"))
        .env("XDG_RUNTIME_DIR", e.tmux)
        .env("HOME", e.home)
        .env("TMUX_TMPDIR", e.tmux)
        .env("COMANDOS_DASH_DIR", e.dash)
        .env("PATH", path)
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err)
        .process_group(0);
    let child = cmd.spawn().map_err(|e| format!("{name}: {e}"))?;
    Ok(Server { child, name })
}

fn wait_ready(srv: &mut Server, port: u16) -> Result<(), String> {
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(20) {
        if let Ok(Some(st)) = srv.child.try_wait() {
            return Err(format!("{} terminó al arrancar ({st})", srv.name));
        }
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err(format!("{} no abrió el puerto {port} en 20 s", srv.name))
}

/// Tres procesos y dos copias de hooks: Python A (copia 1) es el oráculo directo; el frente
/// Rust B y un segundo Python C (ambos copia 2) son el lado bajo prueba (B reenvía a C).
#[allow(dead_code)] // campos de diagnóstico y de propiedad (drop)
pub struct Stack {
    // `Drop::drop` mata primero los tmux privados; luego, en el orden de los campos, los
    // procesos y al final se borra la raíz.
    servers: Vec<Server>,
    pub token: String,
    pub p_py: u16,
    pub p_front: u16,
    pub p_legacy: u16,
    pub pid_py: u32,
    pub pid_front: u32,
    pub pid_legacy: u32,
    tmux_dirs: Vec<PathBuf>,
    front_log: PathBuf,
    _root: TempRoot,
}

impl Drop for Stack {
    fn drop(&mut self) {
        for dir in &self.tmux_dirs {
            kill_tmux(dir);
        }
    }
}

/// Mata los tmux privados si `start_with` falla a mitad (antes de existir `Stack`).
struct TmuxGuard(Vec<PathBuf>);

impl Drop for TmuxGuard {
    fn drop(&mut self) {
        for dir in &self.0 {
            kill_tmux(dir);
        }
    }
}

impl Stack {
    /// Levanta la pila aislada (`parity` y `poll --shadow` la usan con sus opciones).
    pub fn start_with(o: StackOptions) -> Result<Stack, String> {
        let (hooks, comandos, keep) = (o.hooks, o.comandos, o.keep);
        ensure_isolated_marker()?;
        guard_hooks(hooks)?;
        let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .canonicalize()
            .map_err(|e| e.to_string())?;
        if !comandos.is_file() {
            return Err(format!(
                "no existe {} (cargo build -p comandos-cli, o --comandos)",
                comandos.display()
            ));
        }
        let path = std::env::temp_dir().join(format!(
            "comandos-parity-{}-{}",
            std::process::id(),
            unix_secs()
        ));
        fs::create_dir_all(&path).map_err(|e| e.to_string())?;
        let root = TempRoot {
            path: path.clone(),
            keep,
        };
        let d = |n: &str| path.join(n);
        let (home1, home2, tmux1, tmux2) = (d("home1"), d("home2"), d("tmux1"), d("tmux2"));
        for x in [&home1, &home2, &tmux1, &tmux2] {
            fs::create_dir_all(x).map_err(|e| e.to_string())?;
            guard_home(x)?;
        }
        let hooks1 = copy_hooks(hooks, &home1, &repo)?;
        copy_hooks(hooks, &home2, &repo)?;
        if let Some(db) = o.state_db {
            copy_state_db(db, &home1)?;
            copy_state_db(db, &home2)?;
        }
        if let Some(db) = o.usage_db {
            copy_usage_db(db, &home1)?;
            copy_usage_db(db, &home2)?;
        }
        let token = fs::read_to_string(hooks1.join("dash-token"))
            .map(|t| t.trim().to_string())
            .unwrap_or_default();
        let (dash, fakebin) = (d("dash"), d("fakebin"));
        stage_dash(&repo, &dash)?;
        make_fakebin(&fakebin)?;
        // Desde aquí los dos servidores tmux privados mueren pase lo que pase.
        let mut tmux_guard = TmuxGuard(vec![tmux1.clone(), tmux2.clone()]);
        let sessions = tmux_sessions_for(&hooks1);
        if tmux_available() {
            start_tmux_pair(&tmux1, &tmux2, &sessions)?;
        }

        let (p_py, p_front, p_legacy) = (free_port()?, free_port()?, free_port()?);
        let py = |port: u16| {
            let mut c = Command::new("python3");
            c.arg(repo.join("bin/cc-dash"))
                .arg(port.to_string())
                .arg("--no-open");
            c
        };
        let mut front = Command::new(comandos);
        front.args([
            "dash",
            &p_front.to_string(),
            "--no-open",
            "--legacy-port",
            &p_legacy.to_string(),
        ]);
        if o.no_native {
            front.arg("--no-native");
        }
        // `spawn` no la quita: el resumen de reenvíos sale de esta traza.
        front.env("COMANDOS_DASH_TRACE_FORWARD", "1");
        let e1 = SpawnEnv {
            home: &home1,
            tmux: &tmux1,
            dash: &dash,
            fakebin: &fakebin,
        };
        let e2 = SpawnEnv {
            home: &home2,
            tmux: &tmux2,
            dash: &dash,
            fakebin: &fakebin,
        };
        // Locales sueltos en orden inverso si algo falla a mitad: ningún proceso queda vivo.
        let mut s_py = spawn("cc-dash (oráculo)", py(p_py), &e1, &d("py.log"))?;
        let mut s_leg = spawn("cc-dash (heredado)", py(p_legacy), &e2, &d("legacy.log"))?;
        let front_log = d("front.log");
        let mut s_front = spawn("comandos dash", front, &e2, &front_log)?;
        wait_ready(&mut s_py, p_py)?;
        wait_ready(&mut s_leg, p_legacy)?;
        wait_ready(&mut s_front, p_front)?;
        let pids = [s_py.child.id(), s_leg.child.id(), s_front.child.id()];
        let servers = vec![s_py, s_leg, s_front];
        Ok(Stack {
            servers,
            token,
            p_py,
            p_front,
            p_legacy,
            pid_py: pids[0],
            pid_legacy: pids[1],
            pid_front: pids[2],
            tmux_dirs: std::mem::take(&mut tmux_guard.0),
            front_log,
            _root: root,
        })
    }

    /// `tmux <args>` contra el servidor privado del frente (y de su heredado),
    /// nunca contra el del usuario: `TMUX` fuera y `TMUX_TMPDIR` de la pila.
    pub fn front_tmux(&self, args: &[&str]) -> Result<(), String> {
        let dir = self
            .tmux_dirs
            .get(1)
            .ok_or("la pila no tiene tmux privado del frente")?;
        guard_home(dir)?;
        let status = private_tmux(dir)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| format!("tmux: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!(
                "tmux {} falló en el servidor privado",
                args.join(" ")
            ))
        }
    }

    /// Salida de `tmux <args>` contra el servidor privado del frente (`-S`).
    pub fn front_tmux_output(&self, args: &[&str]) -> Result<String, String> {
        let dir = self
            .tmux_dirs
            .get(1)
            .ok_or("la pila no tiene tmux privado del frente")?;
        guard_home(dir)?;
        let out = private_tmux(dir)
            .args(args)
            .stderr(Stdio::null())
            .output()
            .map_err(|e| format!("tmux: {e}"))?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            Err(format!(
                "tmux {} falló en el servidor privado",
                args.join(" ")
            ))
        }
    }

    /// HOME del frente y de su heredado (la copia 2), siempre bajo el temporal.
    pub fn front_home(&self) -> Result<PathBuf, String> {
        let home = self._root.path.join("home2");
        guard_home(&home)?;
        Ok(home)
    }

    /// Raíz temporal de la pila (para ejecutables falsos de `poll --shadow`).
    pub fn root(&self) -> &Path {
        &self._root.path
    }

    /// Retrasa `delay_ms` cada `tmux list-panes` de los hijos (el frente y el
    /// heredado buscan `tmux` en el PATH, que empieza por el fakebin). El
    /// envoltorio sigue pasando `-S` al socket privado.
    pub fn slow_tmux(&self, delay_ms: u64) -> Result<(), String> {
        let path = self._root.path.join("fakebin").join("tmux");
        let script = slow_tmux_wrapper(
            ["/usr/bin/tmux", "/bin/tmux", "/usr/local/bin/tmux"]
                .into_iter()
                .find(|p| Path::new(p).is_file()),
            nix::unistd::getuid().as_raw(),
            delay_ms,
        );
        // Escribir aparte y renombrar: nunca se ejecuta un guion a medias.
        let staged = path.with_extension("lento");
        fs::write(&staged, script).map_err(|e| e.to_string())?;
        fs::set_permissions(&staged, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .map_err(|e| e.to_string())?;
        fs::rename(&staged, &path).map_err(|e| e.to_string())
    }

    /// Rutas que el frente reenvió al heredado (de su traza), con su cuenta.
    pub fn forwarded_summary(&self) -> Vec<(String, usize)> {
        let text = fs::read_to_string(&self.front_log).unwrap_or_default();
        let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
        for line in text.lines() {
            if let Some(route) = line.strip_prefix("comandos dash: reenvío ") {
                *counts.entry(route.to_string()).or_default() += 1;
            }
        }
        counts.into_iter().collect()
    }
}

/// Comprobación de que `Stack::start_with` solo corre dentro del namespace aislado.
fn ensure_isolated_marker() -> Result<(), String> {
    if std::env::var(NETNS_ENV).as_deref() == Ok("1") {
        Ok(())
    } else {
        Err("el arnés debe correr dentro del namespace de red (ensure_isolated)".into())
    }
}

// ------------------------------------------------------------------- ejecución

struct Case {
    name: String,
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
    volatile: Vec<String>,
    expect: Expect,
    forwarded: bool,
    reason: String,
    /// Ruta que se pide al oráculo si difiere (p. ej. `/operator` por una ruta retirada).
    oracle_path: Option<String>,
}

fn load_fixture(path: &Path) -> Result<Vec<Case>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut cases = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let v: Value = serde_json::from_str(line).map_err(|e| format!("línea {}: {e}", i + 1))?;
        let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
        let expect = s("expect")
            .as_deref()
            .and_then(Expect::parse)
            .ok_or_else(|| format!("línea {}: expect inválido", i + 1))?;
        let headers = v
            .get("headers")
            .and_then(Value::as_object)
            .map(|o| {
                o.iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let body = match v.get("body") {
            None | Some(Value::Null) => None,
            Some(Value::String(t)) => Some(t.clone().into_bytes()),
            Some(other) => Some(other.to_string().into_bytes()),
        };
        cases.push(Case {
            name: s("name").ok_or_else(|| format!("línea {}: falta name", i + 1))?,
            method: s("method").unwrap_or_else(|| "GET".into()),
            path: s("path").ok_or_else(|| format!("línea {}: falta path", i + 1))?,
            headers,
            body,
            volatile: v
                .get("volatile")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
            expect,
            forwarded: v.get("forwarded").and_then(Value::as_bool).unwrap_or(false),
            reason: s("reason").unwrap_or_default(),
            oracle_path: s("oracle_path"),
        });
    }
    Ok(cases)
}

/// ¿El frente dice que el heredado no está disponible? Solo entonces una ruta reenviada
/// se marca SKIP; con el heredado vivo cualquier otra cosa se compara.
fn is_legacy_down(r: &Resp) -> bool {
    r.status == 502 && String::from_utf8_lossy(&r.body).contains("Servidor heredado no disponible")
}

struct Args {
    fixture: PathBuf,
    hooks: PathBuf,
    keep: bool,
    comandos: Option<PathBuf>,
    /// `--state-db <ruta>`: base de estado copiada (solo lectura) a los dos HOME.
    state_db: Option<PathBuf>,
    /// `--usage-db <ruta>`: base de uso copiada (solo lectura) a los dos HOME.
    usage_db: Option<PathBuf>,
    /// `--no-native`: el frente reenvía todo, como en la 2a.
    no_native: bool,
}

fn parse_args(args: &[String]) -> Result<Args, String> {
    let (mut fixture, mut hooks, mut keep, mut comandos) = (None, None, false, None);
    let (mut state_db, mut usage_db, mut no_native) = (None, None, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--fixture" => fixture = it.next().map(PathBuf::from),
            "--hooks" => hooks = it.next().map(PathBuf::from),
            "--comandos" => comandos = it.next().map(PathBuf::from),
            "--state-db" => {
                state_db = Some(it.next().map(PathBuf::from).ok_or("--state-db sin ruta")?);
            }
            "--usage-db" => {
                usage_db = Some(it.next().map(PathBuf::from).ok_or("--usage-db sin ruta")?);
            }
            "--keep" => keep = true,
            "--no-native" => no_native = true,
            other => return Err(format!("argumento desconocido: {other}")),
        }
    }
    Ok(Args {
        fixture: fixture.ok_or("falta --fixture")?,
        hooks: hooks.ok_or("falta --hooks")?,
        keep,
        comandos,
        state_db,
        usage_db,
        no_native,
    })
}

pub fn default_comandos() -> Result<PathBuf, String> {
    Ok(std::env::current_exe()
        .map_err(|e| e.to_string())?
        .with_file_name("comandos"))
}

/// Devuelve el código de salida (1 si hay `DIFF`).
pub fn run(args: &[String]) -> Result<i32, String> {
    let a = parse_args(args)?;
    let cases = load_fixture(&a.fixture)?;
    let comandos = match a.comandos {
        Some(p) => p,
        None => default_comandos()?,
    };
    let stack = Stack::start_with(StackOptions {
        hooks: &a.hooks,
        comandos: &comandos,
        keep: a.keep,
        state_db: a.state_db.as_deref(),
        usage_db: a.usage_db.as_deref(),
        no_native: a.no_native,
    })?;
    let results = std::env::temp_dir().join(format!("comandos-parity-results-{}", unix_secs()));

    let (mut ok, mut diff, mut skip) = (0, 0, 0);
    println!("{:<28} {:<7} detalle", "petición", "estado");
    for (n, c) in cases.iter().enumerate() {
        let headers: Vec<(String, String)> = c
            .headers
            .iter()
            .map(|(k, v)| (k.clone(), v.replace("{{token}}", &stack.token)))
            .collect();
        let go = |port, path: &str| request(port, &c.method, path, &headers, c.body.as_deref());
        let oracle = c.oracle_path.as_deref().unwrap_or(&c.path);
        let (r_py, r_rs) = match (go(stack.p_py, oracle), go(stack.p_front, &c.path)) {
            (Ok(a), Ok(b)) => (a, b),
            (a, b) => {
                println!(
                    "{:<28} DIFF    error de transporte py={:?} rs={:?}",
                    c.name,
                    a.err(),
                    b.err()
                );
                diff += 1;
                continue;
            }
        };
        let outcome = if c.expect == Expect::Skip {
            Outcome::Skip(if c.reason.is_empty() {
                "fixture".into()
            } else {
                c.reason.clone()
            })
        } else if c.forwarded && is_legacy_down(&r_rs) {
            Outcome::Skip("reenviada, heredado caído".into())
        } else {
            compare(&r_py, &r_rs, c.expect, &c.volatile)
        };
        match &outcome {
            Outcome::Ok => {
                ok += 1;
                println!("{:<28} OK      {} {}", c.name, r_py.status, c.path);
            }
            Outcome::Skip(why) => {
                skip += 1;
                println!(
                    "{:<28} SKIP    ({why}) py={} rs={}",
                    c.name, r_py.status, r_rs.status
                );
            }
            Outcome::Diff(why) => {
                diff += 1;
                println!(
                    "{:<28} DIFF    {}",
                    c.name,
                    why.lines().next().unwrap_or("")
                );
                let _ = fs::create_dir_all(&results);
                let dump = |ext: &str, r: &Resp| {
                    let mut t = format!("HTTP {}\n", r.status);
                    for (k, v) in &r.headers {
                        t += &format!("{k}: {v}\n");
                    }
                    let mut bytes = t.into_bytes();
                    bytes.extend_from_slice(b"\n");
                    bytes.extend_from_slice(&r.body);
                    let _ = fs::write(results.join(format!("{}.{ext}", n + 1)), bytes);
                };
                dump("py", &r_py);
                dump("rs", &r_rs);
            }
        }
    }
    let forwarded = stack.forwarded_summary();
    println!(
        "\nreenviadas al heredado por el frente ({} rutas distintas):",
        forwarded.len()
    );
    for (route, n) in &forwarded {
        println!("  {n:>4}  {route}");
    }
    drop(stack);
    println!(
        "\nresumen: {ok} OK, {diff} DIFF, {skip} SKIP de {}",
        cases.len()
    );
    if diff > 0 {
        println!("pares que difieren en {}", results.display());
    }
    Ok(i32::from(diff > 0))
}

#[cfg(test)]
mod tests {
    use super::{slow_tmux_wrapper, tmux_wrapper};

    #[test]
    fn slow_wrapper_delays_only_list_panes_and_keeps_the_socket() {
        let slow = slow_tmux_wrapper(Some("/usr/bin/tmux"), 1000, 1500);
        assert!(slow.contains("case \"$1\" in list-panes) sleep 1.500;; esac"));
        assert!(slow.contains("[ -n \"$TMUX_TMPDIR\" ] || exit 1"));
        assert!(slow.contains("exec /usr/bin/tmux -S \"$TMUX_TMPDIR/tmux-1000/default\" \"$@\""));
        assert_eq!(
            slow_tmux_wrapper(Some("/usr/bin/tmux"), 1000, 0),
            tmux_wrapper(Some("/usr/bin/tmux"), 1000)
        );
    }

    #[test]
    fn tmux_wrapper_fails_closed_without_real_tmux() {
        let none = tmux_wrapper(None, 1000);
        assert!(none.contains("exit 1"));
        assert!(!none.contains("exec"));
        let some = tmux_wrapper(Some("/usr/bin/tmux"), 1000);
        assert!(some.contains("[ -n \"$TMUX_TMPDIR\" ] || exit 1"));
        assert!(some.contains("exec /usr/bin/tmux -S \"$TMUX_TMPDIR/tmux-1000/default\" \"$@\""));
    }
}

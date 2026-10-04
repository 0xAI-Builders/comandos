//! Arnés diferencial: la misma petición contra `bin/cc-dash` (oráculo Python) y contra
//! `comandos dash` (frente Rust), ambos sobre copias privadas de `~/.claude/hooks`.
//!
//! Herramienta de desarrollo, no código de producto. El Python solo corre como oráculo.
//! Reglas de oro: jamás toca el `~/.claude/hooks` real, los puertos 4777/4778/4781 ni el
//! tmux del usuario; todo vive bajo `std::env::temp_dir()`.

use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde_json::Value;

const MASK: &str = "<volátil>";
/// Cabeceras que `same` compara; el resto (Date, Server…) es ruido por construcción.
const RELEVANT_HEADERS: [&str; 3] = ["content-type", "content-length", "cache-control"];

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
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
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

fn body_diff(py: &Resp, rs: &Resp, volatile: &[String]) -> Option<String> {
    let parsed = (
        serde_json::from_slice::<Value>(&py.body),
        serde_json::from_slice::<Value>(&rs.body),
    );
    if let (Ok(mut a), Ok(mut b)) = parsed {
        normalize(&mut a, volatile);
        normalize(&mut b, volatile);
        // Igualdad semántica: el espaciado de `json.dumps` no cuenta, los valores sí.
        return (a != b).then(|| format!("cuerpo JSON distinto:\n  py: {a}\n  rs: {b}"));
    }
    (py.body != rs.body).then(|| {
        format!(
            "cuerpo distinto ({} B py, {} B rs)",
            py.body.len(),
            rs.body.len()
        )
    })
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
    if expect == Expect::Same {
        for h in RELEVANT_HEADERS {
            if py.header(h) != rs.header(h) {
                return Outcome::Diff(format!(
                    "cabecera {h}: {:?} (py) vs {:?} (rs)",
                    py.header(h),
                    rs.header(h)
                ));
            }
        }
    }
    if expect == Expect::StatusOnly {
        return Outcome::Ok;
    }
    match body_diff(py, rs, volatile) {
        Some(d) => Outcome::Diff(d),
        None => Outcome::Ok,
    }
}

// ------------------------------------------------------------ cliente HTTP/1.1

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn dechunk(mut raw: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let eol = find(raw, b"\r\n").ok_or("chunk sin tamaño")?;
        let line = std::str::from_utf8(&raw[..eol]).map_err(|e| e.to_string())?;
        let size = usize::from_str_radix(line.split(';').next().unwrap_or("").trim(), 16)
            .map_err(|e| format!("tamaño de chunk: {e}"))?;
        raw = &raw[eol + 2..];
        if size == 0 {
            return Ok(out);
        }
        if raw.len() < size + 2 {
            return Err("chunk truncado".into());
        }
        out.extend_from_slice(&raw[..size]);
        raw = &raw[size + 2..];
    }
}

/// Interpreta una respuesta completa leída hasta EOF: `Content-Length`, `chunked` o cuerpo
/// hasta el cierre.
pub fn parse_response(raw: &[u8]) -> Result<Resp, String> {
    let split = find(raw, b"\r\n\r\n").ok_or("respuesta sin cabeceras")?;
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
    let body =
        if get("transfer-encoding").is_some_and(|v| v.to_ascii_lowercase().contains("chunked")) {
            dechunk(rest)?
        } else if let Some(n) = get("content-length").and_then(|v| v.parse::<usize>().ok()) {
            rest[..n.min(rest.len())].to_vec()
        } else {
            rest.to_vec()
        };
    Ok(Resp {
        status,
        headers,
        body,
    })
}

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
    if !has("connection") {
        head += "Connection: close\r\n";
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
    let _ = s.read_to_end(&mut raw);
    parse_response(&raw)
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

fn free_port() -> Result<u16, String> {
    let l = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    Ok(l.local_addr().map_err(|e| e.to_string())?.port())
}

fn unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Guardia previa a lanzar nada: `--hooks` debe ser un directorio con `state/` y todo `HOME`
/// de hijo debe colgar de `temp_dir()`.
fn guard_hooks(hooks: &Path) -> Result<(), String> {
    if !hooks.is_dir() || !hooks.join("state").is_dir() {
        return Err(format!(
            "--hooks {} no es un directorio con state/ dentro",
            hooks.display()
        ));
    }
    Ok(())
}

fn guard_home(home: &Path) -> Result<(), String> {
    let tmp = std::env::temp_dir();
    let tmp = tmp.canonicalize().unwrap_or(tmp);
    let h = home
        .canonicalize()
        .map_err(|e| format!("{}: {e}", home.display()))?;
    if !h.starts_with(&tmp) || h == tmp {
        return Err(format!(
            "HOME {} no está bajo {}: se aborta",
            h.display(),
            tmp.display()
        ));
    }
    Ok(())
}

fn copy_hooks(src: &Path, home: &Path) -> Result<PathBuf, String> {
    let dest = home.join(".claude").join("hooks");
    fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    // `cp -a` conserva los symlinks como symlinks (nunca se sigue `dash/` al repo).
    let st = Command::new("cp")
        .arg("-a")
        .arg(format!("{}/.", src.display()))
        .arg(&dest)
        .status()
        .map_err(|e| format!("cp: {e}"))?;
    if !st.success() {
        return Err(format!("cp -a {} falló", src.display()));
    }
    Ok(dest)
}

/// Directorio estático común: symlinks a `<repo>/dash/*` más `assets -> <repo>/assets`,
/// igual que `~/.claude/hooks/dash` en producción, pero sin depender de él.
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

fn spawn(
    name: &'static str,
    mut cmd: Command,
    home: &Path,
    tmux: &Path,
    dash: &Path,
    log: &Path,
) -> Result<Server, String> {
    guard_home(home)?;
    guard_home(tmux)?;
    let out = fs::File::create(log).map_err(|e| e.to_string())?;
    let err = out.try_clone().map_err(|e| e.to_string())?;
    cmd.env_remove("TMUX")
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CODEX_HOME")
        .env_remove("GROK_HOME")
        .env("HOME", home)
        .env("TMUX_TMPDIR", tmux)
        .env("COMANDOS_DASH_DIR", dash)
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
        });
    }
    Ok(cases)
}

/// ¿La respuesta del frente es la de «ruta reenviada con el heredado muerto»? Antes de la
/// Task 4 del plan es un 404 propio; después, un 502.
fn is_forward_marker(r: &Resp) -> bool {
    let body = String::from_utf8_lossy(&r.body);
    (r.status == 502 && body.contains("Servidor heredado no disponible"))
        || (r.status == 404 && body.contains("\"No encontrado\""))
}

struct Args {
    fixture: PathBuf,
    hooks: PathBuf,
    keep: bool,
    comandos: Option<PathBuf>,
}

fn parse_args(args: &[String]) -> Result<Args, String> {
    let (mut fixture, mut hooks, mut keep, mut comandos) = (None, None, false, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--fixture" => fixture = it.next().map(PathBuf::from),
            "--hooks" => hooks = it.next().map(PathBuf::from),
            "--comandos" => comandos = it.next().map(PathBuf::from),
            "--keep" => keep = true,
            other => return Err(format!("argumento desconocido: {other}")),
        }
    }
    Ok(Args {
        fixture: fixture.ok_or("falta --fixture")?,
        hooks: hooks.ok_or("falta --hooks")?,
        keep,
        comandos,
    })
}

/// Devuelve el código de salida (1 si hay `DIFF`).
pub fn run(args: &[String]) -> Result<i32, String> {
    let a = parse_args(args)?;
    guard_hooks(&a.hooks)?;
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let repo = repo.canonicalize().map_err(|e| e.to_string())?;
    let comandos = match a.comandos {
        Some(p) => p,
        None => std::env::current_exe()
            .map_err(|e| e.to_string())?
            .with_file_name("comandos"),
    };
    if !comandos.is_file() {
        return Err(format!(
            "no existe {} (cargo build -p comandos-cli, o --comandos)",
            comandos.display()
        ));
    }
    let cases = load_fixture(&a.fixture)?;

    let root = std::env::temp_dir().join(format!("comandos-parity-{}", unix_secs()));
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let results = root.join(format!("parity-{}", unix_secs()));
    fs::create_dir_all(&results).map_err(|e| e.to_string())?;
    let (home_py, home_rs) = (root.join("home-py"), root.join("home-rs"));
    let (tmux_py, tmux_rs) = (root.join("tmux-py"), root.join("tmux-rs"));
    for d in [&home_py, &home_rs, &tmux_py, &tmux_rs] {
        fs::create_dir_all(d).map_err(|e| e.to_string())?;
        guard_home(d)?;
    }
    // Tras la guardia: ya se puede copiar y lanzar.
    let hooks_py = copy_hooks(&a.hooks, &home_py)?;
    copy_hooks(&a.hooks, &home_rs)?;
    let token = fs::read_to_string(hooks_py.join("dash-token"))
        .map(|t| t.trim().to_string())
        .unwrap_or_default();
    let dash = root.join("dash");
    stage_dash(&repo, &dash)?;

    let (p_py, p_rs, p_dead) = (free_port()?, free_port()?, free_port()?);
    let mut py_cmd = Command::new("python3");
    py_cmd
        .arg(repo.join("bin/cc-dash"))
        .arg(p_py.to_string())
        .arg("--no-open");
    let mut rs_cmd = Command::new(&comandos);
    rs_cmd.args([
        "dash",
        &p_rs.to_string(),
        "--no-open",
        "--legacy-port",
        &p_dead.to_string(),
    ]);
    let mut py = spawn(
        "cc-dash",
        py_cmd,
        &home_py,
        &tmux_py,
        &dash,
        &root.join("py.log"),
    )?;
    let mut rs = spawn(
        "comandos dash",
        rs_cmd,
        &home_rs,
        &tmux_rs,
        &dash,
        &root.join("rs.log"),
    )?;
    wait_ready(&mut py, p_py)?;
    wait_ready(&mut rs, p_rs)?;

    let (mut ok, mut diff, mut skip) = (0, 0, 0);
    println!("{:<28} {:<7} detalle", "petición", "estado");
    for (n, c) in cases.iter().enumerate() {
        let headers: Vec<(String, String)> = c
            .headers
            .iter()
            .map(|(k, v)| (k.clone(), v.replace("{{token}}", &token)))
            .collect();
        let go = |port| request(port, &c.method, &c.path, &headers, c.body.as_deref());
        let (r_py, r_rs) = match (go(p_py), go(p_rs)) {
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
        } else if c.forwarded && is_forward_marker(&r_rs) {
            Outcome::Skip("reenviada".into())
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
    drop(py);
    drop(rs);
    println!(
        "\nresumen: {ok} OK, {diff} DIFF, {skip} SKIP de {}",
        cases.len()
    );
    if diff > 0 {
        println!("pares que difieren en {}", results.display());
    }
    // Las copias de hooks pesan cientos de MB: fuera salvo --keep. Los resultados se quedan
    // si hay diferencias.
    if a.keep {
        println!("conservado: {}", root.display());
    } else {
        for d in [&home_py, &home_rs] {
            if guard_home(d).is_ok() {
                let _ = fs::remove_dir_all(d);
            }
        }
        if diff == 0 {
            let _ = fs::remove_dir_all(&root);
        }
    }
    Ok(i32::from(diff > 0))
}

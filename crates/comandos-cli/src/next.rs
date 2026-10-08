//! Session urgency uses the StatusDir authority without changing source state.
use comandos_store::domains::StatusDir;
use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpStream},
    path::{Path, PathBuf},
    time::Duration,
};

pub struct Config {
    pub home: PathBuf,
    /// Always loopback. Callers can supply a private ephemeral test listener.
    pub port: u16,
    pub timeout: Duration,
}
#[derive(Debug)]
pub struct Outcome {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}
impl Outcome {
    fn ok(stdout: String) -> Self {
        Self {
            code: 0,
            stdout,
            stderr: String::new(),
        }
    }
    fn error(stdout: String) -> Self {
        Self {
            code: 1,
            stdout,
            stderr: String::new(),
        }
    }
}

fn pick(home: &Path) -> Result<Option<Value>, String> {
    let rows = StatusDir {
        home,
        dir: home.join(".claude/hooks/state"),
        domain: "session-status",
    }
    .list_readonly()
    .map_err(|e| e.to_string())?;
    let mut items = vec![];
    for (_, body) in rows {
        let Ok(text) = std::str::from_utf8(&body) else {
            continue;
        };
        let Ok(value) = comandos_core::json::workspace_loads(text) else {
            continue;
        };
        if !value.is_object() {
            return Err("estado de sesión no es un objeto JSON".into());
        }
        items.push(value);
    }
    let rank = |v: &Value| match v.get("status").and_then(Value::as_str) {
        Some("waiting") => 0,
        Some("done") => 1,
        _ => 2,
    };
    let Some(priority) = items.iter().map(rank).min() else {
        return Ok(None);
    };
    let mut chosen: Option<Value> = None;
    for value in items.into_iter().filter(|v| rank(v) == priority) {
        let replace = match &chosen {
            None => true,
            Some(old) => {
                let zero = serde_json::json!(0);
                let order = compare_ts(
                    value.get("ts").unwrap_or(&zero),
                    old.get("ts").unwrap_or(&zero),
                )?;
                if priority == 0 {
                    order.is_lt()
                } else {
                    order.is_gt()
                }
            }
        };
        if replace {
            chosen = Some(value);
        }
    }
    Ok(chosen)
}

fn compare_ts(a: &Value, b: &Value) -> Result<std::cmp::Ordering, String> {
    use std::cmp::Ordering;
    enum Number {
        Int(String),
        Float(f64),
    }
    fn number(v: &Value) -> Option<Number> {
        match v {
            Value::Bool(b) => Some(Number::Int(if *b { "1" } else { "0" }.into())),
            Value::Number(n) => {
                let text = n.to_string();
                if !text.contains(['.', 'e', 'E'])
                    && !matches!(text.as_str(), "NaN" | "Infinity" | "-Infinity")
                {
                    Some(Number::Int(text))
                } else {
                    text.parse().ok().map(Number::Float)
                }
            }
            _ => None,
        }
    }
    fn integers(a: &str, b: &str) -> Ordering {
        let digits = |s: &str| {
            let raw = s.strip_prefix('-').unwrap_or(s).trim_start_matches('0');
            (
                s.starts_with('-') && !raw.is_empty(),
                if raw.is_empty() { "0" } else { raw }.to_owned(),
            )
        };
        let (a_negative, a) = digits(a);
        let (b_negative, b) = digits(b);
        match (a_negative, b_negative) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => {
                let order = a.len().cmp(&b.len()).then_with(|| a.cmp(&b));
                if a_negative { order.reverse() } else { order }
            }
        }
    }
    fn int_float(i: &str, f: f64) -> Ordering {
        if f.is_nan() {
            return Ordering::Equal;
        }
        if f == f64::INFINITY {
            return Ordering::Less;
        }
        if f == f64::NEG_INFINITY {
            return Ordering::Greater;
        }
        let result = integers(i, &format!("{:.0}", f.trunc()));
        if result == Ordering::Equal {
            if f.fract() > 0.0 {
                Ordering::Less
            } else if f.fract() < 0.0 {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        } else {
            result
        }
    }
    match (number(a), number(b)) {
        (Some(Number::Int(a)), Some(Number::Int(b))) => Ok(integers(&a, &b)),
        (Some(Number::Float(a)), Some(Number::Float(b))) => {
            Ok(a.partial_cmp(&b).unwrap_or(Ordering::Equal))
        }
        (Some(Number::Int(a)), Some(Number::Float(b))) => Ok(int_float(&a, b)),
        (Some(Number::Float(a)), Some(Number::Int(b))) => Ok(int_float(&b, a).reverse()),
        _ => match (a, b) {
            (Value::String(a), Value::String(b)) => Ok(a.cmp(b)),
            _ => Err("timestamps de sesión no comparables".into()),
        },
    }
}

pub fn run(config: &Config, args: &[String]) -> Outcome {
    let value = match pick(&config.home) {
        Ok(None) => return Outcome::ok("Sin sesiones registradas\n".into()),
        Ok(Some(value)) => value,
        Err(error) => {
            return Outcome {
                code: 1,
                stdout: String::new(),
                stderr: format!("comandos next: {error}\n"),
            };
        }
    };
    let project = match value.get("project") {
        None => "",
        Some(Value::String(project)) => project,
        _ => {
            return Outcome {
                code: 1,
                stdout: String::new(),
                stderr: "comandos next: project de sesión no es texto\n".into(),
            };
        }
    };
    // Original re.sub and Unicode slicing run even before --dry.
    let session = project
        .replace(['.', ':'], "-")
        .chars()
        .take(80)
        .collect::<String>();
    if args.iter().any(|arg| arg == "--dry") {
        let status = value
            .get("status")
            .map_or("None".into(), comandos_core::pomodoro::python_str);
        return Outcome::ok(format!("eleccion: {project} ({status})\n"));
    }
    match post(config, "/focus", serde_json::json!({"session":session})) {
        Ok(()) => Outcome::ok(String::new()),
        Err(ApiError::Http(_)) => match post(
            config,
            "/up",
            serde_json::json!({"session":session,"cwd":value.get("cwd").cloned().unwrap_or(Value::String(String::new()))}),
        ) {
            Ok(()) => Outcome::ok(String::new()),
            Err(error) => {
                Outcome::error(format!("no pude revivir {project}: {}\n", error.message()))
            }
        },
        Err(error) => Outcome::error(format!("cc-dash no responde: {}\n", error.message())),
    }
}
enum ApiError {
    Http(String),
    Other(String),
}
impl ApiError {
    fn message(&self) -> &str {
        match self {
            Self::Http(e) | Self::Other(e) => e,
        }
    }
}
fn url_error(error: std::io::Error) -> ApiError {
    let message = if matches!(
        error.kind(),
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
    ) {
        "timed out".into()
    } else if let Some(code) = error.raw_os_error() {
        format!(
            "[Errno {code}] {}",
            error
                .to_string()
                .split(" (os error")
                .next()
                .unwrap_or("network error")
        )
    } else {
        error.to_string()
    };
    ApiError::Other(format!("<urlopen error {message}>"))
}
fn response_error(error: std::io::Error) -> ApiError {
    if matches!(
        error.kind(),
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
    ) {
        ApiError::Other("timed out".into())
    } else {
        url_error(error)
    }
}
fn line(reader: &mut BufReader<TcpStream>) -> Result<Vec<u8>, ApiError> {
    let mut bytes = vec![];
    reader
        .take(65537)
        .read_until(b'\n', &mut bytes)
        .map_err(response_error)?;
    if bytes.len() > 65536 {
        return Err(ApiError::Other(
            "got more than 65536 bytes when reading header line".into(),
        ));
    }
    Ok(bytes)
}
fn post(config: &Config, path: &str, payload: Value) -> Result<(), ApiError> {
    http(config, "POST", path, Some(&payload.to_string()), 0)
}
fn redirect_target(config: &Config, path: &str, location: &str) -> Option<String> {
    // urllib treats escaped dots and literal backslashes as path data. Keep
    // them opaque while the WHATWG resolver handles reference components.
    let opaque = |value: &str| value.replace('%', "%25").replace('\\', "%5C");
    let base =
        url::Url::parse(&format!("http://127.0.0.1:{}{}", config.port, opaque(path))).ok()?;
    let mut next = base.join(&opaque(location)).ok()?;
    // Compare parsed origins, including scheme/host/port, before sending.
    // Userinfo is outside the configured dash authority as well.
    if next.origin() != base.origin() || !next.username().is_empty() || next.password().is_some() {
        return None;
    }
    // urljoin preserves the supplied path when a reference carries authority.
    // Validate that authority above, then retain its original request bytes.
    let authority = location.strip_prefix("//").or_else(|| {
        let (scheme, rest) = location.split_once(':')?;
        if scheme.eq_ignore_ascii_case("http") {
            rest.strip_prefix("//")
        } else {
            None
        }
    });
    if let Some(authority) = authority {
        let raw = authority
            .find(['/', '?', '#'])
            .map_or("", |at| &authority[at..]);
        let raw = raw.split('#').next().unwrap_or("");
        let mut target = if raw.starts_with('/') {
            raw.to_owned()
        } else {
            format!("/{raw}")
        };
        if target
            .split_once('?')
            .is_some_and(|(_, query)| query.is_empty())
        {
            target.pop();
        }
        return Some(target);
    }
    // Empty-path references retain the raw base path, including dot segments
    // preserved by an earlier authority-bearing redirect. Empty queries inherit.
    let reference = location.split('#').next().unwrap_or("");
    if reference.is_empty() || reference.starts_with('?') {
        return Some(if reference.starts_with('?') && reference.len() > 1 {
            format!("{}{reference}", path.split('?').next().unwrap_or("/"))
        } else {
            path.to_owned()
        });
    }
    // urlunparse omits a trailing delimiter when a supplied path has no query.
    if next.query() == Some("") {
        next.set_query(None);
    }
    next.set_fragment(None);
    // Restore generated backslashes before percents so an original literal
    // %5C (protected as %255C) cannot turn into a backslash during restoration.
    Some(
        next[url::Position::BeforePath..]
            .replace("%5C", "\\")
            .replace("%25", "%"),
    )
}
fn http(
    config: &Config,
    method: &str,
    path: &str,
    body: Option<&str>,
    redirects: u8,
) -> Result<(), ApiError> {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, config.port));
    let mut stream = TcpStream::connect_timeout(&address, config.timeout).map_err(url_error)?;
    stream
        .set_read_timeout(Some(config.timeout))
        .map_err(url_error)?;
    stream
        .set_write_timeout(Some(config.timeout))
        .map_err(url_error)?;
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n",
        config.port
    );
    if let Some(body) = body {
        head.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        ));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).map_err(url_error)?;
    if let Some(body) = body {
        stream.write_all(body.as_bytes()).map_err(url_error)?;
    }
    let mut reader = BufReader::new(stream);
    loop {
        let bytes = line(&mut reader)?;
        if bytes.is_empty() {
            return Err(ApiError::Other(
                "Remote end closed connection without response".into(),
            ));
        }
        let status_line = bytes.iter().map(|b| char::from(*b)).collect::<String>();
        let (version, rest) = status_line
            .trim_end()
            .split_once(char::is_whitespace)
            .unwrap_or((&status_line, ""));
        let rest = rest.trim_start();
        let (status, reason) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        let code = status.parse::<u16>().ok();
        if !version.starts_with("HTTP/") || !code.is_some_and(|c| (100..1000).contains(&c)) {
            return Err(ApiError::Other(status_line));
        }
        if !matches!(version, "HTTP/0.9" | "HTTP/1.0" | "HTTP/1.1") {
            return Err(ApiError::Other(version.to_owned()));
        }
        let code = code.unwrap_or(0);
        let reason = reason.trim();
        let mut location = None;
        for count in 0..=100 {
            let header = line(&mut reader)?;
            if count == 100 {
                return Err(ApiError::Other("got more than 100 headers".into()));
            }
            if header.is_empty() || header == b"\r\n" || header == b"\n" {
                break;
            }
            let text = header.iter().map(|b| char::from(*b)).collect::<String>();
            if let Some((key, value)) = text.split_once(':')
                && key.eq_ignore_ascii_case("location")
            {
                location = Some(value.trim().to_owned());
            }
        }
        if code == 100 {
            continue;
        }
        // urllib changes POST to GET for 301/302/303. This client remains on
        // the configured dash loopback endpoint; other authorities are errors.
        if matches!(code, 301..=303)
            && redirects < 10
            && let Some(location) = location
        {
            let next = redirect_target(config, path, &location)
                .ok_or_else(|| ApiError::Http(format!("HTTP Error {code}: {reason}")))?;
            return http(config, "GET", &next, None, redirects + 1);
        }
        return if (200..300).contains(&code) {
            Ok(())
        } else {
            Err(ApiError::Http(format!("HTTP Error {code}: {reason}")))
        };
    }
}

pub fn main(args: &[String]) -> i32 {
    let Some(home) = std::env::var_os("HOME").filter(|h| !h.is_empty()) else {
        eprintln!("comandos next: HOME ausente");
        return 1;
    };
    let result = run(
        &Config {
            home: home.into(),
            port: 4777,
            timeout: Duration::from_secs(8),
        },
        args,
    );
    print!("{}", result.stdout);
    eprint!("{}", result.stderr);
    result.code
}

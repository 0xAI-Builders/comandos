//! `POST /notify`: el `Handler` de `cc-notifyd` sobre la semántica de
//! `BaseHTTPRequestHandler` (HTTP/1.0, una petición por conexión, un hilo por
//! conexión como `ThreadingTCPServer`). Sin `Server` ni `Date` (ruling 1).
use crate::notice::{Notice, conf_value};
use serde_json::Value;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::Duration;

/// `_MAXLINE` de `http.client` y del `readline(65537)` de la línea de petición.
const MAX_LINE: usize = 65_536;
/// `_MAXHEADERS` de `http.client` (cuenta también la línea en blanco final).
const MAX_HEADERS: usize = 100;
/// Tope de `Content-Length` del `Handler`.
const MAX_BODY: i64 = 200_000;
/// `sys.get_int_max_str_digits()` por omisión de Python 3.10.12.
const MAX_INT_DIGITS: usize = 4_300;
/// El Python no pone plazo; este solo evita que un cliente colgado retenga un hilo.
const READ_TIMEOUT: Duration = Duration::from_secs(60);

/// `DEFAULT_ERROR_MESSAGE` de `http.server`.
const ERROR_TEMPLATE: &str = "<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 4.01//EN\"\n        \"http://www.w3.org/TR/html4/strict.dtd\">\n<html>\n    <head>\n        <meta http-equiv=\"Content-Type\" content=\"text/html;charset=utf-8\">\n        <title>Error response</title>\n    </head>\n    <body>\n        <h1>Error response</h1>\n        <p>Error code: {code}</p>\n        <p>Message: {message}.</p>\n        <p>Error code explanation: {status} - {explain}.</p>\n    </body>\n</html>\n";

/// Atiende conexiones hasta que falle el `accept` del listener. Cada aviso
/// aceptado se envía por `sink` (el hilo de GTK, o la salida de `--headless`).
pub fn serve(listener: TcpListener, sink: Sender<Notice>, hooks: PathBuf) -> io::Result<()> {
    let conf = hooks.join("cc-notify.conf");
    loop {
        let (stream, peer) = listener.accept()?;
        let sink = sink.clone();
        let conf = conf.clone();
        let spawned = std::thread::Builder::new()
            .name("notifyd-http".into())
            .spawn(move || handle(stream, peer.ip(), &sink, &conf));
        if let Err(err) = spawned {
            eprintln!("comandos-notifyd: no se pudo atender una conexión: {err}");
        }
    }
}

fn handle(stream: TcpStream, peer: IpAddr, sink: &Sender<Notice>, conf: &Path) {
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    let mut reader = BufReader::new(&stream);
    let reply = respond(&mut reader, peer, sink, conf);
    let mut writer = &stream;
    if let Some(bytes) = reply {
        let _ = writer.write_all(&bytes);
        let _ = writer.flush();
    }
    // Cierre ordenado: FIN y se descarta lo que el cliente aún envíe, para que el
    // cierre no se convierta en un RST que se coma la respuesta.
    let _ = stream.shutdown(Shutdown::Write);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
    let mut sink_bytes = [0u8; 8192];
    let mut drained = 0usize;
    while drained < 1 << 20 {
        match reader.read(&mut sink_bytes) {
            Ok(0) | Err(_) => break,
            Ok(n) => drained += n,
        }
    }
}

/// Estado de la petición que condiciona las respuestas de error, como los
/// atributos `request_version` y `command` del `BaseHTTPRequestHandler`.
struct Request {
    version: String,
    command: Option<String>,
}

impl Request {
    /// `send_response` + cabeceras + `end_headers` + cuerpo; en HTTP/0.9 solo el cuerpo.
    fn reply(&self, code: u16, reason: &str, headers: &[(&str, String)], body: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        if self.version != "HTTP/0.9" {
            out.extend(latin1_bytes(&format!("HTTP/1.0 {code} {reason}\r\n")));
            for (name, value) in headers {
                out.extend(latin1_bytes(&format!("{name}: {value}\r\n")));
            }
            out.extend_from_slice(b"\r\n");
        }
        out.extend_from_slice(body);
        out
    }

    /// `send_response(code); end_headers()` sin cuerpo.
    fn status(&self, code: u16) -> Vec<u8> {
        self.reply(code, phrases(code).0, &[], b"")
    }

    /// `send_error(code, message, explain)`.
    fn error(&self, code: u16, message: Option<&str>, explain: Option<&str>) -> Vec<u8> {
        let (short, long) = phrases(code);
        let message = message.unwrap_or(short);
        let explain = explain.unwrap_or(long);
        let body = ERROR_TEMPLATE
            .replace("{code}", &code.to_string())
            .replace("{status}", status_name(code))
            .replace("{message}", &html_escape(message))
            .replace("{explain}", &html_escape(explain));
        let headers = [
            ("Connection", "close".to_string()),
            ("Content-Type", "text/html;charset=utf-8".to_string()),
            ("Content-Length", body.len().to_string()),
        ];
        let body = if self.command.as_deref() == Some("HEAD") {
            &b""[..]
        } else {
            body.as_bytes()
        };
        self.reply(code, message, &headers, body)
    }

    /// Respuesta JSON del `Handler` (`Content-Type` y `Content-Length`).
    fn json(&self, body: &[u8]) -> Vec<u8> {
        let headers = [
            ("Content-Type", "application/json".to_string()),
            ("Content-Length", body.len().to_string()),
        ];
        self.reply(200, "OK", &headers, body)
    }
}

/// `handle_one_request` + `parse_request` + `do_POST`. `None`: la conexión se
/// cierra sin responder (petición vacía o excepción no capturada en el Python).
fn respond<R: BufRead>(
    reader: &mut R,
    peer: IpAddr,
    sink: &Sender<Notice>,
    conf: &Path,
) -> Option<Vec<u8>> {
    let mut request = Request {
        version: "HTTP/0.9".into(),
        command: None,
    };
    let raw = read_line(reader, MAX_LINE + 1).ok()?;
    if raw.len() > MAX_LINE {
        request.version = String::new();
        request.command = Some(String::new());
        return Some(request.error(414, None, None));
    }
    if raw.is_empty() {
        return None;
    }
    let line = latin1(&raw);
    let line = line.trim_end_matches(['\r', '\n']);
    let words: Vec<&str> = line
        .split(is_split_space)
        .filter(|w| !w.is_empty())
        .collect();
    if words.is_empty() {
        return None;
    }
    if words.len() >= 3 {
        let version = words.last().copied().unwrap_or_default();
        let Some((base, numbers)) = parse_version(version) else {
            let message = format!("Bad request version ({})", py_repr(version));
            return Some(request.error(400, Some(&message), None));
        };
        if numbers >= (2, 0) {
            let message = format!("Invalid HTTP version ({base})");
            return Some(request.error(505, Some(&message), None));
        }
        request.version = version.to_string();
    }
    let (command, path) = match words.as_slice() {
        [command, path] | [command, path, _] => (*command, *path),
        _ => {
            let message = format!("Bad request syntax ({})", py_repr(line));
            return Some(request.error(400, Some(&message), None));
        }
    };
    if words.len() == 2 && command != "GET" {
        let message = format!("Bad HTTP/0.9 request type ({})", py_repr(command));
        return Some(request.error(400, Some(&message), None));
    }
    request.command = Some(command.to_string());
    // gh-87389: `//ruta` se reduce a una sola barra.
    let path = if path.starts_with("//") {
        format!("/{}", path.trim_start_matches('/'))
    } else {
        path.to_string()
    };
    let headers = match read_headers(reader) {
        Ok(headers) => headers,
        Err(HeaderError::LineTooLong) => {
            return Some(request.error(
                431,
                Some("Line too long"),
                Some("got more than 65536 bytes when reading header line"),
            ));
        }
        Err(HeaderError::TooMany) => {
            return Some(request.error(
                431,
                Some("Too many headers"),
                Some("got more than 100 headers"),
            ));
        }
        Err(HeaderError::Io) => return None,
    };
    if command != "POST" {
        let message = format!("Unsupported method ({})", py_repr(command));
        return Some(request.error(501, Some(&message), None));
    }
    do_post(&request, reader, peer, &path, &headers, sink, conf)
}

fn do_post<R: BufRead>(
    request: &Request,
    reader: &mut R,
    peer: IpAddr,
    path: &str,
    headers: &Headers,
    sink: &Sender<Notice>,
    conf: &Path,
) -> Option<Vec<u8>> {
    // Solo loopback y sin Origin cross-site: cc-notify.sh (curl, sin Origin)
    // pasa; un sitio web con fetch cross-origin (CSRF de popups) se rechaza.
    if !is_loopback_peer(peer) {
        return Some(request.status(403));
    }
    let origin = headers.get("Origin").unwrap_or_default();
    if !origin.is_empty() && !origin_allowed(origin) {
        return Some(request.status(403));
    }
    if path != "/notify" {
        return Some(request.status(404));
    }
    let Some(length) = py_int(headers.get("Content-Length").unwrap_or("0")) else {
        return Some(request.status(400));
    };
    if length > MAX_BODY {
        return Some(request.status(413));
    }
    let Ok(body) = read_body(reader, length) else {
        return Some(request.status(400));
    };
    let payload = if body.is_empty() {
        Some(Value::Object(serde_json::Map::new()))
    } else {
        python_json_loads(&body)
    };
    let Some(payload) = payload else {
        return Some(request.status(400));
    };
    // Popups flotantes: SOLO si se piden (POPUPS=1). Por defecto los avisos
    // viven dentro del tablero (pila superior derecha + campana).
    if conf_value(conf, "POPUPS", "0") != "1" {
        return Some(request.json(br#"{"ok": true, "popup": false}"#));
    }
    let Some(notice) = Notice::from_payload(&payload) else {
        // El Python lanza `AttributeError` (`d.get` sobre algo que no es un
        // objeto) y `socketserver` cierra la conexión sin respuesta.
        eprintln!("comandos-notifyd: cuerpo JSON que no es un objeto; conexión cerrada");
        return None;
    };
    let _ = sink.send(notice);
    Some(request.json(br#"{"ok": true}"#))
}

/// `client_address[0] in ("127.0.0.1", "::1", "::ffff:127.0.0.1")`.
fn is_loopback_peer(peer: IpAddr) -> bool {
    match peer {
        IpAddr::V4(ip) => ip == Ipv4Addr::LOCALHOST,
        IpAddr::V6(ip) => ip.is_loopback() || ip.to_ipv4_mapped() == Some(Ipv4Addr::LOCALHOST),
    }
}

/// `re.match(r"^https?://(localhost|127\.0\.0\.1)(:\d+)?$", origin)`.
fn origin_allowed(origin: &str) -> bool {
    let Some(rest) = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
    else {
        return false;
    };
    let Some(rest) = rest
        .strip_prefix("localhost")
        .or_else(|| rest.strip_prefix("127.0.0.1"))
    else {
        return false;
    };
    // `$` también casa antes de un `\n` final.
    let rest = rest.strip_suffix('\n').unwrap_or(rest);
    rest.is_empty()
        || rest
            .strip_prefix(':')
            .is_some_and(|port| !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()))
}

/// `json.loads(bytes)` de Python: detección de codificación, `NaN`/`Infinity`,
/// claves repetidas (gana la última) y el límite de 4300 dígitos de los enteros.
fn python_json_loads(body: &[u8]) -> Option<Value> {
    let value = comandos_core::json::workspace_loads_bytes(body)?;
    let mut stack = vec![&value];
    while let Some(current) = stack.pop() {
        match current {
            Value::Number(number) => {
                let raw = number.as_str();
                if !raw.contains(['.', 'e', 'E', 'N', 'I'])
                    && raw.bytes().filter(u8::is_ascii_digit).count() > MAX_INT_DIGITS
                {
                    return None;
                }
            }
            Value::Array(values) => stack.extend(values.iter()),
            Value::Object(values) => stack.extend(values.values()),
            _ => {}
        }
    }
    Some(value)
}

/// `int(texto)` de Python para un valor de cabecera (latin-1): espacios a los
/// lados, signo, guiones bajos entre dígitos y el límite de 4300 dígitos.
/// El valor se satura a `i64`, que basta para comparar con los topes.
fn py_int(text: &str) -> Option<i64> {
    let text = text.trim_matches(|c| {
        matches!(
            c,
            '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{85}' | '\u{a0}'
        )
    });
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, text.get(1..)?),
        Some(b'+') => (false, text.get(1..)?),
        _ => (false, text),
    };
    let mut value: i64 = 0;
    let mut count = 0usize;
    let mut previous_digit = false;
    for byte in digits.bytes() {
        match byte {
            b'0'..=b'9' => {
                value = value
                    .saturating_mul(10)
                    .saturating_add(i64::from(byte - b'0'));
                count += 1;
                previous_digit = true;
            }
            b'_' if previous_digit => previous_digit = false,
            _ => return None,
        }
    }
    if count == 0 || !previous_digit || count > MAX_INT_DIGITS {
        return None;
    }
    Some(if negative { -value } else { value })
}

/// `HTTP/<mayor>.<menor>` con `int()` de cada parte; devuelve el texto tras `/`.
fn parse_version(version: &str) -> Option<(&str, (i64, i64))> {
    let base = version.strip_prefix("HTTP/")?;
    let mut parts = base.split('.');
    let (Some(major), Some(minor), None) = (parts.next(), parts.next(), parts.next()) else {
        return None;
    };
    Some((base, (py_int(major)?, py_int(minor)?)))
}

/// Separadores de `str.split()` sobre texto latin-1 (`str.isspace`).
fn is_split_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | '\u{1c}'..='\u{1f}' | ' ' | '\u{85}' | '\u{a0}'
    )
}

fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
}

/// `str.encode("latin-1")`; los textos vienen de latin-1 o de `repr` (ASCII).
fn latin1_bytes(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?'))
        .collect()
}

fn py_repr(text: &str) -> String {
    comandos_core::pomodoro::python_repr(&Value::String(text.to_string()))
}

/// `html.escape(texto, quote=False)`.
fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// `%(code)s` de la plantilla: en Python 3.10 el `str()` de `HTTPStatus` es el
/// nombre del miembro (`HTTPStatus.NOT_IMPLEMENTED`), no el número.
fn status_name(code: u16) -> &'static str {
    match code {
        400 => "HTTPStatus.BAD_REQUEST",
        414 => "HTTPStatus.REQUEST_URI_TOO_LONG",
        431 => "HTTPStatus.REQUEST_HEADER_FIELDS_TOO_LARGE",
        501 => "HTTPStatus.NOT_IMPLEMENTED",
        505 => "HTTPStatus.HTTP_VERSION_NOT_SUPPORTED",
        _ => "???",
    }
}

/// `BaseHTTPRequestHandler.responses` de los códigos que se usan.
fn phrases(code: u16) -> (&'static str, &'static str) {
    match code {
        200 => ("OK", "Request fulfilled, document follows"),
        400 => ("Bad Request", "Bad request syntax or unsupported method"),
        403 => (
            "Forbidden",
            "Request forbidden -- authorization will not help",
        ),
        404 => ("Not Found", "Nothing matches the given URI"),
        413 => ("Request Entity Too Large", "Entity is too large"),
        414 => ("Request-URI Too Long", "URI is too long"),
        431 => (
            "Request Header Fields Too Large",
            "The server is unwilling to process the request because its header fields are too large",
        ),
        501 => ("Not Implemented", "Server does not support this operation"),
        505 => ("HTTP Version Not Supported", "Cannot fulfill request"),
        _ => ("???", "???"),
    }
}

/// `readline(limit)`: hasta `\n` incluido o `limit` bytes.
fn read_line<R: BufRead>(reader: &mut R, limit: usize) -> io::Result<Vec<u8>> {
    let mut line = Vec::new();
    while line.len() < limit {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            break;
        }
        let room = limit - line.len();
        let window = available.get(..room).unwrap_or(available);
        if let Some(end) = window.iter().position(|&b| b == b'\n') {
            line.extend_from_slice(window.get(..=end).unwrap_or(window));
            reader.consume(end + 1);
            return Ok(line);
        }
        let taken = window.len();
        line.extend_from_slice(window);
        reader.consume(taken);
    }
    Ok(line)
}

/// `rfile.read(n)`: exactamente `n` bytes o hasta EOF; con `n < 0`, hasta EOF.
fn read_body<R: Read>(reader: &mut R, length: i64) -> io::Result<Vec<u8>> {
    let mut body = Vec::new();
    match u64::try_from(length) {
        Ok(length) => reader.take(length).read_to_end(&mut body)?,
        Err(_) => reader.read_to_end(&mut body)?,
    };
    Ok(body)
}

enum HeaderError {
    LineTooLong,
    TooMany,
    Io,
}

/// Cabeceras en orden, como las guarda `email.message.Message`.
struct Headers(Vec<(String, String)>);

impl Headers {
    /// `Message.get(nombre)`: la primera, sin distinguir mayúsculas.
    fn get(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

/// `http.client.parse_headers`: lee líneas hasta la vacía y las interpreta con
/// el `FeedParser` de `email` (política `compat32`).
fn read_headers<R: BufRead>(reader: &mut R) -> Result<Headers, HeaderError> {
    let mut raw = Vec::new();
    let mut count = 0usize;
    loop {
        let line = read_line(reader, MAX_LINE + 1).map_err(|_| HeaderError::Io)?;
        if line.len() > MAX_LINE {
            return Err(HeaderError::LineTooLong);
        }
        count += 1;
        let end = matches!(line.as_slice(), b"\r\n" | b"\n" | b"");
        raw.extend_from_slice(&line);
        if count > MAX_HEADERS {
            return Err(HeaderError::TooMany);
        }
        if end {
            break;
        }
    }
    Ok(parse_header_block(&latin1(&raw)))
}

/// Líneas con su terminador (`\r\n`, `\r` o `\n`), como `NLCRE_crack`.
fn split_lines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let bytes = text.as_bytes();
    let mut i = 0;
    while let Some(&byte) = bytes.get(i) {
        let end = match byte {
            b'\r' if bytes.get(i + 1) == Some(&b'\n') => Some(i + 2),
            b'\r' | b'\n' => Some(i + 1),
            _ => None,
        };
        match end {
            Some(end) => {
                lines.push(text.get(start..end).unwrap_or_default());
                start = end;
                i = end;
            }
            None => i += 1,
        }
    }
    if start < text.len() {
        lines.push(text.get(start..).unwrap_or_default());
    }
    lines
}

/// `headerRE = ^(From |[\041-\071\073-\176]*:|[\t ])`.
fn is_header_line(line: &str) -> bool {
    if line.starts_with("From ") || line.starts_with([' ', '\t']) {
        return true;
    }
    let name_len = line
        .bytes()
        .take_while(|b| matches!(b, 0x21..=0x39 | 0x3b..=0x7e))
        .count();
    line.as_bytes().get(name_len) == Some(&b':')
}

/// `FeedParser._parsegen` (solo cabeceras) + `_parse_headers` + `header_source_parse`.
fn parse_header_block(text: &str) -> Headers {
    let lines: Vec<&str> = split_lines(text)
        .into_iter()
        .take_while(|line| is_header_line(line))
        .collect();
    let mut headers = Vec::new();
    let mut last: Vec<&str> = Vec::new();
    let flush = |last: &mut Vec<&str>, headers: &mut Vec<(String, String)>| {
        if let Some((first, rest)) = last.split_first()
            && let Some((name, value)) = first.split_once(':')
        {
            let mut value = value.trim_start_matches([' ', '\t']).to_string();
            for continuation in rest {
                value.push_str(continuation);
            }
            let value = value.trim_end_matches(['\r', '\n']).to_string();
            headers.push((name.to_string(), value));
        }
        last.clear();
    };
    let total = lines.len();
    for (number, line) in lines.iter().enumerate() {
        if line.starts_with([' ', '\t']) {
            // Continuación; sin cabecera previa es un defecto y se ignora.
            if !last.is_empty() {
                last.push(line);
            }
            continue;
        }
        flush(&mut last, &mut headers);
        if line.starts_with("From ") {
            if number != 0 && number + 1 == total {
                // Parece el comienzo del cuerpo: se deja de leer cabeceras.
                return Headers(headers);
            }
            continue;
        }
        if line.find(':').is_some_and(|i| i > 0) {
            last.push(line);
        }
    }
    flush(&mut last, &mut headers);
    Headers(headers)
}

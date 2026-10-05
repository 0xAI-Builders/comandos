//! El tablero (`cc-dash`/`comandos dash`) visto desde los popups: un cliente
//! HTTP/1.0 mínimo (`dash_get`, `dash` del Python) y la caché de `/prefs`
//! (`_theme_cache` de 30 s y `_NPOS_CACHE` de 5 s). Toda la red va fuera del
//! hilo de GTK (ruling 3); el hilo de GTK solo lee la caché.
use crate::theme::{Tokens, theme_name, themes_from_file, tokens};
use serde_json::Value;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Plazo de `dash_get` (1,5 s) y del POST al tablero (5 s).
pub const GET_TIMEOUT: Duration = Duration::from_millis(1500);
pub const POST_TIMEOUT: Duration = Duration::from_secs(5);
/// Vida de la caché del tema y de `notif_pos`.
pub const THEME_TTL: Duration = Duration::from_secs(30);
pub const NOTIF_POS_TTL: Duration = Duration::from_secs(5);
/// Tras un arrastre, `notif_pos` se fija en «libre» 30 s más el TTL.
pub const FREE_PIN: Duration = Duration::from_secs(30);
/// Tope de lo que se lee de una respuesta del tablero.
const MAX_RESPONSE: u64 = 8 << 20;

/// Dirección del tablero (`--dash-url`, por omisión `http://127.0.0.1:4777`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DashClient {
    host: String,
    port: u16,
}

impl DashClient {
    /// Solo `http://<host>[:<puerto>]` sin ruta (o con `/` final).
    pub fn parse(url: &str) -> Option<DashClient> {
        let rest = url.strip_prefix("http://")?.trim_end_matches('/');
        if rest.is_empty() || rest.contains('/') {
            return None;
        }
        let (host, port) = match rest.rsplit_once(':') {
            Some((host, port)) if !host.is_empty() => (host, port.parse().ok()?),
            Some(_) => return None,
            None => (rest, 80),
        };
        Some(DashClient {
            host: host.to_string(),
            port,
        })
    }

    /// Conecta probando cada dirección resuelta en orden, como
    /// `socket.create_connection` de `urlopen` (con `localhost` puede salir
    /// antes `::1` aunque el tablero escuche solo en IPv4).
    fn connect(&self, timeout: Duration) -> Option<TcpStream> {
        let addresses: Vec<SocketAddr> = (self.host.as_str(), self.port)
            .to_socket_addrs()
            .ok()?
            .collect();
        connect_any(&addresses, timeout)
    }

    /// Una petición HTTP/1.0; devuelve estado y cuerpo. `timeout` se aplica a
    /// la conexión y a cada lectura/escritura, como el de `urlopen`. Como
    /// `http.client`, no espera al cierre: con `Body::Skip` vuelve tras la
    /// línea de estado y las cabeceras, y con `Body::Read` lee
    /// `Content-Length` bytes si la cabecera está (si no, hasta el cierre).
    fn exchange(&self, request: &[u8], timeout: Duration, body: Body) -> Option<(u16, Vec<u8>)> {
        let mut stream = self.connect(timeout)?;
        stream.set_read_timeout(Some(timeout)).ok()?;
        stream.set_write_timeout(Some(timeout)).ok()?;
        stream.write_all(request).ok()?;
        read_response(&mut stream.take(MAX_RESPONSE), body)
    }

    /// `dash_get(path)`: el JSON de un GET con estado 2xx; `None` ante cualquier fallo.
    pub fn get_json(&self, path: &str, timeout: Duration) -> Option<Value> {
        let request = format!(
            "GET {path} HTTP/1.0\r\nHost: {}:{}\r\nAccept-Encoding: identity\r\n\r\n",
            self.host, self.port
        );
        let (status, body) = self.exchange(request.as_bytes(), timeout, Body::Read)?;
        if !(200..300).contains(&status) {
            return None;
        }
        let text = String::from_utf8(body).ok()?;
        comandos_core::json::workspace_loads_bytes(text.as_bytes())
    }

    /// `dash(path, payload)`: POST JSON; `true` si respondió sin error (< 400).
    pub fn post_json(&self, path: &str, payload: &Value, timeout: Duration) -> bool {
        self.post_body(path, &payload.to_string(), timeout)
    }

    /// POST de un cuerpo JSON ya serializado; `true` si respondió sin error (< 400).
    pub fn post_body(&self, path: &str, body: &str, timeout: Duration) -> bool {
        let request = format!(
            "POST {path} HTTP/1.0\r\nHost: {}:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            self.host,
            self.port,
            body.len()
        );
        // Solo cuenta el estado: si el tablero ya aplicó la acción pero tarda
        // en cerrar, esperar al cierre daría `false` y la caída a tmux la
        // repetiría (`urlopen` vuelve en cuanto tiene las cabeceras).
        self.exchange(request.as_bytes(), timeout, Body::Skip)
            .is_some_and(|(status, _)| status < 400)
    }
}

/// Si la respuesta se lee entera o solo hasta las cabeceras.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Body {
    Read,
    Skip,
}

/// Primera dirección que acepta la conexión dentro del plazo.
fn connect_any(addresses: &[SocketAddr], timeout: Duration) -> Option<TcpStream> {
    addresses
        .iter()
        .find_map(|address| TcpStream::connect_timeout(address, timeout).ok())
}

/// Un `read` que reintenta las interrupciones; `None` ante cualquier otro error.
fn read_some(reader: &mut impl Read, chunk: &mut [u8]) -> Option<usize> {
    loop {
        match reader.read(chunk) {
            Ok(n) => return Some(n),
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return None,
        }
    }
}

/// `Content-Length` de las cabeceras (sin distinguir mayúsculas); `None` si
/// falta o no es un número.
fn content_length(head: &str) -> Option<u64> {
    head.split("\r\n").skip(1).find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim()
            .eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse().ok())?
    })
}

/// Lee una respuesta HTTP/1.x: estado y, según `body`, el cuerpo.
fn read_response(reader: &mut impl Read, body: Body) -> Option<(u16, Vec<u8>)> {
    let mut raw = Vec::new();
    let mut chunk = [0u8; 4096];
    let split = loop {
        if let Some(split) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            break split;
        }
        let n = read_some(reader, &mut chunk)?;
        if n == 0 {
            return None;
        }
        raw.extend_from_slice(chunk.get(..n)?);
    };
    let head = String::from_utf8_lossy(raw.get(..split)?).into_owned();
    let status = head.split(' ').nth(1)?.parse().ok()?;
    if body == Body::Skip {
        return Some((status, Vec::new()));
    }
    let mut rest = raw.get(split + 4..)?.to_vec();
    match content_length(&head) {
        Some(wanted) => {
            let wanted = usize::try_from(wanted).unwrap_or(usize::MAX);
            while rest.len() < wanted {
                let n = read_some(reader, &mut chunk)?;
                if n == 0 {
                    // Cuerpo más corto que lo anunciado: `IncompleteRead`.
                    return None;
                }
                rest.extend_from_slice(chunk.get(..n)?);
            }
            rest.truncate(wanted);
        }
        None => {
            reader.read_to_end(&mut rest).ok()?;
        }
    }
    Some((status, rest))
}

/// Estado de la caché de `/prefs`.
#[derive(Clone, Debug)]
pub struct PrefsCache {
    theme_at: Option<Instant>,
    tokens: Option<Tokens>,
    notif_pos_at: Option<Instant>,
    notif_pos: Value,
}

impl Default for PrefsCache {
    fn default() -> PrefsCache {
        PrefsCache {
            theme_at: None,
            tokens: None,
            notif_pos_at: None,
            notif_pos: Value::String("free".into()),
        }
    }
}

impl PrefsCache {
    /// Tokens vigentes (los base si aún no se calcularon o el tema falló).
    pub fn tokens(&self) -> Option<&Tokens> {
        self.tokens.as_ref()
    }

    /// `notif_pos` vigente: texto o `None` si no es texto.
    pub fn notif_pos(&self) -> Option<&str> {
        self.notif_pos.as_str()
    }

    /// `not (tokens and now - at < 30)`.
    pub fn theme_stale(&self, now: Instant) -> bool {
        match (&self.tokens, self.theme_at) {
            (Some(_), Some(at)) => now.saturating_duration_since(at) >= THEME_TTL,
            _ => true,
        }
    }

    /// Guarda el resultado de `theme_tokens()`; `None` (el Python lanzó) no toca la caché.
    pub fn store_theme(&mut self, now: Instant, computed: Option<Tokens>) {
        if let Some(t) = computed {
            self.tokens = Some(t);
            self.theme_at = Some(now);
        }
    }

    /// `now - at > 5` (con `at` en el futuro tras un arrastre).
    pub fn notif_pos_stale(&self, now: Instant) -> bool {
        self.notif_pos_at.is_none_or(|at| {
            now.checked_duration_since(at)
                .is_some_and(|d| d > NOTIF_POS_TTL)
        })
    }

    /// `_NPOS_CACHE["at"] = now` antes de pedir `/prefs` (evita peticiones repetidas).
    pub fn mark_notif_pos(&mut self, now: Instant) {
        self.notif_pos_at = Some(now);
    }

    /// `val = prefs.get("notif_pos") or "free"`; si `/prefs` falló o no es
    /// un objeto, el valor anterior se queda.
    pub fn store_notif_pos(&mut self, prefs: Option<&Value>) {
        if let Some(object) = prefs.and_then(Value::as_object) {
            self.notif_pos = match object.get("notif_pos") {
                Some(v) if crate::theme::py_truthy(v) => v.clone(),
                _ => Value::String("free".into()),
            };
        }
    }

    /// Tras soltar un arrastre: «libre» y sin consultar `/prefs` durante 35 s.
    pub fn pin_free(&mut self, now: Instant) {
        self.notif_pos = Value::String("free".into());
        self.notif_pos_at = Some(now + FREE_PIN);
    }
}

/// `theme_tokens()` sin caché: lee `config/themes.json` (si se sabe dónde
/// está) y el tema de `/prefs`.
pub fn compute_tokens(themes_file: Option<&Path>, prefs: Option<&Value>) -> Option<Tokens> {
    let raw = themes_file.and_then(|file| std::fs::read(file).ok());
    tokens(&themes_from_file(raw.as_deref()), &theme_name(prefs))
}

/// Lo que necesita la actualización de la caché (vive fuera del hilo de GTK).
#[derive(Clone, Debug)]
pub struct PrefsSource {
    pub dash: DashClient,
    /// `None` sin checkout: tema base.
    pub themes_file: Option<PathBuf>,
}

impl PrefsSource {
    /// Refresca lo caducado: una sola petición a `/prefs` sirve para el tema
    /// y para `notif_pos` (el Python hacía una por cada uno). Bloquea: se llama
    /// desde un hilo que no es el de GTK. Devuelve si `notif_pos` cambió.
    pub fn refresh(&self, cache: &std::sync::Mutex<PrefsCache>) -> bool {
        let now = Instant::now();
        let (want_theme, want_pos, before) = match cache.lock() {
            Ok(mut c) => {
                let want_pos = c.notif_pos_stale(now);
                if want_pos {
                    c.mark_notif_pos(now);
                }
                (c.theme_stale(now), want_pos, c.notif_pos.clone())
            }
            Err(_) => return false,
        };
        if !want_theme && !want_pos {
            return false;
        }
        let prefs = self.dash.get_json("/prefs", GET_TIMEOUT);
        let Ok(mut c) = cache.lock() else {
            return false;
        };
        if want_theme {
            c.store_theme(
                now,
                compute_tokens(self.themes_file.as_deref(), prefs.as_ref()),
            );
        }
        if want_pos {
            c.store_notif_pos(prefs.as_ref());
        }
        c.notif_pos != before
    }
}

/// Archivos del checkout que leen los popups. Sin checkout no hay ninguno
/// (tema base e iconos «•»): nunca rutas relativas al directorio de trabajo,
/// que en un servicio de usuario es `$HOME`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckoutFiles {
    pub themes_file: Option<PathBuf>,
    pub icons_dir: Option<PathBuf>,
}

impl CheckoutFiles {
    pub fn new(repo: Option<&Path>) -> CheckoutFiles {
        CheckoutFiles {
            themes_file: repo.map(|r| r.join("config/themes.json")),
            icons_dir: repo.map(|r| r.join("dash/icons")),
        }
    }
}

/// Checkout del que salen `config/themes.json` y `dash/icons`: `--repo-root`
/// si se pasó; si no, la regla de `comandos dash` (`COMANDOS_DASH_REPO`, o
/// `<hooks>/dash/index.html` resuelto y dos niveles arriba); el ejecutable
/// (`<exe>/../..`, el `REPO_ROOT` del Python) solo como último recurso, porque
/// instalado en `~/.local/share/comandos` o en `.build/target/release` no
/// apunta al checkout.
pub fn resolve_repo_root(
    flag: Option<&Path>,
    env_repo: Option<&str>,
    hooks: &Path,
    exe: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(flag) = flag {
        return Some(flag.to_path_buf());
    }
    let index = std::fs::canonicalize(hooks.join("dash/index.html")).ok();
    if let Some(root) = comandos_core::repo::repo_root_from(index.as_deref(), env_repo) {
        return Some(root);
    }
    let exe = std::fs::canonicalize(exe?).ok()?;
    Some(exe.parent()?.parent()?.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Una dirección muerta delante de la buena no impide conectar.
    #[test]
    fn connect_any_skips_dead_addresses() {
        let dead = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let dead_addr = dead.local_addr().unwrap();
        drop(dead);
        let live = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let live_addr = live.local_addr().unwrap();
        let stream = connect_any(&[dead_addr, live_addr], Duration::from_secs(2));
        assert_eq!(stream.unwrap().peer_addr().unwrap(), live_addr);
        assert!(connect_any(&[dead_addr], Duration::from_secs(2)).is_none());
        assert!(connect_any(&[], Duration::from_secs(2)).is_none());
    }

    #[test]
    fn content_length_is_case_insensitive() {
        assert_eq!(
            content_length("HTTP/1.0 200 OK\r\nContent-Length: 12"),
            Some(12)
        );
        assert_eq!(
            content_length("HTTP/1.0 200 OK\r\ncontent-length:3"),
            Some(3)
        );
        assert_eq!(
            content_length("HTTP/1.0 200 OK\r\nContent-Length: -1"),
            None
        );
        assert_eq!(content_length("HTTP/1.0 200 OK\r\nX: 1"), None);
    }

    /// Cuerpo más corto que `Content-Length`: fallo, como `IncompleteRead`.
    #[test]
    fn short_body_is_a_failure() {
        let mut raw: &[u8] = b"HTTP/1.0 200 OK\r\nContent-Length: 10\r\n\r\n{}";
        assert_eq!(read_response(&mut raw, Body::Read), None);
        let mut raw: &[u8] = b"HTTP/1.0 200 OK\r\n\r\n{}";
        assert_eq!(
            read_response(&mut raw, Body::Read),
            Some((200, b"{}".to_vec()))
        );
        let mut raw: &[u8] = b"HTTP/1.0 200 OK\r\n";
        assert_eq!(read_response(&mut raw, Body::Skip), None);
    }
}

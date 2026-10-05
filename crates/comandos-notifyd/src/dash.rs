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

    fn address(&self) -> Option<SocketAddr> {
        (self.host.as_str(), self.port)
            .to_socket_addrs()
            .ok()?
            .next()
    }

    /// Una petición HTTP/1.0; devuelve estado y cuerpo. `timeout` se aplica a
    /// la conexión y a cada lectura/escritura, como el de `urlopen`.
    fn exchange(&self, request: &[u8], timeout: Duration) -> Option<(u16, Vec<u8>)> {
        let mut stream = TcpStream::connect_timeout(&self.address()?, timeout).ok()?;
        stream.set_read_timeout(Some(timeout)).ok()?;
        stream.set_write_timeout(Some(timeout)).ok()?;
        stream.write_all(request).ok()?;
        let mut raw = Vec::new();
        stream.take(MAX_RESPONSE).read_to_end(&mut raw).ok()?;
        let split = raw.windows(4).position(|w| w == b"\r\n\r\n")?;
        let head = String::from_utf8_lossy(raw.get(..split)?).into_owned();
        let status = head.split(' ').nth(1)?.parse().ok()?;
        Some((status, raw.get(split + 4..)?.to_vec()))
    }

    /// `dash_get(path)`: el JSON de un GET con estado 2xx; `None` ante cualquier fallo.
    pub fn get_json(&self, path: &str, timeout: Duration) -> Option<Value> {
        let request = format!(
            "GET {path} HTTP/1.0\r\nHost: {}:{}\r\nAccept-Encoding: identity\r\n\r\n",
            self.host, self.port
        );
        let (status, body) = self.exchange(request.as_bytes(), timeout)?;
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
        self.exchange(request.as_bytes(), timeout)
            .is_some_and(|(status, _)| status < 400)
    }
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

/// `theme_tokens()` sin caché: lee `config/themes.json` y el tema de `/prefs`.
pub fn compute_tokens(themes_file: &Path, prefs: Option<&Value>) -> Option<Tokens> {
    let raw = std::fs::read(themes_file).ok();
    tokens(&themes_from_file(raw.as_deref()), &theme_name(prefs))
}

/// Lo que necesita la actualización de la caché (vive fuera del hilo de GTK).
#[derive(Clone, Debug)]
pub struct PrefsSource {
    pub dash: DashClient,
    pub themes_file: PathBuf,
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
            c.store_theme(now, compute_tokens(&self.themes_file, prefs.as_ref()));
        }
        if want_pos {
            c.store_notif_pos(prefs.as_ref());
        }
        c.notif_pos != before
    }
}

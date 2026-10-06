//! Clasificación de rutas del frente `comandos dash`.
//!
//! Rust solo responde lo que el Python respondería sirviendo un archivo:
//! GET/HEAD de una ruta limpia (decodificada, sin recorrido) que resuelve a un
//! archivo regular de `dash_dir` y que, en GET, no captura antes ninguna ruta
//! dinámica de `_do_GET`. Todo lo demás (directorios, ausentes, rutas raras,
//! `/operator`, API) se reenvía y el Python contesta lo suyo: paridad por
//! construcción. La puerta de seguridad ya la aplicó el transporte.
use crate::dash::{
    native::{self, NativeRoute},
    web::WebRoute,
};
use http::Method;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteClass {
    /// GET/HEAD de un archivo regular existente: lo sirve Rust desde `dash_dir`.
    Static,
    /// Ruta web de la Fase 3: composición, compuerta y artefactos versionados.
    Web(WebRoute),
    /// Ruta de un dominio nativo (Fase 2b): la responde Rust salvo `Decline`.
    Native(NativeRoute),
    /// Todo lo demás: se reenvía al Python heredado.
    Forward,
}

/// La clase nativa se comprueba antes que todo lo demás; con `native` en
/// falso (`--no-native`) el resultado es exactamente el de la Fase 2a.
pub fn classify_with(
    method: &Method,
    target: &str,
    asset_exists: &dyn Fn(&str) -> bool,
    native: bool,
) -> RouteClass {
    if native && let Some(route) = native::route(method, target) {
        return RouteClass::Native(route);
    }
    classify(method, target, asset_exists)
}

pub fn classify_with_web(
    method: &Method,
    target: &str,
    asset_exists: &dyn Fn(&str) -> bool,
    native: bool,
    web_exists: &dyn Fn(&str) -> bool,
) -> RouteClass {
    if let Some(route) = WebRoute::route(method, target, web_exists) {
        return RouteClass::Web(route);
    }
    if native && let Some(route) = native::route(method, target) {
        return RouteClass::Native(route);
    }
    classify(method, target, asset_exists)
}

/// `asset_exists` solo recibe rutas ya validadas por `static_path`: absolutas,
/// decodificadas, sin `..`, `.`, segmentos vacíos, NUL ni `\`.
pub fn classify(method: &Method, target: &str, asset_exists: &dyn Fn(&str) -> bool) -> RouteClass {
    let head = *method == Method::HEAD;
    if *method != Method::GET && !head {
        return RouteClass::Forward;
    }
    // `do_HEAD` del Python va directo a `send_head`: sin rutas dinámicas.
    if !head && is_dynamic_get(target) {
        return RouteClass::Forward;
    }
    match static_path(target) {
        Some(path) if asset_exists(&path) => RouteClass::Static,
        _ => RouteClass::Forward,
    }
}

/// Ruta sin consulta ni fragmento, como `Uri::path()` en el transporte.
pub fn path_of(target: &str) -> &str {
    target
        .split_once(['?', '#'])
        .map_or(target, |(path, _)| path)
}

/// Ruta de archivo que Rust puede servir: sin consulta, decodificada
/// (`%XX`, UTF-8 válido), `/` → `/index.html`. `None` si no es absoluta o si
/// tras decodificar tiene segmentos vacíos, `.`, `..`, NUL o `\`; esas rutas
/// las decide el Python (normaliza de otra forma y puede listar directorios).
pub fn static_path(target: &str) -> Option<String> {
    let raw = path_of(target);
    if !raw.starts_with('/') {
        return None;
    }
    let decoded = percent_decode(raw)?;
    if decoded == "/" {
        return Some("/index.html".to_string());
    }
    let clean = decoded[1..].split('/').all(|segment| {
        !segment.is_empty() && segment != "." && segment != ".." && !segment.contains(['\0', '\\'])
    });
    clean.then_some(decoded)
}

/// `%XX` → byte; un `%` sin dos hexadecimales o un resultado que no es UTF-8
/// invalidan la ruta (el Python usaría `surrogatepass`: se le deja a él).
fn percent_decode(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes.get(i + 1..i + 3)?;
            let hex = std::str::from_utf8(hex).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Cómo compara una rama de `_do_GET` (cc-dash) la ruta de la petición.
enum Match {
    /// `self.path.startswith(p)`: ruta cruda con consulta.
    RawPrefix(&'static str),
    /// `self.path == p`.
    RawExact(&'static str),
    /// `urlsplit(self.path).path == p`.
    PathExact(&'static str),
    /// `urlsplit(self.path).path.startswith(p)`.
    PathPrefix(&'static str),
}

use Match::{PathExact, PathPrefix, RawExact, RawPrefix};

/// Ramas de nivel superior de `Handler._do_GET` en `bin/cc-dash`, en orden.
/// Si una coincide, el Python responde dinámicamente aunque exista el archivo;
/// esta tabla debe seguir a `_do_GET` mientras ese código viva.
const DYNAMIC_GET: &[Match] = &[
    RawPrefix("/operator"),
    PathExact("/session-config-history"),
    RawPrefix("/pane-extensions"),
    RawPrefix("/webterm-token"),
    RawPrefix("/state"),
    RawPrefix("/accounts"),
    RawPrefix("/providers"),
    RawPrefix("/optimization/plans"),
    RawPrefix("/opencode/models"),
    RawPrefix("/model-tiers"),
    RawPrefix("/tab-models"),
    RawPrefix("/active-tab"),
    RawPrefix("/dedication"),
    RawPrefix("/analytics/week"),
    RawPrefix("/pomodoro/report"),
    RawExact("/pomodoro"),
    RawExact("/sovereignty"),
    RawPrefix("/model/status"),
    RawExact("/proxy"),
    RawPrefix("/ui-log/summary"),
    RawPrefix("/session-profiles"),
    RawPrefix("/extension-usage"),
    RawPrefix("/session-brain"),
    RawPrefix("/usage/guard"),
    RawPrefix("/usage/changes"),
    RawPrefix("/notifs/count"),
    RawPrefix("/news/latest"),
    PathExact("/push/key"),
    PathExact("/news/editions"),
    PathPrefix("/news/media/"),
    PathExact("/news/source"),
    PathExact("/news/chat"),
    PathExact("/news/notes"),
    PathExact("/news/saved"),
    PathExact("/news/edition"),
    RawPrefix("/models/latest"),
    RawPrefix("/fs/dirs"),
    RawPrefix("/usage/provider-compare"),
    RawPrefix("/usage/experiments"),
    RawPrefix("/usage/analytics"),
    RawPrefix("/usage/interactions"),
    RawPrefix("/usage/state"),
    RawPrefix("/project-profiles"),
    RawPrefix("/ssh"),
    RawPrefix("/conf"),
    RawPrefix("/prefs"),
    RawExact("/tmux-mouse"),
    RawPrefix("/tmux-mouse?"),
    PathExact("/workspace"),
    PathExact("/notices/watch"),
    PathExact("/notices"),
    PathExact("/notices/prefs"),
    PathExact("/workspace/close-group"),
    PathExact("/workspace/client"),
    RawPrefix("/tabs"),
    RawPrefix("/tab-history"),
    RawPrefix("/remote-state"),
    RawPrefix("/remote-qr.png"),
    PathExact("/work-marks"),
    PathExact("/events/v2"),
    RawPrefix("/events"),
    RawPrefix("/commands/catalog"),
    RawExact("/snippets"),
    RawExact("/chains"),
];

fn is_dynamic_get(target: &str) -> bool {
    // Con `//` al inicio `urlsplit` vería un host; esas rutas nunca son
    // estáticas (segmento vacío), así que basta con `path_of`.
    let path = path_of(target);
    DYNAMIC_GET.iter().any(|rule| match rule {
        RawPrefix(p) => target.starts_with(p),
        RawExact(p) => target == *p,
        PathExact(p) => path == *p,
        PathPrefix(p) => path.starts_with(p),
    })
}

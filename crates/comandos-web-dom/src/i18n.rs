//! Idioma del tablero (región «i18n» de `index.html`).
//!
//! En el JS, `let L = "es"` y el arranque hace `if(c._lang === "en") L = "en"`
//! tras `api("/conf")` (sin `localStorage`: `CC_LANG` vive en la configuración
//! y los botones de idioma recargan la página); `t(s)` traduce con `T_EN` y
//! `tf(es, en)` elige por `L`. `L`, `t` y `tf` son `let`/`const` del script en
//! línea: **no** son propiedades de `window` y el puente no los alcanza (ver
//! `bridge`). Por eso el lado Rust lee la misma fuente: `load()` (wasm) pide
//! `/conf` una vez por página con `api::get` y aplica [`lang_from_conf`];
//! hasta que responde, el idioma es español, como `L`. Un componente con texto
//! espera a `load()` antes de pintar (el JS también pinta los textos en inglés
//! solo tras esa respuesta, en `applyI18n`).
//!
//! [`T_EN`] copia la tabla de la región; `tests/i18n_host.rs` la compara con
//! `dash/index.html` para que no derive.

use serde_json::Value;
use std::cell::Cell;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Es,
    En,
}

impl Lang {
    /// El valor de `L` en el JS.
    pub fn code(self) -> &'static str {
        match self {
            Lang::Es => "es",
            Lang::En => "en",
        }
    }
}

thread_local! {
    static LANG: Cell<Lang> = const { Cell::new(Lang::Es) };
}

/// Idioma actual (por omisión español).
pub fn lang() -> Lang {
    LANG.with(Cell::get)
}

pub fn set_lang(l: Lang) {
    LANG.with(|c| c.set(l));
}

/// La regla del arranque: inglés solo si `c._lang === "en"`.
pub fn lang_from_conf(conf: &Value) -> Lang {
    match conf.get("_lang").and_then(Value::as_str) {
        Some("en") => Lang::En,
        _ => Lang::Es,
    }
}

/// `tf(es, en)`.
pub fn tf<'a>(es: &'a str, en: &'a str) -> &'a str {
    match lang() {
        Lang::Es => es,
        Lang::En => en,
    }
}

/// `T_EN` de la región «i18n» (en su orden; una clave repetida en el JS se
/// queda con el último valor, como en un objeto literal).
pub const T_EN: &[(&str, &str)] = &[
    ("esperan", "waiting"),
    ("listos", "done"),
    ("trabajando", "working"),
    ("esp.", "wait."),
    ("list.", "done"),
    ("trab.", "work."),
    ("Esperan tu respuesta", "Waiting for your answer"),
    ("Sesiones", "Sessions"),
    ("Servidores", "Servers"),
    ("gestionar", "manage"),
    ("Ajustes", "Settings"),
    ("Uso", "Usage"),
    ("Ver actividad reciente", "Show recent activity"),
    ("Ocultar actividad", "Hide activity"),
    ("Necesita respuesta", "Needs your answer"),
    ("Suelta", "Detached"),
    ("Listo para responder", "Ready for your reply"),
    (
        "Pregunta pendiente · claude apagado",
        "Pending question · claude is off",
    ),
    (
        "Esperando input o permiso",
        "Waiting for input or permission",
    ),
    ("Trabajando...", "Working..."),
    ("Revivir para retomar", "Revive to resume"),
    ("Conectado", "Connected"),
    (
        "SIN conexion: abre la pestana y teclea el password (o corre cc-keys)",
        "NO connection: open the tab and type the password (or run cc-keys)",
    ),
    (
        "Tunel vivo (sin password) — Abrir reconecta al instante",
        "Live tunnel (no password) — Open reconnects instantly",
    ),
    ("carpeta no encontrada", "folder not found"),
    ("Copiar", "Copy"),
    ("Abrir", "Open"),
    ("Revivir", "Revive"),
    ("Enviar", "Send"),
    ("Favorito", "Favorite"),
    ("suelta", "detached"),
    ("Responde aqui y Enter", "Reply here and press Enter"),
    (
        "Click: ver TODO el texto / colapsar",
        "Click: view ALL the text / collapse",
    ),
    (
        "Copiar la respuesta completa de Claude",
        "Copy Claude's full reply",
    ),
    (
        "Guardar la respuesta como .txt en Descargas",
        "Save the reply as .txt in Downloads",
    ),
    (
        "Guardar la respuesta como PDF (markdown renderizado) en Descargas",
        "Save the reply as PDF (rendered markdown) in Downloads",
    ),
    (
        "Matar la sesion tmux (y lo que corra dentro)",
        "Kill the tmux session (and whatever runs inside)",
    ),
    ("Remoto", "Remote"),
    ("Continuar remoto", "Continue remotely"),
    ("Prender remoto", "Remote on"),
    ("Apagar remoto", "Remote off"),
    ("Terminal ON", "Terminal ON"),
    ("Terminal OFF", "Terminal OFF"),
    ("Abrir terminal ahora", "Open terminal now"),
    ("Dashboard", "Dashboard"),
    ("Terminal web", "Web terminal"),
    ("Fallback terminal", "Fallback terminal"),
    ("Copiar", "Copy"),
    ("Consultando...", "Checking..."),
    (
        "Instala qrencode para generar QR.",
        "Install qrencode to generate QR.",
    ),
    (
        "Apagar remoto no apaga Tailscale; sólo deja de exponer ComandOS y detiene la terminal web.",
        "Remote off does not turn off Tailscale; it only stops exposing ComandOS and stops the web terminal.",
    ),
    (
        "Notificaciones (todo el sistema)",
        "Notifications (system-wide)",
    ),
    ("Volumen (voz y chime)", "Volume (voice and chime)"),
    ("Avisar al terminar", "Notify when done"),
    ("Avisar al pedir atencion", "Notify on attention"),
    ("Voz (anuncia el proyecto)", "Voice (announces the project)"),
    (
        "Chime (suena si la voz esta apagada)",
        "Chime (plays if voice is off)",
    ),
    ("Popups de escritorio", "Desktop popups"),
    ("Probar atencion", "Test attention"),
    ("Probar terminado", "Test done"),
    ("Probar voz", "Test voice"),
    ("Tablero", "Dashboard"),
    ("Notificaciones del navegador", "Browser notifications"),
    (
        "UN solo volumen para todo ComandOS. Estos botones suenan EXACTAMENTE igual que las notificaciones reales (mismo reproductor y volumen) — si algo suena distinto a esto, NO viene de ComandOS.",
        "ONE volume for all of ComandOS. These buttons sound EXACTLY like real notifications (same player, same volume) — if something sounds different, it is NOT ComandOS.",
    ),
    ("Servidores SSH", "SSH servers"),
    ("Agregar servidor", "Add server"),
    ("Alias", "Alias"),
    ("Usuario", "User"),
    ("Host / IP", "Host / IP"),
    ("Puerto", "Port"),
    ("Llave (opcional)", "Key (optional)"),
    ("Guardar", "Save"),
    ("Cancelar edicion", "Cancel edit"),
    ("Conectar", "Connect"),
    ("Editar", "Edit"),
    ("Borrar", "Delete"),
    ("Seguro?", "Sure?"),
    (
        "Viven en ~/.ssh/config: estandar y tuyo. Conectar abre una\n    sesion tmux reconectable. Para dejar de teclear passwords: corre cc-keys una vez.",
        "They live in ~/.ssh/config: standard and yours. Connect opens a reconnectable tmux session. To stop typing passwords: run cc-keys once.",
    ),
    ("espera TU respuesta", "waiting for YOU"),
    ("working", "working"),
    ("idle", "idle"),
    ("done", "done"),
    ("Terminal", "Terminal"),
    (
        "Abrir la terminal REAL de esta sesion",
        "Open this session's REAL terminal",
    ),
    ("recuperar", "recover"),
    ("cerrada", "closed"),
];

/// `t(s)`: `L === "en" ? (T_EN[s] ?? s) : s`.
pub fn t(s: &str) -> &str {
    match lang() {
        Lang::Es => s,
        Lang::En => T_EN
            .iter()
            .rev()
            .find(|(k, _)| *k == s)
            .map_or(s, |(_, v)| *v),
    }
}

#[cfg(target_arch = "wasm32")]
pub use web::load;

#[cfg(target_arch = "wasm32")]
mod web {
    use super::{Lang, lang_from_conf, set_lang};
    use js_sys::Promise;
    use std::cell::OnceCell;
    use wasm_bindgen::JsValue;
    use wasm_bindgen_futures::{JsFuture, future_to_promise};

    thread_local! {
        /// La petición a `/conf`, compartida por todos los que esperan.
        static CONF: OnceCell<Promise> = const { OnceCell::new() };
    }

    /// El idioma de `/conf`, pedido una vez por página (como el arranque del
    /// JS: un fallo deja español).
    pub async fn load() -> Lang {
        let p = CONF.with(|c| {
            c.get_or_init(|| {
                future_to_promise(async {
                    if let Ok(conf) = crate::api::get("/conf").await {
                        set_lang(lang_from_conf(&conf));
                    }
                    Ok(JsValue::UNDEFINED)
                })
            })
            .clone()
        });
        let _ = JsFuture::from(p).await;
        super::lang()
    }
}

//! Contenido visible de un popup (`make_popup` de `bin/cc-notifyd`) calculado
//! sin GTK: textos (`TR` es/en), vista previa, «Ver TODO», icono de estado y
//! texto que se copia. `popup.rs` construye las ventanas a partir de esto.
use crate::markup::{Block, blocks, py_prefix, py_splitlines, py_strip};
use crate::notice::{Lang, Notice};
use crate::stack::{popup_key, valid_pane};
use crate::theme::{Tokens, kind_icon};

/// Claves de `TR` y los textos sueltos de `make_popup`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Text {
    WaitingTag,
    DoneTag,
    SeeAll,
    SeeLess,
    Copy,
    Copied,
    Open,
    CloseAll,
    CloseAllTip,
    ArrowTip,
    ProjectTip,
    CloseTip,
}

/// `tr(clave)` (y los literales `"…" if UI_LANG == "es" else "…"`).
pub fn tr(text: Text, lang: Lang) -> &'static str {
    let (es, en) = match text {
        Text::WaitingTag => ("espera tu respuesta", "waiting for your answer"),
        Text::DoneTag => ("listo", "done"),
        Text::SeeAll => ("… Ver TODO", "… View ALL"),
        Text::SeeLess => ("… Ver menos", "… View less"),
        Text::Copy => ("Copiar", "Copy"),
        Text::Copied => ("Copiado ✓", "Copied ✓"),
        Text::Open => ("Abrir", "Open"),
        Text::CloseAll => ("Cerrar todas · {n}", "Close all · {n}"),
        Text::CloseAllTip => (
            "Cierra todos los avisos de la pila (igual que la ✕ de cada uno)",
            "Closes every notice in the stack (same as each one's ✕)",
        ),
        Text::ArrowTip => ("Expandir / colapsar", "Expand / collapse"),
        Text::ProjectTip => ("Abrir la sesion", "Open the session"),
        Text::CloseTip => ("Cerrar (Esc)", "Close (Esc)"),
    };
    match lang {
        Lang::Es => es,
        Lang::En => en,
    }
}

/// `tr("close_all").format(n=n)`.
pub fn close_all_label(n: usize, lang: Lang) -> String {
    tr(Text::CloseAll, lang).replace("{n}", &n.to_string())
}

/// Flecha de plegado (cerrado / abierto), cruz de cierre y viñeta de reserva del icono.
pub const ARROW_CLOSED: &str = "›";
pub const ARROW_OPEN: &str = "⌄";
pub const CLOSE_GLYPH: &str = "×";
pub const ICON_FALLBACK: &str = "•";
/// Lado del icono de estado, en píxeles.
pub const ICON_SIZE: i32 = 15;

/// El SVG del icono de estado ya tintado (`_icon_image`), o `None` si no hay
/// color utilizable o el archivo no se lee (el Python pinta entonces `•`).
pub fn icon_svg(icons_dir: &std::path::Path, model: &PopupModel) -> Option<Vec<u8>> {
    let color = model.icon_color.as_deref()?;
    let raw = std::fs::read(icons_dir.join(format!("{}.svg", model.icon))).ok()?;
    Some(crate::theme::recolor_svg(&raw, color))
}

/// Topes de la vista previa colapsada: 8 líneas con texto o más de 520 caracteres.
const PREVIEW_LINES: usize = 8;
const PREVIEW_CHARS: usize = 520;
/// Recorte del texto completo que se pinta en «Ver TODO» (`full[:16000]`).
pub const FULL_CHARS: usize = 16_000;

/// Lo que muestra un popup.
#[derive(Clone, Debug, PartialEq)]
pub struct PopupModel {
    /// Clave en `by_session`: un popup por agente (`sesión|pane`).
    pub key: String,
    pub session: String,
    /// El pane solo si cumple `PANE_RE`; si no, vacío.
    pub pane: String,
    pub kind: String,
    pub waiting: bool,
    /// Icono de Lucide y su color; color `None` = viñeta `•` (como el Python).
    pub icon: &'static str,
    pub icon_color: Option<String>,
    pub kind_tip: &'static str,
    pub arrow_tip: &'static str,
    /// `project or session`.
    pub project: String,
    pub project_tip: &'static str,
    /// Primera línea con texto (la línea gris bajo el nombre).
    pub first_line: String,
    /// Vista previa renderizada (`build_full_widget(head_txt)`), si hay texto.
    pub preview: Option<Vec<Block>>,
    /// Texto que va al portapapeles con «Copiar» (`full or src`), si hay texto.
    pub copy: Option<String>,
    /// El texto completo de «Ver TODO», si difiere de la vista previa.
    pub see_all: Option<Vec<Block>>,
    pub copy_label: &'static str,
    pub copied_label: &'static str,
    pub see_all_label: &'static str,
    pub see_less_label: &'static str,
    pub open_label: &'static str,
    pub close_tip: &'static str,
    /// Los «listo» se cierran solos a los 10 s; los «te espera», no.
    pub auto_close: bool,
}

/// `(texto or "")` y la `or` de Python sobre cadenas.
fn or<'a>(first: &'a str, second: &'a str) -> &'a str {
    if first.is_empty() { second } else { first }
}

/// Vista previa: líneas desde el principio hasta 8 con texto o más de 520 caracteres.
fn preview_head(src: &str) -> String {
    let mut head = Vec::new();
    let mut chars = 0;
    let mut shown = 0;
    for line in py_splitlines(src) {
        head.push(line);
        chars += line.chars().count();
        if !py_strip(line).is_empty() {
            shown += 1;
        }
        if shown >= PREVIEW_LINES || chars > PREVIEW_CHARS {
            break;
        }
    }
    head.join("\n")
}

/// `make_popup(title, body, session, kind, project, options, full, expanded, pane)`
/// sin GTK. `tokens` es el tema vigente (solo da el color del icono).
pub fn popup_model(notice: &Notice, lang: Lang, tokens: &Tokens) -> PopupModel {
    let full = py_strip(&notice.full).to_string();
    let pane = if valid_pane(&notice.pane) {
        notice.pane.clone()
    } else {
        String::new()
    };
    let waiting = notice.kind == "waiting";
    let (icon, icon_color) = kind_icon(&notice.kind, tokens);
    let text = or(&full, &notice.body);
    let first_line = py_splitlines(text)
        .into_iter()
        .map(py_strip)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .to_string();
    let src = py_strip(text).to_string();
    let head = (!src.is_empty()).then(|| preview_head(&src));
    let head_txt = head.clone().unwrap_or_default();
    let see_all = (!full.is_empty() && full != py_strip(&head_txt))
        .then(|| blocks(py_prefix(&full, FULL_CHARS)));
    PopupModel {
        key: popup_key(&notice.session, &pane),
        session: notice.session.clone(),
        pane,
        kind: notice.kind.clone(),
        waiting,
        icon,
        icon_color,
        kind_tip: tr(
            if waiting {
                Text::WaitingTag
            } else {
                Text::DoneTag
            },
            lang,
        ),
        arrow_tip: tr(Text::ArrowTip, lang),
        project: or(&notice.project, &notice.session).to_string(),
        project_tip: tr(Text::ProjectTip, lang),
        first_line,
        preview: head.as_deref().map(blocks),
        copy: (!src.is_empty()).then(|| or(&full, &src).to_string()),
        see_all,
        copy_label: tr(Text::Copy, lang),
        copied_label: tr(Text::Copied, lang),
        see_all_label: tr(Text::SeeAll, lang),
        see_less_label: tr(Text::SeeLess, lang),
        open_label: tr(Text::Open, lang),
        close_tip: tr(Text::CloseTip, lang),
        auto_close: !waiting,
    }
}

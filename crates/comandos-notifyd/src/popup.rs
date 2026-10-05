//! Capa fina de GTK3: las ventanas de `make_popup`, `close_popup`,
//! `close_all_popups`, `reposition` y «Cerrar todas» de `bin/cc-notifyd`.
//! Todo lo que se puede calcular sin pantalla (textos, markup, tema, pila,
//! posiciones) viene de los módulos puros; aquí solo se crean widgets y se
//! conectan señales. Corre únicamente en el hilo de GTK: la red y los archivos
//! van a otros hilos y vuelven con `glib::idle_add_once`.
//!
//! Sin `unsafe`: `gtk_widget_destroy` es `unsafe` en gtk3-rs, así que un popup
//! se cierra con `hide()` + `close()` (el `delete-event` por omisión lo
//! destruye en la siguiente vuelta del bucle; se ve igual: desaparece al acto).
use crate::dash::{POST_TIMEOUT, PrefsCache, PrefsSource};
use crate::markup::{Block, LabelText};
use crate::model::{
    ARROW_CLOSED, ARROW_OPEN, CLOSE_GLYPH, ICON_FALLBACK, ICON_SIZE, PopupModel, Text,
    close_all_label, icon_svg, popup_model, tr,
};
use crate::notice::{Lang, Notice};
use crate::position::{
    AnchorWriter, Configure, Geometry, anchor_from, clear_all_position, layout, on_configure,
};
use crate::stack::{
    AUTO_CLOSE_SECS, CLEAR_ALL_MIN, PopupMeta, STACK_MAX, WIDTH, evict_candidate, popup_key,
    valid_pane,
};
use crate::sweep::stale_waiting;
use crate::theme::{build_css, open_button_css};
use gtk::prelude::*;
use gtk::{gdk, gdk_pixbuf, glib, pango};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Acción «Abrir» (`open_session(session, pane)`); la pone `main.rs`.
pub type OpenAction = Box<dyn Fn(&str, &str)>;

/// Lo que los popups necesitan del resto del proceso.
pub struct Context {
    pub lang: Lang,
    /// `dash/icons` del checkout (`_NOTIF_ICONS`).
    /// `None` sin checkout: viñeta «•» en vez de icono.
    pub icons_dir: Option<PathBuf>,
    /// `<hooks>/notifyd-pos.json` (`POS_FILE`).
    pub pos_file: PathBuf,
    /// Caché de `/prefs` que rellenan otros hilos.
    pub prefs: Arc<Mutex<PrefsCache>>,
    pub prefs_source: PrefsSource,
    pub on_open: OpenAction,
}

/// Arrastre en curso (`win._dragging`).
#[derive(Clone, Copy, Debug)]
struct Drag {
    dx: f64,
    dy: f64,
    x0: f64,
    y0: f64,
    moved: bool,
}

/// Estado de un popup que leen sus señales (`_want`, `_dragging`, `_drag_ts`,
/// `_fix`, `_expanded`, `_detail`).
#[derive(Default)]
struct PopupState {
    want: Cell<Option<(i64, i64)>>,
    dragging: Cell<Option<Drag>>,
    drag_at: Cell<Option<Instant>>,
    fixes: Cell<u32>,
    expanded: Cell<bool>,
    detail: Cell<bool>,
}

struct Popup {
    id: u64,
    key: String,
    meta: PopupMeta,
    window: gtk::Window,
    state: Rc<PopupState>,
}

/// La pila (`popups`, `by_session`, `CLEAR_ALL`, `CSS_PROVIDER`, `ANCHOR`).
pub struct Stack {
    ctx: Context,
    popups: Vec<Popup>,
    next_id: u64,
    clear_all: Option<(gtk::Window, gtk::Button)>,
    css: Option<gtk::CssProvider>,
    anchor: Option<(i64, i64)>,
    /// El único escritor de `notifyd-pos.json`.
    anchor_writer: Option<AnchorWriter>,
    refreshing: Arc<AtomicBool>,
}

type Shared = Rc<RefCell<Stack>>;

thread_local! {
    static STACK: RefCell<Option<Shared>> = const { RefCell::new(None) };
}

/// Instala la pila en el hilo de GTK (una vez, tras `gtk::init`).
pub fn install(ctx: Context, anchor: Option<(i64, i64)>) {
    let anchor_writer = AnchorWriter::spawn(ctx.pos_file.clone());
    let shared = Rc::new(RefCell::new(Stack {
        anchor_writer,
        ctx,
        popups: Vec::new(),
        next_id: 0,
        clear_all: None,
        css: None,
        anchor,
        refreshing: Arc::new(AtomicBool::new(false)),
    }));
    apply_theme_css(&shared);
    STACK.with(|s| *s.borrow_mut() = Some(shared));
}

/// Ejecuta `f` con la pila del hilo de GTK, si está instalada.
pub fn with_stack(f: impl FnOnce(&Shared)) {
    let shared = STACK.with(|s| s.borrow().clone());
    if let Some(shared) = shared {
        f(&shared);
    }
}

/// ¿Hay algún popup «te espera»? (`any(w._kind == "waiting" …)` del barrido).
pub fn has_waiting() -> bool {
    let mut out = false;
    with_stack(|shared| {
        out = shared
            .try_borrow()
            .is_ok_and(|s| s.popups.iter().any(|p| p.meta.kind == "waiting"));
    });
    out
}

/// `_close_stale(state)`: cierra los «te espera» que `/state` ya no respalda.
pub fn close_stale(state: &serde_json::Value) {
    with_stack(|shared| {
        let ids: Vec<u64> = match shared.try_borrow() {
            Ok(stack) => {
                let metas: Vec<PopupMeta> = stack.popups.iter().map(|p| p.meta.clone()).collect();
                stale_waiting(&metas, state, now_secs())
                    .into_iter()
                    .filter_map(|i| stack.popups.get(i).map(|p| p.id))
                    .collect()
            }
            Err(_) => return,
        };
        for id in ids {
            close_popup(shared, id);
        }
    });
}

/// `native_notify` → `make_popup` (N2: sin `libnotify`, siempre popup propio;
/// `NATIVE_NOTIFY=1` da lo mismo que el Python cuando `HAVE_NOTIFY` es falso).
pub fn show(notice: Notice) {
    with_stack(|shared| make_popup(shared, &notice, false));
}

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

fn to_i32(v: i64) -> i32 {
    i32::try_from(v).unwrap_or(if v < 0 { i32::MIN } else { i32::MAX })
}

/// `apply_theme_css()`: el proveedor de la pantalla con el tema vigente.
fn apply_theme_css(shared: &Shared) {
    let Ok(mut stack) = shared.try_borrow_mut() else {
        return;
    };
    let tokens = stack
        .ctx
        .prefs
        .lock()
        .ok()
        .and_then(|c| c.tokens().cloned());
    if stack.css.is_none() {
        let provider = gtk::CssProvider::new();
        if let Some(screen) = gdk::Screen::default() {
            gtk::StyleContext::add_provider_for_screen(
                &screen,
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
        stack.css = Some(provider);
    }
    if let (Some(provider), Some(tokens)) = (&stack.css, tokens) {
        // Como el `except: pass` del Python: un CSS que no carga no rompe nada.
        let _ = provider.load_from_data(build_css(&tokens).as_bytes());
    }
}

/// Pide (sin bloquear) refrescar `/prefs` si `notif_pos` caducó; si cambia,
/// se recoloca la pila al volver.
fn request_prefs_refresh(shared: &Shared) {
    let Ok(stack) = shared.try_borrow() else {
        return;
    };
    let stale = stack
        .ctx
        .prefs
        .lock()
        .is_ok_and(|c| c.notif_pos_stale(Instant::now()));
    if !stale || stack.refreshing.swap(true, Ordering::SeqCst) {
        return;
    }
    let cache = Arc::clone(&stack.ctx.prefs);
    let source = stack.ctx.prefs_source.clone();
    let flag = Arc::clone(&stack.refreshing);
    let spawned = std::thread::Builder::new()
        .name("notifyd-prefs".into())
        .spawn(move || {
            let changed = source.refresh(&cache);
            flag.store(false, Ordering::SeqCst);
            if changed {
                glib::idle_add_once(|| with_stack(reposition));
            }
        });
    if spawned.is_err() {
        stack.refreshing.store(false, Ordering::SeqCst);
    }
}

/// `reposition()`: coloca la pila según `notif_pos` y el ancla.
pub fn reposition(shared: &Shared) {
    request_prefs_refresh(shared);
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let Some(monitor) = display.primary_monitor().or_else(|| display.monitor(0)) else {
        return;
    };
    let rect = monitor.geometry();
    let geo = Geometry {
        x: i64::from(rect.x()),
        y: i64::from(rect.y()),
        width: i64::from(rect.width()),
        height: i64::from(rect.height()),
    };
    let (windows, plan) = {
        let Ok(stack) = shared.try_borrow() else {
            return;
        };
        let mode = stack
            .ctx
            .prefs
            .lock()
            .ok()
            .and_then(|c| c.notif_pos().map(str::to_string));
        let heights: Vec<i64> = stack
            .popups
            .iter()
            .map(|p| i64::from(p.window.allocated_height()))
            .collect();
        let plan = layout(geo, mode.as_deref(), stack.anchor, &heights);
        let windows: Vec<(gtk::Window, Rc<PopupState>)> = stack
            .popups
            .iter()
            .map(|p| (p.window.clone(), Rc::clone(&p.state)))
            .collect();
        (windows, plan)
    };
    for ((window, state), &(x, y)) in windows.iter().zip(&plan.positions) {
        state.want.set(Some((x, y)));
        window.move_(to_i32(x), to_i32(y));
    }
    sync_clear_all(shared, plan.end, plan.up);
}

/// `_clear_all_window()` + `_sync_clear_all(x, y, up)`.
fn sync_clear_all(shared: &Shared, end: (i64, i64), up: bool) {
    let Ok(mut stack) = shared.try_borrow_mut() else {
        return;
    };
    let n = stack.popups.len();
    if n < CLEAR_ALL_MIN {
        if let Some((win, _)) = &stack.clear_all {
            win.hide();
        }
        return;
    }
    let lang = stack.ctx.lang;
    if stack.clear_all.is_none() {
        let win = notice_window();
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        row.set_size_request(WIDTH, -1);
        let button = gtk::Button::with_label("");
        button.style_context().add_class("clearall");
        button.set_relief(gtk::ReliefStyle::None);
        button.set_tooltip_text(Some(tr(Text::CloseAllTip, lang)));
        button.set_halign(gtk::Align::Center);
        button.connect_clicked(|_| with_stack(close_all));
        row.pack_start(&button, true, false, 0);
        win.add(&row);
        stack.clear_all = Some((win, button));
    }
    let Some((win, button)) = stack.clear_all.clone() else {
        return;
    };
    drop(stack);
    button.set_label(&close_all_label(n, lang));
    win.show_all();
    let (x, y) = clear_all_position(end, up, i64::from(win.allocated_height()));
    win.move_(to_i32(x), to_i32(y));
}

/// `close_all_popups()`: lo mismo que la ✕ de cada uno.
pub fn close_all(shared: &Shared) {
    let ids: Vec<u64> = match shared.try_borrow() {
        Ok(stack) => stack.popups.iter().map(|p| p.id).collect(),
        Err(_) => return,
    };
    for id in ids {
        close_popup(shared, id);
    }
}

/// `close_popup(win)`: fuera de la pila, ventana cerrada y pila recolocada.
pub fn close_popup(shared: &Shared, id: u64) {
    let removed = match shared.try_borrow_mut() {
        Ok(mut stack) => stack
            .popups
            .iter()
            .position(|p| p.id == id)
            .map(|i| stack.popups.remove(i)),
        Err(_) => return,
    };
    if let Some(popup) = removed {
        close_window(&popup.window);
        retry_close(popup.window.downgrade(), CLOSE_RETRIES);
    }
    reposition(shared);
}

/// Reintentos de un cierre que no llegó a destruir la ventana, y su pausa.
const CLOSE_RETRIES: u32 = 6;
const CLOSE_RETRY_MS: u64 = 500;

/// Oculta la ventana y pide su destrucción. Sin `unsafe` no hay
/// `gtk_widget_destroy`; `close()` encola un `GDK_DELETE` cuyo manejador por
/// omisión la destruye (no tiene manejador propio de `delete-event`). Solo
/// actúa sobre una ventana realizada, así que una que nunca llegó a
/// realizarse se realiza antes (oculta: no se ve).
fn close_window(window: &gtk::Window) {
    window.hide();
    if !window.is_realized() {
        window.realize();
    }
    window.close();
}

/// Comprueba que la ventana se destruyó: `gtk_widget_destroy` la desrealiza,
/// así que una ventana viva y realizada pasado el plazo es un cierre perdido.
/// Con un grupo propio por popup (`notice_window`) ningún grab de otra ventana
/// puede tragarse el `GDK_DELETE`; los reintentos cubren lo que quede y, si
/// se agotan, queda constancia en el journal.
fn retry_close(weak: glib::WeakRef<gtk::Window>, left: u32) {
    later(CLOSE_RETRY_MS, move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        if !window.is_realized() {
            return;
        }
        if left == 0 {
            eprintln!(
                "comandos-notifyd: aviso: una ventana de popup sigue viva tras {CLOSE_RETRIES} cierres"
            );
            return;
        }
        close_window(&window);
        retry_close(weak, left - 1);
    });
}

/// Ventana sin decoración, encima, sin foco al mapear y con fondo RGBA.
fn notice_window() -> gtk::Window {
    let win = gtk::Window::new(gtk::WindowType::Toplevel);
    // Grupo propio: GTK descarta el `GDK_DELETE` de `close()` si el grupo de
    // la ventana tiene un grab de otra ventana (`gtk_grab_add` de un menú o
    // un diálogo); con un grupo por popup eso no puede pasar y la ventana se
    // destruye siempre (I2 de la revisión final).
    gtk::WindowGroup::new().add_window(&win);
    win.style_context().add_class("ccpop");
    win.set_decorated(false);
    win.set_keep_above(true);
    win.set_focus_on_map(false); // no roba el foco al aparecer
    win.set_skip_taskbar_hint(true);
    win.set_skip_pager_hint(true);
    win.set_type_hint(gdk::WindowTypeHint::Notification);
    if let Some(visual) = WidgetExt::screen(&win).and_then(|s| s.rgba_visual()) {
        win.set_visual(Some(&visual));
    }
    win.set_app_paintable(true);
    win
}

/// `build_full_widget`: párrafos con ajuste y tablas sin ajuste con desplazamiento propio.
fn blocks_widget(blocks: &[Block]) -> gtk::Box {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 5);
    for block in blocks {
        let label = gtk::Label::new(None);
        label.set_xalign(0.0);
        label.set_yalign(0.0);
        label.style_context().add_class("body");
        label.set_selectable(true);
        label.set_can_focus(false);
        match block.label_text() {
            LabelText::Markup(markup) => label.set_markup(markup),
            LabelText::Plain(raw) => label.set_text(raw),
        }
        match block {
            Block::Text { .. } => {
                label.set_line_wrap(true);
                label.set_max_width_chars(54);
                column.pack_start(&label, false, false, 0);
            }
            Block::Table { .. } => {
                let scroll =
                    gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
                scroll.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Never);
                scroll.add(&label);
                column.pack_start(&scroll, false, false, 0);
            }
        }
    }
    column
}

/// `_icon_image(name, 15, color)`: el SVG tintado o la viñeta `•`.
fn kind_badge(icons_dir: Option<&std::path::Path>, model: &PopupModel) -> gtk::Widget {
    let pixbuf = icons_dir
        .and_then(|dir| icon_svg(dir, model))
        .and_then(|svg| {
            let loader = gdk_pixbuf::PixbufLoader::with_type("svg").ok()?;
            loader.set_size(ICON_SIZE, ICON_SIZE);
            loader.write(&svg).ok()?;
            loader.close().ok()?;
            loader.pixbuf()
        });
    match pixbuf {
        Some(pb) => gtk::Image::from_pixbuf(Some(&pb)).upcast(),
        None => gtk::Label::new(Some(ICON_FALLBACK)).upcast(),
    }
}

/// Programa `f` una vez dentro de `ms` milisegundos en el hilo de GTK.
fn later(ms: u64, f: impl FnOnce() + 'static) {
    glib::timeout_add_local_once(Duration::from_millis(ms), f);
}

fn later_reposition(ms: u64) {
    later(ms, || with_stack(reposition));
}

/// Abre la sesión del aviso y cierra el popup (`open_session` + `close_popup`).
fn open_and_close(id: u64, session: String, pane: String) {
    with_stack(|shared| {
        if let Ok(stack) = shared.try_borrow() {
            (stack.ctx.on_open)(&session, &pane);
        }
        close_popup(shared, id);
    });
}

fn close_by_id(id: u64) {
    with_stack(|shared| close_popup(shared, id));
}

/// Índice de un popup en la pila y las alturas de los de arriba.
fn heights_above(shared: &Shared, id: u64) -> Option<Vec<i64>> {
    let stack = shared.try_borrow().ok()?;
    let index = stack.popups.iter().position(|p| p.id == id)?;
    Some(
        stack
            .popups
            .iter()
            .take(index)
            .map(|p| i64::from(p.window.allocated_height()))
            .collect(),
    )
}

/// Nueva ancla: se recuerda en memoria y se escribe en `notifyd-pos.json` fuera del hilo de GTK.
fn store_anchor(shared: &Shared, anchor: (i64, i64)) {
    let Ok(mut stack) = shared.try_borrow_mut() else {
        return;
    };
    stack.anchor = Some(anchor);
    if let Some(writer) = &stack.anchor_writer {
        writer.store(anchor);
    }
}

/// `make_popup(title, body, session, kind, project, options, full, expanded, pane)`.
pub fn make_popup(shared: &Shared, notice: &Notice, expanded: bool) {
    apply_theme_css(shared);
    // Tope anti spam: entra el nuevo y sale el más viejo (nunca se pierde el
    // aviso). Acotado: aunque un cierre fallara, el bucle termina.
    for _ in 0..STACK_MAX {
        let victim = match shared.try_borrow() {
            Ok(stack) if stack.popups.len() >= STACK_MAX => {
                let metas: Vec<PopupMeta> = stack.popups.iter().map(|p| p.meta.clone()).collect();
                evict_candidate(&metas).and_then(|i| stack.popups.get(i).map(|p| p.id))
            }
            _ => None,
        };
        match victim {
            Some(id) => close_popup(shared, id),
            None => break,
        }
    }
    // Un popup por AGENTE (sesión+pane): dos agentes de la misma sesión no se pisan.
    let pane = if valid_pane(&notice.pane) {
        notice.pane.as_str()
    } else {
        ""
    };
    let key = popup_key(&notice.session, pane);
    let old = shared
        .try_borrow()
        .ok()
        .and_then(|s| s.popups.iter().find(|p| p.key == key).map(|p| p.id));
    if let Some(id) = old {
        close_popup(shared, id);
    }
    let (tokens, lang, icons_dir, id) = {
        let Ok(mut stack) = shared.try_borrow_mut() else {
            return;
        };
        let tokens = stack
            .ctx
            .prefs
            .lock()
            .ok()
            .and_then(|c| c.tokens().cloned());
        stack.next_id += 1;
        (
            tokens,
            stack.ctx.lang,
            stack.ctx.icons_dir.clone(),
            stack.next_id,
        )
    };
    // Donde el Python lanza en `theme_tokens()` el popup no llega a mostrarse.
    let Some(tokens) = tokens else {
        eprintln!("comandos-notifyd: tema del tablero ilegible; aviso sin popup");
        return;
    };
    let model = popup_model(notice, lang, &tokens);
    let state = Rc::new(PopupState::default());
    state.expanded.set(false);
    state.detail.set(expanded);

    let win = notice_window();
    win.set_default_size(WIDTH, -1);
    let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    card.style_context().add_class("card");

    // Franja de cabecera tintada por estado: la meta-info vive ARRIBA, separada del texto.
    let hd = gtk::Box::new(gtk::Orientation::Horizontal, 9);
    hd.style_context().add_class("hd");
    if model.waiting {
        hd.style_context().add_class("waiting");
    }
    let hdi = gtk::Box::new(gtk::Orientation::Horizontal, 9);
    hdi.set_margin_top(9);
    hdi.set_margin_bottom(3);
    hdi.set_margin_start(10);
    hdi.set_margin_end(30);
    let arrow = gtk::Label::new(Some(ARROW_CLOSED));
    arrow.style_context().add_class("close");
    let arrow_box = gtk::EventBox::new();
    arrow_box.add(&arrow);
    arrow_box.set_tooltip_text(Some(model.arrow_tip));
    {
        let arrow = arrow.clone();
        arrow_box.connect_enter_notify_event(move |_, _| {
            arrow.style_context().add_class("hover");
            glib::Propagation::Proceed
        });
    }
    {
        let arrow = arrow.clone();
        arrow_box.connect_leave_notify_event(move |_, _| {
            arrow.style_context().remove_class("hover");
            glib::Propagation::Proceed
        });
    }
    hdi.pack_start(&arrow_box, false, false, 0);
    let badge = kind_badge(icons_dir.as_deref(), &model);
    badge.style_context().add_class("kbadge");
    if model.waiting {
        badge.style_context().add_class("waiting");
    }
    badge.set_tooltip_text(Some(model.kind_tip));
    hdi.pack_start(&badge, false, false, 0);
    let project = gtk::Label::new(Some(&model.project));
    project.set_xalign(0.0);
    project.style_context().add_class("proj");
    project.set_ellipsize(pango::EllipsizeMode::End);
    let project_box = gtk::EventBox::new();
    project_box.add(&project);
    {
        let (session, pane) = (model.session.clone(), model.pane.clone());
        project_box.connect_button_press_event(move |_, _| {
            open_and_close(id, session.clone(), pane.clone());
            glib::Propagation::Stop
        });
    }
    project_box.set_tooltip_text(Some(model.project_tip));
    hdi.pack_start(&project_box, false, false, 0);
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0); // sin ventana X: el click pasa al plegado
    spacer.set_hexpand(true);
    hdi.pack_start(&spacer, true, true, 0);
    let hdv = gtk::Box::new(gtk::Orientation::Vertical, 1);
    hdv.pack_start(&hdi, false, false, 0);
    let inprev = gtk::Label::new(Some(&model.first_line));
    inprev.set_xalign(0.0);
    inprev.style_context().add_class("inprev");
    inprev.set_ellipsize(pango::EllipsizeMode::End);
    inprev.set_margin_start(48);
    inprev.set_margin_end(30);
    inprev.set_margin_bottom(10);
    hdv.pack_start(&inprev, false, false, 0);
    hd.pack_start(&hdv, true, true, 0);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    content.set_no_show_all(true);
    content.set_margin_top(4);
    content.set_margin_bottom(12);
    content.set_margin_start(14);
    content.set_margin_end(13);

    // Vista previa colapsada RENDERIZADA (markdown y tablas desde el primer vistazo).
    let preview = model.preview.as_deref().map(blocks_widget);
    if let Some(preview) = &preview {
        content.pack_start(preview, false, false, 0);
    }
    // Herramientas del texto: Copiar (todo al portapapeles) y «Ver TODO» si hay más.
    let tools = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    if let Some(text) = model.copy.clone() {
        let copy = gtk::Button::with_label(model.copy_label);
        copy.style_context().add_class("vermas");
        copy.set_relief(gtk::ReliefStyle::None);
        let (copy_label, copied_label) = (model.copy_label, model.copied_label);
        copy.connect_clicked(move |button| {
            gtk::Clipboard::get(&gdk::SELECTION_CLIPBOARD).set_text(&text);
            button.set_label(copied_label);
            let button = button.downgrade();
            glib::timeout_add_seconds_local_once(2, move || {
                if let Some(button) = button.upgrade() {
                    button.set_label(copy_label);
                }
            });
        });
        tools.pack_start(&copy, false, false, 0);
    }
    // «Ver TODO»: el texto COMPLETO aquí mismo, con scroll. Nunca responder a ciegas.
    let full_view = model.see_all.as_deref().map(|blocks| {
        let scroll = gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroll.set_min_content_height(140);
        scroll.set_max_content_height(520);
        scroll.set_propagate_natural_height(true);
        scroll.add(&blocks_widget(blocks));
        scroll.set_no_show_all(true);
        scroll
    });
    if let Some(scroll) = &full_view {
        let toggle_all = gtk::Button::with_label(model.see_all_label);
        toggle_all.style_context().add_class("vermas");
        toggle_all.set_relief(gtk::ReliefStyle::None);
        toggle_all.set_halign(gtk::Align::Start);
        let (scroll, preview, state, window) = (
            scroll.clone(),
            preview.clone(),
            Rc::clone(&state),
            win.downgrade(),
        );
        let (see_all, see_less) = (model.see_all_label, model.see_less_label);
        toggle_all.connect_clicked(move |button| {
            let expanded = !state.expanded.get();
            state.expanded.set(expanded);
            if expanded {
                scroll.set_no_show_all(false);
                scroll.show_all();
                if let Some(preview) = &preview {
                    preview.hide();
                }
                button.set_label(see_less);
            } else {
                scroll.hide();
                if let Some(preview) = &preview {
                    preview.show();
                }
                button.set_label(see_all);
                if let Some(window) = window.upgrade() {
                    window.resize(WIDTH, 1);
                }
            }
            with_stack(reposition);
            later_reposition(80);
        });
        tools.pack_start(&toggle_all, false, false, 0);
    }
    let open = gtk::Button::with_label(model.open_label);
    open.style_context().add_class("btn");
    open.style_context().add_class("primary");
    // Estilo clavado AL WIDGET (prioridad USER): sin foco, `:backdrop` desaturaba el botón.
    let pinned = gtk::CssProvider::new();
    if pinned
        .load_from_data(open_button_css(&tokens).as_bytes())
        .is_ok()
    {
        open.style_context()
            .add_provider(&pinned, gtk::STYLE_PROVIDER_PRIORITY_USER);
        if let Some(child) = open.child() {
            child
                .style_context()
                .add_provider(&pinned, gtk::STYLE_PROVIDER_PRIORITY_USER);
        }
    }
    {
        let (session, pane) = (model.session.clone(), model.pane.clone());
        open.connect_clicked(move |_| open_and_close(id, session.clone(), pane.clone()));
    }
    tools.pack_end(&open, false, false, 0);
    content.pack_start(&tools, false, false, 0);
    if let Some(scroll) = &full_view {
        content.pack_start(scroll, false, false, 0);
    }

    // Título = EventBox (ventana X propia: click fiable); detalle = Revealer.
    let title_box = gtk::EventBox::new();
    title_box.add(&hd);
    let revealer = gtk::Revealer::new();
    revealer.set_transition_type(gtk::RevealerTransitionType::SlideDown);
    revealer.set_transition_duration(150);
    revealer.add(&content);

    let set_open: Rc<dyn Fn(bool)> = {
        let (state, arrow, inprev, content, full_view, revealer, window) = (
            Rc::clone(&state),
            arrow.clone(),
            inprev.clone(),
            content.clone(),
            full_view.clone(),
            revealer.clone(),
            win.downgrade(),
        );
        Rc::new(move |open: bool| {
            state.detail.set(open);
            arrow.set_text(if open { ARROW_OPEN } else { ARROW_CLOSED });
            inprev.set_visible(!open);
            if open {
                content.set_no_show_all(false);
                content.show_all();
                if let Some(scroll) = &full_view
                    && !state.expanded.get()
                {
                    scroll.hide();
                }
            }
            revealer.set_reveal_child(open);
            for ms in [80, 240, 480] {
                let window = window.clone();
                later(ms, move || {
                    if let Some(window) = window.upgrade() {
                        window.resize(WIDTH, 1);
                    }
                    with_stack(reposition);
                });
            }
        })
    };
    let toggle: Rc<dyn Fn()> = {
        let (state, set_open) = (Rc::clone(&state), Rc::clone(&set_open));
        Rc::new(move || set_open(!state.detail.get()))
    };
    for target in [&title_box, &arrow_box] {
        let toggle = Rc::clone(&toggle);
        target.connect_button_release_event(move |_, _| {
            toggle();
            glib::Propagation::Stop
        });
    }
    card.pack_start(&title_box, false, false, 0);
    card.pack_start(&revealer, true, true, 0);
    if state.detail.get() {
        set_open(true);
    }

    // Arrastre manual (mutter ignora `begin_move_drag` en ventanas NOTIFICATION);
    // un click corto sin moverse pliega o despliega.
    win.add_events(
        gdk::EventMask::BUTTON_PRESS_MASK
            | gdk::EventMask::BUTTON1_MOTION_MASK
            | gdk::EventMask::BUTTON_RELEASE_MASK
            | gdk::EventMask::POINTER_MOTION_MASK,
    );
    {
        let state = Rc::clone(&state);
        win.connect_button_press_event(move |window, event| {
            if event.button() != 1 {
                return glib::Propagation::Proceed;
            }
            let (wx, wy) = window.position();
            let (rx, ry) = event.root();
            state.dragging.set(Some(Drag {
                dx: rx - f64::from(wx),
                dy: ry - f64::from(wy),
                x0: rx,
                y0: ry,
                moved: false,
            }));
            glib::Propagation::Stop
        });
    }
    {
        let state = Rc::clone(&state);
        win.connect_motion_notify_event(move |window, event| {
            let Some(mut drag) = state.dragging.get() else {
                return glib::Propagation::Proceed;
            };
            // ARRASTRAR = botón FÍSICAMENTE presionado (si otro widget se comió el
            // release, el arrastre quedaba colgado siguiendo al ratón).
            if !event.state().contains(gdk::ModifierType::BUTTON1_MASK) {
                state.dragging.set(None);
                return glib::Propagation::Proceed;
            }
            let (rx, ry) = event.root();
            if !drag.moved {
                // Umbral GENEROSO: un click con temblor de mano NO es un arrastre.
                if (rx - drag.x0).abs() < 12.0 && (ry - drag.y0).abs() < 12.0 {
                    return glib::Propagation::Proceed;
                }
                // Sin brinco: el arrastre sigue desde DONDE está la ventana ahora.
                let (wx, wy) = window.position();
                drag.dx = rx - f64::from(wx);
                drag.dy = ry - f64::from(wy);
            }
            drag.moved = true;
            state.dragging.set(Some(drag));
            state.drag_at.set(Some(Instant::now()));
            let (nx, ny) = ((rx - drag.dx) as i64, (ry - drag.dy) as i64);
            state.want.set(Some((nx, ny))); // que el re-asiento anti-WM no pelee el arrastre
            window.move_(to_i32(nx), to_i32(ny));
            glib::Propagation::Stop
        });
    }
    {
        let (state, toggle) = (Rc::clone(&state), Rc::clone(&toggle));
        win.connect_button_release_event(move |window, _| {
            let Some(drag) = state.dragging.take() else {
                return glib::Propagation::Proceed;
            };
            if !drag.moved {
                toggle();
                return glib::Propagation::Stop;
            }
            let (x, y) = window.position();
            with_stack(|shared| {
                // Nueva ancla = donde quedó ESTA ventana, menos su desplazamiento en la pila.
                let above = heights_above(shared, id).unwrap_or_default();
                store_anchor(shared, anchor_from((i64::from(x), i64::from(y)), &above));
                if let Ok(stack) = shared.try_borrow() {
                    if let Ok(mut cache) = stack.ctx.prefs.lock() {
                        cache.pin_free(Instant::now());
                    }
                    let dash = stack.ctx.prefs_source.dash.clone();
                    let _ = std::thread::Builder::new()
                        .name("notifyd-prefs-set".into())
                        .spawn(move || {
                            let free = serde_json::json!({"notif_pos": "free"});
                            let _ = dash.post_json("/prefs-set", &free, POST_TIMEOUT);
                        });
                }
            });
            later_reposition(60);
            glib::Propagation::Stop
        });
    }
    {
        let state = Rc::clone(&state);
        win.connect_configure_event(move |_, event| {
            let (ex, ey) = event.position();
            let event_pos = (i64::from(ex), i64::from(ey));
            let action = on_configure(
                state.want.get(),
                state.dragging.get().is_some(),
                state.drag_at.get().map(|t| t.elapsed().as_secs_f64()),
                event_pos,
                state.fixes.get(),
            );
            match action {
                Configure::Ignore => {}
                // Movimiento del WM (mapa inicial, restack): re-asentar nuestra posición.
                Configure::Reassert => {
                    state.fixes.set(state.fixes.get() + 1);
                    later_reposition(60);
                }
                // Arrastre del usuario: esa posición, menos su desplazamiento, es la nueva ancla.
                Configure::Anchor => with_stack(|shared| {
                    if let Some(above) = heights_above(shared, id) {
                        store_anchor(shared, anchor_from(event_pos, &above));
                        later_reposition(60);
                    }
                }),
            }
            false
        });
    }
    {
        let toggle = Rc::clone(&toggle);
        win.connect_key_press_event(move |_, event| {
            let name = event.keyval().name();
            match name.as_deref() {
                Some("Escape") => {
                    close_by_id(id);
                    glib::Propagation::Stop
                }
                Some("Return" | "KP_Enter" | "space") => {
                    toggle();
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
    }

    // ✕ FLOTANTE (Overlay): recibe el click antes que la tarjeta; cerrar siempre funciona.
    let close = gtk::Button::with_label(CLOSE_GLYPH);
    close.style_context().add_class("close");
    close.set_relief(gtk::ReliefStyle::None);
    close.set_halign(gtk::Align::End);
    close.set_valign(gtk::Align::Start);
    close.set_margin_top(5);
    close.set_margin_end(7);
    close.connect_clicked(move |_| close_by_id(id));
    close.set_tooltip_text(Some(model.close_tip));
    let overlay = gtk::Overlay::new();
    overlay.add(&card);
    overlay.add_overlay(&close);
    win.add(&overlay);
    win.set_opacity(0.0);
    win.show_all();
    if !state.detail.get() {
        content.hide(); // `no_show_all` lo protege; el Revealer arranca cerrado
    }

    // Aparición suave en 6 pasos de 25 ms.
    {
        let window = win.downgrade();
        let step = Cell::new(0u32);
        glib::timeout_add_local(Duration::from_millis(25), move || {
            step.set(step.get() + 1);
            if let Some(window) = window.upgrade() {
                window.set_opacity((f64::from(step.get()) / 6.0).min(0.98));
            }
            if step.get() < 6 {
                glib::ControlFlow::Continue
            } else {
                glib::ControlFlow::Break
            }
        });
    }
    // Altura natural, sin aire muerto.
    for ms in [40, 220, 450] {
        let (window, state) = (win.downgrade(), Rc::clone(&state));
        later(ms, move || {
            if let Some(window) = window.upgrade()
                && !state.expanded.get()
            {
                window.resize(WIDTH, 1);
            }
        });
    }
    if expanded && let Some(scroll) = &full_view {
        state.expanded.set(true);
        scroll.set_no_show_all(false);
        scroll.show_all();
        if let Some(preview) = &preview {
            preview.hide();
        }
    }
    if let Ok(mut stack) = shared.try_borrow_mut() {
        stack.popups.push(Popup {
            id,
            key: model.key.clone(),
            meta: PopupMeta {
                kind: model.kind.clone(),
                session: model.session.clone(),
                pane: model.pane.clone(),
                born: now_secs(),
            },
            window: win.clone(),
            state: Rc::clone(&state),
        });
    }
    reposition(shared);
    later_reposition(80);

    if model.auto_close {
        let state = Rc::clone(&state);
        glib::timeout_add_seconds_local(AUTO_CLOSE_SECS, move || {
            let mut alive = false;
            with_stack(|shared| {
                alive = shared
                    .try_borrow()
                    .is_ok_and(|s| s.popups.iter().any(|p| p.id == id));
            });
            // No cerrar mientras el usuario está LEYENDO el texto expandido.
            if alive && !state.expanded.get() {
                close_by_id(id);
                return glib::ControlFlow::Break;
            }
            if alive {
                glib::ControlFlow::Continue
            } else {
                glib::ControlFlow::Break
            }
        });
    }
}

/// Para las pruebas de pantalla (`tests/gtk.rs`): ventanas visibles de la pila
/// en orden, con su clave.
pub fn snapshot() -> Vec<(String, gtk::Window)> {
    let mut out = Vec::new();
    with_stack(|shared| {
        if let Ok(stack) = shared.try_borrow() {
            out = stack
                .popups
                .iter()
                .map(|p| (p.key.clone(), p.window.clone()))
                .collect();
        }
    });
    out
}

/// Para las pruebas de pantalla: la ventana de la pastilla «Cerrar todas» si
/// ya se creó (oculta o no: se oculta, nunca se destruye).
pub fn clear_all_window() -> Option<gtk::Window> {
    let mut out = None;
    with_stack(|shared| {
        if let Ok(stack) = shared.try_borrow() {
            out = stack.clear_all.as_ref().map(|(win, _)| win.clone());
        }
    });
    out
}

/// Para las pruebas de pantalla: la pastilla «Cerrar todas», si está visible.
pub fn clear_all_button() -> Option<gtk::Button> {
    let mut out = None;
    with_stack(|shared| {
        if let Ok(stack) = shared.try_borrow() {
            out = stack
                .clear_all
                .as_ref()
                .filter(|(win, _)| win.is_visible())
                .map(|(_, button)| button.clone());
        }
    });
    out
}

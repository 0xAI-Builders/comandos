//! Popups reales de GTK3. Solo con `COMANDOS_GTK_TESTS=1` y una pantalla ya
//! existente (`DISPLAY` o `WAYLAND_DISPLAY`); si no, se omite con aviso. Nunca
//! se arranca Xvfb. Las ventanas aparecen en el escritorio de quien la corre y
//! se cierran todas al final. El tablero es un servidor HTTP de la prueba y la
//! posición se guarda en un `--hooks-dir` temporal. Sin tmux ni sonido.
//!
//! GTK exige un solo hilo: todo va en una única función de prueba que recorre
//! los escenarios en orden.
mod support;

use comandos_notifyd::dash::{DashClient, PrefsCache, PrefsSource};
use comandos_notifyd::notice::{Lang, Notice};
use comandos_notifyd::popup::{self, Context};
use comandos_notifyd::position::{Geometry, layout};
use gtk::prelude::*;
use std::cell::RefCell;
use std::io::{Read, Write};
use std::path::Path;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn display_enabled() -> bool {
    let wanted = std::env::var("COMANDOS_GTK_TESTS").is_ok_and(|v| v == "1");
    let display = std::env::var_os("DISPLAY").is_some_and(|v| !v.is_empty())
        || std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty());
    wanted && display
}

/// `/prefs` de la prueba: tema `dia` y la pila arriba a la derecha.
fn fake_dash() -> u16 {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            let _ = stream.write_all(
                b"HTTP/1.0 200 OK\r\nContent-Type: application/json\r\n\r\n{\"theme\": \"dia\", \"notif_pos\": \"tr\"}",
            );
        }
    });
    port
}

fn pump(for_ms: u64) {
    let until = Instant::now() + Duration::from_millis(for_ms);
    while Instant::now() < until {
        while gtk::events_pending() {
            gtk::main_iteration();
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn notice(kind: &str, session: &str, pane: &str, full: &str) -> Notice {
    Notice {
        title: "Claude Code".into(),
        body: String::new(),
        session: session.into(),
        kind: kind.into(),
        project: format!("proyecto {session}"),
        options: "Sí\u{1f}No\u{1f}Siempre".into(),
        full: full.into(),
        pane: pane.into(),
    }
}

/// Etiquetas de todos los botones bajo `widget`, en orden de árbol.
fn button_labels(widget: &gtk::Widget, out: &mut Vec<String>) {
    if let Some(button) = widget.downcast_ref::<gtk::Button>() {
        out.push(button.label().map(|l| l.to_string()).unwrap_or_default());
    }
    if let Some(container) = widget.downcast_ref::<gtk::Container>() {
        for child in container.children() {
            button_labels(&child, out);
        }
    }
}

fn labels_of(window: &gtk::Window) -> Vec<String> {
    let mut out = Vec::new();
    button_labels(window.upcast_ref(), &mut out);
    out
}

fn install(hooks: &Path, opened: Rc<RefCell<Vec<(String, String)>>>) {
    let port = fake_dash();
    let dash = DashClient::parse(&format!("http://127.0.0.1:{port}")).unwrap();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cache = Arc::new(Mutex::new(PrefsCache::default()));
    let source = PrefsSource {
        dash,
        themes_file: Some(repo.join("config/themes.json")),
    };
    source.refresh(&cache);
    popup::install(
        Context {
            lang: Lang::Es,
            icons_dir: Some(repo.join("dash/icons")),
            pos_file: hooks.join("notifyd-pos.json"),
            prefs: cache,
            prefs_source: source,
            on_open: Box::new(move |s: &str, p: &str| {
                opened.borrow_mut().push((s.to_string(), p.to_string()))
            }),
        },
        None,
    );
}

/// Ventanas de popup VIVAS para GTK (`list_toplevels`, que incluye las
/// ocultas sin destruir), sin contar la pastilla «Cerrar todas», que se oculta
/// y no se destruye. Solo las de clase `ccpop`: un tooltip que aparezca si el
/// ratón pasa por encima no cuenta.
fn live_popup_windows() -> usize {
    let pill = popup::clear_all_window();
    gtk::Window::list_toplevels()
        .into_iter()
        .filter(|w| w.style_context().has_class("ccpop"))
        .filter(|w| {
            pill.as_ref()
                .is_none_or(|p| p.upcast_ref::<gtk::Widget>() != w)
        })
        .count()
}

/// La pila lógica vacía y, además, GTK sin ninguna ventana de popup viva
/// (I2: una ventana oculta y sin destruir es una fuga de días).
fn close_everything() {
    popup::with_stack(popup::close_all);
    pump(300);
    assert!(popup::snapshot().is_empty(), "quedaron popups abiertos");
    pump(800);
    assert_eq!(live_popup_windows(), 0, "ventanas de popup sin destruir");
}

#[test]
fn gtk_popups() {
    if !display_enabled() {
        eprintln!("gtk_popups: hace falta COMANDOS_GTK_TESTS=1 y una pantalla; se omite");
        return;
    }
    gtk::init().unwrap();
    let hooks = support::tempdir();
    let opened = Rc::new(RefCell::new(Vec::new()));
    install(hooks.path(), Rc::clone(&opened));

    popup_layout_and_buttons();
    close_everything();
    stack_evicts_oldest_calm();
    close_everything();
    clear_all_appears_from_two();
    close_everything();
    many_cycles_leave_no_windows();
    close_everything();
    close_survives_a_foreign_grab();
    close_everything();
    assert!(opened.borrow().is_empty(), "ninguna prueba pulsa «Abrir»");
}

/// Muchos ciclos de abrir y cerrar por las tres vías (✕/«Cerrar todas»,
/// reemplazo por la misma clave y desalojo con la pila llena): al final GTK
/// no conserva ninguna ventana de popup.
fn many_cycles_leave_no_windows() {
    for round in 0..5 {
        for i in 0..12 {
            popup::show(notice("waiting", &format!("c{round}-{i}"), "", "ciclo"));
            pump(20);
        }
        // Reemplazo: la misma sesión vuelve a avisar.
        popup::show(notice("done", &format!("c{round}-11"), "", "otra vez"));
        pump(100);
        assert!(popup::snapshot().len() <= 8);
        popup::with_stack(popup::close_all);
        pump(200);
    }
    pump(800);
    assert!(popup::snapshot().is_empty());
    assert_eq!(live_popup_windows(), 0, "fuga tras los ciclos");
}

/// Con un grab de OTRA ventana activo (`gtk_grab_add`, como un menú o un
/// diálogo modal), el cierre destruye la ventana igual y antes del primer
/// reintento (500 ms): cada popup vive en su propio `WindowGroup`.
fn close_survives_a_foreign_grab() {
    let grabber = gtk::Window::new(gtk::WindowType::Toplevel);
    grabber.set_default_size(10, 10);
    grabber.show();
    pump(200);
    grabber.grab_add();
    popup::show(notice("waiting", "grab", "", "con grab ajeno"));
    pump(300);
    assert_eq!(live_popup_windows(), 1);
    popup::with_stack(popup::close_all);
    pump(300);
    assert_eq!(live_popup_windows(), 0, "el grab ajeno se tragó el cierre");
    grabber.grab_remove();
    grabber.close();
    pump(300);
}

/// Aviso de permiso con tres opciones: el popup es un AVISO PURO como el del
/// Python (sin botones numerados); «Copiar», «… Ver TODO» si hay más texto,
/// «Abrir» y la ✕. Un «listo» corto: «Copiar», «Abrir» y la ✕.
fn popup_layout_and_buttons() {
    let long: String = (0..30).map(|i| format!("línea {i}\n")).collect();
    popup::show(notice(
        "waiting",
        "perm",
        "%1",
        &format!("¿Permiso para editar?\n{long}"),
    ));
    popup::show(notice("done", "listo", "", "Terminé."));
    pump(600);
    let stack = popup::snapshot();
    assert_eq!(
        stack.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
        ["perm|%1", "listo"]
    );
    let (_, waiting) = &stack[0];
    let (_, done) = &stack[1];
    assert_eq!(labels_of(waiting), ["Copiar", "… Ver TODO", "Abrir", "×"]);
    assert_eq!(labels_of(done), ["Copiar", "Abrir", "×"]);
    for (_, win) in &stack {
        assert!(win.style_context().has_class("ccpop"));
        assert!(!win.is_decorated());
        assert_eq!(win.type_hint(), gtk::gdk::WindowTypeHint::Notification);
        assert!(win.allocated_width() >= 320);
        let labels = labels_of(win);
        for digit in ["1", "2", "3"] {
            assert!(!labels.iter().any(|l| l == digit), "sin botones numerados");
        }
    }
    // Pila arriba a la derecha (`notif_pos` = `tr` del tablero de la prueba).
    let display = gtk::gdk::Display::default().unwrap();
    let monitor = display
        .primary_monitor()
        .or_else(|| display.monitor(0))
        .unwrap();
    let rect = monitor.geometry();
    let heights: Vec<i64> = stack
        .iter()
        .map(|(_, w)| i64::from(w.allocated_height()))
        .collect();
    let plan = layout(
        Geometry {
            x: i64::from(rect.x()),
            y: i64::from(rect.y()),
            width: i64::from(rect.width()),
            height: i64::from(rect.height()),
        },
        Some("tr"),
        None,
        &heights,
    );
    for ((_, win), &(x, y)) in stack.iter().zip(&plan.positions) {
        let (wx, wy) = win.position();
        assert!((i64::from(wx) - x).abs() <= 2 && (i64::from(wy) - y).abs() <= 2);
    }
}

/// Con 8 en la pila entra el nuevo y sale el «listo» más viejo, no un «te espera».
fn stack_evicts_oldest_calm() {
    popup::show(notice("waiting", "w0", "", "espera 0"));
    pump(50);
    popup::show(notice("done", "d1", "", "listo 1"));
    pump(50);
    for i in 2..8 {
        popup::show(notice("waiting", &format!("w{i}"), "", "espera"));
        pump(50);
    }
    pump(300);
    assert_eq!(popup::snapshot().len(), 8);
    popup::show(notice("waiting", "nuevo", "", "entra"));
    pump(300);
    let keys: Vec<String> = popup::snapshot().into_iter().map(|(k, _)| k).collect();
    assert_eq!(keys.len(), 8);
    assert!(
        !keys.contains(&"d1".to_string()),
        "salió el «listo» más viejo"
    );
    assert!(keys.contains(&"w0".to_string()));
    assert_eq!(keys.last().map(String::as_str), Some("nuevo"));
}

/// «Cerrar todas · n» aparece desde 2 avisos y cierra la pila entera.
fn clear_all_appears_from_two() {
    popup::show(notice("waiting", "uno", "", "a"));
    pump(300);
    assert!(popup::clear_all_button().is_none());
    popup::show(notice("waiting", "dos", "", "b"));
    pump(300);
    let button = popup::clear_all_button().expect("pastilla visible con 2");
    assert_eq!(button.label().unwrap(), "Cerrar todas · 2");
    button.clicked();
    pump(300);
    assert!(popup::snapshot().is_empty());
    assert!(popup::clear_all_button().is_none());
}

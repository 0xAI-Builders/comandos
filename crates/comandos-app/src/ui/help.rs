//! Filas de ayuda del original; el panel comparte el overlay de la ventana.
use gtk::prelude::*;
pub type Section = (&'static str, &'static [(&'static str, &'static str)]);
const ES: &[Section] = &[
    (
        "Ventana ComandOS",
        &[
            ("F1", "Esta ayuda"),
            (
                "Ctrl+K",
                "Saltar a cualquier pestana/sesion (escribe y Enter)",
            ),
            (
                "Ctrl+Tab",
                "Alternar entre las DOS ultimas pestanas (ping-pong)",
            ),
            ("Ctrl+Shift+Tab", "Pestana anterior (ciclo)"),
            ("Ctrl+T", "Nueva terminal en el directorio actual"),
            (
                "Ctrl+Shift+T",
                "Terminal experimental (xterm.js, con ligaduras)",
            ),
            ("Ctrl+Shift+W", "Cerrar la pestana actual"),
            ("Ctrl+PgUp / PgDn", "Cambiar de pestana"),
            ("F12", "Mostrar / ocultar terminales"),
            ("F5", "Recargar el tablero"),
            ("Ctrl+Shift + / - / 0", "Zoom de letra de la terminal"),
            ("Ctrl+Q", "Salir"),
        ],
    ),
    (
        "Terminal (portapapeles)",
        &[
            ("Ctrl+V", "Pegar del sistema"),
            ("Ctrl+C", "Copiar (si hay seleccion) / interrumpir"),
            (
                "Seleccionar con mouse",
                "Copia al portapapeles automaticamente",
            ),
            ("Click en URL", "Abrir el enlace en el navegador"),
            (
                "Click en ruta (~/... o /...)",
                "Abrir el archivo/carpeta con su app default",
            ),
            (
                "Click derecho",
                "Copiar, pegar, splits, renombrar pestana, cerrar",
            ),
        ],
    ),
    (
        "Splits y ventanas (tmux)",
        &[
            (
                "Click derecho -> Split",
                "Derecha / izquierda / abajo / arriba",
            ),
            ("Alt+flechas", "Moverse entre splits"),
            ("Ctrl-b l", "Alternar ventana claude / shell"),
            ("Ctrl-b z", "Zoom del split (pantalla completa)"),
            ("Ctrl-b [", "Modo scroll (q para salir)"),
            ("Ctrl-b ]", "Pegar del portapapeles del sistema"),
        ],
    ),
    (
        "Global (todo el escritorio)",
        &[
            ("Super+N", "Saltar a la sesion mas urgente"),
            ("Super+T", "Traer la terminal al frente"),
            ("Super+C", "Traer / abrir ComandOS"),
        ],
    ),
];
const EN: &[Section] = &[
    (
        "ComandOS window",
        &[
            ("F1", "This help"),
            ("Ctrl+K", "Jump to any tab/session (type and Enter)"),
            ("Ctrl+Tab", "Toggle between the last TWO tabs (ping-pong)"),
            ("Ctrl+Shift+Tab", "Previous tab (cycle)"),
            ("Ctrl+T", "New terminal in the current directory"),
            (
                "Ctrl+Shift+T",
                "Experimental terminal (xterm.js, with ligatures)",
            ),
            ("Ctrl+Shift+W", "Close the current tab"),
            ("Ctrl+PgUp / PgDn", "Switch tabs"),
            ("F12", "Show / hide terminals"),
            ("F5", "Reload the dashboard"),
            ("Ctrl+Shift + / - / 0", "Terminal font zoom"),
            ("Ctrl+Q", "Quit"),
        ],
    ),
    (
        "Terminal (clipboard)",
        &[
            ("Ctrl+V", "Paste from system clipboard"),
            ("Ctrl+C", "Copy (if selected) / interrupt"),
            ("Mouse selection", "Copies to clipboard automatically"),
            ("Click a URL", "Open the link in your browser"),
            (
                "Click a path (~/... or /...)",
                "Open the file/folder with its default app",
            ),
            ("Right click", "Copy, paste, splits, rename tab, close"),
        ],
    ),
    (
        "Splits and windows (tmux)",
        &[
            ("Right click -> Split", "Right / left / down / up"),
            ("Alt+arrows", "Move between splits"),
            ("Ctrl-b l", "Toggle claude / shell window"),
            ("Ctrl-b z", "Zoom split (full screen)"),
            ("Ctrl-b [", "Scroll mode (q to exit)"),
            ("Ctrl-b ]", "Paste from system clipboard"),
        ],
    ),
    (
        "Global (whole desktop)",
        &[
            ("Super+N", "Jump to the most urgent session"),
            ("Super+T", "Bring the terminal to front"),
            ("Super+C", "Bring / open ComandOS"),
        ],
    ),
];
pub fn shortcuts(english: bool) -> &'static [Section] {
    if english { EN } else { ES }
}
pub fn panel(english: bool) -> gtk::Frame {
    let frame = gtk::Frame::new(None);
    frame.set_size_request(640, 720);
    frame.set_halign(gtk::Align::Center);
    frame.set_valign(gtk::Align::Center);
    frame.style_context().add_class("helpwin");
    let scroll = gtk::ScrolledWindow::new(gtk::Adjustment::NONE, gtk::Adjustment::NONE);
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 4);
    outer.set_margin_start(22);
    outer.set_margin_end(22);
    outer.set_margin_top(22);
    outer.set_margin_bottom(22);
    let title = gtk::Label::new(None);
    title.set_xalign(0.);
    title.set_markup(&format!(
        "<span size='16000' weight='bold'>{}</span>",
        if english {
            "Keyboard shortcuts"
        } else {
            "Atajos de teclado"
        }
    ));
    title.style_context().add_class("help-h");
    outer.pack_start(&title, false, false, 0);
    for (section, rows) in shortcuts(english) {
        let head = gtk::Label::new(None);
        head.set_xalign(0.);
        head.set_markup(&format!(
            "<span weight='bold'>{}</span>",
            glib::markup_escape_text(&section.to_uppercase())
        ));
        head.style_context().add_class("help-sec");
        head.set_margin_top(16);
        outer.pack_start(&head, false, false, 0);
        let grid = gtk::Grid::new();
        grid.set_column_spacing(18);
        grid.set_row_spacing(7);
        grid.set_margin_top(6);
        for (i, (key, description)) in rows.iter().enumerate() {
            let k = gtk::Label::new(Some(key));
            k.set_xalign(0.);
            k.style_context().add_class("help-key");
            let d = gtk::Label::new(Some(description));
            d.set_xalign(0.);
            d.style_context().add_class("help-desc");
            grid.attach(&k, 0, i as i32, 1, 1);
            grid.attach(&d, 1, i as i32, 1, 1);
        }
        outer.pack_start(&grid, false, false, 0);
    }
    let hint = gtk::Label::new(None);
    hint.set_xalign(0.);
    hint.set_markup(&format!("<span size='9000'>{}</span>",glib::markup_escape_text(if english{"Ctrl-b is the tmux 'prefix': release it, then type the letter.  Esc or ? closes this window."}else{"Ctrl-b es el 'prefix' de tmux: sueltas y luego tecleas la letra.  Esc o ? cierra esta ventana."})));
    hint.style_context().add_class("help-hint");
    hint.set_margin_top(20);
    outer.pack_start(&hint, false, false, 0);
    scroll.add(&outer);
    frame.add(&scroll);
    frame
}

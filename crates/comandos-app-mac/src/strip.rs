//! Native menu data; AppKit materializes this policy on main.
#![forbid(unsafe_code)]
use comandos_desktop::Lang;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(isize)]
pub enum MenuAction {
    Quit = 1,
    Reload,
    Toggle,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    NewLocal,
    Close,
    Next,
    Previous,
}
impl MenuAction {
    pub fn from_tag(tag: isize) -> Option<Self> {
        Some(match tag {
            1 => Self::Quit,
            2 => Self::Reload,
            3 => Self::Toggle,
            4 => Self::ZoomIn,
            5 => Self::ZoomOut,
            6 => Self::ZoomReset,
            7 => Self::NewLocal,
            8 => Self::Close,
            9 => Self::Next,
            10 => Self::Previous,
            _ => return None,
        })
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    pub title: &'static str,
    pub action: MenuAction,
    pub key: &'static str,
    pub command: bool,
    pub shift: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuGroup {
    pub title: &'static str,
    pub items: Vec<MenuItem>,
}
pub fn menu_spec(lang: Lang) -> Vec<MenuGroup> {
    let es = lang == Lang::Es;
    let item = |es_title, en_title, action, key, shift| MenuItem {
        title: if es { es_title } else { en_title },
        action,
        key,
        command: action != MenuAction::Toggle,
        shift,
    };
    vec![
        MenuGroup {
            title: "",
            items: vec![item("Salir", "Quit ComandOS", MenuAction::Quit, "q", false)],
        },
        MenuGroup {
            title: if es { "Vista" } else { "View" },
            items: vec![
                item(
                    "Recargar tablero",
                    "Reload dashboard",
                    MenuAction::Reload,
                    "r",
                    false,
                ),
                item(
                    "Mostrar/ocultar terminales",
                    "Toggle terminal pane",
                    MenuAction::Toggle,
                    "\u{f70f}",
                    false,
                ),
                item("Zoom +", "Zoom +", MenuAction::ZoomIn, "=", false),
                item("Zoom -", "Zoom -", MenuAction::ZoomOut, "-", false),
                item("Zoom 100%", "Zoom 100%", MenuAction::ZoomReset, "0", false),
            ],
        },
        MenuGroup {
            title: if es { "Pestanas" } else { "Tabs" },
            items: vec![
                item(
                    "Nueva terminal local",
                    "New local tab",
                    MenuAction::NewLocal,
                    "t",
                    false,
                ),
                item("Cerrar pestana", "Close tab", MenuAction::Close, "w", false),
                item("Siguiente pestana", "Next tab", MenuAction::Next, "]", true),
                item(
                    "Pestana anterior",
                    "Previous tab",
                    MenuAction::Previous,
                    "[",
                    true,
                ),
            ],
        },
    ]
}
pub fn zoom(current: f64, factor: f64) -> f64 {
    if factor == 0.0 {
        return 1.0;
    }
    (if current == 0.0 { 1.0 } else { current } * factor).clamp(0.5, 3.0)
}

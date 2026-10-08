//! Orden de decisión de on_key: los atajos de app preceden al codificador PTY.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyInput {
    pub key: u32,
    pub control: bool,
    pub shift: bool,
    pub terminal: bool,
    pub has_term: bool,
    pub selection: bool,
    pub drag: bool,
}
impl KeyInput {
    pub fn new(key: u32) -> Self {
        Self {
            key,
            control: false,
            shift: false,
            terminal: false,
            has_term: false,
            selection: false,
            drag: false,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    Pass,
    CancelDrag,
    Help,
    Paste,
    CtrlC,
    Copy,
    ToggleTerminals,
    Reload,
    StartAI,
    MruOrNext,
    Cycle(i32),
    Mosaic,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    Switcher,
    Snippets,
    NewTerminal,
    NewXterm,
    CloseTab,
    Quit,
}
pub fn action(e: KeyInput) -> KeyAction {
    use gdk::keys::constants as k;
    let is = |keys: &[gdk::keys::Key]| keys.iter().any(|key| **key == e.key);
    if is(&[k::Escape]) && e.drag {
        return KeyAction::CancelDrag;
    }
    if is(&[k::F1]) {
        return KeyAction::Help;
    }
    if e.control && e.terminal {
        if is(&[k::v, k::V]) {
            return KeyAction::Paste;
        }
        if is(&[k::c, k::C]) && !e.shift {
            return KeyAction::CtrlC;
        }
        if is(&[k::c, k::C]) && e.selection {
            return KeyAction::Copy;
        }
    }
    if is(&[k::F12]) {
        return KeyAction::ToggleTerminals;
    }
    if is(&[k::F5]) {
        return KeyAction::Reload;
    }
    if e.control && e.shift && is(&[k::a, k::A]) {
        return KeyAction::StartAI;
    }
    if e.control && is(&[k::Tab, k::ISO_Left_Tab]) {
        return if e.shift {
            KeyAction::Cycle(-1)
        } else {
            KeyAction::MruOrNext
        };
    }
    if e.control && is(&[k::g, k::G]) {
        return KeyAction::Mosaic;
    }
    if e.control && is(&[k::Page_Down, k::Page_Up]) {
        return KeyAction::Cycle(if is(&[k::Page_Down]) { 1 } else { -1 });
    }
    if e.has_term && e.control && e.shift {
        if is(&[k::plus, k::equal, k::KP_Add]) {
            return KeyAction::ZoomIn;
        }
        if is(&[k::minus, k::underscore, k::KP_Subtract]) {
            return KeyAction::ZoomOut;
        }
        if is(&[k::_0, k::parenright]) {
            return KeyAction::ZoomReset;
        }
    }
    if e.control && is(&[k::k, k::K]) {
        return if e.shift {
            KeyAction::Snippets
        } else {
            KeyAction::Switcher
        };
    }
    if e.control && is(&[k::t, k::T]) {
        return if e.shift {
            KeyAction::NewXterm
        } else {
            KeyAction::NewTerminal
        };
    }
    if e.control && e.shift && is(&[k::w, k::W]) {
        return KeyAction::CloseTab;
    }
    if e.control && !e.shift && is(&[k::q, k::Q]) {
        return KeyAction::Quit;
    }
    KeyAction::Pass
}

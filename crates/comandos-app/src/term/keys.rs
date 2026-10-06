//! Translate GDK keyvals to the engine's existing keyboard encoder.
use super::engine::{KeyAction, KeyInput, Modes, encode_key};
use gdk::keys::constants::{
    _0 as Digit0, _1 as Digit1, _2 as Digit2, _3 as Digit3, _4 as Digit4, _5 as Digit5,
    _6 as Digit6, _7 as Digit7, _8 as Digit8, _9 as Digit9,
};
use gdk::{
    ModifierType,
    keys::{Key, constants::*},
};
#[allow(non_upper_case_globals)] // GDK names are external API constants.
pub fn key_action(key: Key, mods: ModifierType, modes: &Modes) -> KeyAction {
    let (name, code) = match key {
        BackSpace => ("Backspace", 8),
        Tab | ISO_Left_Tab => ("Tab", 9),
        Return | KP_Enter => ("Enter", 13),
        Escape => ("Escape", 27),
        Left | KP_Left => ("ArrowLeft", 37),
        Up | KP_Up => ("ArrowUp", 38),
        Right | KP_Right => ("ArrowRight", 39),
        Down | KP_Down => ("ArrowDown", 40),
        Home | KP_Home => ("Home", 36),
        End | KP_End => ("End", 35),
        Page_Up | KP_Page_Up => ("PageUp", 33),
        Page_Down | KP_Page_Down => ("PageDown", 34),
        Insert | KP_Insert => ("Insert", 45),
        Delete | KP_Delete => ("Delete", 46),
        F1 => ("F1", 112),
        F2 => ("F2", 113),
        F3 => ("F3", 114),
        F4 => ("F4", 115),
        F5 => ("F5", 116),
        F6 => ("F6", 117),
        F7 => ("F7", 118),
        F8 => ("F8", 119),
        F9 => ("F9", 120),
        F10 => ("F10", 121),
        F11 => ("F11", 122),
        F12 => ("F12", 123),
        space => ("", 32),
        Digit0 | parenright => ("", 48),
        Digit1 | exclam => ("", 49),
        Digit2 | at => ("", 50),
        Digit3 | numbersign => ("", 51),
        Digit4 | dollar => ("", 52),
        Digit5 | percent => ("", 53),
        Digit6 | asciicircum => ("", 54),
        Digit7 | ampersand => ("", 55),
        Digit8 | asterisk => ("", 56),
        Digit9 | parenleft => ("", 57),
        semicolon | colon => ("", 186),
        equal | plus => ("", 187),
        comma | less => ("", 188),
        minus | underscore => ("", 189),
        period | greater => ("", 190),
        slash | question => ("", 191),
        grave | asciitilde => ("", 192),
        bracketleft | braceleft => ("", 219),
        backslash | bar => ("", 220),
        bracketright | braceright => ("", 221),
        apostrophe | quotedbl => ("", 222),
        KP_0 => ("", 96),
        KP_1 => ("", 97),
        KP_2 => ("", 98),
        KP_3 => ("", 99),
        KP_4 => ("", 100),
        KP_5 => ("", 101),
        KP_6 => ("", 102),
        KP_7 => ("", 103),
        KP_8 => ("", 104),
        KP_9 => ("", 105),
        KP_Multiply => ("", 106),
        KP_Add => ("", 107),
        KP_Separator => ("", 108),
        KP_Subtract => ("", 109),
        KP_Decimal => ("", 110),
        KP_Divide => ("", 111),
        _ => ("", 0),
    };
    let printable = key
        .to_unicode()
        .map(|character| character.to_string())
        .unwrap_or_default();
    let key_code = if code == 0 {
        key.to_unicode()
            .filter(|character| character.is_ascii_alphabetic())
            .map_or(0, |character| u32::from(character.to_ascii_uppercase()))
    } else {
        code
    };
    encode_key(
        &KeyInput {
            key: if name.is_empty() { &printable } else { name },
            code: "",
            key_code,
            ctrl: mods.contains(ModifierType::CONTROL_MASK),
            alt: mods.contains(ModifierType::MOD1_MASK),
            shift: mods.contains(ModifierType::SHIFT_MASK) || key == ISO_Left_Tab,
            meta: mods.contains(ModifierType::SUPER_MASK),
        },
        modes,
    )
}

pub fn mouse_button(number: u32) -> super::engine::Button {
    match number {
        1 => super::engine::Button::Left,
        2 => super::engine::Button::Middle,
        3 => super::engine::Button::Right,
        _ => super::engine::Button::None,
    }
}
pub fn drag_button(state: ModifierType) -> super::engine::Button {
    if state.contains(ModifierType::BUTTON1_MASK) {
        super::engine::Button::Left
    } else if state.contains(ModifierType::BUTTON2_MASK) {
        super::engine::Button::Middle
    } else if state.contains(ModifierType::BUTTON3_MASK) {
        super::engine::Button::Right
    } else {
        super::engine::Button::None
    }
}
